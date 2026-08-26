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
    Exp,
    Log,
    Sqrt,
    Sigmoid,
    Tanh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuReduceOp {
    Sum,
    Mean,
    Min,
    Max,
    /// Index (u32) of the maximum element; ties resolve to the lowest
    /// index for determinism.
    ArgMax,
}

impl GpuReduceOp {
    /// ArgMax packs `(value, index)` pairs into the partials buffer,
    /// so it needs twice the scalar partials capacity.
    fn partials_stride(self) -> u64 {
        match self {
            GpuReduceOp::ArgMax => 2,
            _ => 1,
        }
    }
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
            GpuUnaryOp::Exp => "exp(input_values[i])",
            GpuUnaryOp::Log => "log(input_values[i])",
            GpuUnaryOp::Sqrt => "sqrt(input_values[i])",
            GpuUnaryOp::Sigmoid => "1.0 / (1.0 + exp(-input_values[i]))",
            GpuUnaryOp::Tanh => "tanh(input_values[i])",
        }
    );
    dispatch_one_input(input, input.len(), &shader, [input.len() as u32, 1, 1])
}

// ===== Parallel tree reduction (two-stage, single-pass pair) =====
//
// The previous sum kernel ran one invocation with a serial O(len) loop
// (workgroup_size(1)). That starved the GPU on every loss reduction.
// The replacement is a classic two-phase tree reduction:
//
//   Stage 1 (`reduce`): workgroup_size(256). Each workgroup folds a
//   strided tile of the input into per-thread accumulators, does a
//   log2(256)=8-step shared-memory tree fold, and writes one partial
//   sum per workgroup.
//
//   Stage 2 (`final_reduce`): one 256-wide workgroup grid-strides over
//   the partials, repeats the shared-memory tree fold, and writes the
//   final scalar.
//
// float atomicAdd is not portable in WGSL, hence the two-phase design.
// Both stages share one bind group (input, partials, out); dispatching
// them inside a single compute pass gives the required ordering, so
// there is exactly one submit and (on the host path) one tiny readback.

const REDUCTION_WORKGROUP_SIZE: u32 = 256;
/// Input elements processed per stage-1 thread before the tree fold.
const REDUCTION_ITEMS_PER_THREAD: u32 = 16;
/// Cap on stage-1 workgroups (and therefore partials). Larger inputs
/// simply make each thread walk more elements via the grid-stride loop.
const REDUCTION_MAX_WORKGROUPS: u32 = 4096;

/// Stage-1 workgroup plan: returns `(workgroups, total_threads,
/// partials)`. `workgroups == partials`; `total_threads` is baked into
/// the shader's grid-stride step.
pub fn reduction_plan(len: usize) -> (u32, u32, u32) {
    debug_assert!(len > 0);
    let tile = u64::from(REDUCTION_WORKGROUP_SIZE) * u64::from(REDUCTION_ITEMS_PER_THREAD);
    let mut workgroups = (((len as u64) + tile - 1) / tile) as u32;
    workgroups = workgroups.clamp(1, REDUCTION_MAX_WORKGROUPS);
    (
        workgroups,
        workgroups * REDUCTION_WORKGROUP_SIZE,
        workgroups,
    )
}

/// WGSL source for the two-entry-point parallel reduction, parametrized
/// by `op`. Shared by the host-materializing path
/// (`dispatch_tree_reduction` in gpu_runtime.rs) and the device-buffer
/// paths (`sum_device`, `mean_device`, `min_device`, `max_device`,
/// `argmax_device` in gpu_device_dispatch.rs).
///
/// Numeric ops fold one accumulator per thread; ArgMax folds a
/// `(value, index)` pair per thread with strict-greater comparison and
/// lowest-index tie-breaking at every level (thread scan, workgroup
/// tree, final fold), so the result index is deterministic regardless
/// of dispatch geometry.
fn reduction_shader(len: usize, total_threads: u32, partials: u32, op: GpuReduceOp) -> String {
    // WGSL has no inf literal; bitcast the IEEE-754 bit patterns.
    const POS_INF: &str = "bitcast<f32>(0x7f800000u)";
    const NEG_INF: &str = "bitcast<f32>(0xff800000u)";

    let numeric = match op {
        GpuReduceOp::Sum | GpuReduceOp::Mean => {
            let final_body = if op == GpuReduceOp::Mean {
                // Divide once in the final phase: mean = sum / len.
                format!("out[0] = scratch[0] / {len}.0;")
            } else {
                "out[0] = scratch[0];".to_string()
            };
            ShaderBody {
                init: "0.0".to_string(),
                stage1_fold: "acc = acc + input_values[idx];".to_string(),
                stage1_combine: "scratch[lid.x] = scratch[lid.x] + scratch[lid.x + stride];"
                    .to_string(),
                final_init: "0.0".to_string(),
                final_fold: "acc = acc + partials[i];".to_string(),
                final_combine:
                    "scratch[lid.x] = scratch[lid.x] + scratch[lid.x + stride];".to_string(),
                final_body,
                partials_stride: 1,
            }
        }
        GpuReduceOp::Min => ShaderBody {
            init: format!("{POS_INF}"),
            stage1_fold: "acc = min(acc, input_values[idx]);".to_string(),
            stage1_combine:
                "scratch[lid.x] = min(scratch[lid.x], scratch[lid.x + stride]);".to_string(),
            final_init: format!("{POS_INF}"),
            final_fold: "acc = min(acc, partials[i]);".to_string(),
            final_combine: "scratch[lid.x] = min(scratch[lid.x], scratch[lid.x + stride]);"
                .to_string(),
            final_body: "out[0] = scratch[0];".to_string(),
            partials_stride: 1,
        },
        GpuReduceOp::Max => ShaderBody {
            init: format!("{NEG_INF}"),
            stage1_fold: "acc = max(acc, input_values[idx]);".to_string(),
            stage1_combine:
                "scratch[lid.x] = max(scratch[lid.x], scratch[lid.x + stride]);".to_string(),
            final_init: format!("{NEG_INF}"),
            final_fold: "acc = max(acc, partials[i]);".to_string(),
            final_combine: "scratch[lid.x] = max(scratch[lid.x], scratch[lid.x + stride]);"
                .to_string(),
            final_body: "out[0] = scratch[0];".to_string(),
            partials_stride: 1,
        },
        GpuReduceOp::ArgMax => {
            return argmax_shader(len, total_threads, partials);
        }
    };

    format!(
        r#"
@group(0) @binding(0) var<storage, read> input_values: array<f32>;
@group(0) @binding(1) var<storage, read_write> partials: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

var<workgroup> scratch: array<f32, 256>;

@compute @workgroup_size(256)
fn reduce(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
){{
    let count = {len}u;
    let step = {total_threads}u;
    var acc = {init};
    var idx = gid.x;
    while (idx < count) {{
        {stage1_fold}
        idx = idx + step;
    }}
    scratch[lid.x] = acc;
    workgroupBarrier();
    var stride = 128u;
    loop {{
        if (lid.x < stride) {{
            {stage1_combine}
        }}
        workgroupBarrier();
        if (stride <= 1u) {{
            break;
        }}
        stride = stride >> 1u;
    }}
    if (lid.x == 0u) {{
        partials[wid.x * {stride}u] = scratch[0];
    }}
}}

@compute @workgroup_size(256)
fn final_reduce(@builtin(local_invocation_id) lid: vec3<u32>) {{
    let count = {partials}u;
    var acc = {final_init};
    var i = lid.x;
    while (i < count) {{
        {final_fold}
        i = i + 256u;
    }}
    scratch[lid.x] = acc;
    workgroupBarrier();
    var stride = 128u;
    loop {{
        if (lid.x < stride) {{
            {final_combine}
        }}
        workgroupBarrier();
        if (stride <= 1u) {{
            break;
        }}
        stride = stride >> 1u;
    }}
    if (lid.x == 0u) {{
        {final_body}
    }}
}}
"#,
        len = len,
        total_threads = total_threads,
        partials = partials,
        init = numeric.init,
        stage1_fold = numeric.stage1_fold,
        stage1_combine = numeric.stage1_combine,
        final_init = numeric.final_init,
        final_fold = numeric.final_fold,
        final_combine = numeric.final_combine,
        final_body = numeric.final_body,
        stride = numeric.partials_stride,
    )
}

/// Injected code fragments for the numeric reduction template.
struct ShaderBody {
    init: String,
    stage1_fold: String,
    stage1_combine: String,
    final_init: String,
    final_fold: String,
    final_combine: String,
    final_body: String,
    partials_stride: u64,
}

/// ArgMax variant of the two-phase reduction. Partials hold
/// `(value, index)` f32 pairs (`partials[2*i]`, `partials[2*i+1]`);
/// the index is carried as a bit-cast f32 so it survives storage
/// round-trips exactly. Every combine prefers strictly greater values
/// and, on ties, the lower index — deterministic for any dispatch
/// geometry. Output scalar is the winning index bit-cast back from
/// f32 to u32 on the host.
fn argmax_shader(len: usize, total_threads: u32, partials: u32) -> String {
    format!(
        r#"
@group(0) @binding(0) var<storage, read> input_values: array<f32>;
@group(0) @binding(1) var<storage, read_write> partials: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

var<workgroup> scratch_val: array<f32, 256>;
var<workgroup> scratch_idx: array<u32, 256>;

@compute @workgroup_size(256)
fn reduce(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
){{
    let count = {len}u;
    let step = {total_threads}u;
    var best = bitcast<f32>(0xff800000u);
    var best_idx = count;
    var idx = gid.x;
    while (idx < count) {{
        let v = input_values[idx];
        if ((v > best) || ((v == best) && (idx < best_idx))) {{
            best = v;
            best_idx = idx;
        }}
        idx = idx + step;
    }}
    scratch_val[lid.x] = best;
    scratch_idx[lid.x] = best_idx;
    workgroupBarrier();
    var stride = 128u;
    loop {{
        if (lid.x < stride) {{
            let rv = scratch_val[lid.x + stride];
            let ri = scratch_idx[lid.x + stride];
            if ((rv > scratch_val[lid.x]) || ((rv == scratch_val[lid.x]) && (ri < scratch_idx[lid.x]))) {{
                scratch_val[lid.x] = rv;
                scratch_idx[lid.x] = ri;
            }}
        }}
        workgroupBarrier();
        if (stride <= 1u) {{
            break;
        }}
        stride = stride >> 1u;
    }}
    if (lid.x == 0u) {{
        partials[wid.x * 2u] = scratch_val[0];
        partials[wid.x * 2u + 1u] = bitcast<f32>(scratch_idx[0]);
    }}
}}

@compute @workgroup_size(256)
fn final_reduce(@builtin(local_invocation_id) lid: vec3<u32>) {{
    let count = {partials}u;
    var best = bitcast<f32>(0xff800000u);
    var best_idx = 0xffffffffu;
    var i = lid.x;
    while (i < count) {{
        let v = partials[i * 2u];
        let ix = bitcast<u32>(partials[i * 2u + 1u]);
        if ((v > best) || ((v == best) && (ix < best_idx))) {{
            best = v;
            best_idx = ix;
        }}
        i = i + 256u;
    }}
    scratch_val[lid.x] = best;
    scratch_idx[lid.x] = best_idx;
    workgroupBarrier();
    var stride = 128u;
    loop {{
        if (lid.x < stride) {{
            let rv = scratch_val[lid.x + stride];
            let ri = scratch_idx[lid.x + stride];
            if ((rv > scratch_val[lid.x]) || ((rv == scratch_val[lid.x]) && (ri < scratch_idx[lid.x]))) {{
                scratch_val[lid.x] = rv;
                scratch_idx[lid.x] = ri;
            }}
        }}
        workgroupBarrier();
        if (stride <= 1u) {{
            break;
        }}
        stride = stride >> 1u;
    }}
    if (lid.x == 0u) {{
        out[0] = bitcast<f32>(scratch_idx[0]);
    }}
}}
"#
    )
}

/// Host-materializing GPU reductions. Each reads back exactly one
/// scalar; without a GPU adapter the context error propagates and every
/// caller falls back to its counted CPU path (unchanged behavior).
pub fn sum(input: &[f32]) -> Result<f32, GpuError> {
    reduce_scalar(input, GpuReduceOp::Sum)
}

pub fn mean(input: &[f32]) -> Result<f32, GpuError> {
    reduce_scalar(input, GpuReduceOp::Mean)
}

pub fn min(input: &[f32]) -> Result<f32, GpuError> {
    reduce_scalar(input, GpuReduceOp::Min)
}

pub fn max(input: &[f32]) -> Result<f32, GpuError> {
    reduce_scalar(input, GpuReduceOp::Max)
}

/// Index of the maximum element; ties resolve to the lowest index.
/// The device writes the index as a bit-cast f32, so the round-trip is
/// exact and no tolerance applies.
pub fn argmax(input: &[f32]) -> Result<usize, GpuError> {
    reduce_scalar(input, GpuReduceOp::ArgMax)
        .map(|bits| f32::to_bits(bits) as usize)
}

fn reduce_scalar(input: &[f32], op: GpuReduceOp) -> Result<f32, GpuError> {
    if input.is_empty() {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu reduction requires at least one element",
        ));
    }
    dispatch_tree_reduction(input, op).map(|values| values[0])
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

pub fn matmul_batched(
    left: &[f32],
    right: &[f32],
    b: usize,
    m: usize,
    k: usize,
    n: usize,
) -> Result<Vec<f32>, GpuError> {
    if left.len() != b * m * k || right.len() != b * k * n {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu batched matmul shape mismatch",
        ));
    }
    let total = b * m * n;
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> left: array<f32>;
@group(0) @binding(1) var<storage, read> right: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let idx = id.x;
    if (idx >= {total}u) {{ return; }}
    let mn = {m}u * {n}u;
    let batch = idx / mn;
    let rem = idx % mn;
    let row = rem / {n}u;
    let col = rem % {n}u;
    var acc = 0.0;
    for (var kk = 0u; kk < {k}u; kk = kk + 1u) {{
        let l_off = batch * {m}u * {k}u + row * {k}u + kk;
        let r_off = batch * {k}u * {n}u + kk * {n}u + col;
        acc = acc + left[l_off] * right[r_off];
    }}
    out[idx] = acc;
}}
"#,
        total = total,
        m = m,
        n = n,
        k = k
    );
    dispatch_two_inputs(left, right, total, &shader, [total as u32, 1, 1])
}

pub fn maxpool2d(
    input: &[f32],
    n: usize,
    c: usize,
    h: usize,
    w: usize,
    kh: usize,
    kw: usize,
    stride_h: usize,
    stride_w: usize,
) -> Result<Vec<f32>, GpuError> {
    let oh = (h - kh) / stride_h + 1;
    let ow = (w - kw) / stride_w + 1;
    if input.len() != n * c * h * w {
        return Err(GpuError::new(GpuErrorKind::ShapeMismatch, "maxpool2d shape mismatch"));
    }
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> inp: array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let idx = id.x;
    if (idx >= {out_len}u) {{ return; }}
    let owu = {ow}u; let ohu = {oh}u; let wu = {w}u; let kwu = {kw}u; let khu = {khu}u;
    let sh = {sh}u; let sw = {sw}u; let cu = {c}u; let hu = {h}u;
    let n_idx = idx / (cu * ohu * owu);
    let rem1 = idx % (cu * ohu * owu);
    let c_idx = rem1 / (ohu * owu);
    let rem2 = rem1 % (ohu * owu);
    let oh_idx = rem2 / owu;
    let ow_idx = rem2 % owu;
    var best = bitcast<f32>(0xff800000u);
    for (var kh_idx = 0u; kh_idx < khu; kh_idx = kh_idx + 1u) {{
        for (var kw_idx = 0u; kw_idx < kwu; kw_idx = kw_idx + 1u) {{
            let ih = oh_idx * sh + kh_idx;
            let iw = ow_idx * sw + kw_idx;
            let off = ((n_idx * cu + c_idx) * hu + ih) * wu + iw;
            let v = inp[off];
            if (v > best) {{ best = v; }}
        }}
    }}
    out[idx] = best;
}}
"#,
        out_len = n * c * oh * ow, ow = ow, oh = oh, w = w, kw = kw, khu = kh, sh = stride_h, sw = stride_w, c = c, h = h
    );
    dispatch_one_input(input, n * c * oh * ow, &shader, [(n * c * oh * ow) as u32, 1, 1])
}

pub fn dropout_device(input: &[f32], p: f32, seed: u64) -> Result<Vec<f32>, GpuError> {
    if !(0.0..1.0).contains(&p) {
        return Err(GpuError::new(GpuErrorKind::InvalidArgument, "dropout p must be in [0,1)"));
    }
    let scale = if p >= 1.0 { 0.0 } else { 1.0 / (1.0 - p) };
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> inp: array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
fn splitmix64(state: ptr<function, u64>) -> u64 {{
    var z = *state + 0x9E3779B97F4A7C15u;
    *state = z;
    z = (z ^ (z >> 30u)) * 0xBF58476D1CE4E5B9u;
    z = (z ^ (z >> 27u)) * 0x94D049BB133111EBu;
    return z ^ (z >> 31u);
}}
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let idx = id.x;
    if (idx >= {len}u) {{ return; }}
    var s = {seed}u + idx * 0x9E3779B97F4A7C15u;
    let r = splitmix64(&s);
    let prob = f32((r >> 11u) & 0x1FFFFFu) / 2097152.0;
    if (prob < {p}f) {{ out[idx] = 0.0; }} else {{ out[idx] = inp[idx] * {scale}f; }}
}}
"#,
        len = input.len(), p = p, scale = scale, seed = seed
    );
    dispatch_one_input(input, input.len(), &shader, [(input.len() as u32 + 63) / 64 * 64, 1, 1])
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

