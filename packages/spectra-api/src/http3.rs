//! HTTP/3 transport over QUIC.
//!
//! The core owns protocol state only. Host adapters map their dispatcher to [`Http3Handler`]
//! and retain [`Http3Server`]. The h3 connection driver is separate from request tasks, so
//! independent QUIC request streams can make progress concurrently. Bodies and header fields are
//! copied into bounded owned values; no raw protocol pointers cross this API.
//!
//! Integration hooks:
//! * export this module from `lib.rs` behind the `http3` feature;
//! * adapt a host callback to `Http3Handler`;
//! * use [`Http3Client::open_request`] when the body must be sent in multiple DATA chunks.

use bytes::{Buf, Bytes};
use futures_util::future::BoxFuture;
use http::{HeaderMap, Request, StatusCode};
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use std::fmt;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::{JoinHandle, JoinSet};
use tokio::sync::{watch, Mutex};

pub const ALPN_HTTP3: &[u8] = b"h3";
const DEFAULT_MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_HEADER_BYTES: usize = 64 * 1024;
const DEFAULT_MAX_CONCURRENT_STREAMS: u32 = 256;
const DEFAULT_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const BODY_CHUNK_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Http3Header {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Http3Request {
    pub method: String,
    pub target: String,
    pub headers: Vec<Http3Header>,
    pub body: Vec<u8>,
    pub trailers: Vec<Http3Header>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Http3Response {
    pub status_code: u16,
    pub headers: Vec<Http3Header>,
    pub body: Vec<u8>,
    pub trailers: Vec<Http3Header>,
}

impl Http3Response {
    pub fn text(status_code: u16, body: impl Into<String>) -> Self {
        Self {
            status_code,
            headers: vec![Http3Header { name: "content-type".into(), value: "text/plain; charset=utf-8".into() }],
            body: body.into().into_bytes(),
            trailers: Vec::new(),
        }
    }
}

pub type Http3Handler = Arc<dyn Fn(Http3Request) -> BoxFuture<'static, Result<Http3Response, Http3Error>> + Send + Sync + 'static>;
pub type Http3Dispatcher = Http3Handler;

#[derive(Debug)]
pub enum Http3Error {
    Io(String),
    Tls(String),
    Protocol(String),
    InvalidUrl(String),
    InvalidHeader(String),
    BodyTooLarge { limit: usize },
    HeaderTooLarge { limit: usize },
    Timeout,
    Cancelled,
    Closed,
    AlreadyStopped,
    Runtime(String),
}

impl fmt::Display for Http3Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "HTTP/3 I/O error: {e}"),
            Self::Tls(e) => write!(f, "HTTP/3 TLS error: {e}"),
            Self::Protocol(e) => write!(f, "HTTP/3 protocol error: {e}"),
            Self::InvalidUrl(e) => write!(f, "invalid HTTP/3 URL: {e}"),
            Self::InvalidHeader(e) => write!(f, "invalid HTTP/3 header: {e}"),
            Self::BodyTooLarge { limit } => write!(f, "HTTP/3 body exceeds {limit} bytes"),
            Self::HeaderTooLarge { limit } => write!(f, "HTTP/3 headers exceed {limit} bytes"),
            Self::Timeout => f.write_str("HTTP/3 operation timed out"),
            Self::Cancelled => f.write_str("HTTP/3 operation cancelled"),
            Self::Closed => f.write_str("HTTP/3 connection is closed"),
            Self::AlreadyStopped => f.write_str("HTTP/3 server is already stopped"),
            Self::Runtime(e) => write!(f, "HTTP/3 runtime error: {e}"),
        }
    }
}
impl std::error::Error for Http3Error {}

#[derive(Clone, Debug)]
pub struct Http3ServerConfig {
    pub bind_addr: SocketAddr,
    pub tls_config: Option<Arc<ServerConfig>>,
    pub max_body_bytes: usize,
    pub max_header_bytes: usize,
    pub max_concurrent_streams: u32,
    pub shutdown_grace_period: Duration,
}

impl Default for Http3ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:0".parse().expect("valid default address"),
            tls_config: None,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            max_header_bytes: DEFAULT_MAX_HEADER_BYTES,
            max_concurrent_streams: DEFAULT_MAX_CONCURRENT_STREAMS,
            shutdown_grace_period: DEFAULT_SHUTDOWN_GRACE,
        }
    }
}

impl Http3ServerConfig {
    pub fn from_tls(bind_addr: SocketAddr, tls_config: Arc<ServerConfig>) -> Self {
        Self { bind_addr, tls_config: Some(tls_config), ..Self::default() }
    }
    pub fn with_tls_config(mut self, tls_config: Arc<ServerConfig>) -> Self {
        self.tls_config = Some(tls_config);
        self
    }
}

#[derive(Clone, Debug)]
pub struct Http3ClientConfig {
    pub root_certificates: Arc<RootCertStore>,
    pub tls_config: Option<Arc<ClientConfig>>,
    pub local_bind_addr: SocketAddr,
    pub max_body_bytes: usize,
    pub max_header_bytes: usize,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
}

impl Default for Http3ClientConfig {
    fn default() -> Self {
        Self {
            root_certificates: Arc::new(RootCertStore::empty()),
            tls_config: None,
            local_bind_addr: "0.0.0.0:0".parse().expect("valid default address"),
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            max_header_bytes: DEFAULT_MAX_HEADER_BYTES,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }
}

impl Http3ClientConfig {
    pub fn with_root_certificates(mut self, roots: Arc<RootCertStore>) -> Self {
        self.root_certificates = roots;
        self
    }
    pub fn with_tls_config(mut self, config: Arc<ClientConfig>) -> Self {
        self.tls_config = Some(config);
        self
    }
}

fn server_quinn_config(config: &Http3ServerConfig) -> Result<quinn::ServerConfig, Http3Error> {
    let source = config.tls_config.as_ref().ok_or_else(|| Http3Error::Tls("HTTP/3 server certificate/key material is required".into()))?;
    let mut tls = (**source).clone();
    // Do not trust an adapter's ALPN list: HTTP/3 must never be negotiated as another protocol.
    tls.alpn_protocols = vec![ALPN_HTTP3.to_vec()];
    let crypto = QuicServerConfig::try_from(tls).map_err(|e| Http3Error::Tls(e.to_string()))?;
    let mut server = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_bidi_streams(config.max_concurrent_streams.into());
    server.transport_config(Arc::new(transport));
    Ok(server)
}

fn client_quinn_config(config: &Http3ClientConfig) -> Result<quinn::ClientConfig, Http3Error> {
    let mut tls = match &config.tls_config {
        Some(tls) => (**tls).clone(),
        None => ClientConfig::builder().with_root_certificates((*config.root_certificates).clone()).with_no_client_auth(),
    };
    tls.alpn_protocols = vec![ALPN_HTTP3.to_vec()];
    let crypto = QuicClientConfig::try_from(tls).map_err(|e| Http3Error::Tls(e.to_string()))?;
    Ok(quinn::ClientConfig::new(Arc::new(crypto)))
}

fn header_map_to_owned(headers: &HeaderMap, limit: usize) -> Result<Vec<Http3Header>, Http3Error> {
    let mut used = 0usize;
    let mut output = Vec::with_capacity(headers.len());
    for (name, value) in headers {
        let value = value.to_str().map_err(|e| Http3Error::InvalidHeader(e.to_string()))?;
        used = used.saturating_add(name.as_str().len()).saturating_add(value.len());
        if used > limit { return Err(Http3Error::HeaderTooLarge { limit }); }
        output.push(Http3Header { name: name.as_str().to_owned(), value: value.to_owned() });
    }
    Ok(output)
}

fn owned_headers(headers: &[Http3Header], limit: usize) -> Result<HeaderMap, Http3Error> {
    let mut used = 0usize;
    let mut map = HeaderMap::new();
    for header in headers {
        used = used.saturating_add(header.name.len()).saturating_add(header.value.len());
        if used > limit { return Err(Http3Error::HeaderTooLarge { limit }); }
        let name = http::header::HeaderName::from_bytes(header.name.as_bytes()).map_err(|e| Http3Error::InvalidHeader(e.to_string()))?;
        let value = http::header::HeaderValue::from_str(&header.value).map_err(|e| Http3Error::InvalidHeader(e.to_string()))?;
        map.append(name, value);
    }
    Ok(map)
}

async fn collect_server_body(
    stream: &mut h3::server::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>,
    max_body: usize,
    max_headers: usize,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(Vec<u8>, Vec<Http3Header>), Http3Error> {
    let mut body = Vec::new();
    loop {
        let next = tokio::select! {
            changed = cancel.changed() => { if changed.is_ok() && *cancel.borrow() { return Err(Http3Error::Cancelled); } continue },
            value = stream.recv_data() => value.map_err(|e| Http3Error::Protocol(e.to_string()))?,
        };
        let Some(mut chunk) = next else { break };
        let size = chunk.remaining();
        if body.len().saturating_add(size) > max_body { return Err(Http3Error::BodyTooLarge { limit: max_body }); }
        body.extend_from_slice(&chunk.copy_to_bytes(size));
    }
    let trailers = tokio::select! {
        changed = cancel.changed() => { if changed.is_ok() && *cancel.borrow() { return Err(Http3Error::Cancelled); } None },
        value = stream.recv_trailers() => value.map_err(|e| Http3Error::Protocol(e.to_string()))?,
    };
    Ok((body, trailers.map(|h| header_map_to_owned(&h, max_headers)).transpose()?.unwrap_or_default()))
}

async fn collect_client_body(
    stream: &mut h3::client::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>,
    max_body: usize,
    max_headers: usize,
) -> Result<(Vec<u8>, Vec<Http3Header>), Http3Error> {
    let mut body = Vec::new();
    while let Some(mut chunk) = stream.recv_data().await.map_err(|e| Http3Error::Protocol(e.to_string()))? {
        let size = chunk.remaining();
        if body.len().saturating_add(size) > max_body { return Err(Http3Error::BodyTooLarge { limit: max_body }); }
        body.extend_from_slice(&chunk.copy_to_bytes(size));
    }
    let trailers = stream.recv_trailers().await.map_err(|e| Http3Error::Protocol(e.to_string()))?;
    Ok((body, trailers.map(|h| header_map_to_owned(&h, max_headers)).transpose()?.unwrap_or_default()))
}

async fn handle_server_request(
    resolver: h3::server::RequestResolver<h3_quinn::Connection, Bytes>,
    handler: Http3Handler,
    config: Http3ServerConfig,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), Http3Error> {
    let (request, mut stream) = resolver.resolve_request().await.map_err(|e| Http3Error::Protocol(e.to_string()))?;
    let (body, trailers) = match collect_server_body(&mut stream, config.max_body_bytes, config.max_header_bytes, &mut cancel).await {
        Ok(value) => value,
        Err(Http3Error::BodyTooLarge { .. }) => {
            let response = http::Response::builder().status(StatusCode::PAYLOAD_TOO_LARGE).body(()).map_err(|e| Http3Error::Protocol(e.to_string()))?;
            stream.send_response(response).await.map_err(|e| Http3Error::Protocol(e.to_string()))?;
            stream.finish().await.map_err(|e| Http3Error::Protocol(e.to_string()))?;
            return Ok(())
        }
        Err(Http3Error::Cancelled) => {
            stream.stop_stream(h3::error::Code::H3_REQUEST_CANCELLED);
            return Err(Http3Error::Cancelled)
        }
        Err(error) => return Err(error),
    };
    let request = Http3Request {
        method: request.method().as_str().to_owned(),
        target: request.uri().to_string(),
        headers: header_map_to_owned(request.headers(), config.max_header_bytes)?,
        body,
        trailers,
    };
    let response = tokio::select! {
        changed = cancel.changed() => { if changed.is_ok() && *cancel.borrow() { stream.stop_stream(h3::error::Code::H3_REQUEST_CANCELLED); return Err(Http3Error::Cancelled); } return Err(Http3Error::Cancelled) },
        value = (handler)(request) => value?,
    };
    let headers = owned_headers(&response.headers, config.max_header_bytes)?;
    let status = StatusCode::from_u16(response.status_code).map_err(|e| Http3Error::Protocol(e.to_string()))?;
    let mut head = http::Response::builder().status(status).body(()).map_err(|e| Http3Error::Protocol(e.to_string()))?;
    *head.headers_mut() = headers;
    stream.send_response(head).await.map_err(|e| Http3Error::Protocol(e.to_string()))?;
    for chunk in response.body.chunks(BODY_CHUNK_BYTES) { stream.send_data(Bytes::copy_from_slice(chunk)).await.map_err(|e| Http3Error::Protocol(e.to_string()))?; }
    if !response.trailers.is_empty() { stream.send_trailers(owned_headers(&response.trailers, config.max_header_bytes)?).await.map_err(|e| Http3Error::Protocol(e.to_string()))?; }
    stream.finish().await.map_err(|e| Http3Error::Protocol(e.to_string()))?;
    Ok(())
}

async fn run_server_connection(quic: quinn::Connection, handler: Http3Handler, config: Http3ServerConfig, mut shutdown: watch::Receiver<bool>) {
    let mut builder = h3::server::builder();
    builder.max_field_section_size(config.max_header_bytes as u64);
    let mut connection = match builder.build(h3_quinn::Connection::new(quic.clone())).await { Ok(value) => value, Err(_) => return };
    let mut requests = JoinSet::new();
    let mut draining = false;
    loop {
        if draining {
            tokio::select! {
                result = connection.accept() => match result { Ok(Some(resolver)) => { requests.spawn(handle_server_request(resolver, handler.clone(), config.clone(), shutdown.clone())); }, Ok(None) | Err(_) => break },
                _ = requests.join_next(), if !requests.is_empty() => {},
            }
        } else {
            tokio::select! {
                result = connection.accept() => match result { Ok(Some(resolver)) => { requests.spawn(handle_server_request(resolver, handler.clone(), config.clone(), shutdown.clone())); }, Ok(None) | Err(_) => break },
                changed = shutdown.changed() => if changed.is_ok() && *shutdown.borrow() { let _ = connection.shutdown(0).await; draining = true; },
                _ = requests.join_next(), if !requests.is_empty() => {},
            }
        }
    }
    let deadline = tokio::time::sleep(config.shutdown_grace_period);
    tokio::pin!(deadline);
    while !requests.is_empty() { tokio::select! { _ = &mut deadline => { requests.abort_all(); break }, _ = requests.join_next() => {} } }
    quic.close(quinn::VarInt::from_u32(0), b"HTTP/3 server shutdown");
}

pub struct Http3Server {
    endpoint: quinn::Endpoint,
    shutdown: watch::Sender<bool>,
    join: Option<JoinHandle<()>>,
}

impl fmt::Debug for Http3Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_struct("Http3Server").field("local_addr", &self.local_addr()).finish() }
}

impl Http3Server {
    pub fn start(config: Http3ServerConfig, handler: Http3Handler) -> Result<Self, Http3Error> {
        tokio::runtime::Handle::try_current().map_err(|e| Http3Error::Runtime(e.to_string()))?;
        let server_config = server_quinn_config(&config)?;
        let endpoint = quinn::Endpoint::server(server_config, config.bind_addr)
            .map_err(|e| Http3Error::Io(e.to_string()))?;
        let (shutdown, mut receiver) = watch::channel(false);
        let accept_endpoint = endpoint.clone();
        let accept_config = config.clone();
        let join = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    incoming = accept_endpoint.accept() => match incoming {
                        Some(incoming) => match incoming.await {
                            Ok(connection) => {
                                connections.spawn(run_server_connection(
                                    connection,
                                    handler.clone(),
                                    accept_config.clone(),
                                    receiver.clone(),
                                ));
                            }
                            Err(_) => {}
                        },
                        None => break,
                    },
                    changed = receiver.changed() => {
                        if changed.is_err() || *receiver.borrow() { break; }
                    }
                }
            }
            let deadline = tokio::time::sleep(accept_config.shutdown_grace_period);
            tokio::pin!(deadline);
            while !connections.is_empty() {
                tokio::select! {
                    _ = &mut deadline => { connections.abort_all(); break; }
                    _ = connections.join_next() => {}
                }
            }
            accept_endpoint.close(quinn::VarInt::from_u32(0), b"HTTP/3 server shutdown");
            accept_endpoint.wait_idle().await;
        });
        Ok(Self { endpoint, shutdown, join: Some(join) })
    }
    pub fn bind(config: Http3ServerConfig, handler: Http3Handler) -> Result<Self, Http3Error> {
        Self::start(config, handler)
    }

    pub fn bind_addr(&self) -> SocketAddr {
        self.endpoint.local_addr().expect("endpoint remains bound")
    }
    pub fn local_addr(&self) -> SocketAddr { self.bind_addr() }
    pub async fn shutdown(&mut self) -> Result<(), Http3Error> {
        if self.join.is_none() { return Err(Http3Error::AlreadyStopped); }
        self.shutdown.send(true).map_err(|_| Http3Error::Closed)?;
        if let Some(join) = self.join.take() {
            join.await.map_err(|e| Http3Error::Runtime(e.to_string()))?;
        }
        Ok(())
    }
}

impl Drop for Http3Server {
    fn drop(&mut self) {
        self.endpoint.close(quinn::VarInt::from_u32(0), b"HTTP/3 server dropped");
        let _ = self.shutdown.send(true);
        if let Some(join) = self.join.take() { join.abort(); }
    }
}

pub struct Http3Client {
    endpoint: quinn::Endpoint,
    sender: Arc<Mutex<h3::client::SendRequest<h3_quinn::OpenStreams, Bytes>>>,
    shutdown: watch::Sender<bool>,
    driver: Arc<Mutex<Option<JoinHandle<()>>>>,
    config: Http3ClientConfig,
    authority: String,
    closed: Arc<AtomicBool>,
}

impl fmt::Debug for Http3Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_struct("Http3Client").field("authority", &self.authority).field("closed", &self.is_closed()).finish() }
}
fn parse_endpoint(url: &str) -> Result<(String, u16), Http3Error> {
    let rest = url.strip_prefix("https://").ok_or_else(|| Http3Error::InvalidUrl("HTTP/3 requires https://".into()))?;
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        return Err(Http3Error::InvalidUrl("missing host".into()));
    }
    // Bracketed IPv6 literal with an optional :port, defaulting to 443 like
    // the HTTP/2 endpoint parser.
    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']').ok_or_else(|| Http3Error::InvalidUrl("invalid IPv6 authority".into()))?;
        let host = &rest[..end];
        if host.is_empty() {
            return Err(Http3Error::InvalidUrl("missing host".into()));
        }
        let port = match rest[end + 1..].strip_prefix(':') {
            None if rest.len() == end + 1 => 443,
            Some(digits) if !digits.is_empty() => digits
                .parse::<u16>()
                .map_err(|error| Http3Error::InvalidUrl(error.to_string()))?,
            _ => return Err(Http3Error::InvalidUrl("invalid IPv6 port".into())),
        };
        return Ok((host.to_string(), port));
    }
    match authority.rsplit_once(':') {
        Some((host, digits)) if !host.is_empty() && !digits.is_empty() => {
            let port = digits
                .parse::<u16>()
                .map_err(|error| Http3Error::InvalidUrl(error.to_string()))?;
            Ok((host.to_string(), port))
        }
        Some(_) => Err(Http3Error::InvalidUrl("authority contains an empty host or port".into())),
        None => Ok((authority.to_string(), 443)),
    }
}

impl Http3Client {
    pub async fn connect(url: &str, config: Http3ClientConfig) -> Result<Self, Http3Error> {
        let (host, port) = parse_endpoint(url)?;
        let addresses = tokio::time::timeout(config.connect_timeout, tokio::net::lookup_host((host.as_str(), port)))
            .await
            .map_err(|_| Http3Error::Timeout)?
            .map_err(|e| Http3Error::Io(e.to_string()))?
            .collect::<Vec<_>>();
        let address = addresses
            .iter()
            .copied()
            .find(|address| address.is_ipv4() == config.local_bind_addr.is_ipv4())
            .ok_or_else(|| Http3Error::InvalidUrl("host has no address matching the local IP family".into()))?;
        let mut endpoint = quinn::Endpoint::client(config.local_bind_addr).map_err(|e| Http3Error::Io(e.to_string()))?;
        endpoint.set_default_client_config(client_quinn_config(&config)?);
        let connecting = endpoint.connect(address, &host).map_err(|e| Http3Error::Protocol(e.to_string()))?;
        let quic = tokio::time::timeout(config.connect_timeout, connecting).await.map_err(|_| Http3Error::Timeout)?.map_err(|e| Http3Error::Protocol(e.to_string()))?;
        let mut builder = h3::client::builder();
        builder.max_field_section_size(config.max_header_bytes as u64);
        let (mut connection, sender) = builder.build(h3_quinn::Connection::new(quic)).await.map_err(|e| Http3Error::Protocol(e.to_string()))?;
        let (shutdown, mut receiver) = watch::channel(false);
        let closed = Arc::new(AtomicBool::new(false));
        let driver_closed = closed.clone();
        let driver = tokio::spawn(async move {
            tokio::select! {
                _ = connection.wait_idle() => {},
                changed = receiver.changed() => if changed.is_ok() && *receiver.borrow() { let _ = connection.shutdown(0).await; let _ = connection.wait_idle().await; },
            }
            driver_closed.store(true, Ordering::Release);
        });
        let authority = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
        Ok(Self { endpoint, sender: Arc::new(Mutex::new(sender)), shutdown, driver: Arc::new(Mutex::new(Some(driver))), config, authority, closed })
    }

    pub fn is_closed(&self) -> bool { self.closed.load(Ordering::Acquire) }

    pub async fn open_request(&self, request: Http3Request) -> Result<Http3ClientRequest, Http3Error> {
        if self.is_closed() { return Err(Http3Error::Closed); }
        if request.body.len() > self.config.max_body_bytes { return Err(Http3Error::BodyTooLarge { limit: self.config.max_body_bytes }); }
        let headers = owned_headers(&request.headers, self.config.max_header_bytes)?;
        let target = if request.target.is_empty() { "/" } else { request.target.as_str() };
        let uri = if target.starts_with("https://") { target.to_owned() } else { format!("https://{}{}", self.authority, target) };
        let mut http_request = Request::builder().method(request.method.as_str()).uri(uri).body(()).map_err(|e| Http3Error::InvalidUrl(e.to_string()))?;
        *http_request.headers_mut() = headers;
        let mut sender = self.sender.lock().await.clone();
        let stream = tokio::time::timeout(self.config.request_timeout, sender.send_request(http_request)).await.map_err(|_| Http3Error::Timeout)?.map_err(|e| Http3Error::Protocol(e.to_string()))?;
        Ok(Http3ClientRequest { stream, max_body_bytes: self.config.max_body_bytes, max_header_bytes: self.config.max_header_bytes, timeout: self.config.request_timeout, sent_body: 0 })
    }
    pub async fn request(&self, request: Http3Request) -> Result<Http3Response, Http3Error> {
        let body = request.body.clone();
        let trailers = request.trailers.clone();
        let mut stream = self.open_request(request).await?;
        if !body.is_empty() { stream.send_data(Bytes::from(body)).await?; }
        if trailers.is_empty() {
            stream.finish().await?;
        } else {
            stream.send_trailers(trailers).await?;
            stream.finish().await?;
        }
        stream.recv_response().await
    }

    pub async fn shutdown(&self) -> Result<(), Http3Error> {
        if self.is_closed() { return Ok(()); }
        self.closed.store(true, Ordering::Release);
        let _ = self.shutdown.send(true);
        if let Some(driver) = self.driver.lock().await.take() {
            driver.await.map_err(|e| Http3Error::Runtime(e.to_string()))?;
        }
        self.endpoint.close(quinn::VarInt::from_u32(0), b"HTTP/3 client shutdown");
        self.endpoint.wait_idle().await;
        Ok(())
    }
}
impl Clone for Http3Client {
    fn clone(&self) -> Self { Self { endpoint: self.endpoint.clone(), sender: self.sender.clone(), shutdown: self.shutdown.clone(), driver: self.driver.clone(), config: self.config.clone(), authority: self.authority.clone(), closed: self.closed.clone() } }
}

pub struct Http3ClientRequest {
    stream: h3::client::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>,
    max_body_bytes: usize,
    max_header_bytes: usize,
    timeout: Duration,
    sent_body: usize,
}

impl Http3ClientRequest {
    pub async fn send_data(&mut self, data: Bytes) -> Result<(), Http3Error> {
        let len = data.len();
        if self.sent_body.saturating_add(len) > self.max_body_bytes { self.cancel(); return Err(Http3Error::BodyTooLarge { limit: self.max_body_bytes }); }
        let result = tokio::time::timeout(self.timeout, self.stream.send_data(data)).await;
        match result { Ok(value) => { value.map_err(|e| Http3Error::Protocol(e.to_string()))?; self.sent_body = self.sent_body.saturating_add(len); Ok(()) }, Err(_) => { self.cancel(); Err(Http3Error::Timeout) } }
    }
    pub async fn send_trailers(&mut self, trailers: Vec<Http3Header>) -> Result<(), Http3Error> {
        let trailers = owned_headers(&trailers, self.max_header_bytes)?;
        let result = tokio::time::timeout(self.timeout, self.stream.send_trailers(trailers)).await;
        match result { Ok(value) => value.map_err(|e| Http3Error::Protocol(e.to_string())), Err(_) => { self.cancel(); Err(Http3Error::Timeout) } }
    }
    pub async fn finish(&mut self) -> Result<(), Http3Error> {
        let result = tokio::time::timeout(self.timeout, self.stream.finish()).await;
        match result { Ok(value) => value.map_err(|e| Http3Error::Protocol(e.to_string())), Err(_) => { self.cancel(); Err(Http3Error::Timeout) } }
    }
    pub async fn recv_response(&mut self) -> Result<Http3Response, Http3Error> {
        let result = tokio::time::timeout(self.timeout, self.receive_inner()).await;
        match result { Ok(value) => value, Err(_) => { self.cancel(); Err(Http3Error::Timeout) } }
    }
    async fn receive_inner(&mut self) -> Result<Http3Response, Http3Error> {
        let response = self.stream.recv_response().await.map_err(|e| Http3Error::Protocol(e.to_string()))?;
        let (body, trailers) = collect_client_body(&mut self.stream, self.max_body_bytes, self.max_header_bytes).await?;
        Ok(Http3Response { status_code: response.status().as_u16(), headers: header_map_to_owned(response.headers(), self.max_header_bytes)?, body, trailers })
    }
    pub fn cancel(&mut self) { self.stream.stop_stream(h3::error::Code::H3_REQUEST_CANCELLED); }
}
impl Drop for Http3ClientRequest { fn drop(&mut self) { self.cancel(); } }

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::generate_simple_self_signed;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

    fn tls_pair() -> (Arc<ServerConfig>, Arc<RootCertStore>) {
        let cert = generate_simple_self_signed(vec!["localhost".into()]).expect("certificate");
        let der = cert.cert.der().clone();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()));
        let server = ServerConfig::builder().with_no_client_auth().with_single_cert(vec![der.clone()], key).expect("server config");
        let mut roots = RootCertStore::empty();
        roots.add(CertificateDer::from(der)).expect("root");
        (Arc::new(server), Arc::new(roots))
    }

    #[tokio::test]
    async fn localhost_round_trip_with_split_data_and_trailers() {
        let (tls, roots) = tls_pair();
        let handler: Http3Handler = Arc::new(|request| Box::pin(async move {
            assert_eq!(request.body, b"split");
            assert_eq!(request.trailers[0].name, "x-input");
            Ok(Http3Response { status_code: 200, headers: vec![], body: b"ok".to_vec(), trailers: vec![Http3Header { name: "x-done".into(), value: "yes".into() }] })
        }));
        let mut server = Http3Server::start(Http3ServerConfig::from_tls("127.0.0.1:0".parse().unwrap(), tls), handler).unwrap();
        let client = Http3Client::connect(&format!("https://localhost:{}", server.local_addr().port()), Http3ClientConfig::default().with_root_certificates(roots)).await.unwrap();
        let mut request = client.open_request(Http3Request { method: "POST".into(), target: "/".into(), headers: vec![], body: vec![], trailers: vec![] }).await.unwrap();
        request.send_data(Bytes::from_static(b"sp")).await.unwrap();
        request.send_data(Bytes::from_static(b"lit")).await.unwrap();
        request.send_trailers(vec![Http3Header { name: "x-input".into(), value: "yes".into() }]).await.unwrap();
        request.finish().await.unwrap();
        let response = request.recv_response().await.unwrap();
        assert_eq!(response.body, b"ok");
        assert_eq!(response.trailers[0].value, "yes");
        client.shutdown().await.unwrap();
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_streams_and_oversized_body_are_isolated() {
        let (tls, roots) = tls_pair();
        let handler: Http3Handler = Arc::new(|request| Box::pin(async move {
            if request.target.ends_with("/slow") { tokio::time::sleep(Duration::from_millis(30)).await; }
            Ok(Http3Response::text(200, request.target))
        }));
        let config = Http3ServerConfig { max_body_bytes: 3, ..Http3ServerConfig::from_tls("127.0.0.1:0".parse().unwrap(), tls) };
        let mut server = Http3Server::start(config, handler).unwrap();
        let client = Http3Client::connect(&format!("https://localhost:{}", server.local_addr().port()), Http3ClientConfig { max_body_bytes: 1024, ..Http3ClientConfig::default().with_root_certificates(roots) }).await.unwrap();
        let first = client.request(Http3Request { method: "GET".into(), target: "/slow".into(), headers: vec![], body: vec![], trailers: vec![] });
        let second = client.request(Http3Request { method: "POST".into(), target: "/large".into(), headers: vec![], body: b"1234".to_vec(), trailers: vec![] });
        let (slow, large) = tokio::join!(first, second);
        assert_eq!(slow.unwrap().status_code, 200);
        assert_eq!(large.unwrap().status_code, 413);
        client.shutdown().await.unwrap();
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn timeout_resets_stream_and_server_drains_on_shutdown() {
        let (tls, roots) = tls_pair();
        let handler: Http3Handler = Arc::new(|_| Box::pin(async move {
            futures_util::future::pending::<Result<Http3Response, Http3Error>>().await
        }));
        let config = Http3ServerConfig { shutdown_grace_period: Duration::from_millis(200), ..Http3ServerConfig::from_tls("127.0.0.1:0".parse().unwrap(), tls) };
        let mut server = Http3Server::start(config, handler).unwrap();
        let client = Http3Client::connect(&format!("https://localhost:{}", server.local_addr().port()), Http3ClientConfig { request_timeout: Duration::from_millis(30), ..Http3ClientConfig::default().with_root_certificates(roots) }).await.unwrap();
        let result = client.request(Http3Request { method: "GET".into(), target: "/hang".into(), headers: vec![], body: vec![], trailers: vec![] }).await;
        assert!(matches!(result, Err(Http3Error::Timeout)));
        server.shutdown().await.unwrap();
        client.shutdown().await.unwrap();
    }
    #[test]
    fn parse_endpoint_defaults_to_443_without_port() {
        assert_eq!(
            parse_endpoint("https://example.com/path").expect("default port"),
            ("example.com".to_string(), 443)
        );
        assert_eq!(
            parse_endpoint("https://example.com:8443/path").expect("explicit port"),
            ("example.com".to_string(), 8443)
        );
        assert_eq!(
            parse_endpoint("https://[::1]/path").expect("bracketed IPv6 default port"),
            ("::1".to_string(), 443)
        );
        assert_eq!(
            parse_endpoint("https://[::1]:9443/path").expect("bracketed IPv6 port"),
            ("::1".to_string(), 9443)
        );
        assert!(parse_endpoint("http://example.com/").is_err());
        assert!(parse_endpoint("https:///path").is_err());
        assert!(parse_endpoint("https://example.com:/path").is_err());
    }
}

