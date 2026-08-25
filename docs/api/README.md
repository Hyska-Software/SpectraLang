# spectra.api Reference

This directory is the reference entry point for the Phase 22 `spectra.api`
surface.

Start with the book chapter:

- [Hello HTTP](../book/09-hello-http.md)
- [Middleware Chain](../book/10-middleware-chain.md)

Runnable examples:

- `examples/api/00_hello_http.spectra`
- `examples/api/01_rest_crud.spectra`
- `examples/api/02_jwt_auth_crud.spectra`
- `examples/api/03_middleware_composition.spectra`

Reference pages:

- [API conformance v0](api-conformance-v0.md)
- [HTTP core types](std-api-http-types.md)
- [Cookie API](std-api-cookie.md)
- [JSON codec](std-api-json.md)
- [JSON derive](std-api-json-derive.md)
- [JWT signing and verification](std-api-jwt.md)
- [Authenticated JWT CRUD example](std-api-jwt-auth-crud.md)
- [OAuth 2.0](std-api-oauth.md)
- [Routing](std-api-routing.md)
- [Query binding](std-api-query.md)
- [URL-encoded forms](std-api-form.md)
- [Multipart uploads](std-api-multipart.md)
- [Handlers](std-api-handler.md)
- [CORS](std-api-cors.md)
- [Middleware](std-api-middleware.md)
- [Request validation and RFC 7807](std-api-validation.md)
- [Unified errors and exception middleware](std-api-errors.md)
- [Threat mitigations and security policies](std-api-security.md)
- [Server-side sessions](std-api-session.md)
- [HTTPS hardening](std-api-https-hardening.md)
- [WebSocket server](std-api-websocket.md)
- [Server-Sent Events](std-api-sse.md)
- [HTTP/2 server transport](std-api-http2.md)
- [HTTP/2 client transport](std-api-http2-client.md)
- [HTTP/3 and QUIC decision](std-api-http3.md)
- [Server lifecycle](std-api-server-lifecycle.md)
- `std.api.client.request(Client, Request)` returns a `Task<Response>` and
  accepts an absolute `http://` or `https://` URL in the request path for
  outbound calls. Both schemes use platform readiness and task cancellation;
  HTTPS uses `rustls`, SNI, and the default WebPKI roots. Native callers can
  provide explicit roots through `ClientConfig::with_tls_config`.
- [REST + SQLite CRUD](std-api-sqlite-crud.md)
- [SQLite migrations](std-api-migrations.md)
- [SQLite connection pool and language-level migrations](std-api-db-pool.md)
