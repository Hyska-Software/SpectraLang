//! Server-Sent Events transport for `std.api.sse`.
//!
//! SSE is deliberately kept as a long-lived HTTP/1.1 transport here instead
//! of being folded into the finite `ServerResponse` serializer.  The server
//! writes a response header once, then serializes events and heartbeats as
//! independent flushed records.  A bounded replay log makes
//! `Last-Event-ID` useful without allowing an unbounded disconnected-client
//! queue to grow.

use crate::handles::ApiHandleTable;
use crate::http::{self, ParsedRequest};
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INTERNAL_ERROR,
    HOST_STATUS_INVALID_ARGUMENT,
};
use spectra_runtime::handles::HandleKind;
use spectra_runtime::stdlib::CancellationToken;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::LazyLock;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const DEFAULT_MAX_EVENT_BYTES: usize = 1024 * 1024;
const DEFAULT_REPLAY_CAPACITY: usize = 1024;
const MAX_REPLAY_BYTES: usize = 64 * 1024 * 1024;
const MAX_ROUTE_PENDING_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_HANDSHAKE_BYTES: usize = 64 * 1024;
const MAX_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60 * 60);
const MAX_REPLAY_CAPACITY: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SseErrorKind {
    Io,
    Handshake,
    InvalidArgument,
    PayloadTooLarge,
    Closed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseError {
    pub kind: SseErrorKind,
    pub message: String,
}

impl SseError {
    fn new(kind: SseErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for SseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SSE {:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for SseError {}

impl From<io::Error> for SseError {
    fn from(error: io::Error) -> Self {
        Self::new(SseErrorKind::Io, error.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseEvent {
    id: Option<String>,
    event_type: Option<String>,
    data: String,
    retry_ms: Option<u64>,
}

impl SseEvent {
    pub fn new(
        id: Option<String>,
        event_type: Option<String>,
        data: String,
        retry_ms: Option<u64>,
    ) -> Result<Self, SseError> {
        for (label, value) in [
            ("event id", id.as_deref()),
            ("event type", event_type.as_deref()),
        ] {
            if value
                .is_some_and(|value| value.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0)))
            {
                return Err(SseError::new(
                    SseErrorKind::InvalidArgument,
                    format!("SSE {label} must not contain line breaks or NUL"),
                ));
            }
        }
        if retry_ms.is_some_and(|value| value > 86_400_000) {
            return Err(SseError::new(
                SseErrorKind::InvalidArgument,
                "SSE retry hint must not exceed 24 hours",
            ));
        }
        let event = Self {
            id,
            event_type,
            data,
            retry_ms,
        };
        if event.wire_format().len() > DEFAULT_MAX_EVENT_BYTES {
            return Err(SseError::new(
                SseErrorKind::PayloadTooLarge,
                "SSE event exceeds the default event limit",
            ));
        }
        Ok(event)
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn event_type(&self) -> Option<&str> {
        self.event_type.as_deref()
    }

    pub fn data(&self) -> &str {
        &self.data
    }

    pub fn retry_ms(&self) -> Option<u64> {
        self.retry_ms
    }

    pub fn wire_format(&self) -> Vec<u8> {
        let mut output = String::new();
        if let Some(id) = &self.id {
            output.push_str("id: ");
            output.push_str(id);
            output.push('\n');
        }
        if let Some(event_type) = &self.event_type {
            output.push_str("event: ");
            output.push_str(event_type);
            output.push('\n');
        }
        let normalized = self.data.replace("\r\n", "\n").replace('\r', "\n");
        for line in normalized.split('\n') {
            output.push_str("data: ");
            output.push_str(line);
            output.push('\n');
        }
        if let Some(retry_ms) = self.retry_ms {
            output.push_str("retry: ");
            output.push_str(&retry_ms.to_string());
            output.push('\n');
        }
        output.push('\n');
        output.into_bytes()
    }
}

#[derive(Clone, Debug)]
pub struct SseConfig {
    pub heartbeat_interval: Duration,
    pub max_event_bytes: usize,
    pub replay_capacity: usize,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
}

impl Default for SseConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
            max_event_bytes: DEFAULT_MAX_EVENT_BYTES,
            replay_capacity: DEFAULT_REPLAY_CAPACITY,
            read_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
        }
    }
}

impl SseConfig {
    fn validate_event(&self, event: &SseEvent) -> Result<Vec<u8>, SseError> {
        let encoded = event.wire_format();
        if encoded.len() > self.max_event_bytes {
            return Err(SseError::new(
                SseErrorKind::PayloadTooLarge,
                "SSE event exceeds the configured event limit",
            ));
        }
        Ok(encoded)
    }
}

/// Upper bound on how long the shared heartbeat driver parks between sweeps.
const HEARTBEAT_SWEEP_MAX_PARK: Duration = Duration::from_millis(250);

/// One registered SSE connection slot. Slots hold their connection weakly so
/// a closed or dropped connection leaves the sweep on its own.
struct HeartbeatSlot {
    connection: Weak<SseConnectionInner>,
    interval: Duration,
    next: Instant,
}

/// Shared heartbeat scheduler: a single driver thread sweeps every registered
/// SSE connection and writes the heartbeat comments itself, replacing the
/// historical thread-per-connection heartbeat. Registration and removal are
/// O(1) map operations keyed by slot id.
///
/// The global reactor (`spectra_runtime::reactor`) is deliberately not reused
/// here even though spectra-api depends on spectra-runtime: its readiness
/// queue is owned by the language scheduler, whose host calls interpret timer
/// tokens as task timeouts. SSE heartbeats write to sockets directly and must
/// not inject events into that queue, so this transport keeps its own single
/// driver thread instead.
#[derive(Default)]
struct HeartbeatScheduler {
    slots: Mutex<HashMap<u64, HeartbeatSlot>>,
    signal: Condvar,
    next_id: AtomicU64,
    driver_started: AtomicBool,
}

/// Live SSE heartbeat driver threads across the process; observability for
/// the "one thread regardless of connection count" guarantee.
static HEARTBEAT_DRIVER_THREADS: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
fn heartbeat_driver_threads() -> usize {
    HEARTBEAT_DRIVER_THREADS.load(Ordering::Acquire)
}

fn heartbeat_slots(
    scheduler: &HeartbeatScheduler,
) -> MutexGuard<'_, HashMap<u64, HeartbeatSlot>> {
    scheduler
        .slots
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl HeartbeatScheduler {
    fn global() -> &'static Self {
        static SCHEDULER: LazyLock<HeartbeatScheduler> =
            LazyLock::new(HeartbeatScheduler::default);
        &SCHEDULER
    }

    /// Register a live connection for periodic heartbeats; returns the O(1)
    /// removal key that [`Self::unregister`] consumes.
    fn register(&self, connection: &Arc<SseConnectionInner>, interval: Duration) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        heartbeat_slots(self).insert(
            id,
            HeartbeatSlot {
                connection: Arc::downgrade(connection),
                interval,
                next: Instant::now() + interval,
            },
        );
        self.signal.notify_one();
        self.ensure_driver();
        id
    }

    fn unregister(&self, id: u64) {
        heartbeat_slots(self).remove(&id);
        self.signal.notify_one();
    }

    fn ensure_driver(&self) {
        if self.driver_started.swap(true, Ordering::AcqRel) {
            return;
        }
        HEARTBEAT_DRIVER_THREADS.fetch_add(1, Ordering::AcqRel);
        let spawned = thread::Builder::new()
            .name("spectra-api-sse-heartbeats".to_string())
            .spawn(|| run_heartbeat_scheduler(Self::global()))
            .is_ok();
        if !spawned {
            self.driver_started.store(false, Ordering::Release);
            HEARTBEAT_DRIVER_THREADS.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

/// Body of the single heartbeat-driver thread: prune dead slots, write one
/// heartbeat per due connection without holding the slot lock across socket
/// writes, then park until the nearest remaining deadline (bounded by
/// [`HEARTBEAT_SWEEP_MAX_PARK`]) or until a registration notifies.
fn run_heartbeat_scheduler(scheduler: &'static HeartbeatScheduler) {
    loop {
        let mut due = Vec::new();
        {
            let mut slots = heartbeat_slots(scheduler);
            let now = Instant::now();
            slots.retain(|id, slot| match slot.connection.upgrade() {
                None => false,
                Some(inner) if inner.closed.load(Ordering::Acquire) => false,
                Some(inner) => {
                    if slot.next <= now {
                        due.push((*id, inner));
                    }
                    true
                }
            });
        }
        for (id, connection) in due {
            let alive = connection.send_heartbeat().is_ok();
            let mut slots = heartbeat_slots(scheduler);
            if alive {
                if let Some(slot) = slots.get_mut(&id) {
                    slot.next = Instant::now() + slot.interval;
                }
            } else {
                slots.remove(&id);
            }
        }
        let wait = {
            let slots = heartbeat_slots(scheduler);
            let now = Instant::now();
            slots
                .values()
                .map(|slot| slot.next.saturating_duration_since(now))
                .min()
                .unwrap_or(HEARTBEAT_SWEEP_MAX_PARK)
                .min(HEARTBEAT_SWEEP_MAX_PARK)
        };
        let (guard, _) = scheduler
            .signal
            .wait_timeout(heartbeat_slots(scheduler), wait)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        drop(guard);
    }
}

struct SseConnectionInner {
    stream: Mutex<TcpStream>,
    closed: AtomicBool,
    peer: SocketAddr,
    last_event_id: Mutex<Option<String>>,
    max_event_bytes: usize,
}

impl fmt::Debug for SseConnectionInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SseConnectionInner")
            .field("peer", &self.peer)
            .field("closed", &self.closed.load(Ordering::Acquire))
            .finish()
    }
}

impl SseConnectionInner {
    fn write_bytes(&self, bytes: &[u8]) -> Result<(), SseError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(SseError::new(
                SseErrorKind::Closed,
                "SSE connection is closed",
            ));
        }
        let mut stream = self
            .stream
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Err(error) = stream.write_all(bytes).and_then(|_| stream.flush()) {
            self.closed.store(true, Ordering::Release);
            return Err(error.into());
        }
        Ok(())
    }

    fn send_event(&self, event: &SseEvent) -> Result<(), SseError> {
        let bytes = event.wire_format();
        if bytes.len() > self.max_event_bytes {
            return Err(SseError::new(
                SseErrorKind::PayloadTooLarge,
                "SSE event exceeds the configured event limit",
            ));
        }
        self.write_bytes(&bytes)?;
        if let Some(id) = event.id() {
            *self
                .last_event_id
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(id.to_string());
        }
        Ok(())
    }

    fn send_heartbeat(&self) -> Result<(), SseError> {
        self.write_bytes(b": heartbeat\n\n")
    }

    fn close(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            let stream = self
                .stream
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

pub struct SseConnection {
    inner: Arc<SseConnectionInner>,
    heartbeat_id: Option<u64>,
}

impl fmt::Debug for SseConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SseConnection")
            .field("peer", &self.peer())
            .field("last_event_id", &self.last_event_id())
            .finish()
    }
}

impl SseConnection {
    fn start(inner: Arc<SseConnectionInner>, heartbeat_interval: Duration) -> Self {
        let heartbeat_id = HeartbeatScheduler::global().register(&inner, heartbeat_interval);
        Self {
            inner,
            heartbeat_id: Some(heartbeat_id),
        }
    }

    pub fn peer(&self) -> SocketAddr {
        self.inner.peer
    }

    pub fn last_event_id(&self) -> Option<String> {
        self.inner
            .last_event_id
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn send_event(&self, event: &SseEvent) -> Result<(), SseError> {
        self.inner.send_event(event)
    }

    pub fn send_heartbeat(&self) -> Result<(), SseError> {
        self.inner.send_heartbeat()
    }

    pub fn close(&self) -> Result<(), SseError> {
        if let Some(id) = self.heartbeat_id {
            HeartbeatScheduler::global().unregister(id);
        }
        self.inner.close();
        Ok(())
    }
}

impl Drop for SseConnection {
    fn drop(&mut self) {
        if let Some(id) = self.heartbeat_id.take() {
            HeartbeatScheduler::global().unregister(id);
        }
        self.inner.close();
    }
}

struct SseRouteSubscriber {
    queue: VecDeque<Vec<u8>>,
    queued_bytes: usize,
    closed: bool,
    last_heartbeat: Instant,
}

impl SseRouteSubscriber {
    fn new(now: Instant) -> Self {
        Self {
            queue: VecDeque::new(),
            queued_bytes: 0,
            closed: false,
            last_heartbeat: now,
        }
    }

    fn enqueue(&mut self, bytes: Vec<u8>) {
        if self.closed {
            return;
        }
        if self.queued_bytes.saturating_add(bytes.len()) > MAX_ROUTE_PENDING_BYTES {
            self.closed = true;
            self.queue.clear();
            self.queued_bytes = 0;
            return;
        }
        self.queued_bytes = self.queued_bytes.saturating_add(bytes.len());
        self.queue.push_back(bytes);
    }

    fn drain_into(&mut self, output: &mut Vec<u8>) {
        while let Some(bytes) = self.queue.pop_front() {
            self.queued_bytes = self.queued_bytes.saturating_sub(bytes.len());
            output.extend_from_slice(&bytes);
        }
    }
}

#[derive(Default)]
struct SseServerState {
    history: VecDeque<SseEvent>,
    history_bytes: usize,
    connections: Vec<Weak<SseConnectionInner>>,
    routed_connections: Vec<Weak<Mutex<SseRouteSubscriber>>>,
}

impl SseServerState {
    fn replay_after(&self, last_event_id: &str) -> Vec<SseEvent> {
        let Some(index) = self
            .history
            .iter()
            .position(|event| event.id() == Some(last_event_id))
        else {
            return self
                .history
                .iter()
                .filter(|event| event.id().is_some())
                .cloned()
                .collect();
        };
        self.history
            .iter()
            .skip(index + 1)
            .filter(|event| event.id().is_some())
            .cloned()
            .collect()
    }

    fn register_routed_connection(
        &mut self,
        last_event_id: Option<&str>,
        now: Instant,
    ) -> Arc<Mutex<SseRouteSubscriber>> {
        let subscriber = Arc::new(Mutex::new(SseRouteSubscriber::new(now)));
        if let Some(last_event_id) = last_event_id.filter(|value| !value.is_empty()) {
            let replay = self.replay_after(last_event_id);
            let mut subscriber_guard = subscriber
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for event in replay {
                subscriber_guard.enqueue(event.wire_format());
            }
        }
        self.routed_connections.push(Arc::downgrade(&subscriber));
        subscriber
    }
}

#[derive(Clone)]
pub(crate) struct RoutedSseResponse {
    server: Arc<Mutex<SseServer>>,
}

pub(crate) struct RoutedSseConnection {
    subscriber: Arc<Mutex<SseRouteSubscriber>>,
    heartbeat_interval: Duration,
}

impl RoutedSseResponse {
    pub(crate) fn open(
        &self,
        request: &ParsedRequest,
    ) -> Result<(Vec<u8>, RoutedSseConnection), SseError> {
        if request.method != "GET" || request.version.major != 1 || request.version.minor != 1 {
            return Err(SseError::new(
                SseErrorKind::Handshake,
                "routed SSE requires GET over HTTP/1.1",
            ));
        }
        if let Some(accept) = header_value(&request.headers, "Accept") {
            let accepted = accept
                .split(',')
                .map(|value| value.trim().split(';').next().unwrap_or_default())
                .any(|value| value.eq_ignore_ascii_case("text/event-stream") || value == "*/*");
            if !accepted {
                return Err(SseError::new(
                    SseErrorKind::Handshake,
                    "SSE client does not accept text/event-stream",
                ));
            }
        }
        let last_event_id = header_value(&request.headers, "Last-Event-ID")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let now = Instant::now();
        let (subscriber, heartbeat_interval) = {
            let server = self
                .server
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let subscriber = server
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .register_routed_connection(last_event_id.as_deref(), now);
            (subscriber, server.config.heartbeat_interval)
        };
        Ok((
            routed_response_headers(),
            RoutedSseConnection {
                subscriber,
                heartbeat_interval,
            },
        ))
    }
}

impl RoutedSseConnection {
    pub(crate) fn poll_into(&self, now: Instant, output: &mut Vec<u8>) -> bool {
        let mut subscriber = self
            .subscriber
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if subscriber.closed {
            return false;
        }
        if now.duration_since(subscriber.last_heartbeat) >= self.heartbeat_interval {
            subscriber.enqueue(b": heartbeat\n\n".to_vec());
            subscriber.last_heartbeat = now;
        }
        if subscriber.closed {
            return false;
        }
        subscriber.drain_into(output);
        true
    }

    pub(crate) fn close(&self) {
        self.subscriber
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .closed = true;
    }
}

fn routed_response_headers() -> Vec<u8> {
    concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: text/event-stream; charset=utf-8\r\n",
        "Cache-Control: no-cache\r\n",
        "Connection: keep-alive\r\n",
        "X-Accel-Buffering: no\r\n",
        "\r\n"
    )
    .as_bytes()
    .to_vec()
}

fn routed_responses() -> &'static Mutex<HashMap<SpectraHostValue, Arc<RoutedSseResponse>>> {
    static RESPONSES: OnceLock<Mutex<HashMap<SpectraHostValue, Arc<RoutedSseResponse>>>> =
        OnceLock::new();
    RESPONSES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn routed_response_for_handle(
    handle: SpectraHostValue,
) -> Option<Arc<RoutedSseResponse>> {
    routed_responses()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&handle)
        .cloned()
}

pub(crate) fn routed_response_for_server(server: Arc<Mutex<SseServer>>) -> Arc<RoutedSseResponse> {
    Arc::new(RoutedSseResponse { server })
}

pub(crate) fn store_routed_response(server: Arc<Mutex<SseServer>>) -> Result<SpectraHostValue, ()> {
    let response = http::Status::new(200)
        .and_then(|status| {
            http::Response::new(status)
                .with_header("Content-Type", "text/event-stream; charset=utf-8")
        })
        .map_err(|_| ())?;
    let response_handle = http::store_response(response);
    routed_responses()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(response_handle, routed_response_for_server(server));
    Ok(response_handle)
}

pub struct SseServer {
    listener: Option<TcpListener>,
    config: SseConfig,
    state: Arc<Mutex<SseServerState>>,
}

impl fmt::Debug for SseServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SseServer")
            .field("local_addr", &self.local_addr().ok())
            .field("config", &self.config)
            .finish()
    }
}

impl SseServer {
    pub fn new() -> Self {
        Self {
            listener: None,
            config: SseConfig::default(),
            state: Arc::new(Mutex::new(SseServerState::default())),
        }
    }

    pub fn listen(&mut self, port: u16) -> Result<(), SseError> {
        if self.listener.is_some() {
            return Err(SseError::new(
                SseErrorKind::InvalidArgument,
                "SSE server is already listening",
            ));
        }
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        listener.set_nonblocking(true)?;
        self.listener = Some(listener);
        Ok(())
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener
            .as_ref()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotConnected, "SSE server is not listening")
            })?
            .local_addr()
    }

    pub fn set_heartbeat_interval(&mut self, interval: Duration) -> Result<(), SseError> {
        if interval.is_zero() || interval > MAX_HEARTBEAT_INTERVAL || self.listener.is_some() {
            return Err(SseError::new(
                SseErrorKind::InvalidArgument,
                "SSE heartbeat interval is outside the configurable range",
            ));
        }
        self.config.heartbeat_interval = interval;
        Ok(())
    }

    pub fn set_replay_capacity(&mut self, capacity: usize) -> Result<(), SseError> {
        if capacity == 0 || capacity > MAX_REPLAY_CAPACITY || self.listener.is_some() {
            return Err(SseError::new(
                SseErrorKind::InvalidArgument,
                "SSE replay capacity is outside the configurable range",
            ));
        }
        self.config.replay_capacity = capacity;
        Ok(())
    }

    pub fn set_max_event_bytes(&mut self, max_bytes: usize) -> Result<(), SseError> {
        if max_bytes == 0 || max_bytes > DEFAULT_MAX_EVENT_BYTES || self.listener.is_some() {
            return Err(SseError::new(
                SseErrorKind::InvalidArgument,
                "SSE event limit is outside the configurable range",
            ));
        }
        self.config.max_event_bytes = max_bytes;
        Ok(())
    }

    pub fn publish_event(&self, event: &SseEvent) -> Result<usize, SseError> {
        let encoded = self.config.validate_event(event)?;
        let (targets, routed_targets) = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.history.push_back(event.clone());
            state.history_bytes = state.history_bytes.saturating_add(encoded.len());
            while state.history.len() > self.config.replay_capacity
                || state.history_bytes > MAX_REPLAY_BYTES
            {
                if let Some(oldest) = state.history.pop_front() {
                    state.history_bytes = state
                        .history_bytes
                        .saturating_sub(oldest.wire_format().len());
                } else {
                    break;
                }
            }
            let mut live = Vec::with_capacity(state.connections.len());
            let mut targets = Vec::new();
            for weak in state.connections.drain(..) {
                if let Some(inner) = weak.upgrade() {
                    live.push(Arc::downgrade(&inner));
                    targets.push(inner);
                }
            }
            state.connections = live;
            let mut routed_live = Vec::with_capacity(state.routed_connections.len());
            let mut routed_targets = Vec::new();
            for weak in state.routed_connections.drain(..) {
                if let Some(subscriber) = weak.upgrade() {
                    routed_live.push(Arc::downgrade(&subscriber));
                    routed_targets.push(subscriber);
                }
            }
            state.routed_connections = routed_live;
            (targets, routed_targets)
        };
        let mut delivered = 0;
        for inner in targets {
            if inner.write_bytes(&encoded).is_ok() {
                if let Some(id) = event.id() {
                    *inner
                        .last_event_id
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(id.to_string());
                }
                delivered += 1;
            }
        }
        for subscriber in routed_targets {
            let mut subscriber = subscriber
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !subscriber.closed {
                subscriber.enqueue(encoded.clone());
                if !subscriber.closed {
                    delivered += 1;
                }
            }
        }
        Ok(delivered)
    }

    pub fn accept(&self, cancellation: &CancellationToken) -> Result<SseConnection, SseError> {
        let listener = self.listener.as_ref().ok_or_else(|| {
            SseError::new(SseErrorKind::InvalidArgument, "SSE server is not listening")
        })?;
        loop {
            if cancellation.load(Ordering::Acquire) {
                return Err(SseError::new(
                    SseErrorKind::Cancelled,
                    "SSE accept was cancelled",
                ));
            }
            match listener.accept() {
                Ok((stream, peer)) => {
                    stream.set_nonblocking(false)?;
                    stream.set_nodelay(true)?;
                    stream.set_read_timeout(Some(self.config.read_timeout))?;
                    stream.set_write_timeout(Some(self.config.write_timeout))?;
                    let HandshakeResult {
                        stream,
                        id: last_event_id,
                    } = accept_handshake(stream, peer, DEFAULT_HANDSHAKE_BYTES)?;
                    let replay_id = last_event_id.clone();
                    let inner = Arc::new(SseConnectionInner {
                        stream: Mutex::new(stream),
                        closed: AtomicBool::new(false),
                        peer,
                        last_event_id: Mutex::new(if last_event_id.is_empty() {
                            None
                        } else {
                            Some(last_event_id)
                        }),
                        max_event_bytes: self.config.max_event_bytes,
                    });
                    {
                        let mut state = self
                            .state
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        let replay = state
                            .replay_after(&replay_id)
                            .into_iter()
                            .filter(|_| !replay_id.is_empty())
                            .collect::<Vec<_>>();
                        for event in replay {
                            inner.send_event(&event)?;
                        }
                        state.connections.push(Arc::downgrade(&inner));
                    }
                    return Ok(SseConnection::start(inner, self.config.heartbeat_interval));
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl Default for SseServer {
    fn default() -> Self {
        Self::new()
    }
}

struct HandshakeResult {
    stream: TcpStream,
    id: String,
}

fn accept_handshake(
    mut stream: TcpStream,
    peer: SocketAddr,
    limit: usize,
) -> Result<HandshakeResult, SseError> {
    let mut buffered = Vec::new();
    let header_end = read_http_headers(&mut stream, &mut buffered, limit)?;
    let request = crate::http::parse_request(&buffered[..header_end]).map_err(|error| {
        SseError::new(
            SseErrorKind::Handshake,
            format!("invalid SSE HTTP request: {error}"),
        )
    })?;
    if request.method != "GET"
        || request.version.major != 1
        || request.version.minor != 1
        || !request.target.starts_with('/')
    {
        return Err(SseError::new(
            SseErrorKind::Handshake,
            "SSE requires GET over HTTP/1.1 with an absolute path",
        ));
    }
    if let Some(accept) = header_value(&request.headers, "Accept") {
        let accepted = accept
            .split(',')
            .map(|value| value.trim().split(';').next().unwrap_or_default())
            .any(|value| value.eq_ignore_ascii_case("text/event-stream") || value == "*/*");
        if !accepted {
            return Err(SseError::new(
                SseErrorKind::Handshake,
                "SSE client does not accept text/event-stream",
            ));
        }
    }
    let id = header_value(&request.headers, "Last-Event-ID")
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let response = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: text/event-stream; charset=utf-8\r\n",
        "Cache-Control: no-cache\r\n",
        "Connection: keep-alive\r\n",
        "X-Accel-Buffering: no\r\n",
        "\r\n"
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    let _ = peer;
    Ok(HandshakeResult { stream, id })
}

fn read_http_headers<R: Read>(
    stream: &mut R,
    buffered: &mut Vec<u8>,
    limit: usize,
) -> Result<usize, SseError> {
    loop {
        if let Some(index) = buffered.windows(4).position(|window| window == b"\r\n\r\n") {
            return Ok(index + 4);
        }
        if buffered.len() >= limit {
            return Err(SseError::new(
                SseErrorKind::PayloadTooLarge,
                "SSE handshake headers exceed the configured limit",
            ));
        }
        let mut chunk = [0_u8; 4096];
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(SseError::new(
                SseErrorKind::Closed,
                "peer closed before the SSE handshake completed",
            ));
        }
        buffered.extend_from_slice(&chunk[..read]);
    }
}

fn header_value<'a>(headers: &'a [crate::http::Header], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str())
}

struct SseStore {
    servers: ApiHandleTable<Arc<Mutex<SseServer>>>,
    connections: ApiHandleTable<Arc<Mutex<SseConnection>>>,
    events: ApiHandleTable<SseEvent>,
}

impl SseStore {
    fn new() -> Self {
        Self {
            servers: ApiHandleTable::new(HandleKind::ApiSseServer),
            connections: ApiHandleTable::new(HandleKind::ApiSseConnection),
            events: ApiHandleTable::new(HandleKind::ApiSseEvent),
        }
    }
}

fn store() -> &'static Mutex<SseStore> {
    static STORE: OnceLock<Mutex<SseStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(SseStore::new()))
}

pub extern "C" fn server_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let mut store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(
        ctx,
        store.servers.insert(Arc::new(Mutex::new(SseServer::new()))),
    )
}

pub extern "C" fn server_response(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let server = {
        let store = store()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(server) = store.servers.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(server)
    };
    match store_routed_response(server) {
        Ok(response_handle) => write_result(ctx, response_handle),
        Err(()) => HOST_STATUS_INTERNAL_ERROR,
    }
}

pub extern "C" fn server_listen(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(port) = u16::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let server = {
        let store = store()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(server) = store.servers.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(server)
    };
    let result = server
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .listen(port);
    write_result(ctx, i64::from(result.is_ok()))
}

pub extern "C" fn server_local_port(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let server = {
        let store = store()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(server) = store.servers.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(server)
    };
    let port = server
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .local_addr()
        .map(|address| i64::from(address.port()))
        .unwrap_or(0);
    write_result(ctx, port)
}

pub extern "C" fn server_set_heartbeat_interval(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(interval_ms) = u64::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let server = {
        let store = store()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(server) = store.servers.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(server)
    };
    let result = server
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .set_heartbeat_interval(Duration::from_millis(interval_ms));
    write_result(ctx, i64::from(result.is_ok()))
}

pub extern "C" fn server_set_replay_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(capacity) = usize::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let server = {
        let store = store()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(server) = store.servers.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(server)
    };
    let result = server
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .set_replay_capacity(capacity);
    write_result(ctx, i64::from(result.is_ok()))
}

pub extern "C" fn server_set_max_event_bytes(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(max_bytes) = usize::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let server = {
        let store = store()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(server) = store.servers.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(server)
    };
    let result = server
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .set_max_event_bytes(max_bytes);
    write_result(ctx, i64::from(result.is_ok()))
}

pub extern "C" fn server_accept(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let server = {
        let store = store()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(server) = store.servers.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(server)
    };
    let task = spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |token| {
        let connection = server
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .accept(&token)
            .map_err(|_| ())?;
        let mut store = store()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(store.connections.insert(Arc::new(Mutex::new(connection))))
    });
    match task {
        Ok(task) => write_result(ctx, task),
        Err(_) => HOST_STATUS_INTERNAL_ERROR,
    }
}

pub extern "C" fn server_publish(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (server, event) = {
        let store = store()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(server) = store.servers.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(event) = store.events.get(&args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        (Arc::clone(server), event.clone())
    };
    let task = spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |_| {
        server
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .publish_event(&event)
            .map(|delivered| delivered as SpectraHostValue)
            .map_err(|_| ())
    });
    match task {
        Ok(task) => write_result(ctx, task),
        Err(_) => HOST_STATUS_INTERNAL_ERROR,
    }
}

pub extern "C" fn event_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 4) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(id), Some(event_type), Some(data)) = (
        read_spectra_string(args[0]),
        read_spectra_string(args[1]),
        read_spectra_string(args[2]),
    ) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(retry_ms) = u64::try_from(args[3]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let id = (!id.is_empty()).then_some(id);
    let event_type = (!event_type.is_empty()).then_some(event_type);
    let retry_ms = (retry_ms != 0).then_some(retry_ms);
    let Ok(event) = SseEvent::new(id, event_type, data, retry_ms) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, store.events.insert(event))
}

fn event_string_result(
    ctx: *mut SpectraHostCallContext,
    event_handle: SpectraHostValue,
    selector: impl FnOnce(&SseEvent) -> Option<&str>,
) -> i32 {
    let store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(event) = store.events.get(&event_handle) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        alloc_spectra_string(selector(event).unwrap_or_default()),
    )
}

pub extern "C" fn event_id(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    event_string_result(ctx, args[0], SseEvent::id)
}

pub extern "C" fn event_type(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    event_string_result(ctx, args[0], SseEvent::event_type)
}

pub extern "C" fn event_data(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    event_string_result(ctx, args[0], |event| Some(event.data()))
}

pub extern "C" fn event_retry_ms(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(event) = store.events.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        event.retry_ms().unwrap_or_default() as SpectraHostValue,
    )
}

pub extern "C" fn event_release(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, i64::from(store.events.remove(&args[0]).is_some()))
}

pub extern "C" fn connection_peer_port(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(connection) = store.connections.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let port = connection
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .peer()
        .port() as SpectraHostValue;
    write_result(ctx, port)
}

pub extern "C" fn connection_last_event_id(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(connection) = store.connections.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let value = connection
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .last_event_id()
        .unwrap_or_default();
    write_result(ctx, alloc_spectra_string(&value))
}

fn connection_task(
    ctx: *mut SpectraHostCallContext,
    connection_handle: SpectraHostValue,
    operation: impl FnOnce(&SseConnection) -> Result<(), SseError> + Send + 'static,
) -> i32 {
    let connection = {
        let store = store()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(connection) = store.connections.get(&connection_handle) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(connection)
    };
    let task = spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |_| {
        let connection = connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        operation(&connection).map_err(|_| ())?;
        Ok(1)
    });
    match task {
        Ok(task) => write_result(ctx, task),
        Err(_) => HOST_STATUS_INTERNAL_ERROR,
    }
}

pub extern "C" fn connection_send(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let event = {
        let store = store()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(event) = store.events.get(&args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        event.clone()
    };
    connection_task(ctx, args[0], move |connection| {
        connection.send_event(&event)
    })
}

pub extern "C" fn connection_heartbeat(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    connection_task(ctx, args[0], SseConnection::send_heartbeat)
}

pub extern "C" fn connection_close(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    connection_task(ctx, args[0], SseConnection::close)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// Serializes tests that observe process-global heartbeat driver state.
    static HEARTBEAT_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn request(last_event_id: Option<&str>) -> String {
        let mut value = String::from(
            "GET /events HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: text/event-stream\r\n",
        );
        if let Some(last_event_id) = last_event_id {
            value.push_str("Last-Event-ID: ");
            value.push_str(last_event_id);
            value.push_str("\r\n");
        }
        value.push_str("\r\n");
        value
    }

    fn read_until(stream: &mut TcpStream, marker: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        let mut chunk = [0_u8; 4096];
        while !data.windows(marker.len()).any(|window| window == marker) {
            let count = stream.read(&mut chunk).expect("read SSE bytes");
            assert!(count > 0, "SSE peer closed before marker");
            data.extend_from_slice(&chunk[..count]);
        }
        data
    }

    #[test]
    fn event_serialization_preserves_multiline_data_and_retry_hint() {
        let event = SseEvent::new(
            Some("42".to_string()),
            Some("update".to_string()),
            "first\nsecond".to_string(),
            Some(1_000),
        )
        .expect("valid SSE event");
        assert_eq!(
            event.wire_format(),
            b"id: 42\nevent: update\ndata: first\ndata: second\nretry: 1000\n\n"
        );
    }

    #[test]
    fn streams_events_and_automatic_heartbeats() {
        let mut server = SseServer::new();
        server
            .set_heartbeat_interval(Duration::from_millis(40))
            .expect("configure SSE heartbeat");
        server.listen(0).expect("listen SSE server");
        let port = server.local_addr().expect("SSE server address").port();
        let server = Arc::new(Mutex::new(server));
        let cancellation = Arc::new(AtomicBool::new(false));
        let (connection_tx, connection_rx) = mpsc::channel();
        let accept_server = Arc::clone(&server);
        let accept_cancel = Arc::clone(&cancellation);
        thread::spawn(move || {
            let connection = accept_server
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .accept(&accept_cancel)
                .expect("accept SSE client");
            connection_tx.send(connection).expect("send SSE connection");
        });

        let mut client = TcpStream::connect(("127.0.0.1", port)).expect("connect SSE client");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("SSE client read timeout");
        client
            .write_all(request(None).as_bytes())
            .expect("write SSE request");
        let response = read_until(&mut client, b"\r\n\r\n");
        assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 200 OK"));
        assert!(String::from_utf8_lossy(&response).contains("text/event-stream"));
        let connection = connection_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive SSE connection");
        let event = SseEvent::new(
            Some("1".to_string()),
            Some("message".to_string()),
            "hello".to_string(),
            Some(500),
        )
        .expect("valid SSE message");
        connection.send_event(&event).expect("send SSE event");
        let event_bytes = read_until(&mut client, b"\n\n");
        let event_text = String::from_utf8_lossy(&event_bytes);
        assert!(event_text.contains("id: 1"));
        assert!(event_text.contains("retry: 500"));
        let heartbeat = read_until(&mut client, b"\n\n");
        assert!(String::from_utf8_lossy(&heartbeat).contains(": heartbeat"));
        connection.close().expect("close SSE connection");
    }

    #[test]
    fn last_event_id_replays_only_new_identified_events() {
        let mut server = SseServer::new();
        server.set_replay_capacity(8).expect("configure replay");
        server.listen(0).expect("listen replay server");
        let first = SseEvent::new(Some("1".to_string()), None, "old".to_string(), None)
            .expect("first event");
        let second = SseEvent::new(Some("2".to_string()), None, "new".to_string(), None)
            .expect("second event");
        let unnamed =
            SseEvent::new(None, None, "not-replayable".to_string(), None).expect("unnamed event");
        assert_eq!(server.publish_event(&first).expect("store first event"), 0);
        assert_eq!(
            server.publish_event(&unnamed).expect("store unnamed event"),
            0
        );
        assert_eq!(
            server.publish_event(&second).expect("store second event"),
            0
        );
        let port = server.local_addr().expect("replay server address").port();
        let cancellation = Arc::new(AtomicBool::new(false));
        let server = Arc::new(Mutex::new(server));
        let accept_server = Arc::clone(&server);
        let accept_cancel = Arc::clone(&cancellation);
        let (connection_tx, connection_rx) = mpsc::channel();
        thread::spawn(move || {
            let connection = accept_server
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .accept(&accept_cancel)
                .expect("accept replay client");
            connection_tx
                .send(connection)
                .expect("send replay connection");
        });
        let mut client = TcpStream::connect(("127.0.0.1", port)).expect("connect replay client");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("replay client read timeout");
        client
            .write_all(request(Some("1")).as_bytes())
            .expect("write replay request");
        let data = read_until(&mut client, b"data: new\n\n");
        let body = String::from_utf8_lossy(&data);
        assert!(body.starts_with("HTTP/1.1 200 OK"));
        assert!(!body.contains("data: old"));
        assert!(body.contains("id: 2\ndata: new\n\n"));
        let connection = connection_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive replay connection");
        assert_eq!(connection.last_event_id().as_deref(), Some("2"));
        connection.close().expect("close replay connection");
    }

    #[test]
    fn fifty_connections_share_a_single_heartbeat_driver_thread() {
        let _heartbeat_guard = HEARTBEAT_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        const CONNECTIONS: usize = 50;
        let baseline_drivers = heartbeat_driver_threads();

        let mut server = SseServer::new();
        server
            .set_heartbeat_interval(Duration::from_millis(60))
            .expect("configure SSE heartbeat");
        server.listen(0).expect("listen SSE server");
        let port = server.local_addr().expect("SSE server address").port();
        let server = Arc::new(Mutex::new(server));
        let cancellation = Arc::new(AtomicBool::new(false));

        let accept_server = Arc::clone(&server);
        let accept_cancel = Arc::clone(&cancellation);
        let (connections_tx, connections_rx) = mpsc::channel();
        thread::spawn(move || {
            let mut accepted = Vec::with_capacity(CONNECTIONS);
            for _ in 0..CONNECTIONS {
                let connection = accept_server
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .accept(&accept_cancel)
                    .expect("accept SSE client");
                accepted.push(connection);
            }
            connections_tx
                .send(accepted)
                .expect("send SSE connections");
        });

        let mut clients = Vec::with_capacity(CONNECTIONS);
        for _ in 0..CONNECTIONS {
            let mut client =
                TcpStream::connect(("127.0.0.1", port)).expect("connect SSE client");
            client
                .set_read_timeout(Some(Duration::from_secs(10)))
                .expect("SSE client read timeout");
            client
                .write_all(request(None).as_bytes())
                .expect("write SSE request");
            let response = read_until(&mut client, b"\r\n\r\n");
            assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 200 OK"));
            clients.push(client);
        }

        let connections = connections_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("receive SSE connections");
        assert_eq!(connections.len(), CONNECTIONS);
        assert!(
            heartbeat_driver_threads() <= baseline_drivers + 1,
            "50 SSE connections must not explode heartbeat thread count"
        );

        for client in &mut clients {
            let heartbeat = read_until(client, b": heartbeat");
            assert!(String::from_utf8_lossy(&heartbeat).contains(": heartbeat"));
        }
        assert!(
            heartbeat_driver_threads() <= baseline_drivers + 1,
            "heartbeat driver count must stay constant across sweeps"
        );

        cancellation.store(true, Ordering::Release);
    }
}
