use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuUnaryOp {
    Neg,
    Relu,
}

/// Stable kind tag for a GPU error, used by `std_tensor_stats_gpu_errors` to
/// surface per-kind counters (R-3023). The previous `Err(String)` path
/// swallowed every error into a single silent `stats_cpu_fallbacks++`
/// counter; this enum restores the diagnostic signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuErrorKind {
    /// Input shape or size does not satisfy the kernel's contract
    ShapeMismatch,
    /// WGSL compilation failed
    ShaderCompile,
    /// wgpu buffer allocation failed
    BufferAlloc,
    /// wgpu command submission failed
    Dispatch,
    /// wgpu readback (map_async) failed
    Readback,
    /// Requested capability (e.g. f16) is not supported by the adapter
    FeatureUnsupported,
    /// Anything else (wgpu adapter missing, poisoned lock, etc.)
    Other,
}

impl GpuErrorKind {
    /// Stable integer code for the public API.
    pub fn code(self) -> i32 {
        match self {
            Self::ShapeMismatch => 0,
            Self::ShaderCompile => 1,
            Self::BufferAlloc => 2,
            Self::Dispatch => 3,
            Self::Readback => 4,
            Self::FeatureUnsupported => 5,
            Self::Other => 6,
        }
    }

    fn from_message(message: &str) -> Self {
        if message.contains("shape") {
            Self::ShapeMismatch
        } else if message.contains("shader") || message.contains("compil") {
            Self::ShaderCompile
        } else if message.contains("buffer") || message.contains("alloc") {
            Self::BufferAlloc
        } else if message.contains("readback") || message.contains("map") {
            Self::Readback
        } else if message.contains("feature") || message.contains("supported") {
            Self::FeatureUnsupported
        } else {
            Self::Other
        }
    }
}

/// A typed GPU error surfaced to `std_tensor_stats_gpu_errors`.
#[derive(Debug, Clone)]
pub struct GpuError {
    pub kind: GpuErrorKind,
    pub message: String,
}

impl GpuError {
    pub fn new(kind: GpuErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Classify a free-form message into a `GpuErrorKind`. Used by callers
    /// that only have the historical `String` payload.
    pub fn from_message(message: impl Into<String>) -> Self {
        let message = message.into();
        let kind = GpuErrorKind::from_message(&message);
        Self { kind, message }
    }
}

struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

static CONTEXT: OnceLock<Result<Mutex<GpuContext>, GpuError>> = OnceLock::new();

pub fn is_available() -> bool {
    context().is_ok()
}

/// Lock the GPU context and run a closure with `&wgpu::Device` and
/// `&wgpu::Queue`. Used by `to_device` to write into pool buffers; the
/// closure receives the same queue that `run_compute` uses, so an upload
/// followed by a kernel dispatch is correctly ordered.
pub fn with_device_queue<R>(
    f: impl FnOnce(&wgpu::Device, &wgpu::Queue) -> R,
) -> Result<R, GpuError> {
    let guard = context()?
        .lock()
        .map_err(|_| GpuError::new(GpuErrorKind::Other, "gpu context poisoned"))?;
    Ok(f(&guard.device, &guard.queue))
}

/// Mirror of the runtime's `TensorDevice` enum used to key the device
/// buffer pool. Only `Wgpu` is exercised by the arena today; the others
/// are reserved for future native backends (R-3201..R-3204).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolDevice {
    Wgpu,
}

/// Mirror of the runtime's `TensorDType` for pool keying. Only `Float`
/// is exercised today; the rest are reserved for the f16/bf16 work in
/// R-3071.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolDType {
    Float,
}

impl PoolDType {
    pub fn byte_size(self) -> u64 {
        match self {
            Self::Float => 4,
        }
    }
}

/// Owned, type-erased device buffer that is safe to hand back to a pool
/// when the last `Arc` is dropped. The Arc-wrapped `wgpu::Buffer` keeps
/// the GPU resource alive for any other holder while a buffer is in the
/// free list (R-3051, D1).
#[derive(Debug, Clone)]
pub struct DeviceBuffer {
    pub buffer: Arc<wgpu::Buffer>,
    pub size: u64,
    pub elements: usize,
    pub device: PoolDevice,
    pub dtype: PoolDType,
}

/// Bucket key. A buffer of bucket `B` is reusable for any `n <= B`; the
/// pool acquires it for a request of `n` and reuses the storage as long
/// as the caller writes the full `n` elements before releasing.
pub type BucketKey = (PoolDevice, PoolDType, u64);

/// Free-list pool of device buffers keyed by `(device, dtype, size_bucket)`
/// (R-3051, D2). Held inside the existing `TensorRegistry` mutex, so it
/// adds no new lock surface. The `bytes_resident` counter is the single
/// source of truth for `stats_device_pool_bytes_resident`.
///
/// Invariant for reuse safety: every `acquire` is followed by either
/// `queue.submit` (with a full data overwrite) before the buffer is
/// released back to the pool, or the buffer is dropped without being
/// pooled. See `acquire` doc-comment.
#[derive(Debug, Default)]
pub struct DeviceArena {
    free: HashMap<BucketKey, VecDeque<DeviceBuffer>>,
    hits: u64,
    misses: u64,
    bytes_resident: u64,
}

pub const MAX_FREE_PER_BUCKET: usize = 16;

fn bucket_for(elements: usize) -> u64 {
    (elements.max(16) as u64).next_power_of_two()
}

impl DeviceArena {
    pub fn new() -> Self {
        Self::default()
    }

    /// Acquire a buffer of bucket `bucket_for(n)`. Increments `hits` on
    /// pool reuse, `misses` on a fresh allocation. `MAX_FREE_PER_BUCKET`
    /// caps the free list per bucket.
    pub fn acquire(
        &mut self,
        device: PoolDevice,
        dtype: PoolDType,
        elements: usize,
        device_for_buffer: &wgpu::Device,
    ) -> DeviceBuffer {
        let bucket = bucket_for(elements);
        let key = (device, dtype, bucket);
        if let Some(list) = self.free.get_mut(&key) {
            if let Some(mut buf) = list.pop_front() {
                self.hits = self.hits.saturating_add(1);
                buf.elements = elements;
                return buf;
            }
        }
        self.misses = self.misses.saturating_add(1);
        let size = bucket * dtype.byte_size();
        let buffer = device_for_buffer.create_buffer(&wgpu::BufferDescriptor {
            label: Some("spectra-runtime-device-pool"),
            size,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        self.bytes_resident = self.bytes_resident.saturating_add(size);
        DeviceBuffer {
            buffer: Arc::new(buffer),
            size,
            elements,
            device,
            dtype,
        }
    }

    /// Return a buffer to the pool. The free list per bucket is capped
    /// at `MAX_FREE_PER_BUCKET`; over-cap releases are dropped, which
    /// drops the `Arc` and lets wgpu reclaim the underlying buffer.
    pub fn release(&mut self, buf: DeviceBuffer) {
        let bucket = bucket_for(buf.elements);
        let key = (buf.device, buf.dtype, bucket);
        let list = self.free.entry(key).or_default();
        if list.len() >= MAX_FREE_PER_BUCKET {
            self.bytes_resident = self.bytes_resident.saturating_sub(buf.size);
            return;
        }
        list.push_back(buf);
    }

    pub fn hits(&self) -> u64 {
        self.hits
    }

    pub fn misses(&self) -> u64 {
        self.misses
    }

    pub fn bytes_resident(&self) -> u64 {
        self.bytes_resident
    }

    pub fn reset(&mut self) {
        self.free.clear();
        self.hits = 0;
        self.misses = 0;
        self.bytes_resident = 0;
    }
}

pub fn adapter_name() -> Option<String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))?;
    Some(adapter.get_info().name)
}

pub fn binary(left: &[f32], right: &[f32], op: GpuBinaryOp) -> Result<Vec<f32>, GpuError> {
    if left.len() != right.len() {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu binary shape mismatch",
        ));
    }
    if left.is_empty() {
        return Ok(Vec::new());
    }
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> left: array<f32>;
@group(0) @binding(1) var<storage, read> right: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let i = id.x;
    if (i >= {len}u) {{
        return;
    }}
    out[i] = {expr};
}}
"#,
        len = left.len(),
        expr = match op {
            GpuBinaryOp::Add => "left[i] + right[i]",
            GpuBinaryOp::Sub => "left[i] - right[i]",
            GpuBinaryOp::Mul => "left[i] * right[i]",
            GpuBinaryOp::Div => "select(left[i] / right[i], 0.0, right[i] == 0.0)",
        }
    );
    dispatch_two_inputs(left, right, left.len(), &shader, [left.len() as u32, 1, 1])
}

pub fn unary(input: &[f32], op: GpuUnaryOp) -> Result<Vec<f32>, GpuError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> input_values: array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let i = id.x;
    if (i >= {len}u) {{
        return;
    }}
    out[i] = {expr};
}}
"#,
        len = input.len(),
        expr = match op {
            GpuUnaryOp::Neg => "-input_values[i]",
            GpuUnaryOp::Relu => "max(input_values[i], 0.0)",
        }
    );
    dispatch_one_input(input, input.len(), &shader, [input.len() as u32, 1, 1])
}

pub fn sum(input: &[f32]) -> Result<f32, GpuError> {
    if input.is_empty() {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu reduction requires at least one element",
        ));
    }
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> input_values: array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main() {{
    var acc = 0.0;
    for (var i = 0u; i < {len}u; i = i + 1u) {{
        acc = acc + input_values[i];
    }}
    out[0] = acc;
}}
"#,
        len = input.len()
    );
    dispatch_one_input(input, 1, &shader, [1, 1, 1]).map(|values| values[0])
}

pub fn matmul(
    left: &[f32],
    right: &[f32],
    m: usize,
    k: usize,
    n: usize,
) -> Result<Vec<f32>, GpuError> {
    if left.len() != m.saturating_mul(k) || right.len() != k.saturating_mul(n) {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu matmul shape mismatch",
        ));
    }
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> left: array<f32>;
@group(0) @binding(1) var<storage, read> right: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let index = id.x;
    if (index >= {out_len}u) {{
        return;
    }}
    let row = index / {n}u;
    let col = index % {n}u;
    var acc = 0.0;
    for (var inner = 0u; inner < {k}u; inner = inner + 1u) {{
        acc = acc + left[row * {k}u + inner] * right[inner * {n}u + col];
    }}
    out[index] = acc;
}}
"#,
        k = k,
        n = n,
        out_len = m * n
    );
    dispatch_two_inputs(left, right, m * n, &shader, [(m * n) as u32, 1, 1])
}

pub fn conv2d(
    input: &[f32],
    kernel: &[f32],
    bias: &[f32],
    dims: [usize; 7],
) -> Result<Vec<f32>, GpuError> {
    let [batch, in_ch, h, w, out_ch, kh, kw] = dims;
    if h < kh || w < kw {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu conv2d invalid kernel dimensions",
        ));
    }
    if input.len() != batch * in_ch * h * w
        || kernel.len() != out_ch * in_ch * kh * kw
        || bias.len() != out_ch
    {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu conv2d shape mismatch",
        ));
    }
    let out_h = h - kh + 1;
    let out_w = w - kw + 1;
    let out_len = batch * out_ch * out_h * out_w;
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> input_values: array<f32>;
@group(0) @binding(1) var<storage, read> kernel_values: array<f32>;
@group(0) @binding(2) var<storage, read> bias_values: array<f32>;
@group(0) @binding(3) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let index = id.x;
    if (index >= {out_len}u) {{
        return;
    }}
    let ox = index % {out_w}u;
    let oy = (index / {out_w}u) % {out_h}u;
    let oc = (index / ({out_w}u * {out_h}u)) % {out_ch}u;
    let n = index / ({out_w}u * {out_h}u * {out_ch}u);
    var acc = bias_values[oc];
    for (var ic = 0u; ic < {in_ch}u; ic = ic + 1u) {{
        for (var ky = 0u; ky < {kh}u; ky = ky + 1u) {{
            for (var kx = 0u; kx < {kw}u; kx = kx + 1u) {{
                let input_idx = ((n * {in_ch}u + ic) * {h}u + oy + ky) * {w}u + ox + kx;
                let kernel_idx = ((oc * {in_ch}u + ic) * {kh}u + ky) * {kw}u + kx;
                acc = acc + input_values[input_idx] * kernel_values[kernel_idx];
            }}
        }}
    }}
    out[index] = acc;
}}
"#,
        in_ch = in_ch,
        h = h,
        w = w,
        out_ch = out_ch,
        kh = kh,
        kw = kw,
        out_h = out_h,
        out_w = out_w,
        out_len = out_len
    );
    dispatch_three_inputs(
        input,
        kernel,
        bias,
        out_len,
        &shader,
        [out_len as u32, 1, 1],
    )
}

// ===== R-3080: GPU backward kernels =====
//
// Each `backward_*` returns a freshly-allocated `Vec<f32>` in row-major
// layout that matches the equivalent CPU implementation in
// `runtime/src/stdlib/mod.rs::autograd_parent_grads`. The CPU path is
// the source of truth for tolerance; tests in
// `tensor_runtime_r1603_backward_*` cross-check both paths via
// finite differences.

/// `out[i, j] = sum_l grad[i, l] * right[j, l]` for `C = A @ B`.
/// Equivalent to `grad @ right.T` in row-major. Output length `m*k`.
pub fn backward_matmul_left(
    grad: &[f32],
    right: &[f32],
    m: usize,
    k: usize,
    n: usize,
) -> Result<Vec<f32>, GpuError> {
    if grad.len() != m.saturating_mul(n) || right.len() != k.saturating_mul(n) {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu backward_matmul_left shape mismatch",
        ));
    }
    if m == 0 || k == 0 {
        return Ok(Vec::new());
    }
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> grad_values: array<f32>;
@group(0) @binding(1) var<storage, read> right_values: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let index = id.x;
    let total = {m}u * {k}u;
    if (index >= total) {{
        return;
    }}
    let i = index / {k}u;
    let j = index % {k}u;
    var acc = 0.0;
    for (var l = 0u; l < {n}u; l = l + 1u) {{
        acc = acc + grad_values[i * {n}u + l] * right_values[j * {n}u + l];
    }}
    out[index] = acc;
}}
"#,
        m = m,
        k = k,
        n = n
    );
    dispatch_two_inputs(grad, right, m * k, &shader, [(m * k) as u32, 1, 1])
}

/// `out[j, l] = sum_i left[i, j] * grad[i, l]` for `C = A @ B`.
/// Equivalent to `left.T @ grad` in row-major. Output length `k*n`.
pub fn backward_matmul_right(
    left: &[f32],
    grad: &[f32],
    m: usize,
    k: usize,
    n: usize,
) -> Result<Vec<f32>, GpuError> {
    if left.len() != m.saturating_mul(k) || grad.len() != m.saturating_mul(n) {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu backward_matmul_right shape mismatch",
        ));
    }
    if k == 0 || n == 0 {
        return Ok(Vec::new());
    }
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> left_values: array<f32>;
@group(0) @binding(1) var<storage, read> grad_values: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let index = id.x;
    let total = {k}u * {n}u;
    if (index >= total) {{
        return;
    }}
    let j = index / {n}u;
    let l = index % {n}u;
    var acc = 0.0;
    for (var i = 0u; i < {m}u; i = i + 1u) {{
        acc = acc + left_values[i * {k}u + j] * grad_values[i * {n}u + l];
    }}
    out[index] = acc;
}}
"#,
        m = m,
        k = k,
        n = n
    );
    dispatch_two_inputs(left, grad, k * n, &shader, [(k * n) as u32, 1, 1])
}

/// `out[i] = grad[i] if output[i] > 0 else 0` for `output = relu(input)`.
/// Output length `n`; `output` carries the post-activation values.
pub fn backward_relu(grad: &[f32], output: &[f32]) -> Result<Vec<f32>, GpuError> {
    if grad.len() != output.len() {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu backward_relu shape mismatch",
        ));
    }
    if grad.is_empty() {
        return Ok(Vec::new());
    }
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> grad_values: array<f32>;
@group(0) @binding(1) var<storage, read> output_values: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let i = id.x;
    if (i >= {len}u) {{
        return;
    }}
    out[i] = select(0.0, grad_values[i], output_values[i] > 0.0);
}}
"#,
        len = grad.len()
    );
    dispatch_two_inputs(grad, output, grad.len(), &shader, [grad.len() as u32, 1, 1])
}

/// Backward pass for `out = sigmoid(input)` (autograd uses
/// `grad * out * (1 - out)`). Kept here because the forward path in
/// `autograd_parent_grads` for `Sigmoid` references a host
/// implementation; when a `Sigmoid` forward is added to the GPU
/// surface, the backward can be reused as-is.
pub fn backward_sigmoid(grad: &[f32], output: &[f32]) -> Result<Vec<f32>, GpuError> {
    if grad.len() != output.len() {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu backward_sigmoid shape mismatch",
        ));
    }
    if grad.is_empty() {
        return Ok(Vec::new());
    }
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> grad_values: array<f32>;
@group(0) @binding(1) var<storage, read> output_values: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let i = id.x;
    if (i >= {len}u) {{
        return;
    }}
    out[i] = grad_values[i] * output_values[i] * (1.0 - output_values[i]);
}}
"#,
        len = grad.len()
    );
    dispatch_two_inputs(grad, output, grad.len(), &shader, [grad.len() as u32, 1, 1])
}

