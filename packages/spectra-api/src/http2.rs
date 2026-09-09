//! HTTP/2 server transport for `std.api.server`.
//!
//! The HTTP/1.1 server deliberately keeps its own mio state machine. HTTP/2
//! has different connection semantics (one connection owns many independent
//! streams, HPACK state, and stream-level flow control), so this module uses
//! the protocol-complete `h2` state machine behind a small synchronous Rust
//! server boundary. The public request/response types stay independent from
//! the external `http` crate used by the protocol implementation.

use crate::server::ServerResponse;
use bytes::Bytes;
use h2::client;
use h2::server::{self, SendResponse};
use http::{Method, Request as H2Request, Response as H2Response, Version};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ServerConfig};
use std::fmt;
use std::future::poll_fn;
use std::io;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{lookup_host, TcpListener as TokioTcpListener, TcpStream};
use tokio::runtime::Builder;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio_rustls::{TlsAcceptor, TlsConnector};

const DEFAULT_MAX_CONCURRENT_STREAMS: u32 = 256;
const DEFAULT_MAX_HEADER_LIST_SIZE: u32 = 64 * 1024;
const DEFAULT_MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

pub const ALPN_HTTP2: &[u8] = b"h2";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Http2Header {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Http2Request {
    pub method: String,
    pub target: String,
    pub headers: Vec<Http2Header>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Http2Response {
    pub status_code: u16,
    pub headers: Vec<Http2Header>,
    pub body: Vec<u8>,
}

impl Http2Response {
    pub fn text(status_code: u16, body: impl Into<String>) -> Self {
        Self {
            status_code,
            headers: vec![Http2Header {
                name: "content-type".to_string(),
                value: "text/plain; charset=utf-8".to_string(),
            }],
            body: body.into().into_bytes(),
        }
    }
}

pub type Http2Handler = Arc<dyn Fn(Http2Request) -> Http2Response + Send + Sync + 'static>;

#[derive(Clone, Debug)]
pub struct Http2Config {
    pub bind_addr: SocketAddr,
    pub max_concurrent_streams: u32,
    pub max_header_list_size: u32,
    pub max_body_bytes: usize,
    pub shutdown_grace_period: Duration,
    pub tls_config: Option<Arc<ServerConfig>>,
}

impl Default for Http2Config {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:0"
                .parse()
                .expect("default HTTP/2 bind address is valid"),
            max_concurrent_streams: DEFAULT_MAX_CONCURRENT_STREAMS,
            max_header_list_size: DEFAULT_MAX_HEADER_LIST_SIZE,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            shutdown_grace_period: DEFAULT_SHUTDOWN_GRACE,
            tls_config: None,
        }
    }
}

impl Http2Config {
    pub fn with_tls(mut self, config: Arc<ServerConfig>) -> Self {
        self.tls_config = Some(config);
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Http2Stats {
    pub accepted_connections: usize,
    pub completed_streams: usize,
    pub rejected_streams: usize,
    pub active_connections: usize,
}

#[derive(Debug)]
pub enum Http2Error {
    Io(io::Error),
    Runtime(String),
    AlreadyStopped,
}

impl fmt::Display for Http2Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "HTTP/2 I/O error: {error}"),
            Self::Runtime(message) => write!(f, "HTTP/2 runtime error: {message}"),
            Self::AlreadyStopped => write!(f, "HTTP/2 server is already stopped"),
        }
    }
}

impl std::error::Error for Http2Error {}

impl From<io::Error> for Http2Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub struct Http2Server {
    local_addr: SocketAddr,
    shutdown: watch::Sender<bool>,
    stopped: Arc<AtomicBool>,
    stats: Arc<Mutex<Http2Stats>>,
    join: Option<JoinHandle<()>>,
}

const DEFAULT_CLIENT_MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_CLIENT_MAX_CONCURRENT_STREAMS: usize = 256;
const DEFAULT_CLIENT_MAX_HEADER_LIST_SIZE: u32 = 64 * 1024;
const DEFAULT_CLIENT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_CLIENT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CLIENT_BODY_CHUNK_BYTES: usize = 16 * 1024;

#[derive(Clone)]
pub struct Http2ClientConfig {
    pub max_body_bytes: usize,
    pub max_concurrent_streams: usize,
    pub max_header_list_size: u32,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub tls_config: Option<Arc<ClientConfig>>,
    pub allow_private_networks: bool,
    pub enable_push: bool,
}

impl fmt::Debug for Http2ClientConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Http2ClientConfig")
            .field("max_body_bytes", &self.max_body_bytes)
            .field("max_concurrent_streams", &self.max_concurrent_streams)
            .field("max_header_list_size", &self.max_header_list_size)
            .field("connect_timeout", &self.connect_timeout)
            .field("request_timeout", &self.request_timeout)
            .field("tls_configured", &self.tls_config.is_some())
            .field("allow_private_networks", &self.allow_private_networks)
            .field("enable_push", &self.enable_push)
            .finish()
    }
}

impl Default for Http2ClientConfig {
    fn default() -> Self {
        Self {
            max_body_bytes: DEFAULT_CLIENT_MAX_BODY_BYTES,
            max_concurrent_streams: DEFAULT_CLIENT_MAX_CONCURRENT_STREAMS,
            max_header_list_size: DEFAULT_CLIENT_MAX_HEADER_LIST_SIZE,
            connect_timeout: DEFAULT_CLIENT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_CLIENT_REQUEST_TIMEOUT,
            tls_config: None,
            allow_private_networks: false,
            enable_push: true,
        }
    }
}

impl Http2ClientConfig {
    pub fn with_tls_config(mut self, config: Arc<ClientConfig>) -> Self {
        self.tls_config = Some(config);
        self
    }

    pub fn allow_private_networks(mut self, allow: bool) -> Self {
        self.allow_private_networks = allow;
        self
    }

    pub fn enable_push(mut self, enabled: bool) -> Self {
        self.enable_push = enabled;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Http2Push {
    pub request: Http2Request,
    pub response: Http2Response,
}

pub type Http2PushCallback = Arc<dyn Fn(Http2Push) + Send + Sync + 'static>;

#[derive(Debug)]
pub enum Http2ClientError {
    InvalidUrl(String),
    Resolve(String),
    SsrfBlocked(SocketAddr),
    Io(String),
    Tls(String),
    Protocol(String),
    Timeout,
    BodyTooLarge,
    Closed,
}

impl fmt::Display for Http2ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(message) => write!(f, "invalid HTTP/2 URL: {message}"),
            Self::Resolve(message) => write!(f, "HTTP/2 DNS resolution failed: {message}"),
            Self::SsrfBlocked(address) => {
                write!(f, "HTTP/2 address blocked by SSRF policy: {address}")
            }
            Self::Io(message) => write!(f, "HTTP/2 client I/O error: {message}"),
            Self::Tls(message) => write!(f, "HTTP/2 TLS error: {message}"),
            Self::Protocol(message) => write!(f, "HTTP/2 protocol error: {message}"),
            Self::Timeout => write!(f, "HTTP/2 client operation timed out"),
            Self::BodyTooLarge => write!(
                f,
                "HTTP/2 response or request body exceeds the configured limit"
            ),
            Self::Closed => write!(f, "HTTP/2 client connection is closed"),
        }
    }
}

impl std::error::Error for Http2ClientError {}

#[derive(Clone, Debug)]
struct Http2Endpoint {
    secure: bool,
    scheme: &'static str,
    host: String,
    authority: String,
    port: u16,
}

struct Http2ClientInner {
    endpoint: Http2Endpoint,
    sender: Mutex<client::SendRequest<Bytes>>,
    max_body_bytes: usize,
    request_timeout: Duration,
    push_callback: RwLock<Option<Http2PushCallback>>,
    closed: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct Http2Client {
    inner: Arc<Http2ClientInner>,
}

impl fmt::Debug for Http2Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Http2Client")
            .field("endpoint", &self.inner.endpoint)
            .field("max_body_bytes", &self.inner.max_body_bytes)
            .field("closed", &self.is_closed())
            .finish()
    }
}

impl Http2Client {
    pub async fn connect(url: &str, config: Http2ClientConfig) -> Result<Self, Http2ClientError> {
        let endpoint = parse_http2_endpoint(url)?;
        let address = resolve_http2_endpoint(&endpoint, config.allow_private_networks).await?;
        let tcp = tokio::time::timeout(config.connect_timeout, TcpStream::connect(address))
            .await
            .map_err(|_| Http2ClientError::Timeout)?
            .map_err(|error| Http2ClientError::Io(error.to_string()))?;
        let closed = Arc::new(AtomicBool::new(false));
        let sender = if endpoint.secure {
            let tls_config = match config.tls_config.as_ref() {
                Some(config) => Arc::clone(config),
                None => crate::tls::TlsClientConfig::with_webpki_roots()
                    .with_alpn_protocols(vec![ALPN_HTTP2.to_vec()])
                    .build()
                    .map_err(|error| Http2ClientError::Tls(error.to_string()))?,
            };
            let connector = TlsConnector::from(tls_config);
            let server_name = ServerName::try_from(endpoint.host.clone())
                .map_err(|error| Http2ClientError::Tls(error.to_string()))?;
            let stream =
                tokio::time::timeout(config.connect_timeout, connector.connect(server_name, tcp))
                    .await
                    .map_err(|_| Http2ClientError::Timeout)?
                    .map_err(|error| Http2ClientError::Tls(error.to_string()))?;
            if stream.get_ref().1.alpn_protocol() != Some(ALPN_HTTP2) {
                return Err(Http2ClientError::Protocol(
                    "TLS endpoint did not negotiate ALPN h2".to_string(),
                ));
            }
            start_h2_client(stream, &config, Arc::clone(&closed)).await?
        } else {
            start_h2_client(tcp, &config, Arc::clone(&closed)).await?
        };

        Ok(Self {
            inner: Arc::new(Http2ClientInner {
                endpoint,
                sender: Mutex::new(sender),
                max_body_bytes: config.max_body_bytes,
                request_timeout: config.request_timeout,
                push_callback: RwLock::new(None),
                closed,
            }),
        })
    }

    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    pub fn set_push_callback(&self, callback: Option<Http2PushCallback>) {
        *self
            .inner
            .push_callback
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = callback;
    }

    pub async fn request(&self, request: Http2Request) -> Result<Http2Response, Http2ClientError> {
        if self.is_closed() {
            return Err(Http2ClientError::Closed);
        }
        tokio::time::timeout(self.inner.request_timeout, self.request_inner(request))
            .await
            .map_err(|_| Http2ClientError::Timeout)?
    }

    async fn request_inner(
        &self,
        request: Http2Request,
    ) -> Result<Http2Response, Http2ClientError> {
        if request.body.len() > self.inner.max_body_bytes {
            return Err(Http2ClientError::BodyTooLarge);
        }
        let h2_request = self.build_request(&request)?;
        let body = request.body;
        let sender = {
            self.inner
                .sender
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        };
        let mut sender = sender
            .ready()
            .await
            .map_err(|error| Http2ClientError::Protocol(error.to_string()))?;
        let (mut response_future, send_stream) =
            sender
                .send_request(h2_request, body.is_empty())
                .map_err(|error| Http2ClientError::Protocol(error.to_string()))?;
        if !body.is_empty() {
            send_h2_request_body(send_stream, body).await?;
        }

        let mut push_promises = response_future.push_promises();
        let callback = self
            .inner
            .push_callback
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let max_body_bytes = self.inner.max_body_bytes;
        let mut push_stream_open = true;
        tokio::pin!(response_future);
        let response = loop {
            if !push_stream_open {
                break response_future.await;
            }
            tokio::select! {
                result = &mut response_future => break result,
                push = push_promises.push_promise() => match push {
                    Some(Ok(promise)) => spawn_h2_push(promise, callback.clone(), max_body_bytes),
                    Some(Err(_)) | None => push_stream_open = false,
                },
            }
        }
        .map_err(|error| Http2ClientError::Protocol(error.to_string()))?;
        if push_stream_open {
            tokio::spawn(collect_h2_pushes(push_promises, callback, max_body_bytes));
        }
        let (parts, body) = response.into_parts();
        let body = collect_h2_body(body, self.inner.max_body_bytes).await?;
        Ok(http2_response_from_h2(parts, body))
    }

    fn build_request(&self, request: &Http2Request) -> Result<H2Request<()>, Http2ClientError> {
        let method = Method::from_bytes(request.method.as_bytes())
            .map_err(|error| Http2ClientError::InvalidUrl(error.to_string()))?;
        let target = if request.target.is_empty() {
            "/"
        } else if request.target.starts_with('/') {
            request.target.as_str()
        } else {
            return Err(Http2ClientError::InvalidUrl(
                "HTTP/2 request target must be an absolute path".to_string(),
            ));
        };
        let uri = format!(
            "{}://{}{}",
            self.inner.endpoint.scheme, self.inner.endpoint.authority, target
        );
        let mut builder = H2Request::builder()
            .method(method)
            .version(Version::HTTP_2)
            .uri(uri);
        for header in &request.headers {
            if header.name.starts_with(':') || header.name.eq_ignore_ascii_case("host") {
                continue;
            }
            if is_forbidden_http2_header(&header.name) {
                continue;
            }
            let name = http::header::HeaderName::try_from(header.name.as_str())
                .map_err(|error| Http2ClientError::InvalidUrl(error.to_string()))?;
            let value = http::header::HeaderValue::try_from(header.value.as_str())
                .map_err(|error| Http2ClientError::InvalidUrl(error.to_string()))?;
            builder = builder.header(name, value);
        }
        builder
            .body(())
            .map_err(|error| Http2ClientError::InvalidUrl(error.to_string()))
    }
}

fn spawn_h2_push(
    promise: client::PushPromise,
    callback: Option<Http2PushCallback>,
    max_body_bytes: usize,
) {
    tokio::spawn(async move {
        let (request, response_future) = promise.into_parts();
        let Ok(response) = response_future.await else {
            return;
        };
        let (parts, body) = response.into_parts();
        let Ok(body) = collect_h2_body(body, max_body_bytes).await else {
            return;
        };
        let request = http2_request_from_h2(request);
        let response = http2_response_from_h2(parts, body);
        if let Some(callback) = callback {
            callback(Http2Push { request, response });
        }
    });
}

async fn collect_h2_pushes(
    mut push_promises: client::PushPromises,
    callback: Option<Http2PushCallback>,
    max_body_bytes: usize,
) {
    while let Some(result) = push_promises.push_promise().await {
        let Ok(promise) = result else { break };
        spawn_h2_push(promise, callback.clone(), max_body_bytes);
    }
}

async fn start_h2_client<I>(
    io: I,
    config: &Http2ClientConfig,
    closed: Arc<AtomicBool>,
) -> Result<client::SendRequest<Bytes>, Http2ClientError>
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut builder = client::Builder::new();
    builder.initial_max_send_streams(config.max_concurrent_streams);
    builder.max_header_list_size(config.max_header_list_size);
    builder.enable_push(config.enable_push);
    let (sender, connection) = builder
        .handshake(io)
        .await
        .map_err(|error| Http2ClientError::Protocol(error.to_string()))?;
    tokio::spawn(async move {
        let _ = connection.await;
        closed.store(true, Ordering::Release);
    });
    Ok(sender)
}

async fn send_h2_request_body(
    mut stream: h2::SendStream<Bytes>,
    body: Vec<u8>,
) -> Result<(), Http2ClientError> {
    let mut offset = 0;
    while offset < body.len() {
        let requested = (body.len() - offset).min(CLIENT_BODY_CHUNK_BYTES);
        stream.reserve_capacity(requested);
        let capacity = poll_fn(|context| stream.poll_capacity(context))
            .await
            .ok_or(Http2ClientError::Closed)?
            .map_err(|error| Http2ClientError::Protocol(error.to_string()))?;
        if capacity == 0 {
            continue;
        }
        let length = capacity.min(body.len() - offset).min(requested);
        let end_stream = offset + length == body.len();
        stream
            .send_data(
                Bytes::copy_from_slice(&body[offset..offset + length]),
                end_stream,
            )
            .map_err(|error| Http2ClientError::Protocol(error.to_string()))?;
        offset += length;
    }
    Ok(())
}

async fn collect_h2_body(
    mut body: h2::RecvStream,
    max_body_bytes: usize,
) -> Result<Vec<u8>, Http2ClientError> {
    let mut output = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk.map_err(|error| Http2ClientError::Protocol(error.to_string()))?;
        if output.len().saturating_add(chunk.len()) > max_body_bytes {
            return Err(Http2ClientError::BodyTooLarge);
        }
        output.extend_from_slice(&chunk);
        body.flow_control()
            .release_capacity(chunk.len())
            .map_err(|error| Http2ClientError::Protocol(error.to_string()))?;
    }
    Ok(output)
}

fn http2_response_from_h2(parts: http::response::Parts, body: Vec<u8>) -> Http2Response {
    let headers = parts
        .headers
        .iter()
        .filter_map(|(name, value)| {
            Some(Http2Header {
                name: name.as_str().to_string(),
                value: value.to_str().ok()?.to_string(),
            })
        })
        .collect();
    Http2Response {
        status_code: parts.status.as_u16(),
        headers,
        body,
    }
}

fn http2_request_from_h2(request: H2Request<()>) -> Http2Request {
    let (parts, _) = request.into_parts();
    let headers = parts
        .headers
        .iter()
        .filter_map(|(name, value)| {
            Some(Http2Header {
                name: name.as_str().to_string(),
                value: value.to_str().ok()?.to_string(),
            })
        })
        .collect();
    let target = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());
    Http2Request {
        method: parts.method.to_string(),
        target,
        headers,
        body: Vec::new(),
    }
}

fn parse_http2_endpoint(url: &str) -> Result<Http2Endpoint, Http2ClientError> {
    let (secure, scheme, remainder) = if let Some(value) = url.strip_prefix("https://") {
        (true, "https", value)
    } else if let Some(value) = url.strip_prefix("http://") {
        (false, "http", value)
    } else {
        return Err(Http2ClientError::InvalidUrl(
            "only http:// and https:// are supported".to_string(),
        ));
    };
    let authority_end = remainder.find('/').unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return Err(Http2ClientError::InvalidUrl(
            "authority must contain a host and no userinfo".to_string(),
        ));
    }
    let (host, port) = if let Some(ipv6_end) = authority.find(']') {
        if !authority.starts_with('[') {
            return Err(Http2ClientError::InvalidUrl(
                "IPv6 authorities must use brackets".to_string(),
            ));
        }
        let host = authority[1..ipv6_end].to_string();
        let port = authority
            .get(ipv6_end + 1..)
            .filter(|suffix| !suffix.is_empty())
            .map(|suffix| {
                suffix
                    .strip_prefix(':')
                    .ok_or_else(|| Http2ClientError::InvalidUrl("invalid IPv6 port".to_string()))
            })
            .transpose()?
            .map(|value| {
                value
                    .parse::<u16>()
                    .map_err(|error| Http2ClientError::InvalidUrl(format!("invalid port: {error}")))
            })
            .transpose()?
            .unwrap_or(if secure { 443 } else { 80 });
        (host, port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if host.is_empty() || port.is_empty() {
            return Err(Http2ClientError::InvalidUrl(
                "authority contains an empty host or port".to_string(),
            ));
        }
        let port = port
            .parse::<u16>()
            .map_err(|error| Http2ClientError::InvalidUrl(format!("invalid port: {error}")))?;
        (host.to_string(), port)
    } else {
        (authority.to_string(), if secure { 443 } else { 80 })
    };
    if host.is_empty() || host.contains('/') || host.contains('#') {
        return Err(Http2ClientError::InvalidUrl("invalid host".to_string()));
    }
    Ok(Http2Endpoint {
        secure,
        scheme,
        host,
        authority: authority.to_string(),
        port,
    })
}

async fn resolve_http2_endpoint(
    endpoint: &Http2Endpoint,
    allow_private_networks: bool,
) -> Result<SocketAddr, Http2ClientError> {
    let addresses = lookup_host((endpoint.host.as_str(), endpoint.port))
        .await
        .map_err(|error| Http2ClientError::Resolve(error.to_string()))?;
    let mut first_blocked = None;
    for address in addresses {
        if allow_private_networks || !crate::security::is_private_or_link_local(address.ip()) {
            return Ok(address);
        }
        first_blocked.get_or_insert(address);
    }
    Err(Http2ClientError::SsrfBlocked(first_blocked.unwrap_or_else(
        || SocketAddr::from(([0, 0, 0, 0], endpoint.port)),
    )))
}

impl fmt::Debug for Http2Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Http2Server")
            .field("local_addr", &self.local_addr)
            .field("stats", &self.stats())
            .finish()
    }
}

impl Http2Server {
    pub fn start(config: Http2Config, handler: Http2Handler) -> Result<Self, Http2Error> {
        let listener = TcpListener::bind(config.bind_addr)?;
        listener.set_nonblocking(true)?;
        let local_addr = listener.local_addr()?;
        let (shutdown, receiver) = watch::channel(false);
        let stopped = Arc::new(AtomicBool::new(false));
        let loop_stopped = Arc::clone(&stopped);
        let stats = Arc::new(Mutex::new(Http2Stats::default()));
        let loop_stats = Arc::clone(&stats);
        let join = thread::Builder::new()
            .name("spectra-api-http2-server".to_string())
            .spawn(move || {
                let runtime = match Builder::new_current_thread()
                    .enable_io()
                    .enable_time()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(_) => {
                        loop_stopped.store(true, Ordering::SeqCst);
                        return;
                    }
                };
                runtime.block_on(run_accept_loop(
                    listener,
                    config,
                    handler,
                    receiver,
                    loop_stopped,
                    loop_stats,
                ));
            })?;
        Ok(Self {
            local_addr,
            shutdown,
            stopped,
            stats,
            join: Some(join),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn stats(&self) -> Http2Stats {
        self.stats
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn shutdown(&mut self) -> Result<Http2Stats, Http2Error> {
        if self.join.is_none() {
            return Err(Http2Error::AlreadyStopped);
        }
        let _ = self.shutdown.send(true);
        if let Some(join) = self.join.take() {
            join.join()
                .map_err(|_| Http2Error::Runtime("HTTP/2 server thread panicked".to_string()))?;
        }
        self.stopped.store(true, Ordering::SeqCst);
        Ok(self.stats())
    }
}

impl Drop for Http2Server {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

async fn run_accept_loop(
    listener: TcpListener,
    config: Http2Config,
    handler: Http2Handler,
    mut shutdown: watch::Receiver<bool>,
    stopped: Arc<AtomicBool>,
    stats: Arc<Mutex<Http2Stats>>,
) {
    let listener = match TokioTcpListener::from_std(listener) {
        Ok(listener) => listener,
        Err(_) => {
            stopped.store(true, Ordering::SeqCst);
            return;
        }
    };
    let tls_acceptor = config
        .tls_config
        .as_ref()
        .map(|config| TlsAcceptor::from(Arc::clone(config)));
    let mut connections = JoinSet::new();

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _peer)) = accepted else { continue };
                stats.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).accepted_connections += 1;
                stats.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).active_connections += 1;
                let handler = Arc::clone(&handler);
                let tls_acceptor = tls_acceptor.clone();
                let connection_stats = Arc::clone(&stats);
                let max_body_bytes = config.max_body_bytes;
                let max_concurrent_streams = config.max_concurrent_streams;
                let max_header_list_size = config.max_header_list_size;
                connections.spawn(async move {
                    if let Some(acceptor) = tls_acceptor {
                        if let Ok(tls_stream) = acceptor.accept(stream).await {
                            if tls_stream
                                .get_ref()
                                .1
                                .alpn_protocol()
                                .is_none_or(|protocol| protocol != ALPN_HTTP2)
                            {
                                return;
                            }
                            serve_h2_connection(
                                tls_stream,
                                handler,
                                max_body_bytes,
                                max_concurrent_streams,
                                max_header_list_size,
                                Arc::clone(&connection_stats),
                            )
                            .await;
                        }
                    } else {
                        serve_h2_connection(
                            stream,
                            handler,
                            max_body_bytes,
                            max_concurrent_streams,
                            max_header_list_size,
                            Arc::clone(&connection_stats),
                        ).await;
                    }
                    let mut stats = connection_stats
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    stats.active_connections = stats.active_connections.saturating_sub(1);
                });
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
    }

    let deadline = tokio::time::sleep(config.shutdown_grace_period);
    tokio::pin!(deadline);
    while !connections.is_empty() {
        tokio::select! {
            joined = connections.join_next() => {
                if joined.is_none() { break; }
            }
            _ = &mut deadline => {
                connections.abort_all();
                break;
            }
        }
    }
    stats
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .active_connections = 0;
    stopped.store(true, Ordering::SeqCst);
}

async fn serve_h2_connection<I>(
    io: I,
    handler: Http2Handler,
    max_body_bytes: usize,
    max_concurrent_streams: u32,
    max_header_list_size: u32,
    stats: Arc<Mutex<Http2Stats>>,
) where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut builder = server::Builder::new();
    builder.max_concurrent_streams(max_concurrent_streams);
    builder.max_header_list_size(max_header_list_size);
    let Ok(mut connection) = builder.handshake(io).await else {
        return;
    };
    let mut streams = JoinSet::new();
    while let Some(result) = connection.accept().await {
        let Ok((request, respond)) = result else {
            break;
        };
        let handler = Arc::clone(&handler);
        let stream_stats = Arc::clone(&stats);
        streams.spawn(async move {
            serve_h2_stream(request, respond, handler, max_body_bytes, stream_stats).await;
        });
    }
    while streams.join_next().await.is_some() {}
}

async fn serve_h2_stream(
    request: H2Request<h2::RecvStream>,
    mut respond: SendResponse<Bytes>,
    handler: Http2Handler,
    max_body_bytes: usize,
    stats: Arc<Mutex<Http2Stats>>,
) {
    let (parts, mut body_stream) = request.into_parts();
    let mut body = Vec::new();
    while let Some(result) = body_stream.data().await {
        let Ok(chunk) = result else {
            return;
        };
        if body.len().saturating_add(chunk.len()) > max_body_bytes {
            let response = H2Response::builder()
                .status(413)
                .body(())
                .expect("HTTP/2 413 response is valid");
            let _ = respond.send_response(response, true);
            stats
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .rejected_streams += 1;
            return;
        }
        body.extend_from_slice(&chunk);
        let _ = body_stream.flow_control().release_capacity(chunk.len());
    }

    let headers = parts
        .headers
        .iter()
        .filter_map(|(name, value)| {
            Some(Http2Header {
                name: name.as_str().to_string(),
                value: value.to_str().ok()?.to_string(),
            })
        })
        .collect::<Vec<_>>();
    let target = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());
    let request = Http2Request {
        method: parts.method.to_string(),
        target,
        headers,
        body,
    };
    let response = handler(request);
    let status = http::StatusCode::from_u16(response.status_code)
        .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);
    let mut builder = H2Response::builder().status(status);
    for header in response.headers {
        if is_forbidden_http2_header(&header.name) {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            http::header::HeaderName::try_from(header.name),
            http::header::HeaderValue::try_from(header.value),
        ) {
            builder = builder.header(name, value);
        }
    }
    let body = response.body;
    let end_stream = body.is_empty();
    let response = match builder.body(()) {
        Ok(response) => response,
        Err(_) => return,
    };
    let Ok(mut sender) = respond.send_response(response, end_stream) else {
        return;
    };
    if !end_stream && sender.send_data(Bytes::from(body), true).is_err() {
        return;
    }
    stats
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .completed_streams += 1;
}

fn is_forbidden_http2_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade"
    )
}

// ---------------------------------------------------------------------------
// TLS gateway: ALPN fan-out listener for TLS-terminated servers.
//
// Topology: `h2` is tokio-based while the HTTP/1.1 pipeline is a mio state
// machine that cannot wrap TLS streams. A TLS-enabled `HttpServer` therefore
// keeps serving cleartext HTTP/1.1 from its mio loop on `bind_addr` and starts
// this gateway on an additional OS-assigned port on the same host. The gateway
// terminates TLS, inspects the negotiated ALPN protocol, and fans out:
//
//   * `h2`           -> HTTP/2 pipeline (`serve_gateway_h2_connection`);
//                      routed SSE responses stream as DATA frames, while
//                      RFC 8441 WebSocket-over-h2 is refused (501) because
//                      extended CONNECT upgrades are not implemented
//   * `http/1.1`/none -> async HTTP/1.1 loop over the TLS stream that calls
//                        the same dispatcher chain as the mio event loop;
//                        routed SSE streams chunked events + heartbeats and
//                        WebSocket upgrades complete the 101 handshake and
//                        pump bidirectional frames, all over this leg
//
// Everything else (sync and async handlers included) behaves like the
// cleartext pipeline on both legs.
// ---------------------------------------------------------------------------

pub(crate) struct TlsGatewayOptions {
    pub(crate) parser_config: crate::http::ParserConfig,
    pub(crate) read_timeout: Duration,
    pub(crate) shutdown_grace_period: Duration,
}

pub(crate) struct TlsGateway {
    local_addr: SocketAddr,
    shutdown: watch::Sender<bool>,
    join: Option<JoinHandle<()>>,
}

impl TlsGateway {
    pub(crate) fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub(crate) fn shutdown(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = self.shutdown.send(true);
            let _ = join.join();
        }
    }
}

impl Drop for TlsGateway {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub(crate) fn spawn_tls_gateway(
    listener: TcpListener,
    certificates: Arc<crate::tls::TlsCertificateStore>,
    options: TlsGatewayOptions,
    dispatcher: crate::server::DispatchHandler,
) -> Result<TlsGateway, io::Error> {
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;
    let (shutdown, receiver) = watch::channel(false);
    let join = thread::Builder::new()
        .name("spectra-api-tls-gateway".to_string())
        .spawn(move || {
            let Ok(runtime) = Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
            else {
                return;
            };
            runtime.block_on(run_tls_gateway_loop(
                listener,
                certificates,
                options,
                dispatcher,
                receiver,
            ));
        })?;
    Ok(TlsGateway {
        local_addr,
        shutdown,
        join: Some(join),
    })
}

async fn run_tls_gateway_loop(
    listener: TcpListener,
    certificates: Arc<crate::tls::TlsCertificateStore>,
    options: TlsGatewayOptions,
    dispatcher: crate::server::DispatchHandler,
    mut shutdown: watch::Receiver<bool>,
) {
    let Ok(listener) = TokioTcpListener::from_std(listener) else {
        return;
    };
    let mut connections = JoinSet::new();

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _peer)) = accepted else { continue };
                let task_certificates = Arc::clone(&certificates);
                let task_dispatcher = Arc::clone(&dispatcher);
                let parser_config = options.parser_config.clone();
                let read_timeout = options.read_timeout;
                connections.spawn(async move {
                    // Read the certificate store per connection so rotations
                    // apply to subsequent handshakes.
                    let Ok(tls_config) = task_certificates.current() else {
                        return;
                    };
                    let acceptor = TlsAcceptor::from(tls_config);
                    let Ok(tls_stream) = acceptor.accept(stream).await else {
                        return;
                    };
                    match tls_stream.get_ref().1.alpn_protocol() {
                        Some(protocol) if protocol == ALPN_HTTP2 => {
                            serve_gateway_h2_connection(
                                tls_stream,
                                task_dispatcher,
                                read_timeout,
                            )
                            .await;
                        }
                        _ => {
                            serve_http11_over_tls(
                                tls_stream,
                                task_dispatcher,
                                parser_config,
                                read_timeout,
                            )
                            .await;
                        }
                    }
                });
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
    }

    let deadline = tokio::time::sleep(options.shutdown_grace_period);
    tokio::pin!(deadline);
    while !connections.is_empty() {
        tokio::select! {
            joined = connections.join_next() => {
                if joined.is_none() { break; }
            }
            _ = &mut deadline => {
                connections.abort_all();
                break;
            }
        }
    }
}

async fn serve_gateway_h2_connection<I>(
    io: I,
    dispatcher: crate::server::DispatchHandler,
    read_timeout: Duration,
) where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut builder = h2::server::Builder::new();
    builder.max_concurrent_streams(DEFAULT_MAX_CONCURRENT_STREAMS);
    builder.max_header_list_size(DEFAULT_MAX_HEADER_LIST_SIZE);
    let Ok(mut connection) = builder.handshake(io).await else {
        return;
    };
    let max_body_bytes = DEFAULT_MAX_BODY_BYTES;
    let mut streams = JoinSet::new();
    while let Some(result) = connection.accept().await {
        let Ok((request, mut respond)) = result else {
            break;
        };
        let dispatcher = Arc::clone(&dispatcher);
        streams.spawn(async move {
            let (parts, body_stream) = request.into_parts();
            match collect_h2_stream_body(body_stream, max_body_bytes).await {
                Ok(body) => {
                    let parsed = gateway_parsed_request(parts, body);
                    match resolve_dispatch_result(&dispatcher, parsed.clone(), read_timeout).await
                    {
                        Http2Outcome::Ready(response) => {
                            send_gateway_h2_response(
                                respond,
                                http2_response_from_server(response),
                            );
                        }
                        Http2Outcome::Sse(route) => {
                            stream_routed_sse_over_h2(respond, route, parsed).await;
                        }
                        Http2Outcome::WebSocket(state) => {
                            stream_routed_websocket_over_h2(respond, state, parsed).await;
                        }
                    }
                }
                Err(()) => {
                    let response = H2Response::builder()
                        .status(413)
                        .body(())
                        .expect("HTTP/2 413 response is valid");
                    let _ = respond.send_response(response, true);
                }
            }
        });
    }
    while streams.join_next().await.is_some() {}
}

async fn collect_h2_stream_body(
    mut body_stream: h2::RecvStream,
    max_body_bytes: usize,
) -> Result<Vec<u8>, ()> {
    let mut body = Vec::new();
    while let Some(result) = body_stream.data().await {
        let chunk = result.map_err(|_| ())?;
        if body.len().saturating_add(chunk.len()) > max_body_bytes {
            return Err(());
        }
        body.extend_from_slice(&chunk);
        let _ = body_stream.flow_control().release_capacity(chunk.len());
    }
    Ok(body)
}

/// Cadence at which the gateway legs drain routed SSE subscribers; events
/// and heartbeats are queued by publishers and by `poll_into` itself, so a
/// short poll keeps latency low without busy-spinning.
const SSE_GATEWAY_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Builds the gateway's internal request from an h2 stream head.
fn gateway_parsed_request(
    parts: http::request::Parts,
    body: Vec<u8>,
) -> crate::http::ParsedRequest {
    crate::http::ParsedRequest {
        method: parts.method.to_string(),
        target: parts
            .uri
            .path_and_query()
            .map(|value| value.as_str().to_string())
            .unwrap_or_else(|| "/".to_string()),
        version: crate::http::HttpVersion::HTTP_11,
        headers: parts
            .headers
            .iter()
            .filter_map(|(name, value)| {
                Some(crate::http::Header {
                    name: name.as_str().to_string(),
                    value: value.to_str().ok()?.to_string(),
                })
            })
            .collect(),
        body: crate::http::HttpBody::from_bytes(body),
        keep_alive: true,
    }
}

fn http2_response_from_server(response: ServerResponse) -> Http2Response {
    Http2Response {
        status_code: response.status_code,
        headers: response
            .headers
            .into_iter()
            .map(|header| Http2Header {
                name: header.name,
                value: header.value,
            })
            .collect(),
        body: response.body.bytes(),
    }
}

/// Streams a routed SSE response over h2: the fixed event-stream headers go
/// out once, then subscriber events plus self-scheduled heartbeat comments
/// are forwarded as DATA frames until the route closes or the peer goes
/// away.
async fn stream_routed_sse_over_h2(
    mut respond: SendResponse<Bytes>,
    route: Arc<crate::sse::RoutedSseResponse>,
    request: crate::http::ParsedRequest,
) {
    let Ok((_, sse)) = route.open(&request) else {
        let response = H2Response::builder()
            .status(http::StatusCode::BAD_REQUEST)
            .body(())
            .expect("HTTP/2 400 response is valid");
        let _ = respond.send_response(response, true);
        return;
    };
    let mut builder = H2Response::builder().status(http::StatusCode::OK);
    for (name, value) in crate::sse::ROUTED_SSE_HEADERS {
        if let (Ok(name), Ok(value)) = (
            http::header::HeaderName::try_from(name),
            http::header::HeaderValue::try_from(value),
        ) {
            builder = builder.header(name, value);
        }
    }
    let Ok(built) = builder.body(()) else {
        return;
    };
    let Ok(mut sender) = respond.send_response(built, false) else {
        return;
    };
    loop {
        let mut chunk = Vec::new();
        let alive = sse.poll_into(std::time::Instant::now(), &mut chunk);
        if !chunk.is_empty() && sender.send_data(Bytes::from(chunk), false).is_err() {
            return;
        }
        if !alive {
            let _ = sender.send_data(Bytes::new(), true);
            return;
        }
        tokio::time::sleep(SSE_GATEWAY_POLL_INTERVAL).await;
    }
}

async fn stream_routed_websocket_over_h2(
    mut respond: SendResponse<Bytes>,
    _state: Arc<crate::websocket::RoutedUpgradeState>,
    _request: crate::http::ParsedRequest,
) {
    // RFC 8441 extended-CONNECT tunneling is not implemented: refuse honestly
    // with 501 instead of faking a 200 OK with an immediately-closed stream.
    let response = H2Response::builder()
        .status(http::StatusCode::NOT_IMPLEMENTED)
        .body(())
        .expect("HTTP/2 501 response is valid");
    let _ = respond.send_response(response, true);
}
/// Dispatcher outcome resolved far enough for a gateway leg to act on it.
enum GatewayOutcome {
    Ready(ServerResponse),
    Sse(Arc<crate::sse::RoutedSseResponse>),
    WebSocket(Arc<crate::websocket::RoutedUpgradeState>),
}

/// Resolves a dispatcher outcome for the TLS gateway's protocol legs,
/// mirroring the mio pipeline: `Ready` passes through, routed SSE and
/// WebSocket handles surface to the leg that owns the connection stream,
/// and `Pending` async tasks are polled to completion exactly like the mio
/// pending-response servicing.
async fn resolve_gateway_outcome(
    dispatcher: &crate::server::DispatchHandler,
    request: crate::http::ParsedRequest,
    read_timeout: Duration,
) -> GatewayOutcome {
    match dispatcher(request) {
        crate::server::HandlerResult::Ready(response) => GatewayOutcome::Ready(response),
        crate::server::HandlerResult::Sse(route) => GatewayOutcome::Sse(route),
        crate::server::HandlerResult::WebSocket(state) => GatewayOutcome::WebSocket(state),
        crate::server::HandlerResult::Pending(pending) => {
            let deadline = std::time::Instant::now() + read_timeout;
            loop {
                match spectra_runtime::stdlib::poll_task_once(pending.task) {
                    Err(_) => {
                        return GatewayOutcome::Ready(crate::server::ServerResponse::text(
                            500,
                            "async handler failed",
                        ))
                    }
                    Ok(true) => break,
                    Ok(false) => {}
                }
                if std::time::Instant::now() >= deadline {
                    let _ = spectra_runtime::stdlib::cancel_task_handle(pending.task);
                    return GatewayOutcome::Ready(crate::server::ServerResponse::text(
                        504,
                        "async handler timeout",
                    ));
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            let response = spectra_runtime::stdlib::task_result_value(pending.task)
                .ok()
                .and_then(crate::http::clone_response)
                .map(crate::server::server_response_from_http);
            GatewayOutcome::Ready(response.unwrap_or_else(|| {
                crate::server::ServerResponse::text(500, "async handler failed")
            }))
        }
    }
}

/// HTTP/2-leg resolution. The h2 responder writes single-shot bodies except
/// for the dedicated SSE streaming path; RFC 8441 WebSocket upgrades have no
/// non-extended-CONNECT mapping and stay refused.
async fn resolve_dispatch_result(
    dispatcher: &crate::server::DispatchHandler,
    request: crate::http::ParsedRequest,
    read_timeout: Duration,
) -> Http2Outcome {
    match resolve_gateway_outcome(dispatcher, request, read_timeout).await {
        GatewayOutcome::Ready(response) => Http2Outcome::Ready(response),
        GatewayOutcome::Sse(route) => Http2Outcome::Sse(route),
        GatewayOutcome::WebSocket(state) => Http2Outcome::WebSocket(state),
    }
}

enum Http2Outcome {
    Ready(ServerResponse),
    Sse(Arc<crate::sse::RoutedSseResponse>),
    WebSocket(Arc<crate::websocket::RoutedUpgradeState>),
}

fn send_gateway_h2_response(mut respond: SendResponse<Bytes>, response: Http2Response) {
    let status = http::StatusCode::from_u16(response.status_code)
        .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);
    let mut builder = H2Response::builder().status(status);
    for header in &response.headers {
        if is_forbidden_http2_header(&header.name) {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            http::header::HeaderName::try_from(header.name.as_str()),
            http::header::HeaderValue::try_from(header.value.as_str()),
        ) {
            builder = builder.header(name, value);
        }
    }
    let body = response.body;
    let end_stream = body.is_empty();
    let built = match builder.body(()) {
        Ok(built) => built,
        Err(_) => return,
    };
    let Ok(mut sender) = respond.send_response(built, end_stream) else {
        return;
    };
    if !end_stream {
        let _ = sender.send_data(Bytes::from(body), true);
    }
}

async fn serve_http11_over_tls<I>(
    mut io: I,
    dispatcher: crate::server::DispatchHandler,
    parser_config: crate::http::ParserConfig,
    read_timeout: Duration,
) where
    I: AsyncRead + AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut parser = crate::http::Http1Parser::request_with_config(parser_config);
    let mut buffer = [0_u8; 8_192];
    loop {
        let request = loop {
            match parser.parse_next_request() {
                Ok(Some(request)) => break Some(request),
                Ok(None) => {}
                Err(_) => {
                    let mut bad_request =
                        crate::server::ServerResponse::text(400, "bad request");
                    bad_request.close = true;
                    let wire = crate::server::response_to_wire(bad_request, false, true);
                    let _ = io.write_all(&wire).await;
                    return;
                }
            }
            let read = tokio::time::timeout(read_timeout, io.read(&mut buffer)).await;
            match read {
                Ok(Ok(0)) | Err(_) | Ok(Err(_)) => return,
                Ok(Ok(read)) => parser.push(&buffer[..read]),
            }
        };
        let Some(request) = request else {
            return;
        };
        let head_only = request.method == "HEAD";
        let keep_alive = request.keep_alive;
        let request_for_stream = request.clone();
        match resolve_gateway_outcome(&dispatcher, request, read_timeout).await {
            GatewayOutcome::Ready(response) => {
                let close = !keep_alive || response.close;
                let wire = crate::server::response_to_wire(response, head_only, close);
                if io.write_all(&wire).await.is_err() {
                    return;
                }
                if close {
                    return;
                }
            }
            GatewayOutcome::WebSocket(state) => {
                serve_websocket_over_tls(io, state, request_for_stream, parser.take_buffered())
                    .await;
                return;
            }
            GatewayOutcome::Sse(route) => {
                // The stream owns the connection from here on: events and
                // heartbeats flow until the route closes or the peer hangs
                // up, so this connection never returns to the keep-alive
                // loop.
                serve_sse_over_tls(io, route, request_for_stream).await;
                return;
            }
        }
    }
}

/// Serves a routed SSE response over the TLS stream: the fixed event-stream
/// head goes out once, then a poll loop drains queued subscriber events plus
/// the heartbeat comments `RoutedSseConnection` schedules itself into chunked
/// writes. (The shared heartbeat driver thread is intentionally not reused:
/// it writes heartbeats synchronously into its own accepted sockets, while a
/// TLS sink must be written through the async runtime that owns it.)
async fn serve_sse_over_tls<I>(
    mut io: I,
    route: Arc<crate::sse::RoutedSseResponse>,
    request: crate::http::ParsedRequest,
) where
    I: AsyncRead + AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;

    let Ok((headers, sse)) = route.open(&request) else {
        let mut response = ServerResponse::text(400, "SSE handshake failed");
        response.close = true;
        let wire = crate::server::response_to_wire(response, false, true);
        let _ = io.write_all(&wire).await;
        return;
    };
    if io.write_all(&headers).await.is_err() {
        return;
    }
    loop {
        let mut chunk = Vec::new();
        let alive = sse.poll_into(std::time::Instant::now(), &mut chunk);
        if !chunk.is_empty() && io.write_all(&chunk).await.is_err() {
            return;
        }
        if !alive {
            return;
        }
        tokio::time::sleep(SSE_GATEWAY_POLL_INTERVAL).await;
    }
}

/// Completes a routed WebSocket upgrade on the TLS stream and pumps frames
/// bidirectionally until the close handshake or an error ends the
/// connection.
async fn serve_websocket_over_tls<I>(
    io: I,
    state: Arc<crate::websocket::RoutedUpgradeState>,
    request: crate::http::ParsedRequest,
    buffered: Vec<u8>,
) where
    I: AsyncRead + AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;

    let config = state.config();
    match crate::websocket::negotiate_upgrade_response(&request, &config) {
        Ok(negotiation) => {
            crate::websocket::serve_tls_upgrade_pump(io, negotiation, config, buffered).await;
        }
        Err(error) => {
            let mut response = ServerResponse::text(400, error.to_string());
            response.close = true;
            let wire = crate::server::response_to_wire(response, false, true);
            let mut io = io;
            let _ = io.write_all(&wire).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use h2::client;
    use http::Request;
    use rcgen::generate_simple_self_signed;
    use rustls::pki_types::ServerName;
    use std::env;
    use std::net::TcpListener as StdTcpListener;
    use tokio::net::TcpStream;
    use tokio::runtime::Builder;
    use tokio_rustls::TlsConnector;

    #[test]
    fn plain_http2_multiplexes_requests_and_round_trips_hpack_headers() {
        let handler: Http2Handler = Arc::new(|request| {
            let value = request
                .headers
                .iter()
                .find(|header| header.name == "x-trace")
                .map(|header| header.value.clone())
                .unwrap_or_default();
            Http2Response {
                status_code: 200,
                headers: vec![Http2Header {
                    name: "x-echo-trace".to_string(),
                    value,
                }],
                body: request.target.into_bytes(),
            }
        });
        let mut server = Http2Server::start(
            Http2Config {
                shutdown_grace_period: Duration::from_millis(100),
                ..Http2Config::default()
            },
            handler,
        )
        .expect("start h2");
        let addr = server.local_addr();
        let runtime = Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("test runtime");
        runtime.block_on(async move {
            let stream = TcpStream::connect(addr).await.expect("connect h2");
            let (mut client, connection) = client::handshake(stream).await.expect("h2 handshake");
            tokio::spawn(async move {
                let _ = connection.await;
            });
            let first = Request::builder()
                .method("GET")
                .uri("/first")
                .header("x-trace", "one")
                .body(())
                .expect("first request");
            let second = Request::builder()
                .method("GET")
                .uri("/second")
                .header("x-trace", "two")
                .body(())
                .expect("second request");
            let (response_one, _) = client.send_request(first, true).expect("first stream");
            let (response_two, _) = client.send_request(second, true).expect("second stream");
            let response_one = response_one.await.expect("first response");
            let response_two = response_two.await.expect("second response");
            assert_eq!(response_one.status(), 200);
            assert_eq!(response_two.status(), 200);
            assert_eq!(response_one.headers()["x-echo-trace"], "one");
            assert_eq!(response_two.headers()["x-echo-trace"], "two");
        });
        let stats = server.shutdown().expect("shutdown h2");
        assert_eq!(stats.completed_streams, 2);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    fn default_tls_alpn_advertises_h2_alongside_http11() {
        let protocols = crate::tls::default_alpn_protocols_for_http2();
        assert_eq!(protocols, vec![b"http/1.1".to_vec(), ALPN_HTTP2.to_vec()]);
    }

    #[test]
    fn client_reuses_one_connection_for_concurrent_requests() {
        let handler: Http2Handler = Arc::new(|request| {
            let trace = request
                .headers
                .iter()
                .find(|header| header.name == "x-trace")
                .map(|header| header.value.clone())
                .unwrap_or_default();
            Http2Response {
                status_code: 200,
                headers: vec![Http2Header {
                    name: "x-echo-trace".to_string(),
                    value: trace,
                }],
                body: request.target.into_bytes(),
            }
        });
        let mut server = Http2Server::start(
            Http2Config {
                shutdown_grace_period: Duration::from_millis(100),
                ..Http2Config::default()
            },
            handler,
        )
        .expect("start HTTP/2 server for client");
        let port = server.local_addr().port();
        let runtime = Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("client test runtime");
        let client = runtime
            .block_on(Http2Client::connect(
                &format!("http://127.0.0.1:{port}"),
                Http2ClientConfig::default().allow_private_networks(true),
            ))
            .expect("connect HTTP/2 client");
        let (first, second) = runtime.block_on(async {
            tokio::join!(
                client.request(Http2Request {
                    method: "GET".to_string(),
                    target: "/first".to_string(),
                    headers: vec![Http2Header {
                        name: "x-trace".to_string(),
                        value: "one".to_string(),
                    }],
                    body: Vec::new(),
                }),
                client.request(Http2Request {
                    method: "GET".to_string(),
                    target: "/second".to_string(),
                    headers: vec![Http2Header {
                        name: "x-trace".to_string(),
                        value: "two".to_string(),
                    }],
                    body: Vec::new(),
                })
            )
        });
        let first = first.expect("first concurrent response");
        let second = second.expect("second concurrent response");
        assert_eq!(first.status_code, 200);
        assert_eq!(second.status_code, 200);
        assert_eq!(first.body, b"/first".to_vec());
        assert_eq!(second.body, b"/second".to_vec());
        assert!(first
            .headers
            .iter()
            .any(|header| header.name == "x-echo-trace" && header.value == "one"));
        assert!(second
            .headers
            .iter()
            .any(|header| header.name == "x-echo-trace" && header.value == "two"));
        drop(client);
        let stats = server.shutdown().expect("shutdown HTTP/2 client server");
        assert_eq!(stats.accepted_connections, 1);
        assert_eq!(stats.completed_streams, 2);
    }

    #[test]
    fn client_https_negotiates_h2_with_explicit_trust_root() {
        let certified =
            generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()])
                .expect("self-signed HTTP/2 client certificate");
        let server_config = crate::tls::TlsServerConfig::new(
            vec![certified.cert.der().to_vec()],
            certified.key_pair.serialize_der(),
        )
        .build()
        .expect("HTTP/2 client test server TLS config");
        let client_config =
            crate::tls::TlsClientConfig::with_roots(vec![certified.cert.der().to_vec()])
                .with_alpn_protocols(vec![ALPN_HTTP2.to_vec()])
                .build()
                .expect("HTTP/2 client trust-root config");
        let handler: Http2Handler = Arc::new(|request| {
            Http2Response::text(200, format!("{} {}", request.method, request.target))
        });
        let mut server = Http2Server::start(
            Http2Config {
                shutdown_grace_period: Duration::from_millis(100),
                tls_config: Some(server_config),
                ..Http2Config::default()
            },
            handler,
        )
        .expect("start HTTPS HTTP/2 server");
        let port = server.local_addr().port();
        let runtime = Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("HTTPS HTTP/2 client runtime");
        let client = runtime
            .block_on(Http2Client::connect(
                &format!("https://127.0.0.1:{port}"),
                Http2ClientConfig::default()
                    .with_tls_config(client_config)
                    .allow_private_networks(true),
            ))
            .expect("connect HTTPS HTTP/2 client");
        let response = runtime
            .block_on(client.request(Http2Request {
                method: "GET".to_string(),
                target: "/secure-client".to_string(),
                headers: Vec::new(),
                body: Vec::new(),
            }))
            .expect("HTTPS HTTP/2 client response");
        assert_eq!(response.status_code, 200);
        assert_eq!(response.body, b"GET /secure-client".to_vec());
        drop(client);
        let stats = server.shutdown().expect("shutdown HTTPS HTTP/2 server");
        assert_eq!(stats.completed_streams, 1);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    fn client_sends_request_body_with_stream_flow_control() {
        let handler: Http2Handler = Arc::new(|request| Http2Response {
            status_code: 200,
            headers: Vec::new(),
            body: request.body,
        });
        let mut server = Http2Server::start(
            Http2Config {
                shutdown_grace_period: Duration::from_millis(100),
                ..Http2Config::default()
            },
            handler,
        )
        .expect("start HTTP/2 body server");
        let port = server.local_addr().port();
        let runtime = Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("HTTP/2 body client runtime");
        let client = runtime
            .block_on(Http2Client::connect(
                &format!("http://127.0.0.1:{port}"),
                Http2ClientConfig::default().allow_private_networks(true),
            ))
            .expect("connect HTTP/2 body client");
        let body = (0..50_000)
            .map(|value| (value % 251) as u8)
            .collect::<Vec<_>>();
        let response = runtime
            .block_on(client.request(Http2Request {
                method: "POST".to_string(),
                target: "/upload".to_string(),
                headers: vec![Http2Header {
                    name: "content-type".to_string(),
                    value: "application/octet-stream".to_string(),
                }],
                body: body.clone(),
            }))
            .expect("HTTP/2 body response");
        assert_eq!(response.status_code, 200);
        assert_eq!(response.body, body);
        drop(client);
        let stats = server.shutdown().expect("shutdown HTTP/2 body server");
        assert_eq!(stats.completed_streams, 1);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    #[ignore = "requires a known external h2 endpoint in SPECTRA_HTTP2_EXTERNAL_URL"]
    fn known_external_http2_endpoint_round_trips() {
        let url = env::var("SPECTRA_HTTP2_EXTERNAL_URL")
            .expect("SPECTRA_HTTP2_EXTERNAL_URL must contain an https h2 endpoint");
        let runtime = Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("external HTTP/2 client runtime");
        let client = runtime
            .block_on(Http2Client::connect(&url, Http2ClientConfig::default()))
            .expect("connect external HTTP/2 endpoint");
        let response = runtime
            .block_on(client.request(Http2Request {
                method: "GET".to_string(),
                target: "/".to_string(),
                headers: Vec::new(),
                body: Vec::new(),
            }))
            .expect("external HTTP/2 response");
        assert!(
            (200..500).contains(&response.status_code),
            "unexpected external HTTP/2 status {}",
            response.status_code
        );
    }

    #[test]
    fn client_accepts_server_push_and_invokes_callback() {
        let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind push server");
        listener
            .set_nonblocking(true)
            .expect("set push listener nonblocking");
        let port = listener.local_addr().expect("push server address").port();
        let server_thread = std::thread::spawn(move || {
            let runtime = Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
                .expect("push server runtime");
            runtime.block_on(async move {
                let listener = TokioTcpListener::from_std(listener).expect("push tokio listener");
                let (stream, _) = listener.accept().await.expect("accept push client");
                let mut connection = server::handshake(stream)
                    .await
                    .expect("push server handshake");
                let Some(Ok((request, mut respond))) = connection.accept().await else {
                    panic!("push server did not receive request");
                };
                let mut request_body = request.into_body();
                while request_body
                    .data()
                    .await
                    .transpose()
                    .expect("read push request")
                    .is_some()
                {}
                let driver = tokio::spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_millis(500), async {
                        while let Some(result) = connection.accept().await {
                            let _ = result;
                        }
                    })
                    .await;
                });
                let pushed_request = Request::builder()
                    .method("GET")
                    .uri(format!("http://127.0.0.1:{port}/asset.js"))
                    .body(())
                    .expect("push request");
                let mut pushed = respond
                    .push_request(pushed_request)
                    .expect("send push promise");
                let pushed_response = H2Response::builder()
                    .status(200)
                    .header("content-type", "application/javascript")
                    .body(())
                    .expect("pushed response");
                let mut pushed_stream = pushed
                    .send_response(pushed_response, false)
                    .expect("send pushed response");
                pushed_stream
                    .send_data(Bytes::from_static(b"console.log('push');"), true)
                    .expect("send pushed body");
                let response = H2Response::builder()
                    .status(200)
                    .body(())
                    .expect("main push response");
                let mut main_stream = respond
                    .send_response(response, false)
                    .expect("send main push response");
                main_stream
                    .send_data(Bytes::from_static(b"index"), true)
                    .expect("send main push body");
                driver.await.expect("join push server driver");
            });
        });

        let runtime = Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("push client runtime");
        let pushes = Arc::new(Mutex::new(Vec::<Http2Push>::new()));
        let callback_pushes = Arc::clone(&pushes);
        let client = runtime
            .block_on(Http2Client::connect(
                &format!("http://127.0.0.1:{port}"),
                Http2ClientConfig::default().allow_private_networks(true),
            ))
            .expect("connect push client");
        client.set_push_callback(Some(Arc::new(move |push| {
            callback_pushes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(push);
        })));
        let response = runtime
            .block_on(client.request(Http2Request {
                method: "GET".to_string(),
                target: "/index".to_string(),
                headers: Vec::new(),
                body: Vec::new(),
            }))
            .expect("main push response");
        assert_eq!(response.status_code, 200);
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if !pushes
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .is_empty()
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("server push callback");
        });
        let pushes = pushes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(pushes.len(), 1);
        assert_eq!(pushes[0].request.target, "/asset.js");
        assert_eq!(pushes[0].response.body, b"console.log('push');".to_vec());
        drop(client);
        server_thread.join().expect("push server thread");
    }

    #[test]
    fn tls_http2_server_negotiates_h2_and_round_trips_request() {
        let certified =
            generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()])
                .expect("self-signed HTTP/2 certificate");
        let server_config = crate::tls::TlsServerConfig::new(
            vec![certified.cert.der().to_vec()],
            certified.key_pair.serialize_der(),
        )
        .build()
        .expect("HTTP/2 server TLS config");
        let client_config =
            crate::tls::TlsClientConfig::with_roots(vec![certified.cert.der().to_vec()])
                .with_alpn_protocols(vec![ALPN_HTTP2.to_vec()])
                .build()
                .expect("HTTP/2 client TLS config");
        let handler: Http2Handler = Arc::new(|request| {
            Http2Response::text(200, format!("{} {}", request.method, request.target))
        });
        let mut server = Http2Server::start(
            Http2Config {
                shutdown_grace_period: Duration::from_millis(100),
                tls_config: Some(server_config),
                ..Http2Config::default()
            },
            handler,
        )
        .expect("start TLS HTTP/2 server");
        let addr = server.local_addr();
        let runtime = Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let stream = TcpStream::connect(addr).await.expect("connect TLS HTTP/2");
            let connector = TlsConnector::from(client_config);
            let name = ServerName::try_from("localhost").expect("server name");
            let stream = connector
                .connect(name, stream)
                .await
                .expect("TLS HTTP/2 handshake");
            assert_eq!(stream.get_ref().1.alpn_protocol(), Some(ALPN_HTTP2));
            let (mut client, connection) = client::handshake(stream)
                .await
                .expect("HTTP/2 handshake over TLS");
            tokio::spawn(async move {
                let _ = connection.await;
            });
            let request = Request::builder()
                .method("GET")
                .uri("/secure")
                .body(())
                .expect("TLS HTTP/2 request");
            let (response, _) = client
                .send_request(request, true)
                .expect("TLS HTTP/2 stream");
            assert_eq!(response.await.expect("TLS HTTP/2 response").status(), 200);
        });
        let stats = server.shutdown().expect("shutdown TLS HTTP/2");
        assert_eq!(stats.completed_streams, 1);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    fn gateway_h2_leg_refuses_routed_websocket_with_501() {
        // RFC 8441 extended-CONNECT tunneling is not implemented: a request
        // the dispatcher routes to a WebSocket upgrade must be refused
        // honestly instead of faking a 200 OK with an idle stream.
        let websocket_server =
            std::sync::Arc::new(std::sync::Mutex::new(crate::websocket::WebSocketServer::new()));
        let mut router = crate::routing::Router::default();
        let route = router
            .add(crate::routing::RouteMethod::Get, "/socket")
            .expect("h2 WebSocket refusal route");
        crate::websocket::register_server_route(std::sync::Arc::clone(&websocket_server), route)
            .expect("attach h2 WebSocket refusal route");
        let state = crate::websocket::routed_upgrade_for_route(route)
            .expect("refusal route has upgrade state");
        let dispatcher: crate::server::DispatchHandler = std::sync::Arc::new(move |_| {
            crate::server::HandlerResult::WebSocket(std::sync::Arc::clone(&state))
        });

        let runtime = Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("h2 refusal test runtime");
        runtime.block_on(async move {
            let (client_io, server_io) = tokio::io::duplex(65_536);
            let server = tokio::spawn(serve_gateway_h2_connection(
                server_io,
                dispatcher,
                Duration::from_secs(2),
            ));
            let (mut h2_client, h2_connection) =
                client::handshake(client_io).await.expect("h2 handshake");
            tokio::spawn(async move {
                let _ = h2_connection.await;
            });
            let request = Request::builder()
                .method("GET")
                .uri("/socket")
                .body(())
                .expect("h2 refusal request");
            let (response, _) = h2_client
                .send_request(request, true)
                .expect("h2 refusal stream");
            let response = response.await.expect("h2 refusal response");
            assert_eq!(response.status(), 501);
            drop(h2_client);
            let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
        });
    }
}
