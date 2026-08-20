use crate::http::{
    self, BodyChunk, Header, Http1Parser, HttpBody, HttpVersion, Method, ParseErrorKind,
    ParsedRequest, ParsedResponse, ParserConfig, Request, Response,
};
use crate::{handler, routing};
use crate::handles::ApiHandleTable;
use crate::{read_args, write_result};
use spectra_runtime::ffi::{
    lookup_host_function, SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
    HOST_STATUS_SUCCESS,
};
use spectra_runtime::tracing::{self, SpanKind, SpanStatus};
use spectra_runtime::handles::HandleKind;
use spectra_runtime::metrics::{self, MetricsRegistry};
use mio::net::{TcpListener as MioTcpListener, TcpStream as MioTcpStream};
use mio::{Events, Interest, Poll, Token};
use std::fmt;
use std::io::{Read, Write};
use std::collections::HashSet;
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const SERVER_STATE_CREATED: SpectraHostValue = 1;
pub const SERVER_STATE_RUNNING: SpectraHostValue = 2;
pub const SERVER_STATE_STOPPED: SpectraHostValue = 3;
pub const SERVER_STATE_STOPPING: SpectraHostValue = 4;
pub const SERVER_SIGNAL_SIGINT: SpectraHostValue = 2;
pub const SERVER_SIGNAL_SIGTERM: SpectraHostValue = 15;

const DEFAULT_READ_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_IDLE_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_SHUTDOWN_GRACE_MS: u64 = 5_000;
const DEFAULT_MAX_CONNECTIONS: usize = 10_000;
const MAX_STREAM_WRITE_BUFFER: usize = 4 * 1024 * 1024;

pub type Handler = Arc<dyn Fn(ParsedRequest) -> ServerResponse + Send + Sync + 'static>;

#[derive(Clone, Copy, Debug)]
pub struct PendingResponse {
    pub(crate) task: SpectraHostValue,
}

pub(crate) enum HandlerResult {
    Ready(ServerResponse),
    Pending(PendingResponse),
    Sse(Arc<crate::sse::RoutedSseResponse>),
}

pub(crate) type DispatchHandler =
    Arc<dyn Fn(ParsedRequest) -> HandlerResult + Send + Sync + 'static>;

const LISTENER_TOKEN: Token = Token(0);

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub bind_addr: SocketAddr,
    pub max_header_bytes: usize,
    pub max_body_bytes: usize,
    pub max_chunk_bytes: usize,
    pub read_timeout: Duration,
    pub idle_timeout: Duration,
    pub shutdown_grace_period: Duration,
    pub max_connections: usize,
    pub poll_interval: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:0"
                .parse()
                .expect("default server bind address is valid"),
            max_header_bytes: 64 * 1024,
            max_body_bytes: 16 * 1024 * 1024,
            max_chunk_bytes: 8 * 1024 * 1024,
            read_timeout: Duration::from_millis(DEFAULT_READ_TIMEOUT_MS),
            idle_timeout: Duration::from_millis(DEFAULT_IDLE_TIMEOUT_MS),
            shutdown_grace_period: Duration::from_millis(DEFAULT_SHUTDOWN_GRACE_MS),
            max_connections: DEFAULT_MAX_CONNECTIONS,
            poll_interval: Duration::from_millis(1),
        }
    }
}

impl ServerConfig {
    fn parser_config(&self) -> ParserConfig {
        ParserConfig {
            max_header_bytes: self.max_header_bytes,
            max_body_bytes: self.max_body_bytes,
            max_chunk_bytes: self.max_chunk_bytes,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ServerResponse {
    pub status_code: u16,
    pub reason: String,
    pub headers: Vec<Header>,
    pub body: HttpBody,
    pub close: bool,
}

impl ServerResponse {
    pub fn text(status_code: u16, body: impl Into<String>) -> Self {
        let body = body.into();
        Self {
            status_code,
            reason: reason_for_status(status_code).to_string(),
            headers: vec![Header {
                name: "Content-Type".to_string(),
                value: "text/plain; charset=utf-8".to_string(),
            }],
            body: HttpBody::from_bytes(body.into_bytes()),
            close: false,
        }
    }

    pub fn bytes(status_code: u16, body: Vec<u8>) -> Self {
        Self {
            status_code,
            reason: reason_for_status(status_code).to_string(),
            headers: Vec::new(),
            body: HttpBody::from_bytes(body),
            close: false,
        }
    }

    pub fn chunked(status_code: u16, chunks: Vec<Vec<u8>>) -> Self {
        Self {
            status_code,
            reason: reason_for_status(status_code).to_string(),
            headers: Vec::new(),
            body: HttpBody {
                chunks: chunks
                    .into_iter()
                    .map(|data| BodyChunk {
                        data,
                        extension: None,
                    })
                    .collect(),
                trailers: Vec::new(),
                chunked: true,
            },
            close: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerStats {
    pub accepted_connections: usize,
    pub completed_requests: usize,
    pub rejected_connections: usize,
    pub body_limit_violations: usize,
    pub timeouts: usize,
    pub parse_errors: usize,
    pub closed_connections: usize,
    pub drained_connections: usize,
    pub cancelled_connections: usize,
    pub shutdown_signals: usize,
    pub peak_connections: usize,
    pub active_connections: usize,
}

#[derive(Debug)]
pub enum ServerError {
    Io(std::io::Error),
    AlreadyStopped,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerError::Io(error) => write!(f, "server I/O error: {error}"),
            ServerError::AlreadyStopped => write!(f, "server is already stopped"),
        }
    }
}

impl std::error::Error for ServerError {}

impl From<std::io::Error> for ServerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub struct HttpServer {
    local_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    stats: Arc<Mutex<ServerStats>>,
    join: Option<JoinHandle<()>>,
    health: Arc<Mutex<spectra_runtime::health::HealthRegistry>>,
    metrics: Arc<Mutex<MetricsRegistry>>,
}

impl HttpServer {
    pub fn start(config: ServerConfig, handler: Handler) -> Result<Self, ServerError> {
        let dispatcher: DispatchHandler = Arc::new(move |request| {
            HandlerResult::Ready(handler(request))
        });
        Self::start_with_dispatcher(config, dispatcher)
    }

    pub(crate) fn start_with_dispatcher(
        config: ServerConfig,
        dispatcher: DispatchHandler,
    ) -> Result<Self, ServerError> {
        let listener = MioTcpListener::bind(config.bind_addr)?;
        let local_addr = listener.local_addr()?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(Mutex::new(ServerStats::default()));
        let loop_shutdown = Arc::clone(&shutdown);
        let loop_stats = Arc::clone(&stats);
        let health = Arc::new(Mutex::new(spectra_runtime::health::global()));
        let loop_health = Arc::clone(&health);
        let metrics = Arc::new(Mutex::new(metrics::global()));
        register_http_metrics(&metrics.lock().unwrap_or_else(|e| e.into_inner()));
        let loop_metrics = Arc::clone(&metrics);
        let join = thread::Builder::new()
            .name("spectra-api-http1-server".to_string())
            .spawn(move || {
                run_accept_loop(
                    listener,
                    config,
                    dispatcher,
                    loop_shutdown,
                    loop_stats,
                    loop_health,
                    loop_metrics,
                )
            })?;

        Ok(Self {
            local_addr,
            shutdown,
            stats,
            join: Some(join),
            health,
            metrics,
        })
    }

    /// Replaces the registry used by the reserved health routes.
    pub fn with_health_registry(self, registry: spectra_runtime::health::HealthRegistry) -> Self {
        *self.health.lock().unwrap_or_else(|e| e.into_inner()) = registry;
        self
    }

    /// Replaces the Prometheus registry used by this server.
    pub fn with_metrics_registry(self, registry: MetricsRegistry) -> Self {
        register_http_metrics(&registry);
        *self.metrics.lock().unwrap_or_else(|e| e.into_inner()) = registry;
        self
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn stats(&self) -> ServerStats {
        self.stats.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn shutdown(&mut self) -> Result<ServerStats, ServerError> {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = TcpStream::connect(self.local_addr);
            let _ = join.join();
            return Ok(self.stats());
        }
        Err(ServerError::AlreadyStopped)
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

struct Connection {
    token: Token,
    stream: MioTcpStream,
    parser: Http1Parser,
    write_buf: Vec<u8>,
    write_pos: usize,
    accepted_at: Instant,
    last_activity: Instant,
    close_after_write: bool,
    pending_response: Option<PendingConnectionResponse>,
    sse: Option<crate::sse::RoutedSseConnection>,
}

struct PendingConnectionResponse {
    task: SpectraHostValue,
    request: ParsedRequest,
    method: String,
    close: bool,
    request_started: Instant,
    trace_span: Option<u64>,
}

impl Connection {
    fn new(
        token: Token,
        stream: MioTcpStream,
        parser_config: ParserConfig,
        now: Instant,
    ) -> std::io::Result<Self> {
        stream.set_nodelay(true)?;
        Ok(Self {
            token,
            stream,
            parser: Http1Parser::request_with_config(parser_config),
            write_buf: Vec::new(),
            write_pos: 0,
            accepted_at: now,
            last_activity: now,
            close_after_write: false,
            pending_response: None,
            sse: None,
        })
    }

    fn has_pending_write(&self) -> bool {
        self.write_pos < self.write_buf.len()
    }
}

fn run_accept_loop(
    mut listener: MioTcpListener,
    config: ServerConfig,
    handler: DispatchHandler,
    shutdown: Arc<AtomicBool>,
    stats: Arc<Mutex<ServerStats>>,
    health: Arc<Mutex<spectra_runtime::health::HealthRegistry>>,
    metrics: Arc<Mutex<MetricsRegistry>>,
) {
    let Ok(mut poll) = Poll::new() else {
        return;
    };
    if poll
        .registry()
        .register(&mut listener, LISTENER_TOKEN, Interest::READABLE)
        .is_err()
    {
        return;
    }

    let mut connections = Vec::<Connection>::new();
    let mut next_token = 1usize;
    let parser_config = config.parser_config();
    let mut events = Events::with_capacity(1024);

    while !shutdown.load(Ordering::SeqCst) {
        if poll
            .poll(&mut events, Some(server_poll_timeout(config.poll_interval)))
            .is_err()
        {
            break;
        }
        let listener_ready = events.iter().any(|event| {
            event.token() == LISTENER_TOKEN && (event.is_readable() || event.is_error())
        });
        let ready_tokens = events
            .iter()
            .filter(|event| event.token() != LISTENER_TOKEN)
            .map(|event| event.token())
            .collect::<HashSet<_>>();
        if listener_ready {
            let mut accept_context = AcceptContext {
                poll: &mut poll,
                config: &config,
                parser_config: &parser_config,
                stats: &stats,
                connections: &mut connections,
                metrics: &metrics,
                next_token: &mut next_token,
            };
            accept_ready_connections(&mut listener, &mut accept_context);
        }
        service_connections(
            &config,
            &handler,
            &stats,
            &shutdown,
            &mut connections,
            false,
            &health,
            &metrics,
            &ready_tokens,
            &mut poll,
        );
        events.clear();
    }

    let drain_deadline = Instant::now() + config.shutdown_grace_period;
    while !connections.is_empty() && Instant::now() < drain_deadline {
        if poll
            .poll(&mut events, Some(server_poll_timeout(config.poll_interval)))
            .is_err()
        {
            break;
        }
        let ready_tokens = events.iter().map(|event| event.token()).collect::<HashSet<_>>();
        service_connections(
            &config,
            &handler,
            &stats,
            &shutdown,
            &mut connections,
            true,
            &health,
            &metrics,
            &ready_tokens,
            &mut poll,
        );
        events.clear();
    }

    for mut connection in connections.drain(..) {
        if let Some(pending) = connection.pending_response.take() {
            let _ = spectra_runtime::stdlib::cancel_task_handle(pending.task);
        }
        let _ = poll.registry().deregister(&mut connection.stream);
        let _ = connection.stream.shutdown(Shutdown::Both);
        record_cancel(&stats);
    }
}

fn server_poll_timeout(configured: Duration) -> Duration {
    if configured.is_zero() {
        Duration::from_millis(1)
    } else {
        configured
    }
}

fn reserved_health_response(
    request: &ParsedRequest,
    health: &Arc<Mutex<spectra_runtime::health::HealthRegistry>>,
) -> Option<ServerResponse> {
    let path = request.target.split('?').next().unwrap_or(request.target.as_str());
    if !matches!(path, "/healthz" | "/readyz" | "/startupz") { return None; }
    let span = tracing::span_start("health.request", SpanKind::Server).ok();
    if request.method != "GET" {
        let mut response = ServerResponse::text(405, "method not allowed");
        response.headers.push(Header { name: "Allow".into(), value: "GET".into() });
        if let Some(id) = span { let _ = tracing::span_set_status(id, SpanStatus::Error); let _ = tracing::span_end(id); }
        return Some(response);
    }
    let registry = health.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let endpoint = path.trim_start_matches('/');
    let status = match endpoint { "healthz" => 200, "startupz" if registry.startup() == spectra_runtime::health::HealthState::Healthy => 200, "readyz" if registry.readiness() != spectra_runtime::health::HealthState::Unavailable => 200, _ => 503 };
    let mut response = ServerResponse::bytes(status, registry.json(endpoint).into_bytes());
    response.headers.push(Header { name: "Content-Type".into(), value: "application/json".into() });
    if let Some(id) = span { let _ = tracing::span_set_attribute(id, "health.endpoint", endpoint); let _ = tracing::span_set_attribute_int(id, "http.response.status_code", status as i64); let _ = tracing::span_set_status(id, if status == 200 { SpanStatus::Ok } else { SpanStatus::Error }); let _ = tracing::span_end(id); }
    Some(response)
}

fn reserved_metrics_response(
    request: &ParsedRequest,
    metrics: &Arc<Mutex<MetricsRegistry>>,
) -> Option<ServerResponse> {
    let path = request.target.split('?').next().unwrap_or(request.target.as_str());
    if path != "/metrics" { return None; }
    if request.method != "GET" {
        let mut response = ServerResponse::text(405, "method not allowed");
        response.headers.push(Header { name: "Allow".into(), value: "GET".into() });
        return Some(response);
    }
    let registry = metrics.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut response = ServerResponse::bytes(200, registry.render_prometheus().into_bytes());
    response.headers.push(Header { name: "Content-Type".into(), value: "text/plain; version=0.0.4; charset=utf-8".into() });
    Some(response)
}

fn register_http_metrics(registry: &MetricsRegistry) {
    let _ = registry.register_counter("spectra_http_requests_total", "Total HTTP requests", &["method", "status"]);
    let _ = registry.register_histogram("spectra_http_request_duration_seconds", "HTTP request duration in seconds", &[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0], &["method"]);
    let _ = registry.register_counter("spectra_http_errors_total", "Total HTTP error responses", &["class"]);
    let _ = registry.register_gauge("spectra_http_active_connections", "Active HTTP connections", &[]);
    let _ = registry.register_counter("spectra_http_accepted_connections_total", "Accepted HTTP connections", &[]);
    let _ = registry.register_counter("spectra_http_timeouts_total", "HTTP connection timeouts", &[]);
}

fn metric_counter(registry: &Arc<Mutex<MetricsRegistry>>, name: &str, labels: &[(&str, &str)], value: f64) {
    let _ = registry.lock().unwrap_or_else(|e| e.into_inner()).counter_inc(name, labels, value);
}
fn metric_gauge(registry: &Arc<Mutex<MetricsRegistry>>, name: &str, value: f64, labels: &[(&str, &str)]) {
    let _ = registry.lock().unwrap_or_else(|e| e.into_inner()).gauge_set(name, labels, value);
}
fn metric_histogram(registry: &Arc<Mutex<MetricsRegistry>>, name: &str, labels: &[(&str, &str)], value: f64) {
    let _ = registry.lock().unwrap_or_else(|e| e.into_inner()).histogram_observe(name, labels, value);
}

struct AcceptContext<'a> {
    poll: &'a mut Poll,
    config: &'a ServerConfig,
    parser_config: &'a ParserConfig,
    stats: &'a Arc<Mutex<ServerStats>>,
    connections: &'a mut Vec<Connection>,
    metrics: &'a Arc<Mutex<MetricsRegistry>>,
    next_token: &'a mut usize,
}

fn accept_ready_connections(listener: &mut MioTcpListener, context: &mut AcceptContext<'_>) {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if context.connections.len() >= context.config.max_connections {
                    let _ = stream.shutdown(Shutdown::Both);
                    let mut stats = context.stats.lock().unwrap_or_else(|e| e.into_inner());
                    stats.rejected_connections += 1;
                    continue;
                }
                let token = Token(*context.next_token);
                *context.next_token = context.next_token.saturating_add(1);
                match Connection::new(
                    token,
                    stream,
                    context.parser_config.clone(),
                    Instant::now(),
                ) {
                    Ok(connection) => {
                        let mut connection = connection;
                        if context
                            .poll
                            .registry()
                            .register(&mut connection.stream, token, Interest::READABLE)
                            .is_err()
                        {
                            let _ = connection.stream.shutdown(Shutdown::Both);
                            let mut stats =
                                context.stats.lock().unwrap_or_else(|e| e.into_inner());
                            stats.rejected_connections += 1;
                            continue;
                        }
                        context.connections.push(connection);
                        let mut stats = context.stats.lock().unwrap_or_else(|e| e.into_inner());
                        stats.accepted_connections += 1;
                        stats.active_connections = context.connections.len();
                        stats.peak_connections = stats.peak_connections.max(context.connections.len());
                        metric_gauge(
                            context.metrics,
                            "spectra_http_active_connections",
                            context.connections.len() as f64,
                            &[],
                        );
                        metric_counter(
                            context.metrics,
                            "spectra_http_accepted_connections_total",
                            &[],
                            1.0,
                        );
                    }
                    Err(_) => {
                        let mut stats = context.stats.lock().unwrap_or_else(|e| e.into_inner());
                        stats.rejected_connections += 1;
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => break,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn service_connections(
    config: &ServerConfig,
    handler: &DispatchHandler,
    stats: &Arc<Mutex<ServerStats>>,
    shutdown: &Arc<AtomicBool>,
    connections: &mut Vec<Connection>,
    draining: bool,
    health: &Arc<Mutex<spectra_runtime::health::HealthRegistry>>,
    metrics: &Arc<Mutex<MetricsRegistry>>,
    ready_tokens: &HashSet<Token>,
    poll: &mut Poll,
) {
    let mut idx = 0usize;
    while idx < connections.len() {
        let ready = draining || ready_tokens.contains(&connections[idx].token);
        let action = service_connection(
            config,
            handler,
            stats,
            &mut connections[idx],
            draining,
            ready,
            health,
            metrics,
        );
        if action == ConnectionAction::Close {
            let mut connection = connections.swap_remove(idx);
            if let Some(pending) = connection.pending_response.take() {
                let _ = spectra_runtime::stdlib::cancel_task_handle(pending.task);
            }
            let _ = poll.registry().deregister(&mut connection.stream);
            let graceful = draining || shutdown.load(Ordering::SeqCst);
            let _ = connection.stream.shutdown(if graceful {
                Shutdown::Write
            } else {
                Shutdown::Both
            });
            record_close(stats, graceful);
        } else {
            let connection = &mut connections[idx];
            let interest = if connection.has_pending_write() {
                Interest::READABLE.add(Interest::WRITABLE)
            } else {
                Interest::READABLE
            };
            if poll
                .registry()
                .reregister(&mut connection.stream, connection.token, interest)
                .is_err()
            {
                let mut connection = connections.swap_remove(idx);
                if let Some(pending) = connection.pending_response.take() {
                    let _ = spectra_runtime::stdlib::cancel_task_handle(pending.task);
                }
                let _ = poll.registry().deregister(&mut connection.stream);
                let graceful = draining || shutdown.load(Ordering::SeqCst);
                let _ = connection.stream.shutdown(if graceful {
                    Shutdown::Write
                } else {
                    Shutdown::Both
                });
                record_close(stats, graceful);
                continue;
            }
            idx += 1;
        }
    }
    stats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .active_connections = connections.len();
    metric_gauge(metrics, "spectra_http_active_connections", connections.len() as f64, &[]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConnectionAction {
    Keep,
    Close,
}

struct ResponseCompletion<'a> {
    method: &'a str,
    close: bool,
    request_started: Instant,
    trace_span: Option<u64>,
    stats: &'a Arc<Mutex<ServerStats>>,
    metrics: &'a Arc<Mutex<MetricsRegistry>>,
}

fn complete_handled_response(
    connection: &mut Connection,
    response: ServerResponse,
    completion: ResponseCompletion<'_>,
) {
    let ResponseCompletion {
        method,
        close,
        request_started,
        trace_span,
        stats,
        metrics,
    } = completion;
    metric_counter(
        metrics,
        "spectra_http_requests_total",
        &[("method", method), ("status", &response.status_code.to_string())],
        1.0,
    );
    metric_histogram(
        metrics,
        "spectra_http_request_duration_seconds",
        &[("method", method)],
        request_started.elapsed().as_secs_f64(),
    );
    if response.status_code >= 400 {
        metric_counter(
            metrics,
            "spectra_http_errors_total",
            &[("class", if response.status_code >= 500 { "5xx" } else { "4xx" })],
            1.0,
        );
    }
    if let Some(id) = trace_span {
        let _ = tracing::span_set_attribute_int(
            id,
            "http.response.status_code",
            response.status_code as i64,
        );
        let _ = tracing::span_set_attribute_int(
            id,
            "http.response.body.size",
            response.body.bytes().len() as i64,
        );
        let _ = tracing::span_set_status(
            id,
            if response.status_code < 500 {
                SpanStatus::Ok
            } else {
                SpanStatus::Error
            },
        );
        let _ = tracing::span_end(id);
    }
    queue_response(connection, response, method == "HEAD", close);
    stats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .completed_requests += 1;
}

fn complete_routed_sse_response(
    connection: &mut Connection,
    headers: Vec<u8>,
    stream: crate::sse::RoutedSseConnection,
    completion: ResponseCompletion<'_>,
) {
    let ResponseCompletion {
        method,
        request_started,
        trace_span,
        stats,
        metrics,
        ..
    } = completion;
    metric_counter(
        metrics,
        "spectra_http_requests_total",
        &[("method", method), ("status", "200")],
        1.0,
    );
    metric_histogram(
        metrics,
        "spectra_http_request_duration_seconds",
        &[("method", method)],
        request_started.elapsed().as_secs_f64(),
    );
    if let Some(id) = trace_span {
        let _ = tracing::span_set_attribute_int(id, "http.response.status_code", 200);
        let _ = tracing::span_set_attribute_int(id, "http.response.body.size", 0);
        let _ = tracing::span_set_status(id, SpanStatus::Ok);
        let _ = tracing::span_end(id);
    }
    connection.write_buf.extend_from_slice(&headers);
    connection.sse = Some(stream);
    connection.close_after_write = false;
    stats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .completed_requests += 1;
}

fn service_pending_response(
    config: &ServerConfig,
    connection: &mut Connection,
    stats: &Arc<Mutex<ServerStats>>,
    metrics: &Arc<Mutex<MetricsRegistry>>,
) -> bool {
    let Some(pending) = connection.pending_response.as_ref() else {
        return true;
    };
    let task = pending.task;
    let ready = match spectra_runtime::stdlib::poll_task_once(task) {
        Ok(ready) => ready,
        Err(_) => {
            let pending = connection.pending_response.take().expect("pending response");
            complete_handled_response(
                connection,
                ServerResponse::text(500, "async handler failed"),
                ResponseCompletion {
                    method: &pending.method,
                    close: pending.close,
                    request_started: pending.request_started,
                    trace_span: pending.trace_span,
                    stats,
                    metrics,
                },
            );
            return true;
        }
    };
    if !ready {
        if connection
            .pending_response
            .as_ref()
            .map(|pending| pending.request_started.elapsed() > config.read_timeout)
            .unwrap_or(false)
        {
            let pending = connection.pending_response.take().expect("pending response");
            let _ = spectra_runtime::stdlib::cancel_task_handle(pending.task);
            complete_handled_response(
                connection,
                ServerResponse::text(504, "async handler timeout"),
                ResponseCompletion {
                    method: &pending.method,
                    close: true,
                    request_started: pending.request_started,
                    trace_span: pending.trace_span,
                    stats,
                    metrics,
                },
            );
            return true;
        }
        return false;
    }

    let pending = connection.pending_response.take().expect("pending response");
    let response_handle = spectra_runtime::stdlib::task_result_value(pending.task).ok();
    if let Some(route_response) = response_handle
        .and_then(crate::sse::routed_response_for_handle)
    {
        match route_response.open(&pending.request) {
            Ok((headers, stream)) => complete_routed_sse_response(
                connection,
                headers,
                stream,
                ResponseCompletion {
                    method: &pending.method,
                    close: pending.close,
                    request_started: pending.request_started,
                    trace_span: pending.trace_span,
                    stats,
                    metrics,
                },
            ),
            Err(error) => complete_handled_response(
                connection,
                ServerResponse::text(400, error.to_string()),
                ResponseCompletion {
                    method: &pending.method,
                    close: pending.close,
                    request_started: pending.request_started,
                    trace_span: pending.trace_span,
                    stats,
                    metrics,
                },
            ),
        }
    } else {
        let response = response_handle
            .and_then(http::clone_response)
            .map(server_response_from_http)
            .unwrap_or_else(|| ServerResponse::text(500, "async handler failed"));
        complete_handled_response(
            connection,
            response,
            ResponseCompletion {
                method: &pending.method,
                close: pending.close,
                request_started: pending.request_started,
                trace_span: pending.trace_span,
                stats,
                metrics,
            },
        );
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn service_connection(
    config: &ServerConfig,
    handler: &DispatchHandler,
    stats: &Arc<Mutex<ServerStats>>,
    connection: &mut Connection,
    draining: bool,
    ready: bool,
    health: &Arc<Mutex<spectra_runtime::health::HealthRegistry>>,
    metrics: &Arc<Mutex<MetricsRegistry>>,
) -> ConnectionAction {
    if ready && write_pending(connection).is_err() {
        return ConnectionAction::Close;
    }
    if connection.close_after_write && !connection.has_pending_write() {
        return ConnectionAction::Close;
    }

    if connection.pending_response.is_some() && !service_pending_response(config, connection, stats, metrics) {
        return ConnectionAction::Keep;
    }

    if connection.sse.is_some() {
        if ready {
            let mut probe = [0_u8; 1];
            match connection.stream.read(&mut probe) {
                Ok(0) => {
                    if let Some(stream) = connection.sse.take() {
                        stream.close();
                    }
                    return ConnectionAction::Close;
                }
                Ok(_) => {
                    if let Some(stream) = connection.sse.take() {
                        stream.close();
                    }
                    return ConnectionAction::Close;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return ConnectionAction::Close,
            }
        }
        let mut queued = Vec::new();
        let alive = connection
            .sse
            .as_ref()
            .map(|stream| stream.poll_into(Instant::now(), &mut queued))
            .unwrap_or(false);
        if !alive {
            if let Some(stream) = connection.sse.take() {
                stream.close();
            }
            return ConnectionAction::Close;
        }
        if !queued.is_empty() {
            if connection
                .write_buf
                .len()
                .saturating_add(queued.len())
                > MAX_STREAM_WRITE_BUFFER
            {
                if let Some(stream) = connection.sse.take() {
                    stream.close();
                }
                return ConnectionAction::Close;
            }
            connection.write_buf.extend_from_slice(&queued);
        }
        if ready && write_pending(connection).is_err() {
            return ConnectionAction::Close;
        }
        return ConnectionAction::Keep;
    }

    let now = Instant::now();
    if connection.parser.buffered_len() > 0
        && now.duration_since(connection.last_activity) > config.read_timeout
    {
        record_timeout(stats);
        metric_counter(metrics, "spectra_http_timeouts_total", &[], 1.0);
        queue_error_response(connection, 408, "request timeout");
        return if connection.has_pending_write() {
            ConnectionAction::Keep
        } else {
            ConnectionAction::Close
        };
    }
    if connection.parser.buffered_len() == 0
        && now.duration_since(connection.last_activity) > config.idle_timeout
    {
        record_timeout(stats);
        metric_counter(metrics, "spectra_http_timeouts_total", &[], 1.0);
        return ConnectionAction::Close;
    }
    if connection.parser.buffered_len() == 0
        && now.duration_since(connection.accepted_at) > config.idle_timeout
    {
        record_timeout(stats);
        return ConnectionAction::Close;
    }

    if !ready {
        return ConnectionAction::Keep;
    }

    let mut read_buf = [0_u8; 8192];
    loop {
        match connection.stream.read(&mut read_buf) {
            Ok(0) => return ConnectionAction::Close,
            Ok(n) => {
                connection.last_activity = Instant::now();
                connection.parser.push(&read_buf[..n]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => return ConnectionAction::Close,
        }
    }

    loop {
        match connection.parser.parse_next_request() {
            Ok(Some(request)) => {
                let request_started = Instant::now();
                let close = !request.keep_alive;
                let method = request.method.clone();
                let extracted_parent = request
                    .headers
                    .iter()
                    .find(|header| header.name.eq_ignore_ascii_case("traceparent"))
                    .and_then(|header| tracing::extract(&header.value).ok());
                let trace_span = tracing::span_start_with_parent(
                    "http.server",
                    SpanKind::Server,
                    extracted_parent,
                )
                .ok();
                if let Some(id) = trace_span {
                    let _ = tracing::span_set_attribute(id, "http.request.method", &method);
                    let _ = tracing::span_set_attribute(id, "url.path", &request.target);
                }
                let request_for_stream = request.clone();
                let result = reserved_metrics_response(&request, metrics)
                    .or_else(|| reserved_health_response(&request, health))
                    .map(HandlerResult::Ready)
                    .unwrap_or_else(|| handler(request));
                match result {
                    HandlerResult::Ready(response) => {
                        complete_handled_response(
                            connection,
                            response,
                            ResponseCompletion {
                                method: &method,
                                close,
                                request_started,
                                trace_span,
                                stats,
                                metrics,
                            },
                        );
                        if write_pending(connection).is_err() {
                            return ConnectionAction::Close;
                        }
                        if connection.close_after_write {
                            break;
                        }
                    }
                    HandlerResult::Sse(route_response) => {
                        match route_response.open(&request_for_stream) {
                            Ok((headers, stream)) => complete_routed_sse_response(
                                connection,
                                headers,
                                stream,
                                ResponseCompletion {
                                    method: &method,
                                    close,
                                    request_started,
                                    trace_span,
                                    stats,
                                    metrics,
                                },
                            ),
                            Err(error) => complete_handled_response(
                                connection,
                                ServerResponse::text(400, error.to_string()),
                                ResponseCompletion {
                                    method: &method,
                                    close,
                                    request_started,
                                    trace_span,
                                    stats,
                                    metrics,
                                },
                            ),
                        }
                        if write_pending(connection).is_err() {
                            return ConnectionAction::Close;
                        }
                        break;
                    }
                    HandlerResult::Pending(pending) => {
                        connection.pending_response = Some(PendingConnectionResponse {
                            task: pending.task,
                            request: request_for_stream,
                            method,
                            close,
                            request_started,
                            trace_span,
                        });
                        break;
                    }
                }
            }
            Ok(None) => break,
            Err(error) => {
                if error.kind == ParseErrorKind::BodyTooLarge {
                    stats
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .body_limit_violations += 1;
                    queue_error_response(connection, 413, "payload too large");
                    metric_counter(metrics, "spectra_http_errors_total", &[("class", "4xx")], 1.0);
                } else {
                    stats.lock().unwrap_or_else(|e| e.into_inner()).parse_errors += 1;
                    queue_error_response(connection, 400, "bad request");
                    metric_counter(metrics, "spectra_http_errors_total", &[("class", "4xx")], 1.0);
                }
                break;
            }
        }
    }

    if !connection.has_pending_write()
        && (connection.close_after_write
            || (draining && connection.parser.buffered_len() == 0))
    {
        ConnectionAction::Close
    } else {
        ConnectionAction::Keep
    }
}

fn write_pending(connection: &mut Connection) -> std::io::Result<()> {
    while connection.write_pos < connection.write_buf.len() {
        match connection
            .stream
            .write(&connection.write_buf[connection.write_pos..])
        {
            Ok(0) => break,
            Ok(n) => {
                connection.write_pos += n;
                connection.last_activity = Instant::now();
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(error),
        }
    }
    if connection.write_pos >= connection.write_buf.len() {
        connection.write_buf.clear();
        connection.write_pos = 0;
    }
    Ok(())
}

fn queue_response(
    connection: &mut Connection,
    response: ServerResponse,
    head_only: bool,
    request_close: bool,
) {
    let close = response.close || request_close;
    let wire = response_to_wire(response, head_only, close);
    connection.write_buf.extend_from_slice(&wire);
    connection.close_after_write = close;
}

fn queue_error_response(connection: &mut Connection, status_code: u16, message: &str) {
    let mut response = ServerResponse::text(status_code, message.to_string());
    response.close = true;
    queue_response(connection, response, false, true);
}

fn response_to_wire(mut response: ServerResponse, head_only: bool, close: bool) -> Vec<u8> {
    upsert_header(
        &mut response.headers,
        "Connection",
        if close { "close" } else { "keep-alive" },
    );
    if response.body.chunked {
        remove_header(&mut response.headers, "Content-Length");
        upsert_header(&mut response.headers, "Transfer-Encoding", "chunked");
    } else {
        remove_header(&mut response.headers, "Transfer-Encoding");
        upsert_header(
            &mut response.headers,
            "Content-Length",
            &response.body.bytes().len().to_string(),
        );
    }

    let body = if head_only {
        HttpBody::empty()
    } else {
        response.body
    };
    let parsed = ParsedResponse {
        version: HttpVersion::HTTP_11,
        status_code: response.status_code,
        reason: response.reason,
        headers: response.headers,
        body,
        keep_alive: !close,
    };
    serialize_response_for_server(&parsed)
}

fn serialize_response_for_server(response: &ParsedResponse) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(response.version.to_string().as_bytes());
    out.push(b' ');
    out.extend_from_slice(response.status_code.to_string().as_bytes());
    out.push(b' ');
    out.extend_from_slice(response.reason.as_bytes());
    out.extend_from_slice(b"\r\n");
    for header in &response.headers {
        out.extend_from_slice(header.name.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(header.value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"\r\n");
    if response.body.chunked {
        for chunk in &response.body.chunks {
            out.extend_from_slice(format!("{:X}\r\n", chunk.data.len()).as_bytes());
            out.extend_from_slice(&chunk.data);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"0\r\n\r\n");
    } else {
        out.extend_from_slice(&response.body.bytes());
    }
    out
}

fn upsert_header(headers: &mut Vec<Header>, name: &str, value: &str) {
    if let Some(header) = headers
        .iter_mut()
        .find(|header| header.name.eq_ignore_ascii_case(name))
    {
        header.value = value.to_string();
    } else {
        headers.push(Header {
            name: name.to_string(),
            value: value.to_string(),
        });
    }
}

fn remove_header(headers: &mut Vec<Header>, name: &str) {
    headers.retain(|header| !header.name.eq_ignore_ascii_case(name));
}

fn record_timeout(stats: &Arc<Mutex<ServerStats>>) {
    stats.lock().unwrap_or_else(|e| e.into_inner()).timeouts += 1;
}

fn record_close(stats: &Arc<Mutex<ServerStats>>, drained: bool) {
    let mut stats = stats.lock().unwrap_or_else(|e| e.into_inner());
    stats.closed_connections += 1;
    if drained {
        stats.drained_connections += 1;
    }
    stats.active_connections = stats.active_connections.saturating_sub(1);
}

fn record_cancel(stats: &Arc<Mutex<ServerStats>>) {
    let mut stats = stats.lock().unwrap_or_else(|e| e.into_inner());
    stats.cancelled_connections += 1;
    stats.closed_connections += 1;
    stats.active_connections = stats.active_connections.saturating_sub(1);
}

fn reason_for_status(status_code: u16) -> &'static str {
    match status_code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

#[cfg(test)]
#[derive(Debug)]
struct ConnectionLimiter {
    max: usize,
    active: usize,
    peak: usize,
    rejected: usize,
}

#[cfg(test)]
impl ConnectionLimiter {
    fn new(max: usize) -> Self {
        Self {
            max,
            active: 0,
            peak: 0,
            rejected: 0,
        }
    }

    fn try_open(&mut self) -> bool {
        if self.active >= self.max {
            self.rejected += 1;
            return false;
        }
        self.active += 1;
        self.peak = self.peak.max(self.active);
        true
    }

    fn close(&mut self) {
        self.active = self.active.saturating_sub(1);
    }
}

struct ServerEntry {
    state: SpectraHostValue,
    config: ServerConfig,
    server: Option<HttpServer>,
    last_stats: ServerStats,
}

struct ServerStore {
    entries: ApiHandleTable<ServerEntry>,
}

impl ServerStore {
    fn new() -> Self {
        Self {
            entries: ApiHandleTable::new(HandleKind::ApiServerEntry),
        }
    }

    fn server_handle(&mut self) -> SpectraHostValue {
        self.entries.insert(ServerEntry {
                state: SERVER_STATE_CREATED,
                config: ServerConfig::default(),
                server: None,
                last_stats: ServerStats::default(),
            })
    }
}

fn store() -> &'static Mutex<ServerStore> {
    static STORE: OnceLock<Mutex<ServerStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(ServerStore::new()))
}
