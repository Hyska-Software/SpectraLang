//! Opt-in W3C tracing with a bounded OTLP/HTTP exporter.

use crate::handles::{HandleId, HandleKind, HandleTable};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_NAME: usize = 256;
const MAX_ATTRIBUTES: usize = 64;
const MAX_VALUE: usize = 4096;
const MAX_RETRIES: usize = 3;
const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_millis(1000);
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanKind {
    Internal = 1,
    Server = 2,
    Client = 3,
    Producer = 4,
    Consumer = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanStatus {
    Unset,
    Ok,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceContext {
    pub trace_id: [u8; 16],
    pub span_id: [u8; 8],
    pub flags: u8,
}

impl TraceContext {
    pub fn traceparent(&self) -> String {
        format!(
            "00-{}-{}-{:02x}",
            hex(&self.trace_id),
            hex(&self.span_id),
            self.flags
        )
    }
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        let parts: Vec<_> = value.split('-').collect();
        if parts.len() != 4
            || parts[0] != "00"
            || parts[1].len() != 32
            || parts[2].len() != 16
            || parts[3].len() != 2
        {
            return Err("E2702");
        }
        let trace_id = decode_hex::<16>(parts[1]).ok_or("E2702")?;
        let span_id = decode_hex::<8>(parts[2]).ok_or("E2702")?;
        let flags = u8::from_str_radix(parts[3], 16).map_err(|_| "E2702")?;
        if trace_id.iter().all(|v| *v == 0) || span_id.iter().all(|v| *v == 0) {
            return Err("E2702");
        }
        Ok(Self {
            trace_id,
            span_id,
            flags,
        })
    }
}

#[derive(Clone, Debug)]
pub struct TraceConfig {
    pub endpoint: String,
    pub service_name: String,
    pub sample_rate: f64,
    pub batch_size: usize,
    pub queue_capacity: usize,
    pub flush_interval: Duration,
    pub shutdown_timeout: Duration,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AttributeValue {
    String(String),
    Int(i64),
    Bool(bool),
}

#[derive(Clone, Debug)]
struct Span {
    context: TraceContext,
    parent: Option<TraceContext>,
    name: String,
    kind: SpanKind,
    attributes: Vec<(String, AttributeValue)>,
    status: SpanStatus,
    started: Instant,
    start_unix_nanos: u64,
}

#[derive(Default)]
struct ExportStats {
    created: AtomicU64,
    enqueued: AtomicU64,
    exported: AtomicU64,
    dropped: AtomicU64,
    failed: AtomicU64,
    retries: AtomicU64,
    worker_alive: AtomicBool,
}

enum WorkerCommand {
    Span(Span),
    Flush(mpsc::SyncSender<Result<usize, &'static str>>),
    Shutdown(mpsc::SyncSender<Result<(), &'static str>>),
}

struct ExporterHandle {
    sender: SyncSender<WorkerCommand>,
    join: Option<JoinHandle<()>>,
    stats: Arc<ExportStats>,
    pending: Arc<AtomicU64>,
    queue_capacity: u64,
    timeout: Duration,
}

struct State {
    configs: HandleTable<TraceConfig>,
    spans: HandleTable<Span>,
    last_error: Option<String>,
    active_config: Option<u64>,
    exporter: Option<ExporterHandle>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            configs: HandleTable::new(HandleKind::TracingConfig),
            spans: HandleTable::new(HandleKind::TracingSpan),
            last_error: None,
            active_config: None,
            exporter: None,
        }
    }
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::default()))
}
fn fallback_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}
fn new_context(parent: Option<&TraceContext>) -> TraceContext {
    let n = fallback_id();
    let mut trace_id = [0u8; 16];
    let mut span_id = [0u8; 8];
    if let Some(parent) = parent {
        trace_id = parent.trace_id;
    } else if getrandom::fill(&mut trace_id).is_err() {
        trace_id[..8].copy_from_slice(&n.to_le_bytes());
        trace_id[8..].copy_from_slice(&unix_nanos().to_le_bytes());
    }
    if getrandom::fill(&mut span_id).is_err() {
        span_id.copy_from_slice(&n.to_le_bytes());
    }
    TraceContext {
        trace_id,
        span_id,
        flags: 1,
    }
}

thread_local! {
    static CONTEXT_STACK: RefCell<Vec<TraceContext>> = const { RefCell::new(Vec::new()) };
    static SPAN_STACK: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

pub fn config_new(endpoint: &str, service_name: &str) -> Result<u64, &'static str> {
    if endpoint.is_empty()
        || service_name.is_empty()
        || service_name.len() > MAX_NAME
        || !endpoint.starts_with("http://")
    {
        return Err("E2701");
    }
    let id = state()
        .lock()
        .unwrap()
        .configs
        .insert(TraceConfig {
            endpoint: endpoint.to_string(),
            service_name: service_name.to_string(),
            sample_rate: 1.0,
            batch_size: 256,
            queue_capacity: 4096,
            flush_interval: DEFAULT_FLUSH_INTERVAL,
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
        })
        .raw() as u64;
    Ok(id)
}
pub fn config_set_sample_rate(id: u64, rate: f64) -> Result<(), &'static str> {
    if !(0.0..=1.0).contains(&rate) {
        return Err("E2701");
    }
    let handle = HandleId::from_raw(id as i64).map_err(|_| "E2701")?;
    state()
        .lock()
        .unwrap()
        .configs
        .get_mut(handle)
        .map(|c| c.sample_rate = rate)
        .map_err(|_| "E2701")
}
pub fn config_set_batch_size(id: u64, size: usize) -> Result<(), &'static str> {
    if size == 0 || size > 65536 {
        return Err("E2701");
    }
    let handle = HandleId::from_raw(id as i64).map_err(|_| "E2701")?;
    state()
        .lock()
        .unwrap()
        .configs
        .get_mut(handle)
        .map(|c| c.batch_size = size)
        .map_err(|_| "E2701")
}
pub fn config_start(id: u64) -> Result<(), &'static str> {
    let config = {
        let mut s = state().lock().unwrap();
        if s.active_config.is_some() || s.exporter.is_some() {
            return Err("E2701");
        }
        let handle = HandleId::from_raw(id as i64).map_err(|_| "E2701")?;
        let config = s.configs.get(handle).map_err(|_| "E2701")?.clone();
        s.active_config = Some(id);
        config
    };
    let timeout = config.shutdown_timeout;
    let (sender, receiver) = mpsc::sync_channel(config.queue_capacity);
    let stats = Arc::new(ExportStats::default());
    let worker_stats = Arc::clone(&stats);
    let pending = Arc::new(AtomicU64::new(0));
    let worker_pending = Arc::clone(&pending);
    let queue_capacity = config.queue_capacity as u64;
    let join = match thread::Builder::new()
        .name("spectra-otlp-exporter".into())
        .spawn(move || worker_loop(config, receiver, worker_stats, worker_pending))
    {
        Ok(join) => join,
        Err(_) => {
            let mut s = state().lock().unwrap();
            s.active_config = None;
            return Err("E2701");
        }
    };
    stats.worker_alive.store(true, Ordering::Release);
    state().lock().unwrap().exporter = Some(ExporterHandle {
        sender,
        join: Some(join),
        stats,
        pending,
        queue_capacity,
        timeout,
    });
    Ok(())
}
pub fn config_shutdown(id: u64) -> Result<(), &'static str> {
    if state().lock().unwrap().active_config != Some(id) {
        return Err("E2701");
    }
    let mut exporter = state().lock().unwrap().exporter.take().ok_or("E2707")?;
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    let send_result = exporter
        .sender
        .send(WorkerCommand::Shutdown(ack_tx))
        .map_err(|_| "E2707");
    let shutdown_result: Result<(), &'static str> =
        send_result.and_then(|_| ack_rx.recv_timeout(exporter.timeout).map_err(|_| "E2707")?);
    let join_result = exporter
        .join
        .take()
        .map(|join| join.join().map_err(|_| "E2707"))
        .unwrap_or(Ok(()));
    let mut s = state().lock().unwrap();
    s.active_config = None;
    if shutdown_result.is_err() || join_result.is_err() {
        s.last_error = Some(
            if join_result.is_err() || shutdown_result == Err("E2707") {
                "E2707"
            } else {
                "E2706"
            }
            .into(),
        );
    }
    shutdown_result.and(join_result)
}

pub fn span_start(name: &str, kind: SpanKind) -> Result<u64, &'static str> {
    let parent = CONTEXT_STACK.with(|stack| stack.borrow().last().cloned());
    span_start_with_parent(name, kind, parent)
}
pub fn span_start_with_parent(
    name: &str,
    kind: SpanKind,
    parent: Option<TraceContext>,
) -> Result<u64, &'static str> {
    if name.is_empty() || name.len() > MAX_NAME {
        return Err("E2701");
    }
    let mut s = state().lock().unwrap();
    if s.active_config.is_none() || s.exporter.is_none() {
        return Err("E2704");
    }
    let context = new_context(parent.as_ref());
    let id = s
        .spans
        .insert(Span {
            context: context.clone(),
            parent,
            name: name.to_string(),
            kind,
            attributes: Vec::new(),
            status: SpanStatus::Unset,
            started: Instant::now(),
            start_unix_nanos: unix_nanos(),
        })
        .raw() as u64;
    if let Some(exporter) = &s.exporter {
        exporter.stats.created.fetch_add(1, Ordering::Relaxed);
    }
    SPAN_STACK.with(|stack| stack.borrow_mut().push(id));
    CONTEXT_STACK.with(|stack| stack.borrow_mut().push(context));
    Ok(id)
}

fn set_attribute(id: u64, key: &str, value: AttributeValue) -> Result<(), &'static str> {
    if key.is_empty() || key.len() > MAX_NAME {
        return Err("E2701");
    }
    let size_ok = match &value {
        AttributeValue::String(v) => v.len() <= MAX_VALUE,
        _ => true,
    };
    if !size_ok {
        return Err("E2701");
    }
    let mut s = state().lock().unwrap();
    let handle = HandleId::from_raw(id as i64).map_err(|_| "E2703")?;
    let span = s.spans.get_mut(handle).map_err(|_| "E2703")?;
    if let Some(existing) = span.attributes.iter_mut().find(|(name, _)| name == key) {
        existing.1 = value;
        return Ok(());
    }
    if span.attributes.len() >= MAX_ATTRIBUTES {
        return Err("E2701");
    }
    span.attributes.push((key.to_string(), value));
    Ok(())
}
pub fn span_set_attribute(id: u64, key: &str, value: &str) -> Result<(), &'static str> {
    set_attribute(id, key, AttributeValue::String(value.to_string()))
}
pub fn span_set_attribute_int(id: u64, key: &str, value: i64) -> Result<(), &'static str> {
    set_attribute(id, key, AttributeValue::Int(value))
}
pub fn span_set_attribute_bool(id: u64, key: &str, value: bool) -> Result<(), &'static str> {
    set_attribute(id, key, AttributeValue::Bool(value))
}
pub fn span_set_status(id: u64, status: SpanStatus) -> Result<(), &'static str> {
    let mut s = state().lock().unwrap();
    let handle = HandleId::from_raw(id as i64).map_err(|_| "E2703")?;
    s.spans
        .get_mut(handle)
        .map_err(|_| "E2703")
        .map(|span| span.status = status)
}
pub fn span_end(id: u64) -> Result<(), &'static str> {
    if !SPAN_STACK.with(|stack| stack.borrow().last().copied() == Some(id)) {
        return Err("E2703");
    }
    let span = {
        let mut s = state().lock().unwrap();
        let handle = HandleId::from_raw(id as i64).map_err(|_| "E2703")?;
        s.spans.remove(handle).map_err(|_| "E2703")?
    };
    let result = {
        let s = state().lock().unwrap();
        let exporter = s.exporter.as_ref().ok_or("E2704")?;
        let mut reserved = false;
        loop {
            let current = exporter.pending.load(Ordering::Acquire);
            if current >= exporter.queue_capacity {
                break;
            }
            if exporter
                .pending
                .compare_exchange(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                reserved = true;
                break;
            }
        }
        if !reserved {
            exporter.stats.dropped.fetch_add(1, Ordering::Relaxed);
            Err("E2705")
        } else {
            match exporter.sender.try_send(WorkerCommand::Span(span)) {
                Ok(()) => {
                    exporter.stats.enqueued.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }
                Err(TrySendError::Full(_)) => {
                    exporter.pending.fetch_sub(1, Ordering::AcqRel);
                    exporter.stats.dropped.fetch_add(1, Ordering::Relaxed);
                    Err("E2705")
                }
                Err(TrySendError::Disconnected(_)) => {
                    exporter.pending.fetch_sub(1, Ordering::AcqRel);
                    Err("E2707")
                }
            }
        }
    };
    if let Err(error) = result {
        state().lock().unwrap().last_error = Some(error.into());
    }
    SPAN_STACK.with(|stack| {
        stack.borrow_mut().pop();
    });
    CONTEXT_STACK.with(|stack| {
        stack.borrow_mut().pop();
    });
    result
}
pub fn current() -> Option<u64> {
    SPAN_STACK.with(|stack| stack.borrow().last().copied())
}
pub fn context(id: u64) -> Result<TraceContext, &'static str> {
    let handle = HandleId::from_raw(id as i64).map_err(|_| "E2703")?;
    state()
        .lock()
        .unwrap()
        .spans
        .get(handle)
        .map_err(|_| "E2703")
        .map(|s| s.context.clone())
}
pub fn inject(id: u64) -> Result<String, &'static str> {
    Ok(context(id)?.traceparent())
}
pub fn extract(value: &str) -> Result<TraceContext, &'static str> {
    TraceContext::parse(value)
}
pub fn current_traceparent() -> Option<String> {
    current()
        .and_then(|id| context(id).ok())
        .map(|ctx| ctx.traceparent())
}

/// Runs `work` with an explicitly propagated parent context.
///
/// Background executors use this helper to preserve request parentage without
/// manufacturing a placeholder span on the worker thread.
pub fn with_context<T>(parent: Option<TraceContext>, work: impl FnOnce() -> T) -> T {
    if let Some(context) = parent {
        CONTEXT_STACK.with(|stack| stack.borrow_mut().push(context));
        struct ContextGuard;
        impl Drop for ContextGuard {
            fn drop(&mut self) {
                CONTEXT_STACK.with(|stack| {
                    stack.borrow_mut().pop();
                });
            }
        }
        let _guard = ContextGuard;
        work()
    } else {
        work()
    }
}
pub fn last_error() -> Option<String> {
    state().lock().unwrap().last_error.clone()
}
pub fn stats() -> Option<(u64, u64, u64, u64, u64, bool)> {
    state().lock().unwrap().exporter.as_ref().map(|e| {
        (
            e.stats.created.load(Ordering::Relaxed),
            e.stats.enqueued.load(Ordering::Relaxed),
            e.stats.exported.load(Ordering::Relaxed),
            e.stats.dropped.load(Ordering::Relaxed),
            e.stats.retries.load(Ordering::Relaxed),
            e.stats.worker_alive.load(Ordering::Acquire),
        )
    })
}
/// # Safety
///
/// The returned pointer is owned by the runtime allocation table and must be
/// treated as an opaque Spectra string handle. Callers must not dereference,
/// resize, or free it except through the runtime allocation APIs.
pub unsafe fn alloc_string(value: &str) -> i64 {
    use crate::ffi::spectra_rt_manual_alloc;
    let raw = spectra_rt_manual_alloc((value.len() + 1) * std::mem::size_of::<i64>()) as *mut i64;
    if raw.is_null() {
        return 0;
    }
    for (i, byte) in value.bytes().enumerate() {
        *raw.add(i) = i64::from(byte);
    }
    *raw.add(value.len()) = 0;
    raw as i64
}

pub fn flush() -> Result<usize, &'static str> {
    let (sender, timeout) = {
        let s = state().lock().unwrap();
        let e = s.exporter.as_ref().ok_or("E2704")?;
        (e.sender.clone(), e.timeout)
    };
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    sender
        .send(WorkerCommand::Flush(ack_tx))
        .map_err(|_| "E2707")?;
    let result = ack_rx.recv_timeout(timeout).map_err(|_| "E2707")?;
    if result.is_err() {
        state().lock().unwrap().last_error = Some("E2706".into());
    }
    result
}

fn worker_loop(
    config: TraceConfig,
    receiver: Receiver<WorkerCommand>,
    stats: Arc<ExportStats>,
    pending: Arc<AtomicU64>,
) {
    let mut batch = Vec::with_capacity(config.batch_size);
    let mut deadline = Instant::now() + config.flush_interval;
    loop {
        let wait = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(wait) {
            Ok(WorkerCommand::Span(span)) => {
                batch.push(span);
                if batch.len() >= config.batch_size {
                    let _ = export_batch(&config, &mut batch, &stats, &pending);
                    deadline = Instant::now() + config.flush_interval;
                }
            }
            Ok(WorkerCommand::Flush(ack)) => {
                let result = export_batch(&config, &mut batch, &stats, &pending);
                let _ = ack.send(result);
                deadline = Instant::now() + config.flush_interval;
            }
            Ok(WorkerCommand::Shutdown(ack)) => {
                let result = export_batch(&config, &mut batch, &stats, &pending).map(|_| ());
                let _ = ack.send(result);
                stats.worker_alive.store(false, Ordering::Release);
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = export_batch(&config, &mut batch, &stats, &pending);
                deadline = Instant::now() + config.flush_interval;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                stats.worker_alive.store(false, Ordering::Release);
                break;
            }
        }
    }
}

fn export_batch(
    config: &TraceConfig,
    batch: &mut Vec<Span>,
    stats: &ExportStats,
    pending: &AtomicU64,
) -> Result<usize, &'static str> {
    if batch.is_empty() {
        return Ok(0);
    }
    let count = batch.len();
    let payload = encode_otlp(&config.service_name, batch);
    let mut sent = false;
    for attempt in 0..MAX_RETRIES {
        match send_otlp(&config.endpoint, &payload) {
            Ok(()) => {
                sent = true;
                break;
            }
            Err(error) if error.transient && attempt + 1 < MAX_RETRIES => {
                stats.retries.fetch_add(1, Ordering::Relaxed);
                thread::sleep(Duration::from_millis(25 * (1 << attempt)));
            }
            Err(_) => break,
        }
    }
    batch.clear();
    pending.fetch_sub(count as u64, Ordering::AcqRel);
    if sent {
        stats.exported.fetch_add(count as u64, Ordering::Relaxed);
        Ok(count)
    } else {
        stats.failed.fetch_add(count as u64, Ordering::Relaxed);
        stats.dropped.fetch_add(count as u64, Ordering::Relaxed);
        Err("E2706")
    }
}

struct SendError {
    transient: bool,
}
fn send_otlp(endpoint: &str, body: &[u8]) -> Result<(), SendError> {
    let url = endpoint
        .strip_prefix("http://")
        .ok_or(SendError { transient: false })?;
    let (authority, path) = url.split_once('/').unwrap_or((url, "v1/traces"));
    let mut addrs = authority
        .to_socket_addrs()
        .map_err(|_| SendError { transient: false })?;
    let addr = addrs.next().ok_or(SendError { transient: false })?;
    let timeout = Duration::from_secs(5);
    let mut stream =
        TcpStream::connect_timeout(&addr, timeout).map_err(|_| SendError { transient: true })?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let request = format!("POST /{} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/x-protobuf\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", path, authority, body.len());
    stream
        .write_all(request.as_bytes())
        .map_err(|_| SendError { transient: true })?;
    stream
        .write_all(body)
        .map_err(|_| SendError { transient: true })?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|_| SendError { transient: true })?;
    let status = response
        .split(|byte| *byte == b' ')
        .nth(1)
        .and_then(|part| std::str::from_utf8(part).ok())
        .and_then(|part| part.split_whitespace().next())
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or(SendError { transient: true })?;
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(SendError {
            transient: status >= 500,
        })
    }
}

fn encode_otlp(service: &str, spans: &[Span]) -> Vec<u8> {
    let mut scope_spans = Vec::new();
    for span in spans {
        let mut out = Vec::new();
        field_bytes(&mut out, 1, &span.context.trace_id);
        field_bytes(&mut out, 2, &span.context.span_id);
        if let Some(parent) = &span.parent {
            field_bytes(&mut out, 4, &parent.span_id);
        }
        field_string(&mut out, 5, &span.name);
        field_varint(&mut out, 6, span.kind as u64);
        field_fixed64(&mut out, 7, span.start_unix_nanos);
        field_fixed64(
            &mut out,
            8,
            span.start_unix_nanos + span.started.elapsed().as_nanos() as u64,
        );
        for (key, value) in &span.attributes {
            let mut kv = Vec::new();
            field_string(&mut kv, 1, key);
            let mut any = Vec::new();
            match value {
                AttributeValue::String(v) => field_string(&mut any, 1, v),
                AttributeValue::Bool(v) => field_varint(&mut any, 2, u64::from(*v)),
                AttributeValue::Int(v) => field_varint(&mut any, 3, *v as u64),
            };
            field_message(&mut kv, 2, &any);
            field_message(&mut out, 9, &kv);
        }
        let mut status = Vec::new();
        field_varint(
            &mut status,
            2,
            match span.status {
                SpanStatus::Unset => 0,
                SpanStatus::Ok => 1,
                SpanStatus::Error => 2,
            },
        );
        field_message(&mut out, 15, &status);
        field_message(&mut scope_spans, 2, &out);
    }
    let mut scope = Vec::new();
    field_string(&mut scope, 1, "spectralang.runtime");
    field_message(&mut scope_spans, 1, &scope);
    let mut resource = Vec::new();
    resource_attribute(&mut resource, "service.name", service);
    resource_attribute(&mut resource, "telemetry.sdk.name", "spectralang-runtime");
    resource_attribute(
        &mut resource,
        "telemetry.sdk.version",
        env!("CARGO_PKG_VERSION"),
    );
    let mut resource_spans = Vec::new();
    field_message(&mut resource_spans, 1, &resource);
    field_message(&mut resource_spans, 2, &scope_spans);
    let mut request = Vec::new();
    field_message(&mut request, 1, &resource_spans);
    request
}
fn resource_attribute(out: &mut Vec<u8>, key: &str, value: &str) {
    let mut kv = Vec::new();
    field_string(&mut kv, 1, key);
    let mut any = Vec::new();
    field_string(&mut any, 1, value);
    field_message(&mut kv, 2, &any);
    field_message(out, 1, &kv);
}
fn field_varint(out: &mut Vec<u8>, field: u32, value: u64) {
    put_varint(out, (field as u64) << 3);
    put_varint(out, value);
}
fn field_fixed64(out: &mut Vec<u8>, field: u32, value: u64) {
    put_varint(out, ((field as u64) << 3) | 1);
    out.extend_from_slice(&value.to_le_bytes());
}
fn field_string(out: &mut Vec<u8>, field: u32, value: &str) {
    field_bytes(out, field, value.as_bytes());
}
fn field_bytes(out: &mut Vec<u8>, field: u32, value: &[u8]) {
    put_varint(out, ((field as u64) << 3) | 2);
    put_varint(out, value.len() as u64);
    out.extend_from_slice(value);
}
fn field_message(out: &mut Vec<u8>, field: u32, value: &[u8]) {
    field_bytes(out, field, value);
}
fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}
fn unix_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
fn hex<const N: usize>(bytes: &[u8; N]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn decode_hex<const N: usize>(value: &str) -> Option<[u8; N]> {
    if value.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for i in 0..N {
        out[i] = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

pub fn begin_external_span(kind: SpanKind, name: &str) -> Result<u64, &'static str> {
    span_start(name, kind)
}
pub fn end_external_span(id: u64, success: bool) -> Result<(), &'static str> {
    span_set_status(
        id,
        if success {
            SpanStatus::Ok
        } else {
            SpanStatus::Error
        },
    )?;
    span_end(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    #[test]
    fn traceparent_round_trip() {
        let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let ctx = TraceContext {
            trace_id: [1; 16],
            span_id: [2; 8],
            flags: 1,
        };
        assert_eq!(TraceContext::parse(&ctx.traceparent()).unwrap(), ctx);
    }
    #[test]
    fn rejects_zero_ids() {
        let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        assert!(
            TraceContext::parse("00-00000000000000000000000000000000-0101010101010101-01").is_err()
        );
    }
    #[test]
    fn typed_attributes_replace_by_key() {
        let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let id = config_new("http://127.0.0.1:1/v1/traces", "test").unwrap();
        config_start(id).unwrap();
        let span = span_start("typed", SpanKind::Internal).unwrap();
        span_set_attribute_int(span, "answer", 42).unwrap();
        span_set_attribute_bool(span, "ready", true).unwrap();
        span_set_attribute_int(span, "answer", 43).unwrap();
        span_end(span).unwrap();
        let _ = config_shutdown(id);
    }
    #[test]
    fn current_context_isolated_between_threads() {
        let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let id = config_new("http://127.0.0.1:1/v1/traces", "isolation").unwrap();
        config_start(id).unwrap();
        let handles = (0..8)
            .map(|_| {
                thread::spawn(|| {
                    let span = span_start("thread", SpanKind::Internal).unwrap();
                    let current_id = current();
                    span_end(span).unwrap();
                    (span, current_id, current())
                })
            })
            .collect::<Vec<_>>();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert!(results
            .iter()
            .all(|(span, current_id, after)| Some(*span) == *current_id && after.is_none()));
        assert_eq!(current(), None);
        let _ = config_shutdown(id);
    }

    #[test]
    fn bounded_queue_reports_overflow_without_false_export() {
        let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let id = config_new("http://127.0.0.1:1/v1/traces", "queue-test").unwrap();
        config_set_batch_size(id, 65536).unwrap();
        config_start(id).unwrap();
        let mut overflow = false;
        for _ in 0..5000 {
            let span = span_start("queue.span", SpanKind::Internal).unwrap();
            if span_end(span) == Err("E2705") {
                overflow = true;
                break;
            }
        }
        assert!(overflow, "bounded exporter queue must reject excess spans");
        let _ = config_shutdown(id);
    }

    #[test]
    fn shutdown_clears_active_exporter_after_export_failure() {
        let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let failed = config_new("http://127.0.0.1:1/v1/traces", "shutdown-failure").unwrap();
        config_start(failed).unwrap();
        let span = span_start("failed.export", SpanKind::Internal).unwrap();
        assert_eq!(span_end(span), Ok(()));
        let failed_result = config_shutdown(failed);
        assert!(
            failed_result == Err("E2706") || failed_result == Err("E2707"),
            "unexpected shutdown result: {failed_result:?}"
        );
        assert_eq!(config_shutdown(failed), Err("E2701"));

        let recovered = config_new("http://127.0.0.1:1/v1/traces", "shutdown-recovery").unwrap();
        config_start(recovered).unwrap();
        let recovery_result = config_shutdown(recovered);
        assert!(recovery_result.is_ok() || recovery_result == Err("E2706"));
    }
}
