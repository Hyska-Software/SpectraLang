# HTTP/2 server transport

R-2404 adds the native HTTP/2 server transport in
`packages/spectra-api/src/http2.rs`. It uses the `h2` protocol state machine,
including HPACK header compression, independent stream lifetimes, and HTTP/2
flow-control windows.

`Http2Server::start` accepts an `Http2Config` and a synchronous Rust
`Http2Handler`. The configuration bounds concurrent streams, decoded header
lists, request bodies, and graceful shutdown. A request exposes the method,
path/query target, headers, and fully bounded body; a response exposes the
status, headers, and bytes body. HTTP/2-only connection headers are filtered
before encoding the response.

## TLS and ALPN

`TlsServerConfig` and `TlsClientConfig` advertise `http/1.1` and `h2` by
default. The HTTP/2 listener accepts a TLS connection only when the negotiated
ALPN protocol is `h2`; a client that selects another protocol is closed before
the HTTP/2 handshake. Plain HTTP/2 remains available for local protocol tests.

The native tests prove all of the release criteria locally:

- two concurrent requests share one connection and round-trip HPACK-compressed
  request/response headers;
- the default TLS configuration contains both ALPN protocols;
- a rustls client negotiates `h2` and completes a request/response exchange;
- body flow-control capacity is released as data is consumed and configured
  body/header/stream limits are applied by the `h2` state machine.

This item is intentionally transport-level. The existing `std.api.server`
host surface remains the HTTP/1.1 routed server; exposing an HTTP/2-aware
Spectra callback/router adapter requires a separate contract because the h2
connection owns multiple concurrent streams and callback scheduling.

Evidence is reproducible with
`python scripts/validate_r2404_http2.py`.
