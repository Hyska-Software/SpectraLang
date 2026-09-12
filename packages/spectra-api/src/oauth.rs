use crate::client::{ClientConfig, ClientRequest, HttpClient};
use crate::handles::ApiHandleTable;
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use ring::digest::{digest, SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::Value;
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
};
use spectra_runtime::handles::HandleKind;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

const PKCE_VERIFIER_BYTES: usize = 32;
const MAX_ERROR_BODY_BYTES: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OAuthError {
    InvalidConfiguration,
    InvalidState,
    StateMismatch,
    InvalidCode,
    MissingRefreshToken,
    RevokedToken,
    MissingRevocationEndpoint,
    Randomness,
    Network(String),
    HttpStatus(u16, String),
    InvalidTokenResponse(String),
}

impl fmt::Display for OAuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => write!(f, "invalid OAuth client configuration"),
            Self::InvalidState => write!(f, "authorization state must not be empty"),
            Self::StateMismatch => write!(f, "authorization state does not match"),
            Self::InvalidCode => write!(f, "authorization code must not be empty"),
            Self::MissingRefreshToken => write!(f, "OAuth token has no refresh token"),
            Self::RevokedToken => write!(f, "OAuth token has been revoked"),
            Self::MissingRevocationEndpoint => write!(f, "revocation endpoint is not configured"),
            Self::Randomness => write!(f, "secure random source is unavailable"),
            Self::Network(message) => write!(f, "OAuth network request failed: {message}"),
            Self::HttpStatus(status, body) => {
                write!(f, "OAuth endpoint returned HTTP {status}: {body}")
            }
            Self::InvalidTokenResponse(message) => {
                write!(f, "invalid OAuth token response: {message}")
            }
        }
    }
}

impl std::error::Error for OAuthError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuthToken {
    pub access_token: String,
    pub token_type: String,
    pub expires_at_ms: Option<u64>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
    revoked: bool,
}

#[derive(Clone, Debug)]
struct OAuthClientState {
    client_id: String,
    client_secret: String,
    authorize_url: String,
    token_url: String,
    redirect_uri: String,
    scope: String,
    revocation_url: Option<String>,
    pkce_verifier: String,
    authorization_state: Option<String>,
}

#[derive(Clone, Debug)]
pub struct OAuthClient {
    state: Arc<Mutex<OAuthClientState>>,
}

impl OAuthClient {
    pub fn new(
        client_id: String,
        client_secret: String,
        authorize_url: String,
        token_url: String,
        redirect_uri: String,
        scope: String,
    ) -> Result<Self, OAuthError> {
        if client_id.trim().is_empty()
            || authorize_url.trim().is_empty()
            || token_url.trim().is_empty()
            || redirect_uri.trim().is_empty()
            || !is_http_url(&authorize_url)
            || !is_http_url(&token_url)
        {
            return Err(OAuthError::InvalidConfiguration);
        }
        let pkce_verifier = generate_pkce_verifier()?;
        Ok(Self {
            state: Arc::new(Mutex::new(OAuthClientState {
                client_id,
                client_secret,
                authorize_url,
                token_url,
                redirect_uri,
                scope,
                revocation_url: None,
                pkce_verifier,
                authorization_state: None,
            })),
        })
    }

    pub fn set_revocation_url(&self, url: String) -> bool {
        if url.trim().is_empty() || !is_http_url(&url) {
            return false;
        }
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .revocation_url = Some(url);
        true
    }

    pub fn authorization_url(&self, state: String) -> Result<String, OAuthError> {
        if state.is_empty() {
            return Err(OAuthError::InvalidState);
        }
        let mut client = self.state.lock().unwrap_or_else(|error| error.into_inner());
        client.authorization_state = Some(state.clone());
        let challenge = pkce_challenge(&client.pkce_verifier);
        let mut parameters = vec![
            ("response_type", "code".to_string()),
            ("client_id", client.client_id.clone()),
            ("redirect_uri", client.redirect_uri.clone()),
            ("state", state),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256".to_string()),
        ];
        if !client.scope.is_empty() {
            parameters.push(("scope", client.scope.clone()));
        }
        Ok(append_query_parameters(&client.authorize_url, &parameters))
    }

    pub fn exchange_code(&self, code: &str, state: &str) -> Result<OAuthToken, OAuthError> {
        if code.is_empty() {
            return Err(OAuthError::InvalidCode);
        }
        let client = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let expected_state = client
            .authorization_state
            .as_deref()
            .ok_or(OAuthError::InvalidState)?;
        if !constant_time_equal(expected_state.as_bytes(), state.as_bytes()) {
            return Err(OAuthError::StateMismatch);
        }
        let fields = client.form_fields();
        let code_verifier = client.pkce_verifier.clone();
        drop(client);

        let mut form = fields;
        form.push(("grant_type", "authorization_code".to_string()));
        form.push(("code", code.to_string()));
        form.push(("code_verifier", code_verifier));
        form.push(("redirect_uri", self.redirect_uri()));
        request_token(&self.token_url(), form, None, None)
    }

    pub fn refresh(&self, token: &OAuthToken) -> Result<OAuthToken, OAuthError> {
        if token.revoked {
            return Err(OAuthError::RevokedToken);
        }
        let refresh_token = token
            .refresh_token
            .clone()
            .ok_or(OAuthError::MissingRefreshToken)?;
        let mut form = {
            let client = self.state.lock().unwrap_or_else(|error| error.into_inner());
            client.form_fields()
        };
        form.push(("grant_type", "refresh_token".to_string()));
        form.push(("refresh_token", refresh_token.clone()));
        request_token(&self.token_url(), form, Some(refresh_token), None)
    }

    pub fn revoke(&self, token: &OAuthToken) -> Result<(), OAuthError> {
        if token.revoked {
            return Err(OAuthError::RevokedToken);
        }
        let (url, mut form) = {
            let client = self.state.lock().unwrap_or_else(|error| error.into_inner());
            let url = client
                .revocation_url
                .clone()
                .ok_or(OAuthError::MissingRevocationEndpoint)?;
            (url, client.form_fields())
        };
        form.push(("token", token.access_token.clone()));
        form.push(("token_type_hint", token.token_type.clone()));
        let response = post_form(&url, form, None)?;
        if (200..=299).contains(&response.status_code) {
            Ok(())
        } else {
            Err(OAuthError::HttpStatus(
                response.status_code,
                response_body_summary(&response),
            ))
        }
    }

    fn redirect_uri(&self) -> String {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .redirect_uri
            .clone()
    }

    fn token_url(&self) -> String {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .token_url
            .clone()
    }
}

impl OAuthClientState {
    fn form_fields(&self) -> Vec<(&'static str, String)> {
        let mut fields = vec![("client_id", self.client_id.clone())];
        if !self.client_secret.is_empty() {
            fields.push(("client_secret", self.client_secret.clone()));
        }
        fields
    }
}

fn is_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn generate_pkce_verifier() -> Result<String, OAuthError> {
    let mut bytes = [0_u8; PKCE_VERIFIER_BYTES];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| OAuthError::Randomness)?;
    Ok(base64url(&bytes))
}

fn pkce_challenge(verifier: &str) -> String {
    base64url(digest(&SHA256, verifier.as_bytes()).as_ref())
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = (left.len() ^ right.len()) as u64;
    for index in 0..left.len().max(right.len()) {
        let left_byte = left.get(index).copied().unwrap_or(0);
        let right_byte = right.get(index).copied().unwrap_or(0);
        difference |= u64::from(left_byte ^ right_byte);
    }
    difference == 0
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity((bytes.len() * 4).div_ceil(3));
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let block = ((bytes[index] as u32) << 16)
            | ((bytes[index + 1] as u32) << 8)
            | bytes[index + 2] as u32;
        output.push(ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 6) & 0x3f) as usize] as char);
        output.push(ALPHABET[(block & 0x3f) as usize] as char);
        index += 3;
    }
    let remaining = bytes.len() - index;
    if remaining == 1 {
        let block = (bytes[index] as u32) << 16;
        output.push(ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 0x3f) as usize] as char);
    } else if remaining == 2 {
        let block = ((bytes[index] as u32) << 16) | ((bytes[index + 1] as u32) << 8);
        output.push(ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 6) & 0x3f) as usize] as char);
    }
    output
}

fn percent_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            output.push(byte as char);
        } else {
            output.push('%');
            output.push(HEX[(byte >> 4) as usize] as char);
            output.push(HEX[(byte & 0xf) as usize] as char);
        }
    }
    output
}

fn append_query_parameters(url: &str, parameters: &[(&str, String)]) -> String {
    let (base, fragment) = url
        .split_once('#')
        .map_or((url, None), |(base, fragment)| (base, Some(fragment)));
    let separator = if base.contains('?') {
        if base.ends_with('?') || base.ends_with('&') {
            ""
        } else {
            "&"
        }
    } else {
        "?"
    };
    let query = parameters
        .iter()
        .map(|(name, value)| format!("{name}={}", percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    match fragment {
        Some(fragment) => format!("{base}{separator}{query}#{fragment}"),
        None => format!("{base}{separator}{query}"),
    }
}

fn form_encode(fields: &[(&str, String)]) -> String {
    fields
        .iter()
        .map(|(name, value)| format!("{name}={}", percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn post_form(
    url: &str,
    fields: Vec<(&'static str, String)>,
    tls: Option<crate::tls::TlsClientConfig>,
) -> Result<crate::client::ClientResponse, OAuthError> {
    let base = ClientConfig {
        max_redirects: 0,
        ..ClientConfig::default()
    };
    #[cfg(test)]
    let base = {
        // OAuth unit tests use a loopback mock server; production OAuth
        // requests retain the default SSRF-deny policy.
        base.allow_private_networks(true)
    };
    let mut config = base;
    if let Some(tls) = tls {
        let roots = tls
            .build()
            .map_err(|error| OAuthError::Network(error.to_string()))?;
        config = config.with_tls_config(roots);
    }
    let client = HttpClient::new(config);
    let request = ClientRequest::new("POST", url)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_header("Accept", "application/json")
        .with_body(form_encode(&fields).into_bytes());
    // The nonblocking client speaks HTTPS via the webpki trust store (or the
    // caller-supplied roots above); the sync client it replaces hard-codes
    // `allow_https = false`, so every real identity provider failed here.
    // Cancellation is never requested on this path; the token only satisfies
    // the bridge signature.
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    client
        .request_nonblocking(request, cancelled)
        .map_err(|error| OAuthError::Network(error.to_string()))
}

fn response_body_summary(response: &crate::client::ClientResponse) -> String {
    let body = response.body.bytes();
    let body = String::from_utf8_lossy(&body);
    body.chars().take(MAX_ERROR_BODY_BYTES).collect()
}

fn request_token(
    token_url: &str,
    mut fields: Vec<(&'static str, String)>,
    fallback_refresh_token: Option<String>,
    tls: Option<crate::tls::TlsClientConfig>,
) -> Result<OAuthToken, OAuthError> {
    let response = post_form(token_url, std::mem::take(&mut fields), tls)?;
    if !(200..=299).contains(&response.status_code) {
        return Err(OAuthError::HttpStatus(
            response.status_code,
            response_body_summary(&response),
        ));
    }
    let body = response.body.bytes();
    let value: Value = serde_json::from_slice(&body)
        .map_err(|error| OAuthError::InvalidTokenResponse(error.to_string()))?;
    let object = value.as_object().ok_or_else(|| {
        OAuthError::InvalidTokenResponse("response must be an object".to_string())
    })?;
    let access_token = object
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OAuthError::InvalidTokenResponse("missing access_token".to_string()))?
        .to_string();
    let token_type = object
        .get("token_type")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OAuthError::InvalidTokenResponse("missing token_type".to_string()))?
        .to_string();
    let expires_at_ms = object
        .get("expires_in")
        .and_then(Value::as_u64)
        .and_then(|seconds| seconds.checked_mul(1_000))
        .and_then(|duration| current_unix_time_ms().and_then(|now| now.checked_add(duration)));
    let refresh_token = object
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or(fallback_refresh_token);
    let scope = object
        .get("scope")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Ok(OAuthToken {
        access_token,
        token_type,
        expires_at_ms,
        refresh_token,
        scope,
        revoked: false,
    })
}

fn current_unix_time_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

struct OAuthStore {
    clients: ApiHandleTable<OAuthClient>,
    tokens: ApiHandleTable<OAuthToken>,
}

impl OAuthStore {
    fn new() -> Self {
        Self {
            clients: ApiHandleTable::new(HandleKind::ApiOAuthClient),
            tokens: ApiHandleTable::new(HandleKind::ApiOAuthToken),
        }
    }
}

fn store() -> &'static Mutex<OAuthStore> {
    static STORE: OnceLock<Mutex<OAuthStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(OAuthStore::new()))
}

pub extern "C" fn client_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 6) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let values = [
        read_spectra_string(args[0]),
        read_spectra_string(args[1]),
        read_spectra_string(args[2]),
        read_spectra_string(args[3]),
        read_spectra_string(args[4]),
        read_spectra_string(args[5]),
    ];
    let [Some(client_id), Some(client_secret), Some(authorize_url), Some(token_url), Some(redirect_uri), Some(scope)] =
        values
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(client) = OAuthClient::new(
        client_id,
        client_secret,
        authorize_url,
        token_url,
        redirect_uri,
        scope,
    ) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
    write_result(ctx, store.clients.insert(client))
}

pub extern "C" fn client_set_revocation_url(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(url) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let client = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        store.clients.get(&args[0]).cloned()
    };
    let Some(client) = client else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, if client.set_revocation_url(url) { 1 } else { 0 })
}

pub extern "C" fn authorization_url(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(state) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let client = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        store.clients.get(&args[0]).cloned()
    };
    let Some(client) = client else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(url) = client.authorization_url(state) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(&url))
}

pub extern "C" fn exchange_code(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(code), Some(state)) = (read_spectra_string(args[1]), read_spectra_string(args[2]))
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let client = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        store.clients.get(&args[0]).cloned()
    };
    let Some(client) = client else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(token) = client.exchange_code(&code, &state) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
    write_result(ctx, store.tokens.insert(token))
}

pub extern "C" fn refresh(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (client, token) = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        let Some(client) = store.clients.get(&args[0]).cloned() else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(token) = store.tokens.get(&args[1]).cloned() else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        (client, token)
    };
    let Ok(token) = client.refresh(&token) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
    write_result(ctx, store.tokens.insert(token))
}

pub extern "C" fn revoke(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (client, token) = {
        let store = store().lock().unwrap_or_else(|error| error.into_inner());
        let Some(client) = store.clients.get(&args[0]).cloned() else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(token) = store.tokens.get(&args[1]).cloned() else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        (client, token)
    };
    if client.revoke(&token).is_err() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(token) = store.tokens.get_mut(&args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    token.revoked = true;
    write_result(ctx, 1)
}

fn token_string(ctx: *mut SpectraHostCallContext, select: impl FnOnce(&OAuthToken) -> &str) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(token) = store.tokens.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(select(token)))
}

pub extern "C" fn token_access_token(ctx: *mut SpectraHostCallContext) -> i32 {
    token_string(ctx, |token| &token.access_token)
}

pub extern "C" fn token_refresh_token(ctx: *mut SpectraHostCallContext) -> i32 {
    token_string(ctx, |token| token.refresh_token.as_deref().unwrap_or(""))
}

pub extern "C" fn token_type(ctx: *mut SpectraHostCallContext) -> i32 {
    token_string(ctx, |token| &token.token_type)
}

pub extern "C" fn token_expires_at_ms(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|error| error.into_inner());
    let Some(token) = store.tokens.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        token
            .expires_at_ms
            .map(|value| value.min(SpectraHostValue::MAX as u64) as SpectraHostValue)
            .unwrap_or(0),
    )
}

pub extern "C" fn token_scope(ctx: *mut SpectraHostCallContext) -> i32 {
    token_string(ctx, |token| token.scope.as_deref().unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    fn query_value(url: &str, name: &str) -> Option<String> {
        url.split_once('?')?.1.split('&').find_map(|pair| {
            let (candidate, value) = pair.split_once('=')?;
            (candidate == name).then_some(value.to_string())
        })
    }

    fn read_request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set mock read timeout");
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let count = stream.read(&mut buffer).expect("read mock request");
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            let header_end = header_end + 4;
            let header = String::from_utf8_lossy(&bytes[..header_end]);
            let content_length = header
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if bytes.len() >= header_end + content_length {
                break;
            }
        }
        String::from_utf8(bytes).expect("mock request is UTF-8")
    }

    fn form_value(body: &str, name: &str) -> Option<String> {
        body.split('&').find_map(|pair| {
            let (candidate, value) = pair.split_once('=')?;
            (candidate == name).then_some(value.to_string())
        })
    }

    fn response(stream: &mut TcpStream, body: &str) {
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .expect("write mock response");
    }

    #[test]
    fn oauth_pkce_code_exchange_refresh_and_revocation_are_end_to_end() {
        let client_id = "spectra-client".to_string();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind OAuth mock");
        let address = listener.local_addr().expect("mock address");
        let client = OAuthClient::new(
            client_id,
            "client-secret".to_string(),
            format!("http://{address}/authorize"),
            format!("http://{address}/token"),
            "http://127.0.0.1:9876/callback".to_string(),
            "openid profile".to_string(),
        )
        .expect("valid OAuth client");
        let authorization_url = client
            .authorization_url("state-123".to_string())
            .expect("authorization URL");
        let challenge = query_value(&authorization_url, "code_challenge")
            .expect("PKCE challenge in authorization URL");
        assert_eq!(
            query_value(&authorization_url, "code_challenge_method").as_deref(),
            Some("S256")
        );
        assert_eq!(
            query_value(&authorization_url, "state").as_deref(),
            Some("state-123")
        );
        assert!(matches!(
            client.exchange_code("code", "wrong-state"),
            Err(OAuthError::StateMismatch)
        ));

        let join = std::thread::spawn(move || {
            for index in 0..3 {
                let (mut stream, _) = listener.accept().expect("accept OAuth mock request");
                let raw = read_request(&mut stream);
                let (request_line, body) =
                    raw.split_once("\r\n\r\n").expect("mock request headers");
                if index == 0 {
                    assert!(request_line.starts_with("POST /token "));
                    assert_eq!(
                        form_value(body, "grant_type").as_deref(),
                        Some("authorization_code")
                    );
                    let verifier = form_value(body, "code_verifier").expect("PKCE verifier");
                    assert_eq!(pkce_challenge(&verifier), challenge);
                    response(
                        &mut stream,
                        "{\"access_token\":\"access-one\",\"token_type\":\"Bearer\",\"expires_in\":3600,\"refresh_token\":\"refresh-one\",\"scope\":\"openid profile\"}",
                    );
                } else if index == 1 {
                    assert_eq!(
                        form_value(body, "grant_type").as_deref(),
                        Some("refresh_token")
                    );
                    assert_eq!(
                        form_value(body, "refresh_token").as_deref(),
                        Some("refresh-one")
                    );
                    response(
                        &mut stream,
                        "{\"access_token\":\"access-two\",\"token_type\":\"Bearer\",\"expires_in\":3600,\"refresh_token\":\"refresh-two\"}",
                    );
                } else {
                    assert!(request_line.starts_with("POST /revoke "));
                    assert_eq!(form_value(body, "token").as_deref(), Some("access-two"));
                    response(&mut stream, "");
                }
            }
        });

        let token = client
            .exchange_code("code", "state-123")
            .expect("authorization code exchange");
        assert_eq!(token.access_token, "access-one");
        assert_eq!(token.refresh_token.as_deref(), Some("refresh-one"));
        assert_eq!(token.scope.as_deref(), Some("openid profile"));

        let refreshed = client.refresh(&token).expect("refresh token exchange");
        assert_eq!(refreshed.access_token, "access-two");
        assert_eq!(refreshed.refresh_token.as_deref(), Some("refresh-two"));

        assert!(client.set_revocation_url(format!("http://{address}/revoke")));
        client.revoke(&refreshed).expect("token revocation");
        join.join().expect("OAuth mock thread");
    }

    #[test]
    fn oauth_client_rejects_invalid_configuration_and_empty_state() {
        assert!(matches!(
            OAuthClient::new(
                String::new(),
                String::new(),
                "http://authorize".to_string(),
                "http://token".to_string(),
                "http://redirect".to_string(),
                String::new(),
            ),
            Err(OAuthError::InvalidConfiguration)
        ));
        let client = OAuthClient::new(
            "client".to_string(),
            String::new(),
            "http://authorize".to_string(),
            "http://token".to_string(),
            "http://redirect".to_string(),
            String::new(),
        )
        .expect("valid OAuth client");
        assert!(matches!(
            client.authorization_url(String::new()),
            Err(OAuthError::InvalidState)
        ));
        assert!(!client.set_revocation_url(String::new()));
    }

    #[test]
    fn token_exchange_over_https_with_custom_roots() {
        use rcgen::generate_simple_self_signed;
        use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
        use rustls::ServerConfig;

        let cert = generate_simple_self_signed(vec!["127.0.0.1".into(), "localhost".into()])
            .expect("certificate");
        let der = cert.cert.der().clone();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()));
        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![der.clone()], key)
            .expect("server config");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind TLS mock");
        let address = listener.local_addr().expect("mock address");
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut stream = stream;
            let mut conn = rustls::ServerConnection::new(std::sync::Arc::new(server_config))
                .expect("server connection");
            let mut tls = rustls::Stream::new(&mut conn, &mut stream);
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                use std::io::Read;
                let count = tls.read(&mut buffer).expect("read mock request");
                if count == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..count]);
                let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n")
                else {
                    continue;
                };
                let header_end = header_end + 4;
                let header = String::from_utf8_lossy(&bytes[..header_end]);
                let content_length = header
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if bytes.len() >= header_end + content_length {
                    break;
                }
            }
            let text = String::from_utf8(bytes).expect("mock request is UTF-8");
            assert!(text.starts_with("POST /token "), "mock IdP saw: {text}");
            assert!(text.contains("grant_type=authorization_code"));
            let body = r#"{"access_token":"https-token","token_type":"Bearer","expires_in":3600,"refresh_token":"https-refresh","scope":"openid"}"#;
            use std::io::Write;
            write!(
                tls,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write mock response");
        });
        let roots = crate::tls::TlsClientConfig::with_roots(vec![der.to_vec()]);
        let token = request_token(
            &format!("https://127.0.0.1:{}/token", address.port()),
            vec![
                ("grant_type", "authorization_code".to_string()),
                ("code", "code-123".to_string()),
            ],
            None,
            Some(roots),
        )
        .expect("HTTPS token exchange");
        assert_eq!(token.access_token, "https-token");
        assert_eq!(token.token_type, "Bearer");
        assert_eq!(token.refresh_token.as_deref(), Some("https-refresh"));
        assert_eq!(token.scope.as_deref(), Some("openid"));
        assert!(token.expires_at_ms.is_some());
        server.join().expect("mock IdP served");
    }
}
