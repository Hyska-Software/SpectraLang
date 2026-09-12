use crate::handler::HandlerError;
use crate::handles::ApiHandleTable;
use crate::http::{self, Method, Request, Response, Status};
use crate::middleware::{self, Middleware, MiddlewareContext, MiddlewareDecision};
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
};
use spectra_runtime::handles::HandleKind;
use std::sync::{Mutex, OnceLock};

const HEADER_ORIGIN: &str = "Origin";
const HEADER_REQUEST_METHOD: &str = "Access-Control-Request-Method";
const HEADER_REQUEST_HEADERS: &str = "Access-Control-Request-Headers";
const HEADER_ALLOW_ORIGIN: &str = "Access-Control-Allow-Origin";
const HEADER_ALLOW_METHODS: &str = "Access-Control-Allow-Methods";
const HEADER_ALLOW_HEADERS: &str = "Access-Control-Allow-Headers";
const HEADER_ALLOW_CREDENTIALS: &str = "Access-Control-Allow-Credentials";
const HEADER_EXPOSE_HEADERS: &str = "Access-Control-Expose-Headers";
const HEADER_MAX_AGE: &str = "Access-Control-Max-Age";
const HEADER_VARY: &str = "Vary";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorsPolicy {
    allow_any_origin: bool,
    allowed_origins: Vec<String>,
    allowed_methods: Vec<Method>,
    allowed_headers: Vec<String>,
    exposed_headers: Vec<String>,
    allow_credentials: bool,
    max_age: Option<u32>,
}

impl Default for CorsPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl CorsPolicy {
    pub fn new() -> Self {
        Self {
            allow_any_origin: false,
            allowed_origins: Vec::new(),
            allowed_methods: Vec::new(),
            allowed_headers: Vec::new(),
            exposed_headers: Vec::new(),
            allow_credentials: false,
            max_age: None,
        }
    }

    pub fn permissive() -> Self {
        Self {
            allow_any_origin: true,
            allowed_origins: Vec::new(),
            allowed_methods: all_methods().to_vec(),
            allowed_headers: Vec::new(),
            exposed_headers: Vec::new(),
            allow_credentials: false,
            max_age: Some(600),
        }
    }

    pub fn allow_origin(mut self, origin: impl Into<String>) -> Self {
        let origin = origin.into();
        if origin == "*" {
            self.allow_any_origin = true;
        } else if !self.allowed_origins.iter().any(|value| value == &origin) {
            self.allowed_origins.push(origin);
        }
        self
    }

    pub fn allow_method(mut self, method: Method) -> Self {
        if !self.allowed_methods.contains(&method) {
            self.allowed_methods.push(method);
        }
        self
    }

    pub fn allow_header(mut self, header: impl Into<String>) -> Result<Self, HandlerError> {
        let header = header.into();
        if header.trim().is_empty() {
            return Err(HandlerError::new(
                400,
                "CORS allowed header cannot be empty",
            ));
        }
        if !self
            .allowed_headers
            .iter()
            .any(|value| value.eq_ignore_ascii_case(&header))
        {
            self.allowed_headers.push(header);
        }
        Ok(self)
    }

    pub fn expose_header(mut self, header: impl Into<String>) -> Result<Self, HandlerError> {
        let header = header.into();
        if header.trim().is_empty() {
            return Err(HandlerError::new(
                400,
                "CORS exposed header cannot be empty",
            ));
        }
        if !self
            .exposed_headers
            .iter()
            .any(|value| value.eq_ignore_ascii_case(&header))
        {
            self.exposed_headers.push(header);
        }
        Ok(self)
    }

    pub fn allow_credentials(mut self, allow: bool) -> Self {
        self.allow_credentials = allow;
        self
    }

    pub fn max_age(mut self, seconds: u32) -> Self {
        self.max_age = Some(seconds);
        self
    }

    pub fn is_preflight_request(&self, request: &Request) -> bool {
        is_preflight_request(request)
    }

    pub fn preflight_response(&self, request: &Request) -> Result<Response, HandlerError> {
        let Some(origin) = request.header(HEADER_ORIGIN) else {
            return forbidden_response();
        };
        let Some(requested_method) = request.header(HEADER_REQUEST_METHOD) else {
            return forbidden_response();
        };
        if self.allowed_origin_value(origin).is_none() || !self.method_allowed(requested_method) {
            return forbidden_response();
        }
        if let Some(requested_headers) = request.header(HEADER_REQUEST_HEADERS) {
            if !self.request_headers_allowed(requested_headers) {
                return forbidden_response();
            }
        }

        let mut response = Response::new(Status::new(204).expect("valid CORS status"));
        response = self.apply_origin(response, origin)?;
        response = response
            .with_header(
                HEADER_ALLOW_METHODS,
                self.allow_methods_value(requested_method),
            )
            .map_err(to_handler_error)?;
        if let Some(value) = self.allow_headers_value(request.header(HEADER_REQUEST_HEADERS)) {
            response = response
                .with_header(HEADER_ALLOW_HEADERS, value)
                .map_err(to_handler_error)?;
        }
        if self.allow_credentials {
            response = response
                .with_header(HEADER_ALLOW_CREDENTIALS, "true")
                .map_err(to_handler_error)?;
        }
        if let Some(max_age) = self.max_age {
            response = response
                .with_header(HEADER_MAX_AGE, max_age.to_string())
                .map_err(to_handler_error)?;
        }
        Ok(response)
    }

    pub fn apply_actual_response(
        &self,
        request: &Request,
        response: Response,
    ) -> Result<Response, HandlerError> {
        let Some(origin) = request.header(HEADER_ORIGIN) else {
            return Ok(response);
        };
        if self.allowed_origin_value(origin).is_none() {
            return Ok(response);
        }
        self.apply_actual_origin(origin, response)
    }

    fn apply_actual_origin(
        &self,
        origin: &str,
        mut response: Response,
    ) -> Result<Response, HandlerError> {
        response = self.apply_origin(response, origin)?;
        if !self.exposed_headers.is_empty() {
            response = response
                .with_header(
                    HEADER_EXPOSE_HEADERS,
                    join_header_values(&self.exposed_headers),
                )
                .map_err(to_handler_error)?;
        }
        if self.allow_credentials {
            response = response
                .with_header(HEADER_ALLOW_CREDENTIALS, "true")
                .map_err(to_handler_error)?;
        }
        Ok(response)
    }

    fn apply_origin(&self, response: Response, origin: &str) -> Result<Response, HandlerError> {
        let Some(value) = self.allowed_origin_value(origin) else {
            return Ok(response);
        };
        let mut response = response
            .with_header(HEADER_ALLOW_ORIGIN, value)
            .map_err(to_handler_error)?;
        if self.allow_credentials || !self.allow_any_origin {
            response = add_vary(response, HEADER_ORIGIN)?;
        }
        Ok(response)
    }

    fn allowed_origin_value(&self, origin: &str) -> Option<String> {
        if self.allow_any_origin {
            if self.allow_credentials {
                Some(origin.to_string())
            } else {
                Some("*".to_string())
            }
        } else if self.allowed_origins.iter().any(|value| value == origin) {
            Some(origin.to_string())
        } else {
            None
        }
    }

    fn method_allowed(&self, requested_method: &str) -> bool {
        if self.allowed_methods.is_empty() {
            return false;
        }
        self.allowed_methods
            .iter()
            .any(|method| method.as_str().eq_ignore_ascii_case(requested_method))
    }

    fn request_headers_allowed(&self, requested_headers: &str) -> bool {
        if requested_headers.trim().is_empty() {
            return true;
        }
        if self.allowed_headers.is_empty() && self.allow_any_origin && !self.allow_credentials {
            return true;
        }
        requested_headers
            .split(',')
            .map(str::trim)
            .filter(|header| !header.is_empty())
            .all(|header| {
                self.allowed_headers
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(header))
            })
    }

    fn allow_methods_value(&self, requested_method: &str) -> String {
        if self.allowed_methods.is_empty() {
            requested_method.to_string()
        } else {
            self.allowed_methods
                .iter()
                .map(|method| method.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    fn allow_headers_value(&self, requested_headers: Option<&str>) -> Option<String> {
        if !self.allowed_headers.is_empty() {
            Some(join_header_values(&self.allowed_headers))
        } else {
            requested_headers
                .filter(|value| !value.trim().is_empty())
                .map(|value| value.to_string())
        }
    }
}

fn add_vary(response: Response, value: &str) -> Result<Response, HandlerError> {
    let Some(existing) = response.header(HEADER_VARY).map(str::to_string) else {
        return response
            .with_header(HEADER_VARY, value)
            .map_err(to_handler_error);
    };
    if existing.split(',').any(|token| {
        let token = token.trim();
        token == "*" || token.eq_ignore_ascii_case(value)
    }) {
        return Ok(response);
    }
    response
        .with_header(HEADER_VARY, format!("{existing}, {value}"))
        .map_err(to_handler_error)
}

#[derive(Clone)]
struct CorsMiddleware {
    policy: CorsPolicy,
}

impl Middleware for CorsMiddleware {
    fn on_request(
        &self,
        request: Request,
        context: &mut MiddlewareContext,
    ) -> Result<MiddlewareDecision, HandlerError> {
        if self.policy.is_preflight_request(&request) {
            return Ok(MiddlewareDecision::ShortCircuit(
                self.policy.preflight_response(&request)?,
            ));
        }
        if let Some(origin) = request.header(HEADER_ORIGIN) {
            if self.policy.allowed_origin_value(origin).is_some() {
                context.set_cors_origin(origin.to_string());
            }
        }
        Ok(MiddlewareDecision::Continue(request))
    }

    fn on_response(
        &self,
        response: Response,
        context: &mut MiddlewareContext,
    ) -> Result<Response, HandlerError> {
        if let Some(origin) = context.cors_origin() {
            self.policy.apply_actual_origin(origin, response)
        } else {
            Ok(response)
        }
    }
}

struct CorsStore {
    policies: ApiHandleTable<CorsPolicy>,
}

impl CorsStore {
    fn new() -> Self {
        Self {
            policies: ApiHandleTable::new(HandleKind::ApiCorsPolicy),
        }
    }

    fn insert(&mut self, policy: CorsPolicy) -> SpectraHostValue {
        self.policies.insert(policy)
    }
}

fn store() -> &'static Mutex<CorsStore> {
    static STORE: OnceLock<Mutex<CorsStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(CorsStore::new()))
}

fn store_policy(policy: CorsPolicy) -> SpectraHostValue {
    store()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(policy)
}

fn clone_policy(handle: SpectraHostValue) -> Option<CorsPolicy> {
    store()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .policies
        .get(&handle)
        .cloned()
}

fn read_policy(handle: SpectraHostValue) -> Result<CorsPolicy, i32> {
    clone_policy(handle).ok_or(HOST_STATUS_INVALID_ARGUMENT)
}

fn write_policy(ctx: *mut SpectraHostCallContext, policy: CorsPolicy) -> i32 {
    write_result(ctx, store_policy(policy))
}

fn bool_arg(value: SpectraHostValue) -> bool {
    value != 0
}

fn all_methods() -> &'static [Method] {
    &[
        Method::Get,
        Method::Head,
        Method::Post,
        Method::Put,
        Method::Patch,
        Method::Delete,
        Method::Options,
    ]
}

fn join_header_values(values: &[String]) -> String {
    values.join(", ")
}

fn is_preflight_request(request: &Request) -> bool {
    request.method == Method::Options
        && request.header(HEADER_ORIGIN).is_some()
        && request.header(HEADER_REQUEST_METHOD).is_some()
}

fn forbidden_response() -> Result<Response, HandlerError> {
    Response::new(Status::new(403).expect("valid CORS status"))
        .with_header(HEADER_VARY, HEADER_ORIGIN)
        .map_err(to_handler_error)
}

fn to_handler_error(err: http::HttpTypeError) -> HandlerError {
    HandlerError::new(500, err.to_string())
}

pub extern "C" fn policy(ctx: *mut SpectraHostCallContext) -> i32 {
    write_policy(ctx, CorsPolicy::new())
}

pub extern "C" fn permissive(ctx: *mut SpectraHostCallContext) -> i32 {
    write_policy(ctx, CorsPolicy::permissive())
}

pub extern "C" fn allow_origin(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(policy) = read_policy(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(origin) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_policy(ctx, policy.allow_origin(origin))
}

pub extern "C" fn allow_method(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(policy) = read_policy(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(method) = Method::from_code(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_policy(ctx, policy.allow_method(method))
}

pub extern "C" fn allow_header(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(policy) = read_policy(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(header) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(policy) = policy.allow_header(header) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_policy(ctx, policy)
}

pub extern "C" fn expose_header(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(policy) = read_policy(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(header) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(policy) = policy.expose_header(header) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_policy(ctx, policy)
}

pub extern "C" fn allow_credentials(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(policy) = read_policy(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_policy(ctx, policy.allow_credentials(bool_arg(args[1])))
}

pub extern "C" fn max_age(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(policy) = read_policy(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(seconds) = u32::try_from(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_policy(ctx, policy.max_age(seconds))
}

pub extern "C" fn middleware(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(policy) = read_policy(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        middleware::register_sync_middleware(CorsMiddleware { policy }),
    )
}

pub extern "C" fn is_preflight(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(request) = http::clone_request(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, if is_preflight_request(&request) { 1 } else { 0 })
}

pub extern "C" fn preflight(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(policy) = read_policy(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(request) = http::clone_request(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match policy.preflight_response(&request) {
        Ok(response) => write_result(ctx, http::store_response(response)),
        Err(_) => HOST_STATUS_INVALID_ARGUMENT,
    }
}

pub extern "C" fn apply(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(policy) = read_policy(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(request) = http::clone_request(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(response) = http::clone_response(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match policy.apply_actual_response(&request, response) {
        Ok(response) => write_result(ctx, http::store_response(response)),
        Err(_) => HOST_STATUS_INVALID_ARGUMENT,
    }
}

pub extern "C" fn allowed_origin(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(policy) = read_policy(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(origin) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        alloc_spectra_string(&policy.allowed_origin_value(&origin).unwrap_or_default()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: Method) -> Request {
        Request::new(method, "/resource")
            .expect("request")
            .with_header(HEADER_ORIGIN, "https://app.example")
            .expect("origin")
    }

    #[test]
    fn restrictive_preflight_emits_configured_cors_headers() {
        let request = request(Method::Options)
            .with_header(HEADER_REQUEST_METHOD, "POST")
            .expect("method")
            .with_header(HEADER_REQUEST_HEADERS, "Authorization, Content-Type")
            .expect("headers");
        let policy = CorsPolicy::new()
            .allow_origin("https://app.example")
            .allow_method(Method::Post)
            .allow_header("Authorization")
            .expect("header")
            .allow_header("Content-Type")
            .expect("header")
            .max_age(300);

        let response = policy.preflight_response(&request).expect("preflight");
        assert_eq!(response.status.code(), 204);
        assert_eq!(
            response.header(HEADER_ALLOW_ORIGIN),
            Some("https://app.example")
        );
        assert_eq!(response.header(HEADER_ALLOW_METHODS), Some("POST"));
        assert_eq!(
            response.header(HEADER_ALLOW_HEADERS),
            Some("Authorization, Content-Type")
        );
        assert_eq!(response.header(HEADER_MAX_AGE), Some("300"));
    }

    #[test]
    fn credentialed_policy_echoes_origin_and_varies() {
        let policy = CorsPolicy::permissive().allow_credentials(true);
        let response = policy
            .apply_actual_response(
                &request(Method::Get),
                Response::new(Status::new(200).unwrap()),
            )
            .expect("actual response");

        assert_eq!(
            response.header(HEADER_ALLOW_ORIGIN),
            Some("https://app.example")
        );
        assert_eq!(response.header(HEADER_ALLOW_CREDENTIALS), Some("true"));
        assert_eq!(response.header(HEADER_VARY), Some(HEADER_ORIGIN));
    }

    #[test]
    fn actual_cors_merges_vary_with_existing_compression_dimension() {
        let policy = CorsPolicy::new().allow_origin("https://app.example");
        let response = Response::new(Status::new(200).unwrap())
            .with_header(HEADER_VARY, "Accept-Encoding")
            .expect("vary header");
        let response = policy
            .apply_actual_response(&request(Method::Get), response)
            .expect("actual response");
        assert_eq!(
            response.header(HEADER_VARY),
            Some("Accept-Encoding, Origin")
        );
    }

    #[test]
    fn denied_origin_leaves_actual_response_unmodified_and_preflight_forbidden() {
        let policy = CorsPolicy::new().allow_origin("https://other.example");
        let response = policy
            .apply_actual_response(
                &request(Method::Get),
                Response::new(Status::new(200).unwrap()),
            )
            .expect("actual response");
        assert_eq!(response.header(HEADER_ALLOW_ORIGIN), None);

        let preflight = request(Method::Options)
            .with_header(HEADER_REQUEST_METHOD, "GET")
            .expect("method");
        let response = policy
            .preflight_response(&preflight)
            .expect("forbidden response");
        assert_eq!(response.status.code(), 403);
        assert_eq!(response.header(HEADER_VARY), Some(HEADER_ORIGIN));
    }
}
