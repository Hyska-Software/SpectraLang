# std.api.oauth

`std.api.oauth` provides an OAuth 2.0 authorization-code client with PKCE
S256, refresh-token exchange, and RFC 7009-style revocation.

## Client and authorization

Create a client with the registered client credentials, authorization endpoint,
token endpoint, redirect URI, and requested scope:

```spectra
let client = client_new(
    "web-client",
    "client-secret",
    "https://identity.example/authorize",
    "https://identity.example/token",
    "https://app.example/callback",
    "openid profile",
)
let login = authorization_url(client, "request-state")
```

`authorization_url` generates a fresh RFC 7636 verifier when the client is
created and sends only its SHA-256 base64url challenge in the URL. The URL
contains `response_type=code`, `client_id`, `redirect_uri`, `state`,
`code_challenge`, and `code_challenge_method=S256`. The supplied state is
stored and must be returned unchanged by the callback.

`exchange_code(client, code, state)` compares the callback state before making
the token request and sends the original PKCE verifier to the token endpoint.
It uses an `application/x-www-form-urlencoded` POST with `client_id`,
optional `client_secret`, `grant_type=authorization_code`, `code`,
`code_verifier`, and `redirect_uri`.

## Refresh and revocation

`refresh(client, token)` exchanges the stored refresh token and retains the
previous refresh token when the provider omits a replacement. Token responses
must contain non-empty `access_token` and `token_type`; `expires_in` is exposed
as an absolute Unix epoch millisecond value.

Configure revocation before calling `revoke`:

```spectra
if client_set_revocation_url(client, "https://identity.example/revoke") {
    revoke(client, token)
}
```

Revocation sends the access token, `token_type_hint`, client ID, and optional
client secret. A successful 2xx response marks the local token handle revoked,
so subsequent refresh attempts fail without sending the credential again.

The native regression uses a local HTTP authorization server to verify the
PKCE challenge against the submitted verifier, then exercises authorization
code exchange, refresh rotation, and revocation. The language fixture
`tests/validation/337_api_oauth.spectra` validates the public URL-generation
surface without contacting an external identity provider.
