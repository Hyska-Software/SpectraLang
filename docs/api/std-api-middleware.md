# std.api.middleware

`std.api.middleware` defines the Phase 23 middleware chain used by the API
platform before feature-specific middleware such as CORS, rate limiting,
authentication, logging, compression, and security headers.

## Ordering Contract

Middleware is appended to a `MiddlewareChain` in registration order.

Request hooks run in append order:

```text
first.on_request -> second.on_request -> terminal handler
```

Response hooks run in reverse order:

```text
terminal response -> second.on_response -> first.on_response
```

If a middleware short-circuits the request, later request hooks are not called.
The response still unwinds through response hooks for middleware that already
ran. For example, if `second` short-circuits, the trace is:

```text
first:request
second:request
second:response
first:response
```

This deterministic contract is the dependency for Phase 23 middleware items
`R-2302` through `R-2316`.

## Production composition order

The reference composition in `examples/api/03_middleware_composition.spectra`
uses this order:

```text
structured logging -> security headers -> CORS -> compression -> rate limit
```

Logging is outermost so it records successful, rejected, and preflight
responses. Security headers run before short-circuit middleware so rejected
responses retain the baseline browser policy. CORS runs before compression so
preflight can terminate without a content encoding. Compression runs before
rate limiting so a `429` response still receives the negotiated encoding and
`Vary: Accept-Encoding`. CORS and compression merge their cache dimensions;
an actual response that passes through both retains `Vary: Accept-Encoding,
Origin` (order is not semantically significant).

The example also configures longest-prefix security policies for `/api` and
`/api/admin`, and uses a route-scoped sliding-window limiter so sibling routes
have independent budgets.

## Traits

Synchronous middleware implements:

```spectra
public trait Middleware {
    func on_request(&self, request: Request) returns Request
    func on_response(&self, response: Response) returns Response
}
```

Async middleware implements:

```spectra
public trait AsyncMiddleware {
    async func on_request(&self, request: Request) returns Request
    async func on_response(&self, response: Response) returns Response
}
```

The native runtime stores middleware as chain entries and supports both sync
and async execution. `execute_sync` rejects async middleware; `execute_async`
can run sync and async middleware in the same chain.

## Public Functions

- `chain()` / `chain_new()` create an empty `MiddlewareChain`.
- `chain_len(chain)` returns the number of entries.
- `register_sync(before, after)` registers testable sync middleware hooks.
- `register_sync_short_circuit(before, after, response)` registers sync
  middleware that returns `response` instead of calling later middleware.
- `register_async(before, after)` registers testable async middleware hooks.
- `register_async_short_circuit(before, after, response)` registers async
  middleware that short-circuits.
- `use_sync(chain, middleware)` appends a sync middleware and returns the new
  chain handle.
- `use_async(chain, middleware)` appends an async middleware and returns the new
  chain handle.
- `execute_sync(chain, request, terminal_response)` runs a sync-only chain.
- `execute_async(chain, request, terminal_response)` runs a mixed sync/async
  chain.
- `register_logging(format)` registers a synchronous request logger. `format`
  must be `"json"` or `"text"`; JSON is intended for production ingestion and
  text is intended for local development.
- `logging_len(logger)` returns the number of completed requests recorded by
  the logger.
- `logging_line(logger, index)` returns the rendered line emitted for a
  completed request.
- `logging_request_id(logger, index)` returns the request ID embedded in that
  line. IDs are generated once per chain execution and are visible to every
  middleware through `MiddlewareContext`.
- `register_rate_limit(algorithm, limit, window_ms, scope, dev_mode)` registers
  a rate limiter. `algorithm` is `"token_bucket"` or `"sliding_window"`;
  `scope` can be `global`, `route`, `tenant`, `user`, `api_key`,
  `route_tenant`, or `route_user`. Tenant and user scopes read
  `X-Spectra-Tenant` and `X-Spectra-User` respectively; `api_key` consumes the
  internal identity emitted by API-key middleware.
- `rate_limit_update(limiter, limit, window_ms)` replaces the limit and window
  and clears counters only for a limiter created with `dev_mode = true`.
- `register_api_key(source)` registers API-key authentication. `source` is
  `header` (default `X-API-Key`), `query` (default `api_key`), or an explicit
  `header:<name>` / `query:<name>` source.
- `api_key_add(auth, key, expires_at_ms)` adds or replaces a key. `0` means no
  expiry; non-zero values are Unix epoch milliseconds.
- `api_key_revoke(auth, key)` adds the key to the revocation set. Rejected
  requests receive a structured `401 application/problem+json` response and
  never echo the secret or the supplied key.
- `register_compression(threshold)` registers a response compression
  middleware. It negotiates `br`, `gzip`, and `deflate` from
  `Accept-Encoding`, prefers the highest q-value (with deterministic
  `br > gzip > deflate` tie-breaking), and leaves bodies below `threshold`
  bytes unchanged.
- `register_security_headers()` registers the default security policy:
  `default-src 'self'`, a restrictive `Permissions-Policy`, `DENY`, `nosniff`,
  and `strict-origin-when-cross-origin`.
- `security_headers_configure(headers, csp, permissions, hsts,
  include_subdomains, preload)` replaces the global CSP and
  `Permissions-Policy`; HSTS is emitted only when enabled, and the two HSTS
  flags are rejected unless HSTS is enabled.
- `security_headers_route(headers, prefix, csp, permissions)` adds or replaces
  a route-prefix override. The longest matching prefix wins and query strings
  are ignored for matching.
- `last_trace()` returns the trace from the most recent chain execution.
- `trace_len(trace)`, `trace_event(trace, index)`, and
`trace_short_circuited(trace)` expose deterministic validation data.

## Structured request logging

The logging middleware must normally be the first entry in a chain so that it
also observes responses produced by short-circuit middleware. Request hooks run
with a generated ID of the form `req-<16 hexadecimal digits>`. The response
hook emits exactly one record containing:

```text
request_id, method, path, status, latency_us
```

JSON output contains those names as JSON fields. Text output uses the stable
`key=value` order `request_id`, `method`, `path`, `status`, `latency_us` and
escapes control characters. The native sink retains rendered lines until the
caller consumes them through the `logging_*` functions; this makes the same
contract testable in Spectra and embeddable without scraping process stderr.

## Rate limiting

The rate limiter is a short-circuit middleware. An accepted request continues
through the chain; an exhausted key receives `429 Too Many Requests` with
`Retry-After`, `X-RateLimit-Limit`, and `X-RateLimit-Remaining` headers. Token
bucket refill is continuous over the configured window. Sliding-window mode
keeps exact request timestamps and evicts them at the window boundary. The
scope is part of the key, so one tenant or user cannot consume another key's
budget. Runtime updates are intentionally rejected for production limiters;
they are a development-only operation and reset the counters atomically.

## API-key authentication

API-key middleware validates the configured source before downstream hooks run.
Missing, unknown, revoked, and expired keys are reported with the stable
`invalid_api_key` error and a reason of `missing`, `unknown`, `revoked`, or
`expired`. A valid key is copied into an internal `x-spectra-api-key` request
context value; this lets a following rate limiter use the `api_key` scope
without returning the credential to the application response. The credential
registry is protected by a mutex and is shared by cloned middleware entries.

## Security headers

Security headers should be registered before authentication, rate limiting, or
other short-circuit middleware so their response hook also runs for rejected
responses. Every response that reaches the middleware receives:

```text
Content-Security-Policy: default-src 'self'
Permissions-Policy: geolocation=(), microphone=(), camera=()
X-Frame-Options: DENY
X-Content-Type-Options: nosniff
Referrer-Policy: strict-origin-when-cross-origin
```

HSTS is opt-in and uses `max-age=31536000`; `includeSubDomains` and `preload`
are appended only when explicitly enabled. Route overrides configure CSP and
Permissions-Policy independently while retaining the global HSTS and fixed
header policy.

## Validation

The executable regression is `tests/validation/148_api_middleware_chain.spectra`:

```powershell
.\target\debug\spectralang.exe run tests\validation\148_api_middleware_chain.spectra
```

The roadmap validator is:

```powershell
python scripts\validate_r2301_middleware_chain.py
```

Structured logging is covered by
`tests/validation/331_api_structured_logging.spectra` and
`scripts/validate_r2303_structured_logging.py`.

Rate limiting is covered by `tests/validation/332_api_rate_limiting.spectra`.

API-key authentication is covered by
`tests/validation/333_api_key_auth.spectra`.

## Response compression

Compression is explicit middleware and should normally be registered near the
outer edge of a chain so it can encode terminal and short-circuit responses.
`register_compression(threshold)` accepts a byte threshold; a response is
compressed only when its body has at least that many bytes and the request
advertises a supported coding:

```text
br;q=1, gzip;q=0.8, deflate;q=0.5
```

The middleware supports Brotli (`br`), gzip (`gzip`), and zlib-wrapped deflate
(`deflate`). Explicit `q=0` values override `*`; malformed or out-of-range
quality values are treated as unavailable. Ties are deterministic:
`br`, then `gzip`, then `deflate`. Every response passing through the
middleware receives `Vary: Accept-Encoding` (merged with an existing `Vary`
value), while `Content-Encoding` is added only after a body is encoded.

Bodies for `HEAD`, informational responses, `204`, `205`, `304`, already
encoded responses, empty responses, and responses with `Cache-Control:
no-transform` are never compressed. An existing `Content-Length` is updated
to the encoded size; the HTTP server also derives the wire length from the
final body.

The executable regression is
`tests/validation/336_api_compression.spectra`, and native codec round-trips
are covered by the `spectra-api` middleware tests.

Security headers are covered by
`tests/validation/334_api_security_headers.spectra` and
`scripts/validate_r2306_security_headers.py`.

The complete composition example is
`examples/api/03_middleware_composition.spectra`; its dedicated gate is
`scripts/validate_r2318_middleware_composition_example.py`.
