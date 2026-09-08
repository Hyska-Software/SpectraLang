# Goal: Real HTTP/3, gRPC, GraphQL, and Suspendable Coroutines

Execute `docs/goals/real-protocols-coroutines/PLAN.md`.

The implementation must be real at the protocol/runtime boundary, not a parser-only or in-process simulation:

- HTTP/3 uses Quinn + h3 over QUIC/TLS 1.3 with DATA/trailer streaming and graceful shutdown.
- gRPC implements interoperable HTTP/2 framing, metadata, deadlines, status trailers, cancellation, and all four RPC cardinalities.
- GraphQL uses a real validated dynamic schema/executor with variables, fragments, directives, introspection, resolver callbacks, errors, and subscriptions.
- `async`/`await` uses lazy task frames and compiled poll/resume/drop state machines; `await` never blocks the executing lane or runs the body at construction.

Preserve the plan's ownership, opaque-handle contract, cutover, kill criteria, and direct target-perspective evidence. Do not claim completion without JIT/AOT coroutine tests and real localhost protocol interoperability tests.
