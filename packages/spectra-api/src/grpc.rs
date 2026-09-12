//! gRPC protocol primitives and a small, real HTTP/2 transport.
//!
//! This module intentionally transports opaque protobuf bytes.  It does not
//! require generated protobuf code: applications provide a [`GrpcService`]
//! and encode/decode their own messages.

use bytes::Bytes;
use h2::client;
use h2::server::{self, SendResponse};
use h2::Reason;
use http::{HeaderMap, HeaderName, HeaderValue, Method, Request, Response, StatusCode};
use std::fmt;
use std::future::poll_fn;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};

const DEFAULT_MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;
const DEFAULT_STREAM_CAPACITY: usize = 16;

/// An owned protobuf message. Bytes are deliberately not interpreted as UTF-8.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpcMessage(Vec<u8>);

impl GrpcMessage {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self::new(bytes)
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrpcCompression {
    Identity,
    Compressed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrpcFrameError {
    InvalidCompressedFlag(u8),
    MessageTooLarge { length: usize, max: usize },
    Truncated { needed: usize, available: usize },
}
impl fmt::Display for GrpcFrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCompressedFlag(v) => write!(f, "invalid gRPC compressed flag {v}"),
            Self::MessageTooLarge { length, max } => {
                write!(f, "gRPC message of {length} bytes exceeds limit {max}")
            }
            Self::Truncated { needed, available } => write!(
                f,
                "truncated gRPC envelope: need {needed} bytes, have {available}"
            ),
        }
    }
}
impl std::error::Error for GrpcFrameError {}

/// Incremental parser. A call to `push` may contain arbitrary DATA-frame
/// boundaries and returns every complete message found in the accumulated data.
pub struct GrpcMessageParser {
    buffer: Vec<u8>,
    max_message_size: usize,
}
impl GrpcMessageParser {
    pub fn new(max_message_size: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_message_size,
        }
    }
    pub fn max_message_size(&self) -> usize {
        self.max_message_size
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<GrpcMessage>, GrpcFrameError> {
        self.buffer.extend_from_slice(bytes);
        let mut result = Vec::new();
        loop {
            if self.buffer.len() < 5 {
                break;
            }
            let flag = self.buffer[0];
            if flag != 0 {
                return Err(GrpcFrameError::InvalidCompressedFlag(flag));
            }
            let length = u32::from_be_bytes([
                self.buffer[1],
                self.buffer[2],
                self.buffer[3],
                self.buffer[4],
            ]) as usize;
            if length > self.max_message_size {
                return Err(GrpcFrameError::MessageTooLarge {
                    length,
                    max: self.max_message_size,
                });
            }
            let total = 5usize.saturating_add(length);
            if self.buffer.len() < total {
                break;
            }
            let payload = self.buffer[5..total].to_vec();
            self.buffer.drain(..total);
            result.push(GrpcMessage::new(payload));
        }
        Ok(result)
    }
    pub fn finish(&self) -> Result<(), GrpcFrameError> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let needed = if self.buffer.len() >= 5 {
            5usize.saturating_add(u32::from_be_bytes([
                self.buffer[1],
                self.buffer[2],
                self.buffer[3],
                self.buffer[4],
            ]) as usize)
        } else {
            5
        };
        Err(GrpcFrameError::Truncated {
            needed,
            available: self.buffer.len(),
        })
    }
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}
impl fmt::Debug for GrpcMessageParser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GrpcMessageParser")
            .field("buffered", &self.buffer.len())
            .field("max_message_size", &self.max_message_size)
            .finish()
    }
}
pub type GrpcFrameParser = GrpcMessageParser;

pub fn encode_grpc_message(
    message: &GrpcMessage,
    compression: GrpcCompression,
) -> Result<Vec<u8>, GrpcFrameError> {
    if message.len() > DEFAULT_MAX_MESSAGE_SIZE {
        return Err(GrpcFrameError::MessageTooLarge {
            length: message.len(),
            max: DEFAULT_MAX_MESSAGE_SIZE,
        });
    }
    let mut out = Vec::with_capacity(message.len() + 5);
    out.push(match compression {
        GrpcCompression::Identity => 0,
        GrpcCompression::Compressed => 1,
    });
    out.extend_from_slice(&(message.len() as u32).to_be_bytes());
    out.extend_from_slice(message.as_bytes());
    Ok(out)
}
pub fn encode_grpc_message_with_limit(
    message: &GrpcMessage,
    compression: GrpcCompression,
    max_message_size: usize,
) -> Result<Vec<u8>, GrpcFrameError> {
    if message.len() > max_message_size || message.len() > u32::MAX as usize {
        return Err(GrpcFrameError::MessageTooLarge {
            length: message.len(),
            max: max_message_size,
        });
    }
    let mut out = Vec::with_capacity(message.len() + 5);
    out.push(match compression {
        GrpcCompression::Identity => 0,
        GrpcCompression::Compressed => 1,
    });
    out.extend_from_slice(&(message.len() as u32).to_be_bytes());
    out.extend_from_slice(message.as_bytes());
    Ok(out)
}
pub fn encode_grpc_frame(
    message: &GrpcMessage,
    compression: GrpcCompression,
) -> Result<Vec<u8>, GrpcFrameError> {
    encode_grpc_message(message, compression)
}
pub type GrpcMessageDecoder = GrpcMessageParser;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpcMetadataEntry {
    pub key: String,
    pub value: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GrpcMetadata(Vec<GrpcMetadataEntry>);
impl GrpcMetadata {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: impl Into<Vec<u8>>,
    ) -> Result<(), GrpcError> {
        let key = key.into();
        validate_metadata_key(&key)?;
        self.0.retain(|entry| entry.key != key);
        self.0.push(GrpcMetadataEntry {
            key,
            value: value.into(),
        });
        Ok(())
    }
    pub fn append(
        &mut self,
        key: impl Into<String>,
        value: impl Into<Vec<u8>>,
    ) -> Result<(), GrpcError> {
        let key = key.into();
        validate_metadata_key(&key)?;
        self.0.push(GrpcMetadataEntry {
            key,
            value: value.into(),
        });
        Ok(())
    }
    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.0
            .iter()
            .find(|e| e.key.eq_ignore_ascii_case(key))
            .map(|e| e.value.as_slice())
    }
    pub fn iter(&self) -> impl Iterator<Item = &GrpcMetadataEntry> {
        self.0.iter()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
fn validate_metadata_key(key: &str) -> Result<(), GrpcError> {
    if key.is_empty()
        || key.bytes().any(|b| {
            !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.' || b == b'_')
        })
    {
        return Err(GrpcError::InvalidMetadata(key.to_string()));
    }
    if key.starts_with("grpc-") {
        return Err(GrpcError::ReservedMetadata(key.to_string()));
    }
    Ok(())
}
pub fn validate_reserved_metadata(key: &str) -> Result<(), GrpcError> {
    if key.to_ascii_lowercase().starts_with("grpc-") {
        Err(GrpcError::ReservedMetadata(key.to_string()))
    } else {
        validate_metadata_key(key)
    }
}

pub fn validate_grpc_request(
    path: &str,
    content_type: &str,
    te: Option<&str>,
) -> Result<(), GrpcError> {
    validate_grpc_path(path)?;
    let media = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if media != "application/grpc"
        && media
            .strip_prefix("application/grpc+")
            .is_none_or(|suffix| suffix.is_empty())
    {
        return Err(GrpcError::InvalidContentType(content_type.to_string()));
    }
    if !te
        .map(|v| {
            v.split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("trailers"))
        })
        .unwrap_or(false)
    {
        return Err(GrpcError::InvalidTe);
    }
    Ok(())
}
pub fn validate_grpc_path(path: &str) -> Result<(), GrpcError> {
    if !path.starts_with('/')
        || path[1..].split('/').count() != 2
        || path[1..].split('/').any(|part| {
            part.is_empty()
                || part
                    .bytes()
                    .any(|b| b <= 0x20 || b == b'?' || b == b'#' || b == b'%')
        })
    {
        return Err(GrpcError::InvalidPath(path.to_string()));
    }
    Ok(())
}

pub fn percent_encode_grpc_message(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    for b in message.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(*b, b'-' | b'.' | b'_' | b'~') {
            out.push(*b as char);
        } else {
            out.push('%');
            out.push(hex((*b >> 4) & 0xf));
            out.push(hex(*b & 0xf));
        }
    }
    out
}
fn hex(v: u8) -> char {
    b"0123456789ABCDEF"[v as usize] as char
}
pub fn percent_decode_grpc_message(value: &str) -> Result<String, GrpcError> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(GrpcError::MalformedPercentEncoding);
            }
            let hi = from_hex(bytes[i + 1]).ok_or(GrpcError::MalformedPercentEncoding)?;
            let lo = from_hex(bytes[i + 2]).ok_or(GrpcError::MalformedPercentEncoding)?;
            out.push(hi << 4 | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| GrpcError::InvalidUtf8)
}
fn from_hex(v: u8) -> Option<u8> {
    match v {
        b'0'..=b'9' => Some(v - b'0'),
        b'a'..=b'f' => Some(v - b'a' + 10),
        b'A'..=b'F' => Some(v - b'A' + 10),
        _ => None,
    }
}

pub fn parse_grpc_timeout(value: &str) -> Result<Duration, GrpcError> {
    if value.len() < 2 || value.len() > 9 {
        return Err(GrpcError::InvalidTimeout(value.to_string()));
    }
    let unit = value.as_bytes()[value.len() - 1];
    let digits = &value[..value.len() - 1];
    if digits.is_empty() || digits.bytes().any(|b| !b.is_ascii_digit()) {
        return Err(GrpcError::InvalidTimeout(value.to_string()));
    }
    let n: u64 = digits
        .parse()
        .map_err(|_| GrpcError::InvalidTimeout(value.to_string()))?;
    let nanos = match unit {
        b'H' => n.checked_mul(3_600_000_000_000),
        b'M' => n.checked_mul(60_000_000_000),
        b'S' => n.checked_mul(1_000_000_000),
        b'm' => n.checked_mul(1_000_000),
        b'u' => n.checked_mul(1_000),
        b'n' => Some(n),
        _ => None,
    }
    .ok_or_else(|| GrpcError::InvalidTimeout(value.to_string()))?;
    Ok(Duration::from_nanos(nanos))
}
pub fn format_grpc_timeout(duration: Duration) -> String {
    let n = duration.as_nanos();
    if n == 0 {
        return "0n".to_string();
    }
    for (unit, scale) in [
        (b'H', 3_600_000_000_000u128),
        (b'M', 60_000_000_000),
        (b'S', 1_000_000_000),
        (b'm', 1_000_000),
        (b'u', 1_000),
        (b'n', 1),
    ] {
        let value = (n / scale).max(1);
        if value <= 99_999_999 {
            return format!("{value}{}", unit as char);
        }
    }
    "99999999H".to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GrpcCode {
    Ok = 0,
    Cancelled = 1,
    Unknown = 2,
    InvalidArgument = 3,
    DeadlineExceeded = 4,
    NotFound = 5,
    AlreadyExists = 6,
    PermissionDenied = 7,
    ResourceExhausted = 8,
    FailedPrecondition = 9,
    Aborted = 10,
    OutOfRange = 11,
    Unimplemented = 12,
    Internal = 13,
    Unavailable = 14,
    DataLoss = 15,
    Unauthenticated = 16,
}
impl GrpcCode {
    fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Self::Ok,
            1 => Self::Cancelled,
            2 => Self::Unknown,
            3 => Self::InvalidArgument,
            4 => Self::DeadlineExceeded,
            5 => Self::NotFound,
            6 => Self::AlreadyExists,
            7 => Self::PermissionDenied,
            8 => Self::ResourceExhausted,
            9 => Self::FailedPrecondition,
            10 => Self::Aborted,
            11 => Self::OutOfRange,
            12 => Self::Unimplemented,
            13 => Self::Internal,
            14 => Self::Unavailable,
            15 => Self::DataLoss,
            16 => Self::Unauthenticated,
            _ => return None,
        })
    }
}
impl fmt::Display for GrpcCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", *self as u8)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpcStatus {
    pub code: GrpcCode,
    pub message: String,
    pub details_bin: Option<Vec<u8>>,
}
impl GrpcStatus {
    pub fn ok() -> Self {
        Self {
            code: GrpcCode::Ok,
            message: String::new(),
            details_bin: None,
        }
    }
    pub fn new(code: GrpcCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details_bin: None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpcTrailers {
    pub metadata: GrpcMetadata,
    pub status: GrpcStatus,
}
pub fn build_grpc_trailers(
    status: &GrpcStatus,
    metadata: &GrpcMetadata,
) -> Result<HeaderMap, GrpcError> {
    let mut map = HeaderMap::new();
    for entry in metadata.iter() {
        let name = HeaderName::try_from(entry.key.as_str())
            .map_err(|_| GrpcError::InvalidMetadata(entry.key.clone()))?;
        let value = HeaderValue::from_bytes(&entry.value)
            .map_err(|_| GrpcError::InvalidMetadata(entry.key.clone()))?;
        map.append(name, value);
    }
    map.insert(
        "grpc-status",
        HeaderValue::from_str(&status.code.to_string()).unwrap(),
    );
    if !status.message.is_empty() {
        map.insert(
            "grpc-message",
            HeaderValue::from_str(&percent_encode_grpc_message(&status.message))
                .map_err(|_| GrpcError::InvalidStatusMessage)?,
        );
    }
    if let Some(details) = &status.details_bin {
        map.insert(
            "grpc-status-details-bin",
            HeaderValue::from_str(&base64_encode(details)).unwrap(),
        );
    }
    Ok(map)
}
pub fn parse_grpc_trailers(headers: &HeaderMap) -> Result<GrpcTrailers, GrpcError> {
    let raw = headers
        .get("grpc-status")
        .ok_or(GrpcError::MissingStatus)?
        .to_str()
        .map_err(|_| GrpcError::InvalidStatus)?;
    let code_num: u8 = raw.parse().map_err(|_| GrpcError::InvalidStatus)?;
    let code = GrpcCode::from_u8(code_num).ok_or(GrpcError::InvalidStatus)?;
    let message = headers
        .get("grpc-message")
        .map(|v| v.to_str().map_err(|_| GrpcError::InvalidStatusMessage))
        .transpose()?
        .map(percent_decode_grpc_message)
        .transpose()?
        .unwrap_or_default();
    let details_bin = headers
        .get("grpc-status-details-bin")
        .map(|v| v.to_str().map_err(|_| GrpcError::InvalidDetails))
        .transpose()?
        .map(base64_decode)
        .transpose()?;
    let mut metadata = GrpcMetadata::new();
    for (key, value) in headers {
        if key.as_str() != "grpc-status"
            && key.as_str() != "grpc-message"
            && key.as_str() != "grpc-status-details-bin"
        {
            metadata.append(key.as_str().to_string(), value.as_bytes().to_vec())?;
        }
    }
    Ok(GrpcTrailers {
        metadata,
        status: GrpcStatus {
            code,
            message,
            details_bin,
        },
    })
}
fn base64_encode(input: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (a << 16) | (b << 8) | c;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
fn base64_decode(input: &str) -> Result<Vec<u8>, GrpcError> {
    let bytes = input.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(GrpcError::InvalidDetails);
    }
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        let mut pad = 0;
        for &c in chunk {
            n <<= 6;
            if c == b'=' {
                pad += 1;
            } else {
                n |= match c {
                    b'A'..=b'Z' => c - b'A',
                    b'a'..=b'z' => c - b'a' + 26,
                    b'0'..=b'9' => c - b'0' + 52,
                    b'+' => 62,
                    b'/' => 63,
                    _ => return Err(GrpcError::InvalidDetails),
                } as u32;
            }
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

#[derive(Debug)]
pub enum GrpcError {
    Io(io::Error),
    Protocol(String),
    Frame(GrpcFrameError),
    InvalidPath(String),
    InvalidContentType(String),
    InvalidTe,
    InvalidMetadata(String),
    ReservedMetadata(String),
    MalformedPercentEncoding,
    InvalidUtf8,
    InvalidTimeout(String),
    MissingStatus,
    InvalidStatus,
    InvalidStatusMessage,
    InvalidDetails,
    Cancelled,
    DeadlineExceeded,
    CapacityClosed,
}
impl fmt::Display for GrpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "gRPC I/O error: {e}"),
            Self::Protocol(e) => write!(f, "gRPC protocol error: {e}"),
            Self::Frame(e) => e.fmt(f),
            Self::InvalidPath(e) => write!(f, "invalid gRPC path: {e}"),
            Self::InvalidContentType(e) => write!(f, "invalid gRPC content-type: {e}"),
            Self::InvalidTe => write!(f, "TE must contain trailers"),
            Self::InvalidMetadata(e) => write!(f, "invalid metadata key/value: {e}"),
            Self::ReservedMetadata(e) => write!(f, "reserved metadata key: {e}"),
            Self::MalformedPercentEncoding => write!(f, "malformed percent encoding"),
            Self::InvalidUtf8 => write!(f, "invalid UTF-8"),
            Self::InvalidTimeout(e) => write!(f, "invalid grpc-timeout: {e}"),
            Self::MissingStatus => write!(f, "missing grpc-status trailer"),
            Self::InvalidStatus => write!(f, "invalid grpc-status"),
            Self::InvalidStatusMessage => write!(f, "invalid grpc-message"),
            Self::InvalidDetails => write!(f, "invalid grpc-status-details-bin"),
            Self::Cancelled => write!(f, "cancelled"),
            Self::DeadlineExceeded => write!(f, "deadline exceeded"),
            Self::CapacityClosed => write!(f, "stream capacity closed"),
        }
    }
}
impl std::error::Error for GrpcError {}
impl From<io::Error> for GrpcError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<GrpcFrameError> for GrpcError {
    fn from(e: GrpcFrameError) -> Self {
        Self::Frame(e)
    }
}

/// Bounded inbound stream exposed to a service or client.
pub struct GrpcReceiver {
    rx: mpsc::Receiver<Result<GrpcMessage, GrpcError>>,
    cancel: watch::Sender<bool>,
}
impl GrpcReceiver {
    pub async fn recv(&mut self) -> Option<Result<GrpcMessage, GrpcError>> {
        self.rx.recv().await
    }
    pub fn cancel(&self) {
        let _ = self.cancel.send(true);
    }
}
/// Bounded outbound stream. Dropping or closing it ends the request stream.
#[derive(Clone)]

pub struct GrpcSender {
    tx: mpsc::Sender<Result<GrpcMessage, GrpcError>>,
    cancel: watch::Sender<bool>,
    end: watch::Sender<bool>,
}
impl GrpcSender {
    pub async fn send(&self, message: GrpcMessage) -> Result<(), GrpcError> {
        self.tx
            .send(Ok(message))
            .await
            .map_err(|_| GrpcError::CapacityClosed)
    }
    async fn send_error(&self, error: GrpcError) {
        let _ = self.tx.send(Err(error)).await;
    }
    pub fn try_send(&self, message: GrpcMessage) -> Result<(), GrpcError> {
        self.tx
            .try_send(Ok(message))
            .map_err(|_| GrpcError::CapacityClosed)
    }
    pub fn finish(&self) {
        let _ = self.end.send(true);
    }
    pub fn cancel(&self) {
        let _ = self.cancel.send(true);
    }
}
fn stream_pair(capacity: usize) -> (GrpcSender, GrpcReceiver) {
    let (tx, rx) = mpsc::channel(capacity);
    let (cancel, _cancel_rx) = watch::channel(false);
    let (end, _end_rx) = watch::channel(false);
    (
        GrpcSender {
            tx,
            cancel: cancel.clone(),
            end,
        },
        GrpcReceiver { rx, cancel },
    )
}
pub struct GrpcRequest {
    pub path: String,
    pub metadata: GrpcMetadata,
    pub inbound: GrpcReceiver,
}
pub struct GrpcResponse {
    pub metadata: GrpcMetadata,
    pub status: GrpcStatus,
    pub outbound: GrpcReceiver,
}
impl GrpcResponse {
    pub fn unary(message: GrpcMessage) -> Self {
        let (tx, rx) = stream_pair(1);
        let _ = tx.try_send(message);
        drop(tx);
        Self {
            metadata: GrpcMetadata::new(),
            status: GrpcStatus::ok(),
            outbound: rx,
        }
    }
    pub fn streaming(capacity: usize) -> (GrpcSender, Self) {
        let (tx, rx) = stream_pair(capacity.max(1));
        (
            tx,
            Self {
                metadata: GrpcMetadata::new(),
                status: GrpcStatus::ok(),
                outbound: rx,
            },
        )
    }
    pub fn with_status(mut self, status: GrpcStatus) -> Self {
        self.status = status;
        self
    }
    pub fn with_metadata(mut self, metadata: GrpcMetadata) -> Self {
        self.metadata = metadata;
        self
    }
}
pub type GrpcBoxFuture =
    std::pin::Pin<Box<dyn Future<Output = Result<GrpcResponse, GrpcError>> + Send>>;
pub trait GrpcService: Send + Sync + 'static {
    fn call(&self, request: GrpcRequest) -> GrpcBoxFuture;
}
impl<F, Fut> GrpcService for F
where
    F: Fn(GrpcRequest) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<GrpcResponse, GrpcError>> + Send + 'static,
{
    fn call(&self, r: GrpcRequest) -> GrpcBoxFuture {
        Box::pin((self)(r))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrpcCallType {
    Unary,
    ClientStreaming,
    ServerStreaming,
    BidiStreaming,
}
/// Optional rustls server identity for a gRPC listener. When present the
/// listener terminates TLS with ALPN `h2` before the h2 handshake; when
/// absent the listener stays cleartext (h2c), which remains the default
/// contract for `GrpcServer::bind` and `client_connect`.
#[derive(Clone, Debug, Default)]
pub struct GrpcTlsIdentity {
    pub cert_chain_der: Vec<Vec<u8>>,
    pub private_key_der: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct GrpcServerConfig {
    pub max_message_size: usize,
    pub stream_capacity: usize,
    pub max_concurrent_streams: u32,
    pub tls: Option<GrpcTlsIdentity>,
}
impl Default for GrpcServerConfig {
    fn default() -> Self {
        Self {
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
            stream_capacity: DEFAULT_STREAM_CAPACITY,
            max_concurrent_streams: 256,
            tls: None,
        }
    }
}
pub struct GrpcServer {
    local_addr: SocketAddr,
    shutdown: watch::Sender<bool>,
}
impl GrpcServer {
    pub async fn bind(
        addr: SocketAddr,
        service: Arc<dyn GrpcService>,
        config: GrpcServerConfig,
    ) -> Result<Self, GrpcError> {
        let accept_tls = match &config.tls {
            Some(identity) => Some(tokio_rustls::TlsAcceptor::from(
                crate::tls::server_config_from_der(
                    identity.cert_chain_der.clone(),
                    identity.private_key_der.clone(),
                    vec![b"h2".to_vec()],
                )
                .map_err(|e| GrpcError::Protocol(e.to_string()))?,
            )),
            None => None,
        };
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        let (shutdown, mut stop) = watch::channel(false);
        tokio::spawn(async move {
            loop {
                tokio::select! { _=stop.changed()=>break, result=listener.accept()=>{ let Ok((io,_))=result else {break}; let service=service.clone(); let cfg=config.clone(); let tls=accept_tls.clone(); tokio::spawn(async move { match tls { Some(acceptor) => { let Ok(tls_io)=acceptor.accept(io).await else{return}; serve_connection(tls_io,service,cfg).await; }, None => { serve_connection(io,service,cfg).await; } } }); } }
            }
        });
        Ok(Self {
            local_addr,
            shutdown,
        })
    }
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(true);
    }
}

async fn serve_connection<I>(io: I, service: Arc<dyn GrpcService>, config: GrpcServerConfig)
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let mut builder = server::Builder::new();
    builder.max_concurrent_streams(config.max_concurrent_streams);
    let Ok(mut connection) = builder.handshake(io).await else {
        return;
    };
    while let Some(result) = connection.accept().await {
        let Ok((request, respond)) = result else {
            break;
        };
        let service = service.clone();
        let cfg = config.clone();
        tokio::spawn(async move {
            serve_stream(request, respond, service, cfg).await;
        });
    }
}
async fn serve_stream(
    request: Request<h2::RecvStream>,
    mut respond: SendResponse<Bytes>,
    service: Arc<dyn GrpcService>,
    config: GrpcServerConfig,
) {
    let (parts, body) = request.into_parts();
    let path = parts.uri.path().to_string();
    let content_type = parts
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let te = parts.headers.get("te").and_then(|v| v.to_str().ok());
    if parts.method != Method::POST || validate_grpc_request(&path, content_type, te).is_err() {
        let Ok(response) = Response::builder()
            .status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
            .body(())
        else {
            return;
        };
        let _ = respond.send_response(response, true);
        return;
    }
    let mut metadata = GrpcMetadata::new();
    for (k, v) in &parts.headers {
        if !matches!(k.as_str(), "content-type" | "te" | "grpc-timeout")
            && metadata
                .append(k.as_str().to_string(), v.as_bytes().to_vec())
                .is_err()
        {
            return;
        }
    }
    let (inbound_tx, inbound_rx) = mpsc::channel(config.stream_capacity.max(1));
    let (cancel, cancel_rx) = watch::channel(false);
    let timeout = match parts.headers.get("grpc-timeout") {
        Some(value) => match value
            .to_str()
            .ok()
            .and_then(|raw| parse_grpc_timeout(raw).ok())
        {
            Some(duration) => Some(duration),
            None => {
                respond.send_reset(Reason::PROTOCOL_ERROR);
                return;
            }
        },
        None => None,
    };
    tokio::spawn(read_body(
        body,
        inbound_tx,
        config.max_message_size,
        cancel_rx,
    ));
    let request = GrpcRequest {
        path,
        metadata,
        inbound: GrpcReceiver {
            rx: inbound_rx,
            cancel: cancel.clone(),
        },
    };
    let result = if let Some(duration) = timeout {
        match tokio::time::timeout(duration, service.call(request)).await {
            Ok(v) => v,
            Err(_) => Err(GrpcError::DeadlineExceeded),
        }
    } else {
        service.call(request).await
    };
    if result
        .as_ref()
        .err()
        .is_some_and(|error| matches!(error, GrpcError::DeadlineExceeded))
    {
        respond.send_reset(Reason::CANCEL);
        return;
    }
    let response = match result {
        Ok(v) => v,
        Err(e) => GrpcResponse {
            metadata: GrpcMetadata::new(),
            status: match e {
                GrpcError::DeadlineExceeded => {
                    GrpcStatus::new(GrpcCode::DeadlineExceeded, "deadline exceeded")
                }
                GrpcError::Cancelled => GrpcStatus::new(GrpcCode::Cancelled, "cancelled"),
                GrpcError::Frame(GrpcFrameError::InvalidCompressedFlag(_)) => GrpcStatus::new(
                    GrpcCode::Unimplemented,
                    "gRPC message compression (grpc-encoding) is not supported",
                ),
                _ => GrpcStatus::new(GrpcCode::Internal, e.to_string()),
            },
            outbound: stream_pair(1).1,
        },
    };
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/grpc");
    for e in response.metadata.iter() {
        if let (Ok(n), Ok(v)) = (
            HeaderName::try_from(e.key.as_str()),
            HeaderValue::from_bytes(&e.value),
        ) {
            builder = builder.header(n, v);
        }
    }
    let Ok(head) = builder.body(()) else { return };
    let Ok(mut sender) = respond.send_response(head, false) else {
        return;
    };
    let mut outbound = response.outbound;
    while let Some(item) = outbound.recv().await {
        let Ok(frame) = item else { break };
        let encoded = match encode_grpc_message_with_limit(
            &frame,
            GrpcCompression::Identity,
            config.max_message_size,
        ) {
            Ok(bytes) => bytes,
            Err(_) => {
                sender.send_reset(Reason::ENHANCE_YOUR_CALM);
                return;
            }
        };
        if send_data_bounded(&mut sender, Bytes::from(encoded))
            .await
            .is_err()
        {
            return;
        }
    }
    if let Ok(trailers) = build_grpc_trailers(&response.status, &GrpcMetadata::new()) {
        let _ = sender.send_trailers(trailers);
    }
    let _ = cancel.send(true);
}
async fn read_body(
    mut body: h2::RecvStream,
    tx: mpsc::Sender<Result<GrpcMessage, GrpcError>>,
    max: usize,
    mut cancel: watch::Receiver<bool>,
) {
    let mut parser = GrpcMessageParser::new(max);
    loop {
        tokio::select! {
            _ = cancel.changed() => break,
            item = body.data() => {
                let Some(item) = item else { break };
                match item {
                    Ok(bytes) => {
                        let len = bytes.len();
                        let _ = body.flow_control().release_capacity(len);
                        match parser.push(&bytes) {
                            Ok(messages) => {
                                for message in messages {
                                    if tx.send(Ok(message)).await.is_err() { return; }
                                }
                            }
                            Err(error) => {
                                let _ = tx.send(Err(error.into())).await;
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        let _ = tx.send(Err(GrpcError::Protocol(error.to_string()))).await;
                        return;
                    }
                }
            }
        }
    }
    if let Err(error) = parser.finish() {
        let _ = tx.send(Err(error.into())).await;
    }
}

async fn send_data_bounded(
    sender: &mut h2::SendStream<Bytes>,
    data: Bytes,
) -> Result<(), GrpcError> {
    let mut offset = 0;
    while offset < data.len() {
        let want = (data.len() - offset).min(16 * 1024);
        sender.reserve_capacity(want);
        let capacity = poll_fn(|cx| sender.poll_capacity(cx))
            .await
            .ok_or(GrpcError::Cancelled)?
            .map_err(|e| GrpcError::Protocol(e.to_string()))?;
        if capacity == 0 {
            continue;
        }
        let amount = capacity.min(want);
        sender
            .send_data(data.slice(offset..offset + amount), false)
            .map_err(|e| GrpcError::Protocol(e.to_string()))?;
        offset += amount;
    }
    Ok(())
}
pub struct GrpcClient {
    sender: client::SendRequest<Bytes>,
    max_message_size: usize,
}
impl GrpcClient {
    pub async fn connect(addr: SocketAddr) -> Result<Self, GrpcError> {
        let io = TcpStream::connect(addr).await?;
        let (sender, connection) = client::Builder::new()
            .handshake(io)
            .await
            .map_err(|e| GrpcError::Protocol(e.to_string()))?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        Ok(Self {
            sender,
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
        })
    }
    pub fn with_max_message_size(mut self, max: usize) -> Self {
        self.max_message_size = max;
        self
    }
    pub async fn unary(
        &mut self,
        path: &str,
        message: GrpcMessage,
        metadata: GrpcMetadata,
        timeout: Option<Duration>,
    ) -> Result<(GrpcMessage, GrpcTrailers), GrpcError> {
        validate_grpc_path(path)?;
        let frame = encode_grpc_message_with_limit(
            &message,
            GrpcCompression::Identity,
            self.max_message_size,
        )?;
        let req = build_request(path, &metadata, timeout)?;
        let (future, mut stream) = self
            .sender
            .send_request(req, false)
            .map_err(|e| GrpcError::Protocol(e.to_string()))?;
        send_data_bounded(&mut stream, Bytes::from(frame)).await?;
        stream
            .send_data(Bytes::new(), true)
            .map_err(|e| GrpcError::Protocol(e.to_string()))?;
        let response = if let Some(t) = timeout {
            tokio::time::timeout(t, future)
                .await
                .map_err(|_| GrpcError::DeadlineExceeded)?
                .map_err(|e| GrpcError::Protocol(e.to_string()))?
        } else {
            future
                .await
                .map_err(|e| GrpcError::Protocol(e.to_string()))?
        };
        let mut body = response.into_body();
        let mut parser = GrpcMessageParser::new(self.max_message_size);
        let mut messages = Vec::new();
        while let Some(chunk) = body.data().await {
            let chunk = chunk.map_err(|e| GrpcError::Protocol(e.to_string()))?;
            let n = chunk.len();
            body.flow_control()
                .release_capacity(n)
                .map_err(|e| GrpcError::Protocol(e.to_string()))?;
            messages.extend(parser.push(&chunk)?);
        }
        parser.finish()?;
        let trailers = body
            .trailers()
            .await
            .map_err(|e| GrpcError::Protocol(e.to_string()))?
            .ok_or(GrpcError::MissingStatus)
            .and_then(|h| parse_grpc_trailers(&h))?;
        if messages.len() != 1 {
            return Err(GrpcError::Protocol(format!(
                "unary response contained {} messages",
                messages.len()
            )));
        }
        Ok((messages.remove(0), trailers))
    }
    pub async fn server_streaming(
        &mut self,
        path: &str,
        message: GrpcMessage,
        metadata: GrpcMetadata,
        timeout: Option<Duration>,
    ) -> Result<GrpcReceiver, GrpcError> {
        let frame = encode_grpc_message_with_limit(
            &message,
            GrpcCompression::Identity,
            self.max_message_size,
        )?;
        let req = build_request(path, &metadata, timeout)?;
        let (future, mut stream) = self
            .sender
            .send_request(req, false)
            .map_err(|e| GrpcError::Protocol(e.to_string()))?;
        send_data_bounded(&mut stream, Bytes::from(frame)).await?;
        stream
            .send_data(Bytes::new(), true)
            .map_err(|e| GrpcError::Protocol(e.to_string()))?;
        let (cap_tx, rx) = stream_pair(DEFAULT_STREAM_CAPACITY);
        tokio::spawn(read_response(future, cap_tx, self.max_message_size));
        Ok(rx)
    }
    pub async fn client_streaming(
        &mut self,
        path: &str,
        metadata: GrpcMetadata,
        timeout: Option<Duration>,
    ) -> Result<GrpcClientStream, GrpcError> {
        self.open_stream(path, metadata, timeout).await
    }
    pub async fn bidi_streaming(
        &mut self,
        path: &str,
        metadata: GrpcMetadata,
        timeout: Option<Duration>,
    ) -> Result<GrpcClientStream, GrpcError> {
        self.open_stream(path, metadata, timeout).await
    }
    async fn open_stream(
        &mut self,
        path: &str,
        metadata: GrpcMetadata,
        timeout: Option<Duration>,
    ) -> Result<GrpcClientStream, GrpcError> {
        validate_grpc_path(path)?;
        let req = build_request(path, &metadata, timeout)?;
        let (future, stream) = self
            .sender
            .send_request(req, false)
            .map_err(|e| GrpcError::Protocol(e.to_string()))?;
        let (cap_tx, rx) = stream_pair(DEFAULT_STREAM_CAPACITY);
        let (capacity_tx, request_rx) = mpsc::channel(DEFAULT_STREAM_CAPACITY);
        let (cancel, cancel_rx) = watch::channel(false);
        let (end, end_rx) = watch::channel(false);
        let request_sender = GrpcSender {
            tx: capacity_tx,
            cancel: cancel.clone(),
            end,
        };
        tokio::spawn(write_request(
            stream,
            request_rx,
            cancel_rx,
            end_rx,
            self.max_message_size,
            timeout,
        ));
        tokio::spawn(read_response(future, cap_tx, self.max_message_size));
        Ok(GrpcClientStream {
            sender: request_sender,
            receiver: rx,
            cancel,
        })
    }
}
impl GrpcClient {
    pub async fn connect_tls(
        addr: SocketAddr,
        root_certs_der: Vec<Vec<u8>>,
        server_name: &str,
    ) -> Result<Self, GrpcError> {
        let name = rustls::pki_types::ServerName::try_from(server_name.to_owned())
            .map_err(|_| GrpcError::Protocol("invalid TLS server name".to_string()))?;
        let tls = crate::tls::client_config_with_roots(root_certs_der, vec![b"h2".to_vec()])
            .map_err(|e| GrpcError::Protocol(e.to_string()))?;
        let io = TcpStream::connect(addr).await?;
        let connector = tokio_rustls::TlsConnector::from(tls);
        let tls_io = connector
            .connect(name, io)
            .await
            .map_err(|e| GrpcError::Protocol(format!("gRPC TLS handshake failed: {e}")))?;
        let (sender, connection) = client::Builder::new()
            .handshake(tls_io)
            .await
            .map_err(|e| GrpcError::Protocol(e.to_string()))?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        Ok(Self {
            sender,
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
        })
    }
}
/// A bidirectional bounded call. For client-streaming calls, send all request
/// messages, then call `finish`; for bidi calls sends and receives may overlap.
pub struct GrpcClientStream {
    pub sender: GrpcSender,
    pub receiver: GrpcReceiver,
    cancel: watch::Sender<bool>,
}
impl GrpcClientStream {
    pub async fn send(&self, message: GrpcMessage) -> Result<(), GrpcError> {
        self.sender.send(message).await
    }
    pub async fn recv(&mut self) -> Option<Result<GrpcMessage, GrpcError>> {
        self.receiver.recv().await
    }
    pub fn finish(&self) {
        self.sender.finish();
    }
    pub fn cancel(&self) {
        let _ = self.cancel.send(true);
        self.sender.cancel();
        self.receiver.cancel();
    }
}
async fn flush_queued_request_messages(
    stream: &mut h2::SendStream<Bytes>,
    input: &mut mpsc::Receiver<Result<GrpcMessage, GrpcError>>,
    max: usize,
) -> bool {
    loop {
        match input.try_recv() {
            Ok(Ok(message)) => {
                let frame = match encode_grpc_message_with_limit(
                    &message,
                    GrpcCompression::Identity,
                    max,
                ) {
                    Ok(frame) => frame,
                    Err(_) => {
                        stream.send_reset(Reason::PROTOCOL_ERROR);
                        return false;
                    }
                };
                if send_data_bounded(stream, Bytes::from(frame)).await.is_err() {
                    return false;
                }
            }
            Ok(Err(_)) => {
                stream.send_reset(Reason::CANCEL);
                return false;
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            | Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
        }
    }
    stream.send_data(Bytes::new(), true).is_ok()
}

async fn write_request(
    mut stream: h2::SendStream<Bytes>,
    mut input: mpsc::Receiver<Result<GrpcMessage, GrpcError>>,
    mut cancel: watch::Receiver<bool>,
    mut end: watch::Receiver<bool>,
    max: usize,
    timeout: Option<Duration>,
) {
    if let Some(duration) = timeout {
        let sleep = tokio::time::sleep(duration);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = cancel.changed() => { stream.send_reset(Reason::CANCEL); return; }
                _ = end.changed() => { let _ = flush_queued_request_messages(&mut stream, &mut input, max).await; return; }
                _ = &mut sleep => { stream.send_reset(Reason::CANCEL); return; }
                item = input.recv() => match item {
                    Some(Ok(message)) => match encode_grpc_message_with_limit(&message, GrpcCompression::Identity, max) {
                        Ok(frame) => { if send_data_bounded(&mut stream, Bytes::from(frame)).await.is_err() { return; } }
                        Err(_) => { stream.send_reset(Reason::PROTOCOL_ERROR); return; }
                    },
                    Some(Err(_)) => { stream.send_reset(Reason::CANCEL); return; }
                    None => { let _ = stream.send_data(Bytes::new(), true); return; }
                }
            }
        }
    } else {
        loop {
            tokio::select! {
                _ = end.changed() => { let _ = flush_queued_request_messages(&mut stream, &mut input, max).await; return; }
                _ = cancel.changed() => { stream.send_reset(Reason::CANCEL); return; }
                item = input.recv() => match item {
                    Some(Ok(message)) => match encode_grpc_message_with_limit(&message, GrpcCompression::Identity, max) {
                        Ok(frame) => { if send_data_bounded(&mut stream, Bytes::from(frame)).await.is_err() { return; } }
                        Err(_) => { stream.send_reset(Reason::PROTOCOL_ERROR); return; }
                    },
                    Some(Err(_)) => { stream.send_reset(Reason::CANCEL); return; }
                    None => { let _ = stream.send_data(Bytes::new(), true); return; }
                }
            }
        }
    }
}
fn build_request(
    path: &str,
    metadata: &GrpcMetadata,
    timeout: Option<Duration>,
) -> Result<Request<()>, GrpcError> {
    let mut b = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header("content-type", "application/grpc")
        .header("te", "trailers");
    if let Some(t) = timeout {
        b = b.header("grpc-timeout", format_grpc_timeout(t));
    }
    for e in metadata.iter() {
        let n = HeaderName::try_from(e.key.as_str())
            .map_err(|_| GrpcError::InvalidMetadata(e.key.clone()))?;
        let v = HeaderValue::from_bytes(&e.value)
            .map_err(|_| GrpcError::InvalidMetadata(e.key.clone()))?;
        b = b.header(n, v);
    }
    b.body(()).map_err(|e| GrpcError::Protocol(e.to_string()))
}
async fn read_response(future: client::ResponseFuture, tx: GrpcSender, max: usize) {
    let response = match future.await {
        Ok(v) => v,
        Err(e) => {
            tx.send_error(GrpcError::Protocol(e.to_string())).await;
            return;
        }
    };
    let mut body = response.into_body();
    let mut parser = GrpcMessageParser::new(max);
    while let Some(chunk) = body.data().await {
        let chunk = match chunk {
            Ok(v) => v,
            Err(e) => {
                tx.send_error(GrpcError::Protocol(e.to_string())).await;
                return;
            }
        };
        let n = chunk.len();
        let _ = body.flow_control().release_capacity(n);
        match parser.push(&chunk) {
            Ok(messages) => {
                for message in messages {
                    if tx.send(message).await.is_err() {
                        return;
                    }
                }
            }
            Err(e) => {
                tx.send_error(e.into()).await;
                return;
            }
        }
    }
    if let Err(e) = parser.finish() {
        tx.send_error(e.into()).await;
        return;
    }
    let trailers = match body.trailers().await {
        Ok(Some(v)) => v,
        Ok(None) => {
            tx.send_error(GrpcError::MissingStatus).await;
            return;
        }
        Err(e) => {
            tx.send_error(GrpcError::Protocol(e.to_string())).await;
            return;
        }
    };
    match parse_grpc_trailers(&trailers) {
        Ok(value) if value.status.code == GrpcCode::Ok => {}
        Ok(value) => {
            tx.send_error(GrpcError::Protocol(format!(
                "gRPC status {}",
                value.status.code
            )))
            .await
        }
        Err(e) => tx.send_error(e).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_envelope_and_binary_bytes() {
        let msg = GrpcMessage::new(vec![0, 255, 1]);
        let frame = encode_grpc_message_with_limit(&msg, GrpcCompression::Identity, 10).unwrap();
        let mut p = GrpcMessageParser::new(10);
        let mut messages = Vec::new();
        for b in frame.chunks(1) {
            messages.extend(p.push(b).unwrap());
        }
        assert_eq!(messages, vec![msg]);
        assert_eq!(p.finish(), Ok(()));
    }
    #[test]
    fn malformed_percent_and_timeout() {
        assert!(percent_decode_grpc_message("bad%G0").is_err());
        assert_eq!(
            parse_grpc_timeout("10m").unwrap(),
            Duration::from_millis(10)
        );
        assert!(parse_grpc_timeout("1x").is_err());
    }
    #[tokio::test(flavor = "current_thread")]
    async fn localhost_h2_unary_round_trip() {
        let service: Arc<dyn GrpcService> = Arc::new(|mut request: GrpcRequest| async move {
            let message = request
                .inbound
                .recv()
                .await
                .expect("request message")
                .expect("valid request");
            Ok(GrpcResponse::unary(message))
        });
        let server = GrpcServer::bind(
            "127.0.0.1:0".parse().unwrap(),
            service,
            GrpcServerConfig::default(),
        )
        .await
        .unwrap();
        let mut client = GrpcClient::connect(server.local_addr()).await.unwrap();
        let input = GrpcMessage::new(vec![0, 255, 1, 2]);
        let (output, trailers) = client
            .unary("/test.Echo/Unary", input.clone(), GrpcMetadata::new(), None)
            .await
            .unwrap();
        assert_eq!(output, input);
        assert_eq!(trailers.status.code, GrpcCode::Ok);
        server.shutdown();
    }
    #[tokio::test(flavor = "current_thread")]
    async fn localhost_streaming_cardinalities_and_capacity() {
        let service: Arc<dyn GrpcService> = Arc::new(|mut request: GrpcRequest| async move {
            let first = request
                .inbound
                .recv()
                .await
                .expect("request message")
                .expect("valid request");
            if request.path.ends_with("/server") {
                let (sender, response) = GrpcResponse::streaming(1);
                tokio::spawn(async move {
                    sender.send(first.clone()).await.unwrap();
                    sender.send(first).await.unwrap();
                });
                Ok(response)
            } else {
                let mut all = first.into_bytes();
                while let Some(item) = request.inbound.recv().await {
                    all.extend_from_slice(&item.unwrap().into_bytes());
                }
                Ok(GrpcResponse::unary(GrpcMessage::new(all)))
            }
        });
        let server = GrpcServer::bind(
            "127.0.0.1:0".parse().unwrap(),
            service,
            GrpcServerConfig::default(),
        )
        .await
        .unwrap();
        let mut client = GrpcClient::connect(server.local_addr()).await.unwrap();
        let mut output = client
            .server_streaming(
                "/test.Echo/server",
                GrpcMessage::new(b"x".to_vec()),
                GrpcMetadata::new(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            output.recv().await.unwrap().unwrap(),
            GrpcMessage::new(b"x".to_vec())
        );
        assert_eq!(
            output.recv().await.unwrap().unwrap(),
            GrpcMessage::new(b"x".to_vec())
        );
        assert!(output.recv().await.is_none());
        let mut call = client
            .client_streaming("/test.Echo/client", GrpcMetadata::new(), None)
            .await
            .unwrap();
        call.send(GrpcMessage::new(b"a".to_vec())).await.unwrap();
        call.send(GrpcMessage::new(b"b".to_vec())).await.unwrap();
        call.finish();
        assert_eq!(
            call.recv().await.unwrap().unwrap(),
            GrpcMessage::new(b"ab".to_vec())
        );
        server.shutdown();
    }
    #[test]
    fn status_round_trip() {
        let mut m = GrpcMetadata::new();
        m.insert("x-test", b"ok".to_vec()).unwrap();
        let mut s = GrpcStatus::new(GrpcCode::InvalidArgument, "bad / bytes");
        s.details_bin = Some(vec![0, 255]);
        let h = build_grpc_trailers(&s, &m).unwrap();
        let t = parse_grpc_trailers(&h).unwrap();
        assert_eq!(t.status, s);
        assert_eq!(t.metadata.get("x-test"), Some(&b"ok"[..]));
    }
    #[test]
    fn compressed_flag_is_rejected_not_misparsed() {
        let mut p = GrpcMessageParser::new(16);
        let error = p
            .push(&[1, 0, 0, 0, 1, 9])
            .expect_err("flag 1 must be rejected");
        assert_eq!(error, GrpcFrameError::InvalidCompressedFlag(1));
        let mut p = GrpcMessageParser::new(16);
        assert!(p.push(&[2, 0, 0, 0, 0]).is_err());
        let msg = GrpcMessage::new(vec![7, 8]);
        let frame = encode_grpc_message_with_limit(&msg, GrpcCompression::Identity, 16).unwrap();
        let mut p = GrpcMessageParser::new(16);
        assert_eq!(p.push(&frame).unwrap(), vec![msg]);
    }
    #[tokio::test(flavor = "current_thread")]
    async fn compressed_request_maps_to_unimplemented_trailers() {
        // Inbound frame errors propagated by the service must surface as
        // grpc-status 12 (UNIMPLEMENTED), never 200/OK or 13/INTERNAL.
        let service: Arc<dyn GrpcService> = Arc::new(|mut request: GrpcRequest| async move {
            let message = request.inbound.recv().await.expect("request item")?;
            Ok(GrpcResponse::unary(message))
        });
        let server = GrpcServer::bind(
            "127.0.0.1:0".parse().unwrap(),
            service,
            GrpcServerConfig::default(),
        )
        .await
        .unwrap();
        let io = TcpStream::connect(server.local_addr()).await.unwrap();
        let (mut sender, connection) = client::Builder::new()
            .handshake::<_, Bytes>(io)
            .await
            .expect("h2 handshake");
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let req = Request::builder()
            .method(Method::POST)
            .uri("/test.Echo/Unary")
            .header("content-type", "application/grpc")
            .header("te", "trailers")
            .body(())
            .expect("request head");
        let (future, mut stream) = sender.send_request(req, false).expect("h2 stream");
        stream
            .send_data(Bytes::from(vec![1, 0, 0, 0, 3, 9, 9, 9]), true)
            .expect("compressed DATA");
        let response = future.await.expect("response head");
        assert_eq!(response.status(), StatusCode::OK);
        let mut body = response.into_body();
        while let Some(chunk) = body.data().await {
            let _ = chunk.expect("body chunk");
        }
        let trailers = body
            .trailers()
            .await
            .expect("trailers")
            .expect("trailer block");
        assert_eq!(trailers.get("grpc-status").expect("grpc-status"), "12");
        server.shutdown();
    }
    #[tokio::test(flavor = "current_thread")]
    async fn compressed_response_surfaces_frame_error() {
        // A peer sending flag-1 frames must surface client-side as
        // GrpcError::Frame, never as a silently misparsed message.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (io, _) = listener.accept().await.expect("accept");
            let mut connection = server::handshake(io).await.expect("server handshake");
            // Keep polling the connection (as serve_connection does) so I/O
            // progresses while the stream is serviced on its own task.
            while let Some(result) = connection.accept().await {
                let (request, mut respond) = result.expect("request");
                tokio::spawn(async move {
                    let (_parts, mut req_body) = request.into_parts();
                    while let Some(_chunk) = req_body.data().await {}
                    let head = Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/grpc")
                        .body(())
                        .expect("response head");
                    let mut sender = respond.send_response(head, false).expect("send head");
                    sender
                        .send_data(Bytes::from(vec![1, 0, 0, 0, 2, 7, 7]), false)
                        .expect("compressed DATA");
                    let mut trailers = HeaderMap::new();
                    trailers.insert("grpc-status", HeaderValue::from_static("0"));
                    sender.send_trailers(trailers).expect("trailers");
                });
            }
        });
        let mut client = GrpcClient::connect(addr).await.unwrap();
        let error = client
            .unary(
                "/test.Echo/Unary",
                GrpcMessage::new(vec![1]),
                GrpcMetadata::new(),
                None,
            )
            .await
            .expect_err("compressed response must fail");
        assert!(
            matches!(
                error,
                GrpcError::Frame(GrpcFrameError::InvalidCompressedFlag(1))
            ),
            "unexpected error: {error}"
        );
    }
    #[tokio::test(flavor = "current_thread")]
    async fn localhost_tls_unary_round_trip() {
        use rcgen::generate_simple_self_signed;
        let certified =
            generate_simple_self_signed(vec!["localhost".to_string()]).expect("certificate");
        let cert_der = certified.cert.der().to_vec();
        let key_der = certified.key_pair.serialize_der();
        let service: Arc<dyn GrpcService> = Arc::new(|mut request: GrpcRequest| async move {
            let message = request
                .inbound
                .recv()
                .await
                .expect("request message")
                .expect("valid request");
            Ok(GrpcResponse::unary(message))
        });
        let mut config = GrpcServerConfig::default();
        assert!(config.tls.is_none(), "cleartext stays the default contract");
        config.tls = Some(GrpcTlsIdentity {
            cert_chain_der: vec![cert_der.clone()],
            private_key_der: key_der,
        });
        let server = GrpcServer::bind("127.0.0.1:0".parse().unwrap(), service, config)
            .await
            .unwrap();
        let mut client = GrpcClient::connect_tls(server.local_addr(), vec![cert_der], "localhost")
            .await
            .unwrap();
        let input = GrpcMessage::new(vec![1, 2, 3]);
        let (output, trailers) = client
            .unary("/test.Echo/Unary", input.clone(), GrpcMetadata::new(), None)
            .await
            .unwrap();
        assert_eq!(output, input);
        assert_eq!(trailers.status.code, GrpcCode::Ok);
        server.shutdown();
    }
    #[tokio::test(flavor = "current_thread")]
    async fn tls_untrusted_root_handshake_fails() {
        use rcgen::generate_simple_self_signed;
        let server_cert =
            generate_simple_self_signed(vec!["localhost".to_string()]).expect("server certificate");
        let other_cert =
            generate_simple_self_signed(vec!["localhost".to_string()]).expect("other certificate");
        let service: Arc<dyn GrpcService> = Arc::new(|mut request: GrpcRequest| async move {
            let message = request
                .inbound
                .recv()
                .await
                .expect("request message")
                .expect("valid request");
            Ok(GrpcResponse::unary(message))
        });
        let config = GrpcServerConfig {
            tls: Some(GrpcTlsIdentity {
                cert_chain_der: vec![server_cert.cert.der().to_vec()],
                private_key_der: server_cert.key_pair.serialize_der(),
            }),
            ..Default::default()
        };
        let server = GrpcServer::bind("127.0.0.1:0".parse().unwrap(), service, config)
            .await
            .unwrap();
        let error = match GrpcClient::connect_tls(
            server.local_addr(),
            vec![other_cert.cert.der().to_vec()],
            "localhost",
        )
        .await
        {
            Ok(_) => panic!("untrusted root must fail the handshake"),
            Err(error) => error,
        };
        assert!(
            matches!(&error, GrpcError::Protocol(message) if message.contains("handshake")),
            "unexpected error: {error}"
        );
        server.shutdown();
    }
}
