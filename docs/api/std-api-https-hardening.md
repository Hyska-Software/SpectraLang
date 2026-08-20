# HTTPS hardening

The HTTPS server combines the `std.api.middleware` security headers policy with
the native rustls configuration in `packages/spectra-api`.

## HSTS preload

Configure security headers through `std.api.middleware`. When HSTS is enabled,
the middleware emits `Strict-Transport-Security` with `max-age`; setting both
`include_subdomains` and `preload` adds the corresponding tokens. The
configuration is opt-in and rejects incompatible values. The executable
coverage is `tests/validation/334_api_security_headers.spectra`, owned by
R-2306.

## OCSP stapling

Native integrations can attach a DER-encoded OCSP response while creating a
`TlsServerConfig` with `with_ocsp_response`. The implementation delegates to
rustls `with_single_cert_with_ocsp`, so the response is carried in the
certificate status data of subsequent TLS handshakes. Empty responses are
rejected instead of silently enabling a false stapling configuration.

## Certificate rotation

Long-lived servers can put the initial configuration in a
`TlsCertificateStore`. Calling `rotate` builds and validates the next rustls
configuration before replacing the stored `Arc<ServerConfig>`. A listener that
uses `serve_single_https_request_rotating` reads the current configuration at
accept time: existing connections retain their selected configuration while
future handshakes use the rotated certificate and OCSP response, without
restarting the TCP listener.

The native regression tests cover both the stapled response observed by a TLS
client verifier and two successful HTTPS handshakes across a rotation. The
dedicated gate is `scripts/validate_r2315_https_hardening.py`.
