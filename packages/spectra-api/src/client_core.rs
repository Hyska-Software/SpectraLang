use crate::http::{self, Header, Http1Parser, HttpBody, ParsedResponse, ParserConfig, Response, Status};
use crate::handles::ApiHandleTable;
use crate::security::SsrfPolicy;
use crate::{read_args, write_result};
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INTERNAL_ERROR,
    HOST_STATUS_INVALID_ARGUMENT,
};
use spectra_runtime::handles::HandleKind;
use std::collections::HashMap;
use spectra_runtime::tracing::{self, SpanKind, SpanStatus};
use std::fmt;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT_MS: SpectraHostValue = 30_000;
const DEFAULT_MAX_REDIRECTS: usize = 10;
const DEFAULT_POOL_IDLE_MS: u64 = 30_000;

#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub timeout: Duration,
    pub max_redirects: usize,
    pub pool_idle_timeout: Duration,
    pub max_header_bytes: usize,
    pub max_body_bytes: usize,
    pub max_chunk_bytes: usize,
    pub user_agent: String,
    pub tls_config: Option<Arc<rustls::ClientConfig>>,
    pub ssrf_policy: SsrfPolicy,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS as u64),
            max_redirects: DEFAULT_MAX_REDIRECTS,
            pool_idle_timeout: Duration::from_millis(DEFAULT_POOL_IDLE_MS),
            max_header_bytes: 64 * 1024,
            max_body_bytes: 16 * 1024 * 1024,
            max_chunk_bytes: 8 * 1024 * 1024,
            user_agent: "spectra-api/0.1".to_string(),
            tls_config: None,
            ssrf_policy: SsrfPolicy::default(),
        }
    }
}

impl ClientConfig {
    fn parser_config(&self) -> ParserConfig {
        ParserConfig {
            max_header_bytes: self.max_header_bytes,
            max_body_bytes: self.max_body_bytes,
            max_chunk_bytes: self.max_chunk_bytes,
        }
    }

    pub fn with_tls_config(mut self, tls_config: Arc<rustls::ClientConfig>) -> Self {
        self.tls_config = Some(tls_config);
        self
    }

    pub fn allow_private_networks(mut self, allow: bool) -> Self {
        self.ssrf_policy = self.ssrf_policy.allow_private_networks(allow);
        self
    }
}

#[derive(Clone, Debug)]
pub struct ClientRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
}

impl ClientRequest {
    pub fn new(method: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push(Header {
            name: name.into(),
            value: value.into(),
        });
        self
    }
}

#[derive(Clone, Debug)]
pub struct ClientResponse {
    pub status_code: u16,
    pub reason: String,
    pub headers: Vec<Header>,
    pub body: HttpBody,
    pub final_url: String,
    pub redirect_count: usize,
    pub keep_alive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientErrorKind {
    InvalidUrl,
    UnsupportedScheme,
    SsrfBlocked,
    ConnectionFailed,
    Timeout,
    Protocol,
    RedirectLimit,
    MissingRedirectLocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientError {
    pub kind: ClientErrorKind,
    pub message: String,
}

impl ClientError {
    fn new(kind: ClientErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ClientError {}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct Authority {
    host: String,
    port: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UrlScheme {
    Http,
    Https,
}

impl UrlScheme {
    fn default_port(self) -> u16 {
        match self {
            Self::Http => 80,
            Self::Https => 443,
        }
    }

    fn is_tls(self) -> bool {
        matches!(self, Self::Https)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }
}

#[derive(Clone, Debug)]
struct ParsedUrl {
    scheme: UrlScheme,
    authority: Authority,
    path_and_query: String,
}

struct PooledConnection {
    stream: TcpStream,
    idle_since: Instant,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ClientStats {
    pub opened_connections: usize,
    pub reused_connections: usize,
    pub pooled_connections: usize,
    pub redirects_followed: usize,
}

pub struct HttpClient {
    config: ClientConfig,
    ssrf_policy: RwLock<SsrfPolicy>,
    pool: Mutex<HashMap<Authority, Vec<PooledConnection>>>,
    stats: Mutex<ClientStats>,
    // Per-client trust-anchor override installed by
    // `spectra.api.client.set_tls_config`. The async bridge prefers it over
    // `config.tls_config` and falls back to webpki roots when unset.
    tls_override: RwLock<Option<Arc<rustls::ClientConfig>>>,
}

impl HttpClient {
    pub fn new(config: ClientConfig) -> Self {
        let ssrf_policy = config.ssrf_policy.clone();
        Self {
            config,
            ssrf_policy: RwLock::new(ssrf_policy),
            pool: Mutex::new(HashMap::new()),
            stats: Mutex::new(ClientStats::default()),
            tls_override: RwLock::new(None),
        }
    }

    /// Installs a per-client TLS trust store, replacing webpki defaults for
    /// subsequent requests. Pooled connections negotiated under the previous
    /// store are dropped so pinned requests never reuse them.
    pub fn set_tls_config(&self, tls_config: Arc<rustls::ClientConfig>) {
        *self
            .tls_override
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(tls_config);
        self.pool
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    pub fn set_ssrf_policy(&self, policy: SsrfPolicy) {
        *self
            .ssrf_policy
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = policy;
        self.pool
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    pub fn get(&self, url: &str) -> Result<ClientResponse, ClientError> {
        self.request(ClientRequest::new("GET", url))
    }

    pub fn head(&self, url: &str) -> Result<ClientResponse, ClientError> {
        self.request(ClientRequest::new("HEAD", url))
    }

    pub fn delete(&self, url: &str) -> Result<ClientResponse, ClientError> {
        self.request(ClientRequest::new("DELETE", url))
    }

    pub fn post(&self, url: &str, body: Vec<u8>) -> Result<ClientResponse, ClientError> {
        self.request(ClientRequest::new("POST", url).with_body(body))
    }

    pub fn put(&self, url: &str, body: Vec<u8>) -> Result<ClientResponse, ClientError> {
        self.request(ClientRequest::new("PUT", url).with_body(body))
    }

    pub fn patch(&self, url: &str, body: Vec<u8>) -> Result<ClientResponse, ClientError> {
        self.request(ClientRequest::new("PATCH", url).with_body(body))
    }

    pub fn stats(&self) -> ClientStats {
        let mut stats = self.stats.lock().unwrap_or_else(|e| e.into_inner()).clone();
        stats.pooled_connections = self
            .pool
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .map(Vec::len)
            .sum();
        stats
    }

    pub fn request(&self, request: ClientRequest) -> Result<ClientResponse, ClientError> {
        let span = tracing::begin_external_span(SpanKind::Client, "http.client").ok();
        if let Some(id) = span {
            let _ = tracing::span_set_attribute(id, "http.request.method", &request.method);
            let _ = tracing::span_set_attribute(id, "url.full", &request.url);
        }
        let mut request = request;
        if let Some(traceparent) = tracing::current_traceparent() {
            upsert_header(&mut request.headers, "traceparent", &traceparent);
        }
        let result = self.request_inner(request);
        if let Some(id) = span {
            if let Ok(response) = &result {
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
            }
            let _ = tracing::span_set_status(
                id,
                if result.is_ok() {
                    SpanStatus::Ok
                } else {
                    SpanStatus::Error
                },
            );
            let _ = tracing::span_end(id);
        }
        result
    }

    fn request_inner(&self, request: ClientRequest) -> Result<ClientResponse, ClientError> {
        let mut current = request;
        for redirect_count in 0..=self.config.max_redirects {
            let parsed_url = parse_http_url(&current.url)?;
            let addresses = self.resolve_destination(&parsed_url.authority)?;
            let (response, reusable_stream) = self.send_once(&current, &parsed_url, &addresses)?;
            if let Some(next_url) = redirect_target(&response, &current.url)? {
                if redirect_count == self.config.max_redirects {
                    return Err(ClientError::new(
                        ClientErrorKind::RedirectLimit,
                        "redirect limit exceeded",
                    ));
                }
                if let Some(stream) = reusable_stream {
                    self.put_connection(parsed_url.authority, stream);
                }
                current = redirected_request(current, next_url, response.status_code)?;
                self.stats
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .redirects_followed += 1;
                continue;
            }

            if let Some(stream) = reusable_stream {
                self.put_connection(parsed_url.authority, stream);
            }
            return Ok(ClientResponse {
                status_code: response.status_code,
                reason: response.reason,
                headers: response.headers,
                body: response.body,
                final_url: current.url,
                redirect_count,
                keep_alive: response.keep_alive,
            });
        }

        Err(ClientError::new(
            ClientErrorKind::RedirectLimit,
            "redirect loop exceeded configured limit",
        ))
    }

    fn send_once(
        &self,
        request: &ClientRequest,
        parsed_url: &ParsedUrl,
        addresses: &[SocketAddr],
    ) -> Result<(ParsedResponse, Option<TcpStream>), ClientError> {
        let mut stream = self.take_or_connect(&parsed_url.authority, addresses)?;
        stream
            .set_read_timeout(Some(self.config.timeout))
            .map_err(|e| io_error(ClientErrorKind::ConnectionFailed, e))?;
        stream
            .set_write_timeout(Some(self.config.timeout))
            .map_err(|e| io_error(ClientErrorKind::ConnectionFailed, e))?;

        let wire = build_request_wire(request, parsed_url, &self.config);
        stream.write_all(&wire).map_err(map_io_error)?;
        stream.flush().map_err(map_io_error)?;

        if request.method.eq_ignore_ascii_case("HEAD") {
            let response = read_head_response(&mut stream, self.config.timeout)?;
            let reusable =
                response.keep_alive && !header_has_token(&response.headers, "connection", "close");
            if reusable {
                return Ok((response, Some(stream)));
            }
            return Ok((response, None));
        }

        let mut parser = Http1Parser::response_with_config(self.config.parser_config());
        let mut buf = [0_u8; 8192];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => {
                    return Err(ClientError::new(
                        ClientErrorKind::Protocol,
                        "connection closed before a complete HTTP response was received",
                    ));
                }
                Ok(n) => {
                    parser.push(&buf[..n]);
                    match parser.parse_next_response() {
                        Ok(Some(response)) => {
                            let reusable = response.keep_alive
                                && !header_has_token(&response.headers, "connection", "close")
                                && parser.buffered_len() == 0;
                            if reusable {
                                return Ok((response, Some(stream)));
                            }
                            return Ok((response, None));
                        }
                        Ok(None) => {}
                        Err(error) => {
                            return Err(ClientError::new(
                                ClientErrorKind::Protocol,
                                format!("invalid HTTP response: {error}"),
                            ));
                        }
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(ClientError::new(
                        ClientErrorKind::Timeout,
                        "HTTP client timed out waiting for response",
                    ));
                }
                Err(error) => return Err(io_error(ClientErrorKind::ConnectionFailed, error)),
            }
        }
    }

    fn take_or_connect(
        &self,
        authority: &Authority,
        addresses: &[SocketAddr],
    ) -> Result<TcpStream, ClientError> {
        self.drop_expired_pool_entries();
        if let Some(stream) = self.take_connection(authority) {
            self.stats
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .reused_connections += 1;
            return Ok(stream);
        }
        let stream = TcpStream::connect(addresses)
            .map_err(|e| io_error(ClientErrorKind::ConnectionFailed, e))?;
        self.stats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .opened_connections += 1;
        Ok(stream)
    }

    fn resolve_destination(&self, authority: &Authority) -> Result<Vec<SocketAddr>, ClientError> {
        let addresses = (authority.host.as_str(), authority.port)
            .to_socket_addrs()
            .map_err(|error| io_error(ClientErrorKind::ConnectionFailed, error))?
            .collect::<Vec<_>>();
        if addresses.is_empty() {
            return Err(ClientError::new(
                ClientErrorKind::ConnectionFailed,
                "URL host did not resolve to an address",
            ));
        }
        let policy = self
            .ssrf_policy
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(blocked) = addresses.iter().find(|address| !policy.allows_address(**address)) {
            return Err(ClientError::new(
                ClientErrorKind::SsrfBlocked,
                format!("SSRF policy rejected resolved address {blocked}"),
            ));
        }
        Ok(addresses)
    }

    fn take_connection(&self, authority: &Authority) -> Option<TcpStream> {
        self.pool
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(authority)
            .and_then(Vec::pop)
            .map(|conn| conn.stream)
    }

    fn put_connection(&self, authority: Authority, stream: TcpStream) {
        let _ = stream.set_read_timeout(None);
        let _ = stream.set_write_timeout(None);
        self.pool
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(authority)
            .or_default()
            .push(PooledConnection {
                stream,
                idle_since: Instant::now(),
            });
    }

    fn drop_expired_pool_entries(&self) {
        let timeout = self.config.pool_idle_timeout;
        let now = Instant::now();
        self.pool
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values_mut()
            .for_each(|connections| {
                connections.retain(|conn| now.duration_since(conn.idle_since) <= timeout);
            });
    }
}

fn read_head_response(
    stream: &mut TcpStream,
    timeout: Duration,
) -> Result<ParsedResponse, ClientError> {
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| io_error(ClientErrorKind::ConnectionFailed, e))?;
    let mut raw = Vec::new();
    let mut buf = [0_u8; 1024];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => {
                return Err(ClientError::new(
                    ClientErrorKind::Protocol,
                    "connection closed before complete HEAD response headers",
                ));
            }
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if let Some(header_end) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                    return parse_head_response_headers(&raw[..header_end]);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Err(ClientError::new(
                    ClientErrorKind::Timeout,
                    "HTTP client timed out waiting for HEAD response",
                ));
            }
            Err(error) => return Err(io_error(ClientErrorKind::ConnectionFailed, error)),
        }
    }
}

fn parse_head_response_headers(raw: &[u8]) -> Result<ParsedResponse, ClientError> {
    let text = std::str::from_utf8(raw).map_err(|_| {
        ClientError::new(
            ClientErrorKind::Protocol,
            "HEAD response headers are not valid UTF-8",
        )
    })?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut parts = status_line.splitn(3, ' ');
    let version_text = parts.next().unwrap_or_default();
    let status_text = parts.next().unwrap_or_default();
    let reason = parts.next().unwrap_or_default().to_string();
    let version = parse_response_version(version_text)?;
    let status_code = status_text.parse::<u16>().map_err(|_| {
        ClientError::new(
            ClientErrorKind::Protocol,
            "HEAD response status code is invalid",
        )
    })?;
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(ClientError::new(
                ClientErrorKind::Protocol,
                "HEAD response header is missing ':'",
            ));
        };
        headers.push(Header {
            name: name.to_string(),
            value: value.trim().to_string(),
        });
    }
    let keep_alive = !header_has_token(&headers, "connection", "close")
        && (version.major > 1
            || (version.major == 1 && version.minor >= 1)
            || header_has_token(&headers, "connection", "keep-alive"));
    Ok(ParsedResponse {
        version,
        status_code,
        reason,
        headers,
        body: HttpBody::empty(),
        keep_alive,
    })
}

fn parse_response_version(text: &str) -> Result<crate::http::HttpVersion, ClientError> {
    let Some(rest) = text.strip_prefix("HTTP/") else {
        return Err(ClientError::new(
            ClientErrorKind::Protocol,
            "response version is missing HTTP/ prefix",
        ));
    };
    let Some((major, minor)) = rest.split_once('.') else {
        return Err(ClientError::new(
            ClientErrorKind::Protocol,
            "response version is malformed",
        ));
    };
    Ok(crate::http::HttpVersion {
        major: major.parse::<u8>().map_err(|_| {
            ClientError::new(
                ClientErrorKind::Protocol,
                "response major version is invalid",
            )
        })?,
        minor: minor.parse::<u8>().map_err(|_| {
            ClientError::new(
                ClientErrorKind::Protocol,
                "response minor version is invalid",
            )
        })?,
    })
}

fn parse_http_url(url: &str) -> Result<ParsedUrl, ClientError> {
    parse_url(url, false)
}

fn parse_url(url: &str, allow_https: bool) -> Result<ParsedUrl, ClientError> {
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("http://") {
        (UrlScheme::Http, rest)
    } else if let Some(rest) = url.strip_prefix("https://") {
        if !allow_https {
            return Err(ClientError::new(
                ClientErrorKind::UnsupportedScheme,
                "HTTPS is only available through the asynchronous TLS client bridge",
            ));
        }
        (UrlScheme::Https, rest)
    } else {
        if url.contains("://") {
            return Err(ClientError::new(
                ClientErrorKind::UnsupportedScheme,
                "URL scheme is not supported by the HTTP client",
            ));
        }
        return Err(ClientError::new(
            ClientErrorKind::InvalidUrl,
            "URL must start with http:// or https://",
        ));
    };

    let (authority_text, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, "/".to_string()),
    };
    if authority_text.is_empty() || authority_text.contains('@') {
        return Err(ClientError::new(
            ClientErrorKind::InvalidUrl,
            "URL authority is invalid",
        ));
    }
    let (host, port) = parse_authority(authority_text, scheme.default_port())?;
    if host.is_empty() {
        return Err(ClientError::new(
            ClientErrorKind::InvalidUrl,
            "URL host is empty",
        ));
    }
    Ok(ParsedUrl {
        scheme,
        authority: Authority { host, port },
        path_and_query: if path.is_empty() {
            "/".to_string()
        } else {
            path
        },
    })
}

fn parse_authority(authority: &str, default_port: u16) -> Result<(String, u16), ClientError> {
    // Bracketed IPv6 literals, mirroring the HTTP/2 endpoint parser: an
    // optional :port follows the bracket, otherwise the scheme default.
    if let Some(ipv6_end) = authority.find(']') {
        if !authority.starts_with('[') {
            return Err(ClientError::new(
                ClientErrorKind::InvalidUrl,
                "IPv6 authorities must use brackets",
            ));
        }
        let host = authority[1..ipv6_end].to_string();
        if host.is_empty() {
            return Err(ClientError::new(
                ClientErrorKind::InvalidUrl,
                "URL host is empty",
            ));
        }
        let port = authority
            .get(ipv6_end + 1..)
            .filter(|suffix| !suffix.is_empty())
            .map(|suffix| {
                suffix.strip_prefix(':').ok_or_else(|| {
                    ClientError::new(ClientErrorKind::InvalidUrl, "invalid IPv6 port")
                })
            })
            .transpose()?
            .map(|value| {
                value.parse::<u16>().map_err(|_| {
                    ClientError::new(ClientErrorKind::InvalidUrl, "URL port is out of range")
                })
            })
            .transpose()?
            .unwrap_or(default_port);
        return Ok((host, port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            let port = port.parse::<u16>().map_err(|_| {
                ClientError::new(ClientErrorKind::InvalidUrl, "URL port is out of range")
            })?;
            Ok((host.to_string(), port))
        }
        Some((_, port)) if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) => Err(
            ClientError::new(ClientErrorKind::InvalidUrl, "URL port is invalid"),
        ),
        _ => Ok((authority.to_string(), default_port)),
    }
}

fn build_request_wire(
    request: &ClientRequest,
    parsed_url: &ParsedUrl,
    config: &ClientConfig,
) -> Vec<u8> {
    let mut headers = request.headers.clone();
    upsert_header(
        &mut headers,
        "Host",
        &host_header_value_for(&parsed_url.authority, parsed_url.scheme),
    );
    upsert_header(&mut headers, "User-Agent", &config.user_agent);
    upsert_header(&mut headers, "Connection", "keep-alive");
    if request.body.is_empty() {
        remove_header(&mut headers, "Content-Length");
    } else {
        upsert_header(
            &mut headers,
            "Content-Length",
            &request.body.len().to_string(),
        );
    }

    let mut out = Vec::new();
    out.extend_from_slice(request.method.as_bytes());
    out.push(b' ');
    out.extend_from_slice(parsed_url.path_and_query.as_bytes());
    out.extend_from_slice(b" HTTP/1.1\r\n");
    for header in &headers {
        out.extend_from_slice(header.name.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(header.value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(&request.body);
    out
}

fn host_header_value_for(authority: &Authority, scheme: UrlScheme) -> String {
    let host = if authority.host.contains(':') {
        format!("[{}]", authority.host)
    } else {
        authority.host.clone()
    };
    if authority.port == scheme.default_port() {
        host
    } else {
        format!("{host}:{}", authority.port)
    }
}

fn redirected_request(
    request: ClientRequest,
    location: String,
    status: u16,
) -> Result<ClientRequest, ClientError> {
    redirected_request_with_options(request, location, status, false)
}

fn redirected_request_with_options(
    mut request: ClientRequest,
    location: String,
    status: u16,
    allow_https: bool,
) -> Result<ClientRequest, ClientError> {
    let original = parse_url(&request.url, allow_https)?;
    request.url = resolve_location_with_options(&request.url, &location, allow_https)?;
    match status {
        301 | 302 => {
            if request.method.eq_ignore_ascii_case("POST") {
                request.method = "GET".to_string();
                request.body.clear();
                remove_header(&mut request.headers, "Content-Length");
            }
        }
        303 => {
            if !request.method.eq_ignore_ascii_case("HEAD") {
                request.method = "GET".to_string();
                request.body.clear();
                remove_header(&mut request.headers, "Content-Length");
            }
        }
        307 | 308 => {}
        _ => {}
    }
    let next = parse_url(&request.url, allow_https)?;
    if next.authority != original.authority || next.scheme != original.scheme {
        remove_header(&mut request.headers, "Host");
    }
    Ok(request)
}

fn redirect_target(
    response: &ParsedResponse,
    current_url: &str,
) -> Result<Option<String>, ClientError> {
    redirect_target_with_options(response, current_url, false)
}

fn redirect_target_with_options(
    response: &ParsedResponse,
    current_url: &str,
    allow_https: bool,
) -> Result<Option<String>, ClientError> {
    if !matches!(response.status_code, 301 | 302 | 303 | 307 | 308) {
        return Ok(None);
    }
    let Some(location) = header_value(&response.headers, "location") else {
        return Err(ClientError::new(
            ClientErrorKind::MissingRedirectLocation,
            "redirect response is missing Location header",
        ));
    };
    resolve_location_with_options(current_url, location, allow_https).map(Some)
}

fn resolve_location_with_options(
    current_url: &str,
    location: &str,
    allow_https: bool,
) -> Result<String, ClientError> {
    if location.starts_with("http://") {
        return Ok(location.to_string());
    }
    if location.starts_with("https://") {
        if allow_https {
            return Ok(location.to_string());
        }
        return Err(ClientError::new(
            ClientErrorKind::UnsupportedScheme,
            "redirect target uses HTTPS but the current client path is not TLS-aware",
        ));
    }
    if location.contains("://") {
        return Err(ClientError::new(
            ClientErrorKind::UnsupportedScheme,
            "redirect target uses an unsupported scheme",
        ));
    }
    let parsed = parse_url(current_url, allow_https)?;
    let scheme = parsed.scheme.as_str();
    if let Some(authority) = location.strip_prefix("//") {
        let candidate = format!("{scheme}://{authority}");
        parse_url(&candidate, allow_https)?;
        return Ok(candidate);
    }
    if location.starts_with('/') {
        return Ok(format!(
            "{scheme}://{}{}",
            host_header_value_for(&parsed.authority, parsed.scheme),
            location
        ));
    }
    let base_path = parsed
        .path_and_query
        .rsplit_once('/')
        .map(|(base, _)| {
            if base.is_empty() {
                "/".to_string()
            } else {
                format!("{base}/")
            }
    })
        .unwrap_or_else(|| "/".to_string());
    Ok(format!(
        "{scheme}://{}{}{}",
        host_header_value_for(&parsed.authority, parsed.scheme),
        base_path,
        location
    ))
}

fn header_value<'a>(headers: &'a [Header], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str())
}

fn header_has_token(headers: &[Header], name: &str, token: &str) -> bool {
    header_value(headers, name)
        .map(|value| {
            value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case(token))
        })
        .unwrap_or(false)
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

fn map_io_error(error: std::io::Error) -> ClientError {
    if matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ) {
        ClientError::new(ClientErrorKind::Timeout, "HTTP client I/O timed out")
    } else {
        io_error(ClientErrorKind::ConnectionFailed, error)
    }
}

fn io_error(kind: ClientErrorKind, error: std::io::Error) -> ClientError {
    ClientError::new(kind, error.to_string())
}
