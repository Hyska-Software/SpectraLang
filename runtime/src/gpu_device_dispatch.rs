// ===== R-3052 full: device-buffer dispatchers (no readback in hot path) =====
//
// Each `*_device` variant binds pre-acquired `DeviceBuffer`s from the
// pool, runs the same WGSL as the host-materializing counterpart, and
// submits without copy-to-readback. The caller owns the output buffer
// and is responsible for storing it on the new tensor's
// `device_storage`. Acquire happens inside the `with_device_queue`
// closure so the GPU context lock is held for the whole sequence.

/// Device-binary: `out[i] = left[i] <op> right[i]`. Inputs and output
/// are pool buffers; no host readback.
pub fn binary_device(
    left: &DeviceBuffer,
    right: &DeviceBuffer,
    op: GpuBinaryOp,
    out: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    if left.elements != right.elements || out.elements != left.elements {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu binary_device shape mismatch",
        ));
    }
    let len = left.elements;
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
        len = len,
        expr = match op {
            GpuBinaryOp::Add => "left[i] + right[i]",
            GpuBinaryOp::Sub => "left[i] - right[i]",
            GpuBinaryOp::Mul => "left[i] * right[i]",
            GpuBinaryOp::Div => "select(left[i] / right[i], 0.0, right[i] == 0.0)",
        }
    );
    run_compute_no_readback(
        device,
        queue,
        &shader,
        &[
            (0, &left.buffer, true),
            (1, &right.buffer, true),
            (2, &out.buffer, false),
        ],
        [len as u32, 1, 1],
    )
}

/// Device-unary: `out[i] = <op>(input[i])`. Inputs and output are pool
/// buffers; no host readback.
pub fn unary_device(
    input: &DeviceBuffer,
    op: GpuUnaryOp,
    out: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    if out.elements != input.elements {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu unary_device shape mismatch",
        ));
    }
    let len = input.elements;
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
        len = len,
        expr = match op {
            GpuUnaryOp::Neg => "-input_values[i]",
            GpuUnaryOp::Relu => "max(input_values[i], 0.0)",
        }
    );
    run_compute_no_readback(
        device,
        queue,
        &shader,
        &[(0, &input.buffer, true), (1, &out.buffer, false)],
        [len as u32, 1, 1],
    )
}

/// Device-matmul: `out[i, j] = sum_k left[i, k] * right[k, j]`. Pool
/// buffers in and out; no host readback.
pub fn matmul_device(
    left: &DeviceBuffer,
    right: &DeviceBuffer,
    m: usize,
    k: usize,
    n: usize,
    out: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    if left.elements != m.saturating_mul(k)
        || right.elements != k.saturating_mul(n)
        || out.elements != m.saturating_mul(n)
    {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu matmul_device shape mismatch",
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
    run_compute_no_readback(
        device,
        queue,
        &shader,
        &[
            (0, &left.buffer, true),
            (1, &right.buffer, true),
            (2, &out.buffer, false),
        ],
        [(m * n) as u32, 1, 1],
    )
}

/// Device-conv2d: standard 2D convolution with bias. Pool buffers in
/// and out; no host readback.
pub fn conv2d_device(
    input: &DeviceBuffer,
    kernel: &DeviceBuffer,
    bias: &DeviceBuffer,
    dims: [usize; 7],
    out: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    let [batch, in_ch, h, w, out_ch, kh, kw] = dims;
    if h < kh || w < kw {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu conv2d_device invalid kernel dimensions",
        ));
    }
    let out_h = h - kh + 1;
    let out_w = w - kw + 1;
    let out_len = batch * out_ch * out_h * out_w;
    if input.elements != batch * in_ch * h * w
        || kernel.elements != out_ch * in_ch * kh * kw
        || bias.elements != out_ch
        || out.elements != out_len
    {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu conv2d_device shape mismatch",
        ));
    }
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
    run_compute_no_readback(
        device,
        queue,
        &shader,
        &[
            (0, &input.buffer, true),
            (1, &kernel.buffer, true),
            (2, &bias.buffer, true),
            (3, &out.buffer, false),
        ],
        [out_len as u32, 1, 1],
    )
}

/// Device-sum: parallel two-stage tree reduction writing 1 f32 into
/// `out`. Stage one (`reduce`, workgroup_size 256) folds a strided tile
/// per workgroup through an 8-step shared-memory tree and emits one
/// partial per workgroup; stage two (`final_reduce`) folds the partials
/// into the final scalar. A scratch partials buffer is allocated for
/// the submission and dropped right after — `queue.submit` retains it
/// for the duration of the GPU work. No intermediate readback: the
/// caller reads only this final scalar (the one allowed readback in
/// the hot path). Without a GPU adapter the context error propagates
/// and callers fall back to their counted CPU path.
pub fn sum_device(
    input: &DeviceBuffer,
    out: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    if input.elements == 0 || out.elements != 1 {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu sum_device shape mismatch",
        ));
    }
    let (workgroups, total_threads, partials_count) = reduction_plan(input.elements);
    let shader = reduction_shader(input.elements, total_threads, partials_count);
    let partials = Arc::new(device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("spectra-runtime-gpu-sum-partials"),
        size: u64::from(partials_count) * std::mem::size_of::<f32>() as u64,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    }));
    run_compute_two_stage_no_readback(
        device,
        queue,
        &shader,
        &[
            (0, &input.buffer, true),
            (1, &partials, false),
            (2, &out.buffer, false),
        ],
        ("reduce", [workgroups, 1, 1]),
        ("final_reduce", [1, 1, 1]),
    )
}

/// Device column sum: for a row-major matrix `input[rows, cols]`, writes
/// `out[col] = sum_row input[row, col]`. Used for `ml.linear` bias grads.
pub fn sum_columns_device(
    input: &DeviceBuffer,
    rows: usize,
    cols: usize,
    out: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    if rows == 0 || cols == 0 || input.elements != rows.saturating_mul(cols) || out.elements != cols
    {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu sum_columns_device shape mismatch",
        ));
    }
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> input_values: array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let col = id.x;
    if (col >= {cols}u) {{
        return;
    }}
    var acc = 0.0;
    for (var row = 0u; row < {rows}u; row = row + 1u) {{
        acc = acc + input_values[row * {cols}u + col];
    }}
    out[col] = acc;
}}
"#,
        rows = rows,
        cols = cols
    );
    run_compute_no_readback(
        device,
        queue,
        &shader,
        &[(0, &input.buffer, true), (1, &out.buffer, false)],
        [cols as u32, 1, 1],
    )
}

/// Device MSE loss: writes one scalar `mean((prediction-target)^2)` into `out`.
pub fn mse_loss_device(
    prediction: &DeviceBuffer,
    target: &DeviceBuffer,
    out: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    if prediction.elements == 0 || prediction.elements != target.elements || out.elements != 1 {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu mse_loss_device shape mismatch",
        ));
    }
    let len = prediction.elements;
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> prediction: array<f32>;
@group(0) @binding(1) var<storage, read> target_values: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main() {{
    var acc = 0.0;
    for (var i = 0u; i < {len}u; i = i + 1u) {{
        let diff = prediction[i] - target_values[i];
        acc = acc + diff * diff;
    }}
    out[0] = acc / f32({len}u);
}}
"#,
        len = len
    );
    run_compute_no_readback(
        device,
        queue,
        &shader,
        &[
            (0, &prediction.buffer, true),
            (1, &target.buffer, true),
            (2, &out.buffer, false),
        ],
        [1, 1, 1],
    )
}

/// Device MSE backward: `out[i] = seed * 2 * (prediction[i]-target[i]) / len`.
pub fn mse_backward_device(
    prediction: &DeviceBuffer,
    target: &DeviceBuffer,
    seed: &DeviceBuffer,
    out: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    if prediction.elements == 0
        || prediction.elements != target.elements
        || seed.elements != 1
        || out.elements != prediction.elements
    {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu mse_backward_device shape mismatch",
        ));
    }
    let len = prediction.elements;
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> prediction: array<f32>;
@group(0) @binding(1) var<storage, read> target_values: array<f32>;
@group(0) @binding(2) var<storage, read> seed: array<f32>;
@group(0) @binding(3) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let i = id.x;
    if (i >= {len}u) {{
        return;
    }}
    out[i] = seed[0] * 2.0 * (prediction[i] - target_values[i]) / f32({len}u);
}}
"#,
        len = len
    );
    run_compute_no_readback(
        device,
        queue,
        &shader,
        &[
            (0, &prediction.buffer, true),
            (1, &target.buffer, true),
            (2, &seed.buffer, true),
            (3, &out.buffer, false),
        ],
        [len as u32, 1, 1],
    )
}

/// Device add-bias: `out[i, j] += bias[j]`. Used by `ml.linear` to add
/// the bias in-place on the matmul output buffer. Pool buffers in and
/// out.
pub fn add_bias_device(
    matmul_out: &DeviceBuffer,
    bias: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    if matmul_out.elements == 0 || bias.elements == 0 || matmul_out.elements % bias.elements != 0 {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu add_bias_device shape mismatch",
        ));
    }
    let total = matmul_out.elements;
    let cols = bias.elements;
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> bias_values: array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let i = id.x;
    if (i >= {total}u) {{
        return;
    }}
    out[i] = out[i] + bias_values[i % {cols}u];
}}
"#,
        total = total,
        cols = cols
    );
    run_compute_no_readback(
        device,
        queue,
        &shader,
        &[(0, &bias.buffer, true), (1, &matmul_out.buffer, false)],
        [total as u32, 1, 1],
    )
}

/// Device add-into: `dst[i] += src[i]`. Used for device-grad
/// accumulation in the autograd path.
pub fn add_into_device(
    dst: &DeviceBuffer,
    src: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    if dst.elements != src.elements {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu add_into_device shape mismatch",
        ));
    }
    let len = dst.elements;
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> src: array<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let i = id.x;
    if (i >= {len}u) {{
        return;
    }}
    dst[i] = dst[i] + src[i];
}}
"#,
        len = len
    );
    run_compute_no_readback(
        device,
        queue,
        &shader,
        &[(0, &src.buffer, true), (1, &dst.buffer, false)],
        [len as u32, 1, 1],
    )
}

/// Device SGD update: `out[i] = param[i] - lr * grad[i]`. The caller
/// typically passes `out == param` for an in-place overwrite; the
/// underlying `Arc<wgpu::Buffer>` is kept alive so any other holder
/// sees the updated values after the next submit.
pub fn sgd_update_device(
    param: &DeviceBuffer,
    grad: &DeviceBuffer,
    lr: f32,
    out: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    if param.elements != grad.elements || out.elements != param.elements {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu sgd_update_device shape mismatch",
        ));
    }
    let len = param.elements;
    if Arc::ptr_eq(&param.buffer, &out.buffer) {
        let shader = format!(
            r#"
@group(0) @binding(0) var<storage, read_write> param: array<f32>;
@group(0) @binding(1) var<storage, read> grad: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let i = id.x;
    if (i >= {len}u) {{
        return;
    }}
    param[i] = param[i] - {lr}f * grad[i];
}}
"#,
            len = len,
            lr = lr
        );
        return run_compute_no_readback(
            device,
            queue,
            &shader,
            &[(0, &param.buffer, false), (1, &grad.buffer, true)],
            [len as u32, 1, 1],
        );
    }
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> param: array<f32>;
@group(0) @binding(1) var<storage, read> grad: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let i = id.x;
    if (i >= {len}u) {{
        return;
    }}
    out[i] = param[i] - {lr}f * grad[i];
}}
"#,
        len = len,
        lr = lr
    );
    run_compute_no_readback(
        device,
        queue,
        &shader,
        &[
            (0, &param.buffer, true),
            (1, &grad.buffer, true),
            (2, &out.buffer, false),
        ],
        [len as u32, 1, 1],
    )
}

/// Device backward matmul-left: `out[i, j] = sum_l grad[i, l] * right[j, l]`
/// for `C = A @ B`. Pool buffers; no readback.
pub fn backward_matmul_left_device(
    grad: &DeviceBuffer,
    right: &DeviceBuffer,
    m: usize,
    k: usize,
    n: usize,
    out: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    if grad.elements != m.saturating_mul(n)
        || right.elements != k.saturating_mul(n)
        || out.elements != m.saturating_mul(k)
    {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu backward_matmul_left_device shape mismatch",
        ));
    }
    if m == 0 || k == 0 {
        return Ok(());
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
    run_compute_no_readback(
        device,
        queue,
        &shader,
        &[
            (0, &grad.buffer, true),
            (1, &right.buffer, true),
            (2, &out.buffer, false),
        ],
        [(m * k) as u32, 1, 1],
    )
}

/// Device backward matmul-right: `out[j, l] = sum_i left[i, j] * grad[i, l]`
/// for `C = A @ B`. Pool buffers; no readback.
pub fn backward_matmul_right_device(
    left: &DeviceBuffer,
    grad: &DeviceBuffer,
    m: usize,
    k: usize,
    n: usize,
    out: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    if left.elements != m.saturating_mul(k)
        || grad.elements != m.saturating_mul(n)
        || out.elements != k.saturating_mul(n)
    {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu backward_matmul_right_device shape mismatch",
        ));
    }
    if k == 0 || n == 0 {
        return Ok(());
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
    run_compute_no_readback(
        device,
        queue,
        &shader,
        &[
            (0, &left.buffer, true),
            (1, &grad.buffer, true),
            (2, &out.buffer, false),
        ],
        [(k * n) as u32, 1, 1],
    )
}

/// Device ML linear input grad: for `Y = X[batch,in] @ W[in,out] + b`,
/// writes `out_grad_input[batch,in] = grad[batch,out] @ W^T` while reading
/// `W` in its normal row-major `[in,out]` layout.
pub fn linear_grad_input_device(
    grad: &DeviceBuffer,
    weight: &DeviceBuffer,
    batch: usize,
    in_features: usize,
    out_features: usize,
    out: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    if grad.elements != batch.saturating_mul(out_features)
        || weight.elements != in_features.saturating_mul(out_features)
        || out.elements != batch.saturating_mul(in_features)
    {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu linear_grad_input_device shape mismatch",
        ));
    }
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> grad_values: array<f32>;
@group(0) @binding(1) var<storage, read> weight_values: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let index = id.x;
    let total = {batch}u * {in_features}u;
    if (index >= total) {{
        return;
    }}
    let row = index / {in_features}u;
    let in_col = index % {in_features}u;
    var acc = 0.0;
    for (var out_col = 0u; out_col < {out_features}u; out_col = out_col + 1u) {{
        acc = acc + grad_values[row * {out_features}u + out_col] * weight_values[in_col * {out_features}u + out_col];
    }}
    out[index] = acc;
}}
"#,
        batch = batch,
        in_features = in_features,
        out_features = out_features
    );
    run_compute_no_readback(
        device,
        queue,
        &shader,
        &[
            (0, &grad.buffer, true),
            (1, &weight.buffer, true),
            (2, &out.buffer, false),
        ],
        [(batch * in_features) as u32, 1, 1],
    )
}

/// Device backward relu: `out[i] = grad[i] if output[i] > 0 else 0`.
/// Pool buffers; no readback.
pub fn backward_relu_device(
    grad: &DeviceBuffer,
    output: &DeviceBuffer,
    out: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    if grad.elements != output.elements || out.elements != grad.elements {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu backward_relu_device shape mismatch",
        ));
    }
    if grad.elements == 0 {
        return Ok(());
    }
    let len = grad.elements;
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
        len = len
    );
    run_compute_no_readback(
        device,
        queue,
        &shader,
        &[
            (0, &grad.buffer, true),
            (1, &output.buffer, true),
            (2, &out.buffer, false),
        ],
        [len as u32, 1, 1],
    )
}

/// Upload a 1-element f32 to a 1-element pool buffer. Used to promote
/// the host scalar backward seed (the loss gradient) to a device
/// buffer so the GPU backward path can consume it without a per-step
/// readback in the chained backward.
pub fn upload_scalar_device(
    value: f32,
    out: &DeviceBuffer,
    _device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), GpuError> {
    if out.elements != 1 {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu upload_scalar_device requires a 1-element buffer",
        ));
    }
    queue.write_buffer(&out.buffer, 0, bytemuck::cast_slice(&[value]));
    Ok(())
}

/// Read back a 1-element pool buffer as an f32. Used by the GPU sum
/// path and by tests that need to inspect a device scalar.
pub fn readback_scalar_device(
    buf: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<f32, GpuError> {
    if buf.elements != 1 {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu readback_scalar_device requires a 1-element buffer",
        ));
    }
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("spectra-runtime-gpu-scalar-readback"),
        size: std::mem::size_of::<f32>() as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("spectra-runtime-gpu-scalar-readback-encoder"),
    });
    encoder.copy_buffer_to_buffer(
        &buf.buffer,
        0,
        &readback,
        0,
        std::mem::size_of::<f32>() as u64,
    );
    queue.submit(Some(encoder.finish()));
    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device.poll(wgpu::Maintain::Wait);
    receiver
        .recv()
        .map_err(|_| GpuError::new(GpuErrorKind::Readback, "scalar readback callback failed"))?
        .map_err(|err| {
            GpuError::new(
                GpuErrorKind::Readback,
                format!("scalar readback failed: {err:?}"),
            )
        })?;
    let mapped = slice.get_mapped_range();
    let value: f32 = *bytemuck::from_bytes(&mapped);
    drop(mapped);
    readback.unmap();
    Ok(value)
}

/// Read back an N-element pool buffer as a `Vec<f32>`. Used by tests
/// that need to inspect device-side values for correctness
/// assertions. Not part of the hot path.
pub fn readback_device_f32(
    buf: &DeviceBuffer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<Vec<f32>, GpuError> {
    if buf.elements == 0 {
        return Ok(Vec::new());
    }
    let size = (buf.elements * std::mem::size_of::<f32>()) as u64;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("spectra-runtime-gpu-test-readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("spectra-runtime-gpu-test-readback-encoder"),
    });
    encoder.copy_buffer_to_buffer(&buf.buffer, 0, &readback, 0, size);
    queue.submit(Some(encoder.finish()));
    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device.poll(wgpu::Maintain::Wait);
    receiver
        .recv()
        .map_err(|_| GpuError::new(GpuErrorKind::Readback, "test readback callback failed"))?
        .map_err(|err| {
            GpuError::new(
                GpuErrorKind::Readback,
                format!("test readback failed: {err:?}"),
            )
        })?;
    let mapped = slice.get_mapped_range();
    let values = bytemuck::cast_slice(&mapped).to_vec();
    drop(mapped);
    readback.unmap();
    Ok(values)
}
