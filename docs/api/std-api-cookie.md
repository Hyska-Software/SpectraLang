# `std.api.http` cookies

The HTTP cookie API keeps the existing `cookie(name, value)` constructor and
adds typed attributes, `Set-Cookie` serialization, and HMAC signing.

## Attributes

`cookie_with_options(name, value, path, domain, max_age, secure, http_only,
same_site)` creates a cookie. Pass an empty string for an omitted path or
domain and `-1` for an omitted `Max-Age`.

`same_site` uses the stable integer codes `0` (unspecified), `1` (Lax), `2`
(Strict), and `3` (None). `SameSite=None` is accepted only with `secure=true`.
The attribute getters are `cookie_path`, `cookie_domain`, `cookie_max_age`,
`cookie_secure`, `cookie_http_only`, and `cookie_same_site`.

## Responses and signing

`response_with_cookie(response, cookie)` appends one `Set-Cookie` field. The
header is available through `cookie_header(cookie)`, and responses can carry
multiple `Set-Cookie` fields without collapsing them into a comma-separated
value.

`cookie_sign(cookie, secret)` returns a copy carrying an HMAC-SHA256 signature.
`cookie_verify(cookie, secret)` performs constant-time verification and returns
`false` for a missing/invalid signature or an expired `Max-Age`. The typed
failure is exposed through `cookie_error_code()` and
`cookie_error_message()`; codes are `1` invalid secret, `2` invalid signature,
`3` expired, and `4` invalid attribute. `cookie_is_expired` checks the local
expiration state without attempting verification.

The language regression is
`tests/validation/338_api_cookie.spectra`; native attribute, serialization,
signature, tamper, expiration, and SameSite tests live in
`packages/spectra-api/src/http_tests.rs`.
