use super::*;
#[derive(Clone, PartialEq)]
pub(crate) enum ServeRequestState {
    Pending,
    /// Completed with the scalar projection plus the full float output vector.
    Complete(SpectraHostValue, Vec<f64>),
    /// Rejected by a guardrail before or during inference. Reads as the
    /// configured fallback value (default `-1`, indistinguishable from
    /// pending/cancelled by value alone); the guardrail diagnostic, audit
    /// event, and `blocked_requests` counter carry the reason. Never queued.
    Blocked {
        fallback: SpectraHostValue,
        reason: &'static str,
    },
    Cancelled,
}

pub(crate) struct ServeHandleTable<T> {
    table: HandleTable<T>,
}

impl<T> ServeHandleTable<T> {
    pub(crate) fn new(kind: HandleKind) -> Self {
        Self {
            table: HandleTable::new(kind),
        }
    }

    pub(crate) fn insert(&mut self, value: T) -> SpectraHostValue {
        self.table.insert(value).raw()
    }

    pub(crate) fn get(&self, raw: SpectraHostValue) -> Option<&T> {
        let handle = HandleId::from_raw(raw).ok()?;
        self.table.get(handle).ok()
    }

    pub(crate) fn get_mut(&mut self, raw: SpectraHostValue) -> Option<&mut T> {
        let handle = HandleId::from_raw(raw).ok()?;
        self.table.get_mut(handle).ok()
    }

    pub(crate) fn clear(&mut self) {
        self.table.clear();
    }
}

/// Activation applied to a served dense layer's outputs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ServeActivation {
    Relu,
    Gelu,
    Tanh,
    Softmax,
}

/// One fully connected layer of a served model: `y = act(x @ W^T + b)`.
pub(crate) struct ServeLinearLayer {
    /// Row-major `[out][in]` weight matrix.
    pub(crate) weights: Vec<Vec<f64>>,
    pub(crate) biases: Vec<f64>,
    pub(crate) activation: ServeActivation,
}

/// Real model behind a serve server: an explicit chain of dense layers or a
/// committed onnxruntime session reused through `ml_onnx_run_inner`.
#[cfg_attr(not(feature = "onnx"), allow(dead_code))]
pub(crate) enum ServeModel {
    Linear(Vec<ServeLinearLayer>),
    Onnx(u64),
}

/// Model id used by the legacy single-model registration/inference hosts
/// (`server_register_model_linear`, the enqueue/process-batch pipeline and a
/// `/infer` request without an explicit `"model"` field).
pub(crate) const SERVE_DEFAULT_MODEL_ID: &str = "default";

/// Monitoring counters tracked separately for every registered `model_id`
/// (see `serve_monitoring_snapshot_json`'s `"models"` sections).
#[derive(Clone, Default)]
pub(crate) struct ServeModelMetrics {
    pub(crate) requests: SpectraHostValue,
    pub(crate) completed_requests: SpectraHostValue,
    pub(crate) blocked_requests: SpectraHostValue,
    pub(crate) error_count: SpectraHostValue,
    pub(crate) latency_samples_ms: Vec<f64>,
}

pub(crate) struct ServeServer {
    pub(crate) model: SpectraHostValue,
    /// Named models registered on this server. The legacy single-model
    /// hosts register under [`SERVE_DEFAULT_MODEL_ID`].
    pub(crate) models: std::collections::BTreeMap<String, ServeModel>,
    pub(crate) model_version: String,
    pub(crate) warm: bool,
    pub(crate) timeout: SpectraHostValue,
    pub(crate) queue: VecDeque<SpectraHostValue>,
    pub(crate) input_policy: Option<(SpectraHostValue, SpectraHostValue)>,
    pub(crate) output_policy: Option<(SpectraHostValue, SpectraHostValue)>,
    pub(crate) rate_limit: Option<SpectraHostValue>,
    pub(crate) accepted_requests: SpectraHostValue,
    pub(crate) fallback: SpectraHostValue,
    pub(crate) last_diagnostic: String,
    pub(crate) audit_events: Vec<String>,
    pub(crate) total_requests: SpectraHostValue,
    pub(crate) completed_requests: SpectraHostValue,
    pub(crate) blocked_requests: SpectraHostValue,
    pub(crate) cancelled_requests: SpectraHostValue,
    pub(crate) error_count: SpectraHostValue,
    pub(crate) batch_count: SpectraHostValue,
    pub(crate) latency_samples_ms: Vec<f64>,
    pub(crate) observed_inputs: Vec<f64>,
    pub(crate) observed_outputs: Vec<f64>,
    /// Per-model monitoring counters keyed by `model_id`.
    pub(crate) model_metrics: std::collections::BTreeMap<String, ServeModelMetrics>,
    /// Optional embedded HTTP/1.1 listener (see `serve_http.rs`); at most one
    /// per server.
    pub(crate) http: Option<ServeHttpRuntime>,
}

pub(crate) struct ServeRegistry {
    pub(crate) servers: ServeHandleTable<ServeServer>,
    pub(crate) requests: ServeHandleTable<(SpectraHostValue, ServeRequestState)>,
}

impl ServeRegistry {
    pub(crate) fn new() -> Self {
        Self {
            servers: ServeHandleTable::new(HandleKind::ServeServer),
            requests: ServeHandleTable::new(HandleKind::ServeRequest),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.servers.clear();
        self.requests.clear();
    }
}

pub(crate) fn serve_registry() -> &'static Mutex<ServeRegistry> {
    static REGISTRY: OnceLock<Mutex<ServeRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(ServeRegistry::new()))
}

pub(crate) fn lock_serve_registry() -> Result<std::sync::MutexGuard<'static, ServeRegistry>, i32> {
    serve_registry()
        .lock()
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)
}

pub(crate) fn serve_guardrail_diagnostic(
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

pub(crate) fn serve_audit_event(
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

pub(crate) fn serve_audit_json(server: &ServeServer) -> String {
    format!(
        "{{\"schema\":\"spectra.serve.audit.v1\",\"model\":{},\"warm\":{},\"accepted_requests\":{},\"events\":[{}]}}",
        server.model,
        i64::from(server.warm),
        server.accepted_requests,
        server.audit_events.join(",")
    )
}


pub(crate) fn serve_record_block(
    server: &mut ServeServer,
    model_id: &str,
    output: SpectraHostValue,
) {
    server.blocked_requests = server.blocked_requests.saturating_add(1);
    server.error_count = server.error_count.saturating_add(1);
    server.observed_outputs.push(output as f64);
    let metrics = server.model_metrics.entry(model_id.to_string()).or_default();
    metrics.requests = metrics.requests.saturating_add(1);
    metrics.blocked_requests = metrics.blocked_requests.saturating_add(1);
    metrics.error_count = metrics.error_count.saturating_add(1);
}

/// Records a completed request together with its MEASURED inference latency
/// (wall-clock `std::time::Instant` delta around the forward pass). No
/// synthetic latency formula exists anymore. Counters fold into the
/// server totals AND the `model_id` section of the monitoring snapshot.
pub(crate) fn serve_record_complete(
    server: &mut ServeServer,
    model_id: &str,
    output_first: f64,
    latency_ms: f64,
) {
    server.completed_requests = server.completed_requests.saturating_add(1);
    server.observed_outputs.push(output_first);
    server.latency_samples_ms.push(latency_ms);
    let metrics = server.model_metrics.entry(model_id.to_string()).or_default();
    metrics.requests = metrics.requests.saturating_add(1);
    metrics.completed_requests = metrics.completed_requests.saturating_add(1);
    metrics.latency_samples_ms.push(latency_ms);
}

pub(crate) fn serve_values_summary(values: &[f64]) -> (f64, f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let sum = values.iter().sum::<f64>();
    (min, max, sum / values.len() as f64)
}

pub(crate) fn serve_p95(values: &[f64]) -> f64 {
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
pub(crate) fn serve_json_samples_array(source: &str, section_marker: &str) -> Option<Vec<f64>> {
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


pub(crate) fn serve_json_f64_array(values: &[f64]) -> String {
    let items = values
        .iter()
        .map(|value| ml_float_json(*value))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", items)
}

pub(crate) fn serve_distribution_summary_json(server: &ServeServer) -> String {
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

pub(crate) fn serve_model_metrics_json(server: &ServeServer) -> String {
    server
        .model_metrics
        .iter()
        .map(|(model_id, metrics)| {
            let latency_avg = if metrics.latency_samples_ms.is_empty() {
                0.0
            } else {
                metrics.latency_samples_ms.iter().sum::<f64>()
                    / metrics.latency_samples_ms.len() as f64
            };
            format!(
                "{{\"model\":{},\"requests\":{},\"completed\":{},\"blocked\":{},\"errors\":{},\"latency_avg_ms\":{},\"latency_p95_ms\":{}}}",
                ml_json_string(model_id),
                metrics.requests,
                metrics.completed_requests,
                metrics.blocked_requests,
                metrics.error_count,
                ml_float_json(latency_avg),
                ml_float_json(serve_p95(&metrics.latency_samples_ms)),
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn serve_monitoring_snapshot_json(server: &ServeServer) -> String {
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
        "{{\"schema\":\"spectra.serve.monitoring_snapshot.v1\",\"model\":{},\"model_version\":{},\"requests\":{},\"completed\":{},\"blocked\":{},\"cancelled\":{},\"errors\":{},\"error_rate\":{},\"batches\":{},\"pending\":{},\"latency_avg_ms\":{},\"latency_p95_ms\":{},\"throughput_per_second\":{},\"models\":[{}]}}",
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
        ml_float_json(throughput),
        serve_model_metrics_json(server)
    )
}

pub(crate) fn serve_json_number(source: &str, key: &str) -> Option<f64> {
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
pub(crate) fn serve_psi(reference: &[f64], live: &[f64]) -> Option<f64> {
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

pub(crate) fn serve_drift_json(
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

pub(crate) fn serve_apply_activation(activation: ServeActivation, values: &[f64]) -> Option<Vec<f64>> {
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

/// Real dense-chain forward pass over a float request vector:
/// `x @ W^T + b` followed by the layer's activation, applied layer by layer.
/// The request vector's length must equal the first layer's input dimension;
/// every following layer must chain on the previous output width (validated
/// per layer below).
pub(crate) fn serve_forward_linear_f64(
    layers: &[ServeLinearLayer],
    input: &[f64],
) -> Result<Vec<f64>, i32> {
    if input.is_empty() {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let mut current = input.to_vec();
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
/// `ml_onnx_run_inner`, exactly like `spectra.std.ml.onnx_run`. The request
/// vector becomes a 1-D float tensor of the same length; scratch tensors are
/// released after extraction.
#[cfg(feature = "onnx")]
pub(crate) fn serve_forward_onnx_f64(session_id: u64, input: &[f64]) -> Result<Vec<f64>, i32> {
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
    let input_handle = ml_alloc_float_tensor(vec![input.len()], input.to_vec())?;
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
pub(crate) fn serve_forward_onnx_f64(_session_id: u64, _input: &[f64]) -> Result<Vec<f64>, i32> {
    Err(HOST_STATUS_INVALID_ARGUMENT)
}
/// Runs one REAL inference against the NAMED registered model and returns
/// the output vector together with its measured latency in milliseconds.
/// Unknown `model_id` or a width mismatch between `input` and the first
/// layer rejects with `HOST_STATUS_INVALID_ARGUMENT`.
pub(crate) fn serve_infer_named(
    server: &ServeServer,
    model_id: &str,
    input: &[f64],
) -> Result<(Vec<f64>, f64), i32> {
    let start = std::time::Instant::now();
    let output = match server.models.get(model_id) {
        Some(ServeModel::Linear(layers)) => serve_forward_linear_f64(layers, input)?,
        Some(ServeModel::Onnx(session_id)) => serve_forward_onnx_f64(*session_id, input)?,
        None => return Err(HOST_STATUS_INVALID_ARGUMENT),
    };
    let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok((output, latency_ms))
}

/// Legacy scalar entry used by the enqueue/process-batch pipeline: infers
/// through the `"default"` model with a length-1 request vector.
pub(crate) fn serve_infer(server: &ServeServer, input: SpectraHostValue) -> Result<(Vec<f64>, f64), i32> {
    serve_infer_f64(server, input as f64)
}

/// Float-native scalar entry used by the embedded HTTP listener so JSON
/// f64 inputs reach the forward pass without an integer round-trip.
pub(crate) fn serve_infer_f64(server: &ServeServer, input: f64) -> Result<(Vec<f64>, f64), i32> {
    serve_infer_named(server, SERVE_DEFAULT_MODEL_ID, &[input])
}

/// Scalar projection of an output vector for the integer result ABI:
/// first component rounded to nearest i64 (saturating).
pub(crate) fn serve_scalar_result(output: &[f64]) -> SpectraHostValue {
    output.first().map(|value| value.round() as i64).unwrap_or(0)
}
