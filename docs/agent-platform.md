# Agent Platform

Runtime and compile-time governance for `std.agent` runs.  The phased design and
production intent live in [`docs/agent-platform-plan.md`](agent-platform-plan.md);
the capability security model — the vocabulary, the single dispatch enforcement
seam and the guarantees a run provides — is recorded in
[`docs/adr/0016-agent-capability-enforcement.md`](adr/0016-agent-capability-enforcement.md).

## Overview

`std.agent` makes agents ordinary Spectra programs. There is no agent language
mode and no second type system: a run is a handle, a tool is a `public async`
function with `#[agent_tool("...")]`, and a model call is a host call.

The platform has three layers, with a one-way dependency rule:

| Layer | What it is | Where it lives |
| --- | --- | --- |
| A — surface | derived, static project view: `surface`, `impact`, `explain`, `docs` | the compiler; consumed by coding agents |
| B — governance | capabilities, taint, approval, budget, durability, trace | the runtime, at one generic dispatch point |
| C — execution | twenty free functions plus `json_schema` | `std.agent`, called by Spectra code |

A informs B, B authorizes C, and C never decides by itself: a layer-C function
that needs an effect asks layer B, and an ungranted effect fails with a
structured error (`capability_denied`, `trust_required`, `budget_exceeded`, …)
rather than an opaque exception.

The life of a run:

1. `agent_start(spec_json)` validates the spec and every grant against the
   catalog, allocates the run and opens the journal. The tool table reaches the
   runtime separately: lowering emits one idempotent registration function per
   module that declares tools, and every dispatch entry point calls it (ADR
   0019).
2. The run is active for the dynamic extent of its own host calls — `ask`,
   `ask_json`, `ask_stream`, `embed`, `act`, `tool_call` — so model-driven
   effects and tool wrappers are bounded by its ceiling; the program's own
   frame keeps pre-phase behaviour.
3. `act` runs the model/tool loop through the single governed dispatch; every
   step is journaled with an idempotency key before it returns.
4. `agent_end(run)` closes the run and returns the report, including the
   ceiling that was hit (if any) and whether the run replayed a journal.

The run contract is fixed in
[`docs/adr/0018-agent-run-contract.md`](adr/0018-agent-run-contract.md), the
tool dispatch ABI in
[`docs/adr/0019-agent-tool-dispatch.md`](adr/0019-agent-tool-dispatch.md), the
single attribute in
[`docs/adr/0017-agent-surface-generation.md`](adr/0017-agent-surface-generation.md).
The adoption path with runnable projects is
[`docs/book/11-agents.md`](book/11-agents.md).

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

51 namespace grants cover 1119 host calls.

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
| `spectra.std.agent` | 21 |
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
## Taint: provenance, sinks and the gate (R-3223)

A run records **provenance** for everything that enters it. `untrusted(run,
value, origin)` tags a value (`user`, `model`, `tool:<name>`,
`external:<source>`); `trust(run, value, reason)` declassifies it and requires
a reason. Both return the value unchanged, so a program tags content inline,
and both are journaled (`kind = "taint"`).

Every message in a run's transcript is tagged with its origin before it reaches
the provider: a prompt is `user`, model output is `model`, and a tool result is
`tool:<name>` and is **untrusted by default**. The ledger is keyed by content
digest and taint is monotone — an untrusted digest is never downgraded by
observation, only by an explicit `trust(run, value, reason)`, and only for the
digest it names. A value derived from an untrusted one has a different digest
and stays untrusted, so `trust` cannot launder a transformation the ledger never
saw.

A **sink** is a catalog entry classified `sink = true` (its effects contain
`mutation`). The sink set is read from `spectra_contract::catalog()`, not from a
list maintained beside it, so a namespace cannot drift out of the
classification. When the run holds untrusted content and a sink is dispatched,
`AgentSpec.untrusted` decides: `block` denies with a `trust_required` reason,
`approve` (the default) routes through the approval registry and fails closed
when no approver is attached, `allow` proceeds. Every decision is journaled
(`kind = "taint_decision"`), so a replayed run never re-asks.

The gate runs at the run's dynamic extent: the model gateway (`ask`,
`ask_json`, `ask_stream`, `embed`) and the dispatch primitives (`act`,
`tool_call`) execute with the run on the active chain, so a tool wrapper's host
calls are decided against the run. A host call the author writes directly on
the program frame is the author's own action, not the run's; that keeps a
program without a run exactly as it was.

### Honest limits

* **Message granularity.** The ledger is per content digest and per message. It
  records that a value with this digest entered the run from some origin and
  whether it was declassified; it records nothing about how that value was
  transformed afterwards. Gating is therefore "this run chain holds untrusted
  content", not "this argument derived from that value" — the conservative
  direction: a sink that does not touch the untrusted value can still be gated,
  and a sink is never reached silently.
* **No string-level flow.** The host ABI carries `i64` scalars and handles;
  there is no per-byte provenance to track, and this item does not pretend
  otherwise. What is bounded is what a run may do *without a decision* once
  external content is in context.
* **Scope predicates are partially deferred.** A sink's declared `scope_keys`
  are read from the dispatch arguments where an extractor exists beside the host
  call (`spectra.api.db.migrate.apply_sqlite` → `table`). The `method` predicate
  on `spectra.api.client.request` is not extracted: the method lives inside the
  `std.api.http.Request` handle owned by `packages/spectra-api`, and decoding it
  from `spectra-agent` would invert the crate dependency. That call is not a
  catalog sink today (`effects = ["host"]`), so nothing classified write-side is
  left ungated; a future method predicate belongs with the request handle's
  owner.
* **Capabilities do not govern the compiler's own machinery.** The coroutine
  ABI (`spectra.async.*`), the JSON derive helpers (`spectra.api.json.*`) and
  the pure format helpers (`spectra.std.convert.*`, `spectra.std.string.*`) the
  compiler emits around author code need no grant, and are excluded from a
  tool's derived effects: requiring a grant for them would mean every
  tool-bearing spec grants the compiler.

## Security model: what a run promises, and what it does not

The promises below are properties the runtime enforces; each is pinned by a
deterministic test (the phase's invariants I1–I10), never only by an eval. The
non-promises are the concept's refusals (plan section 2.5) stated where an
operator or author might otherwise infer them.

### Promised

| Promise | Enforced by |
| --- | --- |
| Every effect executed inside a run's dynamic extent was authorized by that run's capability set. | Capability policy at the single generic dispatch function, called from all four generic entrypoints including the cached and batch paths; denial returns `capability_denied`. |
| A grant that matches no catalog host call fails the build. | `E3201`/`E3202` validation against the contract catalog at compile time. |
| A ceiling is enforced or the run is cancelled; the report names the ceiling that was hit. | Provider-independent accounting, cooperative cancellation, `ceiling` in the report. |
| No effect is executed twice across a journal replay. | Append-only journal with idempotency keys, flushed before an effecting call returns; replay resolves recorded outputs. |
| Human approval fails closed. | `approve` without an attached approver is a deny; the decision is journaled so replay never re-asks. |
| A sensitive sink is not reached silently while untrusted content is in context. | Digest-keyed provenance ledger + catalog sinks + the run's `untrusted` policy; every decision is journaled (`taint_decision`). |
| Declassification is explicit and attributable. | `trust(run, value, reason)` requires a non-blank reason and names the digest it declassifies. |
| Tool metadata exposed to a model is derived, never hand-declared except the description. | `surface --json` reads the compiler's view; tool name, schema, effects and capabilities come from the declaration and the IR call graph. |
| Compensations never execute twice and never implicitly on the fatal path. | LIFO pending list, replay-safe `rollback`, `compensations_pending` in the report. |
| A program with no active run behaves exactly as before the workstream. | The run is active only for the dynamic extent of run-scoped hosts; I5 pins the unchanged behaviour. |

### Not promised

- **No information-flow tracking for strings and scalars.** Taint is
  message/digest granular. Gating is "this run chain holds untrusted content",
  which is conservative: a sink that does not touch the untrusted value can
  still be gated, and a transformation of an untrusted value is a different
  digest that stays untrusted. There is no secret detection.
- **No static verification.** `require(run, condition, message)` is a runtime
  assertion; there are no effect rows in the type system and no effect or
  precondition annotations.
- **No automatic rollback.** Compensation is declared with `compensate` and
  executed only by an explicit `rollback`. The runtime cannot re-enter compiled
  tool code from the fatal panic path, and nothing rolls back the filesystem or
  the network.
- **No capability enforcement on fast host calls.** The 28 fast calls are
  in-process compute with no external effect and no denial channel. The
  exclusion is by construction and pinned by a completeness test (I1) that
  fails if any effect-bearing host call enters the fast path — it is a tested
  invariant, not a promise of convenience.
- **Not a sandbox.** A capability set bounds what a run's own host calls may
  do; it does not isolate the process, the filesystem or the network from the
  embedding program.
- **No async `main`.** Entry points stay `public func main() returns int`;
  async work is driven with `block_on`.
- **No subprocess, so no stdio MCP.** MCP uses the existing HTTP transport.
- **The journal stores digests by default.** `journal_payloads` is opt-in;
  a replay returns recorded outputs, not recorded request payloads.
- **Exactly one attribute.** `#[agent_tool("...")]` is the only new attribute;
  a second one requires revising ADR 0017 first.
