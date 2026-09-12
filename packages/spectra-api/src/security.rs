use crate::handler::HandlerError;
use crate::handles::ApiHandleTable;
use crate::http::{Method, Request, Response, Status};
use crate::middleware::{self, Middleware, MiddlewareContext, MiddlewareDecision};
use crate::{read_args, read_spectra_string, write_result};
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
};
use spectra_runtime::handles::HandleKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Mutex, OnceLock};

const HEADER_ORIGIN: &str = "Origin";

/// Origin allowlist used by the state-changing request middleware.
///
/// Requests without an Origin header are allowed so non-browser clients and
/// signed server-to-server calls remain usable. When a browser sends Origin,
/// every state-changing method must match this exact allowlist.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CsrfPolicy {
    allowed_origins: Vec<String>,
}

impl CsrfPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_origin(mut self, origin: impl Into<String>) -> Self {
        let origin = origin.into();
        if !origin.trim().is_empty()
            && !self
                .allowed_origins
                .iter()
                .any(|allowed| allowed == &origin)
        {
            self.allowed_origins.push(origin);
        }
        self
    }

    pub fn origin_count(&self) -> usize {
        self.allowed_origins.len()
    }

    pub fn allows_origin(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|allowed| allowed == origin)
    }

    pub fn middleware(self) -> CsrfMiddleware {
        CsrfMiddleware { policy: self }
    }
}

#[derive(Clone)]
pub struct CsrfMiddleware {
    policy: CsrfPolicy,
}

impl Middleware for CsrfMiddleware {
    fn on_request(
        &self,
        request: Request,
        _context: &mut MiddlewareContext,
    ) -> Result<MiddlewareDecision, HandlerError> {
        if !is_state_changing(request.method) {
            return Ok(MiddlewareDecision::Continue(request));
        }

        let Some(origin) = request.header(HEADER_ORIGIN) else {
            return Ok(MiddlewareDecision::Continue(request));
        };
        if self.policy.allows_origin(origin) {
            return Ok(MiddlewareDecision::Continue(request));
        }

        Ok(MiddlewareDecision::ShortCircuit(
            Response::new(Status::new(403).expect("valid CSRF status"))
                .with_header("content-type", "application/problem+json")
                .map_err(|error| HandlerError::new(500, error.to_string()))?
                .with_body(
                    br#"{"type":"about:blank","title":"Forbidden","status":403,"detail":"CSRF origin rejected","code":"csrf_origin_rejected"}"#.to_vec(),
                ),
        ))
    }
}

fn is_state_changing(method: Method) -> bool {
    matches!(
        method,
        Method::Post | Method::Put | Method::Patch | Method::Delete
    )
}

/// SSRF policy applied after DNS resolution and before a client socket is
/// opened. The default rejects loopback, RFC1918, link-local, unspecified,
/// multicast, and IPv6 unique-local addresses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SsrfPolicy {
    allow_private_networks: bool,
}

impl SsrfPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_private_networks(mut self, allow: bool) -> Self {
        self.allow_private_networks = allow;
        self
    }

    pub fn allows_private_networks(&self) -> bool {
        self.allow_private_networks
    }

    pub fn allows_address(&self, address: SocketAddr) -> bool {
        self.allow_private_networks || !is_private_or_link_local(address.ip())
    }
}

pub fn is_private_or_link_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_private_or_link_local_v4(ip),
        IpAddr::V6(ip) => is_private_or_link_local_v6(ip),
    }
}

fn is_private_or_link_local_v4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 198 && (18..=19).contains(&octets[1]))
}

fn is_private_or_link_local_v6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip.is_multicast()
        || (segments[0] == 0x0000 && segments[1] == 0x0000 && segments[2] == 0x0000)
}

struct SecurityStore {
    csrf_policies: ApiHandleTable<CsrfPolicy>,
    ssrf_policies: ApiHandleTable<SsrfPolicy>,
}

impl SecurityStore {
    fn new() -> Self {
        Self {
            csrf_policies: ApiHandleTable::new(HandleKind::ApiCsrfPolicy),
            ssrf_policies: ApiHandleTable::new(HandleKind::ApiSsrfPolicy),
        }
    }
}

fn store() -> &'static Mutex<SecurityStore> {
    static STORE: OnceLock<Mutex<SecurityStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(SecurityStore::new()))
}

pub(crate) fn clone_ssrf_policy(handle: SpectraHostValue) -> Option<SsrfPolicy> {
    store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .ssrf_policies
        .get(&handle)
        .cloned()
}

fn read_csrf_policy(handle: SpectraHostValue) -> Option<CsrfPolicy> {
    store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .csrf_policies
        .get(&handle)
        .cloned()
}

fn read_ssrf_policy(handle: SpectraHostValue) -> Option<SsrfPolicy> {
    clone_ssrf_policy(handle)
}

fn bool_arg(value: SpectraHostValue) -> bool {
    value != 0
}

pub extern "C" fn csrf_policy(ctx: *mut SpectraHostCallContext) -> i32 {
    let mut store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, store.csrf_policies.insert(CsrfPolicy::new()))
}

pub extern "C" fn csrf_allow_origin(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(policy) = read_csrf_policy(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(origin) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if origin.trim().is_empty() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, store.csrf_policies.insert(policy.allow_origin(origin)))
}

pub extern "C" fn csrf_origin_count(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(policy) = read_csrf_policy(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, policy.origin_count() as SpectraHostValue)
}

pub extern "C" fn csrf_middleware(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(policy) = read_csrf_policy(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        middleware::register_sync_middleware(policy.middleware()),
    )
}

pub extern "C" fn ssrf_policy(ctx: *mut SpectraHostCallContext) -> i32 {
    let mut store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, store.ssrf_policies.insert(SsrfPolicy::new()))
}

pub extern "C" fn ssrf_allow_private_networks(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(policy) = read_ssrf_policy(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(
        ctx,
        store
            .ssrf_policies
            .insert(policy.allow_private_networks(bool_arg(args[1]))),
    )
}

pub extern "C" fn ssrf_allows(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(policy) = read_ssrf_policy(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(host) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let allowed = host
        .parse::<IpAddr>()
        .map(|ip| policy.allows_address(SocketAddr::new(ip, 0)))
        .unwrap_or(false);
    write_result(ctx, allowed as SpectraHostValue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::MiddlewareChain;

    fn request(method: Method, origin: Option<&str>) -> Request {
        let mut request = Request::new(method, "/mutate").expect("request");
        if let Some(origin) = origin {
            request = request.with_header(HEADER_ORIGIN, origin).expect("origin");
        }
        request
    }

    #[test]
    fn csrf_allows_safe_and_configured_origins_but_rejects_unknown_state_origin() {
        let policy = CsrfPolicy::new().allow_origin("https://app.example");
        let chain = MiddlewareChain::new().use_sync(policy.middleware());
        let safe = chain
            .execute_sync(
                request(Method::Get, Some("https://evil.example")),
                Response::new(Status::new(200).unwrap()),
            )
            .expect("safe request");
        assert_eq!(safe.0.status.code(), 200);

        let allowed = chain
            .execute_sync(
                request(Method::Post, Some("https://app.example")),
                Response::new(Status::new(200).unwrap()),
            )
            .expect("allowed origin");
        assert_eq!(allowed.0.status.code(), 200);

        let rejected = chain
            .execute_sync(
                request(Method::Post, Some("https://evil.example")),
                Response::new(Status::new(200).unwrap()),
            )
            .expect("CSRF response");
        assert_eq!(rejected.0.status.code(), 403);
        assert!(String::from_utf8_lossy(&rejected.0.body).contains("csrf_origin_rejected"));
    }

    #[test]
    fn csrf_allows_non_browser_state_change_without_origin() {
        let chain = MiddlewareChain::new().use_sync(CsrfPolicy::new().middleware());
        let result = chain
            .execute_sync(
                request(Method::Post, None),
                Response::new(Status::new(201).unwrap()),
            )
            .expect("non-browser request");
        assert_eq!(result.0.status.code(), 201);
    }

    #[test]
    fn ssrf_policy_blocks_private_and_link_local_addresses_by_default() {
        let policy = SsrfPolicy::default();
        assert!(!policy.allows_address("127.0.0.1:80".parse().unwrap()));
        assert!(!policy.allows_address("10.0.0.1:80".parse().unwrap()));
        assert!(!policy.allows_address("169.254.169.254:80".parse().unwrap()));
        assert!(!policy.allows_address("[::1]:80".parse().unwrap()));
        assert!(policy.allows_address("8.8.8.8:443".parse().unwrap()));
        assert!(policy
            .clone()
            .allow_private_networks(true)
            .allows_address("127.0.0.1:80".parse().unwrap()));
    }
}
