use super::*;
#[cfg(not(feature = "gpu"))]
pub(crate) fn autograd_parent_grads_gpu_dispatch(
    _node: &AutogradNode,
    _grad: &ParentGrad,
    _registry: &mut TensorRegistry,
) -> Option<Vec<(usize, ParentGrad)>> {
    None
}

pub(crate) fn autograd_parent_grads(
    node: &AutogradNode,
    grad: &ParentGrad,
    registry: &mut TensorRegistry,
) -> Option<Vec<(usize, ParentGrad)>> {
    // R-3080: try the GPU backward path first when both parents are
    // device-resident. The CPU path is the source of truth and is the
    // fall-through on any error or non-Wgpu device. The counter is
    // bumped from inside the dispatch via a global atomic; the registry
    // mutex must not be re-acquired here because the caller is still
    // holding it through `with_tensor_registry`.
    if let Some(gpu_grads) = autograd_parent_grads_gpu_dispatch(node, grad, registry) {
        return Some(gpu_grads);
    }
    autograd_parent_grads_cpu(node, grad)
}

pub(crate) fn autograd_parent_grads_cpu(
    node: &AutogradNode,
    grad: &ParentGrad,
) -> Option<Vec<(usize, ParentGrad)>> {
    let grad = match grad {
        ParentGrad::Host(values) => values.as_slice(),
        #[cfg(feature = "gpu")]
        ParentGrad::Device(_) => return None,
    };
    let host_grad = |v: Vec<f64>| ParentGrad::Host(v);
    let pair = |a: Vec<f64>, b: Vec<f64>| -> Vec<(usize, ParentGrad)> {
        vec![
            (node.parents[0], host_grad(a)),
            (node.parents[1], host_grad(b)),
        ]
    };
    let single =
        |a: Vec<f64>| -> Vec<(usize, ParentGrad)> { vec![(node.parents[0], host_grad(a))] };
    match node.op {
        AutogradOp::Add => Some(pair(grad.to_vec(), grad.to_vec())),
        AutogradOp::Sub => Some(pair(grad.to_vec(), grad.iter().map(|v| -*v).collect())),
        AutogradOp::Mul => Some(pair(
            grad.iter()
                .zip(node.right.iter())
                .map(|(g, r)| g * r)
                .collect(),
            grad.iter()
                .zip(node.left.iter())
                .map(|(g, l)| g * l)
                .collect(),
        )),
        AutogradOp::Div => Some(pair(
            grad.iter()
                .zip(node.right.iter())
                .map(|(g, r)| g / r)
                .collect(),
            grad.iter()
                .zip(node.left.iter().zip(node.right.iter()))
                .map(|(g, (l, r))| -(g * l) / (r * r))
                .collect(),
        )),
        AutogradOp::Neg => Some(single(grad.iter().map(|v| -*v).collect())),
        AutogradOp::Relu => Some(single(
            grad.iter()
                .zip(node.input.iter())
                .map(|(g, x)| if *x > 0.0 { *g } else { 0.0 })
                .collect(),
        )),
        AutogradOp::Exp => Some(single(
            grad.iter()
                .zip(node.output.iter())
                .map(|(g, y)| g * y)
                .collect(),
        )),
        AutogradOp::Log => Some(single(
            grad.iter()
                .zip(node.input.iter())
                .map(|(g, x)| g / x)
                .collect(),
        )),
        AutogradOp::Sqrt => Some(single(
            grad.iter()
                .zip(node.output.iter())
                .map(|(g, y)| g * 0.5 / y)
                .collect(),
        )),
        AutogradOp::Sigmoid => Some(single(
            grad.iter()
                .zip(node.output.iter())
                .map(|(g, y)| g * y * (1.0 - y))
                .collect(),
        )),
        AutogradOp::Tanh => Some(single(
            grad.iter()
                .zip(node.output.iter())
                .map(|(g, y)| g * (1.0 - y * y))
                .collect(),
        )),
        AutogradOp::SumTensor => Some(single(vec![grad[0]; node.input.len()])),
        AutogradOp::MeanTensor => Some(single(vec![
            grad[0] / node.input.len() as f64;
            node.input.len()
        ])),
        AutogradOp::Transpose => {
            let rows = node.input_shape[0];
            let cols = node.input_shape[1];
            Some(single(transpose_f64(grad, cols, rows)))
        }
        AutogradOp::View => Some(single(grad.to_vec())),
        AutogradOp::DotTensor => Some(pair(
            node.right.iter().map(|value| grad[0] * value).collect(),
            node.left.iter().map(|value| grad[0] * value).collect(),
        )),
        AutogradOp::Matmul => {
            let (m, k) = (node.left_shape[0], node.left_shape[1]);
            let n = node.right_shape[1];
            let right_t = transpose_f64(&node.right, k, n);
            let left_t = transpose_f64(&node.left, m, k);
            Some(pair(
                matmul_f64(grad, &right_t, m, n, k),
                matmul_f64(&left_t, grad, k, m, n),
            ))
        }
        AutogradOp::MlLinear => {
            let (batch, in_features, out_features) = (node.aux[0], node.aux[1], node.aux[2]);
            let weight_t = transpose_f64(&node.right, in_features, out_features);
            let input_t = transpose_f64(&node.left, batch, in_features);
            let grad_input = matmul_f64(grad, &weight_t, batch, out_features, in_features);
            let grad_weight = matmul_f64(&input_t, grad, in_features, batch, out_features);
            let mut grad_bias = vec![0.0; out_features];
            for row in 0..batch {
                for col in 0..out_features {
                    grad_bias[col] += grad[row * out_features + col];
                }
            }
            Some(vec![
                (node.parents[0], host_grad(grad_input)),
                (node.parents[1], host_grad(grad_weight)),
                (node.parents[2], host_grad(grad_bias)),
            ])
        }
        AutogradOp::MlMse => {
            let n = node.left.len() as f64;
            Some(single(
                node.left
                    .iter()
                    .zip(node.right.iter())
                    .map(|(p, t)| grad[0] * 2.0 * (p - t) / n)
                    .collect(),
            ))
        }
        AutogradOp::MlBce => {
            let n = node.left.len() as f64;
            Some(single(
                node.left
                    .iter()
                    .zip(node.right.iter())
                    .map(|(p, t)| {
                        let p = p.clamp(1e-7, 1.0 - 1e-7);
                        grad[0] * (p - t) / (p * (1.0 - p) * n)
                    })
                    .collect(),
            ))
        }
        AutogradOp::MlCrossEntropy | AutogradOp::MlNll => {
            let batch = node.aux[0];
            Some(single(
                node.output
                    .iter()
                    .map(|v| grad[0] * v / batch as f64)
                    .collect(),
            ))
        }
        AutogradOp::MlConv2d => {
            let (batch, in_ch, h, w, out_ch, kh, kw, out_h, out_w) = (
                node.aux[0],
                node.aux[1],
                node.aux[2],
                node.aux[3],
                node.aux[4],
                node.aux[5],
                node.aux[6],
                node.aux[7],
                node.aux[8],
            );
            let mut grad_input = vec![0.0; batch * in_ch * h * w];
            let mut grad_kernel = vec![0.0; out_ch * in_ch * kh * kw];
            let mut grad_bias = vec![0.0; out_ch];
            for n in 0..batch {
                for oc in 0..out_ch {
                    for oy in 0..out_h {
                        for ox in 0..out_w {
                            let g = grad[((n * out_ch + oc) * out_h + oy) * out_w + ox];
                            grad_bias[oc] += g;
                            for ic in 0..in_ch {
                                for ky in 0..kh {
                                    for kx in 0..kw {
                                        let iy = oy + ky;
                                        let ix = ox + kx;
                                        let input_idx = ((n * in_ch + ic) * h + iy) * w + ix;
                                        let kernel_idx = ((oc * in_ch + ic) * kh + ky) * kw + kx;
                                        grad_input[input_idx] += g * node.right[kernel_idx];
                                        grad_kernel[kernel_idx] += g * node.left[input_idx];
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Some(vec![
                (node.parents[0], host_grad(grad_input)),
                (node.parents[1], host_grad(grad_kernel)),
                (node.parents[2], host_grad(grad_bias)),
            ])
        }
    }
}

pub(crate) fn tensor_backward_impl(loss_handle: usize) -> Result<(), i32> {
    let mut stack = with_tensor_registry(|registry| {
        let loss = registry.get(loss_handle).ok_or(HOST_STATUS_NOT_FOUND)?;
        if loss.dtype != TensorDType::Float || loss.len() != 1 {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }
        Ok::<_, i32>(vec![(loss_handle, ParentGrad::Host(vec![1.0]))])
    })?;
    let mut visited = Vec::new();

    while let Some((handle, grad)) = stack.pop() {
        let next = with_tensor_registry(|registry| {
            let grad_for_parents = grad.clone();
            let Some(mut boxed) = registry.take(handle) else {
                return Ok::<Vec<(usize, ParentGrad)>, i32>(Vec::new());
            };
            let node = boxed.as_ref().creator.clone();
            if !accumulate_parent_grad(registry, boxed.as_mut(), grad) {
                let _ = registry.put(handle, boxed);
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            visited.push(handle);
            let _ = registry.put(handle, boxed);
            let Some(node) = node else {
                return Ok(Vec::new());
            };
            Ok(autograd_parent_grads(&node, &grad_for_parents, registry).unwrap_or_default())
        })?;
        stack.extend(next);
    }

    with_tensor_registry(|registry| {
        for handle in visited {
            if let Some(tensor) = registry.get_mut(handle) {
                tensor.creator = None;
            }
        }
    });
    Ok(())
}

pub(crate) extern "C" fn std_tensor_requires_grad(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let ok = with_tensor_registry(|registry| {
            let handle = args[0] as usize;
            let Some(mut boxed) = registry.take(handle) else {
                return false;
            };
            let tensor = boxed.as_mut();
            if tensor.dtype != TensorDType::Float {
                let _ = registry.put(handle, boxed);
                return false;
            }
            tensor.requires_grad = args[1] != 0;
            if !tensor.requires_grad {
                tensor.grad = None;
                tensor.creator = None;
                #[cfg(feature = "gpu")]
                {
                    for (_, buf) in tensor.device_grad.drain() {
                        registry.device_arena.release(buf);
                    }
                }
            }
            let _ = registry.put(handle, boxed);
            true
        });
        if !ok {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        tensor_result(ctx_ref, args[0])
    }
}

pub(crate) extern "C" fn std_tensor_backward(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match tensor_backward_impl(args[0] as usize) {
            Ok(()) => tensor_optional_result(ctx_ref, 0),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_grad(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((shape, grad)) = with_tensor_registry(|registry| {
            let tensor = registry.get(args[0] as usize)?;
            if let Some(grad) = tensor.grad.clone() {
                return Some((tensor.shape.clone(), grad));
            }
            #[cfg(feature = "gpu")]
            {
                let device_grad = tensor
                    .device_grad
                    .get(&crate::gpu::PoolDevice::Wgpu)?
                    .clone();
                let values = match crate::gpu::with_device_queue(|device, queue| {
                    crate::gpu::readback_device_f32(&device_grad, device, queue)
                }) {
                    Ok(Ok(values)) => values.into_iter().map(|v| v as f64).collect(),
                    Ok(Err(err)) | Err(err) => {
                        registry.note_gpu_error(err.kind);
                        registry.note_cpu_fallback();
                        return None;
                    }
                };
                return Some((tensor.shape.clone(), values));
            }
            #[cfg(not(feature = "gpu"))]
            {
                None
            }
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        match tensor_alloc(TensorDType::Float, shape, f64_values_to_host(&grad)) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_zero_grad(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let ok = with_tensor_registry(|registry| {
            let handle = args[0] as usize;
            let Some(mut boxed) = registry.take(handle) else {
                return false;
            };
            let tensor = boxed.as_mut();
            tensor.grad = None;
            #[cfg(feature = "gpu")]
            {
                for (_, buf) in tensor.device_grad.drain() {
                    registry.device_arena.release(buf);
                }
            }
            let _ = registry.put(handle, boxed);
            true
        });
        if !ok {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_tensor_set_grad_enabled(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        *lock_unpoisoned(tensor_grad_enabled()) = args[0] != 0;
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_tensor_grad_enabled(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        tensor_result(ctx_ref, tensor_is_grad_enabled() as SpectraHostValue)
    }
}
