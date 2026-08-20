# `std.api.security`

`std.api.security` exposes the default API threat mitigations. The policies are
immutable handles: each builder returns a new policy, so a route can retain a
different security configuration without mutating another route.

## CSRF origin validation

`csrf_policy()` creates an empty exact-origin allowlist. Add origins with
`csrf_allow_origin(policy, origin)` and install the resulting middleware with
`csrf_middleware(policy)`.

For `POST`, `PUT`, `PATCH`, and `DELETE`, an `Origin` header that is not in the
allowlist is rejected with `403` and a Problem Details response. Safe methods
are not blocked. Requests without `Origin` are accepted for non-browser and
server-to-server clients; browser requests should therefore send the standard
header. Use `csrf_origin_count` to inspect the configured allowlist.

## SSRF protection

`ssrf_policy()` denies loopback, RFC1918 private, link-local, unspecified,
multicast, benchmarking, and IPv6 unique-local addresses by default. The
HTTP client validates every resolved address before opening a socket, including
redirect targets and the asynchronous HTTPS path. `ssrf_allows(policy, host)`
can validate an IP literal, while
`ssrf_allow_private_networks(policy, true)` is an explicit opt-in for trusted
internal services. Apply that policy to a client with
`set_ssrf_policy(client, policy)`.

DNS results are validated before connection and the validated address list is
used for the connection attempt, preventing a second unvalidated hostname
resolution between policy checking and socket creation.

## Request limits and timeouts

Servers default to a 16 MiB request body limit, a 5-second read timeout, and a
30-second idle timeout. The HTTP parser rejects an oversized
`Content-Length` before reading the body and tracks chunked bodies incrementally
so the limit is enforced before the complete body is buffered.

Before `serve`, configure a server with:

- `set_max_body_bytes(server, bytes)`;
- `set_read_timeout(server, milliseconds)`;
- `set_idle_timeout(server, milliseconds)`.

All values must be greater than zero. A slow client receives a `408` response
and is disconnected independently of other connections; a stalled asynchronous
handler is cancelled at the read-timeout deadline and returns `504`.

The language regression is `tests/validation/341_api_security.spectra`.
