# gRPC transport over HTTP/2

R-2419 tracks gRPC support (currently `in_progress`). The transport is
implemented in `packages/spectra-api/src/grpc.rs` with the Spectra host
adapter in `packages/spectra-api/src/grpc_host.rs` and host registrations in
`host_calls.rs` (`spectra.api.grpc.*`).

## Opaque-bytes model

The transport carries opaque protobuf bytes and never interprets them. There
is no `.proto` compilation step and no generated message code: applications
provide a `GrpcService` (`Fn(GrpcRequest) -> ...`) and encode/decode their own
messages. `GrpcMessage` is an owned byte vector deliberately not interpreted
as UTF-8. (`prost 0.13` is listed in `packages/spectra-api/Cargo.toml` but
nothing in `src/` uses it for the codec; message framing is hand-rolled
5-byte envelopes in `grpc.rs`.) `.proto` codegen remains the outstanding
R-2419 acceptance item; the opaque-bytes transport is the done part.

Messages, metadata, statuses, and responses use integer handles
(`message_*`, `metadata_*`, `status_*`, `response_*`, `error_*` families).
Bodies cross the host boundary as base64 strings (`message_from_base64`,
`message_to_base64`, `status_set_details_base64`,
`status_details_base64`, `error_details_base64`).

## Cardinalities

All four cardinalities run over the `h2 0.4` state machine on TCP loopback or
any reachable `SocketAddr`:

- unary: `spectra.api.grpc.client_unary` (single request message, single
  response message plus trailers);
- client-streaming: `spectra.api.grpc.client_client_streaming` (send all
  request messages, then `stream_finish`, then receive);
- server-streaming: `spectra.api.grpc.client_server_streaming` (single request
  message, bounded `GrpcReceiver` of response messages);
- bidirectional: `spectra.api.grpc.client_bidi_streaming` (sends and receives
  may overlap).

Open streams share `spectra.api.grpc.stream_send`, `stream_recv`,
`stream_finish`, `stream_cancel`, and `stream_free`. The server side is
`server_bind` (cleartext h2c) with `server_local_port`, `server_shutdown`,
and `server_free`. Request and response streams are bounded
(`stream_capacity`, default 16); a full channel surfaces `CapacityClosed`
instead of blocking forever.

## TLS option

Cleartext is the default contract: `GrpcServer::bind` and `client_connect`
stay cleartext (h2c). TLS is opt-in through new hosts that leave the existing
arity untouched:

- `spectra.api.grpc.server_bind_tls`: DER certificate chain plus PKCS#8
  private key, validated with the shared rustls builder (ALPN `h2`) before any
  socket binds, so bad material fails fast with `INVALID_ARGUMENT`. When the
  identity is present the listener terminates TLS with ALPN `h2` before the h2
  handshake.
- `spectra.api.grpc.client_connect_tls`: DER root certificates (an empty
  string selects no trust anchors, so the handshake honestly fails) plus a
  `server_name` that must parse as a rustls server name. ALPN offers `h2`; a
  server that does not terminate TLS with the same protocol fails the h2
  handshake loudly instead of degrading.

The native tests prove a localhost rustls unary round-trip
(`localhost_tls_unary_round_trip`, `rcgen` self-signed `localhost`
certificate) and an untrusted-root handshake failure
(`tls_untrusted_root_handshake_fails`).

## Compressed-flag rejection

`GrpcMessageParser::push` accepts only flag byte `0` (identity). A flag byte
of `1` (compressed) returns `Err(GrpcFrameError::InvalidCompressedFlag(1))`
instead of passing the bytes through:

- server-side, `serve_stream` maps it to trailers with
  `grpc-status: 12 (UNIMPLEMENTED)` and the message
  `gRPC message compression (grpc-encoding) is not supported`;
- client-side, a flag-1 DATA frame surfaces as
  `GrpcError::Frame(InvalidCompressedFlag(1))`.

There is no `grpc-encoding` negotiation and no gzip support on this path
(`flate2` is used elsewhere in the crate, not for gRPC frames). Flag-0
round-trips keep working; truncated envelopes report `Truncated` and
oversized messages report `MessageTooLarge` (default limit 16 MiB,
configurable per server/client).

## Metadata, timeout, and request rules

- Path: `/package.Service/Method` with exactly two non-empty segments; `?`,
  `#`, `%`, control bytes, and spaces are rejected (`InvalidPath`).
- Content type: `application/grpc` or `application/grpc+<suffix>` (parameters
  after `;` ignored); anything else is `InvalidContentType`. `te` must contain
  `trailers`, otherwise `InvalidTe`. A non-POST method or failed validation
  gets an HTTP `415` without invoking the service.
- Metadata keys are validated; keys starting with `grpc-` are reserved and
  rejected (`ReservedMetadata`). `grpc-message` is percent-encoded on the wire
  and decoded on receipt; `grpc-status-details-bin` is base64-encoded.
- Timeouts use the `grpc-timeout` header (`parse_grpc_timeout` /
  `format_grpc_timeout`, units `H`/`M`/`S`/`m`/`u`/`n`, max 8 digits). An
  unparsable value resets the stream with `PROTOCOL_ERROR`; an expired service
  deadline cancels with `CANCEL` server-side and `DeadlineExceeded`
  client-side.

## Boundary

What this page does not promise: `.proto` file compilation, generated stubs,
JSON transcoding, reflection, health checking, load balancing, retries, or
compression negotiation. Those stay out of scope until R-2419 says otherwise.

## Host surface

All `spectra.api.grpc.*` hosts (`host_calls.rs`, 40 total):
`spectra.api.grpc.message_from_base64`,
`spectra.api.grpc.message_to_base64`, `spectra.api.grpc.message_len`,
`spectra.api.grpc.message_free`, `spectra.api.grpc.metadata_new`,
`spectra.api.grpc.metadata_insert`, `spectra.api.grpc.metadata_append`,
`spectra.api.grpc.metadata_get`, `spectra.api.grpc.metadata_len`,
`spectra.api.grpc.metadata_free`, `spectra.api.grpc.status_new`,
`spectra.api.grpc.status_code`, `spectra.api.grpc.status_message`,
`spectra.api.grpc.status_details_base64`,
`spectra.api.grpc.status_set_details_base64`, `spectra.api.grpc.status_free`,
`spectra.api.grpc.response_message`, `spectra.api.grpc.response_status`,
`spectra.api.grpc.response_metadata`, `spectra.api.grpc.response_free`,
`spectra.api.grpc.error_code`, `spectra.api.grpc.error_message`,
`spectra.api.grpc.error_details_base64`, `spectra.api.grpc.error_free`,
`spectra.api.grpc.client_connect`, `spectra.api.grpc.client_connect_tls`,
`spectra.api.grpc.client_unary`, `spectra.api.grpc.client_client_streaming`,
`spectra.api.grpc.client_server_streaming`,
`spectra.api.grpc.client_bidi_streaming`, `spectra.api.grpc.stream_send`,
`spectra.api.grpc.stream_recv`, `spectra.api.grpc.stream_finish`,
`spectra.api.grpc.stream_cancel`, `spectra.api.grpc.stream_free`,
`spectra.api.grpc.server_bind`, `spectra.api.grpc.server_bind_tls`,
`spectra.api.grpc.server_local_port`, `spectra.api.grpc.server_shutdown`,
`spectra.api.grpc.server_free`.
