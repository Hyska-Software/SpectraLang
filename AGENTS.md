# AGENTS.md

## Purpose

This file defines repository-specific instructions for coding agents working in SpectraLang.

The repository contains implementation planning documents that agents can use when
the user explicitly asks for roadmap or planning work:

- [docs/production-ai-implementation-plan.md](/D:/Lang/SpectraLang/docs/production-ai-implementation-plan.md)
- [docs/roadmap-backlog.md](/D:/Lang/SpectraLang/docs/roadmap-backlog.md)
- [roadmap/roadmap.toml](/D:/Lang/SpectraLang/roadmap/roadmap.toml)

These files are operational project artifacts when that scope is requested, but they
are not mandatory inputs for ordinary implementation tasks. By default, agents
follow the user's request, inspect the relevant code and tests, and validate the
requested change without selecting or updating roadmap items.

---

## Repository Context

SpectraLang is a language and toolchain project with these major areas:

- `compiler/`: lexer, parser, AST, semantic analysis, linting, pipeline
- `midend/`: IR lowering, validation, optimization
- `backend/`: Cranelift codegen, JIT/AOT
- `runtime/`: runtime services, memory, host calls, stdlib plumbing, async reactor, API crate host calls
- `runtime/src/api/`: HTTP parser, server, client, JSON, TLS, routing (sibling to the existing `runtime/src/stdlib/`)
- `runtime/src/reactor/`: platform-specific event loop (`epoll` / `IOCP` / `kqueue`)
- `tools/spectra-cli/`: CLI
- `tools/spectra-lsp/`: language tooling / LSP
- `tools/spectra-interop/`: language interop
- `tests/`: language, semantic, CLI, and project tests
- `docs/`: language docs, project docs, implementation planning docs
- `docs/api/`: API library reference (added in Phase 22)
- `roadmap/`: machine-readable roadmap tracking
- `packages/spectra-api/`: the published Spectra package that delivers the API platform surface (Phase 22+)
- `examples/api/`: runnable API examples for REST, WebSocket, GraphQL, gRPC, SSE, and database integration

The platform targets two complementary workstreams:

- **AI/ML core**: tensors, autodiff, ONNX, RAG, ML serving, model evaluation, drift detection.
- **API platform**: async/await first-class syntax, `spectra.api` package (HTTP/1.1 → HTTP/2 → HTTP/3, TLS, JSON, middleware, auth, OpenAPI, drivers, observability).

---

## Source of Truth Rules

For ordinary implementation work, the user's request, actual code, and passing
tests define the scope and current reality. The planning documents are not required
unless the user explicitly asks to use them.

When the user explicitly requests roadmap or planning work, use the following
precedence:

1. Actual code and tests
2. `roadmap/roadmap.toml`
3. `docs/roadmap-backlog.md`
4. `docs/production-ai-implementation-plan.md`
5. Older planning notes such as `docs/project-manager.md`

Interpretation rules for roadmap or planning work:

- Code and passing tests define current reality.
- `roadmap/roadmap.toml` is the canonical structured execution tracker.
- `docs/roadmap-backlog.md` is the canonical human-readable execution backlog.
- `docs/production-ai-implementation-plan.md` is the canonical long-form strategic implementation plan.
- If older docs conflict with the three files above, update the older docs or explicitly note the conflict.

### Always write comments in English.

---

## Required Planning Files

When roadmap or planning work is explicitly requested, agents must preserve and
maintain the following files:

For ordinary implementation tasks, do not update these files merely because the
code happens to relate to an existing or future roadmap item.

### 1. Strategic Plan

File:
- [docs/production-ai-implementation-plan.md](/D:/Lang/SpectraLang/docs/production-ai-implementation-plan.md)

Purpose:
- long-term implementation vision
- workstream decomposition
- production-gap coverage
- acceptance criteria by phase

Update this file when:
- a major subsystem direction changes
- a new workstream is added
- a major architectural dependency changes
- the project focus for AI/ML capabilities changes

Do not update this file for:
- minor task status changes
- small bug fixes
- one-off tactical implementation notes

### 2. Human Backlog

File:
- [docs/roadmap-backlog.md](/D:/Lang/SpectraLang/docs/roadmap-backlog.md)

Purpose:
- issue-ready work breakdown
- human planning and prioritization
- readable status and acceptance list

Update this file when:
- a roadmap item is added, removed, split, merged, or reprioritized
- acceptance criteria change
- recommended execution ordering changes

### 3. Structured Roadmap

File:
- [roadmap/roadmap.toml](/D:/Lang/SpectraLang/roadmap/roadmap.toml)

Purpose:
- machine-readable project planning
- automation, reporting, dependency tracking

Update this file when:
- status changes
- dependencies change
- ownership changes
- risk changes
- an item is created or closed

When roadmap mode is active, whenever an item is changed in the backlog, update
`roadmap.toml` in the same change unless there is a clear reason not to.

---

## Agent Responsibilities

The user's explicit request defines the scope of ordinary implementation work.
Agents do not need to identify roadmap items, consult planning documents, or
update roadmap state unless the request explicitly activates roadmap or planning
work.

### Activating Roadmap Mode

Roadmap mode is active when the user explicitly:

- asks the agent to follow or execute the roadmap;
- names a roadmap phase or item such as `R-xxxx` as the implementation target; or
- asks to create, revise, validate, or synchronize the planning documents.

Once roadmap mode is active, the roadmap governance rules in this file apply to
the requested scope, including item identification, status, dependencies,
acceptance criteria, document synchronization, and completion reporting.

For ordinary implementation work, the agent should inspect the relevant code and
tests, implement the requested behavior, run proportional validation, and report
the result without changing roadmap documents.

---

## Status Update Rules

This section applies only when roadmap mode is active. It does not require status
updates for ordinary implementation work that the user did not connect to the
roadmap.

Use only these status values in `roadmap/roadmap.toml` unless the file schema is intentionally changed:

- `not_started`
- `in_progress`
- `blocked`
- `complete`

Use these rules consistently:

- `not_started`: no implementation has begun
- `in_progress`: active implementation or active design work is happening
- `blocked`: work cannot continue due to dependency, design uncertainty, or missing prerequisite
- `complete`: acceptance criteria are satisfied and the implementation is validated

Do not mark an item `complete` merely because partial code exists.

An item may be marked `complete` only when:

- implementation is merged in the working tree
- relevant tests or validation exist
- stated acceptance criteria are satisfied or explicitly revised

### Production Completion Rule

Agents must implement roadmap items to the production meaning of the planning
documents, not to a reduced "alpha" interpretation. If an item mentions
vectorization, benchmark evidence, interop, memory safety, diagnostics, or
production hardening, those requirements are part of completion unless the
planning files are explicitly and narrowly revised first.

Partial implementations are allowed only when they are reported and tracked as
partial. In that case:

- keep `roadmap/roadmap.toml` status as `in_progress`, not `complete`
- keep the original production acceptance criteria visible
- add "completed so far" and "remaining before completion" notes in the backlog
- never rename partial work as complete by adding labels such as alpha, MVP, or
  prototype unless the roadmap item itself explicitly defines that as the target
- never claim benchmarks, performance wins, SIMD, BLAS, memory-safety, or
  production readiness without checked-in evidence and validation commands

Before reporting an implementation as complete, agents must compare the final
diff against all acceptance criteria in:

1. `roadmap/roadmap.toml`
2. `docs/roadmap-backlog.md`
3. `docs/production-ai-implementation-plan.md`

If any criterion is not implemented and validated, the item remains
`in_progress`.

### Core Language Correction Rule

When analyzing or correcting core language constructs such as variables,
assignments, `if`, `unless`, `while`, `while let`, `do-while`, `for`, `loop`,
`switch`, `match`, methods, structs/classes, traits, closures, and return paths,
agents must validate the full pipeline, not only parser acceptance.

Required behavior:

- add or update at least one `.spectra` regression in `tests/validation/` or
  `tests/errors/` for the affected construct
- run a frontend/semantic test when the change affects parsing, binding,
  type checking, diagnostics, or return-path analysis
- run a midend/backend or normal `spectralang run` validation when the change
  affects lowering, branches, loops, method calls, aggregate values, or JIT/AOT
  execution
- do not promote a feature from experimental/beta to stable unless it parses
  without flags, compiles, and executes through the normal CLI path when it has
  runtime behavior
- if a construct has compile-only coverage but no reliable execution coverage,
  report the missing execution evidence and do not claim runtime support as
  validated. Add a roadmap item only when roadmap mode is active or the user
  explicitly requests planning follow-up.

Current production baseline:

- `switch`, `unless`, `do-while`, and `loop` are stable core syntax, not active
  experimental features.
- `--enable-experimental <feature>` is accepted only as a compatibility no-op
  until a future gated feature is added and documented.
- `spectralang --list-experimental` must report no active syntax gates unless the
  maturity policy documents and validates a future gate. Updating the roadmap in
  the same change is required only when roadmap mode is active.

### Integrated Project Failure Triage Rule

The technical failure-triage requirements apply whenever this validation track is
executed. Creating a roadmap item for an unfixed failure is required only when
roadmap mode is active or the user explicitly requests that follow-up.

When executing the Phase 20 integrated language and AI Support validation track
(`R-2008` through `R-2013`), agents must not ignore real implementation
failures found in complete projects.

Required behavior:

- if the failure is fixed in the same change, add regression coverage and record
  validation evidence
- if the failure is not fixed in the same change, report it with the reproducing
  project or command; in roadmap mode, add a new roadmap/backlog item beyond
  `R-2008` through `R-2013`
- when a roadmap item is created, it must include owner, phase, dependencies,
  risk, acceptance criteria, and the reproducing project or command
- do not mark the integrated project gate complete while untracked failures
  remain

---

## Acceptance Criteria Rules

These are technical quality rules for the requested work and do not require a
roadmap item or planning-document update.

Acceptance criteria are completion gates, not aspirations.

Agents must:

- keep them specific
- keep them testable
- avoid vague language like "improved", "better", or "more robust" without measurement

Good acceptance criteria:

- "`cargo test -p spectra-cli` passes"
- "JSON diagnostics include stable codes for syntax and semantic errors"
- "MLP example trains end-to-end on the reference dataset"

Bad acceptance criteria:

- "tooling is much better"
- "GPU support is mostly done"
- "docs look complete"

If implementation reveals that an acceptance criterion is wrong or unrealistic:

- update it explicitly
- keep the change narrow
- preserve intent

---

## Dependency Update Rules

This section applies only when roadmap mode is active.

When changing an item in `roadmap.toml`, review:

- `dependencies`
- `phase`
- `owner`
- `priority`
- `risk`

Agents must update dependencies when:

- a prerequisite becomes necessary
- a previous prerequisite is no longer needed
- an item is split into multiple items

Do not leave stale dependencies behind.

---

## Adding New Roadmap Items

This section applies only when roadmap mode is active or the user explicitly asks
to create a roadmap item.

When adding a new roadmap item:

1. Add it to `docs/roadmap-backlog.md`
2. Add it to `roadmap/roadmap.toml`
3. Place it in the correct phase
4. Assign:
   - unique ID
   - owner
   - priority
   - risk
   - dependencies
   - concise summary
   - acceptance criteria

### ID Convention

Use:

- `R-###` for roadmap items

Recommended grouping:

- `R-0xx`: governance
- `R-1xx`: compiler productionization
- `R-2xx`: type system
- `R-3xx`: tensors
- `R-4xx`: kernels/runtime numerics
- `R-5xx`: autodiff
- `R-6xx`: ML framework
- `R-7xx`: accelerators
- `R-8xx`: interop
- `R-9xx`: package manager/registry
- `R-10xx`: tooling maturity
- `R-11xx`: concurrency/serving
- `R-12xx`: security/ops
- `R-13xx`: docs/adoption
- `R-21xx`: async language core
- `R-22xx`: API library foundation (`spectra.api`)
- `R-23xx`: middleware and security
- `R-24xx`: advanced API features
- `R-25xx`: persistence and database
- `R-26xx`: API tooling and developer experience
- `R-27xx`: observability and API operations
- `R-28xx`: API conformance and release

Do not renumber existing IDs unless absolutely necessary.

---

## Owner Groups

This ownership table applies when assigning or reviewing roadmap items.

Every roadmap item in `roadmap/roadmap.toml` is owned by exactly one group.
Owners are responsible for design, implementation, tests, and the
acceptance criteria of items in their group, and for raising cross-cutting
risks in the planning files.

| Owner | Scope |
|---|---|
| `frontend` | lexer, parser, AST, diagnostics, language surface |
| `semantic` | type system, imports, traits, validation |
| `midend` | IR lowering, optimization, validation, graph IR |
| `backend` | Cranelift, object emission, target ABIs |
| `runtime` | runtime services, allocators, reactor, async stdlib, host calls |
| `numerics` | tensor core, kernels, BLAS/GPU integration, numerics conformance |
| `ml` | autodiff, modules, optimizers, datasets, model serving, ML safety |
| `web` | HTTP server/client, routing, middleware, WebSocket, SSE, OpenAPI, `spectra.api` package |
| `db` | drivers, query builder, migrations, ORM, connection pool |
| `tooling` | CLI, formatter, lint, LSP, debugger, benchmarks, scaffolder |
| `ecosystem` | package manager, registry, ADRs, documentation, examples, release |

The `web` and `db` groups were introduced together with the API platform
workstream (Phase 22+). Items that previously fell under `runtime` purely
because they were "in the runtime crate" should be reassigned to `web` or
`db` when the work shifts from runtime infrastructure to the public API
surface or the database layer. The cross-cutting review rules defined in
this file apply uniformly across all owner groups.

---

## Splitting and Merging Items

This section applies only when roadmap mode is active.

If a roadmap item is too large:

- split it into smaller items
- preserve the original intent
- rewire dependencies explicitly
- update both backlog and TOML

If two items are truly redundant:

- merge only if acceptance criteria overlap heavily
- document the merge in the backlog text
- preserve traceability where possible

---

## Documentation Synchronization Rules

These synchronization rules apply only when planning documents are explicitly in
scope or roadmap mode is active.

When changing planning docs:

- keep terminology consistent across all three files
- keep phase names aligned
- keep item IDs aligned
- keep acceptance criteria semantically equivalent between backlog and TOML

Do not update only one planning artifact when the change affects all of them.

Expected synchronization:

- strategic change: update all relevant files
- task-level execution change: update backlog and TOML
- pure status change: usually update TOML, optionally backlog if human readability benefits

---

## Validation Rules for Planning Files

These validation rules apply only after planning files are edited as part of an
explicit roadmap or planning task.

After editing planning files, agents should validate:

### For `roadmap/roadmap.toml`

- file parses successfully as TOML
- all item IDs are unique
- all dependency IDs reference existing items
- all phases referenced by items exist

### For `docs/roadmap-backlog.md`

- no orphan item IDs
- no references to removed phases
- no contradiction with `roadmap.toml`

### For `docs/production-ai-implementation-plan.md`

- phases still match project strategy
- new workstreams are represented cleanly
- acceptance direction remains coherent with backlog

---

## When to Update Older Documentation

Older documents such as:

- [docs/project-manager.md](/D:/Lang/SpectraLang/docs/project-manager.md)
- [README.md](/D:/Lang/SpectraLang/README.md)
- language reference files

should be updated when the user explicitly requests documentation or planning
synchronization, or when the inconsistency is part of the requested scope:

- they materially contradict the current implementation
- they materially contradict roadmap reality in a roadmap or planning task
- they present outdated implementation status as current truth

Do not automatically expand older docs during unrelated tasks or because a code
change is associated with an existing roadmap item.
Only update them when the inconsistency is relevant and significant.

---

## AI/ML Direction Rules

Because SpectraLang is intended to evolve toward AI and machine learning
workloads, planning work should prefer decisions that strengthen:

- tensor-first language/runtime design
- numerical correctness
- reproducibility
- performance visibility
- accelerator readiness
- interop with existing ML ecosystems

These are architectural goals and context, not a requirement to follow roadmap
ordering during ordinary implementation. Roadmap drift is evaluated only when
roadmap mode is active.

In practical terms:

- tensor/runtime/autodiff/interop work is generally higher leverage than cosmetic syntax work
- package manager and tooling are high priority if they unblock real ML use
- documentation should keep the AI production target explicit

---

## API Platform Direction Rules

Because SpectraLang is also intended to be a first-class language for
building HTTP and event-driven APIs natively, planning work should prefer
decisions that strengthen:

- async/await as a first-class language and runtime model
- typed HTTP primitives (`Request`, `Response`, `Method`, `Status`, `Header`, `Cookie`)
- middleware and authentication as composable, documented building blocks
- first-class drivers for the dominant production backends (PostgreSQL, SQLite, Redis)
- observability and threat mitigation as documented defaults, not afterthoughts
- toolchain that turns a scaffolded project into a running service in minutes
- conformance, interop, and production hardening before declaring v1.0

The `spectra.api` package is the canonical home for this surface. New HTTP,
routing, middleware, WebSocket, SSE, or database work that targets the
public API surface belongs in `spectra.api` and is owned by the `web` or
`db` owner groups, not by `runtime` or `frontend`.

For roadmap or API-platform planning, the following sequencing and release
guidance applies. It does not block an ordinary implementation requested by the
user merely because that work is outside the current roadmap sequence:

- Phase 21 (async/await) is the foundation for the planned API platform.
- HTTP/1.1 is the first planned protocol; HTTP/2 and HTTP/3 follow once the
  foundation is stable.
- TLS, authentication, validation, and error handling remain production defaults
  for API implementations.
- Driver coverage should match the dominant production backends first;
  exotic drivers are follow-on work.
- The `spectralang api new` / `spectralang api dev` / `spectralang api doc`
  experience is part of the public contract.
- `R-2801` (API conformance v1) is the release-candidate gate for
  `spectra.api` v1.0 when roadmap mode is active.

---

## Completion Reporting Guidance

This roadmap-specific reporting format applies only when roadmap mode is active.
For ordinary implementation work, report the changed code and validation results
without inventing roadmap IDs or status changes.

When reporting completed work to users or maintainers, agents should:

- mention the roadmap item IDs affected
- state whether code, backlog, and roadmap were updated
- state whether acceptance criteria were satisfied or revised

Recommended concise format:

- `Completed: R-104, R-105`
- `Updated: code + roadmap.toml + roadmap-backlog.md`
- `Acceptance: satisfied`

---

## Do Not

Agents must not:

- mark work complete without the validation appropriate to the requested change
- change item IDs casually
- leave roadmap dependencies stale when roadmap mode is active
- update backlog prose while forgetting `roadmap.toml` when roadmap mode is active
- introduce new planning terminology without aligning existing files

---

## Recommended Default Workflow

For ordinary implementation work (the default):

1. Read the user's request and the relevant code, tests, and local contracts.
2. Implement the requested code or documentation changes.
3. Run the relevant validation/tests.
4. Report the result and any remaining technical gaps.

For roadmap or planning work explicitly requested by the user:

1. Read the relevant items in `roadmap/roadmap.toml` and
   `docs/roadmap-backlog.md`.
2. Implement the requested code or planning changes.
3. Update roadmap status, dependencies, backlog, or strategic plan as required by
   the active scope.
4. Validate TOML parsing and cross-file consistency when planning files change.
