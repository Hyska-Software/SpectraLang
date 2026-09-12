# std.api.http Core Types

`std.api.http` exposes the Phase 22 typed HTTP foundation used by handlers,
clients, routers, and later middleware phases.

## Types

- `Request`: method, path, headers, and body handle.
- `Response`: status, headers, and body handle.
- `Method`: documented HTTP method value.
- `Status`: validated HTTP status value.
- `Header`: validated HTTP header name/value pair.
- `Headers`: case-insensitive header collection.
- `Cookie`: validated cookie name/value pair.
- `Body`: body handle.

These types are represented by runtime handles in the midend/backend. They can
be used in Spectra function signatures such as:

```spectra
from std.api.http import Request, Response, response, status_ok

func handler(req: Request) returns Response {
    return response(status_ok())
}
```

## Method Values

The documented methods are available as stable functions returning method
codes:

- `method_get()`
- `method_head()`
- `method_post()`
- `method_put()`
- `method_patch()`
- `method_delete()`
- `method_options()`

Use `method_name(method)`, `method_allows_body(method)`, and
`method_is_safe(method)` to inspect a method code.

## Status Values

The documented status helpers return validated status codes:

- `status_continue()`
- `status_switching_protocols()`
- `status_ok()`
- `status_created()`
- `status_accepted()`
- `status_no_content()`
- `status_moved_permanently()`
- `status_found()`
- `status_not_modified()`
- `status_bad_request()`
- `status_unauthorized()`
- `status_forbidden()`
- `status_not_found()`
- `status_method_not_allowed()`
- `status_conflict()`
- `status_unsupported_media_type()`
- `status_unprocessable_content()`
- `status_too_many_requests()`
- `status_internal_server_error()`
- `status_bad_gateway()`
- `status_service_unavailable()`
- `status_gateway_timeout()`

Use `status_reason(status)`, `status_class(status)`, and
`status_is_success(status)` for inspection.

## Request and Response

- `request(method, path)` constructs a request handle with a validated method
  and path.
- `request_new(method)` constructs a compatibility request for `/`.
- `request_method(request)` returns the method code.
- `request_path(request)` returns the request path.
- `request_body(request)` returns the request body decoded as UTF-8 text;
  invalid byte sequences are replaced with U+FFFD.
- `request_header(request, name)` returns the first case-insensitive header
  match or an empty string.
- `request_with_header(request, name, value)` returns a new request handle with
  the header inserted or replaced. It is used by CORS, middleware, and
  validation fixtures that need to model incoming HTTP headers from Spectra
  code.
- `request_with_body(request, body)` returns a new request handle with a
  UTF-8 body. It is the stable constructor used by JSON handlers and outbound
  client requests.
- `request_cookie(request, name)` reads a case-insensitive cookie from the
  `Cookie` header or returns an empty string.
- `response(status)` and `response_new(status)` construct a response handle.
- `response_status(response)` returns the status code.
- `response_header(response, name)` performs case-insensitive header lookup.
- `response_body_len(response)` returns the current body length.

## Header and Cookie Validation

`header(name, value)` rejects invalid HTTP token names and invalid field values.
`header_name_is_valid(name)` and `header_value_is_valid(value)` expose the same
validation for user code.

`cookie(name, value)` validates token-compatible names and cookie-octet values.
Cookie lookup is case-insensitive for the cookie name.

## Validation

R-2210 is validated by:

- `cargo test -p spectra-api --offline`
- `cargo test -p spectra-compiler --offline`
- `cargo test -p spectra-midend --offline`
- `spectralang compile tests/validation/134_http_core_types.spectra`
- `spectralang run tests/validation/134_http_core_types.spectra`
- `scripts/validate_r2210_http_core_types.py`
