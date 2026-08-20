# std.api.handler

`std.api.handler` defines the Phase 22 handler contract used by API routes to
produce `std.api.http.Response` values.

Book walkthrough: [Hello HTTP](../book/09-hello-http.md)

## Traits

- `IntoResponse`: converts a value into `Response`.
- `Handler`: synchronous handler contract with `call(request) -> Response`.
- `AsyncHandler`: asynchronous handler contract with
  `async call(request) -> Response`.

The native `spectra-api` crate also provides Rust implementations of
`IntoResponse` for `Response`, `String`, `&str`, `Vec<u8>`, `()`,
`HandlerError`, and `Result<T, HandlerError>` when `T: IntoResponse`.

## Types

- `HandlerHandle`: runtime handle for a registered synchronous handler.
- `AsyncHandlerHandle`: runtime handle for a registered async handler.
- `HandlerError`: typed handler failure with HTTP status and message.

## Response Helpers

The module exposes stable helpers that normalize common handler return
values into `Response`:

```spectra
let ok = handler.text("created")
let body = handler.json("{\"ok\":true}")
let empty = handler.status(204)
let with_id = handler.with_header(ok, "X-Request-Id", "abc")
```

`into_text_response` and `into_status_response` are the public Spectra bridge
for custom `IntoResponse` implementations until package-level native extern
declarations and blanket impls are first-class in Spectra source.

## Dispatch

`register_sync(route_id, response)` and `register_async(route_id, response)`
remain the deterministic compatibility form for materialized responses.

For user code, `register_sync_callback(route_id, fn(Request) -> Response)` and
`register_async_callback(route_id, fn(Request) -> Task<Response>)` register
real closure callbacks. `dispatch_sync` and `dispatch_async` invoke those
callbacks with the request handle and return the normalized `Response`.
`std.api.server.serve` uses the same callback bridge: synchronous callbacks run
on request dispatch, while async callbacks remain pending in the mio server
loop until their `Task<Response>` completes. Disconnect, shutdown-drain, and
timeout paths cancel the pending task. The compiler promotes a callback
closure's manual allocation to the base runtime frame before registration, so
the server may safely retain and invoke it after the registering function
returns.

## Errors

`error(status, message)` creates a `HandlerError`. `error_response(error)`
converts it into a response, and `last_error_message()` exposes the latest
handler failure for integration with the unified error middleware workstream.

## Validation

R-2215 is covered by:

- `packages/spectra-api/src/handler.rs` unit tests.
- `tests/validation/139_api_handler_response_return.spectra`.
- `tests/validation/330_api_handler_callbacks.spectra`.
- `scripts/validate_r2215_handler_response.py`.
- the full `run_tests.ps1` suite.
