use super::*;
pub(crate) extern "C" fn std_serve_server_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let server_id = registry.servers.insert(ServeServer {
            model: args[0],
        models: std::collections::BTreeMap::new(),
            model_version: format!("model-{}", args[0]),
            warm: false,
            timeout: 1,
            queue: VecDeque::new(),
            input_policy: None,
            output_policy: None,
            rate_limit: None,
            accepted_requests: 0,
            fallback: -1,
            last_diagnostic:
                "{\"schema\":\"spectra.serve.guardrail_diagnostic.v1\",\"status\":\"ok\"}"
                    .to_string(),
            audit_events: Vec::new(),
            total_requests: 0,
            completed_requests: 0,
            blocked_requests: 0,
            cancelled_requests: 0,
            error_count: 0,
            batch_count: 0,
            latency_samples_ms: Vec::new(),
            observed_inputs: Vec::new(),
            observed_outputs: Vec::new(),
            model_metrics: std::collections::BTreeMap::new(),
            http: None,
        });
    results[0] = server_id;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_warmup(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(server) = registry.servers.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    server.warm = true;
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_is_warm(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(server) = registry.servers.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = i64::from(server.warm);
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_enqueue(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let ServeRegistry { servers, requests, .. } = &mut *registry;
    let Some(server) = servers.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    let input = args[1];
    server.total_requests = server.total_requests.saturating_add(1);
    server.observed_inputs.push(input as f64);
    let request_id = requests.insert((input, ServeRequestState::Pending));
    if let Some(limit) = server.rate_limit {
        if server.accepted_requests >= limit {
            server.last_diagnostic = serve_guardrail_diagnostic(
                request_id,
                "input",
                "rate_limit",
                server.accepted_requests + 1,
                0,
                limit,
                server.fallback,
            );
            server.audit_events.push(serve_audit_event(
                request_id,
                "blocked",
                "rate_limit",
                input,
                server.fallback,
            ));
            serve_record_block(server, SERVE_DEFAULT_MODEL_ID, server.fallback);
            if let Some((_, state)) = requests.get_mut(request_id) {
                *state = ServeRequestState::Blocked {
                    fallback: server.fallback,
                    reason: "rate_limit",
                };
            }
            results[0] = request_id;
            return HOST_STATUS_SUCCESS;
        }
    }
    if let Some((min, max)) = server.input_policy {
        if input < min || input > max {
            server.last_diagnostic = serve_guardrail_diagnostic(
                request_id,
                "input",
                "range",
                input,
                min,
                max,
                server.fallback,
            );
            server.audit_events.push(serve_audit_event(
                request_id,
                "blocked",
                "input",
                input,
                server.fallback,
            ));
            serve_record_block(server, SERVE_DEFAULT_MODEL_ID, server.fallback);
            if let Some((_, state)) = requests.get_mut(request_id) {
                *state = ServeRequestState::Blocked {
                    fallback: server.fallback,
                    reason: "input_range",
                };
            }
            results[0] = request_id;
            return HOST_STATUS_SUCCESS;
        }
    }
    server.accepted_requests += 1;
    server.audit_events.push(serve_audit_event(
        request_id, "accepted", "input", input, input,
    ));
    server.queue.push_back(request_id);
    results[0] = request_id;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_cancel(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let ServeRegistry { servers, requests, .. } = &mut *registry;
    let Some(server) = servers.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    let Some((_, state)) = requests.get_mut(args[1]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    if *state == ServeRequestState::Pending {
        *state = ServeRequestState::Cancelled;
        server.queue.retain(|request| *request != args[1]);
        server.cancelled_requests = server.cancelled_requests.saturating_add(1);
        server.error_count = server.error_count.saturating_add(1);
        server
            .audit_events
            .push(serve_audit_event(args[1], "cancelled", "request", 0, -1));
        results[0] = 1;
    } else {
        results[0] = 0;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_process_batch(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let max_batch = args[1].max(0) as usize;
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let ServeRegistry { servers, requests, .. } = &mut *registry;
    let Some(server) = servers.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    if !server.warm || max_batch == 0 {
        results[0] = 0;
        return HOST_STATUS_SUCCESS;
    }
    server.batch_count = server.batch_count.saturating_add(1);

    let mut processed = 0;
    for _ in 0..max_batch {
        let Some(request_id) = server.queue.pop_front() else {
            break;
        };
        let Some((input, state)) = requests.get(request_id) else {
            continue;
        };
        if *state != ServeRequestState::Pending {
            continue;
        }
        let input_value = *input;
        if server.timeout == 0 {
            if let Some((_, state)) = requests.get_mut(request_id) {
                *state = ServeRequestState::Cancelled;
            }
            server.cancelled_requests = server.cancelled_requests.saturating_add(1);
            server.error_count = server.error_count.saturating_add(1);
            continue;
        }
        let (output_vec, latency_ms) = match serve_infer(server, input_value) {
            Ok(result) => result,
            Err(status) => {
                // A misconfigured server must not silently consume queued
                // requests; hand the request back and surface the error.
                server.queue.push_front(request_id);
                return status;
            }
        };
        let output = serve_scalar_result(&output_vec);
        if let Some((min, max)) = server.output_policy {
            let blocked = output_vec
                .iter()
                .any(|value| *value < min as f64 || *value > max as f64);
            if blocked {
                server.last_diagnostic = serve_guardrail_diagnostic(
                    request_id,
                    "output",
                    "range",
                    output,
                    min,
                    max,
                    server.fallback,
                );
                server.audit_events.push(serve_audit_event(
                    request_id,
                    "blocked",
                    "output",
                    output,
                    server.fallback,
                ));
                serve_record_block(server, SERVE_DEFAULT_MODEL_ID, server.fallback);
                if let Some((_, state)) = requests.get_mut(request_id) {
                    *state = ServeRequestState::Blocked {
                        fallback: server.fallback,
                        reason: "output_range",
                    };
                }
                processed += 1;
                continue;
            }
        }
        server.audit_events.push(serve_audit_event(
            request_id,
            "completed",
            "output",
            output,
            output,
        ));
        serve_record_complete(
            server,
            SERVE_DEFAULT_MODEL_ID,
            output_vec[0],
            latency_ms,
        );
        if let Some((_, state)) = requests.get_mut(request_id) {
            *state = ServeRequestState::Complete(output, output_vec);
        }
        processed += 1;
    }
    results[0] = processed;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_result(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(_server) = registry.servers.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    let Some((_, state)) = registry.requests.get(args[1]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = match *state {
        ServeRequestState::Pending | ServeRequestState::Cancelled => -1,
        ServeRequestState::Complete(value, _) => value,
        ServeRequestState::Blocked { fallback, .. } => fallback,
    };
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_pending(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(server) = registry.servers.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = server.queue.len() as i64;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_set_timeout(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(server) = registry.servers.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    server.timeout = args[1].max(0);
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_resident_model(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(server) = registry.servers.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = server.model;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_benchmark(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 3) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let server_id = args[0];
    let requests = args[1].max(0);
    let batch = args[2].max(1);

    {
        let mut registry = match lock_serve_registry() {
            Ok(registry) => registry,
            Err(status) => return status,
        };
        let Some(server) = registry.servers.get_mut(server_id) else {
            return HOST_STATUS_NOT_FOUND;
        };
        if !server.models.contains_key(SERVE_DEFAULT_MODEL_ID) {
            // Benchmarking without a real served model would fabricate data.
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        server.warm = true;
    }

    for input in 1..=requests {
        let mut registry = match lock_serve_registry() {
            Ok(registry) => registry,
            Err(status) => return status,
        };
        let ServeRegistry {
            servers,
            requests: requests_registry,
        } = &mut *registry;
        let Some(server) = servers.get_mut(server_id) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let request_id = requests_registry.insert((input, ServeRequestState::Pending));
        server.total_requests = server.total_requests.saturating_add(1);
        server.accepted_requests = server.accepted_requests.saturating_add(1);
        server.observed_inputs.push(input as f64);
        server.queue.push_back(request_id);
    }

    let mut processed_total = 0;
    loop {
        let mut registry = match lock_serve_registry() {
            Ok(registry) => registry,
            Err(status) => return status,
        };
        let ServeRegistry {
            servers,
            requests: requests_registry,
        } = &mut *registry;
        let Some(server) = servers.get_mut(server_id) else {
            return HOST_STATUS_NOT_FOUND;
        };
        if server.queue.is_empty() {
            break;
        }
        server.batch_count = server.batch_count.saturating_add(1);
        let mut processed = 0;
        for _ in 0..batch {
            let Some(request_id) = server.queue.pop_front() else {
                break;
            };
            let Some((input, state)) = requests_registry.get(request_id) else {
                continue;
            };
            if *state != ServeRequestState::Pending {
                continue;
            }
            let input_value = *input;
            if server.timeout == 0 {
                if let Some((_, state)) = requests_registry.get_mut(request_id) {
                    *state = ServeRequestState::Cancelled;
                }
                server.cancelled_requests = server.cancelled_requests.saturating_add(1);
                server.error_count = server.error_count.saturating_add(1);
                continue;
            }
            let (output_vec, latency_ms) = match serve_infer(server, input_value) {
                Ok(result) => result,
                Err(status) => {
                    server.queue.push_front(request_id);
                    return status;
                }
            };
            let output = serve_scalar_result(&output_vec);
            serve_record_complete(server, SERVE_DEFAULT_MODEL_ID, output_vec[0], latency_ms);
            if let Some((_, state)) = requests_registry.get_mut(request_id) {
                *state = ServeRequestState::Complete(output, output_vec);
            }
            processed += 1;
        }
        processed_total += processed;
    }
    results[0] = processed_total;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_set_input_policy(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 3) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if args[1] > args[2] {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(server) = registry.servers.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    server.input_policy = Some((args[1], args[2]));
    server.audit_events.push(format!(
        "{{\"request\":0,\"event\":\"policy_attached\",\"stage\":\"input\",\"value\":{},\"result\":{}}}",
        args[1], args[2]
    ));
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_set_output_policy(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 3) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if args[1] > args[2] {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(server) = registry.servers.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    server.output_policy = Some((args[1], args[2]));
    server.audit_events.push(format!(
        "{{\"request\":0,\"event\":\"policy_attached\",\"stage\":\"output\",\"value\":{},\"result\":{}}}",
        args[1], args[2]
    ));
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_set_rate_limit(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if args[1] <= 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(server) = registry.servers.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    server.rate_limit = Some(args[1]);
    server.audit_events.push(format!(
        "{{\"request\":0,\"event\":\"policy_attached\",\"stage\":\"rate_limit\",\"value\":{},\"result\":{}}}",
        args[1], args[1]
    ));
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_set_fallback(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(server) = registry.servers.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    server.fallback = args[1];
    server.audit_events.push(format!(
        "{{\"request\":0,\"event\":\"fallback_attached\",\"stage\":\"fallback\",\"value\":{},\"result\":{}}}",
        args[1], args[1]
    ));
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_last_diagnostic(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let diagnostic = {
        let registry = match lock_serve_registry() {
            Ok(registry) => registry,
            Err(status) => return status,
        };
        let Some(server) = registry.servers.get(args[0]) else {
            return HOST_STATUS_NOT_FOUND;
        };
        server.last_diagnostic.clone()
    };
    results[0] = unsafe { alloc_spectra_string(&diagnostic) };
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_audit_log(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let audit = {
        let registry = match lock_serve_registry() {
            Ok(registry) => registry,
            Err(status) => return status,
        };
        let Some(server) = registry.servers.get(args[0]) else {
            return HOST_STATUS_NOT_FOUND;
        };
        serve_audit_json(server)
    };
    results[0] = unsafe { alloc_spectra_string(&audit) };
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_set_model_version(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let Some(version) = ml_read_path_arg(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(server) = registry.servers.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    server.model_version = version;
    server.audit_events.push(format!(
        "{{\"request\":0,\"event\":\"model_version_set\",\"stage\":\"monitoring\",\"value\":{},\"result\":{}}}",
        server.model, server.model
    ));
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_monitoring_snapshot(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let snapshot = {
        let registry = match lock_serve_registry() {
            Ok(registry) => registry,
            Err(status) => return status,
        };
        let Some(server) = registry.servers.get(args[0]) else {
            return HOST_STATUS_NOT_FOUND;
        };
        serve_monitoring_snapshot_json(server)
    };
    results[0] = unsafe { alloc_spectra_string(&snapshot) };
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_server_distribution_summary(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let summary = {
        let registry = match lock_serve_registry() {
            Ok(registry) => registry,
            Err(status) => return status,
        };
        let Some(server) = registry.servers.get(args[0]) else {
            return HOST_STATUS_NOT_FOUND;
        };
        serve_distribution_summary_json(server)
    };
    results[0] = unsafe { alloc_spectra_string(&summary) };
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_drift_check(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 3) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if args[2] < 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let Some(reference) = ml_read_path_arg(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(live) = ml_read_path_arg(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(drift) = serve_drift_json(&reference, &live, args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    results[0] = unsafe { alloc_spectra_string(&drift) };
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_export_monitoring(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 5) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let Some(path) = ml_read_path_arg(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(distribution) = ml_json_payload_arg(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(drift) = ml_json_payload_arg(args[3]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(audit) = ml_json_payload_arg(args[4]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let snapshot = {
        let registry = match lock_serve_registry() {
            Ok(registry) => registry,
            Err(status) => return status,
        };
        let Some(server) = registry.servers.get(args[0]) else {
            return HOST_STATUS_NOT_FOUND;
        };
        serve_monitoring_snapshot_json(server)
    };
    if let Some(parent) = std::path::Path::new(&path).parent() {
        if !parent.as_os_str().is_empty() && std::fs::create_dir_all(parent).is_err() {
            return HOST_STATUS_INTERNAL_ERROR;
        }
    }
    let payload = format!(
        "{{\"schema\":\"spectra.serve.monitoring_export.v1\",\"snapshot\":{},\"distribution\":{},\"drift\":{},\"audit\":{}}}",
        snapshot, distribution, drift, audit
    );
    if std::fs::write(&path, payload).is_err() {
        return HOST_STATUS_INTERNAL_ERROR;
    }
    results[0] = unsafe { alloc_spectra_string(&path) };
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_serve_reset(ctx: *mut SpectraHostCallContext) -> i32 {
    let _ = match host_call_void_args(ctx, 0) {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    registry.clear();
    HOST_STATUS_SUCCESS
}

// ── ServeReal ──

/// Parses one dense layer `(weights_tensor, biases_tensor, activation_code)`
/// from host args. Weights must be a 2-D float tensor `[out][in]` (row-major),
/// biases a 1-D float tensor with `out` elements; activation codes are
/// 0 = relu, 1 = gelu, 2 = tanh, 3 = softmax.
pub(crate) fn serve_parse_linear_layer(
    weights_handle: SpectraHostValue,
    biases_handle: SpectraHostValue,
    activation_code: SpectraHostValue,
) -> Result<ServeLinearLayer, i32> {
    let Some((weight_shape, weight_data, _)) =
        ml_tensor_float_data(weights_handle as usize)
    else {
        return Err(HOST_STATUS_NOT_FOUND);
    };
    let Some((bias_shape, bias_data, _)) = ml_tensor_float_data(biases_handle as usize) else {
        return Err(HOST_STATUS_NOT_FOUND);
    };
    if weight_shape.len() != 2 || bias_shape.len() != 1 {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let (rows, cols) = (weight_shape[0], weight_shape[1]);
    if rows == 0 || cols == 0 || bias_data.len() != rows || weight_data.len() != rows * cols {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let activation = match activation_code {
        0 => ServeActivation::Relu,
        1 => ServeActivation::Gelu,
        2 => ServeActivation::Tanh,
        3 => ServeActivation::Softmax,
        _ => return Err(HOST_STATUS_INVALID_ARGUMENT),
    };
    let weights = weight_data
        .chunks(cols)
        .map(|row| row.to_vec())
        .collect();
    Ok(ServeLinearLayer {
        weights,
        biases: bias_data,
        activation,
    })
}

/// `spectra.std.serve.server_register_model_linear(
///     server, w1, b1, act1, w2, b2, act2, ...) -> int`
/// Registers the REAL served model under the `"default"` model id: a chain
/// of dense layers executed as `x @ W^T + b -> act(layer)` per layer.
/// Request inputs are scalars (enqueue/process-batch pipeline), so the first
/// layer's input dimension must be 1 and every following layer must chain on
/// the previous output width. Use the named-model host for wider inputs or
/// multiple models on one server.
pub(crate) extern "C" fn std_serve_server_register_model_linear(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let (args, results) = unsafe {
        let ctx_ref = &mut *ctx;
        // Variadic: (server, w1, b1, act1, ...) -> 1 + 3 * layers args.
        if ctx_ref.arg_len < 4 || (ctx_ref.arg_len - 1) % 3 != 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.args.is_null() || ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        (
            std::slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len),
            std::slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len),
        )
    };
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(server) = registry.servers.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    let mut layers: Vec<ServeLinearLayer> = Vec::new();
    for chunk in args[1..].chunks_exact(3) {
        let layer = match serve_parse_linear_layer(chunk[0], chunk[1], chunk[2]) {
            Ok(layer) => layer,
            Err(status) => return status,
        };
        let input_dim = layer.weights.first().map(|row| row.len()).unwrap_or(0);
        if layers.is_empty() {
            if input_dim != 1 {
                // Scalar request inputs feed a length-1 vector.
                return HOST_STATUS_INVALID_ARGUMENT;
            }
        } else {
            let previous_output = layers.last().unwrap().weights.len();
            if input_dim != previous_output {
                return HOST_STATUS_INVALID_ARGUMENT;
            }
        }
        layers.push(layer);
    }
    server
        .models
        .insert(SERVE_DEFAULT_MODEL_ID.to_string(), ServeModel::Linear(layers));
    server.audit_events.push(format!(
        "{{\"request\":0,\"event\":\"model_registered\",\"stage\":\"model\",\"value\":{},\"result\":{}}}",
        server.model,
        server.model
    ));
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

/// `spectra.std.serve.server_register_model_onnx(server, session_handle) -> int`
///
/// Registers a committed onnxruntime session as the served model. Inference
/// delegates to `ml_onnx_run_inner` exactly like
/// `spectra.std.ml.onnx_run`. Without the runtime `onnx` feature no real
/// session can exist and this host rejects instead of simulating.
pub(crate) extern "C" fn std_serve_server_register_model_onnx(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    #[cfg(not(feature = "onnx"))]
    {
        let _ = (args, results);
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    #[cfg(feature = "onnx")]
    {
        if args[1] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let session_id = args[1] as u64;
        if !ml_onnx_sessions_lock().contains_key(&session_id) {
            return HOST_STATUS_NOT_FOUND;
        }
        let mut registry = match lock_serve_registry() {
            Ok(registry) => registry,
            Err(status) => return status,
        };
        let Some(server) = registry.servers.get_mut(args[0]) else {
            return HOST_STATUS_NOT_FOUND;
        };
        server
            .models
            .insert(SERVE_DEFAULT_MODEL_ID.to_string(), ServeModel::Onnx(session_id));
        server.audit_events.push(format!(
            "{{\"request\":0,\"event\":\"model_registered\",\"stage\":\"model\",\"value\":{},\"result\":{}}}",
            server.model, session_id
        ));
        results[0] = 1;
        HOST_STATUS_SUCCESS
    }
}

/// `spectra.std.serve.server_result_vector(server, request_id) -> tensor`
///
/// Returns the full float output vector of a completed request as a fresh
/// stdlib tensor handle, enabling exact (< 1e-9) verification of forward-pass
/// results. Only requests completed through real inference carry vectors.
pub(crate) extern "C" fn std_serve_server_result_vector(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(_server) = registry.servers.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    let Some((_, state)) = registry.requests.get(args[1]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    let vector = match state {
        ServeRequestState::Complete(_, vector) if !vector.is_empty() => vector.clone(),
        _ => return HOST_STATUS_NOT_FOUND,
    };
    match ml_alloc_float_tensor(vec![vector.len()], vector) {
        Ok(handle) => {
            results[0] = handle as SpectraHostValue;
            HOST_STATUS_SUCCESS
        }
        Err(status) => status,
    }
}

// ── ServeMultiModel ──

/// `spectra.std.serve.server_register_named_model_linear(
///     server, "model_id", w1, b1, act1, ...) -> int`
///
/// Registers a REAL dense-chain model under an explicit `model_id`, allowing
/// MULTIPLE models per server. The first layer may have any input dimension
/// N >= 1: request vectors of length N are accepted and every following
/// layer must chain on the previous output width.
pub(crate) extern "C" fn std_serve_server_register_named_model_linear(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let (args, results) = unsafe {
        let ctx_ref = &mut *ctx;
        // Variadic: (server, model_id, w1, b1, act1, ...) -> 2 + 3 * layers args.
        if ctx_ref.arg_len < 5 || (ctx_ref.arg_len - 2) % 3 != 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.args.is_null() || ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        (
            std::slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len),
            std::slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len),
        )
    };
    let Some(model_id) = ml_read_path_arg(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(server) = registry.servers.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    let mut layers: Vec<ServeLinearLayer> = Vec::new();
    for chunk in args[2..].chunks_exact(3) {
        let layer = match serve_parse_linear_layer(chunk[0], chunk[1], chunk[2]) {
            Ok(layer) => layer,
            Err(status) => return status,
        };
        let input_dim = layer.weights.first().map(|row| row.len()).unwrap_or(0);
        if input_dim == 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if let Some(previous_output) = layers.last().map(|layer| layer.weights.len()) {
            if input_dim != previous_output {
                return HOST_STATUS_INVALID_ARGUMENT;
            }
        }
        layers.push(layer);
    }
    server.audit_events.push(format!(
        "{{\"request\":0,\"event\":\"model_registered\",\"stage\":\"model\",\"model\":{},\"value\":{},\"result\":{}}}",
        ml_json_string(&model_id),
        server.model,
        server.model
    ));
    server.models.insert(model_id, ServeModel::Linear(layers));
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

/// `spectra.std.serve.server_register_named_model_onnx(
///     server, "model_id", session_handle) -> int`
///
/// Registers a committed onnxruntime session under an explicit `model_id`.
/// Inference delegates to `ml_onnx_run_inner` with the named model's session.
/// Without the runtime `onnx` feature no real session can exist and this
/// host rejects instead of simulating.
pub(crate) extern "C" fn std_serve_server_register_named_model_onnx(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 3) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    #[cfg(not(feature = "onnx"))]
    {
        let _ = (args, results);
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    #[cfg(feature = "onnx")]
    {
        let Some(model_id) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[2] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let session_id = args[2] as u64;
        if !ml_onnx_sessions_lock().contains_key(&session_id) {
            return HOST_STATUS_NOT_FOUND;
        }
        let mut registry = match lock_serve_registry() {
            Ok(registry) => registry,
            Err(status) => return status,
        };
        let Some(server) = registry.servers.get_mut(args[0]) else {
            return HOST_STATUS_NOT_FOUND;
        };
        server.audit_events.push(format!(
            "{{\"request\":0,\"event\":\"model_registered\",\"stage\":\"model\",\"model\":{},\"value\":{},\"result\":{}}}",
            ml_json_string(&model_id),
            server.model,
            session_id
        ));
        server.models.insert(model_id, ServeModel::Onnx(session_id));
        results[0] = 1;
        HOST_STATUS_SUCCESS
    }
}

/// `spectra.std.serve.server_infer(server, "model_id", input_tensor) -> request_id`
///
/// Runs one REAL synchronous inference against the named model. The input is
/// a 1-D float tensor handle whose length must equal the registered model's
/// first layer input dimension. Counters fold into both the server totals
/// and the `model_id` section of the monitoring snapshot. The returned
/// request id works with `server_result` / `server_result_vector`. Like the
/// embedded HTTP listener path, direct inference bypasses queue guardrails.
pub(crate) extern "C" fn std_serve_server_infer(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 3) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let Some(model_id) = ml_read_path_arg(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some((shape, data, _)) = ml_tensor_float_data(args[2] as usize) else {
        return HOST_STATUS_NOT_FOUND;
    };
    if shape.len() != 1 || data.is_empty() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let ServeRegistry {
        servers,
        requests,
        ..
    } = &mut *registry;
    let Some(server) = servers.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    if !server.models.contains_key(&model_id) {
        return HOST_STATUS_NOT_FOUND;
    }
    server.total_requests = server.total_requests.saturating_add(1);
    server.accepted_requests = server.accepted_requests.saturating_add(1);
    server.observed_inputs.extend_from_slice(&data);
    let request_id = requests.insert((data[0] as SpectraHostValue, ServeRequestState::Pending));
    let (output_vec, latency_ms) = match serve_infer_named(server, &model_id, &data) {
        Ok(result) => result,
        Err(status) => return status,
    };
    if output_vec.is_empty() {
        return HOST_STATUS_INTERNAL_ERROR;
    }
    let output = serve_scalar_result(&output_vec);
    server.audit_events.push(serve_audit_event(
        request_id,
        "completed",
        "direct",
        output,
        output,
    ));
    serve_record_complete(server, &model_id, output_vec[0], latency_ms);
    if let Some((_, state)) = requests.get_mut(request_id) {
        *state = ServeRequestState::Complete(output, output_vec);
    }
    results[0] = request_id;
    HOST_STATUS_SUCCESS
}
