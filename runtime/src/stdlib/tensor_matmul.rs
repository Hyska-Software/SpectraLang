use super::*;
pub(crate) fn tensor_float_unary(
    ctx: *mut SpectraHostCallContext,
    autograd_op: AutogradOp,
    op: impl Fn(f64) -> f64,
) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((shape, data, requires_grad, creator, device, precision, residency)) =
            with_tensor_registry(|registry| {
                let tensor = registry.get(args[0] as usize)?.clone();
                let element_count = tensor.len();
                let source = tensor.materialize();
                #[cfg(feature = "gpu")]
                let gpu_data = if tensor.device == TensorDevice::Wgpu
                    && tensor.dtype == TensorDType::Float
                {
                    let gpu_op = match autograd_op {
                        AutogradOp::Exp => Some(crate::gpu::GpuUnaryOp::Exp),
                        AutogradOp::Log => Some(crate::gpu::GpuUnaryOp::Log),
                        AutogradOp::Sqrt => Some(crate::gpu::GpuUnaryOp::Sqrt),
                        AutogradOp::Sigmoid => Some(crate::gpu::GpuUnaryOp::Sigmoid),
                        AutogradOp::Tanh => Some(crate::gpu::GpuUnaryOp::Tanh),
                        _ => None,
                    };
                    match gpu_op.map(|gpu_op| gpu_unary_float(&tensor, gpu_op)) {
                        Some(Ok(Some(data))) => {
                            registry.note_gpu_kernel();
                            Some(data)
                        }
                        Some(Ok(None)) | None => None,
                        Some(Err(err)) => {
                            registry.note_gpu_error(err.kind);
                            registry.note_cpu_fallback();
                            None
                        }
                    }
                } else {
                    None
                };
                let data: Vec<SpectraHostValue> = match tensor.dtype {
                    #[cfg(feature = "gpu")]
                    TensorDType::Float if gpu_data.is_some() => gpu_data.unwrap(),
                    _ => source
                        .iter()
                        .map(|bits| {
                            let value = match tensor.dtype {
                                TensorDType::Int => *bits as f64,
                                TensorDType::Float => f64::from_bits(*bits as u64),
                            };
                            op(value).to_bits() as i64
                        })
                        .collect(),
                };
                let requires_grad = tensor_requires_autograd(registry, &[args[0] as usize]);
                let input = tensor_values_as_f64(&tensor);
                let creator = requires_grad.then(|| {
                    AutogradNode::unary(
                        autograd_op,
                        args[0] as usize,
                        tensor.shape.clone(),
                        input,
                        data.iter()
                            .map(|raw| f64::from_bits(*raw as u64))
                            .collect::<Vec<_>>(),
                    )
                });
                #[cfg(feature = "gpu")]
                let residency = if tensor.dtype == TensorDType::Float
                    && tensor.device == TensorDevice::Wgpu
                    && tensor
                        .device_storage
                        .contains_key(&crate::gpu::PoolDevice::Wgpu)
                {
                    if let Some(gpu_op) = match autograd_op {
                        AutogradOp::Exp => Some(crate::gpu::GpuUnaryOp::Exp),
                        AutogradOp::Log => Some(crate::gpu::GpuUnaryOp::Log),
                        AutogradOp::Sqrt => Some(crate::gpu::GpuUnaryOp::Sqrt),
                        AutogradOp::Sigmoid => Some(crate::gpu::GpuUnaryOp::Sigmoid),
                        AutogradOp::Tanh => Some(crate::gpu::GpuUnaryOp::Tanh),
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
                    TensorDType::Float,
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
        let _ = (device, precision, &residency);
        match tensor_alloc_autograd(TensorDType::Float, shape, data, requires_grad, creator) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_matmul(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((dtype, shape, data, requires_grad, creator, device, precision, residency)) =
            with_tensor_registry(|registry| {
                let a = registry.get(args[0] as usize)?.clone();
                let b = registry.get(args[1] as usize)?.clone();
                if a.shape.len() != 2 || b.shape.len() != 2 || a.dtype != b.dtype {
                    return None;
                }
                if a.device != b.device {
                    return None;
                }
                let (m, k) = (a.shape[0], a.shape[1]);
                let (bk, n) = (b.shape[0], b.shape[1]);
                if k != bk {
                    return None;
                }
                let element_count = m.saturating_mul(n).saturating_mul(k);
                let a_data = a.materialize();
                let b_data = b.materialize();
                let out = match a.dtype {
                    TensorDType::Int => {
                        if a.device.is_accelerator() {
                            return None;
                        }
                        kernel_matmul_i64(&a_data, &b_data, m, k, n)
                    }
                    TensorDType::Float => {
                        #[cfg(feature = "gpu")]
                        if a.device == TensorDevice::Wgpu {
                            let a_gpu = tensor_values_as_f32(&a)?;
                            let b_gpu = tensor_values_as_f32(&b)?;
                            match crate::gpu::matmul(&a_gpu, &b_gpu, m, k, n) {
                                Ok(values) => {
                                    registry.note_gpu_kernel();
                                    f32_values_to_host(&values)
                                }
                                Err(err) => {
                                    registry.note_gpu_error(err.kind);
                                    registry.note_cpu_fallback();
                                    kernel_matmul_f64_bits(&a_data, &b_data, m, k, n)
                                }
                            }
                        } else {
                            kernel_matmul_f64_bits(&a_data, &b_data, m, k, n)
                        }
                        #[cfg(not(feature = "gpu"))]
                        {
                            if a.device.is_accelerator() {
                                return None;
                            }
                            kernel_matmul_f64_bits(&a_data, &b_data, m, k, n)
                        }
                    }
                };
                let requires_grad = a.dtype == TensorDType::Float
                    && tensor_requires_autograd(registry, &[args[0] as usize, args[1] as usize]);
                let creator = requires_grad.then(|| AutogradNode {
                    op: AutogradOp::Matmul,
                    parents: vec![args[0] as usize, args[1] as usize],
                    input_shape: Vec::new(),
                    left_shape: a.shape.clone(),
                    right_shape: b.shape.clone(),
                    input: Vec::new(),
                    output: out.iter().map(|raw| f64::from_bits(*raw as u64)).collect(),
                    left: a_data
                        .iter()
                        .map(|raw| f64::from_bits(*raw as u64))
                        .collect(),
                    right: b_data
                        .iter()
                        .map(|raw| f64::from_bits(*raw as u64))
                        .collect(),
                    aux: Vec::new(),
                    #[cfg(feature = "gpu")]
                    device_aux: None,
                });
                #[cfg(feature = "gpu")]
                let residency = if a.dtype == TensorDType::Float
                    && a.device == TensorDevice::Wgpu
                    && tensor_residency_pair(&a, &b)
                {
                    match tensor_residency_matmul(registry, &a, &b, m, k, n) {
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
                let result = Some((
                    a.dtype,
                    vec![m, n],
                    out,
                    requires_grad,
                    creator,
                    a.device,
                    a.precision,
                    #[cfg(feature = "gpu")]
                    residency,
                    #[cfg(not(feature = "gpu"))]
                    residency,
                ));
                registry.note_scratch_reuse();
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

pub(crate) extern "C" fn std_tensor_matmul_batched(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((dtype, shape, data, requires_grad, creator)) = with_tensor_registry(|registry| {
            let a = registry.get(args[0] as usize)?;
            let b = registry.get(args[1] as usize)?;
            if a.shape.len() != 3 || b.shape.len() != 3 || a.dtype != b.dtype {
                return None;
            }
            let (batch, m, k) = (a.shape[0], a.shape[1], a.shape[2]);
            let (bbatch, bk, n) = (b.shape[0], b.shape[1], b.shape[2]);
            if batch != bbatch || k != bk {
                return None;
            }
            let dtype = a.dtype;
            let a_data = a.materialize();
            let b_data = b.materialize();
            let mut out = Vec::with_capacity(batch * m * n);
            for batch_index in 0..batch {
                let a_start = batch_index * m * k;
                let b_start = batch_index * k * n;
                let batch_out = match dtype {
                    TensorDType::Int => kernel_matmul_i64(
                        &a_data[a_start..a_start + m * k],
                        &b_data[b_start..b_start + k * n],
                        m,
                        k,
                        n,
                    ),
                    TensorDType::Float => kernel_matmul_f64_bits(
                        &a_data[a_start..a_start + m * k],
                        &b_data[b_start..b_start + k * n],
                        m,
                        k,
                        n,
                    ),
                };
                out.extend(batch_out);
            }
            registry.note_scratch_reuse();
            registry.note_kernel(batch.saturating_mul(m).saturating_mul(n).saturating_mul(k));
            let requires_grad = dtype == TensorDType::Float
                && tensor_requires_autograd(
                    registry,
                    &[args[0] as usize, args[1] as usize],
                );
            let creator = requires_grad.then(|| AutogradNode {
                op: AutogradOp::BatchedMatmul,
                parents: vec![args[0] as usize, args[1] as usize],
                input_shape: vec![batch, m, n],
                left_shape: vec![batch, m, k],
                right_shape: vec![batch, k, n],
                input: Vec::new(),
                output: Vec::new(),
                left: a_data
                    .iter()
                    .map(|raw| f64::from_bits(*raw as u64))
                    .collect(),
                right: b_data
                    .iter()
                    .map(|raw| f64::from_bits(*raw as u64))
                    .collect(),
                aux: Vec::new(),
                #[cfg(feature = "gpu")]
                device_aux: None,
            });
            Some((dtype, vec![batch, m, n], out, requires_grad, creator))
        }) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match tensor_alloc_autograd(dtype, shape, data, requires_grad, creator) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

/// Dense f64 matmul over row-major slices. The inner product takes the
/// runtime-dispatched SIMD dot kernel (`dot_f64`) over contiguous lanes
/// after one transpose of the right operand. SIMD lane accumulation
/// reorders summation relative to a strictly sequential loop; results agree
/// within floating-point tolerance (~1e-12 relative for well-scaled
/// inputs), matching the tolerance policy documented beside the kernels in
/// `tensor_helpers_kernels.rs`.
pub(crate) fn matmul_f64(left: &[f64], right: &[f64], m: usize, k: usize, n: usize) -> Vec<f64> {
    let mut out = vec![0.0; m * n];
    let mut row_buf = vec![0.0f64; k];
    let right_t = transpose_f64(right, k, n);
    for row in 0..m {
        row_buf.copy_from_slice(&left[row * k..row * k + k]);
        for col in 0..n {
            out[row * n + col] = kernel_dot_f64(&row_buf, &right_t[col * k..col * k + k]);
        }
    }
    out
}

pub(crate) fn transpose_f64(data: &[f64], rows: usize, cols: usize) -> Vec<f64> {
    let mut out = vec![0.0; data.len()];
    for row in 0..rows {
        for col in 0..cols {
            out[col * rows + row] = data[row * cols + col];
        }
    }
    out
}

pub(crate) fn accumulate_tensor_grad(tensor: &mut StdTensor, grad: &[f64]) -> bool {
    if tensor.dtype != TensorDType::Float || tensor.len() != grad.len() {
        return false;
    }
    let target = tensor.grad.get_or_insert_with(|| vec![0.0; grad.len()]);
    for (slot, value) in target.iter_mut().zip(grad.iter()) {
        *slot += *value;
    }
    true
}

