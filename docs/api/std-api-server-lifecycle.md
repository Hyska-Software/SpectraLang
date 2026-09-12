# std.api.server Lifecycle

Roadmap item: `R-2216 Server Lifecycle, Listen, Serve, and Graceful Shutdown`

Book walkthrough: [Hello HTTP](../book/09-hello-http.md)

`std.api.server` owns the runtime lifecycle for an HTTP server. The public
surface is intentionally small:

- `new() -> Server` creates a stopped lifecycle handle in state `1`.
- `listen(Server, int) -> bool` configures the local port. Port `0` requests an
  OS-assigned loopback port.
- `serve(Server, Router) -> task<int>` starts the listener, routes requests
  through `std.api.routing`, and dispatches registered `std.api.handler`
  responses and invokes registered sync callbacks
  (`register_sync_callback`) and async callbacks
  (`register_async_callback`). Async callback tasks are polled by the same mio
  event loop and are cancelled on disconnect or drain timeout.
- `local_port(Server) -> int` reports the configured or OS-assigned port.
- `state(Server) -> int` returns `1` created, `2` running, `3` stopped, or `4`
  stopping.
- `shutdown(Server) -> bool` requests graceful shutdown.
- `signal(Server, int) -> bool` applies the same shutdown path for deterministic
  signal handling. Code `2` is SIGINT and code `15` is SIGTERM.
- `stats(Server, int) -> int` returns lifecycle counters for validation and
  diagnostics.
- `set_tls_certificate(Server, string, string) -> bool` attaches a PEM
  certificate chain and PEM private key to a stopped server. Rejected while
  the server is running or stopping.
- `tls_local_port(Server) -> int` reports the OS-assigned port of the TLS
  gateway listener once `serve` is running; it returns `0` before that.

## TLS Topology

The HTTP/1.1 pipeline is a mio state machine, while the HTTP/2 implementation
is tokio-based; a mio event loop cannot wrap TLS streams. When certificates are
attached via `set_tls_certificate`, `serve` therefore runs two listeners:

- The configured loopback port keeps serving **cleartext HTTP/1.1** on the mio
  event loop with full SSE/WebSocket support (unchanged behavior).
- A dedicated tokio listener on an OS-assigned port on the same host acts as
  the **TLS gateway**. It terminates TLS, inspects the negotiated ALPN
  protocol, and fans out: `h2` connections are served by the HTTP/2 pipeline;
  `http/1.1` (and ALPN-less) clients are served by an async HTTP/1.1 loop that
  dispatches through the same router and handlers as the mio pipeline.

Both protocols are reachable on the SAME TLS port. Limitations of the gateway's
HTTP/1.1 leg: Server-Sent Events streams and WebSocket upgrades require the
mio connection surface and answer `501 Not Implemented` there; synchronous and
asynchronous handlers work normally.

## Shutdown Policy

Shutdown stops accepting new connections immediately. Existing connections are
serviced until they complete or until the configured drain timeout expires. When
the timeout expires, unfinished connections are closed and counted as cancelled.

The default drain timeout is five seconds. The current public Spectra surface
uses the default policy; Rust integration tests cover shorter drain windows so
the cancellation path remains deterministic.

## Stats Keys

- `1`: accepted connections
- `2`: completed requests
- `3`: rejected connections
- `4`: body limit violations
- `5`: timeouts
- `6`: parse errors
- `7`: closed connections
- `8`: drained connections
- `9`: cancelled connections
- `10`: shutdown signals
- `11`: peak connections
- `12`: active connections

## Validation

Coverage is split across the public Spectra surface and Rust integration tests:

- `tests/validation/147_api_server_lifecycle.spectra`
- `packages/spectra-api/src/server.rs` R-2216 drain/cancellation tests
- `packages/spectra-api/src/lib.rs` host-call lifecycle integration test
- `packages/spectra-api/src/lib.rs` callback routing integration test
- `tests/validation/330_api_handler_callbacks.spectra`
- `scripts/validate_r2216_server_lifecycle.py`
