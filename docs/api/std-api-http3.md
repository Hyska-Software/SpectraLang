# HTTP/3 server and client transport

R-2406 records the original scope decision in
[ADR 0014](../adr/0014-http3-quic-decision.md) (see the Supersession section
there for the current criterion status). The localhost-validated surface below
is implemented in `packages/spectra-api/src/http3.rs` with the Spectra host
adapter in `packages/spectra-api/src/http3_host.rs`, behind the `http3` Cargo
feature (the default feature in `packages/spectra-api/Cargo.toml`).

It uses `quinn 0.11.11` for QUIC with `h3 0.0.8` / `h3-quinn 0.0.10` for the
HTTP/3 framing. `ALPN_HTTP3` (`http3.rs`) is `b"h3"`; the server config builder
overwrites any adapter-supplied ALPN list so HTTP/3 is never negotiated as
another protocol.

`Http3Server::start` accepts an `Http3ServerConfig` and an `Http3Handler`
(`Arc<dyn Fn(Http3Request) -> ...>`). The configuration bounds concurrent
bidirectional streams (default 256), decoded header sections (default 64 KiB),
request/response bodies (default 16 MiB), and graceful shutdown (default 5 s).
A request exposes the method, target, headers, fully bounded body, and
trailers; a response exposes the status code, headers, body, and trailers.
The h3 connection driver runs separately from per-request tasks, so concurrent
QUIC request streams make progress independently. Bodies and header fields are
copied into bounded owned values; no raw protocol pointers cross the API.

TLS is required for the server: `server_quinn_config` returns an error when no
`tls_config` is present. Key material stays in Rust-owned
`rustls::ServerConfig` / `RootCertStore` handles
(`register_http3_server_config`, `register_http3_client_config`,
`register_http3_handler`); the Spectra string ABI is never used for
certificates or keys. The client defaults to an empty root store with a 10 s
connect timeout and 30 s request timeout.

The native tests prove the release surface on loopback only:

- `localhost_round_trip_with_split_data_and_trailers`: server bound to
  `127.0.0.1:0`, `rcgen` self-signed `localhost` certificate, client connects
  to `https://localhost:<port>` and round-trips a split DATA body plus
  request/response trailers;
- `concurrent_streams_and_oversized_body_are_isolated`: two concurrent streams
  on one connection stay isolated and the configured body limit surfaces as a
  413-style rejection;
- `timeout_resets_stream_and_server_drains_on_shutdown`: a hanging handler hits
  the client request timeout (`Http3Error::Timeout`) and the server drains
  within its shutdown grace period.

hosts): `spectra.api.http3.server_config_new`,
`spectra.api.http3.client_config_new`, `spectra.api.http3.handler_text`,
`spectra.api.http3.server_start`, `spectra.api.http3.server_local_port`,
`spectra.api.http3.server_shutdown`, `spectra.api.http3.client_connect`,
`spectra.api.http3.client_shutdown`, `spectra.api.http3.client_request_new`,
`spectra.api.http3.client_request_header`, `spectra.api.http3.client_request_open`,
`spectra.api.http3.client_request_send_body`,
`spectra.api.http3.client_request_send_trailers`,
`spectra.api.http3.client_request_finish`,
`spectra.api.http3.client_request_receive_response`,
`spectra.api.http3.client_request_cancel`, `spectra.api.http3.response_status`,
`spectra.api.http3.response_header`, `spectra.api.http3.response_trailer`,
`spectra.api.http3.response_body_base64`, `spectra.api.http3.response_body_len`,
`spectra.api.http3.task_result`, `spectra.api.http3.task_cancel`,
`spectra.api.http3.result_ok`, `spectra.api.http3.result_value`,
`spectra.api.http3.result_error_code`,
`spectra.api.http3.result_error_message`, `spectra.api.http3.handle_drop`.

## Limits

- Validation is loopback-only (`127.0.0.1:0` / `localhost` with `rcgen`
  self-signed certificates). There is no interoperability fixture against
  independent (non-`h3`-crate) peers.
- There is no four-platform matrix evidence (Linux / Windows / macOS /
  BSD-kqueue), and no connection-migration coverage.
- There is no reviewed mapping to the Phase 21 `Task<T>` / `Stream<T>` model;
  the host surface uses its own task/result/outcome handle tables.
- There is no documented resource or performance budget versus the HTTP/2
  transport.
- Productionization of the above gaps is tracked by R-2422; the original
  scope decision remains R-2406 / ADR-0014.
