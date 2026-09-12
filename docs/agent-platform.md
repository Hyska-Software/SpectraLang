# Agent Platform

Runtime and compile-time governance for `std.agent` runs.  The phased design and
production intent live in [`docs/agent-platform-plan.md`](agent-platform-plan.md);
the capability security model — the vocabulary, the single dispatch enforcement
seam and the guarantees a run provides — is recorded in
[`docs/adr/0016-agent-capability-enforcement.md`](adr/0016-agent-capability-enforcement.md).

The reference below is generated from the contract catalog by
`scripts/generate_capability_reference.py`; run it with `--check` in CI to detect
drift.

<!-- BEGIN GENERATED CAPABILITY REFERENCE -->

### Grant forms

`AgentSpec.allow` grants are validated at compile time (R-3215) against the
contract catalog — the same data the runtime dispatches, and the same names
`surface --json` reports.  A grant is one of:

- a namespace prefix, such as `spectra.std.fs`, which grants every host call
  registered beneath it;
- a full host call, such as `spectra.std.fs.fs_read`;
- a scoped host call, such as `spectra.api.client.request:host=api.example.com`,
  accepted only when the catalog declares the scope key for that host call.

Grants name the runtime host call (`spectra.…`), which is what the dispatch
seam evaluates.  The catalog path (`std.fs.fs_read`) is not a grant form;
writing it fails `E3201`, and the diagnostic suggests the runtime name.

### Namespaces

51 namespace grants cover 1115 host calls.

| Namespace grant | Host calls |
| --- | --- |
| `spectra.api.client` | 5 |
| `spectra.api.cors` | 13 |
| `spectra.api.db.migrate` | 2 |
| `spectra.api.db.pool` | 7 |
| `spectra.api.db.postgres` | 34 |
| `spectra.api.db.redis` | 18 |
| `spectra.api.db.sqlite` | 22 |
| `spectra.api.errors` | 9 |
| `spectra.api.form` | 22 |
| `spectra.api.graphql` | 29 |
| `spectra.api.grpc` | 43 |
| `spectra.api.handler` | 19 |
| `spectra.api.health` | 2 |
| `spectra.api.http` | 72 |
| `spectra.api.http3` | 28 |
| `spectra.api.json` | 19 |
| `spectra.api.jwt` | 2 |
| `spectra.api.middleware` | 28 |
| `spectra.api.multipart` | 16 |
| `spectra.api.oauth` | 11 |
| `spectra.api.query` | 22 |
| `spectra.api.routing` | 17 |
| `spectra.api.security` | 7 |
| `spectra.api.server` | 13 |
| `spectra.api.session` | 13 |
| `spectra.api.sse` | 20 |
| `spectra.api.tls` | 4 |
| `spectra.api.trace` | 17 |
| `spectra.api.validation` | 17 |
| `spectra.api.version` | 3 |
| `spectra.api.websocket` | 24 |
| `spectra.std.agent` | 17 |
| `spectra.std.char` | 8 |
| `spectra.std.collections` | 48 |
| `spectra.std.concurrent` | 17 |
| `spectra.std.convert` | 11 |
| `spectra.std.env` | 6 |
| `spectra.std.error` | 7 |
| `spectra.std.fs` | 10 |
| `spectra.std.io` | 7 |
| `spectra.std.math` | 24 |
| `spectra.std.ml` | 117 |
| `spectra.std.numeric` | 65 |
| `spectra.std.option` | 5 |
| `spectra.std.random` | 4 |
| `spectra.std.range` | 8 |
| `spectra.std.result` | 7 |
| `spectra.std.serve` | 31 |
| `spectra.std.string` | 27 |
| `spectra.std.tensor` | 115 |
| `spectra.std.time` | 23 |

### Scope keys

A scoped grant is accepted only where the host call declares the key
(catalog `scope_keys`).

| Host call | Scope keys |
| --- | --- |
| `spectra.api.client.request` | `host`, `method` |
| `spectra.api.db.migrate.apply_sqlite` | `table` |

<!-- END GENERATED CAPABILITY REFERENCE -->
