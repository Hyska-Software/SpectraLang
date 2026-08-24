# HTTP/3 and QUIC status

HTTP/3 and QUIC are intentionally deferred. The accepted decision is recorded
in [ADR 0014](../adr/0014-http3-quic-decision.md), tracked by R-2406.

The stable API transport baseline is HTTP/1.1 plus the native HTTP/2 server
and client transports. SpectraLang does not expose `std.api.http3`, QUIC host
calls, or an HTTP/3 capability flag while the required dependency, security,
platform, and interoperability gates are absent.

The decision will be re-evaluated on 2026-11-30. Reconsideration requires a
maintained compatible Rust QUIC implementation, independent HTTP/3 peers,
Linux/Windows/macOS/BSD evidence, cancellation and migration coverage, and a
reviewed integration with the existing async API contracts.
