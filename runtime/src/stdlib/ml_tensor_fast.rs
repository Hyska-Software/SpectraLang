use super::*;
/// Fast-path helper for `ml.linear(input, weight, bias)`.
///
/// Mirrors `std_ml_linear` but skips the generic host-call dispatch
/// (no `ml_args` parsing, no `ctx_ref` writing, no catch_unwind).
/// Returns the new tensor handle (>0) on success or 0 on error.
pub fn ml_linear_fast(input_h: usize, weight_h: usize, bias_h: usize) -> SpectraHostValue {
    let Some((shape, out, requires_grad, creator)) = with_tensor_registry(|registry| {
        let input = registry.get(input_h)?;
        let weight = registry.get(weight_h)?;
        let bias = registry.get(bias_h)?;
        if input.dtype != TensorDType::Float
            || weight.dtype != TensorDType::Float
            || bias.dtype != TensorDType::Float
            || input.shape.len() != 2
            || weight.shape.len() != 2
            || bias.shape.len() != 1
        {
            return None;
        }
        let (batch, in_features) = (input.shape[0], input.shape[1]);
        let (w_in, out_features) = (weight.shape[0], weight.shape[1]);
        if in_features != w_in || bias.shape[0] != out_features {
            return None;
        }
        let x = tensor_values_as_f64(input);
        let w = tensor_values_as_f64(weight);
        let b = tensor_values_as_f64(bias);
        let mut out = matmul_f64(&x, &w, batch, in_features, out_features);
        for row in 0..batch {
            for col in 0..out_features {
                out[row * out_features + col] += b[col];
            }
        }
        let requires_grad = tensor_requires_autograd(registry, &[input_h, weight_h, bias_h]);
        let creator = requires_grad.then(|| AutogradNode {
            op: AutogradOp::MlLinear,
            parents: vec![input_h, weight_h, bias_h],
            input_shape: input.shape.clone(),
            left_shape: input.shape.clone(),
            right_shape: weight.shape.clone(),
            input: b,
            output: out.clone(),
            left: x,
            right: w,
            aux: vec![batch, in_features, out_features],
            #[cfg(feature = "gpu")]
            device_aux: None,
        });
        registry.note_kernel(batch * in_features * out_features);
        Some((vec![batch, out_features], out, requires_grad, creator))
    }) else {
        return 0;
    };
    match tensor_alloc_autograd(
        TensorDType::Float,
        shape,
        f64_values_to_host(&out),
        requires_grad,
        creator,
    ) {
        Ok(handle) => handle as SpectraHostValue,
        Err(_) => 0,
    }
}

/// Fast-path helper for `ml.mse_loss(prediction, target)`.
///
/// Mirrors `std_ml_mse_loss` but skips the generic host-call dispatch.
/// Returns the new loss tensor handle (>0) on success or 0 on error.
pub fn ml_mse_loss_fast(prediction_h: usize, target_h: usize) -> SpectraHostValue {
    let Some((_pred_shape, pred, pred_requires_grad)) = ml_tensor_float_data(prediction_h) else {
        return 0;
    };
    let Some((_target_shape, target, _target_requires_grad)) = ml_tensor_float_data(target_h)
    else {
        return 0;
    };
    if pred.len() != target.len() {
        return 0;
    }
    let n = pred.len() as f64;
    let value = pred
        .iter()
        .zip(target.iter())
        .map(|(p, t)| (p - t) * (p - t))
        .sum::<f64>()
        / n;
    let grad_pred: Vec<f64> = pred
        .iter()
        .zip(target.iter())
        .map(|(p, t)| 2.0 * (p - t) / n)
        .collect();
    let requires_grad = pred_requires_grad && tensor_is_grad_enabled();
    let creator = requires_grad.then(|| AutogradNode {
        op: AutogradOp::MlMse,
        parents: vec![prediction_h],
        input_shape: vec![pred.len()],
        left_shape: Vec::new(),
        right_shape: Vec::new(),
        input: Vec::new(),
        output: grad_pred,
        left: pred,
        right: target,
        aux: Vec::new(),
        #[cfg(feature = "gpu")]
        device_aux: None,
    });
    match tensor_alloc_autograd(
        TensorDType::Float,
        vec![1],
        vec![value.to_bits() as SpectraHostValue],
        requires_grad,
        creator,
    ) {
        Ok(handle) => handle as SpectraHostValue,
        Err(_) => 0,
    }
}

/// Fast-path helper for `tensor.backward(loss)`.
///
/// Mirrors `std_tensor_backward` but skips the generic host-call dispatch.
/// Returns `HOST_STATUS_SUCCESS` (0) on success or the error code on failure.
pub fn tensor_backward_fast(loss_h: usize) -> i32 {
    match tensor_backward_impl(loss_h) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

/// Materialize the gradient accumulated on a forward tensor.  This is an
/// explicit SSA edge used by compiler-native autodiff; it never walks the
/// creator graph.
pub fn tensor_grad_handle_fast(input_h: usize) -> i64 {
    let Some(values) = with_tensor_registry(|registry| {
        let tensor = registry.get(input_h)?;
        (tensor.dtype == TensorDType::Float).then(|| {
            tensor
                .grad
                .clone()
                .unwrap_or_else(|| vec![0.0; tensor.len()])
        })
    }) else {
        return 0;
    };
    tensor_alloc(
        TensorDType::Float,
        tensor_shape(input_h).unwrap_or_default(),
        f64_values_to_host(&values),
    )
    .map(|handle| handle as i64)
    .unwrap_or(0)
}

pub(crate) fn tensor_shape(handle: usize) -> Option<Vec<usize>> {
    with_tensor_registry(|registry| registry.get(handle).map(|tensor| tensor.shape.clone()))
}

/// Execute one explicitly lowered reverse step.  The compiler supplies the
/// output, upstream value and target handles, so this function may reuse the
/// established mathematical rule without discovering the graph topology.
pub fn tensor_autodiff_apply_fast(
    operation: i64,
    output_h: usize,
    upstream_h: usize,
    target0: usize,
    target1: usize,
    target2: usize,
) -> i32 {
    let expected = match operation {
        0 => AutogradOp::Add,
        1 => AutogradOp::Sub,
        2 => AutogradOp::Mul,
        3 => AutogradOp::Div,
        4 => AutogradOp::Neg,
        5 => AutogradOp::Exp,
        6 => AutogradOp::Log,
        7 => AutogradOp::Relu,
        8 => AutogradOp::Sigmoid,
        9 => AutogradOp::SumTensor,
        10 => AutogradOp::MeanTensor,
        11 => AutogradOp::DotTensor,
        12 => AutogradOp::Matmul,
        13 => AutogradOp::Transpose,
        14 => AutogradOp::View,
        15 => AutogradOp::MlLinear,
        16 => AutogradOp::MlMse,
        17 => AutogradOp::Tanh,
        18 => AutogradOp::Sqrt,
        19 => AutogradOp::MlBce,
        20 => AutogradOp::MlConv2d,
        21 => AutogradOp::MaxPool2d,
        22 => AutogradOp::Dropout,
        23 => AutogradOp::BatchedMatmul,
        24 => AutogradOp::Concat,
        25 => AutogradOp::Stack,
        26 => AutogradOp::Slice,
        27 => AutogradOp::Permute,
        _ => return HOST_STATUS_INVALID_ARGUMENT,
    };
    let targets = [target0, target1, target2]
        .into_iter()
        .filter(|handle| *handle != 0)
        .collect::<Vec<_>>();
    let result = with_tensor_registry(|registry| {
        let output = registry.get(output_h)?;
        let node = output.creator.clone()?;
        if node.op != expected || node.parents.iter().any(|parent| !targets.contains(parent)) {
            return None;
        }
        let grad = if upstream_h == 0 {
            if output.len() != 1 {
                return None;
            }
            vec![1.0]
        } else {
            let upstream = registry.get(upstream_h)?;
            if upstream.dtype != TensorDType::Float {
                return None;
            }
            tensor_values_as_f64(upstream)
        };
        let parent_grads = autograd_parent_grads(&node, &ParentGrad::Host(grad), registry).ok()?;
        for (parent, parent_grad) in parent_grads {
            if !targets.contains(&parent) {
                return None;
            }
            let tensor = registry.get_mut(parent)?;
            match parent_grad {
                ParentGrad::Host(values) => {
                    if !accumulate_tensor_grad(tensor, &values) {
                        return None;
                    }
                }
                #[cfg(feature = "gpu")]
                ParentGrad::Device(_) => return None,
            }
        }
        Some(HOST_STATUS_SUCCESS)
    });
    result.unwrap_or(HOST_STATUS_INVALID_ARGUMENT)
}

/// Fast-path helper for `ml.sgd_step(param, lr)`.
///
/// Mirrors `std_ml_sgd_step` but skips the generic host-call dispatch.
/// Returns `HOST_STATUS_SUCCESS` (0) on success, `HOST_STATUS_INVALID_ARGUMENT`
/// if the learning rate is invalid, or `HOST_STATUS_NOT_FOUND` if the
/// parameter handle is invalid or the tensor has no gradient.
pub fn ml_sgd_step_fast(param_h: usize, lr: f64) -> i32 {
    if !lr.is_finite() || lr < 0.0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    if !ml_optimizer_update(param_h, |value, grad, _| value - lr * grad) {
        return HOST_STATUS_NOT_FOUND;
    }
    HOST_STATUS_SUCCESS
}

/// Fast-path helper for `tensor.full_f(n, value)`.
///
/// Mirrors `std_tensor_full_f` but skips the generic host-call dispatch.
/// `value` is the `f64` fill value written to every element (stored as
/// its raw bit pattern in the tensor buffer). Returns the new tensor
/// handle (>0) on success or 0 on error.
pub fn tensor_full_f_fast(n: usize, value: f64) -> SpectraHostValue {
    if n == 0 {
        return 0;
    }
    let len = n;
    let pattern = value.to_bits() as SpectraHostValue;
    let buffer = with_tensor_registry(|registry| {
        if let Some(buffer) = registry.take_buffer_unfilled(len) {
            registry.metrics.reused_buffers = registry.metrics.reused_buffers.saturating_add(1);
            registry.metrics.pool_hits = registry.metrics.pool_hits.saturating_add(1);
            Some(buffer)
        } else {
            registry.metrics.pool_misses = registry.metrics.pool_misses.saturating_add(1);
            None
        }
    });
    let mut buffer = buffer.unwrap_or_else(|| Vec::with_capacity(len));
    if buffer.len() < len {
        buffer.resize(len, 0);
    }
    fill_i64_pattern(&mut buffer, pattern);
    match tensor_alloc_buffered(TensorDType::Float, vec![len], buffer) {
        Ok(handle) => handle as SpectraHostValue,
        Err(_) => 0,
    }
}
