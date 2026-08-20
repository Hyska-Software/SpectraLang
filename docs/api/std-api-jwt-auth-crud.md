# Authenticated REST CRUD with JWT

`examples/api/02_jwt_auth_crud.spectra` is the reference composition for a
small authenticated API. It exercises the complete language-facing path:

1. issue an HS256 access token with issuer, audience, subject, expiry, and
   not-before claims;
2. require a `Bearer` authorization header and verify the signature and
   registered claims before routing;
3. read the real UTF-8 request body through `request_body`, validate JSON with
   `std.api.validation`, and return RFC 7807 validation responses;
4. return unified `std.api.errors` responses for missing or invalid tokens;
5. register the same callback against list, create, read, update, and delete
   routes and exercise each route through the handler and middleware chain.

The route chain installs `exception_middleware`; authentication failures use
`ApiError`, while invalid payloads use `result_response` from the validation
framework so both classes of failure keep the `application/problem+json`
contract.

The example uses a deterministic clock so its JWT checks remain reproducible.
Production services must load secrets from deployment configuration and use a
clock source appropriate for their trust boundary.

The request construction helpers are:

```spectra
let request = request_with_body(
    request(method_post(), "/users"),
    "{\"name\":\"Ada\",\"age\":37}",
)
let body = request_body(request)
```

Run it with:

```text
spectralang compile examples/api/02_jwt_auth_crud.spectra
spectralang run examples/api/02_jwt_auth_crud.spectra
```
