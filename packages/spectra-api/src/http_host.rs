pub extern "C" fn method_name(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(method_label(args[0])))
}

pub extern "C" fn method_allows_body(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let allows = matches!(args[0], METHOD_POST | METHOD_PUT | METHOD_PATCH);
    write_result(ctx, allows as SpectraHostValue)
}

pub extern "C" fn method_is_safe(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let safe = matches!(args[0], METHOD_GET | METHOD_HEAD | METHOD_OPTIONS);
    write_result(ctx, safe as SpectraHostValue)
}

macro_rules! const_host_call {
    ($name:ident, $value:expr) => {
        pub extern "C" fn $name(ctx: *mut SpectraHostCallContext) -> i32 {
            write_result(ctx, $value)
        }
    };
}

const_host_call!(method_get, METHOD_GET);
const_host_call!(method_head, METHOD_HEAD);
const_host_call!(method_post, METHOD_POST);
const_host_call!(method_put, METHOD_PUT);
const_host_call!(method_patch, METHOD_PATCH);
const_host_call!(method_delete, METHOD_DELETE);
const_host_call!(method_options, METHOD_OPTIONS);
const_host_call!(status_continue, 100);
const_host_call!(status_switching_protocols, 101);
const_host_call!(status_ok, 200);
const_host_call!(status_created, 201);
const_host_call!(status_accepted, 202);
const_host_call!(status_no_content, 204);
const_host_call!(status_moved_permanently, 301);
const_host_call!(status_found, 302);
const_host_call!(status_not_modified, 304);
const_host_call!(status_bad_request, 400);
const_host_call!(status_unauthorized, 401);
const_host_call!(status_forbidden, 403);
const_host_call!(status_not_found, 404);
const_host_call!(status_method_not_allowed, 405);
const_host_call!(status_conflict, 409);
const_host_call!(status_unsupported_media_type, 415);
const_host_call!(status_unprocessable_content, 422);
const_host_call!(status_too_many_requests, 429);
const_host_call!(status_internal_server_error, 500);
const_host_call!(status_bad_gateway, 502);
const_host_call!(status_service_unavailable, 503);
const_host_call!(status_gateway_timeout, 504);

pub extern "C" fn status_reason(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(status_reason_phrase(args[0])))
}

pub extern "C" fn status_class(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let status = args[0];
    let class = if (100..=599).contains(&status) {
        status / 100
    } else {
        0
    };
    write_result(ctx, class)
}

pub extern "C" fn status_is_success(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, ((200..=299).contains(&args[0])) as SpectraHostValue)
}

pub extern "C" fn status(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(code) = u16::try_from(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if Status::new(code).is_err() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    write_result(ctx, args[0])
}

pub extern "C" fn header_name_is_valid(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let valid = read_spectra_string(args[0])
        .map(|name| is_valid_header_name(&name))
        .unwrap_or(false);
    write_result(ctx, valid as SpectraHostValue)
}

pub extern "C" fn header_value_is_valid(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let valid = read_spectra_string(args[0])
        .map(|value| is_valid_header_value(&value))
        .unwrap_or(false);
    write_result(ctx, valid as SpectraHostValue)
}

pub extern "C" fn request_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(method) = Method::from_code(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(request) = Request::new(method, "/") else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    write_result(ctx, store.request_handle(request))
}

pub extern "C" fn request(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(method) = Method::from_code(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(path) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(request) = Request::new(method, path) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    write_result(ctx, store.request_handle(request))
}

pub extern "C" fn request_method(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(request) = store.requests.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, request.method.code())
}

pub extern "C" fn request_path(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(request) = store.requests.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(&request.path))
}

/// Returns the request body as UTF-8 text. API payloads are represented as
/// bytes internally, but the stable language surface currently targets JSON
/// and other UTF-8 application payloads; invalid sequences are replaced with
/// U+FFFD rather than exposing an unsafe partial string.
pub extern "C" fn request_body(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(request) = store.requests.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let body = String::from_utf8_lossy(&request.body);
    write_result(ctx, alloc_spectra_string(&body))
}

pub extern "C" fn request_header(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(name) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(request) = store.requests.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        alloc_spectra_string(request.header(&name).unwrap_or("")),
    )
}

pub extern "C" fn request_with_header(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(name), Some(value)) = (read_spectra_string(args[1]), read_spectra_string(args[2]))
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let request = {
        let store = store().lock().unwrap_or_else(|e| e.into_inner());
        let Some(request) = store.requests.get(&args[0]).cloned() else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        request
    };
    let Ok(request) = request.with_header(name, value) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    write_result(ctx, store.request_handle(request))
}

pub extern "C" fn request_with_body(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(body) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let request = {
        let store = store().lock().unwrap_or_else(|e| e.into_inner());
        let Some(request) = store.requests.get(&args[0]).cloned() else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        request.with_body(body.into_bytes())
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    write_result(ctx, store.request_handle(request))
}

pub extern "C" fn request_cookie(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(name) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(request) = store.requests.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        alloc_spectra_string(&request.cookie(&name).unwrap_or_default()),
    )
}

pub extern "C" fn response_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(status) = u16::try_from(args[0])
        .map_err(|_| ())
        .and_then(|code| Status::new(code).map_err(|_| ()))
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    write_result(ctx, store.response_handle(Response::new(status)))
}

pub extern "C" fn response_status(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(response) = store.responses.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, response.status.code() as SpectraHostValue)
}

pub extern "C" fn response_header(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(name) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(response) = store.responses.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        alloc_spectra_string(response.header(&name).unwrap_or("")),
    )
}

pub extern "C" fn response_body_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(response) = store.responses.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, response.body.len() as SpectraHostValue)
}

pub extern "C" fn header(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(name), Some(value)) = (read_spectra_string(args[0]), read_spectra_string(args[1]))
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(header) = Header::new(name, value) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    write_result(ctx, store.header_handle(header))
}

pub extern "C" fn header_name(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(header) = store.headers.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(&header.name))
}

pub extern "C" fn header_value(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(header) = store.headers.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(&header.value))
}

pub extern "C" fn cookie(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(name), Some(value)) = (read_spectra_string(args[0]), read_spectra_string(args[1]))
    else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let cookie = match Cookie::new(name, value) {
        Ok(cookie) => cookie,
        Err(error) => {
            let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
            store.set_cookie_error(CookieError::InvalidAttribute(error.to_string()));
            return HOST_STATUS_INVALID_ARGUMENT;
        }
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    write_result(ctx, store.cookie_handle(cookie))
}

pub extern "C" fn cookie_with_options(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 8) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(name), Some(value), Some(path), Some(domain)) = (
        read_spectra_string(args[0]),
        read_spectra_string(args[1]),
        read_spectra_string(args[2]),
        read_spectra_string(args[3]),
    ) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(same_site) = CookieSameSite::from_code(args[7]) else {
        let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
        store.set_cookie_error(CookieError::InvalidAttribute(
            "invalid SameSite code".to_string(),
        ));
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if args[4] < -1 {
        let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
        store.set_cookie_error(CookieError::InvalidAttribute(
            "cookie max-age must be -1, zero, or positive".to_string(),
        ));
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let max_age = (args[4] >= 0).then_some(args[4]);
    let result = Cookie::with_options(
        name,
        value,
        (!path.is_empty()).then_some(path),
        (!domain.is_empty()).then_some(domain),
        max_age,
        args[5] != 0,
        args[6] != 0,
        same_site,
    );
    let cookie = match result {
        Ok(cookie) => cookie,
        Err(error) => {
            let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
            store.set_cookie_error(CookieError::InvalidAttribute(error.to_string()));
            return HOST_STATUS_INVALID_ARGUMENT;
        }
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    write_result(ctx, store.cookie_handle(cookie))
}

pub extern "C" fn cookie_name(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(cookie) = store.cookies.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(&cookie.name))
}

pub extern "C" fn cookie_value(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(cookie) = store.cookies.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(&cookie.value))
}

pub extern "C" fn cookie_path(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(cookie) = store.cookies.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        alloc_spectra_string(cookie.path.as_deref().unwrap_or("")),
    )
}

pub extern "C" fn cookie_domain(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(cookie) = store.cookies.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(
        ctx,
        alloc_spectra_string(cookie.domain.as_deref().unwrap_or("")),
    )
}

pub extern "C" fn cookie_max_age(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(cookie) = store.cookies.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, cookie.max_age.unwrap_or(-1))
}

pub extern "C" fn cookie_secure(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(cookie) = store.cookies.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, cookie.secure as SpectraHostValue)
}

pub extern "C" fn cookie_http_only(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(cookie) = store.cookies.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, cookie.http_only as SpectraHostValue)
}

pub extern "C" fn cookie_same_site(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(cookie) = store.cookies.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, cookie.same_site.code())
}

pub extern "C" fn cookie_header(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(cookie) = store.cookies.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, alloc_spectra_string(&cookie.header_value()))
}

pub extern "C" fn response_with_cookie(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (response, cookie) = {
        let store = store().lock().unwrap_or_else(|e| e.into_inner());
        let Some(response) = store.responses.get(&args[0]).cloned() else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(cookie) = store.cookies.get(&args[1]).cloned() else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        (response, cookie)
    };
    let Ok(response) = response.with_header("Set-Cookie", cookie.header_value()) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    write_result(ctx, store.response_handle(response))
}

pub extern "C" fn cookie_sign(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(secret) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let cookie = {
        let store = store().lock().unwrap_or_else(|e| e.into_inner());
        let Some(cookie) = store.cookies.get(&args[0]).cloned() else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        cookie
    };
    let cookie = match cookie.sign(&secret) {
        Ok(cookie) => cookie,
        Err(error) => {
            let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
            store.set_cookie_error(error);
            return HOST_STATUS_INVALID_ARGUMENT;
        }
    };
    let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
    store.last_cookie_error = None;
    write_result(ctx, store.cookie_handle(cookie))
}

pub extern "C" fn cookie_verify(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(secret) = read_spectra_string(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let cookie = {
        let store = store().lock().unwrap_or_else(|e| e.into_inner());
        let Some(cookie) = store.cookies.get(&args[0]).cloned() else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        cookie
    };
    match cookie.verify(&secret) {
        Ok(()) => {
            let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
            store.last_cookie_error = None;
            write_result(ctx, 1)
        }
        Err(error) => {
            let mut store = store().lock().unwrap_or_else(|e| e.into_inner());
            store.set_cookie_error(error);
            write_result(ctx, 0)
        }
    }
}

pub extern "C" fn cookie_is_expired(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = read_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let Some(cookie) = store.cookies.get(&args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_result(ctx, cookie.is_expired() as SpectraHostValue)
}

pub extern "C" fn cookie_error_code(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(_) = read_args(ctx, 0) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    write_result(
        ctx,
        store
            .last_cookie_error
            .as_ref()
            .map(CookieError::code)
            .unwrap_or(0),
    )
}

pub extern "C" fn cookie_error_message(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(_) = read_args(ctx, 0) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let store = store().lock().unwrap_or_else(|e| e.into_inner());
    let message = store
        .last_cookie_error
        .as_ref()
        .map(CookieError::message)
        .unwrap_or_default();
    write_result(ctx, alloc_spectra_string(&message))
}

fn method_label(method: SpectraHostValue) -> &'static str {
    Method::from_code(method)
        .map(Method::as_str)
        .unwrap_or("UNKNOWN")
}

fn status_reason_phrase(status: SpectraHostValue) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown Status",
    }
}

fn is_valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn is_valid_header_value(value: &str) -> bool {
    value
        .bytes()
        .all(|b| b == b'\t' || b == b' ' || (0x21..=0x7e).contains(&b) || b >= 0x80)
}

fn is_valid_cookie_name(name: &str) -> bool {
    is_valid_header_name(name)
}

fn is_valid_cookie_value(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b != b';' && b != b',' && b != b'\\' && b != b'"' && b > 0x20 && b < 0x7f)
}

fn is_valid_request_path(path: &str) -> bool {
    let is_origin_form = path.starts_with('/');
    let is_absolute_http_url = path.starts_with("http://") || path.starts_with("https://");
    !path.is_empty()
        && (is_origin_form || is_absolute_http_url)
        && !path.bytes().any(|b| b <= b' ' || b == 0x7f)
}

fn cookie_value_from_header(header: &str, name: &str) -> Option<String> {
    for pair in header.split(';') {
        let pair = pair.trim();
        let Some((candidate, value)) = pair.split_once('=') else {
            continue;
        };
        if candidate.trim().eq_ignore_ascii_case(name) {
            return Some(value.trim().to_string());
        }
    }
    None
}
