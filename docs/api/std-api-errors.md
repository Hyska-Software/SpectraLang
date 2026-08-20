# `std.api.errors`

`std.api.errors` is the unified API error surface. `ApiError` is an opaque
handle containing an HTTP status, a stable public code, and a public message.

## Deterministic public responses

Create a public error with `new(status, code, message)`. Statuses outside the
client/server error range (`400..=599`) normalize to `500`; empty codes and
messages receive deterministic fallbacks. `status`, `code`, and `message`
inspect the typed value.

`response(error)` maps the value to an RFC 7807 Problem Details response with
`application/problem+json`. The body includes `type`, `title`, `status`,
`detail`, and `code`; the response never depends on Rust debug formatting or
an incidental handler implementation.

## Internal errors and exception middleware

`internal_error(code, detail)` creates a `500` error with the public code
`internal_error` and message `internal server error`. The complete internal
detail is written to the API error log, while the response contains only the
sanitized public fields.

`exception_middleware(status, code, message)` registers a synchronous exception
middleware. Add it to the `MiddlewareChain` used by a route. If a downstream
middleware raises an internal `HandlerError`, the chain records the full
detail and returns the configured public Problem Details response. Different
route chains can register different status/code/message mappings.

The legacy `last_code()` and `last_message()` helpers remain neutral (`0` and
`""`) for compatibility; inspect an `ApiError` handle with `status`, `code`,
and `message` instead. Internal details are never exposed through that legacy
surface.

The language regression is `tests/validation/340_api_errors.spectra`.
Native tests in `packages/spectra-api/src/errors.rs` cover deterministic
mapping, full-detail logging with sanitization, and per-chain custom recovery.
