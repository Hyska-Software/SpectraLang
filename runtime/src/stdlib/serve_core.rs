#[derive(Clone, PartialEq)]
enum ServeRequestState {
    Pending,
    /// Completed with the scalar projection plus the full float output vector.
    Complete(SpectraHostValue, Vec<f64>),
    Cancelled,
}

struct ServeHandleTable<T> {
    table: HandleTable<T>,
}

impl<T> ServeHandleTable<T> {
    fn new(kind: HandleKind) -> Self {
        Self {
            table: HandleTable::new(kind),
        }
    }

    fn insert(&mut self, value: T) -> SpectraHostValue {
        self.table.insert(value).raw()
    }

    fn get(&self, raw: SpectraHostValue) -> Option<&T> {
        let handle = HandleId::from_raw(raw).ok()?;
        self.table.get(handle).ok()
    }

    fn get_mut(&mut self, raw: SpectraHostValue) -> Option<&mut T> {
        let handle = HandleId::from_raw(raw).ok()?;
        self.table.get_mut(handle).ok()
    }

    fn clear(&mut self) {
        self.table.clear();
    }
}

/// Activation applied to a served dense layer's outputs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ServeActivation {
    Relu,
    Gelu,
    Tanh,
    Softmax,
}

/// One fully connected layer of a served model: `y = act(x @ W^T + b)`.
struct ServeLinearLayer {
    /// Row-major `[out][in]` weight matrix.
    weights: Vec<Vec<f64>>,
    biases: Vec<f64>,
    activation: ServeActivation,
}

/// Real model behind a serve server: an explicit chain of dense layers or a
/// committed onnxruntime session reused through `ml_onnx_run_inner`.
#[cfg_attr(not(feature = "onnx"), allow(dead_code))]
enum ServeModel {
    Linear(Vec<ServeLinearLayer>),
    Onnx(u64),
}

struct ServeServer {
    model: SpectraHostValue,
    served_model: Option<ServeModel>,
    model_version: String,
    warm: bool,
    timeout: SpectraHostValue,
    queue: VecDeque<SpectraHostValue>,
    input_policy: Option<(SpectraHostValue, SpectraHostValue)>,
    output_policy: Option<(SpectraHostValue, SpectraHostValue)>,
    rate_limit: Option<SpectraHostValue>,
    accepted_requests: SpectraHostValue,
    fallback: SpectraHostValue,
    last_diagnostic: String,
    audit_events: Vec<String>,
    total_requests: SpectraHostValue,
    completed_requests: SpectraHostValue,
    blocked_requests: SpectraHostValue,
    cancelled_requests: SpectraHostValue,
    error_count: SpectraHostValue,
    batch_count: SpectraHostValue,
    latency_samples_ms: Vec<f64>,
    observed_inputs: Vec<f64>,
    observed_outputs: Vec<f64>,
}

struct ServeRegistry {
    servers: ServeHandleTable<ServeServer>,
    requests: ServeHandleTable<(SpectraHostValue, ServeRequestState)>,
}

impl ServeRegistry {
    fn new() -> Self {
        Self {
            servers: ServeHandleTable::new(HandleKind::ServeServer),
            requests: ServeHandleTable::new(HandleKind::ServeRequest),
        }
    }

    fn clear(&mut self) {
        self.servers.clear();
        self.requests.clear();
    }
}

fn serve_registry() -> &'static Mutex<ServeRegistry> {
    static REGISTRY: OnceLock<Mutex<ServeRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(ServeRegistry::new()))
}

fn lock_serve_registry() -> Result<std::sync::MutexGuard<'static, ServeRegistry>, i32> {
    serve_registry()
        .lock()
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)
}

fn serve_guardrail_diagnostic(
    request_id: SpectraHostValue,
    stage: &str,
    policy: &str,
    value: SpectraHostValue,
    min: SpectraHostValue,
    max: SpectraHostValue,
    fallback: SpectraHostValue,
) -> String {
    format!(
        "{{\"schema\":\"spectra.serve.guardrail_diagnostic.v1\",\"request\":{},\"stage\":\"{}\",\"policy\":\"{}\",\"value\":{},\"min\":{},\"max\":{},\"fallback\":{}}}",
        request_id, stage, policy, value, min, max, fallback
    )
}

fn serve_audit_event(
    request_id: SpectraHostValue,
    event: &str,
    stage: &str,
    value: SpectraHostValue,
    result: SpectraHostValue,
) -> String {
    format!(
        "{{\"request\":{},\"event\":\"{}\",\"stage\":\"{}\",\"value\":{},\"result\":{}}}",
        request_id, event, stage, value, result
    )
}

fn serve_audit_json(server: &ServeServer) -> String {
    format!(
        "{{\"schema\":\"spectra.serve.audit.v1\",\"model\":{},\"warm\":{},\"accepted_requests\":{},\"events\":[{}]}}",
        server.model,
        i64::from(server.warm),
        server.accepted_requests,
        server.audit_events.join(",")
    )
}


fn serve_record_block(server: &mut ServeServer, output: SpectraHostValue) {
    server.blocked_requests = server.blocked_requests.saturating_add(1);
    server.error_count = server.error_count.saturating_add(1);
    server.observed_outputs.push(output as f64);
}

/// Records a completed request together with its MEASURED inference latency
/// (wall-clock `std::time::Instant` delta around the forward pass). No
/// synthetic latency formula exists anymore.
fn serve_record_complete(
    server: &mut ServeServer,
    output_first: f64,
    latency_ms: f64,
) {
    server.completed_requests = server.completed_requests.saturating_add(1);
    server.observed_outputs.push(output_first);
    server.latency_samples_ms.push(latency_ms);
}

fn serve_values_summary(values: &[f64]) -> (f64, f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let sum = values.iter().sum::<f64>();
    (min, max, sum / values.len() as f64)
}

fn serve_p95(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((sorted.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    sorted[index.min(sorted.len() - 1)]
}

/// Extracts a `"samples":[...]` float array from a JSON document, starting
/// the search at `section_marker` so nested objects (inputs vs outputs) do
/// not collide.
fn serve_json_samples_array(source: &str, section_marker: &str) -> Option<Vec<f64>> {
    const ARRAY_KEY: &str = "\"samples\":[";
    let section_start = source.find(section_marker)?;
    let tail = &source[section_start..];
    let start = tail.find(ARRAY_KEY)? + ARRAY_KEY.len();
    let rest = &tail[start..];
    let end = rest.find(']')?;
    let mut out = Vec::new();
    for item in rest[..end].split(',') {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        out.push(trimmed.parse::<f64>().ok()?);
    }
    Some(out)
}


fn serve_json_f64_array(values: &[f64]) -> String {
    let items = values
        .iter()
        .map(|value| ml_float_json(*value))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", items)
}

fn serve_distribution_summary_json(server: &ServeServer) -> String {
    let (input_min, input_max, input_mean) = serve_values_summary(&server.observed_inputs);
    let (output_min, output_max, output_mean) = serve_values_summary(&server.observed_outputs);
    format!(
        "{{\"schema\":\"spectra.serve.distribution_summary.v1\",\"model_version\":{},\"inputs\":{{\"count\":{},\"min\":{},\"max\":{},\"mean\":{},\"samples\":{}}},\"outputs\":{{\"count\":{},\"min\":{},\"max\":{},\"mean\":{},\"samples\":{}}}}}",
        ml_json_string(&server.model_version),
        server.observed_inputs.len(),
        ml_float_json(input_min),
        ml_float_json(input_max),
        ml_float_json(input_mean),
        serve_json_f64_array(&server.observed_inputs),
        server.observed_outputs.len(),
        ml_float_json(output_min),
        ml_float_json(output_max),
        ml_float_json(output_mean),
        serve_json_f64_array(&server.observed_outputs)
    )
}

fn serve_monitoring_snapshot_json(server: &ServeServer) -> String {
    let total_latency_ms = server.latency_samples_ms.iter().sum::<f64>();
    let latency_avg = if server.latency_samples_ms.is_empty() {
        0.0
    } else {
        total_latency_ms / server.latency_samples_ms.len() as f64
    };
    let error_rate = if server.total_requests <= 0 {
        0.0
    } else {
        server.error_count as f64 / server.total_requests as f64
    };
    let throughput = if total_latency_ms <= 0.0 {
        0.0
    } else {
        server.completed_requests as f64 / (total_latency_ms / 1000.0)
    };
    format!(
        "{{\"schema\":\"spectra.serve.monitoring_snapshot.v1\",\"model\":{},\"model_version\":{},\"requests\":{},\"completed\":{},\"blocked\":{},\"cancelled\":{},\"errors\":{},\"error_rate\":{},\"batches\":{},\"pending\":{},\"latency_avg_ms\":{},\"latency_p95_ms\":{},\"throughput_per_second\":{}}}",
        server.model,
        ml_json_string(&server.model_version),
        server.total_requests,
        server.completed_requests,
        server.blocked_requests,
        server.cancelled_requests,
        server.error_count,
        ml_float_json(error_rate),
        server.batch_count,
        server.queue.len(),
        ml_float_json(latency_avg),
        ml_float_json(serve_p95(&server.latency_samples_ms)),
        ml_float_json(throughput)
    )
}

fn serve_json_number(source: &str, key: &str) -> Option<f64> {
    let start = source.find(key)? + key.len();
    let tail = &source[start..];
    let end =
        tail.find(|ch: char| !(ch.is_ascii_digit() || ch == '-' || ch == '.' || ch == '+'))?;
    tail[..end].parse::<f64>().ok()
}

/// Population Stability Index between reference and live samples.
///
/// Each side is histogrammed into 10 equal-width bins spanning the joint
/// `[min, max]` of both samples. With bin percentages `p_i` (reference) and
/// `q_i` (live), the score is
///
/// PSI = Σ_i (q_i − p_i) · ln((q_i + ε) / (p_i + ε)),  ε = 1e-6
///
/// The epsilon keeps empty bins finite; identical histograms score exactly 0.
/// Returns `None` when either side has no samples.
fn serve_psi(reference: &[f64], live: &[f64]) -> Option<f64> {
    const BINS: usize = 10;
    const EPSILON: f64 = 1e-6;
    if reference.is_empty() || live.is_empty() {
        return None;
    }
    let lo = reference
        .iter()
        .chain(live.iter())
        .cloned()
        .fold(f64::INFINITY, f64::min);
    let hi = reference
        .iter()
        .chain(live.iter())
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);
    if !(hi > lo) {
        // Constant joint range: both histograms are a single spike in the
        // same bin, so there is nothing to compare.
        return Some(0.0);
    }
    let width = (hi - lo) / BINS as f64;
    let mut ref_counts = vec![0.0f64; BINS];
    let mut live_counts = vec![0.0f64; BINS];
    let bin_of = |value: f64| -> usize {
        (((value - lo) / width) as usize).min(BINS - 1)
    };
    for value in reference {
        ref_counts[bin_of(*value)] += 1.0;
    }
    for value in live {
        live_counts[bin_of(*value)] += 1.0;
    }
    let mut psi = 0.0;
    for index in 0..BINS {
        let p = ref_counts[index] / reference.len() as f64;
        let q = live_counts[index] / live.len() as f64;
        psi += (q - p) * ((q + EPSILON) / (p + EPSILON)).ln();
    }
    Some(psi)
}

fn serve_drift_json(
    reference: &str,
    live: &str,
    threshold_per_mille: SpectraHostValue,
) -> Option<String> {
    let ref_in = serve_json_number(reference, "\"inputs\":{\"count\":")?;
    let live_in = serve_json_number(live, "\"inputs\":{\"count\":")?;
    let ref_inputs = serve_json_samples_array(reference, "\"inputs\"")?;
    let live_inputs = serve_json_samples_array(live, "\"inputs\"")?;
    // The outputs section also carries a samples array; slice it off first so
    // the lookup cannot land on the inputs histogram.
    let ref_outputs_offset = reference.find("\"outputs\"")?;
    let live_outputs_offset = live.find("\"outputs\"")?;
    let ref_outputs =
        serve_json_samples_array(&reference[ref_outputs_offset..], "\"outputs\"")?;
    let live_outputs = serve_json_samples_array(&live[live_outputs_offset..], "\"outputs\"")?;
    let input_psi = serve_psi(&ref_inputs, &live_inputs)?;
    let output_psi = serve_psi(&ref_outputs, &live_outputs)?;
    // Total drift score across both features, reported per mille of the raw
    // PSI so the existing per-mille threshold keeps its scale.
    let score = (((input_psi + output_psi) * 1000.0).round().max(0.0)) as SpectraHostValue;
    let drifted = score > threshold_per_mille;
    Some(format!(
        "{{\"schema\":\"spectra.serve.drift_check.v1\",\"reference_count\":{},\"live_count\":{},\"input_psi\":{},\"output_psi\":{},\"score_per_mille\":{},\"threshold_per_mille\":{},\"drifted\":{}}}",
        ref_in as i64,
        live_in as i64,
        ml_float_json(input_psi),
        ml_float_json(output_psi),
        score,
        threshold_per_mille,
        if drifted { "true" } else { "false" }
    ))
}

fn serve_apply_activation(activation: ServeActivation, values: &[f64]) -> Option<Vec<f64>> {
    match activation {
        ServeActivation::Relu => Some(values.iter().map(|value| value.max(0.0)).collect()),
        ServeActivation::Tanh => Some(values.iter().map(|value| value.tanh()).collect()),
        ServeActivation::Gelu => Some(values.iter().map(|value| {
            // Tanh approximation of GELU.
            0.5 * value
                * (1.0
                    + (std::f64::consts::FRAC_2_SQRT_PI
                        * (value + 0.044715 * value * value * value))
                    .tanh())
        }).collect()),
        ServeActivation::Softmax => ml_softmax_row(values),
    }
}

/// Real dense-chain forward pass: `x @ W^T + b` followed by the layer's
/// activation, applied layer by layer starting from the scalar request input
/// as a length-1 vector.
fn serve_forward_linear(
    layers: &[ServeLinearLayer],
    input: SpectraHostValue,
) -> Result<Vec<f64>, i32> {
    let mut current = vec![input as f64];
    for layer in layers {
        if layer.biases.len() != layer.weights.len() {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }
        let mut next: Vec<f64> = Vec::with_capacity(layer.weights.len());
        for row in &layer.weights {
            if row.len() != current.len() {
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            let mut acc = 0.0;
            for (weight, value) in row.iter().zip(current.iter()) {
                acc += weight * value;
            }
            next.push(acc);
        }
        for (bias, value) in layer.biases.iter().zip(next.iter_mut()) {
            *value += bias;
        }
        current = serve_apply_activation(layer.activation, &next)
            .ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
    }
    Ok(current)
}

/// ONNX forward pass delegating to the committed onnxruntime session through
/// `ml_onnx_run_inner`, exactly like `spectra.std.ml.onnx_run`. The scalar
/// request input becomes a length-1 float tensor; scratch tensors are
/// released after extraction.
#[cfg(feature = "onnx")]
fn serve_forward_onnx(session_id: u64, input: SpectraHostValue) -> Result<Vec<f64>, i32> {
    // Copy the output name out before running: the borrowed session must not
    // overlap with the mutable lock that `ml_onnx_run_inner` takes.
    let output_name = {
        let mut sessions = ml_onnx_sessions_lock();
        let session = sessions.get_mut(&session_id).ok_or(HOST_STATUS_NOT_FOUND)?;
        if session.inputs().len() != 1 || session.outputs().is_empty() {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }
        session.outputs()[0].name().to_owned()
    };
    let input_handle = ml_alloc_float_tensor(vec![1], vec![input as f64])?;
    let mut output_handle: Option<usize> = None;
    let result = match ml_onnx_run_inner(session_id, input_handle, &output_name) {
        Ok(handle) => {
            let (_, values, _) =
                ml_tensor_float_data(handle).ok_or(HOST_STATUS_INTERNAL_ERROR)?;
            output_handle = Some(handle);
            Ok(values)
        }
        Err(status) => Err(status),
    };
    // Release the scratch tensors so repeated serving does not grow the
    // stdlib tensor registry.
    with_tensor_registry(|registry| {
        let _ = registry.remove(input_handle);
        if let Some(handle) = output_handle {
            let _ = registry.remove(handle);
        }
    });
    result
}

/// Without the `onnx` feature no real session can exist, so serving an ONNX
/// model is rejected instead of being simulated.
#[cfg(not(feature = "onnx"))]
fn serve_forward_onnx(_session_id: u64, _input: SpectraHostValue) -> Result<Vec<f64>, i32> {
    Err(HOST_STATUS_INVALID_ARGUMENT)
}

/// Runs one REAL inference request against the served model and returns the
/// output vector together with its measured latency in milliseconds.
fn serve_infer(server: &ServeServer, input: SpectraHostValue) -> Result<(Vec<f64>, f64), i32> {
    let start = std::time::Instant::now();
    let output = match &server.served_model {
        Some(ServeModel::Linear(layers)) => serve_forward_linear(layers, input)?,
        Some(ServeModel::Onnx(session_id)) => serve_forward_onnx(*session_id, input)?,
        None => return Err(HOST_STATUS_INVALID_ARGUMENT),
    };
    let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok((output, latency_ms))
}

/// Scalar projection of an output vector for the integer result ABI:
/// first component rounded to nearest i64 (saturating).
fn serve_scalar_result(output: &[f64]) -> SpectraHostValue {
    output.first().map(|value| value.round() as i64).unwrap_or(0)
}
