# ADR 0018: Agent Run, Journal and Evaluation Contract

Status: Accepted

Date: 2026-09-12

Roadmap item: R-3201

## Context

Phase 32 delivers governed agent runs. A run needs durable identity (so a
crash resumes instead of restarting), bounded resources (so an agent loop
cannot become a financial incident), and an auditable record of every effect
(so behavior can be reproduced instead of argued about). It also needs a
contract for what may be asserted in an evaluation versus what must be proven
deterministically in CI; conflating the two would let a flaky grader stand in
for a guarantee.

The runtime already owns generational handles (`runtime/src/handles/mod.rs`)
and `std.api.trace` already emits OpenTelemetry-shaped spans. Both are reused
rather than reinvented. The journal format is owned by `R-3217`, but this ADR
freezes the record shape and the replay algorithm first, because every later
governance item (budget, approval, assertions, taint, compensation) writes to
it.

## Decision

### `Run` is a handle

`Run` is an opaque handle with a new `HandleKind` entry, alongside new entries
for the chunk stream and the tool registry. As with every other handle,
`Run` is an encoded `HandleId` (kind, slot, generation), not a pointer; a
reused slot cannot make an old `Run` silently refer to a new object.

A run owns three identities:

- **Lifecycle identity** — created by `agent_start`, consumed by `agent_end`.
  Double end, use after end, and unknown-run handles return typed errors, never
  panics. `agent_start` fails fast when a tool's declared effects exceed the
  run's grant; effects are checked at startup, not on first call.
- **Budget identity** — the declared ceilings (`max_tokens`, `max_cost_micros`,
  `max_seconds`, `max_tool_calls`; 0 means unbounded for the corresponding
  ceiling), and the accumulated usage. Accounting matches provider-reported
  usage exactly. Crossing a ceiling cancels the run cooperatively: the run is
  marked cancelled, in-flight tasks are cancelled through the existing
  cancellation entrypoints, and subsequent calls return typed errors. No
  thread is killed; long operations observe the cancellation flag at their
  await points. `Report.status` distinguishes `completed`, `budget_exceeded`,
  `failed` and `rolled_back`, and names the ceiling that was hit.
- **Journal identity** — the run id every journal record carries, and the run
  goal (`AgentSpec.goal`) cited by tests, evals, journal records and denial
  messages.

`Run` and `ChunkStream` are the opaque surface types described in the plan's
section 4; `AgentSpec` and `Report` are plain records.

### Journal record shape

The journal is an append-only sequence of records:

```text
{ run, step, kind, input digest, output digest, idempotency key, seed, usage, timestamp }
```

- `run` — run identity.
- `step` — monotonically increasing step sequence within the run.
- `kind` — what the record describes (model call, tool call, approval,
  assertion, compaction, compensation, …).
- `input digest` / `output digest` — digests of the request and response.
  **Digests only by default.** Payload capture is opt-in and never captures
  credentials.
- `idempotency key` — derived from run, step and input digest, so a retry after
  a crash collides with the recorded step instead of duplicating the effect.
- `seed` — the effective sampling seed, recorded so a deterministic run can be
  replayed and so a provider that silently ignores the seed can be detected.
- `usage` — tokens and cost attributed to the step.
- `timestamp`.

The writer flushes before returning from any effect-bearing call, so a crash
cannot lose the record of an effect that already happened. This is a durability
requirement on the writer, not a best-effort log.

`R-3217` owns the file format and the writer. This ADR freezes the record
shape; a change to it requires revising this ADR.

### Replay algorithm

Replay is forward-only and keyed:

1. Read the journal in step order.
2. For each step, compute the step key (run, step, input digest) and compare it
   with the recorded record.
3. A **matching key** returns the recorded output instead of executing the
   effect.
4. A **missing key** executes the effect normally and appends a new record.
   Partial journals therefore resume forward rather than failing.
5. Retries after a crash derive the same idempotency key and collide with the
   recorded step, so an effect is never executed twice across a replay.

Replay produces identical tool ordering and outputs. A replayed run must not
ask twice for an approval that was already decided.

### Approval caching

`approve(run, action)` asks a registered approver. The default approver denies
when no UI is attached; there is no implicit allow. Decisions — including
allow-once and allow-always — are journaled with who and when. Replay consults
the journal first and never re-asks a decided action. An approval is scoped to
its action: allow-once applies to one step, allow-always to the run.

### `require` assertion semantics

`require(run, condition, message)` is a governed assertion:

- `true` passes and the outcome is journaled.
- `false` returns a typed error carrying the message and the run goal, and
  marks the run failed.

An assertion failure is a run outcome, not a panic; it is visible in the
journal and in `Report.status`. Assertions are deterministic checks that
belong in a run, distinct from evaluation graders.

### Taint ledger and sink gating (D10)

Provenance is recorded per message and per digest in a run ledger. Strings
cannot carry taint through the i64 ABI, so message-level provenance is the
honest granularity; this ADR records that limit rather than promising
string-level flow.

- Tool results and external content default to **untrusted**.
- `untrusted(run, value, origin)` records the value's digest as untrusted and
  returns the value unchanged.
- `trust(run, value, reason)` records an audited declassification for the
  digest and returns the value unchanged. Declassification is explicit and
  audited; there is no automatic declassification.
- Host calls classified `sink` in the catalog (ADR 0017 `sink` field) are gated
  while the run holds untrusted content, per run policy
  (`untrusted`: `approve` | `block` | `allow`; default `approve`).

Gating decisions are journaled. The invariant is I9: sensitive sinks are not
reached silently while untrusted content is in context.

### Compensation semantics (D11)

Compensation is declared, not inferred:

- `compensate(run, tool, arguments_json)` journals a pending compensation.
  Pending compensations are held **LIFO**.
- Unknown tool names are compile errors when the name is a literal (`E3205`).
- `rollback(run, reason)` executes pending compensations in LIFO order through
  the governed dispatch, so capabilities and taint still apply, and is
  replay-safe: a compensation that already executed during replay is not
  executed again.
- `agent_end` reports `compensations_pending`, so a silent leak is visible even
  when the author never calls `rollback`.

Compensation never executes twice (including after replay) and never executes
implicitly on the fatal path. Automatic rollback from failure paths would
over-promise: the runtime cannot re-enter compiled tool code from the fatal
panic path in this phase. The author decides when to compensate.

### OpenTelemetry GenAI spans

`std.agent` reuses `std.api.trace` and emits GenAI spans mapping to the pinned
OpenTelemetry semantic conventions version **1.34.0**
(`https://opentelemetry.io/schemas/1.34.0`). That version is the minimum
containing both `gen_ai.conversation.id` (added in 1.34.0) and the
`invoke_agent` operation (added in 1.33.0).

Span mapping:

| Spectra activity | Span |
| --- | --- |
| a run's agent invocation | `invoke_agent` |
| a planning model turn | `plan` |
| a tool execution | `execute_tool` |
| a model call | `chat {model}` |

Attributes:

- `gen_ai.agent.name` — the run's agent name;
- `gen_ai.conversation.id` — the conversation/session correlating the run's
  model calls and messages.

The conventions version is recorded on the span. Content capture (prompts,
completions, tool payloads) is opt-in and off by default; spans validate
against the pinned version with content absent unless opted in.

### What an eval may assert versus what a test must cover

An **evaluation** may assert things that are statistical, model-dependent, or
optimization targets: `pass@1`, `pass^k`, answer quality, cost regression
against a checked-in baseline, and whether a judge grader accepts an answer.
Judge grading is opt-in. An eval may also assert deterministic ledger
properties (tool order, journal contents, budget accounting) because those are
recorded, not generated.

A **deterministic test** must cover everything the platform guarantees
independently of model behavior: the replay algorithm (no effect twice across
a replay, partial journals resume forward), approval caching (a decided action
is never re-asked), `require(false)` marking the run failed with message and
goal, LIFO compensation that executes exactly once, taint gating per the
block/approve/allow policy matrix, budget ceiling enforcement with exact
accounting, and span shape against the pinned conventions version with content
absent.

No guarantee may be pinned only by an eval. An eval that passes may not stand
in for a deterministic test of a platform invariant; conversely, an eval is
free to measure things a deterministic test cannot.

## Rationale

**Why a handle.** Handles already provide generation-guarded identity and typed
errors for misuse, which is exactly the lifecycle contract a run needs
(double end, use after end). A pointer or a raw integer would re-implement it
badly.

**Why digests by default.** Crash resume and reproducibility are the operable
properties. Storing payloads by default would leak prompts, completions and
tool arguments into a durable file. Digests give idempotency and comparison
without the leak; capture is opt-in for debugging.

**Why derived idempotency keys.** Deriving the key from run, step and input
digest means a post-crash retry lands on the same key as the recorded step
instead of creating a second effect. This is what makes "no effect twice" true
rather than aspirational.

**Why fail-closed approvals.** An unattended agent with an implicit allow is a
security hole; the default must deny. Caching decisions in the journal is what
makes replay deterministic and non-interactive.

**Why declared compensation.** The runtime cannot, in this phase, re-enter
compiled tool code from a fatal path. Inferring compensation from effects
would promise a rollback the runtime cannot deliver. Declaring it, running it
through the governed dispatch, and reporting what remains pending is honest
and still auditable.

**Why message-level taint.** The ABI carries i64 scalars and handles; strings
have no side channel for provenance. Message-level provenance is what the ABI
can support, and the ADR documents the limit instead of pretending otherwise.

**Why a pinned conventions version.** GenAI conventions are moving. Pinning
1.34.0 makes span tests deterministic and makes a future bump an explicit,
reviewable change rather than silent drift.

## Consequences

- `R-3211` implements the run handle, lifecycle, `Report`, and the model
  gateway, honoring the budget and journal identities above.
- `R-3212` implements memory with provenance (origin, run id, timestamp, goal).
- `R-3213` implements the run context; enforcement (R-3214) consults it.
- `R-3214` routes all four generic dispatch entrypoints through the single
  policy decision (ADR 0016).
- `R-3216` implements budget accounting, cooperative cancellation, and report
  integration.
- `R-3217` implements the journal writer, replay, approval, assertions and
  tracing; it owns the on-disk format and may not change the record shape
  without revising this ADR.
- `R-3220` implements the eval harness with deterministic graders in CI, an
  opt-in judge, and a regression baseline.
- `R-3223` implements the taint ledger and sink gating; `R-3224` implements
  compensation declaration and execution.
- Tests, not evals, pin replay safety, approval caching, assertion semantics,
  LIFO compensation, taint gating, budget enforcement and span conformance.

## Rejected Alternatives

### Store full payloads in the journal by default

Rejected. It leaks prompts, completions, headers and tool arguments into a
durable artifact. Digests by default, opt-in capture, never credentials.

### Automatic rollback of external effects

Rejected. The runtime cannot re-enter compiled tool code from the fatal panic
path in this phase, and an automatic rollback over a filesystem or network
effect would over-promise. Compensation is declared and executed explicitly.

### String-level taint tracking

Rejected. The ABI carries i64 scalars and handles; there is no place to attach
per-string provenance without an ABI change. Message-level provenance is the
honest granularity and the limit is documented.

### An implicit allow when no approver is attached

Rejected. Unattended agents must fail closed. The default approver denies.

### Let evals pin platform invariants

Rejected. Model-dependent evaluation is not a proof of a platform guarantee.
Deterministic tests must cover the invariants; evals measure quality and cost.
