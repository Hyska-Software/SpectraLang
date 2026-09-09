use super::*;
pub(crate) extern "C" fn std_ml_module_add_parameter(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let ok = with_ml_registry(|registry| {
            let Some(module) = registry.modules.get_mut(&(args[0] as usize)) else {
                return false;
            };
            module.parameters.push(args[1] as usize);
            true
        });
        if !ok {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_ml_module_parameter_count(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(count) = with_ml_registry(|registry| {
            registry
                .modules
                .get(&(args[0] as usize))
                .map(|module| module.parameters.len())
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, count as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_module_parameter(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[1] < 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(param) = with_ml_registry(|registry| {
            registry
                .modules
                .get(&(args[0] as usize))
                .and_then(|module| module.parameters.get(args[1] as usize).copied())
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, param as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_module_set_training(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let ok = with_ml_registry(|registry| {
            let Some(module) = registry.modules.get_mut(&(args[0] as usize)) else {
                return false;
            };
            module.training = args[1] != 0;
            true
        });
        if !ok {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_ml_module_is_training(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(training) = with_ml_registry(|registry| {
            registry
                .modules
                .get(&(args[0] as usize))
                .map(|module| module.training)
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, training as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_linear(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (input_h, weight_h, bias_h) = (args[0] as usize, args[1] as usize, args[2] as usize);
        let Some((shape, out, requires_grad, creator, device, precision, residency)) =
            with_tensor_registry(|registry| {
                let input = registry.get(input_h)?.clone();
                let weight = registry.get(weight_h)?.clone();
                let bias = registry.get(bias_h)?.clone();
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
                let x = tensor_values_as_f64(&input);
                let w = tensor_values_as_f64(&weight);
                let b = tensor_values_as_f64(&bias);
                let mut out = matmul_f64(&x, &w, batch, in_features, out_features);
                for row in 0..batch {
                    for col in 0..out_features {
                        out[row * out_features + col] += b[col];
                    }
                }
                let requires_grad =
                    tensor_requires_autograd(registry, &[input_h, weight_h, bias_h]);
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
                #[cfg(feature = "gpu")]
                let residency = if input.device == TensorDevice::Wgpu
                    && input
                        .device_storage
                        .contains_key(&crate::gpu::PoolDevice::Wgpu)
                    && weight
                        .device_storage
                        .contains_key(&crate::gpu::PoolDevice::Wgpu)
                    && bias
                        .device_storage
                        .contains_key(&crate::gpu::PoolDevice::Wgpu)
                {
                    match tensor_residency_ml_linear(
                        registry,
                        &input,
                        &weight,
                        &bias,
                        batch,
                        in_features,
                        out_features,
                    ) {
                        ResidencyOutcome::Ok(buf) => Some(buf),
                        ResidencyOutcome::CpuFallback => None,
                        ResidencyOutcome::Error(err) => {
                            registry.note_gpu_error(err.kind);
                            registry.note_cpu_fallback();
                            None
                        }
                    }
                } else {
                    None
                };
                #[cfg(not(feature = "gpu"))]
                let residency: Option<()> = None;
                registry.note_kernel(batch * in_features * out_features);
                Some((
                    vec![batch, out_features],
                    out,
                    requires_grad,
                    creator,
                    input.device,
                    input.precision,
                    #[cfg(feature = "gpu")]
                    residency,
                    #[cfg(not(feature = "gpu"))]
                    residency,
                ))
            })
        else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        #[cfg(feature = "gpu")]
        {
            if let Some(buf) = residency {
                let host_data = f64_values_to_host(&out);
                return match tensor_alloc_autograd_on_device_with_buffer(
                    TensorDType::Float,
                    shape,
                    host_data,
                    requires_grad,
                    creator,
                    TensorDevice::Wgpu,
                    TensorPrecision::F32,
                    buf,
                ) {
                    Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
                    Err(code) => code,
                };
            }
        }
        let _ = residency;
        if device == TensorDevice::Wgpu {
            match tensor_alloc_autograd_on_device(
                TensorDType::Float,
                shape,
                f64_values_to_host(&out),
                requires_grad,
                creator,
                device,
                precision,
            ) {
                Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
                Err(code) => code,
            }
        } else {
            match tensor_alloc_autograd(
                TensorDType::Float,
                shape,
                f64_values_to_host(&out),
                requires_grad,
                creator,
            ) {
                Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
                Err(code) => code,
            }
        }
    }
}

pub(crate) extern "C" fn std_ml_conv2d(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 10) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (input_h, kernel_h, bias_h) = (args[0] as usize, args[1] as usize, args[2] as usize);
        let dims = [
            args[3] as usize,
            args[4] as usize,
            args[5] as usize,
            args[6] as usize,
            args[7] as usize,
            args[8] as usize,
            args[9] as usize,
        ];
        if args[3..].iter().any(|v| *v <= 0) {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let (batch, in_ch, h, w, out_ch, kh, kw) = (
            dims[0], dims[1], dims[2], dims[3], dims[4], dims[5], dims[6],
        );
        if h < kh || w < kw {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some((out, requires_grad, creator, device, residency)) =
            with_tensor_registry(|registry| {
                let input = registry.get(input_h)?.clone();
                let kernel = registry.get(kernel_h)?.clone();
                let bias = registry.get(bias_h)?.clone();
                if input.dtype != TensorDType::Float
                    || kernel.dtype != TensorDType::Float
                    || bias.dtype != TensorDType::Float
                    || input.device != kernel.device
                    || input.device != bias.device
                    || input.len() != batch * in_ch * h * w
                    || kernel.len() != out_ch * in_ch * kh * kw
                    || bias.len() != out_ch
                {
                    return None;
                }
                let x = tensor_values_as_f64(&input);
                let k = tensor_values_as_f64(&kernel);
                let b = tensor_values_as_f64(&bias);
                let device = input.device;
                let (out_h, out_w) = (h - kh + 1, w - kw + 1);
                let cpu_conv2d = || {
                    let mut out = vec![0.0; batch * out_ch * out_h * out_w];
                    for n in 0..batch {
                        for oc in 0..out_ch {
                            for oy in 0..out_h {
                                for ox in 0..out_w {
                                    let mut acc = b[oc];
                                    for ic in 0..in_ch {
                                        for ky in 0..kh {
                                            for kx in 0..kw {
                                                let input_idx =
                                                    ((n * in_ch + ic) * h + oy + ky) * w + ox + kx;
                                                let kernel_idx =
                                                    ((oc * in_ch + ic) * kh + ky) * kw + kx;
                                                acc += x[input_idx] * k[kernel_idx];
                                            }
                                        }
                                    }
                                    out[((n * out_ch + oc) * out_h + oy) * out_w + ox] = acc;
                                }
                            }
                        }
                    }
                    out
                };
                #[cfg(feature = "gpu")]
                let out = if input.device == TensorDevice::Wgpu {
                    let x_gpu = tensor_values_as_f32(&input)?;
                    let k_gpu = tensor_values_as_f32(&kernel)?;
                    let b_gpu = tensor_values_as_f32(&bias)?;
                    match crate::gpu::conv2d(&x_gpu, &k_gpu, &b_gpu, dims) {
                        Ok(values) => {
                            registry.note_gpu_kernel();
                            values.iter().map(|value| *value as f64).collect::<Vec<_>>()
                        }
                        Err(err) => {
                            registry.note_gpu_error(err.kind);
                            registry.note_cpu_fallback();
                            cpu_conv2d()
                        }
                    }
                } else {
                    cpu_conv2d()
                };
                #[cfg(not(feature = "gpu"))]
                let out = {
                    if input.device.is_accelerator() {
                        return None;
                    }
                    cpu_conv2d()
                };
                let requires_grad =
                    tensor_requires_autograd(registry, &[input_h, kernel_h, bias_h]);
                let creator = requires_grad.then(|| AutogradNode {
                    op: AutogradOp::MlConv2d,
                    parents: vec![input_h, kernel_h, bias_h],
                    input_shape: input.shape.clone(),
                    left_shape: input.shape.clone(),
                    right_shape: kernel.shape.clone(),
                    input: b,
                    output: out.clone(),
                    left: x,
                    right: k,
                    aux: vec![batch, in_ch, h, w, out_ch, kh, kw, out_h, out_w],
                    #[cfg(feature = "gpu")]
                    device_aux: None,
                });
                #[cfg(feature = "gpu")]
                let residency = if input.device == TensorDevice::Wgpu
                    && input
                        .device_storage
                        .contains_key(&crate::gpu::PoolDevice::Wgpu)
                    && kernel
                        .device_storage
                        .contains_key(&crate::gpu::PoolDevice::Wgpu)
                    && bias
                        .device_storage
                        .contains_key(&crate::gpu::PoolDevice::Wgpu)
                {
                    match tensor_residency_conv2d(registry, &input, &kernel, &bias, dims) {
                        ResidencyOutcome::Ok(buf) => Some(buf),
                        ResidencyOutcome::CpuFallback => None,
                        ResidencyOutcome::Error(err) => {
                            registry.note_gpu_error(err.kind);
                            registry.note_cpu_fallback();
                            None
                        }
                    }
                } else {
                    None
                };
                #[cfg(not(feature = "gpu"))]
                let residency: Option<()> = None;
                registry.note_kernel(batch * out_ch * out_h * out_w * in_ch * kh * kw);
                Some((
                    out,
                    requires_grad,
                    creator,
                    device,
                    #[cfg(feature = "gpu")]
                    residency,
                    #[cfg(not(feature = "gpu"))]
                    residency,
                ))
            })
        else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        #[cfg(feature = "gpu")]
        {
            if let Some(buf) = residency {
                return match tensor_alloc_autograd_on_device_with_buffer(
                    TensorDType::Float,
                    vec![out.len()],
                    f64_values_to_host(&out),
                    requires_grad,
                    creator,
                    device,
                    TensorPrecision::F32,
                    buf,
                ) {
                    Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
                    Err(code) => code,
                };
            }
        }
        let _ = residency;
        match tensor_alloc_autograd_on_device(
            TensorDType::Float,
            vec![out.len()],
            f64_values_to_host(&out),
            requires_grad,
            creator,
            device,
            TensorPrecision::F32,
        ) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_dropout(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((shape, data, requires_grad)) = ml_tensor_float_data(args[0] as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let p = f64::from_bits(args[1] as u64);
        let training = args[2] != 0;
        if !(0.0..1.0).contains(&p) {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        // Inverted dropout with a seeded per-call mask: the seed advances
        // the shared global stream, and the recorded node stores it so the
        // backward pass regenerates the exact keep pattern. Inference mode
        // stays an identity passthrough.
        let seed = lcg_next(&mut lock_unpoisoned(random_state()));
        let scale = 1.0 / (1.0 - p);
        let mut stream = seed;
        let out = if training {
            data.iter()
                .map(|v| {
                    if dropout_keep_draw(&mut stream, p) {
                        v * scale
                    } else {
                        0.0
                    }
                })
                .collect::<Vec<_>>()
        } else {
            data.clone()
        };
        let creator = requires_grad.then(|| AutogradNode {
            op: AutogradOp::Dropout,
            parents: vec![args[0] as usize],
            input_shape: shape.clone(),
            left_shape: Vec::new(),
            right_shape: Vec::new(),
            input: data,
            output: out.clone(),
            left: Vec::new(),
            right: Vec::new(),
            aux: vec![seed as usize, training as usize, p.to_bits() as usize],
            #[cfg(feature = "gpu")]
            device_aux: None,
        });
        match tensor_alloc_autograd(
            TensorDType::Float,
            shape,
            f64_values_to_host(&out),
            requires_grad,
            creator,
        ) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_max_pool2d(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 7) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (input_h, batch, channels, h, w, pool_h, pool_w) = (
            args[0] as usize,
            args[1] as usize,
            args[2] as usize,
            args[3] as usize,
            args[4] as usize,
            args[5] as usize,
            args[6] as usize,
        );
        if pool_h == 0 || pool_w == 0 || h % pool_h != 0 || w % pool_w != 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some((shape, data, requires_grad)) = ml_tensor_float_data(input_h) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if data.len() != batch * channels * h * w {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let (out_h, out_w) = (h / pool_h, w / pool_w);
        let mut out = vec![0.0; batch * channels * out_h * out_w];
        for n in 0..batch {
            for c in 0..channels {
                for oy in 0..out_h {
                    for ox in 0..out_w {
                        let mut best = f64::NEG_INFINITY;
                        for py in 0..pool_h {
                            for px in 0..pool_w {
                                let iy = oy * pool_h + py;
                                let ix = ox * pool_w + px;
                                best = best.max(data[((n * channels + c) * h + iy) * w + ix]);
                            }
                        }
                        out[((n * channels + c) * out_h + oy) * out_w + ox] = best;
                    }
                }
            }
        }
        // Record the argmax-routing node so gradients flow back to the
        // winning input positions. Values are untouched, so existing
        // forward-only callers observe identical outputs.
        let creator = requires_grad.then(|| AutogradNode {
            op: AutogradOp::MaxPool2d,
            parents: vec![input_h],
            input_shape: shape.clone(),
            left_shape: Vec::new(),
            right_shape: Vec::new(),
            input: data,
            output: out.clone(),
            left: Vec::new(),
            right: Vec::new(),
            aux: vec![batch, channels, h, w, pool_h, pool_w, out_h, out_w],
            #[cfg(feature = "gpu")]
            device_aux: None,
        });
        match tensor_alloc_autograd(
            TensorDType::Float,
            vec![out.len()],
            f64_values_to_host(&out),
            requires_grad,
            creator,
        ) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) fn ml_two_tensor_loss(
    ctx: *mut SpectraHostCallContext,
    op: AutogradOp,
    value_and_grad: impl Fn(&[f64], &[f64]) -> Option<(f64, Vec<f64>)>,
) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((_pred_shape, pred, pred_requires_grad)) = ml_tensor_float_data(args[0] as usize)
        else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((_target_shape, target, _target_requires_grad)) =
            ml_tensor_float_data(args[1] as usize)
        else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if pred.len() != target.len() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some((value, grad_pred)) = value_and_grad(&pred, &target) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let requires_grad = pred_requires_grad && tensor_is_grad_enabled();
        let creator = requires_grad.then(|| AutogradNode {
            op,
            parents: vec![args[0] as usize],
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
        ml_loss_tensor(ctx_ref, value, requires_grad, creator)
    }
}

pub(crate) extern "C" fn std_ml_mse_loss(ctx: *mut SpectraHostCallContext) -> i32 {
    #[cfg(feature = "gpu")]
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let pred_h = args[0] as usize;
        let target_h = args[1] as usize;
        let device_loss = with_tensor_registry(|registry| {
            let prediction = registry.get(pred_h)?.clone();
            let target = registry.get(target_h)?.clone();
            if prediction.dtype != TensorDType::Float
                || target.dtype != TensorDType::Float
                || prediction.len() != target.len()
                || prediction.len() == 0
            {
                return None;
            }
            let pred_buf = prediction
                .device_storage
                .get(&crate::gpu::PoolDevice::Wgpu)?
                .clone();
            let target_buf = target
                .device_storage
                .get(&crate::gpu::PoolDevice::Wgpu)?
                .clone();
            let outcome = crate::gpu::with_device_queue(|device, queue| {
                let out_buf = registry.device_arena.acquire(
                    crate::gpu::PoolDevice::Wgpu,
                    crate::gpu::PoolDType::Float,
                    1,
                    device,
                );
                let loss_result =
                    crate::gpu::mse_loss_device(&pred_buf, &target_buf, &out_buf, device, queue);
                let value_result = loss_result.and_then(|()| {
                    crate::gpu::readback_scalar_device(&out_buf, device, queue)
                        .map(|value| value as f64)
                });
                (value_result, out_buf)
            });
            match outcome {
                Ok((Ok(value), out_buf)) => {
                    registry.device_arena.release(out_buf);
                    registry.note_gpu_kernel();
                    let requires_grad = prediction.requires_grad && tensor_is_grad_enabled();
                    let creator = requires_grad.then(|| AutogradNode {
                        op: AutogradOp::MlMse,
                        parents: vec![pred_h],
                        input_shape: vec![prediction.len()],
                        left_shape: Vec::new(),
                        right_shape: Vec::new(),
                        input: Vec::new(),
                        output: Vec::new(),
                        left: Vec::new(),
                        right: Vec::new(),
                        aux: vec![prediction.len()],
                        #[cfg(feature = "gpu")]
                        device_aux: Some(target_buf),
                    });
                    Some((value, requires_grad, creator))
                }
                Ok((Err(err), out_buf)) => {
                    registry.device_arena.release(out_buf);
                    registry.note_gpu_error(err.kind);
                    registry.note_cpu_fallback();
                    None
                }
                Err(err) => {
                    registry.note_gpu_error(err.kind);
                    registry.note_cpu_fallback();
                    None
                }
            }
        });
        if let Some((value, requires_grad, creator)) = device_loss {
            return ml_loss_tensor(ctx_ref, value, requires_grad, creator);
        }
    }
    ml_two_tensor_loss(ctx, AutogradOp::MlMse, |pred, target| {
        let n = pred.len() as f64;
        let value = pred
            .iter()
            .zip(target.iter())
            .map(|(p, t)| (p - t) * (p - t))
            .sum::<f64>()
            / n;
        Some((value, Vec::new()))
    })
}

pub(crate) extern "C" fn std_ml_bce_loss(ctx: *mut SpectraHostCallContext) -> i32 {
    #[cfg(feature = "gpu")]
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let pred_h = args[0] as usize;
        let target_h = args[1] as usize;
        let device_loss = with_tensor_registry(|registry| {
            let prediction = registry.get(pred_h)?.clone();
            let target = registry.get(target_h)?.clone();
            if prediction.dtype != TensorDType::Float
                || target.dtype != TensorDType::Float
                || prediction.len() != target.len()
                || prediction.len() == 0
            {
                return None;
            }
            let pred_buf = prediction
                .device_storage
                .get(&crate::gpu::PoolDevice::Wgpu)?
                .clone();
            let target_buf = target
                .device_storage
                .get(&crate::gpu::PoolDevice::Wgpu)?
                .clone();
            let outcome = crate::gpu::with_device_queue(|device, queue| {
                let out_buf = registry.device_arena.acquire(
                    crate::gpu::PoolDevice::Wgpu,
                    crate::gpu::PoolDType::Float,
                    1,
                    device,
                );
                let loss_result =
                    crate::gpu::bce_loss_device(&pred_buf, &target_buf, &out_buf, device, queue);
                let value_result = loss_result.and_then(|()| {
                    crate::gpu::readback_scalar_device(&out_buf, device, queue)
                        .map(|value| value as f64)
                });
                (value_result, out_buf)
            });
            match outcome {
                Ok((Ok(value), out_buf)) => {
                    registry.device_arena.release(out_buf);
                    registry.note_gpu_kernel();
                    let requires_grad = prediction.requires_grad && tensor_is_grad_enabled();
                    let creator = requires_grad.then(|| AutogradNode {
                        op: AutogradOp::MlBce,
                        parents: vec![pred_h],
                        input_shape: vec![prediction.len()],
                        left_shape: Vec::new(),
                        right_shape: Vec::new(),
                        input: Vec::new(),
                        output: Vec::new(),
                        left: Vec::new(),
                        right: Vec::new(),
                        aux: vec![prediction.len()],
                        device_aux: Some(target_buf),
                    });
                    Some((value, requires_grad, creator))
                }
                Ok((Err(err), out_buf)) => {
                    registry.device_arena.release(out_buf);
                    registry.note_gpu_error(err.kind);
                    registry.note_cpu_fallback();
                    None
                }
                Err(err) => {
                    registry.note_gpu_error(err.kind);
                    registry.note_cpu_fallback();
                    None
                }
            }
        });
        if let Some((value, requires_grad, creator)) = device_loss {
            return ml_loss_tensor(ctx_ref, value, requires_grad, creator);
        }
    }
    ml_two_tensor_loss(ctx, AutogradOp::MlBce, |pred, target| {
        let n = pred.len() as f64;
        let value = pred
            .iter()
            .zip(target.iter())
            .map(|(p, t)| {
                let p = p.clamp(1e-7, 1.0 - 1e-7);
                -(t * p.ln() + (1.0 - t) * (1.0 - p).ln())
            })
            .sum::<f64>()
            / n;
        Some((value, Vec::new()))
    })
}
