# ADR 0014: HTTP/3 and QUIC scope decision

Status: Accepted

Date: 2026-08-20

Roadmap item: R-2406

## Context

SpectraLang now has a validated HTTP/1.1 API path and a native HTTP/2
transport. HTTP/3 is not a header-only extension: it adds QUIC over UDP,
TLS 1.3 handshake integration, connection migration, QPACK, stream
priorities, loss recovery, anti-amplification rules, and different proxy and
operational behavior. It also needs platform-specific integration and a real
interoperability matrix before it can be part of the stable API contract.

The current workspace has no approved QUIC dependency, no checked-in HTTP/3
server/client conformance fixture, and no release evidence for UDP connection
migration or the required Linux, Windows, macOS, and BSD paths. Adding a
partial protocol façade would create a public feature that cannot satisfy the
project's production completion rules.

## Decision

Defer HTTP/3 and QUIC implementation. Keep HTTP/1.1 and HTTP/2 as the stable
API protocol surfaces and do not publish `std.api.http3`, QUIC host calls, or
an HTTP/3 capability flag until the entry criteria below are satisfied.

Re-evaluate this decision on 2026-11-30, or earlier if all of these become
available:

1. a maintained Rust QUIC/HTTP/3 implementation with a compatible license,
   security process, and stable async integration;
2. native server and client request/response coverage plus HTTP/3
   interoperability fixtures against at least two independent peers;
3. CI evidence for Linux, Windows, macOS, and the supported BSD/kqueue path,
   including cancellation, shutdown, flow control, and connection migration;
4. a reviewed mapping to the Phase 21 `Task<T>`/`Stream<T>` model and the
   existing `spectra.api` TLS, SSRF, timeout, and observability contracts; and
5. a documented resource and performance budget compared with the HTTP/2
   implementation.

## Consequences

- R-2406 is complete as a scope decision, not as an HTTP/3 implementation.
- HTTP/2 remains the highest protocol version with a stable native transport.
- Consumers receive no misleading HTTP/3 API surface or runtime success path.
- The next implementation can start from explicit interoperability and
  platform gates instead of replacing them with local-only smoke tests.
