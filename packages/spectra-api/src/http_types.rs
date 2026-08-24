use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use crate::handles::ApiHandleTable;
use ring::hmac;
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
};
use spectra_runtime::handles::HandleKind;
use std::fmt;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const METHOD_GET: SpectraHostValue = 1;
pub const METHOD_HEAD: SpectraHostValue = 2;
pub const METHOD_POST: SpectraHostValue = 3;
pub const METHOD_PUT: SpectraHostValue = 4;
pub const METHOD_PATCH: SpectraHostValue = 5;
pub const METHOD_DELETE: SpectraHostValue = 6;
pub const METHOD_OPTIONS: SpectraHostValue = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
}

impl Method {
    pub fn from_code(code: SpectraHostValue) -> Option<Self> {
        match code {
            METHOD_GET => Some(Self::Get),
            METHOD_HEAD => Some(Self::Head),
            METHOD_POST => Some(Self::Post),
            METHOD_PUT => Some(Self::Put),
            METHOD_PATCH => Some(Self::Patch),
            METHOD_DELETE => Some(Self::Delete),
            METHOD_OPTIONS => Some(Self::Options),
            _ => None,
        }
    }

    pub fn code(self) -> SpectraHostValue {
        match self {
            Self::Get => METHOD_GET,
            Self::Head => METHOD_HEAD,
            Self::Post => METHOD_POST,
            Self::Put => METHOD_PUT,
            Self::Patch => METHOD_PATCH,
            Self::Delete => METHOD_DELETE,
            Self::Options => METHOD_OPTIONS,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
        }
    }

    pub fn allows_body(self) -> bool {
        matches!(self, Self::Post | Self::Put | Self::Patch)
    }

    pub fn is_safe(self) -> bool {
        matches!(self, Self::Get | Self::Head | Self::Options)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Status {
    code: u16,
}

impl Status {
    pub fn new(code: u16) -> Result<Self, HttpTypeError> {
        if (100..=599).contains(&code) {
            Ok(Self { code })
        } else {
            Err(HttpTypeError::InvalidStatus(code))
        }
    }

    pub fn code(self) -> u16 {
        self.code
    }

    pub fn reason(self) -> &'static str {
        status_reason_phrase(self.code as SpectraHostValue)
    }

    pub fn class(self) -> u16 {
        self.code / 100
    }

    pub fn is_success(self) -> bool {
        (200..=299).contains(&self.code)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpTypeError {
    InvalidMethod(SpectraHostValue),
    InvalidStatus(u16),
    InvalidHeaderName(String),
    InvalidHeaderValue(String),
    InvalidCookieName(String),
    InvalidCookieValue(String),
    InvalidCookiePath(String),
    InvalidCookieDomain(String),
    InvalidCookieSameSite,
    InvalidPath(String),
}

impl fmt::Display for HttpTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMethod(code) => write!(f, "invalid HTTP method code {code}"),
            Self::InvalidStatus(code) => write!(f, "invalid HTTP status code {code}"),
            Self::InvalidHeaderName(name) => write!(f, "invalid HTTP header name {name:?}"),
            Self::InvalidHeaderValue(value) => write!(f, "invalid HTTP header value {value:?}"),
            Self::InvalidCookieName(name) => write!(f, "invalid HTTP cookie name {name:?}"),
            Self::InvalidCookieValue(value) => write!(f, "invalid HTTP cookie value {value:?}"),
            Self::InvalidCookiePath(path) => write!(f, "invalid HTTP cookie path {path:?}"),
            Self::InvalidCookieDomain(domain) => {
                write!(f, "invalid HTTP cookie domain {domain:?}")
            }
            Self::InvalidCookieSameSite => {
                write!(f, "SameSite=None cookies must set Secure")
            }
            Self::InvalidPath(path) => write!(f, "invalid HTTP request path {path:?}"),
        }
    }
}

impl std::error::Error for HttpTypeError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
}

impl Header {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Result<Self, HttpTypeError> {
        let name = name.into();
        let value = value.into();
        if !is_valid_header_name(&name) {
            return Err(HttpTypeError::InvalidHeaderName(name));
        }
        if !is_valid_header_value(&value) {
            return Err(HttpTypeError::InvalidHeaderValue(value));
        }
        Ok(Self { name, value })
    }

    pub fn name_eq(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Headers {
    entries: Vec<Header>,
}

impl Headers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_vec(entries: Vec<Header>) -> Self {
        Self { entries }
    }

    pub fn insert(
        &mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), HttpTypeError> {
        let header = Header::new(name, value)?;
        if header.name_eq("set-cookie") {
            self.entries.push(header);
        } else if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|existing| existing.name_eq(&header.name))
        {
            existing.value = header.value;
        } else {
            self.entries.push(header);
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|header| header.name_eq(name))
            .map(|header| header.value.as_str())
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let old_len = self.entries.len();
        self.entries.retain(|header| !header.name_eq(name));
        self.entries.len() != old_len
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Header> {
        self.entries.iter()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CookieSameSite {
    Unspecified,
    Lax,
    Strict,
    None,
}

impl CookieSameSite {
    pub fn from_code(code: SpectraHostValue) -> Option<Self> {
        match code {
            0 => Some(Self::Unspecified),
            1 => Some(Self::Lax),
            2 => Some(Self::Strict),
            3 => Some(Self::None),
            _ => None,
        }
    }

    pub fn code(self) -> SpectraHostValue {
        match self {
            Self::Unspecified => 0,
            Self::Lax => 1,
            Self::Strict => 2,
            Self::None => 3,
        }
    }

    fn token(self) -> Option<&'static str> {
        match self {
            Self::Unspecified => None,
            Self::Lax => Some("Lax"),
            Self::Strict => Some("Strict"),
            Self::None => Some("None"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CookieError {
    InvalidSecret,
    InvalidSignature,
    Expired,
    InvalidAttribute(String),
}

impl CookieError {
    pub fn code(&self) -> SpectraHostValue {
        match self {
            Self::InvalidSecret => 1,
            Self::InvalidSignature => 2,
            Self::Expired => 3,
            Self::InvalidAttribute(_) => 4,
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidSecret => "cookie signing secret must not be empty".to_string(),
            Self::InvalidSignature => "cookie signature is missing or invalid".to_string(),
            Self::Expired => "cookie has expired".to_string(),
            Self::InvalidAttribute(message) => message.clone(),
        }
    }
}

impl fmt::Display for CookieError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for CookieError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub path: Option<String>,
    pub domain: Option<String>,
    pub max_age: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: CookieSameSite,
    signature: Option<String>,
    created_at_ms: i64,
}

impl Cookie {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Result<Self, HttpTypeError> {
        let name = name.into();
        let value = value.into();
        if !is_valid_cookie_name(&name) {
            return Err(HttpTypeError::InvalidCookieName(name));
        }
        if !is_valid_cookie_value(&value) {
            return Err(HttpTypeError::InvalidCookieValue(value));
        }
        Ok(Self {
            name,
            value,
            path: None,
            domain: None,
            max_age: None,
            secure: false,
            http_only: false,
            same_site: CookieSameSite::Unspecified,
            signature: None,
            created_at_ms: unix_time_ms(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_options(
        name: impl Into<String>,
        value: impl Into<String>,
        path: Option<String>,
        domain: Option<String>,
        max_age: Option<i64>,
        secure: bool,
        http_only: bool,
        same_site: CookieSameSite,
    ) -> Result<Self, HttpTypeError> {
        let mut cookie = Self::new(name, value)?;
        if let Some(path) = path {
            cookie = cookie.with_path(path)?;
        }
        if let Some(domain) = domain {
            cookie = cookie.with_domain(domain)?;
        }
        if same_site == CookieSameSite::None && !secure {
            return Err(HttpTypeError::InvalidCookieSameSite);
        }
        cookie.max_age = max_age;
        cookie.secure = secure;
        cookie.http_only = http_only;
        cookie.same_site = same_site;
        Ok(cookie)
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Result<Self, HttpTypeError> {
        let path = path.into();
        if !is_valid_cookie_path(&path) {
            return Err(HttpTypeError::InvalidCookiePath(path));
        }
        self.path = Some(path);
        Ok(self)
    }

    pub fn with_domain(mut self, domain: impl Into<String>) -> Result<Self, HttpTypeError> {
        let domain = domain.into();
        if !is_valid_cookie_domain(&domain) {
            return Err(HttpTypeError::InvalidCookieDomain(domain));
        }
        self.domain = Some(domain);
        Ok(self)
    }

    pub fn with_max_age(mut self, max_age: Option<i64>) -> Self {
        self.max_age = max_age;
        self.created_at_ms = unix_time_ms();
        self
    }

    pub fn with_secure(mut self, secure: bool) -> Result<Self, HttpTypeError> {
        if self.same_site == CookieSameSite::None && !secure {
            return Err(HttpTypeError::InvalidCookieSameSite);
        }
        self.secure = secure;
        Ok(self)
    }

    pub fn with_http_only(mut self, http_only: bool) -> Self {
        self.http_only = http_only;
        self
    }

    pub fn with_same_site(mut self, same_site: CookieSameSite) -> Result<Self, HttpTypeError> {
        if same_site == CookieSameSite::None && !self.secure {
            return Err(HttpTypeError::InvalidCookieSameSite);
        }
        self.same_site = same_site;
        Ok(self)
    }

    pub fn sign(&self, secret: &str) -> Result<Self, CookieError> {
        if secret.is_empty() {
            return Err(CookieError::InvalidSecret);
        }
        let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
        let signature = hmac::sign(&key, self.signing_payload().as_bytes());
        let mut signed = self.clone();
        signed.signature = Some(cookie_base64url_encode(signature.as_ref()));
        Ok(signed)
    }

    pub fn verify(&self, secret: &str) -> Result<(), CookieError> {
        if secret.is_empty() {
            return Err(CookieError::InvalidSecret);
        }
        if self.is_expired_at(unix_time_ms()) {
            return Err(CookieError::Expired);
        }
        let Some(signature) = self.signature.as_deref() else {
            return Err(CookieError::InvalidSignature);
        };
        let Some(signature) = cookie_base64url_decode(signature) else {
            return Err(CookieError::InvalidSignature);
        };
        let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
        hmac::verify(&key, self.signing_payload().as_bytes(), &signature)
            .map_err(|_| CookieError::InvalidSignature)
    }

    pub fn is_expired(&self) -> bool {
        self.is_expired_at(unix_time_ms())
    }

    pub fn signature(&self) -> Option<&str> {
        self.signature.as_deref()
    }

    pub fn header_value(&self) -> String {
        let value = self
            .signature
            .as_ref()
            .map(|signature| format!("{}.{}", self.value, signature))
            .unwrap_or_else(|| self.value.clone());
        let mut output = format!("{}={}", self.name, value);
        if let Some(path) = &self.path {
            output.push_str("; Path=");
            output.push_str(path);
        }
        if let Some(domain) = &self.domain {
            output.push_str("; Domain=");
            output.push_str(domain);
        }
        if let Some(max_age) = self.max_age {
            output.push_str("; Max-Age=");
            output.push_str(&max_age.to_string());
        }
        if self.secure {
            output.push_str("; Secure");
        }
        if self.http_only {
            output.push_str("; HttpOnly");
        }
        if let Some(same_site) = self.same_site.token() {
            output.push_str("; SameSite=");
            output.push_str(same_site);
        }
        output
    }

    fn signing_payload(&self) -> String {
        format!(
            "name={}\0value={}\0path={}\0domain={}\0max-age={}\0secure={}\0http-only={}\0same-site={}",
            self.name,
            self.value,
            self.path.as_deref().unwrap_or(""),
            self.domain.as_deref().unwrap_or(""),
            self.max_age.map_or_else(String::new, |value| value.to_string()),
            self.secure,
            self.http_only,
            self.same_site.code(),
        )
    }

    fn is_expired_at(&self, now_ms: i64) -> bool {
        if let Some(max_age) = self.max_age {
            if max_age <= 0 {
                return true;
            }
            if let Some(expires_at) = self
                .created_at_ms
                .checked_add(max_age.saturating_mul(1_000))
            {
                if now_ms >= expires_at {
                    return true;
                }
            }
        }
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl Request {
    pub fn new(method: Method, path: impl Into<String>) -> Result<Self, HttpTypeError> {
        let path = path.into();
        if !is_valid_request_path(&path) {
            return Err(HttpTypeError::InvalidPath(path));
        }
        Ok(Self {
            method,
            path,
            headers: Headers::new(),
            body: Vec::new(),
        })
    }

    pub fn with_header(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, HttpTypeError> {
        self.headers.insert(name, value)?;
        Ok(self)
    }

    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }

    pub fn cookie(&self, name: &str) -> Option<String> {
        cookie_value_from_header(self.header("cookie")?, name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: Status,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: Status) -> Self {
        Self {
            status,
            headers: Headers::new(),
            body: Vec::new(),
        }
    }

    pub fn with_header(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, HttpTypeError> {
        self.headers.insert(name, value)?;
        Ok(self)
    }

    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpVersion {
    pub major: u8,
    pub minor: u8,
}

impl HttpVersion {
    pub const HTTP_10: Self = Self { major: 1, minor: 0 };
    pub const HTTP_11: Self = Self { major: 1, minor: 1 };
}

impl fmt::Display for HttpVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HTTP/{}.{}", self.major, self.minor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyChunk {
    pub data: Vec<u8>,
    pub extension: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpBody {
    pub chunks: Vec<BodyChunk>,
    pub trailers: Vec<Header>,
    pub chunked: bool,
}

impl HttpBody {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        if bytes.is_empty() {
            Self::empty()
        } else {
            Self {
                chunks: vec![BodyChunk {
                    data: bytes,
                    extension: None,
                }],
                trailers: Vec::new(),
                chunked: false,
            }
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        let len = self.chunks.iter().map(|chunk| chunk.data.len()).sum();
        let mut out = Vec::with_capacity(len);
        for chunk in &self.chunks {
            out.extend_from_slice(&chunk.data);
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedRequest {
    pub method: String,
    pub target: String,
    pub version: HttpVersion,
    pub headers: Vec<Header>,
    pub body: HttpBody,
    pub keep_alive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedResponse {
    pub version: HttpVersion,
    pub status_code: u16,
    pub reason: String,
    pub headers: Vec<Header>,
    pub body: HttpBody,
    pub keep_alive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseErrorKind {
    Incomplete,
    InvalidStartLine,
    InvalidMethod,
    InvalidTarget,
    InvalidVersion,
    InvalidStatus,
    InvalidHeader,
    HeaderTooLarge,
    BodyTooLarge,
    BodyLengthMismatch,
    InvalidChunkSize,
    InvalidChunkTerminator,
    UnsupportedTransferEncoding,
    ObsoleteLineFolding,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub position: usize,
    pub message: String,
}

impl ParseError {
    fn new(kind: ParseErrorKind, position: usize, message: impl Into<String>) -> Self {
        Self {
            kind,
            position,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.position)
    }
}

impl std::error::Error for ParseError {}

#[derive(Clone, Debug)]
pub struct ParserConfig {
    pub max_header_bytes: usize,
    pub max_body_bytes: usize,
    pub max_chunk_bytes: usize,
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self {
            max_header_bytes: 64 * 1024,
            max_body_bytes: 16 * 1024 * 1024,
            max_chunk_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParserMode {
    Request,
    Response,
}

pub struct Http1Parser {
    mode: ParserMode,
    config: ParserConfig,
    buffer: Vec<u8>,
    consumed: usize,
}

impl Http1Parser {
    pub fn request() -> Self {
        Self::request_with_config(ParserConfig::default())
    }

    pub fn response() -> Self {
        Self::response_with_config(ParserConfig::default())
    }

    pub fn request_with_config(config: ParserConfig) -> Self {
        Self::new(ParserMode::Request, config)
    }

    pub fn response_with_config(config: ParserConfig) -> Self {
        Self::new(ParserMode::Response, config)
    }

    fn new(mode: ParserMode, config: ParserConfig) -> Self {
        Self {
            mode,
            config,
            buffer: Vec::new(),
            consumed: 0,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }

    /// Transfers bytes that arrived after the parsed HTTP message to the
    /// protocol that owns the connection after an upgrade (for example,
    /// WebSocket).  The HTTP parser must not discard those bytes because a
    /// client may pipeline the first WebSocket frame with its handshake.
    pub(crate) fn take_buffered(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buffer)
    }

    pub fn parse_next_request(&mut self) -> Result<Option<ParsedRequest>, ParseError> {
        if self.mode != ParserMode::Request {
            return Err(ParseError::new(
                ParseErrorKind::InvalidStartLine,
                self.consumed,
                "parser is configured for HTTP responses",
            ));
        }
        let Some(message) = self.parse_next_message()? else {
            return Ok(None);
        };
        match message {
            ParsedMessage::Request(request) => Ok(Some(request)),
            ParsedMessage::Response(_) => unreachable!("request parser returned a response"),
        }
    }

    pub fn parse_next_response(&mut self) -> Result<Option<ParsedResponse>, ParseError> {
        if self.mode != ParserMode::Response {
            return Err(ParseError::new(
                ParseErrorKind::InvalidStartLine,
                self.consumed,
                "parser is configured for HTTP requests",
            ));
        }
        let Some(message) = self.parse_next_message()? else {
            return Ok(None);
        };
        match message {
            ParsedMessage::Response(response) => Ok(Some(response)),
            ParsedMessage::Request(_) => unreachable!("response parser returned a request"),
        }
    }

    fn parse_next_message(&mut self) -> Result<Option<ParsedMessage>, ParseError> {
        let Some(header_end) = find_header_end(&self.buffer) else {
            if self.buffer.len() > self.config.max_header_bytes {
                return Err(ParseError::new(
                    ParseErrorKind::HeaderTooLarge,
                    self.consumed + self.config.max_header_bytes,
                    "HTTP header section exceeds configured limit",
                ));
            }
            return Ok(None);
        };
        if header_end > self.config.max_header_bytes {
            return Err(ParseError::new(
                ParseErrorKind::HeaderTooLarge,
                self.consumed + self.config.max_header_bytes,
                "HTTP header section exceeds configured limit",
            ));
        }

        let (start_line, headers) =
            parse_head(&self.buffer[..header_end], self.consumed, self.mode)?;
        let body_start = header_end + 4;
        let body_meta = BodyMeta::from_headers(&headers, self.consumed)?;
        let Some((body, consumed_body_bytes)) = parse_body(
            &self.buffer,
            body_start,
            self.consumed,
            &self.config,
            &body_meta,
        )?
        else {
            return Ok(None);
        };

        let total_len = body_start + consumed_body_bytes;
        let keep_alive = determine_keep_alive(&start_line.version, &headers);
        let parsed = match (self.mode, start_line.kind) {
            (ParserMode::Request, StartLineKind::Request { method, target, .. }) => {
                ParsedMessage::Request(ParsedRequest {
                    method,
                    target,
                    version: start_line.version,
                    headers,
                    body,
                    keep_alive,
                })
            }
            (
                ParserMode::Response,
                StartLineKind::Response {
                    status_code,
                    reason,
                },
            ) => ParsedMessage::Response(ParsedResponse {
                version: start_line.version,
                status_code,
                reason,
                headers,
                body,
                keep_alive,
            }),
            _ => unreachable!("parser mode and start-line kind diverged"),
        };

        self.buffer.drain(..total_len);
        self.consumed += total_len;
        Ok(Some(parsed))
    }
}

enum ParsedMessage {
    Request(ParsedRequest),
    Response(ParsedResponse),
}

struct StartLine {
    version: HttpVersion,
    kind: StartLineKind,
}

enum StartLineKind {
    Request { method: String, target: String },
    Response { status_code: u16, reason: String },
}

enum BodyMeta {
    Empty,
    ContentLength(usize),
    Chunked,
}

struct HttpStore {
    requests: ApiHandleTable<Request>,
    responses: ApiHandleTable<Response>,
    headers: ApiHandleTable<Header>,
    cookies: ApiHandleTable<Cookie>,
    last_cookie_error: Option<CookieError>,
}

impl HttpStore {
    fn new() -> Self {
        Self {
            requests: ApiHandleTable::new(HandleKind::ApiHttpRequest),
            responses: ApiHandleTable::new(HandleKind::ApiHttpResponse),
            headers: ApiHandleTable::new(HandleKind::ApiHttpHeader),
            cookies: ApiHandleTable::new(HandleKind::ApiHttpCookie),
            last_cookie_error: None,
        }
    }

    fn request_handle(&mut self, request: Request) -> SpectraHostValue {
        self.requests.insert(request)
    }

    fn response_handle(&mut self, response: Response) -> SpectraHostValue {
        self.responses.insert(response)
    }

    fn header_handle(&mut self, header: Header) -> SpectraHostValue {
        self.headers.insert(header)
    }

    fn cookie_handle(&mut self, cookie: Cookie) -> SpectraHostValue {
        self.cookies.insert(cookie)
    }

    fn set_cookie_error(&mut self, error: CookieError) {
        self.last_cookie_error = Some(error);
    }
}

fn store() -> &'static Mutex<HttpStore> {
    static STORE: OnceLock<Mutex<HttpStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HttpStore::new()))
}

fn unix_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn is_valid_cookie_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.is_empty()
        && !path
            .bytes()
            .any(|byte| byte <= 0x20 || byte == 0x7f || byte == b';' || byte == b',')
}

fn is_valid_cookie_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn cookie_base64url_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity((bytes.len() * 4).div_ceil(3));
    let mut index = 0;
    while index < bytes.len() {
        let first = bytes[index];
        let second = bytes.get(index + 1).copied();
        let third = bytes.get(index + 2).copied();
        output.push(ALPHABET[(first >> 2) as usize] as char);
        output.push(ALPHABET[((first & 0x03) << 4 | second.map_or(0, |value| value >> 4)) as usize] as char);
        if let Some(second) = second {
            output.push(ALPHABET[((second & 0x0f) << 2 | third.map_or(0, |value| value >> 6)) as usize] as char);
        }
        if let Some(third) = third {
            output.push(ALPHABET[(third & 0x3f) as usize] as char);
        }
        index += 3;
    }
    output
}

fn cookie_base64url_decode(value: &str) -> Option<Vec<u8>> {
    fn digit(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }

    if value.is_empty() || value.len() % 4 == 1 {
        return None;
    }
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity((bytes.len() * 3) / 4);
    let mut index = 0;
    while index < bytes.len() {
        let first = digit(bytes[index])?;
        let second = digit(*bytes.get(index + 1)?)?;
        output.push((first << 2) | (second >> 4));
        if let Some(third) = bytes.get(index + 2).copied() {
            let third = digit(third)?;
            output.push((second << 4) | (third >> 2));
            if let Some(fourth) = bytes.get(index + 3).copied() {
                let fourth = digit(fourth)?;
                output.push((third << 6) | fourth);
            }
        }
        index += 4.min(bytes.len() - index);
    }
    Some(output)
}

pub(crate) fn store_response(response: Response) -> SpectraHostValue {
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    store.response_handle(response)
}

pub(crate) fn store_request(request: Request) -> SpectraHostValue {
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    store.request_handle(request)
}

pub(crate) fn clone_request(handle: SpectraHostValue) -> Option<Request> {
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    store.requests.get(&handle).cloned()
}

pub(crate) fn clone_response(handle: SpectraHostValue) -> Option<Response> {
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    store.responses.get(&handle).cloned()
}
