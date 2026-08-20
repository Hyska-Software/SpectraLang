#[derive(Clone, Copy, PartialEq, Eq)]
enum ServeRequestState {
    Pending,
    Complete(SpectraHostValue),
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

struct ServeServer {
    model: SpectraHostValue,
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
    latency_samples_ms: Vec<SpectraHostValue>,
    observed_inputs: Vec<SpectraHostValue>,
    observed_outputs: Vec<SpectraHostValue>,
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

fn serve_latency_for(input: SpectraHostValue, output: SpectraHostValue) -> SpectraHostValue {
    1 + (input.abs().saturating_add(output.abs()) % 17)
}

fn serve_record_block(server: &mut ServeServer, output: SpectraHostValue) {
    server.blocked_requests = server.blocked_requests.saturating_add(1);
    server.error_count = server.error_count.saturating_add(1);
    server.observed_outputs.push(output);
}

fn serve_record_complete(
    server: &mut ServeServer,
    input: SpectraHostValue,
    output: SpectraHostValue,
) {
    server.completed_requests = server.completed_requests.saturating_add(1);
    server.observed_outputs.push(output);
    server
        .latency_samples_ms
        .push(serve_latency_for(input, output));
}

fn serve_values_summary(values: &[SpectraHostValue]) -> (SpectraHostValue, SpectraHostValue, f64) {
    if values.is_empty() {
        return (0, 0, 0.0);
    }
    let min = *values.iter().min().unwrap_or(&0);
    let max = *values.iter().max().unwrap_or(&0);
    let sum = values.iter().map(|value| *value as f64).sum::<f64>();
    (min, max, sum / values.len() as f64)
}

fn serve_p95(values: &[SpectraHostValue]) -> SpectraHostValue {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort();
    let index = ((sorted.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    sorted[index.min(sorted.len() - 1)]
}

fn serve_distribution_summary_json(server: &ServeServer) -> String {
    let (input_min, input_max, input_mean) = serve_values_summary(&server.observed_inputs);
    let (output_min, output_max, output_mean) = serve_values_summary(&server.observed_outputs);
    format!(
        "{{\"schema\":\"spectra.serve.distribution_summary.v1\",\"model_version\":{},\"inputs\":{{\"count\":{},\"min\":{},\"max\":{},\"mean\":{}}},\"outputs\":{{\"count\":{},\"min\":{},\"max\":{},\"mean\":{}}}}}",
        ml_json_string(&server.model_version),
        server.observed_inputs.len(),
        input_min,
        input_max,
        ml_float_json(input_mean),
        server.observed_outputs.len(),
        output_min,
        output_max,
        ml_float_json(output_mean)
    )
}

fn serve_monitoring_snapshot_json(server: &ServeServer) -> String {
    let total_latency = server.latency_samples_ms.iter().sum::<SpectraHostValue>();
    let latency_avg = if server.latency_samples_ms.is_empty() {
        0.0
    } else {
        total_latency as f64 / server.latency_samples_ms.len() as f64
    };
    let error_rate = if server.total_requests <= 0 {
        0.0
    } else {
        server.error_count as f64 / server.total_requests as f64
    };
    let throughput = if total_latency <= 0 {
        server.completed_requests as f64
    } else {
        server.completed_requests as f64 / (total_latency as f64 / 1000.0)
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
        serve_p95(&server.latency_samples_ms),
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

fn serve_drift_json(
    reference: &str,
    live: &str,
    threshold_per_mille: SpectraHostValue,
) -> Option<String> {
    let ref_in = serve_json_number(reference, "\"inputs\":{\"count\":")?;
    let live_in = serve_json_number(live, "\"inputs\":{\"count\":")?;
    let ref_input_mean = serve_json_number(reference, "\"mean\":")?;
    let live_input_mean = serve_json_number(live, "\"mean\":")?;
    let ref_outputs = reference.find("\"outputs\"")?;
    let live_outputs = live.find("\"outputs\"")?;
    let ref_output_mean = serve_json_number(&reference[ref_outputs..], "\"mean\":")?;
    let live_output_mean = serve_json_number(&live[live_outputs..], "\"mean\":")?;
    let input_delta = (live_input_mean - ref_input_mean).abs();
    let output_delta = (live_output_mean - ref_output_mean).abs();
    let denom = ref_input_mean.abs().max(ref_output_mean.abs()).max(1.0);
    let score = ((input_delta + output_delta) / denom * 1000.0).round() as SpectraHostValue;
    let drifted = score > threshold_per_mille;
    Some(format!(
        "{{\"schema\":\"spectra.serve.drift_check.v1\",\"reference_count\":{},\"live_count\":{},\"input_mean_delta\":{},\"output_mean_delta\":{},\"score_per_mille\":{},\"threshold_per_mille\":{},\"drifted\":{}}}",
        ref_in as i64,
        live_in as i64,
        ml_float_json(input_delta),
        ml_float_json(output_delta),
        score.max(0),
        threshold_per_mille,
        if drifted { "true" } else { "false" }
    ))
}
