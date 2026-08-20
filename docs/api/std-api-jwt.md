# std.api.jwt

`std.api.jwt` provides JWT signing and verification for `HS256`, `RS256`, and
`ES256`.

```spectra
let key = "a secret with at least 32 bytes"
let token = sign(
    "HS256",
    key,
    "{\"iss\":\"issuer\",\"aud\":\"api\",\"sub\":\"user-1\"}",
)
let valid = verify(token, "HS256", key, "issuer", "api", 0)
```

`sign(algorithm, key, claims_json)` accepts an object-valued JSON claims
document and returns a compact JWT. HS256 keys are raw UTF-8 secrets and must
contain at least 32 bytes. RS256 and ES256 keys are base64url-encoded DER
documents (PKCS#8 private keys for signing; RSA public-key DER or an
uncompressed P-256 public key for verification). PEM values are also accepted.

`verify(token, algorithm, key, issuer, audience, now_ms)` checks the protected
algorithm, the cryptographic signature, and the registered claims. `exp` and
`nbf` are NumericDate values in seconds; `iss` and `aud` are checked when an
expected value is supplied. A zero `now_ms` uses the current Unix clock, while
a non-zero value makes tests and replay analysis deterministic. Invalid tokens,
keys, signatures, or claims return `false`.

The implementation uses `ring` for HMAC, RSA PKCS#1 v1.5, and P-256 ECDSA;
signature verification is delegated to constant-time cryptographic primitives.

Validation is covered by `tests/validation/335_api_jwt.spectra` and the native
`spectra-api` JWT tests.
