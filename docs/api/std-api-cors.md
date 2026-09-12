# std.api.cors

`std.api.cors` defines the Phase 23 CORS middleware surface for `spectra.api`.
It builds on the deterministic middleware chain from `std.api.middleware` and
the typed HTTP `Request` / `Response` handles from `std.api.http`.

## Public Types

- `CorsPolicy`: immutable policy handle. Builder functions return a new policy
  handle so restrictive, permissive, and credentialed variants can coexist.

## Policy Builders

- `policy() -> CorsPolicy`: starts with a deny-by-default policy.
- `permissive() -> CorsPolicy`: allows any origin with all standard methods and
  a default max-age. It does not enable credentials.
- `allow_origin(policy, origin) -> CorsPolicy`: allows an exact origin, or `*`
  for wildcard behavior.
- `allow_method(policy, method) -> CorsPolicy`: allows a method code from
  `std.api.http`.
- `allow_header(policy, header) -> CorsPolicy`: allows a requested preflight
  header name.
- `expose_header(policy, header) -> CorsPolicy`: adds a response header to
  `Access-Control-Expose-Headers`.
- `allow_credentials(policy, true) -> CorsPolicy`: emits
  `Access-Control-Allow-Credentials: true` and echoes the concrete origin.
- `max_age(policy, seconds) -> CorsPolicy`: sets `Access-Control-Max-Age`.

## Runtime Functions

- `is_preflight(request) -> bool`: detects `OPTIONS` requests with `Origin` and
  `Access-Control-Request-Method`.
- `preflight(policy, request) -> Response`: evaluates the configured policy and
 returns `204` with `Access-Control-*` headers or `403` for denied origins,
  methods, or headers.
- `apply(policy, request, response) -> Response`: applies actual-response CORS
  headers for non-preflight requests when the request origin is allowed.
- `middleware(policy) -> MiddlewareHandle`: turns the policy into a sync
  middleware. Preflight requests short-circuit the chain; actual requests add
  response headers during the unwind phase.
- `allowed_origin(policy, origin) -> string`: returns the emitted
  `Access-Control-Allow-Origin` value for diagnostics and tests, or `""` when
  denied.

## Behavior

Restrictive policies deny by default. A valid preflight must match the configured
origin, method, and requested headers. Denied preflights return `403` and do not
claim success with partial CORS headers.

Credentialed wildcard policies echo the concrete request origin and add
`Vary: Origin`, because browsers reject `Access-Control-Allow-Origin: *` when
credentials are allowed. Non-credentialed permissive policies emit `*`.
When another middleware already contributed a `Vary` dimension, CORS appends
`Origin` instead of replacing it.

## Validation

- `tests/validation/149_api_cors_middleware.spectra` covers permissive,
  restrictive preflight, credentialed, exposed-header, and middleware-chain
  behavior.
- `scripts/validate_r2302_cors_middleware.py` validates code, host calls,
  compiler builtins, midend lowering, docs, planning state, and executable
  behavior.
- The full `run_tests.ps1` suite runs the R-2302 validator.
