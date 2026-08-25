use super::*;
pub(crate) fn with_tensor_registry<F, R>(action: F) -> R
where
    F: FnOnce(&mut TensorRegistry) -> R,
{
    let mut guard = lock_unpoisoned(tensor_registry());
    action(&mut guard)
}

pub(crate) fn tensor_strides(shape: &[usize]) -> Vec<usize> {
    let mut strides = vec![1; shape.len()];
    if shape.len() > 1 {
        for idx in (0..shape.len() - 1).rev() {
            strides[idx] = strides[idx + 1] * shape[idx + 1];
        }
    }
    strides
}

pub(crate) fn tensor_is_grad_enabled() -> bool {
    *lock_unpoisoned(tensor_grad_enabled())
}

pub(crate) fn tensor_values_as_f64(tensor: &StdTensor) -> Vec<f64> {
    tensor
        .materialize()
        .iter()
        .map(|raw| match tensor.dtype {
            TensorDType::Int => *raw as f64,
            TensorDType::Float => f64::from_bits(*raw as u64),
        })
        .collect()
}

pub(crate) fn f64_values_to_host(values: &[f64]) -> Vec<SpectraHostValue> {
    values
        .iter()
        .map(|value| value.to_bits() as SpectraHostValue)
        .collect()
}

pub(crate) fn json_escape(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(feature = "gpu")]
pub(crate) fn tensor_values_as_f32(tensor: &StdTensor) -> Option<Vec<f32>> {
    if tensor.dtype != TensorDType::Float {
        return None;
    }
    Some(
        tensor
            .materialize()
            .iter()
            .map(|raw| f64::from_bits(*raw as u64) as f32)
            .collect(),
    )
}

#[cfg(feature = "gpu")]
pub(crate) fn f32_values_to_host(values: &[f32]) -> Vec<SpectraHostValue> {
    values
        .iter()
        .map(|value| (*value as f64).to_bits() as SpectraHostValue)
        .collect()
}

#[cfg(feature = "gpu")]
pub(crate) fn gpu_binary_float(
    left: &StdTensor,
    right: &StdTensor,
    op: crate::gpu::GpuBinaryOp,
) -> Result<Option<Vec<SpectraHostValue>>, crate::gpu::GpuError> {
    if left.device != TensorDevice::Wgpu
        || right.device != TensorDevice::Wgpu
        || left.dtype != TensorDType::Float
        || right.dtype != TensorDType::Float
    {
        return Ok(None);
    }
    let Some(left_data) = tensor_values_as_f32(left) else {
        return Ok(None);
    };
    let Some(right_data) = tensor_values_as_f32(right) else {
        return Ok(None);
    };
    match crate::gpu::binary(&left_data, &right_data, op) {
        Ok(values) => Ok(Some(f32_values_to_host(&values))),
        Err(err) => Err(err),
    }
}

#[cfg(feature = "gpu")]
pub(crate) fn gpu_unary_float(
    tensor: &StdTensor,
    op: crate::gpu::GpuUnaryOp,
) -> Result<Option<Vec<SpectraHostValue>>, crate::gpu::GpuError> {
    if tensor.device != TensorDevice::Wgpu || tensor.dtype != TensorDType::Float {
        return Ok(None);
    }
    let Some(data) = tensor_values_as_f32(tensor) else {
        return Ok(None);
    };
    match crate::gpu::unary(&data, op) {
        Ok(values) => Ok(Some(f32_values_to_host(&values))),
        Err(err) => Err(err),
    }
}

/// R-3052 full: return `true` when both tensors have a Wgpu pool buffer
/// on `device_storage` — the precondition for the residency-aware
/// dispatch path.
#[cfg(feature = "gpu")]
pub(crate) fn tensor_residency_pair(left: &StdTensor, right: &StdTensor) -> bool {
    left.device_storage
        .contains_key(&crate::gpu::PoolDevice::Wgpu)
        && right
            .device_storage
            .contains_key(&crate::gpu::PoolDevice::Wgpu)
}

/// R-3052 full: outcome of a residency-aware op. `Ok(buf)` means the
/// caller should store `buf` on the new tensor's `device_storage`;
/// `CpuFallback` means the caller should fall back to the existing
/// host-materializing path; `Error` means the GPU path failed and the
/// caller should record the error and fall back.
#[cfg(feature = "gpu")]
pub(crate) enum ResidencyOutcome {
    Ok(crate::gpu::DeviceBuffer),
    CpuFallback,
    Error(crate::gpu::GpuError),
}

/// R-3052 full: residency-aware binary. Acquires an output pool buffer
/// inside the same `with_device_queue` closure used by `to_device` so
/// queue ordering is preserved across the chain.
#[cfg(feature = "gpu")]
pub(crate) fn tensor_residency_binary(
    registry: &mut TensorRegistry,
    left: &StdTensor,
    right: &StdTensor,
    op: crate::gpu::GpuBinaryOp,
    element_count: usize,
) -> ResidencyOutcome {
    let left_buf = match left.device_storage.get(&crate::gpu::PoolDevice::Wgpu) {
        Some(buf) => buf.clone(),
        None => return ResidencyOutcome::CpuFallback,
    };
    let right_buf = match right.device_storage.get(&crate::gpu::PoolDevice::Wgpu) {
        Some(buf) => buf.clone(),
        None => return ResidencyOutcome::CpuFallback,
    };
    match crate::gpu::with_device_queue(|device, queue| {
        let out_buf = registry.device_arena.acquire(
            crate::gpu::PoolDevice::Wgpu,
            crate::gpu::PoolDType::Float,
            element_count,
            device,
        );
        let result = crate::gpu::binary_device(&left_buf, &right_buf, op, &out_buf, device, queue);
        (result, out_buf)
    }) {
        Ok((Ok(()), buf)) => {
            registry.note_gpu_kernel();
            ResidencyOutcome::Ok(buf)
        }
        Ok((Err(err), buf)) => {
            registry.device_arena.release(buf);
            ResidencyOutcome::Error(err)
        }
        Err(err) => ResidencyOutcome::Error(err),
    }
}

/// R-3052 full: residency-aware unary. Same pattern as binary.
#[cfg(feature = "gpu")]
pub(crate) fn tensor_residency_unary(
    registry: &mut TensorRegistry,
    tensor: &StdTensor,
    op: crate::gpu::GpuUnaryOp,
    element_count: usize,
) -> ResidencyOutcome {
    let in_buf = match tensor.device_storage.get(&crate::gpu::PoolDevice::Wgpu) {
        Some(buf) => buf.clone(),
        None => return ResidencyOutcome::CpuFallback,
    };
    match crate::gpu::with_device_queue(|device, queue| {
        let out_buf = registry.device_arena.acquire(
            crate::gpu::PoolDevice::Wgpu,
            crate::gpu::PoolDType::Float,
            element_count,
            device,
        );
        let result = crate::gpu::unary_device(&in_buf, op, &out_buf, device, queue);
        (result, out_buf)
    }) {
        Ok((Ok(()), buf)) => {
            registry.note_gpu_kernel();
            ResidencyOutcome::Ok(buf)
        }
        Ok((Err(err), buf)) => {
            registry.device_arena.release(buf);
            ResidencyOutcome::Error(err)
        }
        Err(err) => ResidencyOutcome::Error(err),
    }
}

/// R-3052 full: residency-aware matmul. Output is a fresh pool buffer.
#[cfg(feature = "gpu")]
pub(crate) fn tensor_residency_matmul(
    registry: &mut TensorRegistry,
    left: &StdTensor,
    right: &StdTensor,
    m: usize,
    k: usize,
    n: usize,
) -> ResidencyOutcome {
    let left_buf = match left.device_storage.get(&crate::gpu::PoolDevice::Wgpu) {
        Some(buf) => buf.clone(),
        None => return ResidencyOutcome::CpuFallback,
    };
    let right_buf = match right.device_storage.get(&crate::gpu::PoolDevice::Wgpu) {
        Some(buf) => buf.clone(),
        None => return ResidencyOutcome::CpuFallback,
    };
    let out_elements = m * n;
    match crate::gpu::with_device_queue(|device, queue| {
        let out_buf = registry.device_arena.acquire(
            crate::gpu::PoolDevice::Wgpu,
            crate::gpu::PoolDType::Float,
            out_elements,
            device,
        );
        let result =
            crate::gpu::matmul_device(&left_buf, &right_buf, m, k, n, &out_buf, device, queue);
        (result, out_buf)
    }) {
        Ok((Ok(()), buf)) => {
            registry.note_gpu_kernel();
            ResidencyOutcome::Ok(buf)
        }
        Ok((Err(err), buf)) => {
            registry.device_arena.release(buf);
            ResidencyOutcome::Error(err)
        }
        Err(err) => ResidencyOutcome::Error(err),
    }
}

/// R-3052 full: residency-aware conv2d. Output is a fresh pool buffer.
#[cfg(feature = "gpu")]
pub(crate) fn tensor_residency_conv2d(
    registry: &mut TensorRegistry,
    input: &StdTensor,
    kernel: &StdTensor,
    bias: &StdTensor,
    dims: [usize; 7],
) -> ResidencyOutcome {
    let input_buf = match input.device_storage.get(&crate::gpu::PoolDevice::Wgpu) {
        Some(buf) => buf.clone(),
        None => return ResidencyOutcome::CpuFallback,
    };
    let kernel_buf = match kernel.device_storage.get(&crate::gpu::PoolDevice::Wgpu) {
        Some(buf) => buf.clone(),
        None => return ResidencyOutcome::CpuFallback,
    };
    let bias_buf = match bias.device_storage.get(&crate::gpu::PoolDevice::Wgpu) {
        Some(buf) => buf.clone(),
        None => return ResidencyOutcome::CpuFallback,
    };
    let [batch, _in_ch, h, w, out_ch, kh, kw] = dims;
    let out_h = h - kh + 1;
    let out_w = w - kw + 1;
    let out_elements = batch * out_ch * out_h * out_w;
    match crate::gpu::with_device_queue(|device, queue| {
        let out_buf = registry.device_arena.acquire(
            crate::gpu::PoolDevice::Wgpu,
            crate::gpu::PoolDType::Float,
            out_elements,
            device,
        );
        let result = crate::gpu::conv2d_device(
            &input_buf,
            &kernel_buf,
            &bias_buf,
            dims,
            &out_buf,
            device,
            queue,
        );
        (result, out_buf)
    }) {
        Ok((Ok(()), buf)) => {
            registry.note_gpu_kernel();
            ResidencyOutcome::Ok(buf)
        }
        Ok((Err(err), buf)) => {
            registry.device_arena.release(buf);
            ResidencyOutcome::Error(err)
        }
        Err(err) => ResidencyOutcome::Error(err),
    }
}

/// R-3052 full: residency-aware sum reduction. Writes 1 f32 into a
/// 1-element pool buffer; the caller reads it back as the scalar loss
/// (the only allowed readback in the hot path).
#[cfg(feature = "gpu")]
pub(crate) fn tensor_residency_sum(registry: &mut TensorRegistry, tensor: &StdTensor) -> Option<f32> {
    let in_buf = tensor
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
        let sum_result = crate::gpu::sum_device(&in_buf, &out_buf, device, queue);
        let value_result = match &sum_result {
            Ok(()) => crate::gpu::readback_scalar_device(&out_buf, device, queue),
            Err(e) => Err(crate::gpu::GpuError {
                kind: e.kind,
                message: e.message.clone(),
            }),
        };
        (sum_result, value_result, out_buf)
    });
    match outcome {
        Ok((Ok(()), Ok(value), buf)) => {
            registry.note_gpu_kernel();
            registry.device_arena.release(buf);
            Some(value)
        }
        Ok((sum_res, val_res, buf)) => {
            let err = sum_res.err().or(val_res.err()).unwrap_or_else(|| {
                crate::gpu::GpuError::new(
                    crate::gpu::GpuErrorKind::Other,
                    "sum residency failed without error",
                )
            });
            registry.device_arena.release(buf);
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
}

/// R-3052 full: residency-aware `ml.linear` forward. Runs a device
/// matmul followed by a device bias-add (in-place on the matmul
/// output buffer) and returns the resulting buffer.
#[cfg(feature = "gpu")]
pub(crate) fn tensor_residency_ml_linear(
    registry: &mut TensorRegistry,
    input: &StdTensor,
    weight: &StdTensor,
    bias: &StdTensor,
    batch: usize,
    in_features: usize,
    out_features: usize,
) -> ResidencyOutcome {
    let matmul_out =
        match tensor_residency_matmul(registry, input, weight, batch, in_features, out_features) {
            ResidencyOutcome::Ok(buf) => buf,
            other => return other,
        };
    let bias_buf = match bias.device_storage.get(&crate::gpu::PoolDevice::Wgpu) {
        Some(buf) => buf.clone(),
        None => {
            registry.device_arena.release(matmul_out);
            return ResidencyOutcome::CpuFallback;
        }
    };
    match crate::gpu::with_device_queue(|device, queue| {
        let result = crate::gpu::add_bias_device(&matmul_out, &bias_buf, device, queue);
        (result, ())
    }) {
        Ok((Ok(()), _)) => {
            registry.note_gpu_kernel();
            ResidencyOutcome::Ok(matmul_out)
        }
        Ok((Err(err), _)) => {
            registry.device_arena.release(matmul_out);
            ResidencyOutcome::Error(err)
        }
        Err(err) => {
            registry.device_arena.release(matmul_out);
            ResidencyOutcome::Error(err)
        }
    }
}

pub(crate) fn tensor_requires_autograd(registry: &TensorRegistry, parents: &[usize]) -> bool {
    if !tensor_is_grad_enabled() {
        return false;
    }
    parents.iter().any(|handle| {
        registry
            .get(*handle)
            .map(|tensor| tensor.requires_grad && tensor.dtype == TensorDType::Float)
            .unwrap_or(false)
    })
}

pub(crate) fn max_tensor_offset(shape: &[usize], strides: &[usize], base_offset: usize) -> Option<usize> {
    let mut max_offset = base_offset;
    for (dim, stride) in shape.iter().zip(strides.iter()) {
        max_offset = max_offset.checked_add(dim.saturating_sub(1).checked_mul(*stride)?)?;
    }
    Some(max_offset)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TensorKernelStrategy {
    Scalar = 1,
    #[allow(dead_code)]
    Avx2 = 2,
    #[allow(dead_code)]
    Neon = 3,
    #[allow(dead_code)]
    Blas = 4,
    #[allow(dead_code)]
    Gpu = 5,
}

impl TensorKernelStrategy {
    pub(crate) fn current() -> Self {
        #[cfg(feature = "blas")]
        {
            return Self::Blas;
        }
        #[cfg(feature = "gpu")]
        {
            if crate::gpu::is_available() {
                return Self::Gpu;
            }
        }
        Self::Scalar
    }

    pub(crate) fn code(self) -> SpectraHostValue {
        self as SpectraHostValue
    }
}

pub(crate) fn kernel_dot_i64(left: &[SpectraHostValue], right: &[SpectraHostValue]) -> SpectraHostValue {
    debug_assert_eq!(left.len(), right.len());
    let len = left.len();
    let mut acc0 = 0i64;
    let mut acc1 = 0i64;
    let mut acc2 = 0i64;
    let mut acc3 = 0i64;
    let mut idx = 0usize;
    unsafe {
        let left_ptr = left.as_ptr();
        let right_ptr = right.as_ptr();
        while idx + 4 <= len {
            acc0 += *left_ptr.add(idx) * *right_ptr.add(idx);
            acc1 += *left_ptr.add(idx + 1) * *right_ptr.add(idx + 1);
            acc2 += *left_ptr.add(idx + 2) * *right_ptr.add(idx + 2);
            acc3 += *left_ptr.add(idx + 3) * *right_ptr.add(idx + 3);
            idx += 4;
        }
        let mut acc = acc0 + acc1 + acc2 + acc3;
        while idx < len {
            acc += *left_ptr.add(idx) * *right_ptr.add(idx);
            idx += 1;
        }
        acc
    }
}

pub(crate) fn kernel_dot_f64_bits(left: &[SpectraHostValue], right: &[SpectraHostValue]) -> SpectraHostValue {
    let mut acc0 = 0.0f64;
    let mut acc1 = 0.0f64;
    let mut acc2 = 0.0f64;
    let mut acc3 = 0.0f64;
    let chunks = left.chunks_exact(4);
    let remainder = chunks.remainder();
    for (a, b) in chunks.zip(right.chunks_exact(4)) {
        acc0 += f64::from_bits(a[0] as u64) * f64::from_bits(b[0] as u64);
        acc1 += f64::from_bits(a[1] as u64) * f64::from_bits(b[1] as u64);
        acc2 += f64::from_bits(a[2] as u64) * f64::from_bits(b[2] as u64);
        acc3 += f64::from_bits(a[3] as u64) * f64::from_bits(b[3] as u64);
    }
    let mut acc = acc0 + acc1 + acc2 + acc3;
    let offset = left.len() - remainder.len();
    for idx in 0..remainder.len() {
        acc +=
            f64::from_bits(left[offset + idx] as u64) * f64::from_bits(right[offset + idx] as u64);
    }
    acc.to_bits() as i64
}

pub(crate) fn kernel_transpose_i64(
    data: &[SpectraHostValue],
    rows: usize,
    cols: usize,
) -> Vec<SpectraHostValue> {
    let mut out = vec![0; data.len()];
    for row in 0..rows {
        for col in 0..cols {
            out[col * rows + row] = data[row * cols + col];
        }
    }
    out
}

pub(crate) fn kernel_matmul_i64(
    left: &[SpectraHostValue],
    right: &[SpectraHostValue],
    m: usize,
    k: usize,
    n: usize,
) -> Vec<SpectraHostValue> {
    let right_t = kernel_transpose_i64(right, k, n);
    let mut out = vec![0; m * n];
    for row in 0..m {
        let lhs = &left[row * k..row * k + k];
        for col in 0..n {
            let rhs = &right_t[col * k..col * k + k];
            out[row * n + col] = kernel_dot_i64(lhs, rhs);
        }
    }
    out
}

pub(crate) fn kernel_matmul_f64_bits(
    left: &[SpectraHostValue],
    right: &[SpectraHostValue],
    m: usize,
    k: usize,
    n: usize,
) -> Vec<SpectraHostValue> {
    let right_t = kernel_transpose_i64(right, k, n);
    let mut out = vec![0; m * n];
    for row in 0..m {
        let lhs = &left[row * k..row * k + k];
        for col in 0..n {
            let rhs = &right_t[col * k..col * k + k];
            out[row * n + col] = kernel_dot_f64_bits(lhs, rhs);
        }
    }
    out
}

#[doc(hidden)]
pub fn tensor_bench_kernel_dot_i64(left: &[i64], right: &[i64]) -> i64 {
    kernel_dot_i64(left, right)
}

#[doc(hidden)]
pub fn tensor_bench_kernel_matmul_i64(
    left: &[i64],
    right: &[i64],
    m: usize,
    k: usize,
    n: usize,
) -> Vec<i64> {
    kernel_matmul_i64(left, right, m, k, n)
}
