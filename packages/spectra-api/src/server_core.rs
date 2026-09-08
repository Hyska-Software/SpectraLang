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
#[cfg(unix)]
use std::os::unix::io::{FromRawFd, IntoRawFd};
#[cfg(windows)]
use std::os::windows::io::{FromRawSocket, IntoRawSocket};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
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
const DEFAULT_MAX_WORKER_THREADS: usize = 8;
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
    WebSocket(Arc<crate::websocket::RoutedUpgradeState>),
}

pub(crate) type DispatchHandler =
    Arc<dyn Fn(ParsedRequest) -> HandlerResult + Send + Sync + 'static>;

/// Monotonic per-server connection identity, assigned at accept time.
static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

/// Parsed request handed to an off-thread handler worker.
struct HandlerJob {
    conn_id: u64,
    request: ParsedRequest,
    /// W3C context of the `http.server` span so spans created on the
    /// worker thread (db.*, handler children) keep the request's trace
    /// instead of starting orphan roots.
    context: Option<tracing::TraceContext>,
}

/// Handler outcome produced by a worker. SSE and WebSocket variants carry
/// only the routed handle: `open`/upgrade touch the mio stream and MUST run
/// on the event-loop thread.
enum OffloadedOutcome {
    Ready(ServerResponse),
    Sse(Arc<crate::sse::RoutedSseResponse>),
    WebSocket(Arc<crate::websocket::RoutedUpgradeState>),
    Pending(PendingResponse),
    Panicked,
}

struct HandlerCompletion {
    conn_id: u64,
    outcome: OffloadedOutcome,
}

/// Per-connection request state retained while a worker runs the handler.
struct AwaitingWorker {
    request: ParsedRequest,
    method: String,
    close: bool,
    request_started: Instant,
    trace_span: Option<u64>,
}

const HANDLER_JOB_CHANNEL_CAPACITY: usize = 16_384;

fn offload_handler_result(result: HandlerResult) -> OffloadedOutcome {
    match result {
        HandlerResult::Ready(response) => OffloadedOutcome::Ready(response),
        HandlerResult::Sse(route_response) => OffloadedOutcome::Sse(route_response),
        HandlerResult::WebSocket(state) => OffloadedOutcome::WebSocket(state),
        HandlerResult::Pending(pending) => OffloadedOutcome::Pending(pending),
    }
}

const LISTENER_TOKEN: Token = Token(0);

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub bind_addr: SocketAddr,
    pub worker_threads: usize,
    pub max_header_bytes: usize,
    pub max_body_bytes: usize,
    pub max_chunk_bytes: usize,
    pub read_timeout: Duration,
    pub idle_timeout: Duration,
    pub shutdown_grace_period: Duration,
    pub max_connections: usize,
    pub poll_interval: Duration,
    /// When present, `serve` additionally starts a dedicated TLS gateway
    /// listener next to the cleartext mio HTTP/1.1 listener. The gateway is a
    /// tokio runtime thread performing TLS handshakes with ALPN fan-out:
    /// `h2` connections are served by the HTTP/2 pipeline, `http/1.1`
    /// connections by an async HTTP/1.1 loop that dispatches into the same
    /// handler as the mio event loop.
    pub tls_certificates: Option<Arc<crate::tls::TlsCertificateStore>>,
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
            worker_threads: default_worker_threads(),
            tls_certificates: None,
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

fn default_worker_threads() -> usize {
    std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1)
        .min(DEFAULT_MAX_WORKER_THREADS)
        .max(1)
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
    tls_gateway: Option<crate::http2::TlsGateway>,
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
        // Topology note: the HTTP/1.1 pipeline is mio-based and cannot wrap
        // TLS streams, while the h2 implementation is tokio-based. When TLS
        // certificates are configured we therefore run two listeners:
        //
        //   * `bind_addr` — cleartext HTTP/1.1 on the mio event loop
        //     (unchanged behavior, full SSE/WebSocket support).
        //   * an OS-assigned port on the same host — the TLS gateway. It
        //     negotiates ALPN per connection: `h2` is served by the existing
        //     HTTP/2 pipeline; `http/1.1` (or no ALPN) is served by an async
        //     HTTP/1.1 loop that dispatches through the same handler chain.
        //
        // `tls_local_addr` reports the gateway address and
        // `spectra.api.server.tls_local_port` exposes it to programs.
        let tls_gateway = match &config.tls_certificates {
            Some(certificates) => {
                let gateway_listener =
                    std::net::TcpListener::bind(SocketAddr::new(local_addr.ip(), 0))?;
                let options = crate::http2::TlsGatewayOptions {
                    parser_config: config.parser_config(),
                    read_timeout: config.read_timeout,
                    shutdown_grace_period: config.shutdown_grace_period,
                };
                Some(crate::http2::spawn_tls_gateway(
                    gateway_listener,
                    Arc::clone(certificates),
                    options,
                    Arc::clone(&dispatcher),
                )?)
            }
            None => None,
        };
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
            tls_gateway,
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

    /// Address of the TLS gateway listener, present only when the server was
    /// started with TLS certificates configured.
    pub fn tls_local_addr(&self) -> Option<SocketAddr> {
        self.tls_gateway.as_ref().map(|gateway| gateway.local_addr())
    }

    pub fn stats(&self) -> ServerStats {
        self.stats.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn shutdown(&mut self) -> Result<ServerStats, ServerError> {
        if let Some(gateway) = self.tls_gateway.as_mut() {
            gateway.shutdown();
        }
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
    id: u64,
    token: Token,
    stream: Option<MioTcpStream>,
    parser: Http1Parser,
    write_buf: Vec<u8>,
    write_pos: usize,
    accepted_at: Instant,
    last_activity: Instant,
    close_after_write: bool,
    pending_response: Option<PendingConnectionResponse>,
    awaiting_worker: Option<AwaitingWorker>,
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
            id: NEXT_CONNECTION_ID.fetch_add(1, Ordering::SeqCst),
            token,
            stream: Some(stream),
            parser: Http1Parser::request_with_config(parser_config),
            write_buf: Vec::new(),
            write_pos: 0,
            accepted_at: now,
            last_activity: now,
            close_after_write: false,
            pending_response: None,
            awaiting_worker: None,
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

    let pool = HandlerPool::spawn(config.worker_threads, &handler);
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
            &stats,
            &shutdown,
            &mut connections,
            false,
            &health,
            &metrics,
            &ready_tokens,
            &mut poll,
            &pool,
        );
        drain_handler_completions(
            &pool,
            &stats,
            &mut connections,
            &metrics,
            &mut poll,
            false,
            &shutdown,
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
            &stats,
            &shutdown,
            &mut connections,
            true,
            &health,
            &metrics,
            &ready_tokens,
            &mut poll,
            &pool,
        );
        drain_handler_completions(
            &pool,
            &stats,
            &mut connections,
            &metrics,
            &mut poll,
            true,
            &shutdown,
        );
        events.clear();
    }

    for mut connection in connections.drain(..) {
        if let Some(pending) = connection.pending_response.take() {
            let _ = spectra_runtime::stdlib::cancel_task_handle(pending.task);
        }
        if let Some(mut stream) = connection.stream.take() {
            let _ = poll.registry().deregister(&mut stream);
            let _ = stream.shutdown(Shutdown::Both);
        }
        record_cancel(&stats);
    }
    // Workers may still be finishing the last in-flight handlers; stop
    // accepting jobs and join them before the loop thread exits.
    pool.shutdown();
}

fn server_poll_timeout(configured: Duration) -> Duration {
    if configured.is_zero() {
        Duration::from_millis(1)
    } else {
        configured
    }
}

/// Fixed-size worker pool that runs request handlers off the mio event-loop
/// thread. Workers consume jobs from a bounded channel and report outcomes
/// back through an unbounded completion channel drained by the event loop.
struct HandlerPool {
    jobs: Option<mpsc::SyncSender<HandlerJob>>,
    completions: mpsc::Receiver<HandlerCompletion>,
    workers: Vec<JoinHandle<()>>,
}

impl HandlerPool {
    fn spawn(worker_threads: usize, handler: &DispatchHandler) -> Self {
        let (jobs, job_receiver) = mpsc::sync_channel::<HandlerJob>(HANDLER_JOB_CHANNEL_CAPACITY);
        let (completion_sender, completions) = mpsc::channel::<HandlerCompletion>();
        let job_receiver = Arc::new(Mutex::new(job_receiver));
        let workers = (0..worker_threads.max(1))
            .map(|index| {
                let job_receiver = Arc::clone(&job_receiver);
                let completion_sender = completion_sender.clone();
                let handler = Arc::clone(handler);
                thread::Builder::new()
                    .name(format!("spectra-api-handler-{index}"))
                    .spawn(move || loop {
                        let Ok(job) = job_receiver
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .recv()
                        else {
                            // All senders dropped: the event loop is shutting down.
                            break;
                        };
                        let outcome =
                            catch_unwind(AssertUnwindSafe(|| {
                                tracing::with_context(job.context, || handler(job.request))
                            }))
                            .map(offload_handler_result)
                            .unwrap_or(OffloadedOutcome::Panicked);

                        let _ = completion_sender.send(HandlerCompletion {
                            conn_id: job.conn_id,
                            outcome,
                        });
                    })
                    .expect("handler worker thread spawns")
            })
            .collect();
        drop(completion_sender);
        Self {
            jobs: Some(jobs),
            completions,
            workers,
        }
    }

    fn submit(
        &self,
        conn_id: u64,
        request: ParsedRequest,
        context: Option<tracing::TraceContext>,
    ) -> Result<(), mpsc::TrySendError<HandlerJob>> {
        self.jobs
            .as_ref()
            .expect("handler pool submits only while running")
            .try_send(HandlerJob {
                conn_id,
                request,
                context,
            })
    }

    /// Stops accepting jobs, waits for in-flight handlers to finish, and
    /// joins every worker. Called once the event loop finished draining.
    fn shutdown(mut self) {
        self.jobs.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
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
                        let registered = connection
                            .stream
                            .as_mut()
                            .map(|stream| {
                                context
                                    .poll
                                    .registry()
                                    .register(stream, token, Interest::READABLE)
                                    .is_ok()
                            })
                            .unwrap_or(false);
                        if !registered {
                            if let Some(stream) = connection.stream.take() {
                                let _ = stream.shutdown(Shutdown::Both);
                            }
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
    stats: &Arc<Mutex<ServerStats>>,
    shutdown: &Arc<AtomicBool>,
    connections: &mut Vec<Connection>,
    draining: bool,
    health: &Arc<Mutex<spectra_runtime::health::HealthRegistry>>,
    metrics: &Arc<Mutex<MetricsRegistry>>,
    ready_tokens: &HashSet<Token>,
    poll: &mut Poll,
    pool: &HandlerPool,
) {
    let mut idx = 0usize;
    while idx < connections.len() {
        let ready = draining || ready_tokens.contains(&connections[idx].token);
        let action = service_connection(
            config,
            stats,
            &mut connections[idx],
            draining,
            ready,
            health,
            metrics,
            pool,
        );
        match action {
            ConnectionAction::Close => {
                let mut connection = connections.swap_remove(idx);
                if let Some(pending) = connection.pending_response.take() {
                    let _ = spectra_runtime::stdlib::cancel_task_handle(pending.task);
                }
                if let Some(mut stream) = connection.stream.take() {
                    let _ = poll.registry().deregister(&mut stream);
                    let graceful = draining || shutdown.load(Ordering::SeqCst);
                    let _ = stream.shutdown(if graceful {
                        Shutdown::Write
                    } else {
                        Shutdown::Both
                    });
                }
                record_close(stats, draining || shutdown.load(Ordering::SeqCst));
            }
            ConnectionAction::Upgrade(upgrade) => {
                let mut connection = connections.swap_remove(idx);
                if let Some(pending) = connection.pending_response.take() {
                    let _ = spectra_runtime::stdlib::cancel_task_handle(pending.task);
                }
                execute_websocket_upgrade(upgrade, poll);
            }
            ConnectionAction::Keep => {
                let connection = &mut connections[idx];
                let interest = if connection.has_pending_write() {
                    Interest::READABLE.add(Interest::WRITABLE)
                } else {
                    Interest::READABLE
                };
                let registered = connection
                    .stream
                    .as_mut()
                    .map(|stream| {
                        poll.registry()
                            .reregister(stream, connection.token, interest)
                            .is_ok()
                    })
                    .unwrap_or(false);
                if !registered {
                    let mut connection = connections.swap_remove(idx);
                    if let Some(pending) = connection.pending_response.take() {
                        let _ = spectra_runtime::stdlib::cancel_task_handle(pending.task);
                    }
                    if let Some(mut stream) = connection.stream.take() {
                        let _ = poll.registry().deregister(&mut stream);
                        let graceful = draining || shutdown.load(Ordering::SeqCst);
                        let _ = stream.shutdown(if graceful {
                            Shutdown::Write
                        } else {
                            Shutdown::Both
                        });
                    }
                    record_close(stats, draining || shutdown.load(Ordering::SeqCst));
                    continue;
                }
                idx += 1;
            }
        }
    }
    stats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .active_connections = connections.len();
    metric_gauge(metrics, "spectra_http_active_connections", connections.len() as f64, &[]);
}

struct PendingWebSocketUpgrade {
    stream: MioTcpStream,
    request: ParsedRequest,
    buffered: Vec<u8>,
    state: Arc<crate::websocket::RoutedUpgradeState>,
}

enum ConnectionAction {
    Keep,
    Close,
    Upgrade(PendingWebSocketUpgrade),
}

/// Hands a completed WebSocket handshake to the routed upgrade workers.
/// Shared by the inline dispatch path and the offload completion path.
fn execute_websocket_upgrade(mut upgrade: PendingWebSocketUpgrade, poll: &mut Poll) {
    let _ = poll.registry().deregister(&mut upgrade.stream);
    if let Ok(stream) = mio_stream_into_std(upgrade.stream) {
        let _ = crate::websocket::enqueue_routed_upgrade(
            stream,
            upgrade.request,
            upgrade.buffered,
            upgrade.state,
        );
    }
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

/// Returns `true` when the connection finished waiting (completed or timed
/// out); `false` means it must keep waiting for its worker.
fn service_awaiting_worker(
    config: &ServerConfig,
    connection: &mut Connection,
    stats: &Arc<Mutex<ServerStats>>,
    metrics: &Arc<Mutex<MetricsRegistry>>,
) -> bool {
    let Some(awaiting) = connection.awaiting_worker.as_ref() else {
        return true;
    };
    if awaiting.request_started.elapsed() <= config.read_timeout {
        return false;
    }
    let awaiting = connection.awaiting_worker.take().expect("connection awaits worker");
    record_timeout(stats);
    metric_counter(metrics, "spectra_http_timeouts_total", &[], 1.0);
    complete_handled_response(
        connection,
        ServerResponse::text(504, "handler timeout"),
        ResponseCompletion {
            method: &awaiting.method,
            close: true,
            request_started: awaiting.request_started,
            trace_span: awaiting.trace_span,
            stats,
            metrics,
        },
    );
    // The worker's late completion is dropped: it no longer matches a
    // connection with an active `awaiting_worker`.
    true
}

/// Drains worker completions and finishes each response on the event-loop
/// thread. Completions whose connection closed or timed out are dropped.
#[allow(clippy::too_many_arguments)]
fn drain_handler_completions(
    pool: &HandlerPool,
    stats: &Arc<Mutex<ServerStats>>,
    connections: &mut Vec<Connection>,
    metrics: &Arc<Mutex<MetricsRegistry>>,
    poll: &mut Poll,
    draining: bool,
    shutdown: &Arc<AtomicBool>,
) {
    let graceful = draining || shutdown.load(Ordering::SeqCst);
    while let Ok(completion) = pool.completions.try_recv() {
        let Some(index) = connections.iter().position(|connection| {
            connection.id == completion.conn_id && connection.awaiting_worker.is_some()
        }) else {
            continue;
        };
        let mut connection = connections.swap_remove(index);
        let awaiting = connection
            .awaiting_worker
            .take()
            .expect("matched connection awaits worker");
        match completion.outcome {
            OffloadedOutcome::Ready(response) => {
                complete_handled_response(
                    &mut connection,
                    response,
                    ResponseCompletion {
                        method: &awaiting.method,
                        close: awaiting.close,
                        request_started: awaiting.request_started,
                        trace_span: awaiting.trace_span,
                        stats,
                        metrics,
                    },
                );
                if write_pending(&mut connection).is_err() {
                    retire_connection(connection, poll, stats, graceful);
                    continue;
                }
            }
            OffloadedOutcome::Panicked => {
                let mut response = ServerResponse::text(500, "handler panicked");
                response.close = true;
                complete_handled_response(
                    &mut connection,
                    response,
                    ResponseCompletion {
                        method: &awaiting.method,
                        close: true,
                        request_started: awaiting.request_started,
                        trace_span: awaiting.trace_span,
                        stats,
                        metrics,
                    },
                );
                if write_pending(&mut connection).is_err() {
                    retire_connection(connection, poll, stats, graceful);
                    continue;
                }
            }
            OffloadedOutcome::Sse(route_response) => match route_response.open(&awaiting.request)
            {
                Ok((headers, stream)) => {
                    complete_routed_sse_response(
                        &mut connection,
                        headers,
                        stream,
                        ResponseCompletion {
                            method: &awaiting.method,
                            close: awaiting.close,
                            request_started: awaiting.request_started,
                            trace_span: awaiting.trace_span,
                            stats,
                            metrics,
                        },
                    );
                    if write_pending(&mut connection).is_err() {
                        retire_connection(connection, poll, stats, graceful);
                        continue;
                    }
                }
                Err(error) => {
                    complete_handled_response(
                        &mut connection,
                        ServerResponse::text(400, error.to_string()),
                        ResponseCompletion {
                            method: &awaiting.method,
                            close: awaiting.close,
                            request_started: awaiting.request_started,
                            trace_span: awaiting.trace_span,
                            stats,
                            metrics,
                        },
                    );
                    if write_pending(&mut connection).is_err() {
                        retire_connection(connection, poll, stats, graceful);
                        continue;
                    }
                }
            },
            OffloadedOutcome::WebSocket(state) => {
                metric_counter(
                    metrics,
                    "spectra_http_requests_total",
                    &[("method", &awaiting.method), ("status", "101")],
                    1.0,
                );
                metric_histogram(
                    metrics,
                    "spectra_http_request_duration_seconds",
                    &[("method", &awaiting.method)],
                    awaiting.request_started.elapsed().as_secs_f64(),
                );
                if let Some(id) = awaiting.trace_span {
                    let _ = tracing::span_set_attribute_int(id, "http.response.status_code", 101);
                    let _ = tracing::span_set_status(id, SpanStatus::Ok);
                    let _ = tracing::span_end(id);
                }
                stats
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .completed_requests += 1;
                if let Some(stream) = connection.stream.take() {
                    execute_websocket_upgrade(
                        PendingWebSocketUpgrade {
                            stream,
                            request: awaiting.request,
                            buffered: connection.parser.take_buffered(),
                            state,
                        },
                        poll,
                    );
                    continue;
                }
                retire_connection(connection, poll, stats, graceful);
                continue;
            }
            OffloadedOutcome::Pending(pending) => {
                connection.pending_response = Some(PendingConnectionResponse {
                    task: pending.task,
                    request: awaiting.request,
                    method: awaiting.method,
                    close: awaiting.close,
                    request_started: awaiting.request_started,
                    trace_span: awaiting.trace_span,
                });
            }
        }
        connections.push(connection);
    }
}

/// Closes a connection from outside the regular service pass.
fn retire_connection(
    mut connection: Connection,
    poll: &mut Poll,
    stats: &Arc<Mutex<ServerStats>>,
    graceful: bool,
) {
    if let Some(pending) = connection.pending_response.take() {
        let _ = spectra_runtime::stdlib::cancel_task_handle(pending.task);
    }
    if let Some(mut stream) = connection.stream.take() {
        let _ = poll.registry().deregister(&mut stream);
        let _ = stream.shutdown(if graceful {
            Shutdown::Write
        } else {
            Shutdown::Both
        });
    }
    record_close(stats, graceful);
}
#[allow(clippy::too_many_arguments)]
fn service_connection(
    config: &ServerConfig,
    stats: &Arc<Mutex<ServerStats>>,
    connection: &mut Connection,
    draining: bool,
    ready: bool,
    health: &Arc<Mutex<spectra_runtime::health::HealthRegistry>>,
    metrics: &Arc<Mutex<MetricsRegistry>>,
    pool: &HandlerPool,
) -> ConnectionAction {
    if ready && write_pending(connection).is_err() {
        return ConnectionAction::Close;
    }
    if connection.close_after_write && !connection.has_pending_write() {
        return ConnectionAction::Close;
    }

    if connection.pending_response.is_some()
        && !service_pending_response(config, connection, stats, metrics)
    {
        return ConnectionAction::Keep;
    }

    if connection.awaiting_worker.is_some()
        && !service_awaiting_worker(config, connection, stats, metrics)
    {
        return ConnectionAction::Keep;
    }

    if connection.sse.is_some() {
        if ready {
            let mut probe = [0_u8; 1];
            match connection
                .stream
                .as_mut()
                .expect("SSE connection retains its stream")
                .read(&mut probe)
            {
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
        match connection
            .stream
            .as_mut()
            .expect("HTTP connection retains its stream")
            .read(&mut read_buf)
        {
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
                // Worker threads start with an empty context stack; carry the
                // freshly created `http.server` context so handler-created
                // spans (db.*, downstream calls) stay children of this span.
                let worker_context =
                    trace_span.and_then(|id| tracing::context(id).ok());
                let request_for_stream = request.clone();
                let reserved = reserved_metrics_response(&request, metrics)
                    .or_else(|| reserved_health_response(&request, health))
                    .map(HandlerResult::Ready);
                let result = match reserved {
                    Some(result) => result,
                    None => match pool.submit(connection.id, request, worker_context) {
                        Ok(()) => {
                            connection.awaiting_worker = Some(AwaitingWorker {
                                request: request_for_stream,
                                method,
                                close,
                                request_started,
                                trace_span,
                            });
                            // Response arrives through the completion channel;
                            // buffered pipelined bytes stay in the parser until
                            // this request finishes.
                            break;
                        }
                        Err(_) => {
                            let mut response =
                                ServerResponse::text(503, "worker pool unavailable");
                            response.close = true;
                            HandlerResult::Ready(response)
                        }
                    },
                };
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
                    HandlerResult::WebSocket(state) => {
                        metric_counter(
                            metrics,
                            "spectra_http_requests_total",
                            &[("method", &method), ("status", "101")],
                            1.0,
                        );
                        metric_histogram(
                            metrics,
                            "spectra_http_request_duration_seconds",
                            &[("method", &method)],
                            request_started.elapsed().as_secs_f64(),
                        );
                        if let Some(id) = trace_span {
                            let _ = tracing::span_set_attribute_int(
                                id,
                                "http.response.status_code",
                                101,
                            );
                            let _ = tracing::span_set_status(id, SpanStatus::Ok);
                            let _ = tracing::span_end(id);
                        }
                        stats
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .completed_requests += 1;
                        let Some(stream) = connection.stream.take() else {
                            return ConnectionAction::Close;
                        };
                        return ConnectionAction::Upgrade(PendingWebSocketUpgrade {
                            stream,
                            request: request_for_stream,
                            buffered: connection.parser.take_buffered(),
                            state,
                        });
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
            .as_mut()
            .expect("HTTP connection retains its stream while writing")
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

#[cfg(unix)]
fn mio_stream_into_std(stream: MioTcpStream) -> std::io::Result<TcpStream> {
    // Mio owns the descriptor until this conversion; transferring the raw
    // descriptor avoids cloning the connection and preserves any bytes that
    // the HTTP parser has already removed from the socket.
    Ok(unsafe { TcpStream::from_raw_fd(stream.into_raw_fd()) })
}

#[cfg(windows)]
fn mio_stream_into_std(stream: MioTcpStream) -> std::io::Result<TcpStream> {
    Ok(unsafe { TcpStream::from_raw_socket(stream.into_raw_socket()) })
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

pub(crate) fn response_to_wire(mut response: ServerResponse, head_only: bool, close: bool) -> Vec<u8> {
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
