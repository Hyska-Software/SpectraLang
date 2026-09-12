use crate::handles::ApiHandleTable;
use crate::http::{self, Request, Response, Status};
use crate::{alloc_spectra_string, read_args, read_spectra_string, write_result};
use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT, HOST_STATUS_SUCCESS,
};
use spectra_runtime::handles::HandleKind;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Mutex, OnceLock};

const CONTENT_TYPE: &str = "Content-Type";
const TEXT_PLAIN: &str = "text/plain; charset=utf-8";
const APPLICATION_JSON: &str = "application/json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandlerError {
    pub status: u16,
    pub message: String,
}

impl HandlerError {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        let status = if (400..=599).contains(&status) {
            status
        } else {
            500
        };
        Self {
            status,
            message: message.into(),
        }
    }
}

pub trait IntoResponse {
    fn into_response(self) -> Result<Response, HandlerError>;
}

impl IntoResponse for Response {
    fn into_response(self) -> Result<Response, HandlerError> {
        Ok(self)
    }
}

impl IntoResponse for String {
    fn into_response(self) -> Result<Response, HandlerError> {
        text_response(200, self)
    }
}

impl IntoResponse for &str {
    fn into_response(self) -> Result<Response, HandlerError> {
        text_response(200, self.to_string())
    }
}

impl IntoResponse for Vec<u8> {
    fn into_response(self) -> Result<Response, HandlerError> {
        response_with_body(200, None, self)
    }
}

impl IntoResponse for () {
    fn into_response(self) -> Result<Response, HandlerError> {
        response_with_body(204, None, Vec::new())
    }
}

impl IntoResponse for HandlerError {
    fn into_response(self) -> Result<Response, HandlerError> {
        error_to_response(&self)
    }
}

impl<T> IntoResponse for Result<T, HandlerError>
where
    T: IntoResponse,
{
    fn into_response(self) -> Result<Response, HandlerError> {
        match self {
            Ok(value) => value.into_response(),
            Err(error) => error.into_response(),
        }
    }
}

pub trait Handler: Send + Sync + 'static {
    fn call(&self, request: Request) -> Result<Response, HandlerError>;
}

impl<F, R> Handler for F
where
    F: Fn(Request) -> R + Send + Sync + 'static,
    R: IntoResponse,
{
    fn call(&self, request: Request) -> Result<Response, HandlerError> {
        (self)(request).into_response()
    }
}

pub trait AsyncHandler: Send + Sync + 'static {
    fn call<'a>(
        &'a self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Response, HandlerError>> + Send + 'a>>;
}

impl<F, Fut, R> AsyncHandler for F
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = R> + Send + 'static,
    R: IntoResponse + 'static,
{
    fn call<'a>(
        &'a self,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Response, HandlerError>> + Send + 'a>> {
        Box::pin(async move { (self)(request).await.into_response() })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HandlerKind {
    Sync,
    Async,
}

pub(crate) type ClosureInvoker = unsafe extern "C" fn(i64, *const i64, usize, *mut i64) -> i32;

#[derive(Clone, Copy, Debug)]
pub(crate) struct CallbackEntry {
    pub(crate) closure: SpectraHostValue,
    pub(crate) invoke: ClosureInvoker,
}

#[derive(Clone, Debug)]
struct HandlerEntry {
    route_id: SpectraHostValue,
    response: Option<SpectraHostValue>,
    callback: Option<CallbackEntry>,
    kind: HandlerKind,
}

struct HandlerStore {
    handlers: ApiHandleTable<HandlerEntry>,
    errors: ApiHandleTable<HandlerError>,
    last_error: Option<HandlerError>,
}

impl HandlerStore {
    fn new() -> Self {
        Self {
            handlers: ApiHandleTable::new(HandleKind::ApiHandler),
            errors: ApiHandleTable::new(HandleKind::ApiHandlerError),
            last_error: None,
        }
    }

    fn handler_handle(&mut self, entry: HandlerEntry) -> SpectraHostValue {
        self.handlers.insert(entry)
    }

    fn error_handle(&mut self, error: HandlerError) -> SpectraHostValue {
        self.last_error = Some(error.clone());
        self.errors.insert(error)
    }
}

fn store() -> &'static Mutex<HandlerStore> {
    static STORE: OnceLock<Mutex<HandlerStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HandlerStore::new()))
}

fn response_with_body(
    status: u16,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> Result<Response, HandlerError> {
    let status =
        Status::new(status).map_err(|_| HandlerError::new(500, "invalid response status"))?;
    let mut response = Response::new(status).with_body(body);
    if let Some(content_type) = content_type {
        response = response
            .with_header(CONTENT_TYPE, content_type)
            .map_err(|err| HandlerError::new(500, err.to_string()))?;
    }
    Ok(response)
}

fn text_response(status: u16, body: String) -> Result<Response, HandlerError> {
    response_with_body(status, Some(TEXT_PLAIN), body.into_bytes())
}

fn json_response(status: u16, body: String) -> Result<Response, HandlerError> {
    response_with_body(status, Some(APPLICATION_JSON), body.into_bytes())
}

fn error_to_response(error: &HandlerError) -> Result<Response, HandlerError> {
    text_response(error.status, error.message.clone())
}

fn write_response(
    ctx: *mut SpectraHostCallContext,
    response: Result<Response, HandlerError>,
) -> i32 {
    match response {
        Ok(response) => write_result(ctx, http::store_response(response)),
        Err(error) => {
            let response = error_to_response(&error);
            store().lock().unwrap_or_else(|e| e.into_inner()).last_error = Some(error);
            match response {
                Ok(response) => write_result(ctx, http::store_response(response)),
                Err(_) => HOST_STATUS_INVALID_ARGUMENT,
            }
        }
    }
}

pub(crate) fn response_for_route(route_id: SpectraHostValue) -> Option<Response> {
    let handler_store = store().lock().unwrap_or_else(|e| e.into_inner());
    let response = handler_store
        .handlers
        .iter()
        .filter(|(_, entry)| entry.route_id == route_id)
        .max_by_key(|(handle, _)| *handle)
        .and_then(|(_, entry)| entry.response);
    drop(handler_store);
    response.and_then(http::clone_response)
}

pub(crate) enum RegisteredHandler {
    Response {
        handle: SpectraHostValue,
        response: Response,
    },
    Callback {
        kind: HandlerKind,
        callback: CallbackEntry,
    },
}

pub(crate) fn registered_handler_for_route(
    route_id: SpectraHostValue,
) -> Option<RegisteredHandler> {
    let handler_store = store().lock().unwrap_or_else(|e| e.into_inner());
    let entry = handler_store
        .handlers
        .iter()
        .filter(|(_, entry)| entry.route_id == route_id)
        .max_by_key(|(handle, _)| *handle)
        .map(|(_, entry)| entry.clone())?;
    drop(handler_store);

    match (entry.response, entry.callback) {
        (Some(handle), _) => response_for_route(route_id)
            .map(|response| RegisteredHandler::Response { handle, response }),
        (None, Some(callback)) => Some(RegisteredHandler::Callback {
            kind: entry.kind,
            callback,
        }),
        (None, None) => None,
    }
}

pub(crate) fn invoke_callback(
    callback: CallbackEntry,
    request: SpectraHostValue,
) -> Result<SpectraHostValue, i32> {
    let args = [request];
    let mut result = 0_i64;
    let status =
        unsafe { (callback.invoke)(callback.closure, args.as_ptr(), args.len(), &mut result) };
    if status == HOST_STATUS_SUCCESS {
        Ok(result)
    } else {
        Err(status)
    }
}

#[cfg(test)]
pub(crate) fn register_sync_response_for_route(
    route_id: SpectraHostValue,
    response: Response,
) -> SpectraHostValue {
    let response = http::store_response(response);
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    store.handler_handle(HandlerEntry {
        route_id,
        response: Some(response),
        callback: None,
        kind: HandlerKind::Sync,
    })
}

#[cfg(test)]
pub(crate) fn register_sync_response_handle_for_route(
    route_id: SpectraHostValue,
    response: SpectraHostValue,
) -> SpectraHostValue {
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    store.handler_handle(HandlerEntry {
        route_id,
        response: Some(response),
        callback: None,
        kind: HandlerKind::Sync,
    })
}

pub extern "C" fn text(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(body) = read_spectra_string(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_response(ctx, text_response(200, body))
}

pub extern "C" fn json(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(body) = read_spectra_string(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_response(ctx, json_response(200, body))
}

pub extern "C" fn bytes(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(body) = read_spectra_string(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_response(ctx, response_with_body(200, None, body.into_bytes()))
}

pub extern "C" fn status(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(status) = u16::try_from(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_response(ctx, response_with_body(status, None, Vec::new()))
}

pub extern "C" fn with_header(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(name), Some(value)) = (read_spectra_string(args[1]), read_spectra_string(args[2]))
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(response) = http::clone_response(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let response = response
        .with_header(name, value)
        .map_err(|err| HandlerError::new(500, err.to_string()));
    write_response(ctx, response)
}

pub extern "C" fn into_response(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(response) = http::clone_response(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_response(ctx, response.into_response())
}

pub extern "C" fn into_text_response(ctx: *mut SpectraHostCallContext) -> i32 {
    text(ctx)
}

pub extern "C" fn into_status_response(ctx: *mut SpectraHostCallContext) -> i32 {
    status(ctx)
}

pub extern "C" fn error(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(status) = u16::try_from(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(message) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let handle = store.error_handle(HandlerError::new(status, message));
    write_result(ctx, handle)
}

pub extern "C" fn error_response(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(error) = store.errors.get(&args[0]).cloned() else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    drop(store);
    write_response(ctx, error.into_response())
}

pub extern "C" fn error_code(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(error) = store.errors.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, error.status as SpectraHostValue)
}

pub extern "C" fn error_message(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(error) = store.errors.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(&error.message))
}

pub extern "C" fn last_error_message(ctx: *mut SpectraHostCallContext) -> i32 {
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let message = store
        .last_error
        .as_ref()
        .map(|error| error.message.as_str())
        .unwrap_or("");
    write_result(ctx, alloc_spectra_string(message))
}

pub extern "C" fn register_sync(ctx: *mut SpectraHostCallContext) -> i32 {
    register_handler(ctx, HandlerKind::Sync)
}

pub extern "C" fn register_async(ctx: *mut SpectraHostCallContext) -> i32 {
    register_handler(ctx, HandlerKind::Async)
}

pub extern "C" fn register_sync_callback(ctx: *mut SpectraHostCallContext) -> i32 {
    register_callback(ctx, HandlerKind::Sync)
}

pub extern "C" fn register_async_callback(ctx: *mut SpectraHostCallContext) -> i32 {
    register_callback(ctx, HandlerKind::Async)
}

fn register_handler(ctx: *mut SpectraHostCallContext, kind: HandlerKind) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if http::clone_response(args[1]).is_none() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let handle = store.handler_handle(HandlerEntry {
        route_id: args[0],
        response: Some(args[1]),
        callback: None,
        kind,
    });
    write_result(ctx, handle)
}

fn register_callback(ctx: *mut SpectraHostCallContext, kind: HandlerKind) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(invoke) = (unsafe { ctx.as_ref() }).and_then(|ctx| ctx.invoke_fn) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if args[1] == 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    let handle = store.handler_handle(HandlerEntry {
        route_id: args[0],
        response: None,
        callback: Some(CallbackEntry {
            closure: args[1],
            invoke,
        }),
        kind,
    });
    write_result(ctx, handle)
}

pub extern "C" fn dispatch_sync(ctx: *mut SpectraHostCallContext) -> i32 {
    dispatch_handler(ctx, HandlerKind::Sync)
}

pub extern "C" fn dispatch_async(ctx: *mut SpectraHostCallContext) -> i32 {
    dispatch_handler(ctx, HandlerKind::Async)
}

fn dispatch_handler(ctx: *mut SpectraHostCallContext, expected: HandlerKind) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let handler_store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = handler_store.handlers.get(&args[0]).cloned() else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    drop(handler_store);
    if entry.kind != expected {
        let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
        store.last_error = Some(HandlerError::new(500, "handler kind mismatch"));
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let _route_id = entry.route_id;
    if let Some(response) = entry.response.and_then(http::clone_response) {
        return write_response(ctx, Ok(response));
    }
    let Some(callback) = entry.callback else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(result) = invoke_callback(callback, args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match entry.kind {
        HandlerKind::Sync => {
            let Some(response) = http::clone_response(result) else {
                return HOST_STATUS_INVALID_ARGUMENT;
            };
            write_response(ctx, Ok(response))
        }
        HandlerKind::Async => {
            let Ok(response_handle) = spectra_runtime::stdlib::block_on_task_value(result) else {
                return HOST_STATUS_INVALID_ARGUMENT;
            };
            let Some(response) = http::clone_response(response_handle) else {
                return HOST_STATUS_INVALID_ARGUMENT;
            };
            write_response(ctx, Ok(response))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Method;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn request() -> Request {
        Request::new(Method::Get, "/users").expect("valid request")
    }

    fn block_on_ready<F: Future + ?Sized>(mut future: Pin<Box<F>>) -> F::Output {
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        fn wake(_: *const ()) {}
        fn wake_by_ref(_: *const ()) {}
        fn drop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
        let raw = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw) };
        let mut cx = Context::from_waker(&waker);
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("test future unexpectedly pending"),
        }
    }

    #[test]
    fn into_response_covers_text_bytes_unit_error_and_result() {
        let text = "hello".to_string().into_response().expect("text response");
        assert_eq!(text.status.code(), 200);
        assert_eq!(text.header(CONTENT_TYPE), Some(TEXT_PLAIN));
        assert_eq!(text.body, b"hello");

        let bytes = vec![1_u8, 2, 3].into_response().expect("bytes response");
        assert_eq!(bytes.status.code(), 200);
        assert_eq!(bytes.body, vec![1, 2, 3]);

        let empty = ().into_response().expect("unit response");
        assert_eq!(empty.status.code(), 204);
        assert!(empty.body.is_empty());

        let failed: Result<String, HandlerError> = Err(HandlerError::new(404, "missing"));
        let response = failed.into_response().expect("error response");
        assert_eq!(response.status.code(), 404);
        assert_eq!(response.body, b"missing");
    }

    #[test]
    fn sync_handler_accepts_any_into_response_return() {
        let handler = |request: Request| {
            assert_eq!(request.path, "/users");
            "created".to_string()
        };
        let response = Handler::call(&handler, request()).expect("handler response");
        assert_eq!(response.status.code(), 200);
        assert_eq!(response.body, b"created");
    }

    #[test]
    fn async_handler_accepts_any_into_response_return() {
        let handler = |_request: Request| async { "async ok".to_string() };
        let future = AsyncHandler::call(&handler, request());
        let response = block_on_ready(future).expect("async response");
        assert_eq!(response.status.code(), 200);
        assert_eq!(response.body, b"async ok");
    }

    #[test]
    fn host_registration_dispatches_sync_and_async_handlers() {
        let response = http::store_response(text_response(200, "ready".to_string()).unwrap());
        let mut handler_store = store().lock().unwrap_or_else(|e| e.into_inner());
        let sync = handler_store.handler_handle(HandlerEntry {
            route_id: 7,
            response: Some(response),
            callback: None,
            kind: HandlerKind::Sync,
        });
        let async_h = handler_store.handler_handle(HandlerEntry {
            route_id: 8,
            response: Some(response),
            callback: None,
            kind: HandlerKind::Async,
        });
        drop(handler_store);
        let handler_store = store().lock().unwrap_or_else(|e| e.into_inner());
        assert!(handler_store.handlers.contains_key(&sync));
        assert!(handler_store.handlers.contains_key(&async_h));
    }
}
