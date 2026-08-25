use crate::handler::HandlerError;
use crate::handles::ApiHandleTable;
use crate::http::{self, Request, Response};
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::json;
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
};
use spectra_runtime::handles::HandleKind;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type MiddlewareFuture =
    Pin<Box<dyn Future<Output = Result<(Response, MiddlewareTrace), HandlerError>> + Send>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MiddlewareDecision {
    Continue(Request),
    ShortCircuit(Response),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MiddlewareTrace {
    events: Vec<String>,
    short_circuited: bool,
}

impl MiddlewareTrace {
    pub fn push(&mut self, event: impl Into<String>) {
        self.events.push(event.into());
    }

    pub fn events(&self) -> &[String] {
        &self.events
    }

    pub fn short_circuited(&self) -> bool {
        self.short_circuited
    }
}

#[derive(Clone, Debug, Default)]
pub struct MiddlewareContext {
    trace: MiddlewareTrace,
    cors_origin: Option<String>,
    request_id: Option<String>,
    method: Option<String>,
    path: Option<String>,
    started_at: Option<Instant>,
    security_csp: Option<String>,
    security_permissions: Option<String>,
    compression_encoding: Option<CompressionEncoding>,
    compression_vary: bool,
}

impl MiddlewareContext {
    pub fn trace(&self) -> &MiddlewareTrace {
        &self.trace
    }

    pub fn trace_mut(&mut self) -> &mut MiddlewareTrace {
        &mut self.trace
    }

    pub fn set_cors_origin(&mut self, origin: impl Into<String>) {
        self.cors_origin = Some(origin.into());
    }

    pub fn cors_origin(&self) -> Option<&str> {
        self.cors_origin.as_deref()
    }

    /// Returns the stable request identifier assigned for this chain execution.
    ///
    /// The identifier is created before the first middleware request hook runs,
    /// so every middleware in the chain observes the same value regardless of
    /// registration order.
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    fn begin_request(&mut self, request: &Request) {
        self.request_id = Some(next_request_id());
        self.method = Some(request.method.as_str().to_string());
        self.path = Some(request.path.clone());
        self.started_at = Some(Instant::now());
    }
}

static REQUEST_ID_RNG: LazyLock<SystemRandom> = LazyLock::new(SystemRandom::new);

/// Degraded-mode fallback used only if the OS entropy source fails; see
/// [`next_request_id`].
static REQUEST_ID_FALLBACK_SEQ: AtomicU64 = AtomicU64::new(1);

/// Generates an unpredictable request identifier: `req-` followed by 32
/// lowercase hex digits drawn from the OS CSPRNG
/// (`ring::rand::SystemRandom`). Consumers treat the id as an opaque
/// correlation token (log lines and `logging_request_id` pass it through),
/// so replacing the previous predictable `req-<16-hex counter>` format is
/// safe. If the entropy source fails — practically only on a broken OS — the
/// identifier degrades to the old monotonic sequence rather than failing the
/// request.
fn next_request_id() -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = [0_u8; 16];
    let rng: &SystemRandom = &REQUEST_ID_RNG;
    if rng.fill(&mut bytes).is_err() {
        let sequence = REQUEST_ID_FALLBACK_SEQ.fetch_add(1, Ordering::Relaxed);
        return format!("req-{sequence:016x}");
    }
    let mut id = String::with_capacity("req-".len() + 2 * bytes.len());
    id.push_str("req-");
    for byte in bytes {
        id.push(HEX[(byte >> 4) as usize] as char);
        id.push(HEX[(byte & 0x0f) as usize] as char);
    }
    id
}

pub trait Middleware: Send + Sync + 'static {
    fn on_request(
        &self,
        request: Request,
        context: &mut MiddlewareContext,
    ) -> Result<MiddlewareDecision, HandlerError>;

    fn on_response(
        &self,
        response: Response,
        _context: &mut MiddlewareContext,
    ) -> Result<Response, HandlerError> {
        Ok(response)
    }

    fn on_error(
        &self,
        error: HandlerError,
        _context: &mut MiddlewareContext,
    ) -> Result<Response, HandlerError> {
        Err(error)
    }
}

pub trait AsyncMiddleware: Send + Sync + 'static {
    fn on_request<'a>(
        &'a self,
        request: Request,
        context: &'a mut MiddlewareContext,
    ) -> Pin<Box<dyn Future<Output = Result<MiddlewareDecision, HandlerError>> + Send + 'a>>;

    fn on_response<'a>(
        &'a self,
        response: Response,
        _context: &'a mut MiddlewareContext,
    ) -> Pin<Box<dyn Future<Output = Result<Response, HandlerError>> + Send + 'a>> {
        Box::pin(async move { Ok(response) })
    }

    fn on_error<'a>(
        &'a self,
        error: HandlerError,
        _context: &'a mut MiddlewareContext,
    ) -> Pin<Box<dyn Future<Output = Result<Response, HandlerError>> + Send + 'a>> {
        Box::pin(async move { Err(error) })
    }
}

#[derive(Clone)]
enum MiddlewareEntry {
    Sync(Arc<dyn Middleware>),
    Async(Arc<dyn AsyncMiddleware>),
}

#[derive(Clone, Default)]
pub struct MiddlewareChain {
    entries: Vec<MiddlewareEntry>,
}

impl MiddlewareChain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn use_sync<M>(mut self, middleware: M) -> Self
    where
        M: Middleware,
    {
        self.entries
            .push(MiddlewareEntry::Sync(Arc::new(middleware)));
        self
    }

    pub fn use_async<M>(mut self, middleware: M) -> Self
    where
        M: AsyncMiddleware,
    {
        self.entries
            .push(MiddlewareEntry::Async(Arc::new(middleware)));
        self
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn execute_sync(
        &self,
        request: Request,
        terminal_response: Response,
    ) -> Result<(Response, MiddlewareTrace), HandlerError> {
        let mut context = MiddlewareContext::default();
        context.begin_request(&request);
        let result = self.execute_sync_inner(request, terminal_response, &mut context);
        match result {
            Ok(response) => Ok((response, context.trace)),
            Err(error) => self.recover_sync_error(error, &mut context),
        }
    }

    fn execute_sync_inner(
        &self,
        request: Request,
        terminal_response: Response,
        context: &mut MiddlewareContext,
    ) -> Result<Response, HandlerError> {
        let mut current_request = request;
        let mut executed = Vec::new();
        let mut response = None;

        for (index, entry) in self.entries.iter().enumerate() {
            match entry {
                MiddlewareEntry::Sync(middleware) => {
                    match middleware.on_request(current_request.clone(), context)? {
                        MiddlewareDecision::Continue(next) => {
                            current_request = next;
                            executed.push(index);
                        }
                        MiddlewareDecision::ShortCircuit(short) => {
                            context.trace.short_circuited = true;
                            executed.push(index);
                            response = Some(short);
                            break;
                        }
                    }
                }
                MiddlewareEntry::Async(_) => {
                    return Err(HandlerError::new(
                        500,
                        "async middleware requires execute_async",
                    ));
                }
            }
        }

        let mut response = response.unwrap_or(terminal_response);
        for index in executed.into_iter().rev() {
            if let MiddlewareEntry::Sync(middleware) = &self.entries[index] {
                response = middleware.on_response(response, context)?;
            }
        }

        Ok(response)
    }

    pub async fn execute_async(
        &self,
        request: Request,
        terminal_response: Response,
    ) -> Result<(Response, MiddlewareTrace), HandlerError> {
        let mut context = MiddlewareContext::default();
        context.begin_request(&request);
        let result = self
            .execute_async_inner(request, terminal_response, &mut context)
            .await;
        match result {
            Ok(response) => Ok((response, context.trace)),
            Err(error) => self.recover_async_error(error, &mut context).await,
        }
    }

    async fn execute_async_inner(
        &self,
        request: Request,
        terminal_response: Response,
        context: &mut MiddlewareContext,
    ) -> Result<Response, HandlerError> {
        let mut current_request = request;
        let mut executed = Vec::new();
        let mut response = None;

        for (index, entry) in self.entries.iter().enumerate() {
            let decision = match entry {
                MiddlewareEntry::Sync(middleware) => {
                    middleware.on_request(current_request.clone(), context)?
                }
                MiddlewareEntry::Async(middleware) => {
                    middleware
                        .on_request(current_request.clone(), context)
                        .await?
                }
            };

            match decision {
                MiddlewareDecision::Continue(next) => {
                    current_request = next;
                    executed.push(index);
                }
                MiddlewareDecision::ShortCircuit(short) => {
                    context.trace.short_circuited = true;
                    executed.push(index);
                    response = Some(short);
                    break;
                }
            }
        }

        let mut response = response.unwrap_or(terminal_response);
        for index in executed.into_iter().rev() {
            response = match &self.entries[index] {
                MiddlewareEntry::Sync(middleware) => middleware.on_response(response, context)?,
                MiddlewareEntry::Async(middleware) => {
                    middleware.on_response(response, context).await?
                }
            };
        }

        Ok(response)
    }

    fn recover_sync_error(
        &self,
        mut error: HandlerError,
        context: &mut MiddlewareContext,
    ) -> Result<(Response, MiddlewareTrace), HandlerError> {
        for entry in self.entries.iter().rev() {
            let MiddlewareEntry::Sync(middleware) = entry else {
                continue;
            };
            match middleware.on_error(error, context) {
                Ok(response) => return Ok((response, context.trace.clone())),
                Err(next) => error = next,
            }
        }
        Err(error)
    }

    async fn recover_async_error(
        &self,
        mut error: HandlerError,
        context: &mut MiddlewareContext,
    ) -> Result<(Response, MiddlewareTrace), HandlerError> {
        for entry in self.entries.iter().rev() {
            let result = match entry {
                MiddlewareEntry::Sync(middleware) => middleware.on_error(error, context),
                MiddlewareEntry::Async(middleware) => middleware.on_error(error, context).await,
            };
            match result {
                Ok(response) => return Ok((response, context.trace.clone())),
                Err(next) => error = next,
            }
        }
        Err(error)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RecordedMiddleware {
    before_marker: String,
    after_marker: String,
    short_circuit_response: Option<SpectraHostValue>,
}

impl Middleware for RecordedMiddleware {
    fn on_request(
        &self,
        request: Request,
        context: &mut MiddlewareContext,
    ) -> Result<MiddlewareDecision, HandlerError> {
        context.trace_mut().push(self.before_marker.clone());
        if let Some(response_handle) = self.short_circuit_response {
            let response = http::clone_response(response_handle)
                .ok_or_else(|| HandlerError::new(500, "invalid middleware response handle"))?;
            Ok(MiddlewareDecision::ShortCircuit(response))
        } else {
            Ok(MiddlewareDecision::Continue(request))
        }
    }

    fn on_response(
        &self,
        response: Response,
        context: &mut MiddlewareContext,
    ) -> Result<Response, HandlerError> {
        context.trace_mut().push(self.after_marker.clone());
        response
            .with_header("x-spectra-middleware", self.after_marker.clone())
            .map_err(|err| HandlerError::new(500, err.to_string()))
    }
}

impl AsyncMiddleware for RecordedMiddleware {
    fn on_request<'a>(
        &'a self,
        request: Request,
        context: &'a mut MiddlewareContext,
    ) -> Pin<Box<dyn Future<Output = Result<MiddlewareDecision, HandlerError>> + Send + 'a>> {
        Box::pin(async move { Middleware::on_request(self, request, context) })
    }

    fn on_response<'a>(
        &'a self,
        response: Response,
        context: &'a mut MiddlewareContext,
    ) -> Pin<Box<dyn Future<Output = Result<Response, HandlerError>> + Send + 'a>> {
        Box::pin(async move { Middleware::on_response(self, response, context) })
    }
}

/// Output format used by [`StructuredLoggingMiddleware`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestLogFormat {
    Json,
    Text,
}

impl RequestLogFormat {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "text" => Some(Self::Text),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Text => "text",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestLogRecord {
    request_id: String,
    method: String,
    path: String,
    status: u16,
    latency_us: u64,
}

impl RequestLogRecord {
    fn from_context(context: &MiddlewareContext, response: &Response) -> Option<Self> {
        let request_id = context.request_id()?.to_string();
        let method = context.method.as_deref()?.to_string();
        let path = context.path.as_deref()?.to_string();
        let latency_us = context
            .started_at
            .map(|started| started.elapsed().as_micros().min(u64::MAX as u128) as u64)
            .unwrap_or_default();
        Some(Self {
            request_id,
            method,
            path,
            status: response.status.code(),
            latency_us,
        })
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn line(&self, format: RequestLogFormat) -> String {
        match format {
            RequestLogFormat::Json => serde_json::to_string(&json!({
                "request_id": self.request_id,
                "method": self.method,
                "path": self.path,
                "status": self.status,
                "latency_us": self.latency_us,
            }))
            .expect("request log JSON values are always serializable"),
            RequestLogFormat::Text => format!(
                "request_id={} method={} path={} status={} latency_us={}",
                escape_log_text(&self.request_id),
                escape_log_text(&self.method),
                escape_log_text(&self.path),
                self.status,
                self.latency_us,
            ),
        }
    }
}

fn escape_log_text(value: &str) -> String {
    value.escape_default().collect()
}

#[derive(Clone, Debug)]
struct RequestLogEntry {
    record: RequestLogRecord,
    line: String,
}

type RequestLogRecords = Arc<Mutex<Vec<RequestLogEntry>>>;

/// Middleware that emits exactly one structured record when its request has
/// completed. The shared record buffer is also the native sink exposed by the
/// `std.api.middleware.logging_*` inspection functions; embedders can consume
/// the rendered lines without parsing stderr output.
#[derive(Clone)]
pub struct StructuredLoggingMiddleware {
    format: RequestLogFormat,
    records: RequestLogRecords,
}

impl StructuredLoggingMiddleware {
    pub fn new(format: RequestLogFormat) -> Self {
        Self {
            format,
            records: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn records(&self) -> RequestLogRecords {
        Arc::clone(&self.records)
    }

    fn record(&self, context: &MiddlewareContext, response: &Response) {
        let Some(record) = RequestLogRecord::from_context(context, response) else {
            return;
        };
        let line = record.line(self.format);
        self.records
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(RequestLogEntry { record, line });
    }
}

impl Middleware for StructuredLoggingMiddleware {
    fn on_request(
        &self,
        request: Request,
        context: &mut MiddlewareContext,
    ) -> Result<MiddlewareDecision, HandlerError> {
        // MiddlewareContext is initialized by MiddlewareChain before the first
        // hook. Keep this fallback for native embedders that invoke the trait
        // directly in tests.
        if context.request_id.is_none() {
            context.begin_request(&request);
        }
        Ok(MiddlewareDecision::Continue(request))
    }

    fn on_response(
        &self,
        response: Response,
        context: &mut MiddlewareContext,
    ) -> Result<Response, HandlerError> {
        self.record(context, &response);
        Ok(response)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateLimitAlgorithm {
    TokenBucket,
    SlidingWindow,
}

impl RateLimitAlgorithm {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "token_bucket" | "token-bucket" => Some(Self::TokenBucket),
            "sliding_window" | "sliding-window" => Some(Self::SlidingWindow),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateLimitScope {
    Global,
    Route,
    Tenant,
    User,
    ApiKey,
    RouteTenant,
    RouteUser,
}

impl RateLimitScope {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "global" => Some(Self::Global),
            "route" => Some(Self::Route),
            "tenant" => Some(Self::Tenant),
            "user" => Some(Self::User),
            "api_key" | "api-key" => Some(Self::ApiKey),
            "route_tenant" | "route-tenant" => Some(Self::RouteTenant),
            "route_user" | "route-user" => Some(Self::RouteUser),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct RateLimitConfig {
    algorithm: RateLimitAlgorithm,
    limit: u64,
    window: Duration,
    scope: RateLimitScope,
    dev_mode: bool,
}

enum RateLimitCounter {
    Bucket { tokens: f64, last: Instant },
    Sliding { events: VecDeque<Instant> },
}

impl RateLimitCounter {
    fn new(config: RateLimitConfig, now: Instant) -> Self {
        match config.algorithm {
            RateLimitAlgorithm::TokenBucket => Self::Bucket {
                tokens: config.limit as f64,
                last: now,
            },
            RateLimitAlgorithm::SlidingWindow => Self::Sliding {
                events: VecDeque::new(),
            },
        }
    }
}

struct RateLimitState {
    config: RateLimitConfig,
    counters: HashMap<String, RateLimitCounter>,
}

#[derive(Clone)]
pub struct RateLimitMiddleware {
    state: Arc<Mutex<RateLimitState>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RateLimitDecision {
    allowed: bool,
    remaining: u64,
    retry_after_ms: u64,
}

impl RateLimitMiddleware {
    pub fn new(
        algorithm: RateLimitAlgorithm,
        limit: u64,
        window_ms: u64,
        scope: RateLimitScope,
        dev_mode: bool,
    ) -> Option<Self> {
        let window = Duration::from_millis(window_ms);
        if limit == 0 || window.is_zero() {
            return None;
        }
        Some(Self {
            state: Arc::new(Mutex::new(RateLimitState {
                config: RateLimitConfig {
                    algorithm,
                    limit,
                    window,
                    scope,
                    dev_mode,
                },
                counters: HashMap::new(),
            })),
        })
    }

    fn update(&self, limit: u64, window_ms: u64) -> bool {
        if limit == 0 || window_ms == 0 {
            return false;
        }
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.config.dev_mode {
            return false;
        }
        state.config.limit = limit;
        state.config.window = Duration::from_millis(window_ms);
        state.counters.clear();
        true
    }

    fn key_for(scope: RateLimitScope, request: &Request) -> String {
        let tenant = || request.header("x-spectra-tenant").unwrap_or("anonymous");
        let user = || request.header("x-spectra-user").unwrap_or("anonymous");
        match scope {
            RateLimitScope::Global => "global".to_string(),
            RateLimitScope::Route => format!("route:{}", request.path),
            RateLimitScope::Tenant => format!("tenant:{}", tenant()),
            RateLimitScope::User => format!("user:{}", user()),
            RateLimitScope::ApiKey => format!(
                "api_key:{}",
                request
                    .header("x-spectra-api-key")
                    .or_else(|| request.header("x-api-key"))
                    .unwrap_or("anonymous")
            ),
            RateLimitScope::RouteTenant => {
                format!("route:{}|tenant:{}", request.path, tenant())
            }
            RateLimitScope::RouteUser => format!("route:{}|user:{}", request.path, user()),
        }
    }

    fn consume(&self, request: &Request) -> RateLimitDecision {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let config = state.config;
        let key = Self::key_for(config.scope, request);
        let counter = state
            .counters
            .entry(key)
            .or_insert_with(|| RateLimitCounter::new(config, now));
        let counter_mismatch = matches!(
            (config.algorithm, &*counter),
            (
                RateLimitAlgorithm::TokenBucket,
                RateLimitCounter::Sliding { .. }
            ) | (
                RateLimitAlgorithm::SlidingWindow,
                RateLimitCounter::Bucket { .. }
            )
        );
        if counter_mismatch {
            *counter = RateLimitCounter::new(config, now);
        }

        match counter {
            RateLimitCounter::Bucket { tokens, last } => {
                let rate_per_second = config.limit as f64 / config.window.as_secs_f64();
                let elapsed = now.duration_since(*last).as_secs_f64();
                *tokens = (*tokens + elapsed * rate_per_second).min(config.limit as f64);
                *last = now;
                if *tokens >= 1.0 {
                    *tokens -= 1.0;
                    RateLimitDecision {
                        allowed: true,
                        remaining: tokens.floor() as u64,
                        retry_after_ms: 0,
                    }
                } else {
                    let wait_seconds = (1.0 - *tokens) / rate_per_second;
                    RateLimitDecision {
                        allowed: false,
                        remaining: 0,
                        retry_after_ms: (wait_seconds * 1000.0).ceil().max(1.0) as u64,
                    }
                }
            }
            RateLimitCounter::Sliding { events } => {
                while events
                    .front()
                    .is_some_and(|started| now.duration_since(*started) >= config.window)
                {
                    events.pop_front();
                }
                if events.len() < config.limit as usize {
                    events.push_back(now);
                    RateLimitDecision {
                        allowed: true,
                        remaining: config.limit.saturating_sub(events.len() as u64),
                        retry_after_ms: 0,
                    }
                } else {
                    let retry_after_ms = events
                        .front()
                        .map(|started| {
                            config
                                .window
                                .saturating_sub(now.duration_since(*started))
                                .as_millis()
                                .max(1) as u64
                        })
                        .unwrap_or(1);
                    RateLimitDecision {
                        allowed: false,
                        remaining: 0,
                        retry_after_ms,
                    }
                }
            }
        }
    }

    fn rejection(decision: RateLimitDecision, limit: u64) -> Result<Response, HandlerError> {
        let retry_after_seconds = decision.retry_after_ms.saturating_add(999) / 1000;
        Response::new(http::Status::new(429).expect("429 is a valid HTTP status"))
            .with_header("retry-after", retry_after_seconds.to_string())
            .and_then(|response| response.with_header("x-ratelimit-limit", limit.to_string()))
            .and_then(|response| {
                response.with_header("x-ratelimit-remaining", decision.remaining.to_string())
            })
            .map(|response| response.with_body(b"Too Many Requests".to_vec()))
            .map_err(|error| HandlerError::new(500, error.to_string()))
    }
}

impl Middleware for RateLimitMiddleware {
    fn on_request(
        &self,
        request: Request,
        _context: &mut MiddlewareContext,
    ) -> Result<MiddlewareDecision, HandlerError> {
        let decision = self.consume(&request);
        if decision.allowed {
            Ok(MiddlewareDecision::Continue(request))
        } else {
            let limit = self
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .config
                .limit;
            Ok(MiddlewareDecision::ShortCircuit(Self::rejection(
                decision, limit,
            )?))
        }
    }

    fn on_response(
        &self,
        response: Response,
        _context: &mut MiddlewareContext,
    ) -> Result<Response, HandlerError> {
        Ok(response)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ApiKeySource {
    Header(String),
    Query(String),
}

impl ApiKeySource {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if let Some(name) = value.strip_prefix("header:") {
            return (!name.is_empty()).then(|| Self::Header(name.to_string()));
        }
        if let Some(name) = value.strip_prefix("query:") {
            return (!name.is_empty()).then(|| Self::Query(name.to_string()));
        }
        match value.to_ascii_lowercase().as_str() {
            "header" => Some(Self::Header("X-API-Key".to_string())),
            "query" => Some(Self::Query("api_key".to_string())),
            _ => None,
        }
    }

    fn extract<'a>(&self, request: &'a Request) -> Option<&'a str> {
        match self {
            Self::Header(name) => request.header(name),
            Self::Query(name) => query_parameter(&request.path, name),
        }
    }
}

fn query_parameter<'a>(path: &'a str, name: &str) -> Option<&'a str> {
    let query = path.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (candidate, value) = pair.split_once('=')?;
        (candidate == name).then_some(value)
    })
}

#[derive(Clone, Debug)]
struct ApiKeyRecord {
    expires_at_ms: Option<u64>,
    revoked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ApiKeyRejection {
    Missing,
    Unknown,
    Revoked,
    Expired,
}

impl ApiKeyRejection {
    fn reason(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Unknown => "unknown",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
        }
    }
}

#[derive(Clone)]
pub struct ApiKeyMiddleware {
    source: ApiKeySource,
    keys: Arc<Mutex<HashMap<String, ApiKeyRecord>>>,
}

impl ApiKeyMiddleware {
    fn new(source: ApiKeySource) -> Self {
        Self {
            source,
            keys: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn add(&self, key: String, expires_at_ms: u64) -> bool {
        if key.is_empty() {
            return false;
        }
        self.keys
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                key,
                ApiKeyRecord {
                    expires_at_ms: (expires_at_ms != 0).then_some(expires_at_ms),
                    revoked: false,
                },
            );
        true
    }

    fn revoke(&self, key: &str) -> bool {
        let mut keys = self.keys.lock().unwrap_or_else(|error| error.into_inner());
        let Some(record) = keys.get_mut(key) else {
            return false;
        };
        record.revoked = true;
        true
    }

    fn validate(&self, request: &Request) -> Result<String, ApiKeyRejection> {
        let key = self
            .source
            .extract(request)
            .filter(|value| !value.is_empty())
            .ok_or(ApiKeyRejection::Missing)?;
        let keys = self.keys.lock().unwrap_or_else(|error| error.into_inner());
        let record = keys.get(key).ok_or(ApiKeyRejection::Unknown)?;
        if record.revoked {
            return Err(ApiKeyRejection::Revoked);
        }
        if record
            .expires_at_ms
            .is_some_and(|expires_at| current_unix_time_ms().is_some_and(|now| now >= expires_at))
        {
            return Err(ApiKeyRejection::Expired);
        }
        Ok(key.to_string())
    }

    fn rejection(reason: ApiKeyRejection) -> Result<Response, HandlerError> {
        let payload = serde_json::to_string(&json!({
            "error": "invalid_api_key",
            "reason": reason.reason(),
        }))
        .expect("API key error JSON values are always serializable");
        Response::new(http::Status::new(401).expect("401 is a valid HTTP status"))
            .with_header("content-type", "application/problem+json")
            .map(|response| response.with_body(payload.into_bytes()))
            .map_err(|error| HandlerError::new(500, error.to_string()))
    }
}

fn current_unix_time_ms() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
}

impl Middleware for ApiKeyMiddleware {
    fn on_request(
        &self,
        request: Request,
        _context: &mut MiddlewareContext,
    ) -> Result<MiddlewareDecision, HandlerError> {
        match self.validate(&request) {
            Ok(key) => Ok(MiddlewareDecision::Continue(
                request
                    .with_header("x-spectra-api-key", key)
                    .map_err(|error| HandlerError::new(500, error.to_string()))?,
            )),
            Err(reason) => Ok(MiddlewareDecision::ShortCircuit(Self::rejection(reason)?)),
        }
    }

    fn on_response(
        &self,
        response: Response,
        _context: &mut MiddlewareContext,
    ) -> Result<Response, HandlerError> {
        Ok(response)
    }
}

const DEFAULT_SECURITY_CSP: &str = "default-src 'self'";
const DEFAULT_SECURITY_PERMISSIONS: &str = "geolocation=(), microphone=(), camera=()";
const DEFAULT_HSTS_MAX_AGE: &str = "31536000";

#[derive(Clone, Debug)]
struct SecurityRouteOverride {
    prefix: String,
    csp: String,
    permissions: String,
}

#[derive(Clone, Debug)]
struct SecurityHeadersConfig {
    csp: String,
    permissions: String,
    hsts: bool,
    include_subdomains: bool,
    preload: bool,
    routes: Vec<SecurityRouteOverride>,
}

impl Default for SecurityHeadersConfig {
    fn default() -> Self {
        Self {
            csp: DEFAULT_SECURITY_CSP.to_string(),
            permissions: DEFAULT_SECURITY_PERMISSIONS.to_string(),
            hsts: false,
            include_subdomains: false,
            preload: false,
            routes: Vec::new(),
        }
    }
}

impl SecurityHeadersConfig {
    fn valid_policy(header_name: &str, value: &str) -> Option<String> {
        let value = value.trim();
        if value.is_empty() || http::Header::new(header_name, value).is_err() {
            return None;
        }
        Some(value.to_string())
    }

    fn valid_prefix(value: &str) -> Option<String> {
        let value = value.trim();
        if value.is_empty() || !value.starts_with('/') || value.contains('?') || value.contains('#')
        {
            return None;
        }
        let value = if value.len() > 1 {
            value.trim_end_matches('/')
        } else {
            value
        };
        Some(value.to_string())
    }

    fn configure(
        &mut self,
        csp: &str,
        permissions: &str,
        hsts: bool,
        include_subdomains: bool,
        preload: bool,
    ) -> bool {
        let Some(csp) = Self::valid_policy("content-security-policy", csp) else {
            return false;
        };
        let Some(permissions) = Self::valid_policy("permissions-policy", permissions) else {
            return false;
        };
        if (include_subdomains || preload) && !hsts {
            return false;
        }
        self.csp = csp;
        self.permissions = permissions;
        self.hsts = hsts;
        self.include_subdomains = include_subdomains;
        self.preload = preload;
        true
    }

    fn configure_route(&mut self, prefix: &str, csp: &str, permissions: &str) -> bool {
        let Some(prefix) = Self::valid_prefix(prefix) else {
            return false;
        };
        let Some(csp) = Self::valid_policy("content-security-policy", csp) else {
            return false;
        };
        let Some(permissions) = Self::valid_policy("permissions-policy", permissions) else {
            return false;
        };

        if let Some(route) = self.routes.iter_mut().find(|route| route.prefix == prefix) {
            route.csp = csp;
            route.permissions = permissions;
        } else {
            self.routes.push(SecurityRouteOverride {
                prefix,
                csp,
                permissions,
            });
        }
        true
    }

    fn route_matches(prefix: &str, path: &str) -> bool {
        if prefix == "/" {
            return true;
        }
        path == prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|suffix| suffix.starts_with('/'))
    }

    fn policies_for(&self, path: &str) -> (String, String) {
        let path = path.split_once('?').map_or(path, |(path, _)| path);
        let route = self
            .routes
            .iter()
            .filter(|route| Self::route_matches(&route.prefix, path))
            .max_by_key(|route| route.prefix.len());
        route
            .map(|route| (route.csp.clone(), route.permissions.clone()))
            .unwrap_or_else(|| (self.csp.clone(), self.permissions.clone()))
    }

    fn hsts_value(&self) -> Option<String> {
        if !self.hsts {
            return None;
        }
        let mut value = format!("max-age={DEFAULT_HSTS_MAX_AGE}");
        if self.include_subdomains {
            value.push_str("; includeSubDomains");
        }
        if self.preload {
            value.push_str("; preload");
        }
        Some(value)
    }
}

#[derive(Clone)]
pub struct SecurityHeadersMiddleware {
    config: Arc<Mutex<SecurityHeadersConfig>>,
}

impl SecurityHeadersMiddleware {
    pub fn new() -> Self {
        Self {
            config: Arc::new(Mutex::new(SecurityHeadersConfig::default())),
        }
    }

    fn configure(
        &self,
        csp: &str,
        permissions: &str,
        hsts: bool,
        include_subdomains: bool,
        preload: bool,
    ) -> bool {
        self.config
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .configure(csp, permissions, hsts, include_subdomains, preload)
    }

    fn configure_route(&self, prefix: &str, csp: &str, permissions: &str) -> bool {
        self.config
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .configure_route(prefix, csp, permissions)
    }

    fn apply_headers(
        &self,
        response: Response,
        context: &MiddlewareContext,
    ) -> Result<Response, HandlerError> {
        let (csp, permissions, hsts) = {
            let config = self
                .config
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let (csp, permissions) = context
                .security_csp
                .as_deref()
                .zip(context.security_permissions.as_deref())
                .map(|(csp, permissions)| (csp.to_string(), permissions.to_string()))
                .unwrap_or_else(|| config.policies_for(context.path.as_deref().unwrap_or("/")));
            (csp, permissions, config.hsts_value())
        };

        let response = response
            .with_header("content-security-policy", csp)
            .and_then(|response| response.with_header("permissions-policy", permissions))
            .and_then(|response| response.with_header("x-frame-options", "DENY"))
            .and_then(|response| response.with_header("x-content-type-options", "nosniff"))
            .and_then(|response| {
                response.with_header("referrer-policy", "strict-origin-when-cross-origin")
            })
            .map_err(|error| HandlerError::new(500, error.to_string()))?;
        match hsts {
            Some(value) => response
                .with_header("strict-transport-security", value)
                .map_err(|error| HandlerError::new(500, error.to_string())),
            None => Ok(response),
        }
    }
}

impl Default for SecurityHeadersMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl Middleware for SecurityHeadersMiddleware {
    fn on_request(
        &self,
        request: Request,
        context: &mut MiddlewareContext,
    ) -> Result<MiddlewareDecision, HandlerError> {
        let (csp, permissions) = self
            .config
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .policies_for(&request.path);
        context.security_csp = Some(csp);
        context.security_permissions = Some(permissions);
        Ok(MiddlewareDecision::Continue(request))
    }

    fn on_response(
        &self,
        response: Response,
        context: &mut MiddlewareContext,
    ) -> Result<Response, HandlerError> {
        self.apply_headers(response, context)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompressionEncoding {
    Brotli,
    Gzip,
    Deflate,
}

impl CompressionEncoding {
    fn token(self) -> &'static str {
        match self {
            Self::Brotli => "br",
            Self::Gzip => "gzip",
            Self::Deflate => "deflate",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompressionMiddleware {
    threshold: usize,
}

impl CompressionMiddleware {
    pub fn new(threshold: usize) -> Self {
        Self { threshold }
    }

    fn parse_quality(value: &str) -> f32 {
        let Ok(quality) = value.trim().parse::<f32>() else {
            return 0.0;
        };
        if quality.is_finite() && (0.0..=1.0).contains(&quality) {
            quality
        } else {
            0.0
        }
    }

    fn preferences(value: &str) -> HashMap<String, f32> {
        let mut preferences: HashMap<String, f32> = HashMap::new();
        for item in value.split(',') {
            let mut parameters = item.split(';');
            let token = parameters
                .next()
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(str::to_ascii_lowercase);
            let Some(token) = token else {
                continue;
            };

            let mut quality = 1.0;
            for parameter in parameters {
                let parameter = parameter.trim();
                let Some((name, raw_value)) = parameter.split_once('=') else {
                    if parameter.eq_ignore_ascii_case("q") {
                        quality = 0.0;
                    }
                    continue;
                };
                if name.trim().eq_ignore_ascii_case("q") {
                    quality = Self::parse_quality(raw_value);
                    break;
                }
            }

            preferences
                .entry(token)
                .and_modify(|current| *current = current.max(quality))
                .or_insert(quality);
        }
        preferences
    }

    fn negotiate(value: Option<&str>) -> Option<CompressionEncoding> {
        let preferences = Self::preferences(value?);
        let wildcard = preferences.get("*").copied();
        let candidates = [
            CompressionEncoding::Brotli,
            CompressionEncoding::Gzip,
            CompressionEncoding::Deflate,
        ];
        let mut selected = None;
        let mut selected_quality = 0.0;
        for encoding in candidates {
            let quality = preferences
                .get(encoding.token())
                .copied()
                .or(wildcard)
                .unwrap_or(0.0);
            if quality > selected_quality {
                selected = Some(encoding);
                selected_quality = quality;
            }
        }
        selected
    }

    fn compress(encoding: CompressionEncoding, body: &[u8]) -> Result<Vec<u8>, HandlerError> {
        match encoding {
            CompressionEncoding::Gzip => {
                let mut encoder =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(body).map_err(|error| {
                    HandlerError::new(500, format!("gzip compression failed: {error}"))
                })?;
                encoder.finish().map_err(|error| {
                    HandlerError::new(500, format!("gzip compression failed: {error}"))
                })
            }
            CompressionEncoding::Deflate => {
                let mut encoder =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(body).map_err(|error| {
                    HandlerError::new(500, format!("deflate compression failed: {error}"))
                })?;
                encoder.finish().map_err(|error| {
                    HandlerError::new(500, format!("deflate compression failed: {error}"))
                })
            }
            CompressionEncoding::Brotli => {
                let mut compressed = Vec::new();
                {
                    let mut encoder = brotli::CompressorWriter::new(&mut compressed, 4096, 5, 22);
                    encoder.write_all(body).map_err(|error| {
                        HandlerError::new(500, format!("brotli compression failed: {error}"))
                    })?;
                    encoder.flush().map_err(|error| {
                        HandlerError::new(500, format!("brotli compression failed: {error}"))
                    })?;
                }
                Ok(compressed)
            }
        }
    }

    fn add_vary(response: Response) -> Result<Response, HandlerError> {
        let Some(existing) = response.header("vary").map(str::to_string) else {
            return response
                .with_header("vary", "Accept-Encoding")
                .map_err(|error| HandlerError::new(500, error.to_string()));
        };
        if existing.split(',').any(|value| {
            let value = value.trim();
            value == "*" || value.eq_ignore_ascii_case("accept-encoding")
        }) {
            return Ok(response);
        }
        response
            .with_header("vary", format!("{existing}, Accept-Encoding"))
            .map_err(|error| HandlerError::new(500, error.to_string()))
    }

    fn should_skip(response: &Response, context: &MiddlewareContext) -> bool {
        let status = response.status.code();
        status / 100 == 1
            || matches!(status, 204 | 205 | 304)
            || context.method.as_deref() == Some("HEAD")
            || response.body.is_empty()
            || response.header("content-encoding").is_some()
            || response.header("cache-control").is_some_and(|value| {
                value
                    .split(',')
                    .any(|directive| directive.trim().eq_ignore_ascii_case("no-transform"))
            })
    }

    fn apply(
        &self,
        mut response: Response,
        context: &MiddlewareContext,
    ) -> Result<Response, HandlerError> {
        if context.compression_vary {
            response = Self::add_vary(response)?;
        }
        let Some(encoding) = context.compression_encoding else {
            return Ok(response);
        };
        if Self::should_skip(&response, context) || response.body.len() < self.threshold {
            return Ok(response);
        }

        let had_content_length = response.header("content-length").is_some();
        let compressed = Self::compress(encoding, &response.body)?;
        response.body = compressed;
        let compressed_len = response.body.len();
        response = response
            .with_header("content-encoding", encoding.token())
            .map_err(|error| HandlerError::new(500, error.to_string()))?;
        if had_content_length {
            response = response
                .with_header("content-length", compressed_len.to_string())
                .map_err(|error| HandlerError::new(500, error.to_string()))?;
        }
        Ok(response)
    }
}

impl Middleware for CompressionMiddleware {
    fn on_request(
        &self,
        request: Request,
        context: &mut MiddlewareContext,
    ) -> Result<MiddlewareDecision, HandlerError> {
        context.compression_vary = true;
        context.compression_encoding = if request.method == http::Method::Head {
            None
        } else {
            Self::negotiate(request.header("accept-encoding"))
        };
        Ok(MiddlewareDecision::Continue(request))
    }

    fn on_response(
        &self,
        response: Response,
        context: &mut MiddlewareContext,
    ) -> Result<Response, HandlerError> {
        self.apply(response, context)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StoredMiddlewareKind {
    Sync,
    Async,
}

#[derive(Clone)]
struct StoredMiddleware {
    kind: StoredMiddlewareKind,
    entry: MiddlewareEntry,
}

struct MiddlewareStore {
    chains: ApiHandleTable<Vec<SpectraHostValue>>,
    middlewares: ApiHandleTable<StoredMiddleware>,
    traces: ApiHandleTable<MiddlewareTrace>,
    logs: HashMap<SpectraHostValue, RequestLogRecords>,
    rate_limits: HashMap<SpectraHostValue, RateLimitMiddleware>,
    api_keys: HashMap<SpectraHostValue, ApiKeyMiddleware>,
    security_headers: HashMap<SpectraHostValue, SecurityHeadersMiddleware>,
    last_trace: SpectraHostValue,
}

impl MiddlewareStore {
    fn new() -> Self {
        Self {
            chains: ApiHandleTable::new(HandleKind::ApiMiddlewareChain),
            middlewares: ApiHandleTable::new(HandleKind::ApiMiddleware),
            traces: ApiHandleTable::new(HandleKind::ApiMiddlewareTrace),
            logs: HashMap::new(),
            rate_limits: HashMap::new(),
            api_keys: HashMap::new(),
            security_headers: HashMap::new(),
            last_trace: 0,
        }
    }

    fn insert_chain(&mut self, entries: Vec<SpectraHostValue>) -> SpectraHostValue {
        self.chains.insert(entries)
    }

    fn insert_middleware(&mut self, middleware: StoredMiddleware) -> SpectraHostValue {
        self.middlewares.insert(middleware)
    }

    fn insert_trace(&mut self, trace: MiddlewareTrace) -> SpectraHostValue {
        let handle = self.traces.insert(trace);
        self.last_trace = handle;
        handle
    }
}

fn store() -> &'static Mutex<MiddlewareStore> {
    static STORE: OnceLock<Mutex<MiddlewareStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(MiddlewareStore::new()))
}

fn build_chain(
    chain_handle: SpectraHostValue,
    allow_async: bool,
) -> Result<MiddlewareChain, HandlerError> {
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let handles = store
        .chains
        .get(&chain_handle)
        .cloned()
        .ok_or_else(|| HandlerError::new(500, "middleware chain not found"))?;
    let mut chain = MiddlewareChain::new();
    for handle in handles {
        let middleware = store
            .middlewares
            .get(&handle)
            .cloned()
            .ok_or_else(|| HandlerError::new(500, "middleware handle not found"))?;
        match (middleware.kind, middleware.entry) {
            (StoredMiddlewareKind::Sync, MiddlewareEntry::Sync(middleware)) => {
                chain.entries.push(MiddlewareEntry::Sync(middleware));
            }
            (StoredMiddlewareKind::Async, MiddlewareEntry::Async(middleware)) if allow_async => {
                chain.entries.push(MiddlewareEntry::Async(middleware));
            }
            (StoredMiddlewareKind::Async, _) => {
                return Err(HandlerError::new(
                    500,
                    "async middleware requires execute_async",
                ));
            }
            _ => return Err(HandlerError::new(500, "middleware kind mismatch")),
        }
    }
    Ok(chain)
}

pub(crate) fn register_sync_middleware<M>(middleware: M) -> SpectraHostValue
where
    M: Middleware,
{
    store()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert_middleware(StoredMiddleware {
            kind: StoredMiddlewareKind::Sync,
            entry: MiddlewareEntry::Sync(Arc::new(middleware)),
        })
}

pub(crate) fn register_async_middleware<M>(middleware: M) -> SpectraHostValue
where
    M: AsyncMiddleware,
{
    store()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert_middleware(StoredMiddleware {
            kind: StoredMiddlewareKind::Async,
            entry: MiddlewareEntry::Async(Arc::new(middleware)),
        })
}

fn write_response_and_trace(
    ctx: *mut SpectraHostCallContext,
    result: Result<(Response, MiddlewareTrace), HandlerError>,
) -> i32 {
    match result {
        Ok((response, trace)) => {
            let response = http::store_response(response);
            store()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert_trace(trace);
            write_result(ctx, response)
        }
        Err(_) => HOST_STATUS_INVALID_ARGUMENT,
    }
}

fn block_on_ready_middleware(
    mut future: MiddlewareFuture,
) -> Result<(Response, MiddlewareTrace), HandlerError> {
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    fn wake(_: *const ()) {}
    fn wake_by_ref(_: *const ()) {}
    fn drop(_: *const ()) {}
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
    let raw = RawWaker::new(std::ptr::null(), &VTABLE);
    let waker = unsafe { Waker::from_raw(raw) };
    let mut cx = Context::from_waker(&waker);
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(value) => value,
        Poll::Pending => Err(HandlerError::new(
            500,
            "middleware future returned pending without a reactor",
        )),
    }
}

fn register_recorded(ctx: *mut SpectraHostCallContext, kind: StoredMiddlewareKind) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(before_marker), Some(after_marker)) =
        (read_spectra_string(args[0]), read_spectra_string(args[1]))
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let recorded = RecordedMiddleware {
        before_marker,
        after_marker,
        short_circuit_response: None,
    };
    let handle = match kind {
        StoredMiddlewareKind::Sync => register_sync_middleware(recorded),
        StoredMiddlewareKind::Async => register_async_middleware(recorded),
    };
    write_result(ctx, handle)
}

fn register_short_circuit(ctx: *mut SpectraHostCallContext, kind: StoredMiddlewareKind) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(before_marker), Some(after_marker)) =
        (read_spectra_string(args[0]), read_spectra_string(args[1]))
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if http::clone_response(args[2]).is_none() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let recorded = RecordedMiddleware {
        before_marker,
        after_marker,
        short_circuit_response: Some(args[2]),
    };
    let handle = match kind {
        StoredMiddlewareKind::Sync => register_sync_middleware(recorded),
        StoredMiddlewareKind::Async => register_async_middleware(recorded),
    };
    write_result(ctx, handle)
}

pub extern "C" fn chain(ctx: *mut SpectraHostCallContext) -> i32 {
    chain_new(ctx)
}

pub extern "C" fn chain_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let handle = store.insert_chain(Vec::new());
    write_result(ctx, handle)
}

pub extern "C" fn chain_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entries) = store.chains.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, entries.len() as SpectraHostValue)
}

pub extern "C" fn register_sync(ctx: *mut SpectraHostCallContext) -> i32 {
    register_recorded(ctx, StoredMiddlewareKind::Sync)
}

pub extern "C" fn register_sync_short_circuit(ctx: *mut SpectraHostCallContext) -> i32 {
    register_short_circuit(ctx, StoredMiddlewareKind::Sync)
}

pub extern "C" fn register_async(ctx: *mut SpectraHostCallContext) -> i32 {
    register_recorded(ctx, StoredMiddlewareKind::Async)
}

pub extern "C" fn register_async_short_circuit(ctx: *mut SpectraHostCallContext) -> i32 {
    register_short_circuit(ctx, StoredMiddlewareKind::Async)
}

/// Registers a sync middleware that records one structured request line on
/// response. The returned handle is a normal `MiddlewareHandle`, so it can be
/// appended with `use_sync` like every other sync middleware.
pub extern "C" fn register_logging(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(format_value) = read_spectra_string(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(format) = RequestLogFormat::parse(&format_value) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };

    let logging = StructuredLoggingMiddleware::new(format);
    let records = logging.records();
    let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
    let handle = store.insert_middleware(StoredMiddleware {
        kind: StoredMiddlewareKind::Sync,
        entry: MiddlewareEntry::Sync(Arc::new(logging)),
    });
    store.logs.insert(handle, records);
    write_result(ctx, handle)
}

pub extern "C" fn logging_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(records) = store.logs.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let length = records
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .len();
    write_result(ctx, length as SpectraHostValue)
}

pub extern "C" fn logging_line(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(index) = usize::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(records) = store.logs.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(entry) = records
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(index)
        .cloned()
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(&entry.line))
}

pub extern "C" fn logging_request_id(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(index) = usize::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(records) = store.logs.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(entry) = records
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(index)
        .cloned()
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(entry.record.request_id()))
}

pub extern "C" fn register_rate_limit(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 5) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(algorithm_value), Some(scope_value)) =
        (read_spectra_string(args[0]), read_spectra_string(args[3]))
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(algorithm) = RateLimitAlgorithm::parse(&algorithm_value) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(scope) = RateLimitScope::parse(&scope_value) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(limit) = u64::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(window_ms) = u64::try_from(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if args[4] != 0 && args[4] != 1 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let Some(rate_limit) =
        RateLimitMiddleware::new(algorithm, limit, window_ms, scope, args[4] == 1)
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
    let handle = store.insert_middleware(StoredMiddleware {
        kind: StoredMiddlewareKind::Sync,
        entry: MiddlewareEntry::Sync(Arc::new(rate_limit.clone())),
    });
    store.rate_limits.insert(handle, rate_limit);
    write_result(ctx, handle)
}

pub extern "C" fn rate_limit_update(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(limit) = u64::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(window_ms) = u64::try_from(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let rate_limit = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        store.rate_limits.get(&args[0]).cloned()
    };
    let Some(rate_limit) = rate_limit else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        if rate_limit.update(limit, window_ms) {
            1
        } else {
            0
        },
    )
}

pub extern "C" fn register_api_key(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(source_value) = read_spectra_string(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(source) = ApiKeySource::parse(&source_value) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let api_key = ApiKeyMiddleware::new(source);
    let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
    let handle = store.insert_middleware(StoredMiddleware {
        kind: StoredMiddlewareKind::Sync,
        entry: MiddlewareEntry::Sync(Arc::new(api_key.clone())),
    });
    store.api_keys.insert(handle, api_key);
    write_result(ctx, handle)
}

pub extern "C" fn api_key_add(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(key) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(expires_at_ms) = u64::try_from(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let api_key = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        store.api_keys.get(&args[0]).cloned()
    };
    let Some(api_key) = api_key else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        if api_key.add(key, expires_at_ms) {
            1
        } else {
            0
        },
    )
}

pub extern "C" fn api_key_revoke(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(key) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let api_key = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        store.api_keys.get(&args[0]).cloned()
    };
    let Some(api_key) = api_key else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, if api_key.revoke(&key) { 1 } else { 0 })
}

pub extern "C" fn register_security_headers(ctx: *mut SpectraHostCallContext) -> i32 {
    if read_args(ctx, 0).is_err() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let security_headers = SecurityHeadersMiddleware::new();
    let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
    let handle = store.insert_middleware(StoredMiddleware {
        kind: StoredMiddlewareKind::Sync,
        entry: MiddlewareEntry::Sync(Arc::new(security_headers.clone())),
    });
    store.security_headers.insert(handle, security_headers);
    write_result(ctx, handle)
}

pub extern "C" fn register_compression(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(threshold) = usize::try_from(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let compression = CompressionMiddleware::new(threshold);
    let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
    let handle = store.insert_middleware(StoredMiddleware {
        kind: StoredMiddlewareKind::Sync,
        entry: MiddlewareEntry::Sync(Arc::new(compression)),
    });
    write_result(ctx, handle)
}

pub extern "C" fn security_headers_configure(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 6) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(csp), Some(permissions)) =
        (read_spectra_string(args[1]), read_spectra_string(args[2]))
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(hsts) = host_bool(args[3]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(include_subdomains) = host_bool(args[4]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(preload) = host_bool(args[5]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let security_headers = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        store.security_headers.get(&args[0]).cloned()
    };
    let Some(security_headers) = security_headers else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        if security_headers.configure(&csp, &permissions, hsts, include_subdomains, preload) {
            1
        } else {
            0
        },
    )
}

pub extern "C" fn security_headers_route(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 4) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(prefix), Some(csp), Some(permissions)) = (
        read_spectra_string(args[1]),
        read_spectra_string(args[2]),
        read_spectra_string(args[3]),
    ) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let security_headers = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        store.security_headers.get(&args[0]).cloned()
    };
    let Some(security_headers) = security_headers else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        if security_headers.configure_route(&prefix, &csp, &permissions) {
            1
        } else {
            0
        },
    )
}

fn host_bool(value: SpectraHostValue) -> Option<bool> {
    match value {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

pub extern "C" fn use_sync(ctx: *mut SpectraHostCallContext) -> i32 {
    append_middleware(ctx, StoredMiddlewareKind::Sync)
}

pub extern "C" fn use_async(ctx: *mut SpectraHostCallContext) -> i32 {
    append_middleware(ctx, StoredMiddlewareKind::Async)
}

fn append_middleware(ctx: *mut SpectraHostCallContext, expected: StoredMiddlewareKind) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entries) = store.chains.get(&args[0]).cloned() else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(middleware) = store.middlewares.get(&args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if middleware.kind != expected {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut next_entries = entries;
    next_entries.push(args[1]);
    let handle = store.insert_chain(next_entries);
    write_result(ctx, handle)
}

pub extern "C" fn execute_sync(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(request) = http::clone_request(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(response) = http::clone_response(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let result =
        build_chain(args[0], false).and_then(|chain| chain.execute_sync(request, response));
    write_response_and_trace(ctx, result)
}

pub extern "C" fn execute_async(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(request) = http::clone_request(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(response) = http::clone_response(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let result = build_chain(args[0], true).and_then(|chain| {
        block_on_ready_middleware(Box::pin(async move {
            chain.execute_async(request, response).await
        }))
    });
    write_response_and_trace(ctx, result)
}

pub extern "C" fn last_trace(ctx: *mut SpectraHostCallContext) -> i32 {
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    write_result(ctx, store.last_trace)
}

pub extern "C" fn trace_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(trace) = store.traces.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, trace.events().len() as SpectraHostValue)
}

pub extern "C" fn trace_event(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(index) = usize::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(trace) = store.traces.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(event) = trace.events().get(index) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(event))
}

pub extern "C" fn trace_short_circuited(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(trace) = store.traces.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, if trace.short_circuited() { 1 } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, Status};
    use regex::Regex;
    use std::collections::HashSet;
    use std::io::Read;

    fn request() -> Request {
        Request::new(Method::Get, "/middleware").expect("valid request")
    }

    fn response(status: u16) -> Response {
        Response::new(Status::new(status).expect("valid status"))
    }

    fn block_on_ready<F: Future + ?Sized>(mut future: Pin<Box<F>>) -> F::Output {
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        fn wake(_: *const ()) {}
        fn wake_by_ref(_: *const ()) {}
        fn drop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
        let raw = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw) };
        let mut cx = Context::from_waker(&waker);
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("test future unexpectedly pending"),
        }
    }

    #[test]
    fn sync_chain_runs_request_order_and_response_reverse_order() {
        let chain = MiddlewareChain::new()
            .use_sync(RecordedMiddleware {
                before_marker: "first:request".to_string(),
                after_marker: "first:response".to_string(),
                short_circuit_response: None,
            })
            .use_sync(RecordedMiddleware {
                before_marker: "second:request".to_string(),
                after_marker: "second:response".to_string(),
                short_circuit_response: None,
            });

        let (response, trace) = chain
            .execute_sync(request(), response(200))
            .expect("middleware response");

        assert_eq!(response.status.code(), 200);
        assert_eq!(
            trace.events(),
            &[
                "first:request".to_string(),
                "second:request".to_string(),
                "second:response".to_string(),
                "first:response".to_string(),
            ]
        );
        assert!(!trace.short_circuited());
    }

    #[test]
    fn short_circuit_stops_remaining_requests_and_unwinds_executed_hooks() {
        let short = http::store_response(response(429));
        let chain = MiddlewareChain::new()
            .use_sync(RecordedMiddleware {
                before_marker: "first:request".to_string(),
                after_marker: "first:response".to_string(),
                short_circuit_response: None,
            })
            .use_sync(RecordedMiddleware {
                before_marker: "limit:request".to_string(),
                after_marker: "limit:response".to_string(),
                short_circuit_response: Some(short),
            })
            .use_sync(RecordedMiddleware {
                before_marker: "never:request".to_string(),
                after_marker: "never:response".to_string(),
                short_circuit_response: None,
            });

        let (response, trace) = chain
            .execute_sync(request(), response(200))
            .expect("short circuit response");

        assert_eq!(response.status.code(), 429);
        assert_eq!(
            trace.events(),
            &[
                "first:request".to_string(),
                "limit:request".to_string(),
                "limit:response".to_string(),
                "first:response".to_string(),
            ]
        );
        assert!(trace.short_circuited());
    }

    #[test]
    fn async_chain_accepts_sync_and_async_middleware() {
        let chain = MiddlewareChain::new()
            .use_sync(RecordedMiddleware {
                before_marker: "sync:request".to_string(),
                after_marker: "sync:response".to_string(),
                short_circuit_response: None,
            })
            .use_async(RecordedMiddleware {
                before_marker: "async:request".to_string(),
                after_marker: "async:response".to_string(),
                short_circuit_response: None,
            });

        let (response, trace) =
            block_on_ready(Box::pin(chain.execute_async(request(), response(204))))
                .expect("async middleware response");

        assert_eq!(response.status.code(), 204);
        assert_eq!(
            trace.events(),
            &[
                "sync:request".to_string(),
                "async:request".to_string(),
                "async:response".to_string(),
                "sync:response".to_string(),
            ]
        );
    }

    #[test]
    fn structured_logging_emits_json_with_request_identity_and_fields() {
        let logger = StructuredLoggingMiddleware::new(RequestLogFormat::Json);
        let records = logger.records();
        let chain = MiddlewareChain::new().use_sync(logger);
        let request = Request::new(Method::Post, "/orders").expect("valid request");

        let (response, _) = chain
            .execute_sync(request, response(201))
            .expect("logging chain response");

        assert_eq!(response.status.code(), 201);
        let entries = records.lock().expect("record lock");
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert!(entry.record.request_id().starts_with("req-"));
        let value: serde_json::Value = serde_json::from_str(&entry.line).expect("valid JSON");
        assert_eq!(value["request_id"], entry.record.request_id());
        assert_eq!(value["method"], "POST");
        assert_eq!(value["path"], "/orders");
        assert_eq!(value["status"], 201);
        assert!(value["latency_us"].is_u64());
    }

    #[test]
    fn structured_logging_text_records_short_circuit_response() {
        let logger = StructuredLoggingMiddleware::new(RequestLogFormat::Text);
        let records = logger.records();
        let chain = MiddlewareChain::new()
            .use_sync(logger)
            .use_sync(RecordedMiddleware {
                before_marker: "limit:request".to_string(),
                after_marker: "limit:response".to_string(),
                short_circuit_response: Some(http::store_response(response(429))),
            });

        chain
            .execute_sync(request(), response(200))
            .expect("short circuit response");

        let entries = records.lock().expect("record lock");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].line.contains("method=GET"));
        assert!(entries[0].line.contains("path=/middleware"));
        assert!(entries[0].line.contains("status=429"));
        assert!(entries[0]
            .line
            .contains(&format!("request_id={}", entries[0].record.request_id())));
    }

    #[test]
    fn token_bucket_returns_429_after_capacity_and_supports_dev_reload() {
        let limiter = RateLimitMiddleware::new(
            RateLimitAlgorithm::TokenBucket,
            2,
            1_000,
            RateLimitScope::Route,
            true,
        )
        .expect("valid limiter");
        let mut context = MiddlewareContext::default();
        assert!(matches!(
            limiter.on_request(request(), &mut context),
            Ok(MiddlewareDecision::Continue(_))
        ));
        assert!(matches!(
            limiter.on_request(request(), &mut context),
            Ok(MiddlewareDecision::Continue(_))
        ));
        let denied = limiter
            .on_request(request(), &mut context)
            .expect("rate limit decision");
        let MiddlewareDecision::ShortCircuit(response) = denied else {
            panic!("third request must be rejected");
        };
        assert_eq!(response.status.code(), 429);
        assert_eq!(response.header("retry-after"), Some("1"));
        assert!(limiter.update(3, 1_000));
        assert!(matches!(
            limiter.on_request(request(), &mut context),
            Ok(MiddlewareDecision::Continue(_))
        ));
    }

    #[test]
    fn sliding_window_isolates_tenants_and_production_cannot_reload() {
        let limiter = RateLimitMiddleware::new(
            RateLimitAlgorithm::SlidingWindow,
            1,
            1_000,
            RateLimitScope::Tenant,
            false,
        )
        .expect("valid limiter");
        let tenant_a = request()
            .with_header("x-spectra-tenant", "tenant-a")
            .expect("valid tenant header");
        let tenant_b = request()
            .with_header("x-spectra-tenant", "tenant-b")
            .expect("valid tenant header");
        let mut context = MiddlewareContext::default();
        assert!(matches!(
            limiter.on_request(tenant_a.clone(), &mut context),
            Ok(MiddlewareDecision::Continue(_))
        ));
        assert!(matches!(
            limiter.on_request(tenant_a, &mut context),
            Ok(MiddlewareDecision::ShortCircuit(_))
        ));
        assert!(matches!(
            limiter.on_request(tenant_b, &mut context),
            Ok(MiddlewareDecision::Continue(_))
        ));
        assert!(!limiter.update(2, 1_000));
    }

    #[test]
    fn security_headers_apply_defaults_and_longest_route_policy() {
        let security = SecurityHeadersMiddleware::new();
        let chain = MiddlewareChain::new().use_sync(security.clone());
        let (default_response, _) = chain
            .execute_sync(
                Request::new(Method::Get, "/public").expect("valid public request"),
                response(200),
            )
            .expect("default security response");
        assert_eq!(
            default_response.header("content-security-policy"),
            Some(DEFAULT_SECURITY_CSP)
        );
        assert_eq!(
            default_response.header("permissions-policy"),
            Some(DEFAULT_SECURITY_PERMISSIONS)
        );
        assert_eq!(default_response.header("x-frame-options"), Some("DENY"));
        assert_eq!(
            default_response.header("x-content-type-options"),
            Some("nosniff")
        );
        assert_eq!(
            default_response.header("referrer-policy"),
            Some("strict-origin-when-cross-origin")
        );
        assert_eq!(default_response.header("strict-transport-security"), None);

        assert!(security.configure_route(
            "/api",
            "default-src https://api.example",
            "geolocation=(self)"
        ));
        assert!(security.configure_route("/api/admin", "default-src 'none'", "camera=()"));

        let (admin_response, _) = chain
            .execute_sync(
                Request::new(Method::Get, "/api/admin/users?active=true")
                    .expect("valid admin request"),
                response(200),
            )
            .expect("admin security response");
        assert_eq!(
            admin_response.header("content-security-policy"),
            Some("default-src 'none'")
        );
        assert_eq!(
            admin_response.header("permissions-policy"),
            Some("camera=()")
        );

        let (api_response, _) = chain
            .execute_sync(
                Request::new(Method::Get, "/api/orders").expect("valid API request"),
                response(200),
            )
            .expect("API security response");
        assert_eq!(
            api_response.header("content-security-policy"),
            Some("default-src https://api.example")
        );
        assert_eq!(
            api_response.header("permissions-policy"),
            Some("geolocation=(self)")
        );
    }

    #[test]
    fn security_headers_validate_hsts_and_cover_short_circuit_responses() {
        let security = SecurityHeadersMiddleware::new();
        assert!(!security.configure(
            "default-src 'self'",
            DEFAULT_SECURITY_PERMISSIONS,
            false,
            true,
            false
        ));
        assert!(!security.configure(
            "default-src 'self'",
            DEFAULT_SECURITY_PERMISSIONS,
            false,
            false,
            true
        ));
        assert!(!security.configure("", DEFAULT_SECURITY_PERMISSIONS, true, true, true));
        assert!(security.configure("default-src 'none'", "camera=()", true, true, true));

        let short = http::store_response(response(401));
        let chain = MiddlewareChain::new()
            .use_sync(security)
            .use_sync(RecordedMiddleware {
                before_marker: "auth:request".to_string(),
                after_marker: "auth:response".to_string(),
                short_circuit_response: Some(short),
            });
        let (response, _) = chain
            .execute_sync(request(), response(200))
            .expect("short-circuit security response");
        assert_eq!(response.status.code(), 401);
        assert_eq!(
            response.header("content-security-policy"),
            Some("default-src 'none'")
        );
        assert_eq!(response.header("permissions-policy"), Some("camera=()"));
        assert_eq!(
            response.header("strict-transport-security"),
            Some("max-age=31536000; includeSubDomains; preload")
        );
    }

    #[test]
    fn compression_negotiates_each_encoding_and_round_trips_body() {
        let body = b"Spectra compression keeps response representations deterministic. ".repeat(32);
        for (accept_encoding, expected) in [
            ("br;q=1, gzip;q=0.5, deflate;q=0.1", "br"),
            ("gzip;q=1, br;q=0.5, deflate;q=0.1", "gzip"),
            ("deflate;q=1, gzip;q=0.5, br;q=0", "deflate"),
        ] {
            let middleware = CompressionMiddleware::new(1);
            let request = request()
                .with_header("Accept-Encoding", accept_encoding)
                .expect("valid accept-encoding header");
            let response = response(200)
                .with_header("Content-Length", body.len().to_string())
                .expect("valid content length")
                .with_body(body.clone());
            let (response, _) = MiddlewareChain::new()
                .use_sync(middleware)
                .execute_sync(request, response)
                .expect("compression response");

            assert_eq!(response.header("content-encoding"), Some(expected));
            assert_eq!(response.header("vary"), Some("Accept-Encoding"));
            assert_eq!(
                response.header("content-length"),
                Some(response.body.len().to_string().as_str())
            );
            assert_ne!(response.body, body);

            let mut decoded = Vec::new();
            match expected {
                "br" => {
                    brotli::Decompressor::new(&response.body[..], 4096)
                        .read_to_end(&mut decoded)
                        .expect("valid brotli payload");
                }
                "gzip" => {
                    flate2::read::GzDecoder::new(&response.body[..])
                        .read_to_end(&mut decoded)
                        .expect("valid gzip payload");
                }
                "deflate" => {
                    flate2::read::ZlibDecoder::new(&response.body[..])
                        .read_to_end(&mut decoded)
                        .expect("valid deflate payload");
                }
                _ => unreachable!("test only uses supported encodings"),
            }
            assert_eq!(decoded, body);
        }
    }

    #[test]
    fn compression_honors_q_values_threshold_and_http_exclusions() {
        let body = b"compressible response body ".repeat(16);
        let middleware = CompressionMiddleware::new(body.len());
        let wildcard_request = request()
            .with_header("Accept-Encoding", "gzip;q=0, *;q=0.8")
            .expect("valid wildcard accept-encoding header");
        let (small, _) = MiddlewareChain::new()
            .use_sync(middleware.clone())
            .execute_sync(
                wildcard_request,
                response(200).with_body(body[..8].to_vec()),
            )
            .expect("small response");
        assert_eq!(small.header("content-encoding"), None);
        assert_eq!(small.header("vary"), Some("Accept-Encoding"));

        let tie_request = request()
            .with_header("Accept-Encoding", "br;q=0, gzip;q=0.8, deflate;q=0.8")
            .expect("valid tie accept-encoding header");
        let (large, _) = MiddlewareChain::new()
            .use_sync(middleware.clone())
            .execute_sync(tie_request, response(200).with_body(body.clone()))
            .expect("large response");
        assert_eq!(large.header("content-encoding"), Some("gzip"));

        let head_request = Request::new(Method::Head, "/middleware")
            .expect("valid HEAD request")
            .with_header("Accept-Encoding", "gzip")
            .expect("valid HEAD accept-encoding header");
        let (head, _) = MiddlewareChain::new()
            .use_sync(middleware.clone())
            .execute_sync(head_request, response(200).with_body(body.clone()))
            .expect("HEAD response");
        assert_eq!(head.header("content-encoding"), None);
        assert_eq!(head.header("vary"), Some("Accept-Encoding"));

        let (no_content, _) = MiddlewareChain::new()
            .use_sync(middleware.clone())
            .execute_sync(
                request()
                    .with_header("Accept-Encoding", "gzip")
                    .expect("valid no-content accept-encoding header"),
                response(204).with_body(body.clone()),
            )
            .expect("204 response");
        assert_eq!(no_content.header("content-encoding"), None);

        let preencoded = response(200)
            .with_header("Content-Encoding", "identity")
            .expect("valid content encoding")
            .with_body(body);
        let (preencoded, _) = MiddlewareChain::new()
            .use_sync(middleware)
            .execute_sync(
                request()
                    .with_header("Accept-Encoding", "gzip")
                    .expect("valid preencoded accept-encoding header"),
                preencoded,
            )
            .expect("preencoded response");
        assert_eq!(preencoded.header("content-encoding"), Some("identity"));
        assert_eq!(preencoded.header("vary"), Some("Accept-Encoding"));
    }

    #[test]
    fn api_key_auth_handles_valid_missing_expired_revoked_and_query_keys() {
        let auth = ApiKeyMiddleware::new(ApiKeySource::Header("X-API-Key".to_string()));
        assert!(auth.add("valid-key".to_string(), 0));
        assert!(auth.add("expired-key".to_string(), 1));
        assert!(auth.add("revoked-key".to_string(), 0));
        assert!(auth.revoke("revoked-key"));

        let mut context = MiddlewareContext::default();
        let valid = request()
            .with_header("X-API-Key", "valid-key")
            .expect("valid API key header");
        let MiddlewareDecision::Continue(valid_request) = auth
            .on_request(valid, &mut context)
            .expect("valid API key decision")
        else {
            panic!("valid API key must continue");
        };
        assert_eq!(valid_request.header("x-spectra-api-key"), Some("valid-key"));

        let missing = auth
            .on_request(request(), &mut context)
            .expect("missing API key decision");
        let MiddlewareDecision::ShortCircuit(missing_response) = missing else {
            panic!("missing API key must be rejected");
        };
        assert_eq!(missing_response.status.code(), 401);
        assert_eq!(
            missing_response.header("content-type"),
            Some("application/problem+json")
        );
        assert!(String::from_utf8_lossy(&missing_response.body).contains("missing"));

        for (key, reason) in [("expired-key", "expired"), ("revoked-key", "revoked")] {
            let request = request()
                .with_header("X-API-Key", key)
                .expect("valid API key header");
            let MiddlewareDecision::ShortCircuit(response) = auth
                .on_request(request, &mut context)
                .expect("invalid API key decision")
            else {
                panic!("{key} must be rejected");
            };
            assert_eq!(response.status.code(), 401);
            assert!(String::from_utf8_lossy(&response.body).contains(reason));
        }

        let query_auth = ApiKeyMiddleware::new(ApiKeySource::Query("api_key".to_string()));
        assert!(query_auth.add("query-key".to_string(), 0));
        let query_request =
            Request::new(Method::Get, "/secure?api_key=query-key").expect("valid query request");
        assert!(matches!(
            query_auth.on_request(query_request, &mut context),
            Ok(MiddlewareDecision::Continue(_))
        ));
    }

    #[test]
    fn api_key_identity_can_feed_api_key_rate_limit_scope() {
        let auth = ApiKeyMiddleware::new(ApiKeySource::Header("X-API-Key".to_string()));
        assert!(auth.add("shared-key".to_string(), 0));
        let limiter = RateLimitMiddleware::new(
            RateLimitAlgorithm::SlidingWindow,
            1,
            1_000,
            RateLimitScope::ApiKey,
            false,
        )
        .expect("valid API key limiter");
        let chain = MiddlewareChain::new().use_sync(auth).use_sync(limiter);
        let request = request()
            .with_header("X-API-Key", "shared-key")
            .expect("valid API key header");
        let (_, _) = chain
            .execute_sync(request.clone(), response(200))
            .expect("first authenticated request");
        let (second, _) = chain
            .execute_sync(request, response(200))
            .expect("second authenticated request");
        assert_eq!(second.status.code(), 429);
    }

    #[test]
    fn request_ids_are_random_unique_and_formatted() {
        let pattern = Regex::new(r"^req-[0-9a-f]{32}$").expect("valid id regex");
        let mut ids = HashSet::new();
        for _ in 0..10_000 {
            let id = next_request_id();
            assert!(pattern.is_match(&id), "unexpected id format: {id}");
            ids.insert(id);
        }
        assert_eq!(ids.len(), 10_000, "request ids must be unique");
    }
}
