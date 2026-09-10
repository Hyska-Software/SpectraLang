use super::*;
pub(crate) fn tensor_binary(
    ctx: *mut SpectraHostCallContext,
    op: AutogradOp,
    int_op: impl Fn(i64, i64) -> i64,
    float_op: ElementwiseOp,
) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((dtype, shape, data, requires_grad, creator, device, precision, residency)) =
            with_tensor_registry(|registry| {
                let left = registry.get(args[0] as usize)?.clone();
                let right = registry.get(args[1] as usize)?.clone();
                if left.shape != right.shape || left.dtype != right.dtype {
                    return None;
                }
                if left.device != right.device {
                    return None;
                }
                let element_count = left.len();
                let left_data = left.materialize();
                let right_data = right.materialize();
                #[cfg(feature = "gpu")]
                let gpu_data = if left.device == TensorDevice::Wgpu {
                    let gpu_op = match op {
                        AutogradOp::Add => Some(crate::gpu::GpuBinaryOp::Add),
                        AutogradOp::Sub => Some(crate::gpu::GpuBinaryOp::Sub),
                        AutogradOp::Mul => Some(crate::gpu::GpuBinaryOp::Mul),
                        AutogradOp::Div => Some(crate::gpu::GpuBinaryOp::Div),
                        _ => None,
                    }?;
                    match gpu_binary_float(&left, &right, gpu_op) {
                        Ok(Some(data)) => {
                            registry.note_gpu_kernel();
                            Some(data)
                        }
                        Ok(None) => None,
                        Err(err) => {
                            registry.note_gpu_error(err.kind);
                            registry.note_cpu_fallback();
                            None
                        }
                    }
                } else {
                    None
                };
                let data = match left.dtype {
                    TensorDType::Int => {
                        if left.device.is_accelerator() {
                            return None;
                        }
                        left_data
                            .iter()
                            .zip(right_data.iter())
                            .map(|(a, b)| int_op(*a, *b))
                            .collect()
                    }
                    TensorDType::Float => {
                        #[cfg(feature = "gpu")]
                        if let Some(data) = gpu_data {
                            data
                        } else {
                            tensor_binary_float_kernel(&left_data, &right_data, float_op)
                        }
                        #[cfg(not(feature = "gpu"))]
                        {
                            if left.device.is_accelerator() {
                                return None;
                            }
                            tensor_binary_float_kernel(&left_data, &right_data, float_op)
                        }
                    }
                };
                let requires_grad = left.dtype == TensorDType::Float
                    && tensor_requires_autograd(registry, &[args[0] as usize, args[1] as usize]);
                let creator = requires_grad.then(|| {
                    AutogradNode::binary(
                        op,
                        args[0] as usize,
                        args[1] as usize,
                        left.shape.clone(),
                        left_data
                            .iter()
                            .map(|raw| f64::from_bits(*raw as u64))
                            .collect(),
                        right_data
                            .iter()
                            .map(|raw| f64::from_bits(*raw as u64))
                            .collect(),
                    )
                });
                #[cfg(feature = "gpu")]
                let residency = if left.dtype == TensorDType::Float
                    && left.device == TensorDevice::Wgpu
                    && tensor_residency_pair(&left, &right)
                {
                    if let Some(gpu_op) = match op {
                        AutogradOp::Add => Some(crate::gpu::GpuBinaryOp::Add),
                        AutogradOp::Sub => Some(crate::gpu::GpuBinaryOp::Sub),
                        AutogradOp::Mul => Some(crate::gpu::GpuBinaryOp::Mul),
                        AutogradOp::Div => Some(crate::gpu::GpuBinaryOp::Div),
                        _ => None,
                    } {
                        let outcome =
                            tensor_residency_binary(registry, &left, &right, gpu_op, element_count);
                        match outcome {
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
                    }
                } else {
                    None
                };
                #[cfg(not(feature = "gpu"))]
                let residency: Option<()> = None;
                let _ = residency.as_ref();
                let result = Some((
                    left.dtype,
                    left.shape.clone(),
                    data,
                    requires_grad,
                    creator,
                    left.device,
                    left.precision,
                    #[cfg(feature = "gpu")]
                    residency,
                    #[cfg(not(feature = "gpu"))]
                    residency,
                ));
                registry.note_kernel(element_count);
                result
            })
        else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        #[cfg(feature = "gpu")]
        {
            if let Some(buf) = residency {
                return match tensor_alloc_autograd_on_device_with_buffer(
                    dtype,
                    shape,
                    data,
                    requires_grad,
                    creator,
                    device,
                    precision,
                    buf,
                ) {
                    Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
                    Err(code) => code,
                };
            }
        }
        let _ = residency;
        match tensor_alloc_autograd_on_device(
            dtype,
            shape,
            data,
            requires_grad,
            creator,
            device,
            precision,
        ) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_sum(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(value) = with_tensor_registry(|registry| {
            let tensor = registry.get(args[0] as usize)?.clone();
            let data = tensor.materialize();
            let value = match tensor.dtype {
                TensorDType::Int => {
                    if tensor.device.is_accelerator() {
                        return None;
                    }
                    data.iter().sum()
                }
                TensorDType::Float => {
                    #[cfg(not(feature = "gpu"))]
                    {
                        if tensor.device.is_accelerator() {
                            return None;
                        }
                        data.iter()
                            .map(|bits| f64::from_bits(*bits as u64))
                            .sum::<f64>() as i64
                    }
                    #[cfg(feature = "gpu")]
                    {
                        if tensor.device == TensorDevice::Wgpu
                            && tensor
                                .device_storage
                                .contains_key(&crate::gpu::PoolDevice::Wgpu)
                        {
                            match tensor_residency_reduce(
                                registry,
                                &tensor,
                                crate::gpu::GpuReduceOp::Sum,
                            ) {
                                Some(value) => value as i64,
                                None => data
                                    .iter()
                                    .map(|bits| f64::from_bits(*bits as u64))
                                    .sum::<f64>() as i64,
                            }
                        } else if tensor.device == TensorDevice::Wgpu {
                            let gpu_data = tensor_values_as_f32(&tensor)?;
                            match crate::gpu::sum(&gpu_data) {
                                Ok(value) => {
                                    registry.note_gpu_kernel();
                                    value as i64
                                }
                                Err(err) => {
                                    registry.note_gpu_error(err.kind);
                                    registry.note_cpu_fallback();
                                    data.iter()
                                        .map(|bits| f64::from_bits(*bits as u64))
                                        .sum::<f64>() as i64
                                }
                            }
                        } else {
                            data.iter()
                                .map(|bits| f64::from_bits(*bits as u64))
                                .sum::<f64>() as i64
                        }
                    }
                }
            };
            Some(value)
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, value)
    }
}

pub(crate) extern "C" fn std_tensor_sum_f(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(sum) = with_tensor_registry(|registry| {
            let tensor = registry.get(args[0] as usize)?.clone();
            let data = tensor.materialize();
            let sum = match tensor.dtype {
                TensorDType::Int => {
                    if tensor.device.is_accelerator() {
                        return None;
                    }
                    data.iter().map(|v| *v as f64).sum::<f64>()
                }
                TensorDType::Float => {
                    #[cfg(not(feature = "gpu"))]
                    {
                        if tensor.device.is_accelerator() {
                            return None;
                        }
                        data.iter()
                            .map(|bits| f64::from_bits(*bits as u64))
                            .sum::<f64>()
                    }
                    #[cfg(feature = "gpu")]
                    {
                        if tensor.device == TensorDevice::Wgpu
                            && tensor
                                .device_storage
                                .contains_key(&crate::gpu::PoolDevice::Wgpu)
                        {
                            match tensor_residency_reduce(
                                registry,
                                &tensor,
                                crate::gpu::GpuReduceOp::Sum,
                            ) {
                                Some(value) => value as f64,
                                None => data
                                    .iter()
                                    .map(|bits| f64::from_bits(*bits as u64))
                                    .sum::<f64>(),
                            }
                        } else if tensor.device == TensorDevice::Wgpu {
                            let gpu_data = tensor_values_as_f32(&tensor)?;
                            match crate::gpu::sum(&gpu_data) {
                                Ok(value) => {
                                    registry.note_gpu_kernel();
                                    value as f64
                                }
                                Err(_) => {
                                    registry.note_cpu_fallback();
                                    data.iter()
                                        .map(|bits| f64::from_bits(*bits as u64))
                                        .sum::<f64>()
                                }
                            }
                        } else {
                            data.iter()
                                .map(|bits| f64::from_bits(*bits as u64))
                                .sum::<f64>()
                        }
                    }
                }
            };
            Some(sum)
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, sum.to_bits() as i64)
    }
}

pub(crate) extern "C" fn std_tensor_sum_t(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_reduction_tensor(ctx, AutogradOp::SumTensor, |values| values.iter().sum())
}

pub(crate) extern "C" fn std_tensor_mean_f(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(value) = with_tensor_registry(|registry| {
            let tensor = registry.get(args[0] as usize)?.clone();
            #[cfg(feature = "gpu")]
            if let Some(mean_bits) =
                tensor_residency_scalar(registry, &tensor, crate::gpu::GpuReduceOp::Mean, |value| {
                    f64::from(value).to_bits() as i64
                })
            {
                return Some(mean_bits);
            }
            let data = tensor.materialize();
            if data.is_empty() {
                return Some(f64::NAN.to_bits() as i64);
            }
            let sum = match tensor.dtype {
                TensorDType::Int => data.iter().map(|v| *v as f64).sum::<f64>(),
                TensorDType::Float => data
                    .iter()
                    .map(|bits| f64::from_bits(*bits as u64))
                    .sum::<f64>(),
            };
            Some((sum / data.len() as f64).to_bits() as i64)
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, value)
    }
}

pub(crate) extern "C" fn std_tensor_mean_t(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_reduction_tensor(ctx, AutogradOp::MeanTensor, |values| {
        values.iter().sum::<f64>() / values.len() as f64
    })
}

pub(crate) fn tensor_reduction_tensor(
    ctx: *mut SpectraHostCallContext,
    op: AutogradOp,
    reduce: impl Fn(&[f64]) -> f64,
) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((value, requires_grad, creator)) = with_tensor_registry(|registry| {
            let tensor = registry.get(args[0] as usize)?.clone();
            #[cfg(feature = "gpu")]
            {
                // Device fast path: scalar reductions over a device-resident
                // float tensor that is not part of an autograd graph never
                // need the host values, so fold on the GPU and skip the
                // element-wise CPU pass entirely. Grad-enabled tensors fall
                // through: the backward creator needs the input copy anyway.
                if tensor.dtype == TensorDType::Float
                    && tensor.device == TensorDevice::Wgpu
                    && tensor.len() > 0
                    && tensor
                        .device_storage
                        .contains_key(&crate::gpu::PoolDevice::Wgpu)
                    && !tensor_requires_autograd(registry, &[args[0] as usize])
                {
                    let gpu_op = match op {
                        AutogradOp::MeanTensor => crate::gpu::GpuReduceOp::Mean,
                        _ => crate::gpu::GpuReduceOp::Sum,
                    };
                    if let Some(reduced) = tensor_residency_reduce(registry, &tensor, gpu_op) {
                        registry.note_kernel(tensor.len());
                        return Some((f64::from(reduced), false, None));
                    }
                }
            }
            let values = tensor_values_as_f64(&tensor);
            if values.is_empty() {
                return None;
            }
            let value = reduce(&values);
            let requires_grad = tensor.dtype == TensorDType::Float
                && tensor_requires_autograd(registry, &[args[0] as usize]);
            let creator = requires_grad.then(|| AutogradNode {
                op,
                parents: vec![args[0] as usize],
                input_shape: tensor.shape.clone(),
                left_shape: Vec::new(),
                right_shape: Vec::new(),
                input: values,
                output: vec![value],
                left: Vec::new(),
                right: Vec::new(),
                aux: Vec::new(),
                #[cfg(feature = "gpu")]
                device_aux: None,
            });
            registry.note_kernel(tensor.len());
            Some((value, requires_grad, creator))
        }) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match tensor_alloc_autograd(
            TensorDType::Float,
            vec![1],
            vec![value.to_bits() as SpectraHostValue],
            requires_grad,
            creator,
        ) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_max(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(value) = with_tensor_registry(|registry| {
            let tensor = registry.get(args[0] as usize)?.clone();
            #[cfg(feature = "gpu")]
            if let Some(max_bits) =
                tensor_residency_scalar(registry, &tensor, crate::gpu::GpuReduceOp::Max, |value| {
                    f64::from(value).to_bits() as i64
                })
            {
                return Some(max_bits);
            }
            let data = tensor.materialize();
            Some(match tensor.dtype {
                TensorDType::Int => data.iter().copied().max().unwrap_or(0),
                // Numeric compare on the decoded f64 values; the previous
                // raw-bit compare only agreed for non-negative data.
                TensorDType::Float if !data.is_empty() => data
                    .iter()
                    .map(|bits| f64::from_bits(*bits as u64))
                    .fold(f64::NEG_INFINITY, f64::max)
                    .to_bits() as i64,
                _ => 0,
            })
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, value)
    }
}

pub(crate) extern "C" fn std_tensor_min(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(value) = with_tensor_registry(|registry| {
            let tensor = registry.get(args[0] as usize)?.clone();
            #[cfg(feature = "gpu")]
            if let Some(min_bits) =
                tensor_residency_scalar(registry, &tensor, crate::gpu::GpuReduceOp::Min, |value| {
                    f64::from(value).to_bits() as i64
                })
            {
                return Some(min_bits);
            }
            let data = tensor.materialize();
            Some(match tensor.dtype {
                TensorDType::Int => data.iter().copied().min().unwrap_or(0),
                TensorDType::Float if !data.is_empty() => data
                    .iter()
                    .map(|bits| f64::from_bits(*bits as u64))
                    .fold(f64::INFINITY, f64::min)
                    .to_bits() as i64,
                _ => 0,
            })
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, value)
    }
}

pub(crate) extern "C" fn std_tensor_argmax(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(value) = with_tensor_registry(|registry| {
            let tensor = registry.get(args[0] as usize)?.clone();
            #[cfg(feature = "gpu")]
            if let Some(index) = tensor_residency_scalar(
                registry,
                &tensor,
                crate::gpu::GpuReduceOp::ArgMax,
                |value| value.to_bits() as i64,
            ) {
                return Some(index);
            }
            let data = tensor.materialize();
            if data.is_empty() {
                return Some(-1);
            }
            let mut best_index = 0usize;
            match tensor.dtype {
                TensorDType::Int => {
                    let mut best = data[0];
                    for (index, value) in data.iter().copied().enumerate().skip(1) {
                        if value > best {
                            best = value;
                            best_index = index;
                        }
                    }
                }
                TensorDType::Float => {
                    let mut best = f64::from_bits(data[0] as u64);
                    for (index, raw) in data.iter().copied().enumerate().skip(1) {
                        let value = f64::from_bits(raw as u64);
                        if value > best {
                            best = value;
                            best_index = index;
                        }
                    }
                }
            }
            Some(best_index as SpectraHostValue)
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, value)
    }
}

pub(crate) extern "C" fn std_tensor_transpose(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(tensor) = with_tensor_registry(|registry| {
            let tensor = registry.get(args[0] as usize)?;
            if tensor.shape.len() != 2 {
                return None;
            }
            let element_count = tensor.len();
            let mut result = StdTensor::from_storage(
                tensor.dtype,
                vec![tensor.shape[1], tensor.shape[0]],
                vec![tensor.strides[1], tensor.strides[0]],
                tensor.storage.clone(),
                tensor.offset,
                TensorLayout::View,
            )?;
            result.device = tensor.device;
            result.precision = tensor.precision;
            let requires_grad = tensor.dtype == TensorDType::Float
                && tensor_requires_autograd(registry, &[args[0] as usize]);
            if requires_grad {
                result.requires_grad = true;
                result.creator = Some(AutogradNode {
                    op: AutogradOp::Transpose,
                    parents: vec![args[0] as usize],
                    input_shape: tensor.shape.clone(),
                    left_shape: Vec::new(),
                    right_shape: Vec::new(),
                    input: tensor_values_as_f64(tensor),
                    output: Vec::new(),
                    left: Vec::new(),
                    right: Vec::new(),
                    aux: Vec::new(),
                    #[cfg(feature = "gpu")]
                    device_aux: None,
                });
            }
            registry.note_kernel(element_count);
            Some(result)
        }) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match tensor_insert(tensor) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_dot(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(value) = with_tensor_registry(|registry| {
            let left = registry.get(args[0] as usize)?;
            let right = registry.get(args[1] as usize)?;
            if left.shape.len() != 1 || left.shape != right.shape || left.dtype != right.dtype {
                return None;
            }
            let element_count = left.len();
            let left_data = left.materialize();
            let right_data = right.materialize();
            let value = match left.dtype {
                TensorDType::Int => Some(kernel_dot_i64(&left_data, &right_data)),
                TensorDType::Float => Some(kernel_dot_f64_bits(&left_data, &right_data)),
            };
            registry.note_kernel(element_count);
            value
        }) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        tensor_result(ctx_ref, value)
    }
}

pub(crate) extern "C" fn std_tensor_dot_t(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((value, requires_grad, creator)) = with_tensor_registry(|registry| {
            let left = registry.get(args[0] as usize)?;
            let right = registry.get(args[1] as usize)?;
            if left.shape.len() != 1
                || left.shape != right.shape
                || left.dtype != TensorDType::Float
                || right.dtype != TensorDType::Float
            {
                return None;
            }
            let left_values = tensor_values_as_f64(left);
            let right_values = tensor_values_as_f64(right);
            let value = left_values
                .iter()
                .zip(right_values.iter())
                .map(|(a, b)| a * b)
                .sum::<f64>();
            let requires_grad =
                tensor_requires_autograd(registry, &[args[0] as usize, args[1] as usize]);
            let creator = requires_grad.then(|| AutogradNode {
                op: AutogradOp::DotTensor,
                parents: vec![args[0] as usize, args[1] as usize],
                input_shape: vec![1],
                left_shape: left.shape.clone(),
                right_shape: right.shape.clone(),
                input: Vec::new(),
                output: vec![value],
                left: left_values,
                right: right_values,
                aux: Vec::new(),
                #[cfg(feature = "gpu")]
                device_aux: None,
            });
            registry.note_kernel(left.len());
            Some((value, requires_grad, creator))
        }) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match tensor_alloc_autograd(
            TensorDType::Float,
            vec![1],
            vec![value.to_bits() as SpectraHostValue],
            requires_grad,
            creator,
        ) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_neg(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_unary(ctx, AutogradOp::Neg, |v| v.saturating_neg(), |v| -v)
}

pub(crate) extern "C" fn std_tensor_exp_f(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_float_unary(ctx, AutogradOp::Exp, f64::exp)
}

pub(crate) extern "C" fn std_tensor_log_f(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_float_unary(ctx, AutogradOp::Log, f64::ln)
}

pub(crate) extern "C" fn std_tensor_sqrt_f(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_float_unary(ctx, AutogradOp::Sqrt, f64::sqrt)
}

pub(crate) extern "C" fn std_tensor_relu(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_unary(ctx, AutogradOp::Relu, |v| v.max(0), |v| v.max(0.0))
}

pub(crate) extern "C" fn std_tensor_sigmoid_f(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_float_unary(ctx, AutogradOp::Sigmoid, |v| 1.0 / (1.0 + (-v).exp()))
}

pub(crate) extern "C" fn std_tensor_tanh_f(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_float_unary(ctx, AutogradOp::Tanh, f64::tanh)
}

pub(crate) fn tensor_unary(
    ctx: *mut SpectraHostCallContext,
    op: AutogradOp,
    int_op: impl Fn(i64) -> i64,
    float_op: impl Fn(f64) -> f64,
) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((dtype, shape, data, requires_grad, creator, device, precision, residency)) =
            with_tensor_registry(|registry| {
                let tensor = registry.get(args[0] as usize)?.clone();
                let element_count = tensor.len();
                let source = tensor.materialize();
                #[cfg(feature = "gpu")]
                let gpu_data = if tensor.device == TensorDevice::Wgpu {
                    let gpu_op = match op {
                        AutogradOp::Neg => Some(crate::gpu::GpuUnaryOp::Neg),
                        AutogradOp::Relu => Some(crate::gpu::GpuUnaryOp::Relu),
                        _ => None,
                    }?;
                    match gpu_unary_float(&tensor, gpu_op) {
                        Ok(Some(data)) => {
                            registry.note_gpu_kernel();
                            Some(data)
                        }
                        Ok(None) => None,
                        Err(err) => {
                            registry.note_gpu_error(err.kind);
                            registry.note_cpu_fallback();
                            None
                        }
                    }
                } else {
                    None
                };
                let data: Vec<SpectraHostValue> = match tensor.dtype {
                    TensorDType::Int => {
                        if tensor.device.is_accelerator() {
                            return None;
                        }
                        source.iter().map(|value| int_op(*value)).collect()
                    }
                    TensorDType::Float => {
                        #[cfg(feature = "gpu")]
                        if let Some(data) = gpu_data {
                            data
                        } else {
                            source
                                .iter()
                                .map(|bits| float_op(f64::from_bits(*bits as u64)).to_bits() as i64)
                                .collect()
                        }
                        #[cfg(not(feature = "gpu"))]
                        {
                            if tensor.device.is_accelerator() {
                                return None;
                            }
                            source
                                .iter()
                                .map(|bits| float_op(f64::from_bits(*bits as u64)).to_bits() as i64)
                                .collect()
                        }
                    }
                };
                let requires_grad = tensor.dtype == TensorDType::Float
                    && tensor_requires_autograd(registry, &[args[0] as usize]);
                let creator = requires_grad.then(|| {
                    AutogradNode::unary(
                        op,
                        args[0] as usize,
                        tensor.shape.clone(),
                        source
                            .iter()
                            .map(|raw| f64::from_bits(*raw as u64))
                            .collect(),
                        data.iter().map(|raw| f64::from_bits(*raw as u64)).collect(),
                    )
                });
                #[cfg(feature = "gpu")]
                let residency = if tensor.dtype == TensorDType::Float
                    && tensor.device == TensorDevice::Wgpu
                    && tensor
                        .device_storage
                        .contains_key(&crate::gpu::PoolDevice::Wgpu)
                {
                    if let Some(gpu_op) = match op {
                        AutogradOp::Neg => Some(crate::gpu::GpuUnaryOp::Neg),
                        AutogradOp::Relu => Some(crate::gpu::GpuUnaryOp::Relu),
                        _ => None,
                    } {
                        match tensor_residency_unary(registry, &tensor, gpu_op, element_count) {
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
                    }
                } else {
                    None
                };
                #[cfg(not(feature = "gpu"))]
                let residency: Option<()> = None;
                let result = Some((
                    tensor.dtype,
                    tensor.shape.clone(),
                    data,
                    requires_grad,
                    creator,
                    tensor.device,
                    tensor.precision,
                    #[cfg(feature = "gpu")]
                    residency,
                    #[cfg(not(feature = "gpu"))]
                    residency,
                ));
                registry.note_kernel(element_count);
                result
            })
        else {
            return HOST_STATUS_NOT_FOUND;
        };
        #[cfg(feature = "gpu")]
        {
            if let Some(buf) = residency {
                return match tensor_alloc_autograd_on_device_with_buffer(
                    dtype,
                    shape,
                    data,
                    requires_grad,
                    creator,
                    device,
                    precision,
                    buf,
                ) {
                    Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
                    Err(code) => code,
                };
            }
        }
        let _ = residency;
        match tensor_alloc_autograd_on_device(
            dtype,
            shape,
            data,
            requires_grad,
            creator,
            device,
            precision,
        ) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}
