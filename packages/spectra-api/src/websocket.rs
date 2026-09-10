//! RFC 6455 WebSocket server and frame transport.
//!
//! The HTTP/1 server deliberately keeps its request parser and connection
//! state machine focused on HTTP.  This module owns the protocol transition
//! and the long-lived framed connection used by the public
//! `std.api.websocket` surface.  The implementation is synchronous at the
//! socket boundary but every potentially blocking public operation is exposed
//! through a cancellable runtime task.

use crate::handles::ApiHandleTable;
use crate::http::ParsedRequest;
use crate::routing;
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use ring::digest;
use ring::rand::{SecureRandom, SystemRandom};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INTERNAL_ERROR,
    HOST_STATUS_INVALID_ARGUMENT,
};
use spectra_runtime::handles::HandleKind;
use spectra_runtime::stdlib::CancellationToken;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

pub const MESSAGE_TEXT: SpectraHostValue = 1;
pub const MESSAGE_BINARY: SpectraHostValue = 2;

const DEFAULT_MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_HANDSHAKE_BYTES: usize = 64 * 1024;
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const WEBSOCKET_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameRole {
    Client,
    Server,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebSocketFrame {
    pub fin: bool,
    pub rsv1: bool,
    pub opcode: u8,
    pub masked: bool,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebSocketMessage {
    Text(String),
    Binary(Vec<u8>),
}

impl WebSocketMessage {
    pub fn kind(&self) -> SpectraHostValue {
        match self {
            Self::Text(_) => MESSAGE_TEXT,
            Self::Binary(_) => MESSAGE_BINARY,
        }
    }

    pub fn payload(&self) -> Vec<u8> {
        match self {
            Self::Text(value) => value.as_bytes().to_vec(),
            Self::Binary(value) => value.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebSocketErrorKind {
    Io,
    Protocol,
    Handshake,
    UnsupportedScheme,
    PayloadTooLarge,
    InvalidArgument,
    Compression,
    Utf8,
    Closed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebSocketError {
    pub kind: WebSocketErrorKind,
    pub message: String,
}

impl WebSocketError {
    fn new(kind: WebSocketErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebSocket {:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for WebSocketError {}

impl From<io::Error> for WebSocketError {
    fn from(error: io::Error) -> Self {
        Self::new(WebSocketErrorKind::Io, error.to_string())
    }
}

#[derive(Clone, Debug)]
pub struct WebSocketConfig {
    pub max_frame_bytes: usize,
    pub max_message_bytes: usize,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub per_message_deflate: bool,
    pub subprotocols: Vec<String>,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            max_frame_bytes: DEFAULT_MAX_FRAME_BYTES,
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            read_timeout: DEFAULT_READ_TIMEOUT,
            write_timeout: DEFAULT_WRITE_TIMEOUT,
            per_message_deflate: false,
            subprotocols: Vec::new(),
        }
    }
}

#[derive(Debug)]
enum WebSocketTransport {
    Tcp(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Read for WebSocketTransport {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(buffer),
            Self::Tls(stream) => stream.read(buffer),
        }
    }
}

impl Write for WebSocketTransport {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(buffer),
            Self::Tls(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            Self::Tls(stream) => stream.flush(),
        }
    }
}

impl WebSocketTransport {
    fn peer_addr(&self) -> io::Result<SocketAddr> {
        match self {
            Self::Tcp(stream) => stream.peer_addr(),
            Self::Tls(stream) => stream.get_ref().peer_addr(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct WebSocketClient {
    config: WebSocketConfig,
    reconnect_attempts: u32,
    reconnect_backoff: Duration,
    ssrf_policy: crate::security::SsrfPolicy,
    tls_config: Option<Arc<ClientConfig>>,
}

impl WebSocketClient {
    pub fn new() -> Self {
        Self {
            config: WebSocketConfig::default(),
            reconnect_attempts: 0,
            reconnect_backoff: Duration::from_millis(100),
            ssrf_policy: crate::security::SsrfPolicy::default(),
            tls_config: None,
        }
    }

    pub fn set_per_message_deflate(&mut self, enabled: bool) {
        self.config.per_message_deflate = enabled;
    }

    pub fn set_max_message_bytes(&mut self, max_bytes: usize) -> Result<(), WebSocketError> {
        if max_bytes == 0 || max_bytes > DEFAULT_MAX_MESSAGE_BYTES {
            return Err(WebSocketError::new(
                WebSocketErrorKind::InvalidArgument,
                "WebSocket message limit is outside the supported range",
            ));
        }
        self.config.max_message_bytes = max_bytes;
        self.config.max_frame_bytes = self.config.max_frame_bytes.min(max_bytes);
        Ok(())
    }

    pub fn set_reconnect(
        &mut self,
        attempts: u32,
        backoff: Duration,
    ) -> Result<(), WebSocketError> {
        if attempts > 10 || backoff > Duration::from_secs(60) {
            return Err(WebSocketError::new(
                WebSocketErrorKind::InvalidArgument,
                "WebSocket reconnect configuration is outside the supported range",
            ));
        }
        self.reconnect_attempts = attempts;
        self.reconnect_backoff = backoff;
        Ok(())
    }

    pub fn allow_private_networks(&mut self, allow: bool) {
        self.ssrf_policy = self.ssrf_policy.clone().allow_private_networks(allow);
    }

    pub fn set_tls_config(&mut self, config: Arc<ClientConfig>) {
        self.tls_config = Some(config);
    }

    pub fn connect(
        &self,
        url: &str,
        cancellation: &CancellationToken,
    ) -> Result<WebSocketConnection, WebSocketError> {
        let mut attempt = 0_u32;
        loop {
            if cancellation.load(std::sync::atomic::Ordering::Acquire) {
                return Err(WebSocketError::new(
                    WebSocketErrorKind::Cancelled,
                    "WebSocket connect was cancelled",
                ));
            }
            match self.connect_once(url, cancellation) {
                Ok(connection) => return Ok(connection),
                Err(error) if attempt < self.reconnect_attempts => {
                    let factor = 1_u32.checked_shl(attempt.min(5)).unwrap_or(32);
                    let delay = self.reconnect_backoff.saturating_mul(factor);
                    sleep_with_cancellation(delay, cancellation)?;
                    attempt += 1;
                    let _ = error;
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn connect_once(
        &self,
        url: &str,
        cancellation: &CancellationToken,
    ) -> Result<WebSocketConnection, WebSocketError> {
        let parsed = parse_websocket_url(url)?;
        let addresses = (parsed.host.as_str(), parsed.port)
            .to_socket_addrs()
            .map_err(|error| WebSocketError::new(WebSocketErrorKind::Io, error.to_string()))?
            .collect::<Vec<_>>();
        let address = addresses
            .iter()
            .copied()
            .find(|address| self.ssrf_policy.allows_address(*address))
            .ok_or_else(|| {
                WebSocketError::new(
                    WebSocketErrorKind::InvalidArgument,
                    "WebSocket SSRF policy rejected every resolved address",
                )
            })?;
        if cancellation.load(std::sync::atomic::Ordering::Acquire) {
            return Err(WebSocketError::new(
                WebSocketErrorKind::Cancelled,
                "WebSocket connect was cancelled",
            ));
        }
        let stream = TcpStream::connect_timeout(&address, self.config.read_timeout)
            .map_err(|error| WebSocketError::new(WebSocketErrorKind::Io, error.to_string()))?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(self.config.read_timeout))?;
        stream.set_write_timeout(Some(self.config.write_timeout))?;
        let transport = if parsed.tls {
            let server_name = ServerName::try_from(parsed.host.clone()).map_err(|error| {
                WebSocketError::new(
                    WebSocketErrorKind::Handshake,
                    format!("invalid WebSocket TLS server name: {error}"),
                )
            })?;
            let tls_config = match &self.tls_config {
                Some(config) => Arc::clone(config),
                None => crate::tls::TlsClientConfig::with_webpki_roots()
                    .build()
                    .map_err(|error| {
                        WebSocketError::new(
                            WebSocketErrorKind::Handshake,
                            format!("failed to build WebSocket TLS trust store: {error}"),
                        )
                    })?,
            };
            let connection = ClientConnection::new(tls_config, server_name).map_err(|error| {
                WebSocketError::new(
                    WebSocketErrorKind::Handshake,
                    format!("failed to create WebSocket TLS connection: {error}"),
                )
            })?;
            WebSocketTransport::Tls(Box::new(StreamOwned::new(connection, stream)))
        } else {
            WebSocketTransport::Tcp(stream)
        };
        client_handshake(transport, &parsed, &self.config)
    }
}

impl Default for WebSocketClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
struct ParsedWebSocketUrl {
    host: String,
    port: u16,
    host_header: String,
    path: String,
    tls: bool,
}

#[derive(Clone, Debug)]
struct FragmentState {
    opcode: u8,
    compressed: bool,
    payload: Vec<u8>,
}

/// Incremental assembly of data frames into complete WebSocket messages.
/// Shared verbatim by the synchronous `receive_message` machine and the
/// async TLS-gateway pump so both legs enforce identical fragmentation,
/// compression, and size-limit semantics.
#[derive(Default)]
struct MessageAssembler {
    fragments: Option<FragmentState>,
}

impl MessageAssembler {
    fn new() -> Self {
        Self::default()
    }

    /// Feeds one data frame (opcode 0x0/0x1/0x2). Returns the completed
    /// message when a FIN frame closes a fragmented sequence.
    fn assemble(
        &mut self,
        frame: WebSocketFrame,
        config: &WebSocketConfig,
        per_message_deflate: bool,
    ) -> Result<Option<WebSocketMessage>, WebSocketError> {
        match frame.opcode {
            0x0 => {
                let Some(mut state) = self.fragments.take() else {
                    return Err(WebSocketError::new(
                        WebSocketErrorKind::Protocol,
                        "continuation frame without a fragmented message",
                    ));
                };
                if frame.rsv1 {
                    return Err(WebSocketError::new(
                        WebSocketErrorKind::Protocol,
                        "continuation frame must not set RSV1",
                    ));
                }
                append_with_limit(&mut state.payload, &frame.payload, config.max_message_bytes)?;
                if frame.fin {
                    return finish_fragment(state, config, per_message_deflate);
                }
                self.fragments = Some(state);
            }
            0x1 | 0x2 => {
                if self.fragments.is_some() {
                    return Err(WebSocketError::new(
                        WebSocketErrorKind::Protocol,
                        "new data frame interrupted a fragmented message",
                    ));
                }
                let state = FragmentState {
                    opcode: frame.opcode,
                    compressed: frame.rsv1,
                    payload: frame.payload,
                };
                if frame.fin {
                    return finish_fragment(state, config, per_message_deflate);
                }
                self.fragments = Some(state);
            }
            _ => {
                return Err(WebSocketError::new(
                    WebSocketErrorKind::Protocol,
                    "reserved WebSocket opcode",
                ));
            }
        }
        Ok(None)
    }
}

fn finish_fragment(
    state: FragmentState,
    config: &WebSocketConfig,
    per_message_deflate: bool,
) -> Result<Option<WebSocketMessage>, WebSocketError> {
    let payload = if state.compressed {
        if !per_message_deflate {
            return Err(WebSocketError::new(
                WebSocketErrorKind::Protocol,
                "compressed WebSocket message was not negotiated",
            ));
        }
        decompress_message(&state.payload, config.max_message_bytes)?
    } else {
        state.payload
    };
    if payload.len() > config.max_message_bytes {
        return Err(WebSocketError::new(
            WebSocketErrorKind::PayloadTooLarge,
            "WebSocket message exceeds the configured limit",
        ));
    }
    match state.opcode {
        0x1 => String::from_utf8(payload)
            .map(|value| Some(WebSocketMessage::Text(value)))
            .map_err(|_| {
                WebSocketError::new(
                    WebSocketErrorKind::Utf8,
                    "WebSocket text message is not valid UTF-8",
                )
            }),
        0x2 => Ok(Some(WebSocketMessage::Binary(payload))),
        _ => Err(WebSocketError::new(
            WebSocketErrorKind::Protocol,
            "invalid fragmented message opcode",
        )),
    }
}

pub struct WebSocketConnection {
    stream: WebSocketTransport,
    peer: SocketAddr,
    config: WebSocketConfig,
    role: FrameRole,
    per_message_deflate: bool,
    assembler: MessageAssembler,
    buffered: Vec<u8>,
    closed: bool,
}

impl fmt::Debug for WebSocketConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebSocketConnection")
            .field("peer", &self.peer)
            .field("role", &self.role)
            .field("per_message_deflate", &self.per_message_deflate)
            .field("closed", &self.closed)
            .finish()
    }
}

impl WebSocketConnection {
    pub fn peer(&self) -> SocketAddr {
        self.peer
    }

    pub fn negotiated_per_message_deflate(&self) -> bool {
        self.per_message_deflate
    }

    pub fn send_text(&mut self, value: &str) -> Result<(), WebSocketError> {
        self.send_message(WebSocketMessage::Text(value.to_string()))
    }

    pub fn send_binary(&mut self, value: Vec<u8>) -> Result<(), WebSocketError> {
        self.send_message(WebSocketMessage::Binary(value))
    }

    pub fn send_message(&mut self, message: WebSocketMessage) -> Result<(), WebSocketError> {
        if self.closed {
            return Err(WebSocketError::new(
                WebSocketErrorKind::Closed,
                "cannot send on a closed WebSocket",
            ));
        }
        let (opcode, mut payload) = match message {
            WebSocketMessage::Text(value) => (0x1, value.into_bytes()),
            WebSocketMessage::Binary(value) => (0x2, value),
        };
        if payload.len() > self.config.max_message_bytes {
            return Err(WebSocketError::new(
                WebSocketErrorKind::PayloadTooLarge,
                "WebSocket message exceeds the configured limit",
            ));
        }
        let mut compressed = false;
        if self.per_message_deflate && !payload.is_empty() {
            let candidate = compress_message(&payload)?;
            if candidate.len() < payload.len() {
                payload = candidate;
                compressed = true;
            }
        }
        self.write_frame(WebSocketFrame {
            fin: true,
            rsv1: compressed,
            opcode,
            masked: false,
            payload,
        })
    }

    pub fn send_ping(&mut self, payload: Vec<u8>) -> Result<(), WebSocketError> {
        self.send_control(0x9, payload)
    }

    pub fn send_pong(&mut self, payload: Vec<u8>) -> Result<(), WebSocketError> {
        self.send_control(0xA, payload)
    }

    pub fn close(&mut self, code: u16, reason: &str) -> Result<(), WebSocketError> {
        if self.closed {
            return Ok(());
        }
        let mut payload = Vec::new();
        if code != 0 {
            validate_close_code(code)?;
            if !reason.is_empty() && reason.len() > 123 {
                return Err(WebSocketError::new(
                    WebSocketErrorKind::PayloadTooLarge,
                    "WebSocket close reason is too long",
                ));
            }
            payload.extend_from_slice(&code.to_be_bytes());
            payload.extend_from_slice(reason.as_bytes());
        }
        self.send_control(0x8, payload)?;
        self.closed = true;
        Ok(())
    }

    pub fn receive_message(&mut self) -> Result<Option<WebSocketMessage>, WebSocketError> {
        if self.closed {
            return Ok(None);
        }
        loop {
            let frame = self.read_frame()?;
            match frame.opcode {
                0x8 => {
                    self.handle_peer_close(&frame.payload)?;
                    return Ok(None);
                }
                0x9 => {
                    self.send_pong(frame.payload)?;
                }
                0xA => {}
                _ => {
                    if let Some(message) =
                        self.assembler
                            .assemble(frame, &self.config, self.per_message_deflate)?
                    {
                        return Ok(Some(message));
                    }
                }
            }
        }
    }

    fn handle_peer_close(&mut self, payload: &[u8]) -> Result<(), WebSocketError> {
        validate_close_payload(payload)?;
        if !self.closed {
            self.write_frame(WebSocketFrame {
                fin: true,
                rsv1: false,
                opcode: 0x8,
                masked: false,
                payload: payload.to_vec(),
            })?;
        }
        self.closed = true;
        Ok(())
    }

    fn send_control(&mut self, opcode: u8, payload: Vec<u8>) -> Result<(), WebSocketError> {
        if payload.len() > 125 {
            return Err(WebSocketError::new(
                WebSocketErrorKind::PayloadTooLarge,
                "WebSocket control payload exceeds 125 bytes",
            ));
        }
        self.write_frame(WebSocketFrame {
            fin: true,
            rsv1: false,
            opcode,
            masked: false,
            payload,
        })
    }

    fn write_frame(&mut self, frame: WebSocketFrame) -> Result<(), WebSocketError> {
        let encoded = encode_frame(&frame, self.role)?;
        self.stream.write_all(&encoded)?;
        self.stream.flush()?;
        Ok(())
    }

    fn read_frame(&mut self) -> Result<WebSocketFrame, WebSocketError> {
        let header = self.read_exact_buffered(2)?;
        let first = header[0];
        let second = header[1];
        if first & 0x30 != 0 {
            return Err(WebSocketError::new(
                WebSocketErrorKind::Protocol,
                "RSV2 and RSV3 are reserved",
            ));
        }
        let fin = first & 0x80 != 0;
        let rsv1 = first & 0x40 != 0;
        let opcode = first & 0x0f;
        let masked = second & 0x80 != 0;
        let expected_mask = matches!(self.role, FrameRole::Server);
        if masked != expected_mask {
            return Err(WebSocketError::new(
                WebSocketErrorKind::Protocol,
                if expected_mask {
                    "client-to-server WebSocket frames must be masked"
                } else {
                    "server-to-client WebSocket frames must not be masked"
                },
            ));
        }
        if rsv1 && (!self.per_message_deflate || !matches!(opcode, 0x1 | 0x2)) {
            return Err(WebSocketError::new(
                WebSocketErrorKind::Protocol,
                "RSV1 is only valid on negotiated data frames",
            ));
        }
        let length_marker = second & 0x7f;
        let length = match length_marker {
            value @ 0..=125 => value as u64,
            126 => u16::from_be_bytes(self.read_exact_buffered(2)?.try_into().unwrap()) as u64,
            127 => {
                let value = u64::from_be_bytes(self.read_exact_buffered(8)?.try_into().unwrap());
                if value & (1 << 63) != 0 {
                    return Err(WebSocketError::new(
                        WebSocketErrorKind::Protocol,
                        "WebSocket payload length has the high bit set",
                    ));
                }
                value
            }
            _ => unreachable!(),
        };
        let is_control = opcode & 0x8 != 0;
        if is_control && (!fin || length > 125) {
            return Err(WebSocketError::new(
                WebSocketErrorKind::Protocol,
                "control frames must be final and at most 125 bytes",
            ));
        }
        let length = usize::try_from(length).map_err(|_| {
            WebSocketError::new(
                WebSocketErrorKind::PayloadTooLarge,
                "WebSocket payload length does not fit the host",
            )
        })?;
        if length > self.config.max_frame_bytes || length > self.config.max_message_bytes {
            return Err(WebSocketError::new(
                WebSocketErrorKind::PayloadTooLarge,
                "WebSocket frame exceeds the configured limit",
            ));
        }
        let mask: Option<[u8; 4]> = if masked {
            Some(self.read_exact_buffered(4)?.try_into().unwrap())
        } else {
            None
        };
        let mut payload = self.read_exact_buffered(length)?;
        if let Some(mask) = mask {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % 4];
            }
        }
        Ok(WebSocketFrame {
            fin,
            rsv1,
            opcode,
            masked,
            payload,
        })
    }

    fn read_exact_buffered(&mut self, length: usize) -> Result<Vec<u8>, WebSocketError> {
        while self.buffered.len() < length {
            let mut chunk = [0_u8; 8192];
            let read = self.stream.read(&mut chunk)?;
            if read == 0 {
                return Err(WebSocketError::new(
                    WebSocketErrorKind::Closed,
                    "WebSocket peer closed the connection",
                ));
            }
            self.buffered.extend_from_slice(&chunk[..read]);
            if self.buffered.len() > self.config.max_frame_bytes.saturating_add(14) {
                return Err(WebSocketError::new(
                    WebSocketErrorKind::PayloadTooLarge,
                    "buffered WebSocket frame exceeds the configured limit",
                ));
            }
        }
        Ok(self.buffered.drain(..length).collect())
    }
}

/// State shared by a WebSocket route attached to the HTTP server.  The
/// connection queue is deliberately separate from the dedicated listener:
/// both surfaces expose the same `WebSocketServer`/`server_accept` contract,
/// while only the HTTP server owns the listening socket for a routed upgrade.
pub(crate) struct RoutedUpgradeState {
    config: Mutex<WebSocketConfig>,
    pending: Mutex<VecDeque<WebSocketConnection>>,
    ready: Condvar,
    closed: AtomicBool,
}

impl RoutedUpgradeState {
    fn new(config: WebSocketConfig) -> Self {
        Self {
            config: Mutex::new(config),
            pending: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
            closed: AtomicBool::new(false),
        }
    }

    pub(crate) fn config(&self) -> WebSocketConfig {
        self.config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn update_config(&self, config: WebSocketConfig) {
        *self
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = config;
    }

    fn enqueue(&self, connection: WebSocketConnection) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push_back(connection);
        self.ready.notify_one();
    }

    fn accept(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<WebSocketConnection, WebSocketError> {
        loop {
            if cancellation.load(Ordering::Acquire) {
                return Err(WebSocketError::new(
                    WebSocketErrorKind::Cancelled,
                    "routed WebSocket accept was cancelled",
                ));
            }
            if self.closed.load(Ordering::Acquire) {
                return Err(WebSocketError::new(
                    WebSocketErrorKind::Closed,
                    "routed WebSocket server is closed",
                ));
            }
            let mut pending = self
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(connection) = pending.pop_front() {
                return Ok(connection);
            }
            let (guard, _) = self
                .ready
                .wait_timeout(pending, Duration::from_millis(25))
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            drop(guard);
        }
    }
}

struct RoutedUpgradeJob {
    stream: TcpStream,
    request: ParsedRequest,
    buffered: Vec<u8>,
    state: Arc<RoutedUpgradeState>,
}

fn routed_upgrade_workers() -> &'static SyncSender<RoutedUpgradeJob> {
    static WORKERS: OnceLock<SyncSender<RoutedUpgradeJob>> = OnceLock::new();
    WORKERS.get_or_init(|| {
        // Handshake work is bounded and short, but it may still block on a
        // slow peer while the 101 response is written.  Keep it off mio's
        // reactor and cap the worker count so a connection flood cannot
        // create an unbounded number of operating-system threads.
        let (sender, receiver) = mpsc::sync_channel::<RoutedUpgradeJob>(16_384);
        let receiver = Arc::new(Mutex::new(receiver));
        for index in 0..4 {
            let receiver = Arc::clone(&receiver);
            thread::Builder::new()
                .name(format!("spectra-websocket-upgrade-{index}"))
                .spawn(move || loop {
                    let job = receiver
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .recv();
                    let Ok(job) = job else {
                        break;
                    };
                    let config = job.state.config();
                    let _ = job.stream.set_nonblocking(false);
                    let _ = job.stream.set_read_timeout(Some(config.read_timeout));
                    let _ = job.stream.set_write_timeout(Some(config.write_timeout));
                    let Ok(peer) = job.stream.peer_addr() else {
                        continue;
                    };
                    if let Ok(connection) = accept_handshake_request(
                        job.stream,
                        peer,
                        config,
                        job.request,
                        job.buffered,
                    ) {
                        job.state.enqueue(connection);
                    }
                })
                .expect("routed WebSocket upgrade worker");
        }
        sender
    })
}

/// Queues a parsed HTTP upgrade for the bounded handshake workers.
pub(crate) fn enqueue_routed_upgrade(
    stream: TcpStream,
    request: ParsedRequest,
    buffered: Vec<u8>,
    state: Arc<RoutedUpgradeState>,
) -> Result<(), WebSocketError> {
    match routed_upgrade_workers().try_send(RoutedUpgradeJob {
        stream,
        request,
        buffered,
        state,
    }) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(_)) => Err(WebSocketError::new(
            WebSocketErrorKind::Io,
            "WebSocket upgrade queue is full",
        )),
        Err(TrySendError::Disconnected(_)) => Err(WebSocketError::new(
            WebSocketErrorKind::Io,
            "WebSocket upgrade workers are unavailable",
        )),
    }
}

/// Bidirectional async pump for WebSocket upgrades served by the TLS
/// gateway's HTTP/1.1 leg.
///
/// Design: the proven synchronous frame machine (`read_frame`,
/// `receive_message`) is bound to a blocking `std::io` transport, and a
/// tokio-rustls server stream cannot be borrowed as one without parking a
/// thread. Wrapping every frame read in `spawn_blocking` would keep one
/// blocking thread alive per upgraded connection and still need shared
/// mutable state between the blocking and async halves. The lower-risk port
/// chosen here keeps the pump fully async (`read_exact`/`write_all` over the
/// TLS stream) while reusing the machine's pure building blocks verbatim:
/// [`encode_frame`], [`MessageAssembler`], [`validate_close_payload`], and
/// byte-identical frame-header validation (mask requirement for client
/// frames, RSV rules, control-frame limits, configured size caps).
///
/// Semantics mirror `receive_message`: ping frames are answered with pongs,
/// pong frames are ignored, completed messages are echoed back unmasked and
/// uncompressed, and any protocol or limit violation terminates the
/// connection exactly as the synchronous path drops it.
pub(crate) async fn serve_tls_upgrade_pump<I>(
    mut io: I,
    negotiation: RoutedUpgradeNegotiation,
    config: WebSocketConfig,
    buffered: Vec<u8>,
) where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;

    let mut buffered = buffered;
    let mut assembler = MessageAssembler::new();
    if io.write_all(negotiation.response.as_bytes()).await.is_err() {
        return;
    }
    loop {
        let Some(frame) = tls_read_frame(
            &mut io,
            &mut buffered,
            &config,
            negotiation.per_message_deflate,
        )
        .await
        else {
            return;
        };
        match frame.opcode {
            0x8 => {
                if validate_close_payload(&frame.payload).is_err() {
                    return;
                }
                let echo = WebSocketFrame {
                    fin: true,
                    rsv1: false,
                    opcode: 0x8,
                    masked: false,
                    payload: frame.payload,
                };
                if tls_write_frame(&mut io, echo).await.is_err() {
                    return;
                }
                return;
            }
            0x9 => {
                let pong = WebSocketFrame {
                    fin: true,
                    rsv1: false,
                    opcode: 0xA,
                    masked: false,
                    payload: frame.payload,
                };
                if tls_write_frame(&mut io, pong).await.is_err() {
                    return;
                }
            }
            0xA => {}
            _ => {
                let assembled = assembler.assemble(frame, &config, negotiation.per_message_deflate);
                match assembled {
                    Ok(Some(message)) => {
                        let (opcode, payload) = match &message {
                            WebSocketMessage::Text(value) => (0x1_u8, value.as_bytes().to_vec()),
                            WebSocketMessage::Binary(payload) => (0x2_u8, payload.clone()),
                        };
                        let echo = WebSocketFrame {
                            fin: true,
                            rsv1: false,
                            opcode,
                            masked: false,
                            payload,
                        };
                        if tls_write_frame(&mut io, echo).await.is_err() {
                            return;
                        }
                    }
                    Ok(None) => {}
                    Err(_) => return,
                }
            }
        }
    }
}

/// Async counterpart of `WebSocketConnection::read_exact_buffered`.
async fn tls_read_exact<I>(
    io: &mut I,
    buffered: &mut Vec<u8>,
    length: usize,
    max_buffered: usize,
) -> Option<Vec<u8>>
where
    I: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;

    while buffered.len() < length {
        let mut chunk = [0_u8; 8_192];
        let read = io.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffered.extend_from_slice(&chunk[..read]);
        if buffered.len() > max_buffered {
            return None;
        }
    }
    Some(buffered.drain(..length).collect())
}

/// Async counterpart of `WebSocketConnection::read_frame` with identical
/// validation; any violation returns `None`, which ends the connection.
async fn tls_read_frame<I>(
    io: &mut I,
    buffered: &mut Vec<u8>,
    config: &WebSocketConfig,
    per_message_deflate: bool,
) -> Option<WebSocketFrame>
where
    I: tokio::io::AsyncRead + Unpin,
{
    let max_buffered = config.max_frame_bytes.saturating_add(14);
    let header = tls_read_exact(io, buffered, 2, max_buffered).await?;
    let first = header[0];
    let second = header[1];
    if first & 0x30 != 0 {
        return None;
    }
    let fin = first & 0x80 != 0;
    let rsv1 = first & 0x40 != 0;
    let opcode = first & 0x0f;
    // The TLS gateway always pumps the server side of an upgraded
    // connection, so client-to-server frames must be masked.
    let masked = second & 0x80 != 0;
    if !masked {
        return None;
    }
    if rsv1 && (!per_message_deflate || !matches!(opcode, 0x1 | 0x2)) {
        return None;
    }
    let length_marker = second & 0x7f;
    let length = match length_marker {
        value @ 0..=125 => value as u64,
        126 => {
            let bytes = tls_read_exact(io, buffered, 2, max_buffered).await?;
            u16::from_be_bytes(bytes.try_into().ok()?) as u64
        }
        127 => {
            let bytes = tls_read_exact(io, buffered, 8, max_buffered).await?;
            let value = u64::from_be_bytes(bytes.try_into().ok()?);
            if value & (1 << 63) != 0 {
                return None;
            }
            value
        }
        _ => return None,
    };
    let is_control = opcode & 0x8 != 0;
    if is_control && (!fin || length > 125) {
        return None;
    }
    let length = usize::try_from(length).ok()?;
    if length > config.max_frame_bytes || length > config.max_message_bytes {
        return None;
    }
    let mask_bytes = tls_read_exact(io, buffered, 4, max_buffered).await?;
    let mask: [u8; 4] = mask_bytes.try_into().ok()?;
    let mut payload = tls_read_exact(io, buffered, length, max_buffered).await?;
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[index % 4];
    }
    Some(WebSocketFrame {
        fin,
        rsv1,
        opcode,
        masked,
        payload,
    })
}

async fn tls_write_frame<I>(io: &mut I, frame: WebSocketFrame) -> Result<(), ()>
where
    I: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;

    let encoded = encode_frame(&frame, FrameRole::Server).map_err(|_| ())?;
    io.write_all(&encoded).await.map_err(|_| ())?;
    io.flush().await.map_err(|_| ())
}

pub struct WebSocketServer {
    listener: Option<TcpListener>,
    config: WebSocketConfig,
    routed: bool,
    routed_state: Arc<RoutedUpgradeState>,
}

impl fmt::Debug for WebSocketServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebSocketServer")
            .field("local_addr", &self.local_addr().ok())
            .field("config", &self.config)
            .finish()
    }
}

impl WebSocketServer {
    pub fn new() -> Self {
        let config = WebSocketConfig::default();
        Self {
            listener: None,
            routed: false,
            routed_state: Arc::new(RoutedUpgradeState::new(config.clone())),
            config,
        }
    }

    pub fn listen(&mut self, port: u16) -> Result<(), WebSocketError> {
        if self.listener.is_some() || self.routed {
            return Err(WebSocketError::new(
                WebSocketErrorKind::InvalidArgument,
                "WebSocket server is already listening or attached to an HTTP route",
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
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    "WebSocket server is not listening",
                )
            })?
            .local_addr()
    }

    pub fn set_per_message_deflate(&mut self, enabled: bool) -> Result<(), WebSocketError> {
        if self.listener.is_some() {
            return Err(WebSocketError::new(
                WebSocketErrorKind::InvalidArgument,
                "WebSocket compression must be configured before listen",
            ));
        }
        self.config.per_message_deflate = enabled;
        self.routed_state.update_config(self.config.clone());
        Ok(())
    }

    pub fn set_max_message_bytes(&mut self, max_bytes: usize) -> Result<(), WebSocketError> {
        if max_bytes == 0 || max_bytes > DEFAULT_MAX_MESSAGE_BYTES {
            return Err(WebSocketError::new(
                WebSocketErrorKind::InvalidArgument,
                "WebSocket message limit is outside the supported range",
            ));
        }
        if self.listener.is_some() {
            return Err(WebSocketError::new(
                WebSocketErrorKind::InvalidArgument,
                "WebSocket limits must be configured before listen",
            ));
        }
        self.config.max_message_bytes = max_bytes;
        self.config.max_frame_bytes = self.config.max_frame_bytes.min(max_bytes);
        self.routed_state.update_config(self.config.clone());
        Ok(())
    }

    fn attach_to_http_route(&mut self) -> Result<Arc<RoutedUpgradeState>, WebSocketError> {
        if self.listener.is_some() || self.routed {
            return Err(WebSocketError::new(
                WebSocketErrorKind::InvalidArgument,
                "WebSocket server is already listening or attached to an HTTP route",
            ));
        }
        self.routed = true;
        self.routed_state.update_config(self.config.clone());
        Ok(Arc::clone(&self.routed_state))
    }

    fn detach_from_http_route(&mut self) {
        self.routed = false;
    }

    pub fn accept(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<WebSocketConnection, WebSocketError> {
        let Some(listener) = self.listener.as_ref() else {
            if self.routed {
                return self.routed_state.accept(cancellation);
            }
            return Err(WebSocketError::new(
                WebSocketErrorKind::InvalidArgument,
                "WebSocket server is not listening or attached to an HTTP route",
            ));
        };
        loop {
            if cancellation.load(std::sync::atomic::Ordering::Acquire) {
                return Err(WebSocketError::new(
                    WebSocketErrorKind::Cancelled,
                    "WebSocket accept was cancelled",
                ));
            }
            match listener.accept() {
                Ok((stream, peer)) => {
                    stream.set_nonblocking(false)?;
                    stream.set_nodelay(true)?;
                    stream.set_read_timeout(Some(self.config.read_timeout))?;
                    stream.set_write_timeout(Some(self.config.write_timeout))?;
                    return accept_handshake(stream, peer, self.config.clone());
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl Default for WebSocketServer {
    fn default() -> Self {
        Self::new()
    }
}

pub fn encode_frame(frame: &WebSocketFrame, role: FrameRole) -> Result<Vec<u8>, WebSocketError> {
    validate_frame_for_encoding(frame)?;
    let masked = matches!(role, FrameRole::Client);
    let mut output = Vec::with_capacity(frame.payload.len().saturating_add(14));
    let mut first = frame.opcode & 0x0f;
    if frame.fin {
        first |= 0x80;
    }
    if frame.rsv1 {
        first |= 0x40;
    }
    output.push(first);
    let length = frame.payload.len();
    let mask_bit = if masked { 0x80 } else { 0 };
    match length {
        0..=125 => output.push(mask_bit | length as u8),
        126..=65_535 => {
            output.push(mask_bit | 126);
            output.extend_from_slice(&(length as u16).to_be_bytes());
        }
        _ => {
            output.push(mask_bit | 127);
            output.extend_from_slice(&(length as u64).to_be_bytes());
        }
    }
    if masked {
        let mut key = [0_u8; 4];
        SystemRandom::new().fill(&mut key).map_err(|_| {
            WebSocketError::new(WebSocketErrorKind::Io, "failed to generate WebSocket mask")
        })?;
        output.extend_from_slice(&key);
        output.extend(
            frame
                .payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ key[index % 4]),
        );
    } else {
        output.extend_from_slice(&frame.payload);
    }
    Ok(output)
}

fn validate_frame_for_encoding(frame: &WebSocketFrame) -> Result<(), WebSocketError> {
    if frame.opcode > 0xA
        || frame.opcode == 0x3
        || frame.opcode == 0x4
        || frame.opcode == 0x5
        || frame.opcode == 0x6
        || frame.opcode == 0x7
    {
        return Err(WebSocketError::new(
            WebSocketErrorKind::Protocol,
            "reserved WebSocket opcode cannot be encoded",
        ));
    }
    let control = frame.opcode & 0x8 != 0;
    if control && (!frame.fin || frame.rsv1 || frame.payload.len() > 125) {
        return Err(WebSocketError::new(
            WebSocketErrorKind::Protocol,
            "control frames must be final, uncompressed, and at most 125 bytes",
        ));
    }
    if frame.opcode == 0x0 && frame.rsv1 {
        return Err(WebSocketError::new(
            WebSocketErrorKind::Protocol,
            "continuation frames must not set RSV1",
        ));
    }
    Ok(())
}

fn append_with_limit(
    destination: &mut Vec<u8>,
    source: &[u8],
    limit: usize,
) -> Result<(), WebSocketError> {
    if source.len() > limit.saturating_sub(destination.len()) {
        return Err(WebSocketError::new(
            WebSocketErrorKind::PayloadTooLarge,
            "fragmented WebSocket message exceeds the configured limit",
        ));
    }
    destination.extend_from_slice(source);
    Ok(())
}

fn compress_message(payload: &[u8]) -> Result<Vec<u8>, WebSocketError> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(payload)
        .map_err(|error| WebSocketError::new(WebSocketErrorKind::Compression, error.to_string()))?;
    let mut encoded = encoder
        .finish()
        .map_err(|error| WebSocketError::new(WebSocketErrorKind::Compression, error.to_string()))?;
    if encoded.ends_with(&[0, 0, 0xff, 0xff]) {
        encoded.truncate(encoded.len() - 4);
    }
    Ok(encoded)
}

fn decompress_message(payload: &[u8], limit: usize) -> Result<Vec<u8>, WebSocketError> {
    let mut input = payload.to_vec();
    input.extend_from_slice(&[0, 0, 0xff, 0xff]);
    let mut decoder = DeflateDecoder::new(input.as_slice());
    let mut decoded = Vec::new();
    decoder
        .by_ref()
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut decoded)
        .map_err(|error| WebSocketError::new(WebSocketErrorKind::Compression, error.to_string()))?;
    if decoded.len() > limit {
        return Err(WebSocketError::new(
            WebSocketErrorKind::PayloadTooLarge,
            "decompressed WebSocket message exceeds the configured limit",
        ));
    }
    Ok(decoded)
}

fn accept_handshake(
    mut stream: TcpStream,
    peer: SocketAddr,
    config: WebSocketConfig,
) -> Result<WebSocketConnection, WebSocketError> {
    let mut buffered = Vec::new();
    let header_end = read_http_headers(&mut stream, &mut buffered, DEFAULT_HANDSHAKE_BYTES)?;
    let request = crate::http::parse_request(&buffered[..header_end]).map_err(|error| {
        WebSocketError::new(
            WebSocketErrorKind::Handshake,
            format!("invalid WebSocket HTTP handshake: {error}"),
        )
    })?;
    let remaining = buffered.split_off(header_end);
    accept_handshake_request(stream, peer, config, request, remaining)
}

fn accept_handshake_request(
    mut stream: TcpStream,
    peer: SocketAddr,
    config: WebSocketConfig,
    request: ParsedRequest,
    buffered: Vec<u8>,
) -> Result<WebSocketConnection, WebSocketError> {
    let negotiation = negotiate_upgrade_response(&request, &config)?;
    stream.write_all(negotiation.response.as_bytes())?;
    stream.flush()?;
    Ok(WebSocketConnection {
        stream: WebSocketTransport::Tcp(stream),
        peer,
        config,
        role: FrameRole::Server,
        per_message_deflate: negotiation.per_message_deflate,
        buffered,
        assembler: MessageAssembler::new(),
        closed: false,
    })
}

/// Negotiated WebSocket upgrade outcome, decoupled from any socket so both
/// the cleartext upgrade workers and the TLS gateway's async pump can write
/// the identical 101 response through their own transport.
pub(crate) struct RoutedUpgradeNegotiation {
    /// Complete `HTTP/1.1 101` response head, terminated by a blank line.
    pub(crate) response: String,
    pub(crate) per_message_deflate: bool,
}

/// Validates an upgrade request and renders the 101 response without
/// touching I/O. Mirrors [`accept_handshake_request`] byte for byte.
pub(crate) fn negotiate_upgrade_response(
    request: &ParsedRequest,
    config: &WebSocketConfig,
) -> Result<RoutedUpgradeNegotiation, WebSocketError> {
    if request.method != "GET" || request.version.major != 1 || request.version.minor != 1 {
        return Err(WebSocketError::new(
            WebSocketErrorKind::Handshake,
            "WebSocket handshake requires GET over HTTP/1.1",
        ));
    }
    if !header_contains_token(&request.headers, "Upgrade", "websocket")
        || !header_contains_token(&request.headers, "Connection", "upgrade")
    {
        return Err(WebSocketError::new(
            WebSocketErrorKind::Handshake,
            "WebSocket handshake requires Upgrade and Connection tokens",
        ));
    }
    let key = header_value(&request.headers, "Sec-WebSocket-Key").ok_or_else(|| {
        WebSocketError::new(
            WebSocketErrorKind::Handshake,
            "Sec-WebSocket-Key header is missing",
        )
    })?;
    let decoded_key = base64_decode(key.trim()).ok_or_else(|| {
        WebSocketError::new(
            WebSocketErrorKind::Handshake,
            "Sec-WebSocket-Key is not valid base64",
        )
    })?;
    if decoded_key.len() != 16 {
        return Err(WebSocketError::new(
            WebSocketErrorKind::Handshake,
            "Sec-WebSocket-Key must decode to 16 bytes",
        ));
    }
    if header_value(&request.headers, "Sec-WebSocket-Version").map(str::trim) != Some("13") {
        return Err(WebSocketError::new(
            WebSocketErrorKind::Handshake,
            "only WebSocket version 13 is supported",
        ));
    }
    let requested_extension = header_value(&request.headers, "Sec-WebSocket-Extensions")
        .is_some_and(|value| extension_contains(value, "permessage-deflate"));
    let per_message_deflate = config.per_message_deflate && requested_extension;
    let protocol = negotiate_subprotocol(&request.headers, &config.subprotocols);
    let mut response = String::from(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n",
    );
    response.push_str("Sec-WebSocket-Accept: ");
    response.push_str(&websocket_accept_value(key.trim()));
    response.push_str("\r\n");
    if let Some(protocol) = protocol {
        response.push_str("Sec-WebSocket-Protocol: ");
        response.push_str(protocol);
        response.push_str("\r\n");
    }
    if per_message_deflate {
        response.push_str(
            "Sec-WebSocket-Extensions: permessage-deflate; server_no_context_takeover; client_no_context_takeover\r\n",
        );
    }
    response.push_str("\r\n");
    Ok(RoutedUpgradeNegotiation {
        response,
        per_message_deflate,
    })
}

pub(crate) fn is_upgrade_request(request: &ParsedRequest) -> bool {
    request.method == "GET"
        && request.version.major == 1
        && request.version.minor == 1
        && header_contains_token(&request.headers, "Upgrade", "websocket")
        && header_contains_token(&request.headers, "Connection", "upgrade")
}

fn client_handshake(
    mut stream: WebSocketTransport,
    url: &ParsedWebSocketUrl,
    config: &WebSocketConfig,
) -> Result<WebSocketConnection, WebSocketError> {
    let mut nonce = [0_u8; 16];
    SystemRandom::new().fill(&mut nonce).map_err(|_| {
        WebSocketError::new(
            WebSocketErrorKind::Io,
            "failed to generate WebSocket handshake nonce",
        )
    })?;
    let key = base64_encode(&nonce);
    let mut request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {}\r\nSec-WebSocket-Version: 13\r\n",
        url.path, url.host_header, key
    );
    if config.per_message_deflate {
        request.push_str("Sec-WebSocket-Extensions: permessage-deflate\r\n");
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let mut buffered = Vec::new();
    let header_end = read_http_headers(&mut stream, &mut buffered, DEFAULT_HANDSHAKE_BYTES)?;
    let response = crate::http::parse_response(&buffered[..header_end]).map_err(|error| {
        WebSocketError::new(
            WebSocketErrorKind::Handshake,
            format!("invalid WebSocket HTTP response: {error}"),
        )
    })?;
    if response.status_code != 101
        || response.version.major != 1
        || response.version.minor != 1
        || !header_contains_token(&response.headers, "Upgrade", "websocket")
        || !header_contains_token(&response.headers, "Connection", "upgrade")
    {
        return Err(WebSocketError::new(
            WebSocketErrorKind::Handshake,
            "WebSocket server did not return a valid 101 upgrade",
        ));
    }
    let expected_accept = websocket_accept_value(&key);
    if header_value(&response.headers, "Sec-WebSocket-Accept").map(str::trim)
        != Some(expected_accept.as_str())
    {
        return Err(WebSocketError::new(
            WebSocketErrorKind::Handshake,
            "WebSocket server returned an invalid Sec-WebSocket-Accept",
        ));
    }
    let negotiated_per_message_deflate =
        header_value(&response.headers, "Sec-WebSocket-Extensions")
            .is_some_and(|value| extension_contains(value, "permessage-deflate"));
    if negotiated_per_message_deflate && !config.per_message_deflate {
        return Err(WebSocketError::new(
            WebSocketErrorKind::Handshake,
            "WebSocket server negotiated an extension the client did not request",
        ));
    }
    let peer = stream.peer_addr()?;
    let remaining = buffered.split_off(header_end);
    Ok(WebSocketConnection {
        stream,
        peer,
        config: config.clone(),
        role: FrameRole::Client,
        per_message_deflate: negotiated_per_message_deflate,
        buffered: remaining,
        assembler: MessageAssembler::new(),
        closed: false,
    })
}

fn parse_websocket_url(url: &str) -> Result<ParsedWebSocketUrl, WebSocketError> {
    let (tls, rest) = if let Some(rest) = url.strip_prefix("ws://") {
        (false, rest)
    } else if let Some(rest) = url.strip_prefix("wss://") {
        (true, rest)
    } else {
        return Err(WebSocketError::new(
            WebSocketErrorKind::UnsupportedScheme,
            "WebSocket client supports ws:// and wss:// URLs",
        ));
    };
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    if authority.is_empty()
        || authority.contains('@')
        || authority.bytes().any(|byte| byte.is_ascii_whitespace())
        || path.bytes().any(|byte| matches!(byte, b'\r' | b'\n'))
        || path.contains('#')
    {
        return Err(WebSocketError::new(
            WebSocketErrorKind::InvalidArgument,
            "WebSocket URL authority or path is invalid",
        ));
    }
    let (host, port) = parse_websocket_authority(authority, if tls { 443 } else { 80 })?;
    let host_header = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    Ok(ParsedWebSocketUrl {
        host,
        port,
        host_header,
        path: if path.is_empty() {
            "/".to_string()
        } else {
            path.to_string()
        },
        tls,
    })
}

fn parse_websocket_authority(
    authority: &str,
    default_port: u16,
) -> Result<(String, u16), WebSocketError> {
    if authority.starts_with('[') {
        let closing = authority.find(']').ok_or_else(|| {
            WebSocketError::new(
                WebSocketErrorKind::InvalidArgument,
                "WebSocket IPv6 authority is missing ]",
            )
        })?;
        let host = &authority[1..closing];
        if host.is_empty() {
            return Err(WebSocketError::new(
                WebSocketErrorKind::InvalidArgument,
                "WebSocket host is empty",
            ));
        }
        let suffix = &authority[closing + 1..];
        let port = if suffix.is_empty() {
            default_port
        } else if let Some(port) = suffix.strip_prefix(':') {
            parse_websocket_port(port)?
        } else {
            return Err(WebSocketError::new(
                WebSocketErrorKind::InvalidArgument,
                "WebSocket IPv6 authority has an invalid port",
            ));
        };
        return Ok((host.to_string(), port));
    }
    if authority.matches(':').count() > 1 {
        return Err(WebSocketError::new(
            WebSocketErrorKind::InvalidArgument,
            "IPv6 WebSocket literals must be enclosed in brackets",
        ));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => {
            if host.is_empty() {
                return Err(WebSocketError::new(
                    WebSocketErrorKind::InvalidArgument,
                    "WebSocket host is empty",
                ));
            }
            Ok((host.to_string(), parse_websocket_port(port)?))
        }
        None => Ok((authority.to_string(), default_port)),
    }
}

fn parse_websocket_port(port: &str) -> Result<u16, WebSocketError> {
    if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(WebSocketError::new(
            WebSocketErrorKind::InvalidArgument,
            "WebSocket port is invalid",
        ));
    }
    port.parse::<u16>().map_err(|_| {
        WebSocketError::new(
            WebSocketErrorKind::InvalidArgument,
            "WebSocket port is out of range",
        )
    })
}

fn sleep_with_cancellation(
    duration: Duration,
    cancellation: &CancellationToken,
) -> Result<(), WebSocketError> {
    let mut remaining = duration;
    while !remaining.is_zero() {
        if cancellation.load(std::sync::atomic::Ordering::Acquire) {
            return Err(WebSocketError::new(
                WebSocketErrorKind::Cancelled,
                "WebSocket reconnect was cancelled",
            ));
        }
        let step = remaining.min(Duration::from_millis(10));
        thread::sleep(step);
        remaining = remaining.saturating_sub(step);
    }
    Ok(())
}

fn read_http_headers<R: Read>(
    stream: &mut R,
    buffered: &mut Vec<u8>,
    limit: usize,
) -> Result<usize, WebSocketError> {
    loop {
        if let Some(index) = buffered.windows(4).position(|window| window == b"\r\n\r\n") {
            return Ok(index + 4);
        }
        if buffered.len() >= limit {
            return Err(WebSocketError::new(
                WebSocketErrorKind::PayloadTooLarge,
                "WebSocket handshake headers exceed the configured limit",
            ));
        }
        let mut chunk = [0_u8; 4096];
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(WebSocketError::new(
                WebSocketErrorKind::Closed,
                "peer closed before the WebSocket handshake completed",
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

fn header_contains_token(headers: &[crate::http::Header], name: &str, token: &str) -> bool {
    header_value(headers, name).is_some_and(|value| {
        value
            .split(',')
            .any(|part| part.trim().eq_ignore_ascii_case(token))
    })
}

fn extension_contains(value: &str, extension: &str) -> bool {
    value
        .split(',')
        .filter_map(|entry| entry.trim().split(';').next())
        .any(|name| name.eq_ignore_ascii_case(extension))
}

fn negotiate_subprotocol<'a>(
    headers: &'a [crate::http::Header],
    supported: &[String],
) -> Option<&'a str> {
    let requested = header_value(headers, "Sec-WebSocket-Protocol")?;
    requested
        .split(',')
        .map(str::trim)
        .find(|candidate| supported.iter().any(|item| item == candidate))
}

fn validate_close_code(code: u16) -> Result<(), WebSocketError> {
    if !(1000..=4999).contains(&code) || matches!(code, 1004 | 1005 | 1006 | 1015) {
        return Err(WebSocketError::new(
            WebSocketErrorKind::Protocol,
            "invalid WebSocket close code",
        ));
    }
    Ok(())
}
fn validate_close_payload(payload: &[u8]) -> Result<(), WebSocketError> {
    if payload.len() == 1 {
        return Err(WebSocketError::new(
            WebSocketErrorKind::Protocol,
            "close frame payload cannot contain one byte",
        ));
    }
    if payload.len() >= 2 {
        let code = u16::from_be_bytes([payload[0], payload[1]]);
        validate_close_code(code)?;
        std::str::from_utf8(&payload[2..]).map_err(|_| {
            WebSocketError::new(
                WebSocketErrorKind::Utf8,
                "WebSocket close reason is not valid UTF-8",
            )
        })?;
    }
    Ok(())
}

fn websocket_accept_value(key: &str) -> String {
    let mut input = Vec::with_capacity(key.len() + WEBSOCKET_GUID.len());
    input.extend_from_slice(key.as_bytes());
    input.extend_from_slice(WEBSOCKET_GUID);
    base64_encode(digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, &input).as_ref())
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied();
        let third = chunk.get(2).copied();
        output.push(TABLE[(first >> 2) as usize] as char);
        output.push(TABLE[((first & 0x03) << 4 | second.unwrap_or(0) >> 4) as usize] as char);
        output.push(match second {
            Some(second) => {
                TABLE[((second & 0x0f) << 2 | third.unwrap_or(0) >> 6) as usize] as char
            }
            None => '=',
        });
        output.push(match third {
            Some(third) => TABLE[(third & 0x3f) as usize] as char,
            None => '=',
        });
    }
    output
}

fn base64_decode(value: &str) -> Option<Vec<u8>> {
    if value.is_empty() || !value.len().is_multiple_of(4) {
        return None;
    }
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(value.len() / 4 * 3);
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let final_chunk = index + 1 == bytes.len() / 4;
        let a = base64_digit(chunk[0])?;
        let b = base64_digit(chunk[1])?;
        let c = if chunk[2] == b'=' {
            if !final_chunk || chunk[3] != b'=' {
                return None;
            }
            if b & 0x0f != 0 {
                return None;
            }
            0
        } else {
            base64_digit(chunk[2])?
        };
        let d = if chunk[3] == b'=' {
            if !final_chunk {
                return None;
            }
            if c & 0x03 != 0 {
                return None;
            }
            0
        } else {
            base64_digit(chunk[3])?
        };
        output.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            output.push((b << 4) | (c >> 2));
        }
        if chunk[3] != b'=' {
            output.push((c << 6) | d);
        }
    }
    Some(output)
}

fn base64_digit(value: u8) -> Option<u8> {
    Some(match value {
        b'A'..=b'Z' => value - b'A',
        b'a'..=b'z' => value - b'a' + 26,
        b'0'..=b'9' => value - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    })
}

struct WebSocketStore {
    clients: ApiHandleTable<Arc<Mutex<WebSocketClient>>>,
    servers: ApiHandleTable<Arc<Mutex<WebSocketServer>>>,
    connections: ApiHandleTable<Arc<Mutex<WebSocketConnection>>>,
    messages: ApiHandleTable<WebSocketMessage>,
    routed: HashMap<SpectraHostValue, Arc<RoutedUpgradeState>>,
}

impl WebSocketStore {
    fn new() -> Self {
        Self {
            clients: ApiHandleTable::new(HandleKind::ApiWebSocketClient),
            servers: ApiHandleTable::new(HandleKind::ApiWebSocketServer),
            connections: ApiHandleTable::new(HandleKind::ApiWebSocket),
            messages: ApiHandleTable::new(HandleKind::ApiWebSocketMessage),
            routed: HashMap::new(),
        }
    }
}

fn store() -> &'static Mutex<WebSocketStore> {
    static STORE: OnceLock<Mutex<WebSocketStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(WebSocketStore::new()))
}

pub(crate) fn routed_upgrade_for_route(
    route_id: SpectraHostValue,
) -> Option<Arc<RoutedUpgradeState>> {
    store()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .routed
        .get(&route_id)
        .cloned()
}

pub(crate) fn register_server_route(
    server: Arc<Mutex<WebSocketServer>>,
    route_id: SpectraHostValue,
) -> Result<(), WebSocketError> {
    if routing::route_method(route_id) != Some(routing::RouteMethod::Get) {
        return Err(WebSocketError::new(
            WebSocketErrorKind::InvalidArgument,
            "WebSocket HTTP upgrades require a GET route",
        ));
    }
    if store()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .routed
        .contains_key(&route_id)
    {
        return Err(WebSocketError::new(
            WebSocketErrorKind::InvalidArgument,
            "HTTP route is already attached to a WebSocket server",
        ));
    }
    let state = server
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .attach_to_http_route()?;
    let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
    if store.routed.contains_key(&route_id) {
        server
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .detach_from_http_route();
        return Err(WebSocketError::new(
            WebSocketErrorKind::InvalidArgument,
            "HTTP route is already attached to a WebSocket server",
        ));
    }
    store.routed.insert(route_id, state);
    Ok(())
}

pub extern "C" fn server_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
    write_result(
        ctx,
        store
            .servers
            .insert(Arc::new(Mutex::new(WebSocketServer::new()))),
    )
}

pub extern "C" fn client_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
    write_result(
        ctx,
        store
            .clients
            .insert(Arc::new(Mutex::new(WebSocketClient::new()))),
    )
}

pub extern "C" fn client_set_per_message_deflate(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if !matches!(args[1], 0 | 1) {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(client) = store.clients.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    client
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .set_per_message_deflate(args[1] == 1);
    write_result(ctx, 1)
}

pub extern "C" fn client_set_max_message_bytes(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(limit) = usize::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(client) = store.clients.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let result = client
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .set_max_message_bytes(limit);
    write_result(ctx, i64::from(result.is_ok()))
}

pub extern "C" fn client_set_reconnect(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(attempts) = u32::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(backoff_ms) = u64::try_from(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(client) = store.clients.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let result = client
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .set_reconnect(attempts, Duration::from_millis(backoff_ms));
    write_result(ctx, i64::from(result.is_ok()))
}

pub extern "C" fn client_allow_private_networks(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if !matches!(args[1], 0 | 1) {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(client) = store.clients.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    client
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .allow_private_networks(args[1] == 1);
    write_result(ctx, 1)
}

pub extern "C" fn client_connect(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(url) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let client = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        let Some(client) = store.clients.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(client)
    };
    let task = spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |token| {
        let connection = client
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .connect(&url, &token)
            .map_err(|_| ())?;
        let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
        Ok(store.connections.insert(Arc::new(Mutex::new(connection))))
    });
    match task {
        Ok(task) => write_result(ctx, task),
        Err(_) => HOST_STATUS_INTERNAL_ERROR,
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
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        let Some(server) = store.servers.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(server)
    };
    let result = server
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .listen(port);
    write_result(ctx, i64::from(result.is_ok()))
}

pub extern "C" fn server_route(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let server = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        let Some(server) = store.servers.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(server)
    };
    let result = register_server_route(server, args[1]);
    write_result(ctx, i64::from(result.is_ok()))
}

pub extern "C" fn server_local_port(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let server = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        let Some(server) = store.servers.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(server)
    };
    let port = server
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .local_addr()
        .map(|address| i64::from(address.port()))
        .unwrap_or(0);
    write_result(ctx, port)
}

pub extern "C" fn server_set_per_message_deflate(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if !matches!(args[1], 0 | 1) {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(server) = store.servers.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let result = server
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .set_per_message_deflate(args[1] == 1);
    write_result(ctx, i64::from(result.is_ok()))
}

pub extern "C" fn server_set_max_message_bytes(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(limit) = usize::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(server) = store.servers.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let result = server
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .set_max_message_bytes(limit);
    write_result(ctx, i64::from(result.is_ok()))
}

pub extern "C" fn server_accept(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let server = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        let Some(server) = store.servers.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(server)
    };
    let task = spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |token| {
        let connection = server
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .accept(&token)
            .map_err(|_| ())?;
        let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
        Ok(store.connections.insert(Arc::new(Mutex::new(connection))))
    });
    match task {
        Ok(task) => write_result(ctx, task),
        Err(_) => HOST_STATUS_INTERNAL_ERROR,
    }
}

pub extern "C" fn connection_peer_port(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let connection = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        let Some(connection) = store.connections.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(connection)
    };
    let port = connection
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .peer()
        .port() as i64;
    write_result(ctx, port)
}

pub extern "C" fn connection_receive(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let connection = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        let Some(connection) = store.connections.get(&args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(connection)
    };
    let task = spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |_| {
        let message = connection
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .receive_message()
            .map_err(|_| ())?;
        let Some(message) = message else {
            return Ok(0);
        };
        let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
        Ok(store.messages.insert(message))
    });
    match task {
        Ok(task) => write_result(ctx, task),
        Err(_) => HOST_STATUS_INTERNAL_ERROR,
    }
}

fn connection_task(
    ctx: *mut SpectraHostCallContext,
    connection_handle: SpectraHostValue,
    operation: impl FnOnce(&mut WebSocketConnection) -> Result<(), WebSocketError> + Send + 'static,
) -> i32 {
    let connection = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        let Some(connection) = store.connections.get(&connection_handle) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        Arc::clone(connection)
    };
    let task = spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |_| {
        let mut connection = connection.lock().unwrap_or_else(|error| error.into_inner());
        operation(&mut connection).map_err(|_| ())?;
        Ok(1)
    });
    match task {
        Ok(task) => write_result(ctx, task),
        Err(_) => HOST_STATUS_INTERNAL_ERROR,
    }
}

pub extern "C" fn connection_send_text(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(value) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    connection_task(ctx, args[0], move |connection| connection.send_text(&value))
}

pub extern "C" fn connection_send_binary_base64(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(value) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(value) = base64_decode(&value) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    connection_task(ctx, args[0], move |connection| {
        connection.send_binary(value)
    })
}

pub extern "C" fn connection_ping(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(value) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    connection_task(ctx, args[0], move |connection| {
        connection.send_ping(value.into_bytes())
    })
}

pub extern "C" fn connection_close(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(code) = u16::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(reason) = read_spectra_string(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    connection_task(ctx, args[0], move |connection| {
        connection.close(code, &reason)
    })
}

pub extern "C" fn message_kind(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(message) = store.messages.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, message.kind())
}

pub extern "C" fn message_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(message) = store.messages.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, message.payload().len() as i64)
}

pub extern "C" fn message_text(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(WebSocketMessage::Text(value)) = store.messages.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(value))
}

pub extern "C" fn message_base64(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(WebSocketMessage::Binary(value)) = store.messages.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(&base64_encode(value)))
}

pub extern "C" fn message_release(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
    write_result(ctx, i64::from(store.messages.remove(&args[0]).is_some()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::generate_simple_self_signed;
    use rustls::{ServerConnection, StreamOwned};
    use std::env;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;

    fn client_handshake(stream: &mut TcpStream, key: &str, extensions: Option<&str>) {
        let mut request = format!(
            "GET /socket HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n"
        );
        if let Some(extensions) = extensions {
            request.push_str(&format!("Sec-WebSocket-Extensions: {extensions}\r\n"));
        }
        request.push_str("\r\n");
        stream
            .write_all(request.as_bytes())
            .expect("write WebSocket handshake");
        let mut response = Vec::new();
        let mut buf = [0_u8; 1024];
        loop {
            let count = stream.read(&mut buf).expect("read WebSocket handshake");
            response.extend_from_slice(&buf[..count]);
            if response.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let response = String::from_utf8(response).expect("UTF-8 WebSocket handshake");
        assert!(response.starts_with("HTTP/1.1 101 Switching Protocols"));
        assert!(response.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
        if extensions.is_some() {
            assert!(response.contains("permessage-deflate"));
        }
    }

    fn masked_frame(fin: bool, opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
        let frame = WebSocketFrame {
            fin,
            rsv1: false,
            opcode,
            masked: true,
            payload: payload.to_vec(),
        };
        let mut encoded = vec![(if fin { 0x80 } else { 0 }) | opcode];
        let length = payload.len();
        encoded.push(0x80 | length as u8);
        encoded.extend_from_slice(&mask);
        encoded.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
        assert_eq!(frame.payload, payload);
        encoded
    }

    #[test]
    fn handshake_accepts_rfc_example_and_reassembles_fragmented_message() {
        let mut server = WebSocketServer::new();
        server.listen(0).expect("listen WebSocket server");
        let port = server.local_addr().expect("server address").port();
        let cancellation = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel();
        let server = Arc::new(Mutex::new(server));
        let accept_server = Arc::clone(&server);
        let accept_cancel = Arc::clone(&cancellation);
        let thread = std::thread::spawn(move || {
            ready_tx.send(()).expect("signal WebSocket server");
            accept_server
                .lock()
                .expect("server lock")
                .accept(&accept_cancel)
                .expect("WebSocket handshake")
        });
        ready_rx.recv().expect("server ready");
        let mut client = TcpStream::connect(("127.0.0.1", port)).expect("connect WebSocket server");
        client_handshake(&mut client, "dGhlIHNhbXBsZSBub25jZQ==", None);
        client
            .write_all(&masked_frame(false, 0x1, b"Hel", [1, 2, 3, 4]))
            .expect("write first fragment");
        client
            .write_all(&masked_frame(true, 0x0, b"lo", [4, 3, 2, 1]))
            .expect("write final fragment");
        let mut connection = thread.join().expect("WebSocket accept thread");
        assert_eq!(
            connection.receive_message().expect("receive message"),
            Some(WebSocketMessage::Text("Hello".into()))
        );
    }

    #[test]
    fn ping_is_answered_with_pong_and_close_is_validated() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ping listener");
        let address = listener.local_addr().expect("ping address");
        let server = std::thread::spawn(move || {
            let (stream, peer) = listener.accept().expect("accept ping client");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("server read timeout");
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .expect("server write timeout");
            let mut connection =
                accept_handshake(stream, peer, WebSocketConfig::default()).expect("ping handshake");
            connection.receive_message().expect("consume ping")
        });
        let mut client = TcpStream::connect(address).expect("connect ping server");
        client_handshake(&mut client, "dGhlIHNhbXBsZSBub25jZQ==", None);
        client
            .write_all(&masked_frame(true, 0x9, b"hi", [8, 7, 6, 5]))
            .expect("write ping");
        let mut pong_header = [0_u8; 2];
        client
            .read_exact(&mut pong_header)
            .expect("read pong header");
        assert_eq!(pong_header[0], 0x8a);
        assert_eq!(pong_header[1], 2);
        let mut pong_payload = [0_u8; 2];
        client
            .read_exact(&mut pong_payload)
            .expect("read pong payload");
        assert_eq!(&pong_payload, b"hi");
        client
            .write_all(&masked_frame(
                true,
                0x8,
                &1000_u16.to_be_bytes(),
                [5, 6, 7, 8],
            ))
            .expect("write close after ping");
        assert_eq!(server.join().expect("ping server thread"), None);
    }

    #[test]
    fn per_message_deflate_is_negotiated_and_round_trips() {
        let config = WebSocketConfig {
            per_message_deflate: true,
            ..Default::default()
        };
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind compression listener");
        let address = listener.local_addr().expect("compression address");
        let server = std::thread::spawn(move || {
            let (stream, peer) = listener.accept().expect("accept compression client");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("compression read timeout");
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .expect("compression write timeout");
            let mut connection =
                accept_handshake(stream, peer, config).expect("compression handshake");
            connection.receive_message().expect("compressed receive")
        });
        let mut client = TcpStream::connect(address).expect("connect compression server");
        client_handshake(
            &mut client,
            "dGhlIHNhbXBsZSBub25jZQ==",
            Some("permessage-deflate"),
        );
        let payload = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let compressed = compress_message(payload).expect("compress test payload");
        let mut encoded = masked_frame(true, 0x1, &compressed, [9, 8, 7, 6]);
        encoded[0] |= 0x40;
        client.write_all(&encoded).expect("write compressed frame");
        assert_eq!(
            server.join().expect("compression server thread"),
            Some(WebSocketMessage::Text(
                String::from_utf8(payload.to_vec()).unwrap()
            ))
        );
    }

    #[test]
    fn frame_encoder_masks_client_frames_and_rejects_invalid_control_frames() {
        let frame = WebSocketFrame {
            fin: true,
            rsv1: false,
            opcode: 0x1,
            masked: false,
            payload: b"hello".to_vec(),
        };
        let encoded = encode_frame(&frame, FrameRole::Client).expect("client frame encoding");
        assert_ne!(encoded[1] & 0x80, 0);
        assert_eq!(encoded.len(), 2 + 4 + 5);
        let invalid = WebSocketFrame {
            fin: false,
            rsv1: false,
            opcode: 0x9,
            masked: false,
            payload: Vec::new(),
        };
        assert!(encode_frame(&invalid, FrameRole::Server).is_err());
    }

    #[test]
    fn client_handshake_round_trips_text_and_binary_frames() {
        let mut server = WebSocketServer::new();
        server.listen(0).expect("listen client test server");
        let port = server.local_addr().expect("client server address").port();
        let cancellation = Arc::new(AtomicBool::new(false));
        let server_thread = std::thread::spawn(move || {
            let mut connection = server
                .accept(&cancellation)
                .expect("accept client handshake");
            assert_eq!(
                connection.receive_message().expect("receive client text"),
                Some(WebSocketMessage::Text("hello".into()))
            );
            connection.send_text("echo").expect("send text echo");
            assert_eq!(
                connection.receive_message().expect("receive client binary"),
                Some(WebSocketMessage::Binary(vec![1, 2, 3]))
            );
        });

        let mut client = WebSocketClient::new();
        client.allow_private_networks(true);
        let token = Arc::new(AtomicBool::new(false));
        let mut connection = client
            .connect(&format!("ws://127.0.0.1:{port}/socket"), &token)
            .expect("connect WebSocket client");
        connection.send_text("hello").expect("send client text");
        assert_eq!(
            connection.receive_message().expect("receive text echo"),
            Some(WebSocketMessage::Text("echo".into()))
        );
        connection
            .send_binary(vec![1, 2, 3])
            .expect("send client binary");
        server_thread.join().expect("client server thread");
    }

    #[test]
    fn client_wss_handshake_round_trips_with_explicit_trust_root() {
        let certified =
            generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()])
                .expect("self-signed WebSocket certificate");
        let cert_der = certified.cert.der().to_vec();
        let server_config = crate::tls::TlsServerConfig::new(
            vec![cert_der.clone()],
            certified.key_pair.serialize_der(),
        )
        .build()
        .expect("build WebSocket TLS server config");
        let client_config = crate::tls::TlsClientConfig::with_roots(vec![cert_der])
            .build()
            .expect("build WebSocket TLS client config");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind secure WebSocket server");
        let address = listener.local_addr().expect("secure WebSocket address");
        let server_thread = std::thread::spawn(move || {
            let (tcp, peer) = listener.accept().expect("accept secure WebSocket client");
            tcp.set_read_timeout(Some(Duration::from_secs(5)))
                .expect("secure WebSocket read timeout");
            tcp.set_write_timeout(Some(Duration::from_secs(5)))
                .expect("secure WebSocket write timeout");
            let connection = ServerConnection::new(server_config).expect("TLS server connection");
            let mut stream = StreamOwned::new(connection, tcp);
            let mut buffered = Vec::new();
            let header_end = read_http_headers(&mut stream, &mut buffered, DEFAULT_HANDSHAKE_BYTES)
                .expect("read secure WebSocket handshake");
            let request = crate::http::parse_request(&buffered[..header_end])
                .expect("parse secure WebSocket handshake");
            assert_eq!(request.method, "GET");
            assert_eq!(request.target, "/socket");
            let key =
                header_value(&request.headers, "Sec-WebSocket-Key").expect("secure WebSocket key");
            let response = format!(
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
                websocket_accept_value(key.trim())
            );
            stream
                .write_all(response.as_bytes())
                .expect("write secure WebSocket handshake");
            stream.flush().expect("flush secure WebSocket handshake");

            let mut header = [0_u8; 2];
            stream
                .read_exact(&mut header)
                .expect("read secure WebSocket frame header");
            assert_eq!(header[0], 0x81);
            assert_ne!(header[1] & 0x80, 0);
            let length = usize::from(header[1] & 0x7f);
            assert!(length < 126);
            let mut mask = [0_u8; 4];
            stream
                .read_exact(&mut mask)
                .expect("read secure frame mask");
            let mut payload = vec![0_u8; length];
            stream
                .read_exact(&mut payload)
                .expect("read secure frame payload");
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % 4];
            }
            assert_eq!(payload, b"secure");
            let response = encode_frame(
                &WebSocketFrame {
                    fin: true,
                    rsv1: false,
                    opcode: 0x1,
                    masked: false,
                    payload: b"echo-secure".to_vec(),
                },
                FrameRole::Server,
            )
            .expect("encode secure WebSocket response");
            stream.write_all(&response).expect("write secure response");
            stream.flush().expect("flush secure response");
            peer
        });

        let mut client = WebSocketClient::new();
        client.allow_private_networks(true);
        client.set_tls_config(client_config);
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut connection = client
            .connect(
                &format!("wss://127.0.0.1:{}/socket", address.port()),
                &cancellation,
            )
            .expect("connect secure WebSocket client");
        connection.send_text("secure").expect("send secure text");
        assert_eq!(
            connection.receive_message().expect("receive secure echo"),
            Some(WebSocketMessage::Text("echo-secure".into()))
        );
        assert_eq!(
            server_thread
                .join()
                .expect("secure WebSocket server thread")
                .ip(),
            address.ip()
        );
    }

    #[test]
    #[ignore = "requires a known external WebSocket echo server in SPECTRA_WEBSOCKET_EXTERNAL_URL"]
    fn client_external_echo_server_round_trips_text_and_binary() {
        let url = env::var("SPECTRA_WEBSOCKET_EXTERNAL_URL")
            .expect("SPECTRA_WEBSOCKET_EXTERNAL_URL must contain a ws:// or wss:// endpoint");
        let mut client = WebSocketClient::new();
        client.set_per_message_deflate(true);
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut connection = client
            .connect(&url, &cancellation)
            .expect("connect external WebSocket echo server");
        connection
            .send_text("spectralang-external-echo")
            .expect("send external text");
        assert_eq!(
            connection.receive_message().expect("receive external text"),
            Some(WebSocketMessage::Text(
                "spectralang-external-echo".to_string()
            ))
        );
        let binary = b"spectralang-binary-echo".to_vec();
        connection
            .send_binary(binary.clone())
            .expect("send external binary");
        assert_eq!(
            connection
                .receive_message()
                .expect("receive external binary"),
            Some(WebSocketMessage::Binary(binary))
        );
        connection.close(1000, "done").expect("close external echo");
    }

    #[test]
    fn client_reconnects_after_a_failed_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind reconnect listener");
        let address = listener.local_addr().expect("reconnect address");
        let server_thread = std::thread::spawn(move || {
            let (first, _) = listener.accept().expect("accept first reconnect attempt");
            drop(first);
            let (stream, peer) = listener.accept().expect("accept second reconnect attempt");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("reconnect read timeout");
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .expect("reconnect write timeout");
            let _connection = accept_handshake(stream, peer, WebSocketConfig::default())
                .expect("accept retried handshake");
        });

        let mut client = WebSocketClient::new();
        client.allow_private_networks(true);
        client
            .set_reconnect(1, Duration::from_millis(1))
            .expect("configure reconnect");
        let token = Arc::new(AtomicBool::new(false));
        let connection = client
            .connect(&format!("ws://{}", address), &token)
            .expect("reconnect WebSocket client");
        drop(connection);
        server_thread.join().expect("reconnect server thread");
    }
}
