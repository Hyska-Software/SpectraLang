use super::*;
// ── ServeHttp ────────────────────────────────────────────────────────────────
//
// OPTIONAL embedded HTTP/1.1 listener inside the runtime, implemented directly
// on `mio` (the same backend as `async_network_reactor`). A served server can
// expose two fixed routes without any external wiring:
//
//   POST /infer   JSON body {"inputs":[f64]} (exactly one scalar) runs the REAL
//                 registered forward pass (dense chain or ONNX session) and
//                 answers {"outputs":[f64],"latency_ms":f64}.
//   GET  /metrics Returns the existing monitoring snapshot JSON
//                 (`spectra.serve.monitoring_snapshot.v1`).
//
// Hard limits: request head <= 16 KiB, body <= 4 MiB; every read/write phase is
// deadline-bounded and keep-alive is simple sequential request handling on one
// accepted connection at a time.
//
// NOTE: this listener is a minimal embedded surface. The complete api server
// (packages/spectra-api) remains RECOMMENDED for production deployments: it
// adds routing, TLS termination, concurrency, and observability far beyond
// these two fixed routes.

pub(crate) const SERVE_HTTP_MAX_HEAD_BYTES: usize = 16 * 1024;
pub(crate) const SERVE_HTTP_MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const SERVE_HTTP_REQUEST_TIMEOUT_MS: u64 = 5_000;
pub(crate) const SERVE_HTTP_ACCEPT_POLL_MS: u64 = 100;

/// Live HTTP runtime attached to a `ServeServer`. Dropping it signals the
/// worker to stop; the worker observes the flag within one accept-poll cycle.
pub(crate) struct ServeHttpRuntime {
    /// Port actually bound (resolves ephemeral port 0).
    pub(crate) port: u16,
    pub(crate) running: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(crate) worker: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ServeHttpRuntime {
    fn drop(&mut self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        // Deliberately not joined here: dropping can happen under the serve
        // registry lock, and joining could block on a lingering keep-alive
        // peer. The detached worker exits within one poll cycle.
    }
}

/// One parsed HTTP/1.1 request.
pub(crate) struct ServeHttpRequest {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) body: Vec<u8>,
    pub(crate) keep_alive: bool,
}

pub(crate) enum ServeHttpReadError {
    Closed,
    TimedOut,
    HeadTooLarge,
    BodyTooLarge,
    Malformed,
}

impl ServeHttpReadError {
    /// Fixed error response emitted before the connection is closed.
    pub(crate) fn response(&self) -> Option<(u16, &'static str, &'static str)> {
        match self {
            ServeHttpReadError::HeadTooLarge => Some((431, "Request Header Fields Too Large", "{\"error\":\"head_too_large\"}")),
            ServeHttpReadError::BodyTooLarge => Some((413, "Payload Too Large", "{\"error\":\"body_too_large\"}")),
            ServeHttpReadError::Malformed => Some((400, "Bad Request", "{\"error\":\"malformed_request\"}")),
            ServeHttpReadError::Closed | ServeHttpReadError::TimedOut => None,
        }
    }
}

pub(crate) fn serve_http_find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Blocks until the listener is readable or the shutdown flag flips.
pub(crate) fn serve_http_wait_readable(
    poll: &mut mio::Poll,
    events: &mut mio::Events,
    token: mio::Token,
    deadline: std::time::Instant,
) -> bool {
    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            return false;
        }
        match poll.poll(events, Some(deadline - now)) {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return false,
        }
        if events.iter().any(|event| event.token() == token && event.is_readable()) {
            return true;
        }
    }
}

/// Reads one full HTTP/1.1 request (head + Content-Length body) from a
/// non-blocking stream, enforcing head/body limits and the request deadline.
pub(crate) fn serve_http_read_request(
    poll: &mut mio::Poll,
    events: &mut mio::Events,
    stream: &mut mio::net::TcpStream,
) -> Result<ServeHttpRequest, ServeHttpReadError> {
    const TOKEN: mio::Token = mio::Token(0);
    let started = std::time::Instant::now();
    let deadline = started + std::time::Duration::from_millis(SERVE_HTTP_REQUEST_TIMEOUT_MS);
    let mut buffer: Vec<u8> = Vec::with_capacity(2048);
    let mut chunk = [0u8; 8192];

    // ── head ──
    let head_end = loop {
        if let Some(position) = serve_http_find_subsequence(&buffer, b"\r\n\r\n") {
            break position;
        }
        if buffer.len() > SERVE_HTTP_MAX_HEAD_BYTES {
            return Err(ServeHttpReadError::HeadTooLarge);
        }
        if !serve_http_wait_readable(poll, events, TOKEN, deadline) {
            return Err(ServeHttpReadError::TimedOut);
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Err(ServeHttpReadError::Closed),
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(ServeHttpReadError::Closed),
        }
    };

    // ── head parsing ──
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().ok_or(ServeHttpReadError::Malformed)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or(ServeHttpReadError::Malformed)?.to_ascii_uppercase();
    let target = parts.next().ok_or(ServeHttpReadError::Malformed)?.to_string();
    let version = parts.next().unwrap_or("HTTP/1.1");
    let mut content_length: usize = 0;
    // HTTP/1.1 defaults to keep-alive; HTTP/1.0 defaults to close.
    let mut keep_alive = version.eq_ignore_ascii_case("HTTP/1.1");
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => {
                content_length = value
                    .trim()
                    .parse()
                    .map_err(|_| ServeHttpReadError::Malformed)?;
            }
            "connection" => {
                let value = value.to_ascii_lowercase();
                if value.contains("close") {
                    keep_alive = false;
                } else if value.contains("keep-alive") {
                    keep_alive = true;
                }
            }
            _ => {}
        }
    }
    if content_length > SERVE_HTTP_MAX_BODY_BYTES {
        return Err(ServeHttpReadError::BodyTooLarge);
    }

    // ── body ──
    let mut body: Vec<u8> = buffer[head_end + 4..].to_vec();
    while body.len() < content_length {
        if !serve_http_wait_readable(poll, events, TOKEN, deadline) {
            return Err(ServeHttpReadError::TimedOut);
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Err(ServeHttpReadError::Closed),
            Ok(read) => body.extend_from_slice(&chunk[..read]),
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(ServeHttpReadError::Closed),
        }
    }
    body.truncate(content_length);

    Ok(ServeHttpRequest {
        method,
        path: target.split('?').next().unwrap_or("").to_string(),
        body,
        keep_alive,
    })
}

pub(crate) fn serve_http_status_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

pub(crate) fn serve_http_response_bytes(status: u16, content_type: &str, body: &str, close: bool) -> Vec<u8> {
    let connection = if close { "close" } else { "keep-alive" };
    format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: {}\r\n\r\n{}",
        status,
        serve_http_status_reason(status),
        content_type,
        body.len(),
        connection,
        body
    )
    .into_bytes()
}

/// Maps an inference host-status failure onto an HTTP error response.
pub(crate) fn serve_http_infer_error_response(status: i32) -> Vec<u8> {
    let (status_code, body) = match status {
        HOST_STATUS_NOT_FOUND => (404u16, "{\"error\":\"unknown_server\"}"),
        HOST_STATUS_INVALID_ARGUMENT => (400u16, "{\"error\":\"inference_rejected\"}"),
        _ => (500u16, "{\"error\":\"inference_failed\"}"),
    };
    serve_http_response_bytes(status_code, "application/json", body, false)
}

/// Executes one real inference against the registered served model and folds
/// the result into the same monitoring counters the queue pipeline uses.
pub(crate) fn serve_http_run_inference(
    registry: &mut ServeRegistry,
    server_handle: SpectraHostValue,
    input: f64,
) -> Result<(Vec<f64>, f64), Vec<u8>> {
    let server = registry
        .servers
        .get_mut(server_handle)
        .ok_or_else(|| serve_http_response_bytes(404, "application/json", "{\"error\":\"unknown_server\"}", false))?;
    if server.served_model.is_none() {
        return Err(serve_http_response_bytes(
            503,
            "application/json",
            "{\"error\":\"no_registered_model\"}",
            false,
        ));
    }
    server.total_requests = server.total_requests.saturating_add(1);
    server.observed_inputs.push(input);
    match serve_infer_f64(server, input) {
        Ok((output, latency_ms)) => {
            if let Some(first) = output.first() {
                serve_record_complete(server, *first, latency_ms);
            } else {
                return Err(serve_http_response_bytes(
                    500,
                    "application/json",
                    "{\"error\":\"empty_output\"}",
                    false,
                ));
            }
            Ok((output, latency_ms))
        }
        Err(status) => Err(serve_http_infer_error_response(status)),
    }
}

/// Routes one parsed request. Returns the encoded response plus whether the
/// connection must be closed afterwards.
pub(crate) fn serve_http_route(
    server_handle: SpectraHostValue,
    request: &ServeHttpRequest,
) -> (Vec<u8>, bool) {
    let close = !request.keep_alive;
    match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/infer") => {
            let parsed: serde_json::Value = match serde_json::from_slice(&request.body) {
                Ok(parsed) => parsed,
                Err(_) => {
                    return (
                        serve_http_response_bytes(
                            400,
                            "application/json",
                            "{\"error\":\"invalid_json\"}",
                            false,
                        ),
                        close,
                    )
                }
            };
            let inputs = parsed.get("inputs").and_then(serde_json::Value::as_array);
            let input = match inputs {
                Some(values) if values.len() == 1 => values[0].as_f64(),
                _ => None,
            };
            let Some(input) = input else {
                return (
                    serve_http_response_bytes(
                        400,
                        "application/json",
                        "{\"error\":\"expected_inputs_single_f64_array\"}",
                        false,
                    ),
                    close,
                );
            };
            let outcome = match lock_serve_registry() {
                Ok(mut registry) => serve_http_run_inference(&mut registry, server_handle, input),
                Err(status) => Err(serve_http_infer_error_response(status)),
            };
            match outcome {
                Ok((output, latency_ms)) => {
                    let body = format!(
                        "{{\"outputs\":{},\"latency_ms\":{}}}",
                        serve_json_f64_array(&output),
                        ml_float_json(latency_ms)
                    );
                    (
                        serve_http_response_bytes(200, "application/json", &body, close),
                        close,
                    )
                }
                Err(response) => (response, close),
            }
        }
        ("GET", "/metrics") => {
            let body = match lock_serve_registry() {
                Ok(registry) => match registry.servers.get(server_handle) {
                    Some(server) => serve_monitoring_snapshot_json(server),
                    None => "{\"error\":\"unknown_server\"}".to_string(),
                },
                Err(_) => "{\"error\":\"registry_locked\"}".to_string(),
            };
            (
                serve_http_response_bytes(200, "application/json", &body, close),
                close,
            )
        }
        (_, "/infer") | (_, "/metrics") => (
            serve_http_response_bytes(
                405,
                "application/json",
                "{\"error\":\"method_not_allowed\"}",
                false,
            ),
            close,
        ),
        _ => (
            serve_http_response_bytes(404, "application/json", "{\"error\":\"not_found\"}", false),
            close,
        ),
    }
}

/// Serves one accepted connection sequentially until close/shutdown.
pub(crate) fn serve_http_serve_connection(
    server_handle: SpectraHostValue,
    mut stream: mio::net::TcpStream,
    running: &std::sync::atomic::AtomicBool,
) {
    let mut poll = match mio::Poll::new() {
        Ok(poll) => poll,
        Err(_) => return,
    };
    // READABLE only: an idle socket is always writable, so registering
    // WRITABLE here would make every poll return instantly and busy-spin the
    // read deadline down. `serve_http_write_all` flips the interest when a
    // write actually blocks.
    if poll
        .registry()
        .register(&mut stream, mio::Token(0), mio::Interest::READABLE)
        .is_err()
    {
        return;
    }
    let mut events = mio::Events::with_capacity(8);
    loop {
        if !running.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let request = match serve_http_read_request(&mut poll, &mut events, &mut stream) {
            Ok(request) => request,
            Err(error) => {
                if let Some((status, reason_hint, body)) = error.response() {
                    let _ = reason_hint;
                    let bytes =
                        serve_http_response_bytes(status, "application/json", body, true);
                    let _ = serve_http_write_all(&mut poll, &mut events, &mut stream, &bytes);
                }
                return;
            }
        };
        let (response, close) = serve_http_route(server_handle, &request);
        if !serve_http_write_all(&mut poll, &mut events, &mut stream, &response) {
            return;
        }
        if close || !running.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
    }
}

/// Writes all bytes tolerating non-blocking WouldBlock via WRITABLE polling.
pub(crate) fn serve_http_write_all(
    poll: &mut mio::Poll,
    events: &mut mio::Events,
    stream: &mut mio::net::TcpStream,
    mut data: &[u8],
) -> bool {
    const TOKEN: mio::Token = mio::Token(0);
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(SERVE_HTTP_REQUEST_TIMEOUT_MS);
    while !data.is_empty() {
        match stream.write(data) {
            Ok(0) => return false,
            Ok(written) => data = &data[written..],
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::Interrupted =>
            {
                let now = std::time::Instant::now();
                if now >= deadline {
                    return false;
                }
                // Wait specifically for writability, then restore the
                // read-oriented registration.
                if poll
                    .registry()
                    .reregister(stream, TOKEN, mio::Interest::WRITABLE)
                    .is_err()
                {
                    return false;
                }
                loop {
                    let now = std::time::Instant::now();
                    if now >= deadline {
                        let _ = poll.registry().reregister(
                            stream,
                            TOKEN,
                            mio::Interest::READABLE,
                        );
                        return false;
                    }
                    match poll.poll(events, Some(deadline - now)) {
                        Ok(_) => break,
                        Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => {
                            let _ = poll.registry().reregister(
                                stream,
                                TOKEN,
                                mio::Interest::READABLE,
                            );
                            return false;
                        }
                    }
                }
                if poll
                    .registry()
                    .reregister(stream, TOKEN, mio::Interest::READABLE)
                    .is_err()
                {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

/// Dedicated accept-loop worker: polls the mio listener, drains pending
/// accepts, and serves each connection to completion.
pub(crate) fn serve_http_worker(
    server_handle: SpectraHostValue,
    mut listener: mio::net::TcpListener,
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let mut poll = match mio::Poll::new() {
        Ok(poll) => poll,
        Err(_) => return,
    };
    if poll
        .registry()
        .register(&mut listener, mio::Token(0), mio::Interest::READABLE)
        .is_err()
    {
        return;
    }
    let mut events = mio::Events::with_capacity(16);
    while running.load(std::sync::atomic::Ordering::Relaxed) {
        match poll.poll(
            &mut events,
            Some(std::time::Duration::from_millis(SERVE_HTTP_ACCEPT_POLL_MS)),
        ) {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
        for event in events.iter() {
            if event.token() != mio::Token(0) || !event.is_readable() {
                continue;
            }
            loop {
                match listener.accept() {
                    Ok((stream, _address)) => {
                        let _ = stream.set_nodelay(true);
                        serve_http_serve_connection(server_handle, stream, &running);
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }
        }
    }
}

/// `spectra.std.serve.http_start(server, port) -> bound_port`
///
/// Starts the OPTIONAL embedded HTTP/1.1 listener for an already-registered
/// serve server. `port == 0` binds an OS-assigned ephemeral port whose real
/// value is returned. The routes are fixed: POST /infer and GET /metrics.
pub(crate) extern "C" fn std_serve_http_start(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let server_handle = args[0];
    if !(0..=65_535).contains(&args[1]) {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], args[1] as u16));
    let listener = match mio::net::TcpListener::bind(address) {
        Ok(listener) => listener,
        Err(_) => return HOST_STATUS_INTERNAL_ERROR,
    };
    let Ok(local) = listener.local_addr() else {
        return HOST_STATUS_INTERNAL_ERROR;
    };
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let worker_running = std::sync::Arc::clone(&running);
    let worker = match std::thread::Builder::new()
        .name(format!("spectra-serve-http-{}", local.port()))
        .spawn(move || serve_http_worker(server_handle, listener, worker_running))
    {
        Ok(worker) => worker,
        Err(_) => return HOST_STATUS_INTERNAL_ERROR,
    };
    let mut registry = match lock_serve_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(server) = registry.servers.get_mut(server_handle) else {
        running.store(false, std::sync::atomic::Ordering::Relaxed);
        return HOST_STATUS_NOT_FOUND;
    };
    if server.http.is_some() {
        running.store(false, std::sync::atomic::Ordering::Relaxed);
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    server.http = Some(ServeHttpRuntime {
        port: local.port(),
        running,
        worker: Some(worker),
    });
    results[0] = local.port() as SpectraHostValue;
    HOST_STATUS_SUCCESS
}

/// `spectra.std.serve.http_stop(server) -> 1`
///
/// Signals the embedded listener worker to exit, joins it, and detaches the
/// runtime from the server. Unknown server or never-started listener rejects.
pub(crate) extern "C" fn std_serve_http_stop(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    // Take the runtime out first, then release the registry lock before
    // joining: an in-flight request on the worker needs the same lock.
    let runtime = {
        let mut registry = match lock_serve_registry() {
            Ok(registry) => registry,
            Err(status) => return status,
        };
        let Some(server) = registry.servers.get_mut(args[0]) else {
            return HOST_STATUS_NOT_FOUND;
        };
        server.http.take()
    };
    let Some(mut runtime) = runtime else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    runtime.running.store(false, std::sync::atomic::Ordering::Relaxed);
    if let Some(worker) = runtime.worker.take() {
        let _ = worker.join();
    }
    results[0] = 1;
    HOST_STATUS_SUCCESS
}
