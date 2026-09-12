use crate::handler::HandlerError;
use crate::handles::ApiHandleTable;
use crate::http::{Request, Response, Status};
use crate::middleware::{Middleware, MiddlewareContext, MiddlewareDecision};
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use serde_json::json;
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
};
use spectra_runtime::handles::HandleKind;
use std::sync::{Mutex, OnceLock};

pub const ERROR_NONE: SpectraHostValue = 0;
const PROBLEM_TYPE: &str = "https://spectra.dev/problems/api-error";
const PROBLEM_CONTENT_TYPE: &str = "application/problem+json";
const DEFAULT_INTERNAL_CODE: &str = "internal_error";
const DEFAULT_INTERNAL_MESSAGE: &str = "internal server error";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiError {
    status: u16,
    public_code: String,
    public_message: String,
    internal_code: Option<String>,
    internal_detail: Option<String>,
}

impl ApiError {
    pub fn new(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: normalize_status(status),
            public_code: normalize_code(code.into(), "api_error"),
            public_message: normalize_message(message.into(), "request failed"),
            internal_code: None,
            internal_detail: None,
        }
    }

    pub fn internal(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::sanitized(
            500,
            DEFAULT_INTERNAL_CODE,
            DEFAULT_INTERNAL_MESSAGE,
            code,
            detail,
        )
    }

    pub fn sanitized(
        status: u16,
        public_code: impl Into<String>,
        public_message: impl Into<String>,
        internal_code: impl Into<String>,
        internal_detail: impl Into<String>,
    ) -> Self {
        Self {
            status: normalize_status(status),
            public_code: normalize_code(public_code.into(), DEFAULT_INTERNAL_CODE),
            public_message: normalize_message(public_message.into(), DEFAULT_INTERNAL_MESSAGE),
            internal_code: Some(normalize_code(internal_code.into(), DEFAULT_INTERNAL_CODE)),
            internal_detail: Some(internal_detail.into()),
        }
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn code(&self) -> &str {
        &self.public_code
    }

    pub fn message(&self) -> &str {
        &self.public_message
    }

    pub fn response(&self) -> Result<Response, HandlerError> {
        let status =
            Status::new(self.status).map_err(|error| HandlerError::new(500, error.to_string()))?;
        let body = json!({
            "type": PROBLEM_TYPE,
            "title": self.public_code,
            "status": self.status,
            "detail": self.public_message,
            "code": self.public_code,
        })
        .to_string()
        .into_bytes();
        Response::new(status)
            .with_header("Content-Type", PROBLEM_CONTENT_TYPE)
            .map_err(|error| HandlerError::new(500, error.to_string()))
            .map(|response| response.with_body(body))
    }

    fn log_entry(&self) -> Option<ErrorLogEntry> {
        Some(ErrorLogEntry {
            status: self.status,
            code: self
                .internal_code
                .clone()
                .unwrap_or_else(|| self.public_code.clone()),
            detail: self
                .internal_detail
                .clone()
                .unwrap_or_else(|| self.public_message.clone()),
        })
    }
}

fn normalize_status(status: u16) -> u16 {
    if (400..=599).contains(&status) {
        status
    } else {
        500
    }
}

fn normalize_code(code: String, fallback: &str) -> String {
    if code.trim().is_empty() {
        fallback.to_string()
    } else {
        code
    }
}

fn normalize_message(message: String, fallback: &str) -> String {
    if message.trim().is_empty() {
        fallback.to_string()
    } else {
        message
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ErrorLogEntry {
    pub(crate) status: u16,
    pub(crate) code: String,
    pub(crate) detail: String,
}

struct ErrorStore {
    errors: ApiHandleTable<ApiError>,
    last_error: Option<ApiError>,
    logs: Vec<ErrorLogEntry>,
}

impl ErrorStore {
    fn new() -> Self {
        Self {
            errors: ApiHandleTable::new(HandleKind::ApiError),
            last_error: None,
            logs: Vec::new(),
        }
    }

    fn insert(&mut self, error: ApiError) -> SpectraHostValue {
        self.last_error = Some(error.clone());
        self.errors.insert(error)
    }

    fn log(&mut self, error: &ApiError) {
        if let Some(entry) = error.log_entry() {
            eprintln!(
                "spectra.api internal error code={} status={} detail={}",
                entry.code, entry.status, entry.detail
            );
            self.logs.push(entry);
        }
    }
}

fn store() -> &'static Mutex<ErrorStore> {
    static STORE: OnceLock<Mutex<ErrorStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(ErrorStore::new()))
}

pub(crate) fn log_internal_failure(error: &HandlerError, config: &ExceptionConfig) {
    let api_error = ApiError::sanitized(
        config.status,
        config.public_code.clone(),
        config.public_message.clone(),
        "middleware_error",
        error.message.clone(),
    );
    let mut store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store.last_error = Some(api_error.clone());
    store.log(&api_error);
}

#[derive(Clone, Debug)]
pub(crate) struct ExceptionConfig {
    pub(crate) status: u16,
    pub(crate) public_code: String,
    pub(crate) public_message: String,
}

impl ExceptionConfig {
    fn new(status: u16, public_code: String, public_message: String) -> Self {
        Self {
            status: normalize_status(status),
            public_code: normalize_code(public_code, DEFAULT_INTERNAL_CODE),
            public_message: normalize_message(public_message, DEFAULT_INTERNAL_MESSAGE),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ExceptionMiddleware {
    config: ExceptionConfig,
}

impl ExceptionMiddleware {
    pub(crate) fn new(config: ExceptionConfig) -> Self {
        Self { config }
    }
}

impl Middleware for ExceptionMiddleware {
    fn on_request(
        &self,
        request: Request,
        _context: &mut MiddlewareContext,
    ) -> Result<MiddlewareDecision, HandlerError> {
        Ok(MiddlewareDecision::Continue(request))
    }

    fn on_error(
        &self,
        error: HandlerError,
        _context: &mut MiddlewareContext,
    ) -> Result<Response, HandlerError> {
        log_internal_failure(&error, &self.config);
        ApiError::sanitized(
            self.config.status,
            self.config.public_code.clone(),
            self.config.public_message.clone(),
            "middleware_error",
            error.message,
        )
        .response()
    }
}

pub extern "C" fn last_code(ctx: *mut SpectraHostCallContext) -> i32 {
    write_result(ctx, ERROR_NONE)
}

pub extern "C" fn last_message(ctx: *mut SpectraHostCallContext) -> i32 {
    write_result(ctx, alloc_spectra_string(""))
}

pub extern "C" fn new(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(status) = u16::try_from(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(code), Some(message)) = (read_spectra_string(args[1]), read_spectra_string(args[2]))
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_result(ctx, store.insert(ApiError::new(status, code, message)))
}

pub extern "C" fn internal(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(code), Some(detail)) = (read_spectra_string(args[0]), read_spectra_string(args[1]))
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let error = ApiError::internal(code, detail);
    let mut store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    store.log(&error);
    write_result(ctx, store.insert(error))
}

pub extern "C" fn status(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(error) = store.errors.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, error.status() as SpectraHostValue)
}

pub extern "C" fn code(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(error) = store.errors.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(error.code()))
}

pub extern "C" fn message(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(error) = store.errors.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(error.message()))
}

pub extern "C" fn response(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let error = {
        let store = store()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(error) = store.errors.get(&args[0]).cloned() else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        error
    };
    match error.response() {
        Ok(response) => write_result(ctx, crate::http::store_response(response)),
        Err(_) => HOST_STATUS_INVALID_ARGUMENT,
    }
}

pub extern "C" fn exception_middleware(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(status) = u16::try_from(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(public_code), Some(public_message)) =
        (read_spectra_string(args[1]), read_spectra_string(args[2]))
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let handle = crate::middleware::register_sync_middleware(ExceptionMiddleware::new(
        ExceptionConfig::new(status, public_code, public_message),
    ));
    write_result(ctx, handle)
}

#[cfg(test)]
pub(crate) fn logged_errors() -> Vec<ErrorLogEntry> {
    store()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .logs
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Method;
    use crate::middleware::AsyncMiddleware;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    struct FailingMiddleware;

    impl Middleware for FailingMiddleware {
        fn on_request(
            &self,
            _request: Request,
            _context: &mut MiddlewareContext,
        ) -> Result<MiddlewareDecision, HandlerError> {
            Err(HandlerError::new(500, "database password=secret"))
        }
    }

    struct FailingAsyncMiddleware;

    impl AsyncMiddleware for FailingAsyncMiddleware {
        fn on_request<'a>(
            &'a self,
            _request: Request,
            _context: &'a mut MiddlewareContext,
        ) -> Pin<Box<dyn Future<Output = Result<MiddlewareDecision, HandlerError>> + Send + 'a>>
        {
            Box::pin(async { Err(HandlerError::new(500, "async token=secret")) })
        }
    }

    fn block_on_ready<F: Future>(future: F) -> F::Output {
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        fn wake(_: *const ()) {}
        fn wake_by_ref(_: *const ()) {}
        fn drop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
        let raw = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw) };
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("test future unexpectedly pending"),
        }
    }

    fn request() -> Request {
        Request::new(Method::Get, "/errors").expect("valid request")
    }

    #[test]
    fn public_error_maps_to_deterministic_problem_response() {
        let error = ApiError::new(404, "not_found", "user does not exist");
        let response = error.response().expect("response");
        assert_eq!(response.status.code(), 404);
        assert_eq!(response.header("content-type"), Some(PROBLEM_CONTENT_TYPE));
        let body = String::from_utf8(response.body).expect("utf8 problem");
        assert!(body.contains("not_found"));
        assert!(body.contains("user does not exist"));
    }

    #[test]
    fn internal_error_logs_full_detail_and_sanitizes_response() {
        let error = ApiError::internal("database", "password=secret");
        {
            let mut store = store()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            store.log(&error);
        }
        let response = error.response().expect("response");
        let body = String::from_utf8(response.body).expect("utf8 problem");
        assert!(body.contains(DEFAULT_INTERNAL_MESSAGE));
        assert!(!body.contains("password=secret"));
        assert!(logged_errors()
            .iter()
            .any(|entry| entry.detail == "password=secret"));
    }

    #[test]
    fn exception_middleware_recovers_per_chain_with_custom_public_mapping() {
        let middleware = ExceptionMiddleware::new(ExceptionConfig::new(
            503,
            "maintenance".to_string(),
            "service temporarily unavailable".to_string(),
        ));
        let chain = crate::middleware::MiddlewareChain::new()
            .use_sync(middleware)
            .use_sync(FailingMiddleware);
        let (response, _) = chain
            .execute_sync(request(), Response::new(Status::new(200).expect("status")))
            .expect("exception middleware response");
        assert_eq!(response.status.code(), 503);
        let body = String::from_utf8(response.body).expect("utf8 problem");
        assert!(body.contains("maintenance"));
        assert!(body.contains("service temporarily unavailable"));
        assert!(!body.contains("database password=secret"));
        assert!(logged_errors()
            .iter()
            .any(|entry| entry.detail == "database password=secret"));

        let async_chain = crate::middleware::MiddlewareChain::new()
            .use_sync(ExceptionMiddleware::new(ExceptionConfig::new(
                503,
                "maintenance".to_string(),
                "service temporarily unavailable".to_string(),
            )))
            .use_async(FailingAsyncMiddleware);
        let (async_response, _) = block_on_ready(
            async_chain.execute_async(request(), Response::new(Status::new(200).expect("status"))),
        )
        .expect("async exception middleware response");
        assert_eq!(async_response.status.code(), 503);
        let async_body = String::from_utf8(async_response.body).expect("utf8 problem");
        assert!(async_body.contains("maintenance"));
        assert!(!async_body.contains("async token=secret"));
        assert!(logged_errors()
            .iter()
            .any(|entry| entry.detail == "async token=secret"));
    }
}
