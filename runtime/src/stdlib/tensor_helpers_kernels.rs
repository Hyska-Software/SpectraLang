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

/// R-3052 full: residency-aware scalar reduction (`sum`, `mean`, `min`,
/// `max`, `argmax`). Writes 1 f32 into a 1-element pool buffer and reads
/// it back as the scalar result (the only allowed readback in the hot
/// path). For `ArgMax` the readback f32 carries the winning index
/// bit-cast, so callers convert with `value.to_bits()` instead of using
/// the numeric value.
#[cfg(feature = "gpu")]
pub(crate) fn tensor_residency_reduce(
    registry: &mut TensorRegistry,
    tensor: &StdTensor,
    op: crate::gpu::GpuReduceOp,
) -> Option<f32> {
    let in_buf = tensor
        .device_storage
        .get(&crate::gpu::PoolDevice::Wgpu)?
        .clone();
    let run = match op {
        crate::gpu::GpuReduceOp::Sum => crate::gpu::sum_device,
        crate::gpu::GpuReduceOp::Mean => crate::gpu::mean_device,
        crate::gpu::GpuReduceOp::Min => crate::gpu::min_device,
        crate::gpu::GpuReduceOp::Max => crate::gpu::max_device,
        crate::gpu::GpuReduceOp::ArgMax => crate::gpu::argmax_device,
    };
    let outcome = crate::gpu::with_device_queue(|device, queue| {
        let out_buf = registry.device_arena.acquire(
            crate::gpu::PoolDevice::Wgpu,
            crate::gpu::PoolDType::Float,
            1,
            device,
        );
        let reduce_result = run(&in_buf, &out_buf, device, queue);
        let value_result = match &reduce_result {
            Ok(()) => crate::gpu::readback_scalar_device(&out_buf, device, queue),
            Err(e) => Err(crate::gpu::GpuError {
                kind: e.kind,
                message: e.message.clone(),
            }),
        };
        (reduce_result, value_result, out_buf)
    });
    match outcome {
        Ok((Ok(()), Ok(value), buf)) => {
            registry.note_gpu_kernel();
            registry.device_arena.release(buf);
            Some(value)
        }
        Ok((reduce_res, val_res, buf)) => {
            let err = reduce_res.err().or(val_res.err()).unwrap_or_else(|| {
                crate::gpu::GpuError::new(
                    crate::gpu::GpuErrorKind::Other,
                    "residency reduce failed without error",
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

/// R-3052 full: guarded scalar-reduction fast path shared by
/// `sum`/`mean`/`min`/`max`/`argmax`. Returns `None` (without touching
/// the GPU) unless the tensor is a non-empty device-resident Wgpu f32
/// float tensor; otherwise runs the residency reduce and converts the
/// readback f32 with `convert` (numeric ops decode the value; ArgMax
/// decodes the bit-cast index).
#[cfg(feature = "gpu")]
pub(crate) fn tensor_residency_scalar(
    registry: &mut TensorRegistry,
    tensor: &StdTensor,
    op: crate::gpu::GpuReduceOp,
    convert: impl Fn(f32) -> SpectraHostValue,
) -> Option<SpectraHostValue> {
    if tensor.dtype != TensorDType::Float
        || tensor.device != TensorDevice::Wgpu
        || tensor.len() == 0
        || !tensor
            .device_storage
            .contains_key(&crate::gpu::PoolDevice::Wgpu)
    {
        return None;
    }
    tensor_residency_reduce(registry, tensor, op).map(convert)
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

pub(crate) fn max_tensor_offset(
    shape: &[usize],
    strides: &[usize],
    base_offset: usize,
) -> Option<usize> {
    let mut max_offset = base_offset;
    for (dim, stride) in shape.iter().zip(strides.iter()) {
        max_offset = max_offset.checked_add(dim.saturating_sub(1).checked_mul(*stride)?)?;
    }
    Some(max_offset)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TensorKernelStrategy {
    Scalar = 1,
    Avx2 = 2,
    Neon = 3,
    // Discriminant 4 is retired: it belonged to a fake "Blas" strategy
    // that reported availability merely because a Cargo feature flag
    // existed, although no BLAS call site ever existed in this crate.
    // The numeric gap is kept so strategy codes stay stable.
    Gpu = 5,
}

impl TensorKernelStrategy {
    /// Every strategy variant this build recognizes, including paths that
    /// cfg may disable at a given build (e.g. `Gpu` without the `gpu`
    /// feature, `Neon` on non-arm64 hosts). Enumerating all variants here
    /// keeps each one genuinely constructed instead of decorated with
    /// `#[allow(dead_code)]`.
    pub(crate) const ALL: [Self; 4] = [Self::Scalar, Self::Avx2, Self::Neon, Self::Gpu];

    /// Strategy actually selected for upcoming tensor kernel dispatch.
    /// Reports a device GPU only when one is available through a real GPU
    /// call site, and otherwise reports the widest SIMD path that the f64
    /// CPU kernels below will truly take on this machine.
    pub(crate) fn current() -> Self {
        #[cfg(feature = "gpu")]
        {
            if crate::gpu::is_available() {
                return Self::Gpu;
            }
        }
        Self::cpu()
    }

    /// Widest CPU SIMD capability used by the real `std::arch` kernels
    /// below; exactly the strategy those kernels will actually take.
    /// Exactly one cfg arm below survives per target architecture and acts
    /// as the tail expression.
    pub(crate) fn cpu() -> Self {
        #[cfg(target_arch = "aarch64")]
        {
            // NEON fp64 arithmetic ships on every arm64 target, so no
            // runtime probe is needed.
            Self::Neon
        }
        #[cfg(target_arch = "x86_64")]
        {
            if std::arch::is_x86_feature_detected!("avx2") {
                Self::Avx2
            } else {
                // SSE-only machines run 128-bit SIMD kernels below but are
                // reported as Scalar because the stable strategy contract
                // has no dedicated SSE discriminant; AVX2 is the claimed
                // widened tier on x86_64.
                Self::Scalar
            }
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            Self::Scalar
        }
    }

    pub(crate) fn code(self) -> SpectraHostValue {
        debug_assert!(Self::ALL.contains(&self));
        self as SpectraHostValue
    }
}

pub(crate) fn kernel_dot_i64(
    left: &[SpectraHostValue],
    right: &[SpectraHostValue],
) -> SpectraHostValue {
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

use core::ops::{Add, Div, Mul, Sub};

// ---------------------------------------------------------------------------
// Real SIMD kernels: stable `std::arch` intrinsics only, no dependencies.
//
// Semantics contract:
// * Elementwise add/sub/mul/div are per-element IEEE-754 operations, so the
//   SIMD paths are **bit-identical** to the scalar reference — no
//   reassociation is possible.
// * Dot products accumulate in independent SIMD lanes, which reorders
//   summation versus strictly sequential scalar accumulation. Results agree
//   with the sequential reference within floating-point tolerance (~1e-12
//   relative for well-scaled inputs), matching the tolerances the tensor
//   tests already apply. The bit-exact sequential path remains available as
//   `kernel_dot_f64_bits_scalar_exact` for exactness checks.
// ---------------------------------------------------------------------------

/// Per-element arithmetic kernel selector shared by every dispatch path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ElementwiseOp {
    Add,
    Sub,
    Mul,
    Div,
}

impl ElementwiseOp {
    /// Per-element reference application; generic over float width so the
    /// scalar tails in the f32/f64 kernels share one definition.
    #[inline]
    pub(crate) fn apply<T>(self, a: T, b: T) -> T
    where
        T: Add<Output = T> + Sub<Output = T> + Mul<Output = T> + Div<Output = T>,
    {
        match self {
            ElementwiseOp::Add => a + b,
            ElementwiseOp::Sub => a - b,
            ElementwiseOp::Mul => a * b,
            ElementwiseOp::Div => a / b,
        }
    }
}

/// Instantiate lane-wise elementwise and dot-product kernels for one float
/// width on one instruction set. Each invocation wraps its routines in a
/// uniquely named nested module (`=> $module_name`) so several widths can
/// coexist on a single ISA. `$zero` is a full expression because some zero
/// vectors (NEON) are constructed with an argument rather than a nullary
/// intrinsic.
///
/// Every generated routine is `unsafe` with an explicit availability
/// contract: the caller MUST have established the corresponding ISA support
/// first (runtime detection via `std::arch::is_x86_feature_detected!` on
/// x86_64; the arm64 baseline guarantees NEON unconditionally).
macro_rules! simd_lane_kernels {
    (
        $(#[$fattr:meta])*
        $elem:ty, $vec:ty, $zero:expr, $loadu:ident, $storeu:ident,
        $add:ident, $sub:ident, $mul:ident, $div:ident, $lanes:literal => $module_name:ident
    ) => {
        pub(crate) mod $module_name {
            use super::*;

            /// Writes `left <op> right` into `out`. The scalar tail handles
            /// the remainder below one vector width; the elementwise result
            /// is bit-identical to the sequential scalar reference.
            ///
            /// # Safety
            /// Availability contract in the defining macro documentation.
            $(#[$fattr])*
            pub(crate) unsafe fn elementwise_into(
                left: &[$elem],
                right: &[$elem],
                out: &mut [$elem],
                op: ElementwiseOp,
            ) {
                debug_assert_eq!(left.len(), right.len());
                let len = left.len();
                let l = left.as_ptr();
                let r = right.as_ptr();
                let o = out.as_mut_ptr();
                let mut idx = 0usize;
                while idx + $lanes <= len {
                    // SAFETY: idx + lanes <= len bounds every access below.
                    unsafe {
                        let a = $loadu(l.add(idx));
                        let b = $loadu(r.add(idx));
                        let c = match op {
                            ElementwiseOp::Add => $add(a, b),
                            ElementwiseOp::Sub => $sub(a, b),
                            ElementwiseOp::Mul => $mul(a, b),
                            ElementwiseOp::Div => $div(a, b),
                        };
                        $storeu(o.add(idx), c);
                    }
                    idx += $lanes;
                }
                while idx < len {
                    out[idx] = op.apply(*left.get_unchecked(idx), *right.get_unchecked(idx));
                    idx += 1;
                }
            }

            /// Chunked lane accumulation with two independent accumulators
            /// for ILP; each lane keeps its own partial-sum chain. Summation
            /// order differs from strict scalar sequencing; see the
            /// tolerance policy in this file's header comment.
            ///
            /// # Safety
            /// Availability contract in the defining macro documentation.
            $(#[$fattr])*
            pub(crate) unsafe fn dot(left: &[$elem], right: &[$elem]) -> $elem {
                debug_assert_eq!(left.len(), right.len());
                let len = left.len();
                let l = left.as_ptr();
                let r = right.as_ptr();
                let mut acc0: $vec = $zero;
                let mut acc1: $vec = $zero;
                let pair_stride = $lanes * 2;
                let mut idx = 0usize;
                while idx + pair_stride <= len {
                    // SAFETY: idx + pair_stride <= len bounds all four loads.
                    unsafe {
                        acc0 = $add(acc0, $mul($loadu(l.add(idx)), $loadu(r.add(idx))));
                        acc1 = $add(
                            acc1,
                            $mul($loadu(l.add(idx + $lanes)), $loadu(r.add(idx + $lanes))),
                        );
                    }
                    idx += pair_stride;
                }
                if idx + $lanes <= len {
                    // SAFETY: checked immediately above.
                    unsafe {
                        acc0 = $add(acc0, $mul($loadu(l.add(idx)), $loadu(r.add(idx))));
                    }
                    idx += $lanes;
                }
                // Lane reduction: fold the accumulators per lane, spill the
                // combined vector to the stack, sum the surviving lanes
                // sequentially, then finish any scalar tail.
                let combined = $add(acc0, acc1);
                let mut lanes = [0 as $elem; $lanes];
                // SAFETY: `lanes` holds exactly one vector of storage.
                unsafe { $storeu(lanes.as_mut_ptr(), combined) };
                let mut total = 0 as $elem;
                for lane in lanes.iter() {
                    total += *lane;
                }
                while idx < len {
                    total += *left.get_unchecked(idx) * *right.get_unchecked(idx);
                    idx += 1;
                }
                total
            }
        }
    };
}

#[cfg(target_arch = "x86_64")]
mod simd_avx2 {
    use super::ElementwiseOp;
    use core::arch::x86_64::*;

    simd_lane_kernels!(
        #[target_feature(enable = "avx2")]
        f64, __m256d, _mm256_setzero_pd(),
        _mm256_loadu_pd, _mm256_storeu_pd, _mm256_add_pd, _mm256_sub_pd,
        _mm256_mul_pd, _mm256_div_pd, 4 => f64x4
    );

    // f32 tensors do not exist on the production host surface (the registry
    // exposes Int/Float=f64 only), so f32 lanes are exercised by the SIMD
    // regression suite alone.
    #[cfg(test)]
    simd_lane_kernels!(
        #[target_feature(enable = "avx2")]
        f32, __m256, _mm256_setzero_ps(),
        _mm256_loadu_ps, _mm256_storeu_ps, _mm256_add_ps, _mm256_sub_ps,
        _mm256_mul_ps, _mm256_div_ps, 8 => f32x8
    );
}

#[cfg(target_arch = "x86_64")]
mod simd_sse {
    use super::ElementwiseOp;
    use core::arch::x86_64::*;

    simd_lane_kernels!(
        #[target_feature(enable = "sse4.2")]
        f64, __m128d, _mm_setzero_pd(),
        _mm_loadu_pd, _mm_storeu_pd, _mm_add_pd, _mm_sub_pd,
        _mm_mul_pd, _mm_div_pd, 2 => f64x2
    );

    // f32 lanes are exercised by the SIMD regression suite alone; see the
    // AVX2 block note.
    #[cfg(test)]
    simd_lane_kernels!(
        #[target_feature(enable = "sse4.2")]
        f32, __m128, _mm_setzero_ps(),
        _mm_loadu_ps, _mm_storeu_ps, _mm_add_ps, _mm_sub_ps,
        _mm_mul_ps, _mm_div_ps, 4 => f32x4
    );
}

#[cfg(target_arch = "aarch64")]
mod simd_neon {
    use super::ElementwiseOp;
    use core::arch::aarch64::*;

    simd_lane_kernels!(
        f64, float64x2_t, vdupq_n_f64(0.0),
        vld1q_f64, vst1q_f64, vaddq_f64, vsubq_f64,
        vmulq_f64, vdivq_f64, 2 => f64x2
    );

    // f32 lanes are exercised by the SIMD regression suite alone.
    #[cfg(test)]
    simd_lane_kernels!(
        f32, float32x4_t, vdupq_n_f32(0.0),
        vld1q_f32, vst1q_f32, vaddq_f32, vsubq_f32,
        vmulq_f32, vdivq_f32, 4 => f32x4
    );
}

/// Views a host-value slice as raw `f64` bits. Host float tensors store
/// IEEE-754 bit patterns in i64 slots, so reinterpretation is lossless.
fn widen_float_slices(values: &[SpectraHostValue]) -> &[f64] {
    const _: () = assert!(std::mem::size_of::<SpectraHostValue>() == std::mem::size_of::<f64>());
    const _: () = assert!(std::mem::align_of::<SpectraHostValue>() == std::mem::align_of::<f64>());
    // SAFETY: identical size and alignment asserted above; the returned
    // view borrows the same bytes for the lifetime of `values`.
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast::<f64>(), values.len()) }
}

/// Runtime-dispatching elementwise kernel for f64 lanes. Falls back to the
/// strict sequential loop on ISAs without a compiled SIMD path; otherwise
/// takes the widest vector route this build supports on this machine.
pub(crate) fn elementwise_f64(left: &[f64], right: &[f64], out: &mut [f64], op: ElementwiseOp) {
    debug_assert_eq!(left.len(), right.len());
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: avx2 availability verified on the preceding line.
            unsafe { simd_avx2::f64x4::elementwise_into(left, right, out, op) };
            return;
        }
        if std::arch::is_x86_feature_detected!("sse4.2") {
            // SAFETY: sse4.2 availability verified on the preceding line.
            unsafe { simd_sse::f64x2::elementwise_into(left, right, out, op) };
            return;
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON fp64 arithmetic is part of the arm64 baseline.
        unsafe { simd_neon::f64x2::elementwise_into(left, right, out, op) };
        return;
    }
    // Portable fallback with strict sequential ordering. aarch64 always
    // returns above, so this loop is unreachable and cfg'd out there.
    #[cfg(not(target_arch = "aarch64"))]
    {
        for (idx, slot) in out.iter_mut().enumerate() {
            *slot = op.apply(left[idx], right[idx]);
        }
    }
}

/// Runtime-dispatching elementwise kernel for f32 lanes.
#[cfg(test)]
pub(crate) fn elementwise_f32(left: &[f32], right: &[f32], out: &mut [f32], op: ElementwiseOp) {
    debug_assert_eq!(left.len(), right.len());
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: avx2 availability verified on the preceding line.
            unsafe { simd_avx2::f32x8::elementwise_into(left, right, out, op) };
            return;
        }
        if std::arch::is_x86_feature_detected!("sse4.2") {
            // SAFETY: sse4.2 availability verified on the preceding line.
            unsafe { simd_sse::f32x4::elementwise_into(left, right, out, op) };
            return;
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON fp32 arithmetic is part of the arm64 baseline.
        unsafe { simd_neon::f32x4::elementwise_into(left, right, out, op) };
        return;
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        for (idx, slot) in out.iter_mut().enumerate() {
            *slot = op.apply(left[idx], right[idx]);
        }
    }
}

/// Runtime-dispatching dot product for f64 lanes. Chunked SIMD accumulation
/// reorders sums relative to a strictly sequential loop; agreement is within
/// the documented tolerance policy (see file header).
pub(crate) fn dot_f64(left: &[f64], right: &[f64]) -> f64 {
    debug_assert_eq!(left.len(), right.len());
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: avx2 availability verified on the preceding line.
            return unsafe { simd_avx2::f64x4::dot(left, right) };
        }
        if std::arch::is_x86_feature_detected!("sse4.2") {
            // SAFETY: sse4.2 availability verified on the preceding line.
            return unsafe { simd_sse::f64x2::dot(left, right) };
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON fp64 arithmetic is part of the arm64 baseline.
        return unsafe { simd_neon::f64x2::dot(left, right) };
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let mut total = 0.0f64;
        for idx in 0..left.len() {
            total += left[idx] * right[idx];
        }
        total
    }
}

/// Runtime-dispatching dot product for f32 lanes.
#[cfg(test)]
pub(crate) fn dot_f32(left: &[f32], right: &[f32]) -> f32 {
    debug_assert_eq!(left.len(), right.len());
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: avx2 availability verified on the preceding line.
            return unsafe { simd_avx2::f32x8::dot(left, right) };
        }
        if std::arch::is_x86_feature_detected!("sse4.2") {
            // SAFETY: sse4.2 availability verified on the preceding line.
            return unsafe { simd_sse::f32x4::dot(left, right) };
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON fp32 arithmetic is part of the arm64 baseline.
        return unsafe { simd_neon::f32x4::dot(left, right) };
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let mut total = 0.0f32;
        for idx in 0..left.len() {
            total += left[idx] * right[idx];
        }
        total
    }
}

pub(crate) fn kernel_elementwise_f64_bits(
    left: &[SpectraHostValue],
    right: &[SpectraHostValue],
    op: ElementwiseOp,
) -> Vec<SpectraHostValue> {
    debug_assert_eq!(left.len(), right.len());
    let mut out = vec![0.0f64; left.len()];
    elementwise_f64(
        widen_float_slices(left),
        widen_float_slices(right),
        &mut out,
        op,
    );
    out.iter()
        .map(|value| value.to_bits() as SpectraHostValue)
        .collect()
}

/// Scalar reference for the elementwise float path. Per-element operations
/// cannot be reordered, so this matches the SIMD kernels bit-for-bit; it
/// stays the authority for exactness checks.
#[cfg(test)]
pub(crate) fn kernel_elementwise_f64_bits_scalar(
    left: &[SpectraHostValue],
    right: &[SpectraHostValue],
    op: ElementwiseOp,
) -> Vec<SpectraHostValue> {
    debug_assert_eq!(left.len(), right.len());
    left.iter()
        .zip(right.iter())
        .map(|(a, b)| {
            op.apply(f64::from_bits(*a as u64), f64::from_bits(*b as u64))
                .to_bits() as SpectraHostValue
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn kernel_elementwise_f32_values(
    left: &[f32],
    right: &[f32],
    op: ElementwiseOp,
) -> Vec<f32> {
    debug_assert_eq!(left.len(), right.len());
    let mut out = vec![0.0f32; left.len()];
    elementwise_f32(left, right, &mut out, op);
    out
}

/// Tensor-op entry point for binary float elementwise over host-value bit
/// slices. Takes the wide SIMD route for every lane-safe input. Divide
/// keeps the legacy language rule (`x / 0 -> NaN` for every numerator,
/// including infinities) instead of raw IEEE-754 signed-infinity results,
/// so whenever any divisor is zero the whole computation falls back to the
/// scalar loop that implements that rule exactly.
pub(crate) fn tensor_binary_float_kernel(
    left: &[SpectraHostValue],
    right: &[SpectraHostValue],
    op: ElementwiseOp,
) -> Vec<SpectraHostValue> {
    debug_assert_eq!(left.len(), right.len());
    if op == ElementwiseOp::Div && widen_float_slices(right).contains(&0.0) {
        return left
            .iter()
            .zip(right.iter())
            .map(|(a, b)| {
                let dividend = f64::from_bits(*a as u64);
                let divisor = f64::from_bits(*b as u64);
                (if divisor == 0.0 {
                    f64::NAN
                } else {
                    dividend / divisor
                })
                .to_bits() as SpectraHostValue
            })
            .collect();
    }
    kernel_elementwise_f64_bits(left, right, op)
}

pub(crate) fn kernel_dot_f64_bits(
    left: &[SpectraHostValue],
    right: &[SpectraHostValue],
) -> SpectraHostValue {
    kernel_dot_f64(widen_float_slices(left), widen_float_slices(right)).to_bits()
        as SpectraHostValue
}

pub(crate) fn kernel_dot_f64(left: &[f64], right: &[f64]) -> f64 {
    dot_f64(left, right)
}
#[cfg(test)]
pub(crate) fn kernel_dot_f32(left: &[f32], right: &[f32]) -> f32 {
    dot_f32(left, right)
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

#[cfg(test)]
mod simd_kernel_tests {
    use super::*;

    /// Deterministic LCG noise in [-1, 1); small magnitudes keep the
    /// 1e-12 relative-tolerance assertions meaningful after reassociation.
    fn noise(bits_seed: u64) -> impl Iterator<Item = f64> {
        let mut state = bits_seed | 1;
        std::iter::from_fn(move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let unit = ((state >> 11) as f64) / ((1u64 << 53) as f64);
            Some(unit * 2.0 - 1.0)
        })
    }

    fn host_from(values: &[f64]) -> Vec<SpectraHostValue> {
        values
            .iter()
            .map(|v| v.to_bits() as SpectraHostValue)
            .collect()
    }

    fn sequential_dot(left: &[f64], right: &[f64]) -> f64 {
        left.iter()
            .zip(right.iter())
            .map(|(a, b)| a * b)
            .sum::<f64>()
    }

    /// Sizes straddle every lane-count boundary: below width, past SSE
    /// double pairs, past AVX2 quads, and well past loop unrolling.
    const SIZES: [usize; 5] = [1, 7, 8, 33, 1000];

    #[test]
    fn simd_kernel_elementwise_matches_scalar_reference_on_odd_sizes() {
        for size in SIZES {
            let left: Vec<f64> = noise(0xA5A5 + size as u64).take(size).collect();
            let right: Vec<f64> = noise(0x5A5A + size as u64)
                .take(size)
                .map(|v| if v == 0.0 { 0.25 } else { v })
                .collect();
            let l = host_from(&left);
            let r = host_from(&right);
            for op in [
                ElementwiseOp::Add,
                ElementwiseOp::Sub,
                ElementwiseOp::Mul,
                ElementwiseOp::Div,
            ] {
                let simd = kernel_elementwise_f64_bits(&l, &r, op);
                let reference = kernel_elementwise_f64_bits_scalar(&l, &r, op);
                assert_eq!(simd.len(), reference.len(), "size {size} op {op:?}");
                for idx in 0..simd.len() {
                    assert_eq!(
                        simd[idx], reference[idx],
                        "bit mismatch at [{idx}] size {size} op {op:?}"
                    );
                    let got = f64::from_bits(simd[idx] as u64);
                    let want = f64::from_bits(reference[idx] as u64);
                    assert!(
                        (got - want).abs() <= 1e-12 * want.abs().max(1.0),
                        "value mismatch at [{idx}] size {size} op {op:?}: {got} vs {want}"
                    );
                }
            }
        }
    }

    #[test]
    fn simd_kernel_elementwise_f32_matches_scalar_reference_on_odd_sizes() {
        for size in SIZES {
            let left: Vec<f64> = noise(0x1111 + size as u64).take(size).collect();
            let right: Vec<f64> = noise(0x2222 + size as u64).take(size).collect();
            let to_f32 = |v: &f64| -> f32 { (if *v == 0.0 { 0.25 } else { *v }) as f32 };
            let l: Vec<f32> = left.iter().map(to_f32).collect();
            let r: Vec<f32> = right.iter().map(to_f32).collect();
            for op in [
                ElementwiseOp::Add,
                ElementwiseOp::Sub,
                ElementwiseOp::Mul,
                ElementwiseOp::Div,
            ] {
                let simd = kernel_elementwise_f32_values(&l, &r, op);
                for idx in 0..size {
                    let want = op.apply(l[idx], r[idx]);
                    assert_eq!(
                        simd[idx], want,
                        "f32 bit mismatch at [{idx}] size {size} op {op:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn simd_kernel_dot_f64_matches_sequential_within_tolerance() {
        for size in SIZES {
            let left: Vec<f64> = noise(0xD07D + size as u64).take(size).collect();
            let right: Vec<f64> = noise(0xBEEF + size as u64).take(size).collect();
            let got = kernel_dot_f64(&left, &right);
            let want = sequential_dot(&left, &right);
            assert!(
                (got - want).abs() <= 1e-12 * want.abs().max(1.0),
                "dot f64 drift at size {size}: {got} vs {want}"
            );
        }
    }

    #[test]
    fn simd_kernel_dot_f32_matches_sequential_within_tolerance() {
        for size in SIZES {
            let left: Vec<f32> = noise(0xF300 + size as u64)
                .take(size)
                .map(|v| v as f32)
                .collect();
            let right: Vec<f32> = noise(0x40C0 + size as u64)
                .take(size)
                .map(|v| v as f32)
                .collect();
            let got = kernel_dot_f32(&left, &right);
            let want: f32 = left.iter().zip(right.iter()).map(|(a, b)| a * b).sum();
            assert!(
                (got - want).abs() <= 1e-3 * want.abs().max(1.0),
                "dot f32 drift at size {size}: {got} vs {want}"
            );
        }
    }

    #[test]
    fn simd_kernel_matmul_f64_bits_matches_scalar_reference_within_tolerance() {
        let cases: [(usize, usize, usize); 4] = [(2, 7, 3), (3, 8, 5), (5, 33, 4), (7, 16, 9)];
        for (m, k, n) in cases {
            let total_l = m * k;
            let total_r = k * n;
            let left: Vec<f64> = noise(0xA000 + total_l as u64).take(total_l).collect();
            let right: Vec<f64> = noise(0xB000 + total_r as u64).take(total_r).collect();
            let l = host_from(&left);
            let r = host_from(&right);
            let simd = kernel_matmul_f64_bits(&l, &r, m, k, n);
            for row in 0..m {
                for col in 0..n {
                    let mut acc = 0.0f64;
                    for inner in 0..k {
                        acc += left[row * k + inner] * right[inner * n + col];
                    }
                    let got = f64::from_bits(simd[row * n + col] as u64);
                    assert!(
                        (got - acc).abs() <= 1e-12 * acc.abs().max(1.0),
                        "matmul drift at ({row},{col}) m{k}xn{n}: {got} vs {acc}"
                    );
                }
            }
        }
    }

    #[test]
    fn simd_kernel_div_by_zero_keeps_nan_rule() {
        let left: [SpectraHostValue; 3] = [
            1.0f64.to_bits() as i64,
            (-1.0f64).to_bits() as i64,
            0.0f64.to_bits() as i64,
        ];
        let right: [SpectraHostValue; 3] = [
            2.0f64.to_bits() as i64,
            0.0f64.to_bits() as i64,
            0.0f64.to_bits() as i64,
        ];
        let out = tensor_binary_float_kernel(&left, &right, ElementwiseOp::Div);
        // Legacy language rule: any zero divisor yields NaN for that lane;
        // nonzero divisors compute normally (1/2 stays 0.5).
        assert_eq!(f64::from_bits(out[0] as u64), 0.5);
        assert!(f64::from_bits(out[1] as u64).is_nan());
        assert!(f64::from_bits(out[2] as u64).is_nan());
    }

    #[test]
    fn simd_kernel_strategy_reports_detected_path() {
        // Every variant must be constructible without dead_code shims.
        for strategy in TensorKernelStrategy::ALL {
            match strategy {
                TensorKernelStrategy::Scalar
                | TensorKernelStrategy::Avx2
                | TensorKernelStrategy::Neon
                | TensorKernelStrategy::Gpu => {}
            }
        }
        // Strategy codes stay stable even though the fake Blas tier was
        // retired (its discriminant 4 intentionally remains unused).
        assert_eq!(TensorKernelStrategy::Scalar.code(), 1);
        assert_eq!(TensorKernelStrategy::Avx2.code(), 2);
        assert_eq!(TensorKernelStrategy::Neon.code(), 3);
        assert_eq!(TensorKernelStrategy::Gpu.code(), 5);

        let cpu = TensorKernelStrategy::cpu();
        let current = TensorKernelStrategy::current();
        // current() must equal the CPU capability unless a real GPU call
        // site is available, in which case it reports Gpu.
        if current == TensorKernelStrategy::Gpu {
            #[cfg(feature = "gpu")]
            assert!(crate::gpu::is_available());
        } else {
            assert_eq!(current, cpu);
        }
        // Cross-check against raw feature probing on this machine.
        #[cfg(target_arch = "x86_64")]
        {
            let avx2 = std::arch::is_x86_feature_detected!("avx2");
            assert_eq!(
                cpu == TensorKernelStrategy::Avx2,
                avx2,
                "cpu() must mirror avx2 runtime detection"
            );
        }
        #[cfg(target_arch = "aarch64")]
        assert_eq!(cpu, TensorKernelStrategy::Neon);
    }
}
