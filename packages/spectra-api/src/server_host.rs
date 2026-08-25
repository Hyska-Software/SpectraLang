pub extern "C" fn server_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    write_result(ctx, store.server_handle())
}

pub extern "C" fn server_state(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = store.entries.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, entry.state)
}

pub extern "C" fn server_shutdown(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = store.entries.get_mut(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    shutdown_entry(ctx, entry, false)
}

pub extern "C" fn server_listen(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(port) = u16::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = store.entries.get_mut(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if entry.state == SERVER_STATE_RUNNING || entry.state == SERVER_STATE_STOPPING {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    entry.config.bind_addr = SocketAddr::from(([127, 0, 0, 1], port));
    write_result(ctx, 1)
}

pub extern "C" fn server_set_max_body_bytes(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(max_body_bytes) = usize::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if max_body_bytes == 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(entry) = store.entries.get_mut(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if matches!(entry.state, SERVER_STATE_RUNNING | SERVER_STATE_STOPPING) {
        return write_result(ctx, 0);
    }
    entry.config.max_body_bytes = max_body_bytes;
    write_result(ctx, 1)
}

pub extern "C" fn server_set_read_timeout(ctx: *mut SpectraHostCallContext) -> i32 {
    set_server_timeout(ctx, true)
}

pub extern "C" fn server_set_idle_timeout(ctx: *mut SpectraHostCallContext) -> i32 {
    set_server_timeout(ctx, false)
}

fn set_server_timeout(ctx: *mut SpectraHostCallContext, read_timeout: bool) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(timeout_ms) = u64::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if timeout_ms == 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut store = store().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(entry) = store.entries.get_mut(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if matches!(entry.state, SERVER_STATE_RUNNING | SERVER_STATE_STOPPING) {
        return write_result(ctx, 0);
    }
    let timeout = std::time::Duration::from_millis(timeout_ms);
    if read_timeout {
        entry.config.read_timeout = timeout;
    } else {
        entry.config.idle_timeout = timeout;
    }
    write_result(ctx, 1)
}

pub extern "C" fn server_serve(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(router) = routing::clone_router(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let handler = routed_handler(router);
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = store.entries.get_mut(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if entry.state == SERVER_STATE_RUNNING || entry.state == SERVER_STATE_STOPPING {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    match HttpServer::start_with_dispatcher(entry.config.clone(), handler) {
        Ok(server) => {
            entry.config.bind_addr = server.local_addr();
            entry.state = SERVER_STATE_RUNNING;
            entry.server = Some(server);
            write_result(ctx, ready_task(1).unwrap_or(1))
        }
        Err(_) => {
            entry.state = SERVER_STATE_STOPPED;
            write_result(ctx, ready_task(0).unwrap_or(0))
        }
    }
}

pub extern "C" fn server_local_port(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = store.entries.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, entry.config.bind_addr.port() as SpectraHostValue)
}

pub extern "C" fn server_signal(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if !matches!(args[1], SERVER_SIGNAL_SIGINT | SERVER_SIGNAL_SIGTERM) {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = store.entries.get_mut(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    shutdown_entry(ctx, entry, true)
}

pub extern "C" fn server_stats(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = store.entries.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let stats = entry
        .server
        .as_ref()
        .map(HttpServer::stats)
        .unwrap_or_else(|| entry.last_stats.clone());
    write_result(ctx, stat_value(&stats, args[1]))
}

fn shutdown_entry(
    ctx: *mut SpectraHostCallContext,
    entry: &mut ServerEntry,
    from_signal: bool,
) -> i32 {
    if entry.state == SERVER_STATE_STOPPED {
        return write_result(ctx, 1);
    }
    entry.state = SERVER_STATE_STOPPING;
    if let Some(mut server) = entry.server.take() {
        if from_signal {
            server
                .stats
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .shutdown_signals += 1;
        }
        match server.shutdown() {
            Ok(stats) => entry.last_stats = stats,
            Err(_) => return write_result(ctx, 0),
        }
    }
    entry.state = SERVER_STATE_STOPPED;
    write_result(ctx, 1)
}

fn routed_handler(router: routing::Router) -> DispatchHandler {
    Arc::new(move |request| {
        let Some(route_method) = route_method_from_request(&request.method) else {
            return HandlerResult::Ready(ServerResponse::text(400, "unsupported method"));
        };
        let route_match = match router.match_path(route_method, &request.target) {
            Ok(Some(route_match)) => route_match,
            Ok(None) => {
                return HandlerResult::Ready(ServerResponse::text(404, "route not found"))
            }
            Err(_) => {
                return HandlerResult::Ready(ServerResponse::text(400, "invalid route path"))
            }
        };
        if let Some(upgrade) = crate::websocket::routed_upgrade_for_route(route_match.route_id) {
            return if crate::websocket::is_upgrade_request(&request) {
                HandlerResult::WebSocket(upgrade)
            } else {
                HandlerResult::Ready(ServerResponse::text(
                    426,
                    "WebSocket upgrade required",
                ))
            };
        }
        let Some(request_handle) = request_handle_from_parsed(&request) else {
            return HandlerResult::Ready(ServerResponse::text(500, "invalid request"));
        };
        let Some(registered) = handler::registered_handler_for_route(route_match.route_id) else {
            return HandlerResult::Ready(ServerResponse::text(500, "handler not registered"));
        };
        match registered {
            handler::RegisteredHandler::Response { handle, response } => {
                handler_result_from_response(handle, response)
            }
            handler::RegisteredHandler::Callback { kind, callback } => {
                let Ok(result) = handler::invoke_callback(callback, request_handle) else {
                    return HandlerResult::Ready(ServerResponse::text(
                        500,
                        "handler callback failed",
                    ));
                };
                match kind {
                    handler::HandlerKind::Sync => {
                        let Some(response) = http::clone_response(result) else {
                            return HandlerResult::Ready(ServerResponse::text(
                                500,
                                "sync handler returned an invalid response",
                            ));
                        };
                        handler_result_from_response(result, response)
                    }
                    handler::HandlerKind::Async => HandlerResult::Pending(PendingResponse {
                        task: result,
                    }),
                }
            }
        }
    })
}

fn handler_result_from_response(
    handle: SpectraHostValue,
    response: Response,
) -> HandlerResult {
    if let Some(route_response) = crate::sse::routed_response_for_handle(handle) {
        HandlerResult::Sse(route_response)
    } else {
        HandlerResult::Ready(server_response_from_http(response))
    }
}

fn request_handle_from_parsed(request: &ParsedRequest) -> Option<SpectraHostValue> {
    let method = method_from_request(&request.method)?;
    let mut typed = Request::new(method, request.target.clone()).ok()?;
    for header in &request.headers {
        typed = typed.with_header(&header.name, &header.value).ok()?;
    }
    typed = typed.with_body(request.body.bytes());
    Some(http::store_request(typed))
}

fn method_from_request(method: &str) -> Option<Method> {
    match method {
        "GET" => Some(Method::Get),
        "HEAD" => Some(Method::Head),
        "POST" => Some(Method::Post),
        "PUT" => Some(Method::Put),
        "PATCH" => Some(Method::Patch),
        "DELETE" => Some(Method::Delete),
        "OPTIONS" => Some(Method::Options),
        _ => None,
    }
}

fn route_method_from_request(method: &str) -> Option<routing::RouteMethod> {
    match method {
        "GET" => Some(routing::RouteMethod::Get),
        "HEAD" => Some(routing::RouteMethod::Head),
        "POST" => Some(routing::RouteMethod::Post),
        "PUT" => Some(routing::RouteMethod::Put),
        "PATCH" => Some(routing::RouteMethod::Patch),
        "DELETE" => Some(routing::RouteMethod::Delete),
        "OPTIONS" => Some(routing::RouteMethod::Options),
        _ => None,
    }
}

pub(crate) fn server_response_from_http(response: Response) -> ServerResponse {
    ServerResponse {
        status_code: response.status.code(),
        reason: response.status.reason().to_string(),
        headers: response.headers.iter().cloned().collect(),
        body: HttpBody::from_bytes(response.body),
        close: false,
    }
}

fn stat_value(stats: &ServerStats, key: SpectraHostValue) -> SpectraHostValue {
    match key {
        1 => stats.accepted_connections as SpectraHostValue,
        2 => stats.completed_requests as SpectraHostValue,
        3 => stats.rejected_connections as SpectraHostValue,
        4 => stats.body_limit_violations as SpectraHostValue,
        5 => stats.timeouts as SpectraHostValue,
        6 => stats.parse_errors as SpectraHostValue,
        7 => stats.closed_connections as SpectraHostValue,
        8 => stats.drained_connections as SpectraHostValue,
        9 => stats.cancelled_connections as SpectraHostValue,
        10 => stats.shutdown_signals as SpectraHostValue,
        11 => stats.peak_connections as SpectraHostValue,
        12 => stats.active_connections as SpectraHostValue,
        _ => 0,
    }
}

fn ready_task(value: SpectraHostValue) -> Option<SpectraHostValue> {
    let function = lookup_host_function("spectra.async.task.ready")?;
    let args = [value];
    let mut result = [0_i64];
    let mut ctx = SpectraHostCallContext {
        args: args.as_ptr(),
        arg_len: args.len(),
        results: result.as_mut_ptr(),
        result_len: result.len(),
        invoke_fn: None,
    };
    if function(&mut ctx as *mut _) == HOST_STATUS_SUCCESS {
        Some(result[0])
    } else {
        None
    }
}

use crate::read_spectra_string;

// ---------------------------------------------------------------------------
// TLS surface
//
// `spectra.api.server.set_tls_certificate` attaches a PEM certificate chain
// and private key to a stopped server. On `serve`, the mio listener keeps
// serving cleartext HTTP/1.1 on the configured port while a dedicated tokio
// gateway listener (OS-assigned port, same host) terminates TLS with ALPN
// fan-out: `h2` connections are served by the HTTP/2 pipeline and `http/1.1`
// connections by the async HTTP/1.1 loop sharing the same router/handlers.
// `spectra.api.server.tls_local_port` reports the gateway port once serving.
// ---------------------------------------------------------------------------

const PEM_BEGIN_MARKER: &str = "-----BEGIN ";

pub extern "C" fn server_set_tls_certificate(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(certificate_pem), Some(private_key_pem)) = (
        read_spectra_string(args[1]),
        read_spectra_string(args[2]),
    ) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(certificate_chain), Some(private_key)) = (
        pem_certificate_chain(&certificate_pem),
        pem_private_key_der(&private_key_pem),
    ) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(certificates) = crate::tls::TlsCertificateStore::new(
        crate::tls::TlsServerConfig::new(certificate_chain, private_key),
    ) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = store.entries.get_mut(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if matches!(entry.state, SERVER_STATE_RUNNING | SERVER_STATE_STOPPING) {
        return write_result(ctx, 0);
    }
    entry.config.tls_certificates = Some(Arc::new(certificates));
    write_result(ctx, 1)
}

pub extern "C" fn server_tls_local_port(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = store.entries.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let port = entry
        .server
        .as_ref()
        .and_then(HttpServer::tls_local_addr)
        .map(|address| address.port() as SpectraHostValue)
        .unwrap_or(0);
    write_result(ctx, port)
}

fn pem_certificate_chain(pem: &str) -> Option<Vec<Vec<u8>>> {
    let chain: Option<Vec<Vec<u8>>> = pem_sections(pem)
        .into_iter()
        .filter(|(label, _)| label == "CERTIFICATE")
        .map(|(_, payload)| base64_decode_standard(&payload))
        .collect();
    let chain = chain?;
    if chain.is_empty() {
        None
    } else {
        Some(chain)
    }
}

fn pem_private_key_der(pem: &str) -> Option<Vec<u8>> {
    pem_sections(pem)
        .into_iter()
        .find(|(label, _)| label.ends_with("PRIVATE KEY"))
        .and_then(|(_, payload)| base64_decode_standard(&payload))
}

/// Splits a PEM document into `(label, base64 payload)` sections.
fn pem_sections(pem: &str) -> Vec<(String, String)> {
    let mut sections = Vec::new();
    let mut rest = pem;
    while let Some(start) = rest.find(PEM_BEGIN_MARKER) {
        let after_begin = &rest[start + PEM_BEGIN_MARKER.len()..];
        let Some(label_end) = after_begin.find("-----") else {
            break;
        };
        let label = &after_begin[..label_end];
        let payload_start = label_end + "-----".len();
        let end_marker = format!("-----END {label}-----");
        let Some(payload_end) = after_begin[payload_start..].find(&end_marker) else {
            break;
        };
        sections.push((
            label.to_string(),
            after_begin[payload_start..payload_start + payload_end].to_string(),
        ));
        rest = &after_begin[payload_start + payload_end + end_marker.len()..];
    }
    sections
}

fn base64_value(byte: u8) -> Option<u32> {
    match byte {
        b'A'..=b'Z' => Some((byte - b'A') as u32),
        b'a'..=b'z' => Some((byte - b'a') as u32 + 26),
        b'0'..=b'9' => Some((byte - b'0') as u32 + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Standard-alphabet base64 decoder for PEM payloads. Whitespace is ignored
/// and `=` padding terminates the data.
fn base64_decode_standard(text: &str) -> Option<Vec<u8>> {
    let mut decoded = Vec::with_capacity(text.len() / 4 * 3);
    let mut accumulator: u32 = 0;
    let mut bits: u32 = 0;
    for character in text.chars() {
        if character.is_whitespace() {
            continue;
        }
        if character == '=' {
            break;
        }
        let value = base64_value(character as u8)?;
        accumulator = (accumulator << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            decoded.push(((accumulator >> bits) & 0xFF) as u8);
        }
    }
    Some(decoded)
}
