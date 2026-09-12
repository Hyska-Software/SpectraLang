# SpectraLang Agent Platform — Phase 32 Implementation Plan (`std.agent`)

Status: ready to execute
Date: 2026-09-11
Owner groups: `runtime` (lead), `semantic`, `midend`, `backend`, `tooling`, `web`, `ml`, `ecosystem`, `frontend`
Inputs: `docs/agent-platform-concept.md` (vision, unchanged), verified code state (this document, section 2.3)
Canonical tracker: `roadmap/roadmap.toml` (`phase_32`), `docs/roadmap-backlog.md` (Phase 32)
Strategy chapter: `docs/production-ai-implementation-plan.md` ("Agent Platform Vision")
Supersedes: `docs/agent-platform-roadmap.yaml` (folded into this document; the draft file was retired)

This document is the complete, executable specification for Phase 32. It is written to be
read top to bottom: it states what the concept asks for, what the code can actually
support, which adaptations are required, and the exact sequence of work items — each with
tasks, file anchors, acceptance criteria and a validation command.

---

## 0. Document map

| Artifact | Role |
|---|---|
| `docs/agent-platform-concept.md` | Vision. Input to this plan. Not edited by it. |
| **this document** | Single detailed specification for Phase 32: analysis, decisions, items, standards. |
| `roadmap/roadmap.toml` | Machine-readable tracker. One concise record per item. |
| `docs/roadmap-backlog.md` | Human backlog. One concise section per item. |
| `docs/production-ai-implementation-plan.md` | Strategic chapter: vision, sequencing, risk register. |

Rule of precedence: code and passing tests define current reality; this document defines
the intended change; the tracker and backlog carry status. When this document and the
tracker disagree about an item's acceptance, this document wins and the tracker is fixed.

---

## 1. Executive summary

Phase 32 delivers `std.agent`: a native, governed runtime for agents as ordinary Spectra
programs, plus the machine-readable project surface that coding agents consume. Concretely:

- **A. Surface (compiler describes the program):** `spectralang surface --json`,
  `impact --json`, `explain --json`, `docs --json`.
- **B. Governance (runtime enforces):** capability sets on runs, enforced at the single
  host-call dispatch point; message-level taint with sensitive-sink gating; mandatory
  budgets; durable journaled runs with replay; human approval; declared compensation;
  OpenTelemetry GenAI spans.
- **C. Library (Spectra code consumes):** `std.agent` — 20 free functions plus
  `json_schema` on derived records — for model calls, streaming, tool loops, memory,
  approval, assertions, provenance and compensation.

Phase 32 delivers it in six milestones over 24 items (`R-3201` … `R-3224`):

| Milestone | Items | Focus |
|---|---|---|
| M0 — Decisions and surface | `R-3201`–`R-3205` | 4 ADRs, fast-path invariant, `surface`/`impact`/`explain`/`docs` |
| M1 — Single source of truth | `R-3206`–`R-3208` | Catalog schema, generated lowering tables, generated host-call table |
| M2 — Namespace seam | `R-3209` | `std.agent` namespace + crate, one function end to end (JIT and AOT) |
| M3 — Core library | `R-3210`, `R-3211`, `R-3222`, `R-3212` | Tool attribute, model gateway/run, tool loop `act`, memory |
| M4 — Governance | `R-3213`–`R-3217`, `R-3223`, `R-3224` | Run context, capability enforcement, vocabulary, budget, journal/approval/assertions/tracing, taint, compensation |
| M5 — Interop, eval, release | `R-3218`–`R-3221` | MCP, A2A/ACP, eval harness, packaging + conformance gate |

This plan refuses, on purpose:

1. **No second native staticlib.** `spectra.agent` is an rlib aggregated by the existing
   `spectra-api` registration path (one static archive, one registration pair).
2. **No second attribute.** Exactly one new attribute exists: `#[agent_tool("description")]`.
3. **No effect annotations.** Effects and required capabilities are derived from the IR.

Two mechanisms were added by this plan relative to the concept's first sketch, because
verification showed the concept's assumptions could not hold as written (section 2.2):
the tool-dispatch mechanism (section 3.2, D9) and the taint/compensation semantics
(D10, D11). One function was adapted (`ask_stream` returns an opaque chunk stream, not
`Stream<string>`), and one return type was adapted (`embed` returns a 1-D float tensor,
not `List<float>`).

Definition of done for the phase is section 13. Every item ships: code + regression
fixture + validator script registered in `run_tests.ps1` + documentation updates.

---

## 2. Concept analysis: what is implementable, adapted, or refused

### 2.1 Verdict per concept pillar

| Concept pillar | Concept proposal | Verdict | Where it lands |
|---|---|---|---|
| Permissions | `permissions()` + runtime blocks | **Implementable, stronger** | Capability set per run enforced at the single generic host-call dispatch point inside a run. `R-3214`. |
| Effects | `#[effects(...)]` declared by hand | **Implementable, inverted** | Effects derived from `InstructionKind::HostCall` names in the IR; never written. `R-3202`, `R-3214` (§4 of item T4). |
| Introspection | `inspect::dependencies` macro | **Implementable, moved to tooling** | CLI over compiler data: `surface --json`, `impact --json`. `R-3202`, `R-3203`. |
| Agent metadata | `#[derive(Agent)]` | **Implementable as designed (no type)** | An agent is an ordinary function plus a `Run` handle. `R-3211`. |
| Tools | `#[derive(Tool)]` + `impl` | **Implementable as one attribute + derived dispatch** | `#[agent_tool("desc")]`; name/schema/effects/capabilities derived; dispatch via synthesized marshalling wrappers (D9). `R-3210`, `R-3222`. |
| Transactions | `transaction!` rolling back fs and HTTP | **Partially implementable** | Journal + replay + declared compensation + explicit `rollback(run, reason)`. No automatic rollback of external effects; no rollback of the filesystem. `R-3217`, `R-3224`, concept §11. |
| Contracts | `#[requires]`/`#[ensures]` "checked by the compiler" | **Implementable as runtime assertion** | `require(run, condition, message)`. No static verification; documented as such. `R-3217`. |
| Intent | `#[intent(preserve = "...")]` | **Implementable as data** | `AgentSpec.goal`, cited by tests and evals; part of the run report and the journal. `R-3211`, `R-3220`. |
| Memory | `#[derive(Memory)]` | **Implementable as thin layer** | `remember`/`recall` over the validated `std.ml` vector index (R-1803). `R-3212`. |
| Taint (concept §6.2) | Full information flow | **Partially implementable, scoped honestly** | Message/transcript provenance + explicit `untrusted`/`trust` ledger + catalog-classified sensitive sinks + run policy. No string-level flow (ABI carries i64 scalars and handles). `R-3223`. |
| Budget | ceilings on tokens/cost/time/calls | **Implementable as designed** | Provider-independent accounting + cooperative cancellation. Default, not optional. `R-3216`. |
| Durability | every effect recorded; replay | **Implementable as designed** | Append-only journal, digests not payloads, flush-before-return, idempotency keys, replay without re-execution. `R-3217`. |
| Approval | `approve(run, action)` | **Implementable as designed** | Registered approver, default deny when none is attached, decisions journaled so replay never re-asks. `R-3217`. |
| Tracing | OTel GenAI spans | **Implementable by mapping** | `std.api.trace` exists; the work is span naming/attributes for `invoke_agent`, `chat`, `execute_tool`, `plan`. `R-3217`. |
| MCP | client and server | **Implementable over HTTP; stdio refused** | Client and server over the existing HTTP stack; stdio requires subprocess support the language does not have. `R-3218`. |
| A2A / ACP | server adapters | **Implementable as adapters** | Built on run + journal + approval; no new primitives. `R-3219`. |
| Evals | `pass@1`, `pass^k`, graders | **Implementable as designed** | Deterministic graders in CI; judge grader opt-in; regression baseline. `R-3220`. |
| Adoption E0–E4 | surface → catalog → core → governance → interop | **Confirmed order** | Milestones M0–M5 keep the concept's ordering; E1 (single source) lands before any library surface. |
| Adoption E5 | `extern` native, syntax sugar | **Refused for this phase** | Non-goal; revisit only if M3–M5 reveal a repeated form. |

### 2.2 Adaptations required (and why)

Each row is a place where the concept, verified against the code, cannot hold as written.

1. **`ask_stream` does not return `Stream<string>`.** No host call today returns a stream;
   host handle types lower to `int`; `for` loops do not iterate streams
   (`compiler/src/semantic/semantic_statements.rs:324-336`), and stream consumption is
   `await next()` (`tests/validation/296_l4_stream_dyn_poll.spectra:18-20`).

   **Adaptation:** `ask_stream(run, prompt) -> Result<ChunkStream, Error>` where
   `ChunkStream` is a std.agent opaque handle, consumed with
   `stream_next(stream) -> Result<Option<string>, Error>` and
   `stream_close(stream) -> Result<bool, Error>`. Backed by the existing runtime stream
   facility (`HandleKind::AsyncStream`, `spectra.async.stream.*` hosts,
   `runtime/src/stdlib/registration.rs:895-909`) with new source-visible names. (D12)

2. **`embed` does not return `List<float>`.** No host call in any table returns `List<T>`;
   numeric vectors in this repo are tensors (`IRType::Tensor`) or handles.

   **Adaptation:** `embed(run, text) -> Result<Tensor, Error>`, a 1-D float tensor
   (repo numeric convention; tensor accessors already exist). `remember`/`recall` consume
   embeddings internally so the common path never touches the representation.

3. **`untrusted`/`trust` gain an explicit `run` parameter.** The concept writes
   `untrusted(value, origin) -> string`. Provenance must be attributable and journaled;
   the only thread-local in the runtime today is the tracing context stack
   (`runtime/src/tracing/mod.rs:197-200`), so the ledger is reached through the run.

   **Adaptation:** `untrusted(run, value, origin) -> string`,
   `trust(run, value, reason) -> string`. Semantics unchanged: returns the same string,
   records provenance/declassification in the run ledger.

4. **Compensation is declared and journaled, executed on explicit `rollback`.** The
   concept says "compensation is one declared line, and the runtime registers that the
   author said how". Automatic execution on the fatal capability-denial path would
   require re-entering compiled code from the runtime's fatal path; in this phase the
   tool-dispatch callback is invoked by the run loop, not by the panic path.

   **Adaptation:** `compensate(run, tool, arguments_json)` journals a pending
   compensation (LIFO); `rollback(run, reason)` executes them through the governed
   dispatch; `agent_end` reports `compensations_pending`, so a silent leak is visible.
   (D11)

5. **No async `main`.** The AOT executable shim calls `main` directly
   (`backend/src/aot.rs:397-404`, `:741-834`); every checked-in entry point is sync
   (`examples/api/00_hello_http.spectra:9`). Adding async-main would be a language
   change beyond this phase's refusals.

   **Adaptation:** entry points stay `public func main() returns int`; agent programs
   drive async work with `block_on(...)`, the established repo pattern. Examples and
   fixtures follow it.

6. **The local provider bridges to `generate_ex`, not `generate`.** `generate` is not
   reachable from Spectra source (no semantic/lowering entry; runtime + catalog only).
   `generate_ex` carries temperature/top_k/seed (compiler surface
   `builtin_ml_async.rs:393-404`).

7. **Illustrative error codes in the concept are not real.** The concept's JSON sample
   (`E0311`) and the draft roadmap's `E0461`/`E0462` do not exist. Real codes are
   `E003`/`E004` for type/return mismatches; phase-scoped families use the phase number
   (`E2101-E2120` = Phase 21). Phase 32 therefore allocates **`E3201-E3209`** (section 7.3).

8. **`spectralang explain --json <code>` vs the existing `fmt --explain` flag.**
   `--explain[=text|json]` exists as a `fmt` flag. The new capability is a top-level
   `explain` subcommand; the flag is untouched. The command must not be confused with
   `spectralang api doc` (package docs generation).

9. **Diagnostic JSON currently has no `expected`/`actual`/`fix`.** `JsonDiagnostic` is
   `{severity, code, message, phase, hint, range, related}`
   (`tools/spectra-cli/src/cli_diagnostics.rs:395-403`). `R-3204` adds the three fields
   as an additive schema change; absence must not alter existing output for other codes.

10. **Unknown function attributes are currently ignored silently.** Only `derive`/`json`
    on records/enums are validated by the compiler; `#[spectra_async_test]` is a CLI-only
    convention (`tools/spectra-cli/src/cli_package.rs:183`). `R-3210` adds validation for
    `#[agent_tool]` only; the lenient treatment of other function attributes is left
    unchanged (documented), so existing sources keep compiling.

Adaptations recorded while implementing (they refine the surface, not the guarantees):

11. **Records cross the host ABI as JSON documents.** The host-call ABI carries scalars,
    handles and pointers; there is no record-value channel. `AgentSpec` and `Report`
    therefore remain compiler-declared records for typed authoring, and the ABI form is
    JSON: `agent_start(spec: AgentSpec)` is written with the existing derive as
    `agent_start(spec.to_json())`, and `agent_end(run)` returns the report JSON, decoded
    with `Report::from_json(...)`. Zero new ABI; the typed surface is preserved.
12. **The provider transport is injected, not imported.** `spectra-api` depends on
    `spectra-agent` (D1 aggregation), so `spectra-agent` cannot depend on the API client.
    `spectra-agent` defines an `HttpTransport` trait installed at registration time;
    `spectra-api` supplies the real implementation over its client (TLS, pooling, SSRF
    unchanged), and tests install a mock transport. The dependency DAG stays acyclic.
13. **`stream_next` encodes end-of-stream as an empty chunk.** Nested
    `Result<Option<string>, Error>` is not a host-return shape; model chunks are never
    empty, so `stream_next(stream) -> Result<string, Error>` returns `""` at the end and
    an `Err` on failure. Documented in section 4.2 and the bindings.
14. **The `std.agent` midend table is hand-written until the generator absorbs new
    namespaces.** R-3207's generator owns the seven legacy tables; the new namespace's
    `midend/src/lowering_std_agent.rs` follows the same descriptor shape and is a
    three-edit path for each new function. Absorbing it into the generator's LAYOUT is a
    recorded follow-up, not a correctness gap (all `--check` gates stay green).
15. **Memory persistence has no path-bearing surface entry.** The M3 surface table
    pins `remember(run, text)` and `recall(run, query, top_k)`, so there is no
    argument through which a caller could name an artifact path, and an implicit
    write inside the host call would be an ungoverned filesystem effect. The item
    therefore lands the persistence *format and API* at the module level:
    `packages/spectra-agent/src/memory.rs` stores a `VectorIndex` (public seam
    `spectra_runtime::vector_index::{write_artifact, read_artifact}`) whose
    provenance ledger travels in the artifact metadata, and the spectra-agent
    regression test persists, drops the writer and loads again — the restart path.
    Stores are scoped by the run goal so a restarted agent with the same goal
    recalls earlier memory across runs, which the checked-in fixture proves.
    Wiring an explicit `persist`/`load` surface waits for R-3217, whose journal is
    the phase's path-bearing artifact.
16. **Tool registration is per module, not a static table passed to `agent_start`.**
    A relocated data blob is not expressible in the IR, so `R-3222` lowers each module
    that declares tools into an idempotent `__spectra_agent_register_tools_<module>()`
    synthesized function (one `register_tool(name, wrapper_addr, description,
    input_schema, effects_json)` host call per tool), called from every function that
    dispatches and from the registration of each imported module that declares tools.
    The wrapper ABI is frozen in ADR 0019:
    `extern "C" fn __spectra_agent_tool_<name>(run, args_json_ptr, out_slot_ptr) -> i64`,
    invoked by address through the same callback shape the coroutine machinery proves in
    JIT and AOT. The wrapper receives the raw JSON argument document so it can reuse the
    derived `from_json` lowering, and async tools are driven through the task ramp inside
    the wrapper.
17. **`tool_call(run, name, arguments_json)` is part of the surface.** `R-3222` added the
    dispatcher primitive so the wrapper ABI is independently testable and the MCP/A2A
    adapters (`R-3218`/`R-3219`) reuse one governed path. It charges the tool-call budget
    before invoking and returns the tool's JSON result or a typed error. The count of
    free functions is 20 with it.

### 2.3 Verified substrate (exists today)

Verified by direct code inspection (`docs/agent-platform-plan.md` authoring pass; line
references current at the time of writing).

| Piece | Where | Why it matters here |
|---|---|---|
| Every external effect is a named host call | `runtime/src/ffi_host_registry.rs:226` (register), `runtime/src/ffi_lifecycle.rs:280` (dispatch) | Single enforcement point |
| No FFI / `extern` / subprocess in the language | `compiler/src/token.rs:5` (no keyword) | No escape hatch around enforcement |
| Four generic dispatch entrypoints | `ffi_lifecycle.rs:280` (single), `:311` (cached single), `:344` (batch), `:387` (cached batch); cache-hit path never locks the registry (`ffi_host_registry.rs:78-101`) | All four must route through one policy decision (D3) |
| Host status codes | `ffi_core.rs:320-323` (0 success, 1 invalid, 2 not found, 3 internal); call context has 5 fields (`:327-340`); values are i64 (`:316-317`) | Denial needs a new status + a fatal symbol (D4) |
| Fast host calls | `abi.rs:439-501` (`FastHostCall::ALL`, 28 entries, COUNT `:470`); resolve at `:624-629`; codegen picks at `codegen_instruction_host.rs:24` | Fast path is in-process compute; excluded from policy, pinned by test (D2) |
| Failure is fatal today | `codegen_instruction_host.rs:629-635`, `:652-657` → `panic.rs:63-68` (exit 101) | Denial must reuse this channel for the fatal path |
| IR carries the host-call name | `midend/src/ir.rs:276-281` (`InstructionKind::HostCall`) | Effects and capabilities are derivable |
| Reverse calls into compiled code work today | `CoroutineCreate` passes poll/drop function addresses as i64 host args (`codegen_instruction_async.rs:109-121` → `async_abi.rs:130-149`); JIT materializes addresses via `finalized_function_ptrs` (`codegen_instruction_indirect.rs:21-35`), AOT via `func_addr` relocations (`aot.rs:500`, `:561`); closures carry a code pointer in slot 0 (`ffi_lifecycle.rs:467`) | Makes the tool-dispatch design (D9) feasible in JIT and AOT |
| Whole-function synthesis in the midend | lambdas `lowering_impl_module.rs:570-574`, coroutines `:566-568`, monomorphization; `Module::add_function` `ir.rs:601`; `Builder::build_call` `builder.rs:289` | The marshalling wrappers and dispatch glue are synthesizable |
| JSON derive already produces schema data | `lowering_json_derive.rs:12-20` (`JsonFieldSchema`), `:519-553` (`derive_schema_string`) | `Cobranca::json_schema()` is a reformat, not a re-derivation |
| Async, Task, Stream, reactor, cancellation, worker pool | `midend/src/lowering_async.rs:14` (coroutine state machine), `runtime/src/stdlib/async_task_stream.rs:57` (resume), `registration.rs:865-960` | An agent loop is an ordinary async program |
| Runtime stream facility | `HandleKind::AsyncStream = 24`; hosts `registration.rs:895-909`; names `spectra.async.stream.*` | `ask_stream` reuses it with new source-visible names |
| Structured errors as values | `runtime/src/stdlib/error.rs:20-25` (6-word layout), `:48-77` (`alloc_error`), `fs.rs:49-61`/`:77-110` (tagged Ok/Err) | Typed errors for every new function |
| Tracing context stack | `runtime/src/tracing/mod.rs:197-200`, propagation helper `:492-508` | Precedent for the run context thread-local (D5) |
| 109 handle kinds, generational | `runtime/src/handles/mod.rs:13-123`; slot table `:329-455` | New kinds: agent run, chunk stream, tool registry |
| `std.ml` embeddings/vector index/RAG | `runtime/src/stdlib/registration.rs:690-838` (vector index `:804-813`); compiler surface `builtin_ml_async.rs` | Memory is a thin layer |
| HTTP client with TLS/pool/SSRF | `packages/spectra-api/src/client_core.rs:23-33` (`ClientConfig`), `:441-520` | Provider transport needs no new stack |
| `std.api.trace` | `compiler/src/semantic/builtin_api_services.rs:819-874`; fixture `tests/validation/193_opentelemetry_tracing.spectra` | GenAI spans map onto it |
| Contract catalog (1233 entries) | `packages/spectra-contract/catalog/stdlib.toml`; schema `contract/src/lib.rs:18-32`; integrity tests `:46-84` | Half the introspection already exists as data |
| Contract governance | `scripts/stdlib_contract.toml` (probes `:51-193`, namespaces `:277-434`, rules `:435-542`); `scripts/validate_r3007_stdlib_contract.py:248-268`, `:353-497`, `:572-716` | New namespace must enter the same governance |
| Anti-drift tests | `packages/spectra-api/tests/contract_drift.rs` (6 tests) | The four-copy problem is contained, not eliminated; M1 eliminates it |
| CLI conventions | exit codes `lib.rs:77-82`, routed `:276-285`; JSON diagnostics `cli_diagnostics.rs:395-403`; command dispatch `cli_parse_core.rs` | `surface`/`impact`/`explain`/`docs` follow the `release-info` pattern |
| Single registration pair | `packages/spectra-api/src/api_registration.rs:13-16` (`spectra_api_register_host_calls`), `:18-21` (`spectra_api_host_call_count`) | `spectra.agent` aggregates here; no linker change (D1) |
| Async tests | `#[spectra_async_test]` via `spectralang package test` (`cli_package.rs:183-197`, `:234-239`) | Behavior tests for agent programs have a home |
| Fixture conventions | `tests/validation/NNN_*.spectra` (highest 363), `tests/errors/` descriptive names, `tests/projects/valid/*` | New fixtures follow the same convention |
| Validator registration | `run_tests.ps1` (125 `Invoke-HostCommand` slots; `$binary` at `:20`, helper `:1002`, registration blocks `:1074-2809`) | Every item's validator registers here |

### 2.4 Verified gaps (must be closed by this phase)

| Gap | Evidence | Consequence |
|---|---|---|
| No capability model anywhere | zero matches for capability/permission enforcement in compiler/runtime/CLI | Greenfield enforcement (D2–D8) |
| Function attributes have no validation path | `semantic_module_analysis.rs:70` (`Item::Function` arm) never reads `func.attributes` | `#[agent_tool]` validation is new compiler work (R-3210) |
| No project-level view reachable after compilation | `pipeline.rs:134` (private registry), `CompilationResult` `:117-121` | R-3202 adds an accessor + snapshot |
| Surface exists in four hand-maintained copies | lowering tables (`lowering_std_host*.rs`), compiler tables (`builtin_*.rs`), runtime registration (`registration.rs`), catalog | M1 generates two of them from the catalog, with diff-empty gates |
| No host call returns a stream | verified: no `Stream<T>` host return exists | `ask_stream` adaptation (2.2.1) |
| No subprocess | absent from surface and catalog | MCP over stdio refused; HTTP is the transport (R-3218) |
| TCP/UDP/stream hosts are dead code | names `spectra.async.*`; frontend declares no `std.async`; midend gate requires `std.*` | `ask_stream` gives them their first source-visible consumer |
| No ambient run identity | only thread-local is tracing (`tracing/mod.rs:197-200`) | Run context must be built explicitly (R-3213) |
| `?` operator has no fixtures | verified: zero `.spectra` fixtures use `?` | Agent fixtures add its first regression coverage |
| `generate` unreachable; `text_embed_model` catalog/compiler drift | `builtin_ml_async.rs:393-404`, `:426-430` vs `catalog/stdlib.toml:11375` | The gateway uses compiler-declared signatures only; drift recorded as pre-existing risk |

### 2.5 What Phase 32 refuses

Copied forward from the concept and enforced by this plan:

- Effect rows in the type system; effect annotations of any kind.
- `extern` native declarations in packages; macros; a general attribute system.
- Subprocess execution; MCP over stdio.
- A separate "agent language mode"; model-provider syntax in the language.
- Auto-execution of compensations from the fatal panic path (D11).
- Full information-flow tracking for strings and scalars (D10).
- Any second attribute (a second attribute requires revising ADR 0017 first).
- Async `main` (entry points stay sync; `block_on` drives async work).

---

## 3. Architecture

### 3.1 Three layers and the dependency rule

```text
┌─ A. SURFACE (derived, static) ───────────────────────────────────────┐
│  spectralang surface --json · impact --json · explain --json · docs  │
│  Source: types + SIR + contract catalog         Consumer: coding agent│
└──────────────────────────────────────────────────────────────────────┘
                                 │ informs
┌─ B. GOVERNANCE (enforced, runtime) ──────────────────────────────────┐
│  capabilities · taint · approval · budget · durability · trace       │
│  Single point: generic host-call dispatch   Consumer: the run        │
└──────────────────────────────────────────────────────────────────────┘
                                 │ authorizes
┌─ C. EXECUTION (std.agent) ───────────────────────────────────────────┐
│  20 free functions + json_schema; tools are ordinary functions        │
│  Consumer: Spectra code written by humans or agents                  │
└──────────────────────────────────────────────────────────────────────┘
```

Dependency rule: **A informs B, B authorizes C, C never decides by itself.** A layer-C
function that needs an effect asks layer B; if B has not granted the capability, the
effect fails with a structured error — never an opaque exception.

### 3.2 Design decisions

| ID | Decision | Rationale | Consequence |
|---|---|---|---|
| D1 | `spectra.agent` is an rlib crate `packages/spectra-agent`, depended on by `packages/spectra-api`, registered by the existing `spectra_api::register()`. | Each staticlib embeds `spectra-runtime`; linking two duplicates Rust symbols (`linker.rs:286-288`), and the AOT shim imports `spectra_api_register_host_calls` (`aot.rs:764-775`). | No linker/runtime_lib/aot/CLI-lib-discovery changes. |
| D2 | Capability enforcement applies to the generic dispatch path only. | The 28 fast host calls are in-process compute with no external effect and several return raw values, so they cannot report denial. | Completeness test fails if any fast call falls under an effect-bearing namespace (I1). |
| D3 | Policy is evaluated by one extracted function `dispatch_generic(name, func_ptr, args, arg_len, results, result_len) -> i32`, called from all four generic entrypoints, including the cached and batch paths. | The cached path never consults the registry (`ffi_host_registry.rs:78-101`); `invoke_host_function` receives only a pointer (`:162`). | Policy runs on cache hit and per batch item; one decision, four call sites. |
| D4 | Denial is `HOST_STATUS_DENIED` on the returned status plus a dedicated fatal runtime symbol for the lowering's existing panic path, and `authorize(run, host)` for callers that want a pre-check. | Lowering turns any non-zero status into a fatal trap (`codegen_instruction_host.rs:629-657`); there is no typed error channel on that path. | Two paths, one enforcement decision, no silent partial handling. |
| D5 | The active run is a stacking thread-local mirroring the tracing context, with explicit propagation to workers and a fail-closed default for detached work. | No ambient run identity exists; the tracing pattern is proven (`tracing/mod.rs:197-200`, `:492-508`, used at `async_task_stream.rs:314-328`). | No ABI change to the repr(C) context; detached work inside a run needs its own grant. |
| D6 | Exactly one new attribute: `#[agent_tool("description")]`. Name, input schema, effects and required capabilities are derived. | Function attributes have no validation path today; the first one is expensive and must be the last. | Tool name uniqueness is validated project-wide; description is the only authored string. |
| D7 | `surface`, `impact`, `explain` and `docs` are CLI subcommands with JSON output and the existing exit-code contract. | The midend/backend do not depend on serde; the registry is private to the pipeline. | The compiler gains one accessor and one serializable snapshot; serde stays in the CLI. |
| D8 | The contract catalog is the single source for generated midend lowering tables and the Rust host-call table, via a three-step extract → regenerate → diff-empty migration, then deletion of the manual copies. | Four hand-maintained copies of one surface is the repo's dominant drift hazard. | Generated files carry a do-not-edit header and a `--check` staleness mode wired into tests. |
| D9 | **Tool dispatch:** the `act` loop lives in the runtime; the compiler/midend synthesizes (a) one marshalling wrapper per tool in the tool's own module — decode JSON args via the derived `from_json`, call the tool, encode the result via `to_json` — and (b) a static tool table (names + wrapper addresses) passed to the runtime as hidden lowering-provided arguments of `agent_start`. The runtime invokes wrappers by address through the proven i64 callback ABI. | Pure-IR loops would require JSON parsing and name matching in IR; the callback ABI is already exercised by `CoroutineCreate` (poll/drop addresses) in JIT and AOT. Runtime-side orchestration keeps the loop, budget, journal and transcript in one place, and the same wrapper invocation serves `act`, `rollback` and the MCP server. | A synthesized dispatcher per project; wrapper ABI frozen in ADR 0019 and proven by an M0/M3 spike before the loop is built. Cross-module wrappers rely on the same relocations as cross-module calls (JIT shared module, AOT object linking). |
| D10 | **Taint:** provenance is recorded per message and per digest in a run ledger; tool results and external content default to untrusted; `untrusted`/`trust` record ledger entries; host calls classified as `sink` in the catalog are gated while the run holds untrusted content, per run policy (`block` \| `approve` \| `allow`; default `approve`). | Strings cannot carry taint through the i64 ABI; pretending otherwise would be a false promise. Message-level provenance is the honest granularity. | Documented limits; gating is journaled; declassification is explicit and audited. |
| D11 | **Compensation:** `compensate(run, tool, arguments_json)` journals a pending compensation (LIFO); `rollback(run, reason)` executes pending compensations through the governed dispatch (capabilities and taint still apply) and is replay-safe; `agent_end` reports `compensations_pending`. | The runtime cannot re-enter compiled tool code from the fatal panic path in this phase; automatic rollback from failure paths would overpromise. | The author decides when to compensate; a silent leak is still visible in the report. |
| D12 | **`ask_stream`:** returns an opaque `ChunkStream` handle; consumed with `stream_next` (awaitable, `Option<string>`; empty = end) and `stream_close`. | No host returns a stream; handles lower to int; loops do not iterate streams. | Mirrors the std.api streaming idiom; backed by the existing runtime stream facility. |
| D13 | Entry points stay synchronous; `block_on` drives async work in `main`. | The AOT shim calls `main` directly; async main is a language change outside this phase. | Concept examples are adjusted; no compiler/backend entry-point change. |
| D14 | Capability vocabulary = host-call namespace prefixes (`std.api.client.request`, scoped `:host=…`), validated at compile time with did-you-mean. | The catalog already knows every host call; a grant that matches nothing must fail the build. | New codes `E3201`/`E3202`; scoped forms only where the host call registers an extractor. |
| D15 | Durability: append-only journal records `{run, step, kind, input digest, output digest, idempotency key, seed, usage, timestamp}`; digests only by default; flush before returning from any effect-bearing call; replay returns recorded outputs for matching keys and appends for missing ones. | Crash resume and reproducible behavior are the operable properties; storing payloads by default would leak. | `R-3217` owns the format; ADR 0018 freezes it. |

### 3.3 Invariants (each pinned by a test)

| ID | Invariant | Pinned by |
|---|---|---|
| I1 | No host call that touches the outside world is in the fast path. | Completeness test walking `FastHostCall::ALL` against effect namespaces (`R-3201`). |
| I2 | Every effect executed inside a run was authorized by that run's capability set. | Negative fixtures across all four dispatch entrypoints, including cached and batch (`R-3214`). |
| I3 | No effect is executed twice across a journal replay. | Interrupt-and-resume fixture with a counting test server (`R-3217`). |
| I4 | Adding a native function does not require editing the same surface in four places. | Generator `--check` modes + contract drift tests (`R-3206`–`R-3208`). |
| I5 | A program without an active run behaves exactly as before this workstream. | Existing validation suite unchanged + dedicated regression fixture (`R-3214`). |
| I6 | Tool metadata exposed to a model is derived, never hand-declared, except the description string. | `surface --json` cross-checked against the compiler view (`R-3210`). |
| I7 | Tool calls made during `act`, `rollback` or the MCP server cannot bypass the governed dispatch. | Injection fixtures through each path; denial fixtures (`R-3222`, `R-3224`, `R-3218`). |
| I8 | Compensation never executes twice (including after replay) and never on the fatal path implicitly. | Counting-server fixture + replay fixture (`R-3224`). |
| I9 | Sensitive sinks are not reached silently while untrusted content is in context. | Hostile-description fixture; policy matrix (block/approve/allow) (`R-3223`). |
| I10 | Every budget ceiling is enforced or the run is cancelled; accounting matches provider usage exactly. | Mock-provider accounting fixture; cancellation fixture (`R-3216`). |

---

## 4. Surface specification

### 4.1 Records

```spectra
record AgentSpec {
    goal: string,             // what this run is for; cited by tests, evals, journal and denial messages
    model: string,            // provider-scoped model name
    endpoint: string,         // provider base URL; empty selects the provider default from the environment
    allow: List<string>,      // capability grants; empty means default deny inside the run
    max_tokens: int,          // 0 = provider default; otherwise a hard ceiling
    max_cost_micros: int,     // 0 = no cost ceiling; otherwise a hard ceiling
    max_seconds: int,         // wall-clock ceiling; 0 = unlimited
    max_tool_calls: int,      // 0 = unlimited; otherwise a hard ceiling
    untrusted: string,        // "approve" (default) | "block" | "allow" — taint policy (D10)
    seed: int,                // -1 = provider default; >=0 requests deterministic sampling
    journal: string,          // journal directory; empty = ".spectra/journal"
}

record Report {
    status: string,           // "completed" | "budget_exceeded" | "failed" | "rolled_back"
    steps: int,
    tool_calls: int,
    tokens_in: int,
    tokens_out: int,
    cost_micros: int,
    elapsed_ms: int,
    compensations_pending: int,
}
```

`Run` and `ChunkStream` are opaque handle types owned by `std.agent`.
A convenience constructor `agent_spec(goal, model)` supplies documented defaults for the
remaining fields; the raw record stays available for full control. Additive either way.

### 4.2 Functions

| Function | Signature | Behavior |
|---|---|---|
| `agent_start` | `(spec: AgentSpec) returns Result<Run, Error>` | Validates the spec, validates every capability against the catalog, checks that the project's tools are known, allocates the run, opens the journal, registers the tool table (hidden lowering-provided arguments). The ABI form is `agent_start(spec.to_json())` (adaptation 11). |
| `agent_end` | `(run: Run) returns Result<string, Error>` | Closes the run, frees the handle, returns the report as a JSON document (`Report::from_json` decodes it; adaptation 11). Double end / use-after-end return typed errors. |
| `ask` | `(run: Run, prompt: string) returns Result<string, Error>` | One model turn under budget and journal. |
| `ask_stream` | `(run: Run, prompt: string) returns Result<ChunkStream, Error>` | Same turn delivered in chunks. |
| `stream_next` | `(stream: ChunkStream) returns Result<string, Error>` | Awaitable; the empty chunk marks the end (adaptation 13). |
| `stream_close` | `(stream: ChunkStream) returns Result<bool, Error>` | Releases the stream; idempotent. |
| `ask_json` | `(run: Run, prompt: string, schema: string) returns Result<string, Error>` | Schema-constrained response, validated client-side regardless of provider-side decoding. |
| `act` | `(run: Run, prompt: string) returns Result<string, Error>` | Tool loop: model turn → tool call → journaled execution through the governed dispatch → repeat until a final answer or a ceiling. |
| `tool_call` | `(run: Run, name: string, arguments_json: string) returns Result<string, Error>` | Governed dispatcher primitive `act` is built on: charges the tool-call budget, invokes the registered wrapper by address, and returns the tool's JSON result or a typed error. Added by `R-3222` so the dispatch ABI is independently testable and reusable by the MCP/A2A adapters. |
| `embed` | `(run: Run, text: string) returns Result<Tensor, Error>` | 1-D float embedding tensor. |
| `remember` | `(run: Run, text: string) returns Result<bool, Error>` | Appends to run memory with origin, run id, timestamp and goal. |
| `recall` | `(run: Run, query: string, top_k: int) returns Result<string, Error>` | Deterministic retrieval (score ties broken by insertion order); payload capped by tokens. |
| `approve` | `(run: Run, action: string) returns Result<bool, Error>` | Asks the registered approver; default approver denies when none is attached. Decisions are journaled. |
| `require` | `(run: Run, condition: bool, message: string) returns Result<bool, Error>` | Governed assertion: `true` passes; `false` returns a typed error carrying the message and the run goal, and marks the run failed. |
| `budget_remaining` | `(run: Run) returns Result<int, Error>` | Tokens remaining before the run's `max_tokens` ceiling (`i64::MAX` when unlimited); introspection never fails because a run was cancelled. Added by `R-3216` per its task text; the authored surface stays otherwise as specified. |
| `untrusted` | `(run: Run, value: string, origin: string) returns string` | Records the value's digest as untrusted in the run ledger; returns the value unchanged. |
| `trust` | `(run: Run, value: string, reason: string) returns string` | Records an audited declassification for the value's digest; returns the value unchanged. |
| `compensate` | `(run: Run, tool: string, arguments_json: string) returns Result<bool, Error>` | Journals a pending compensation (LIFO). Unknown tool names are compile errors when literal (`E3205`). |
| `rollback` | `(run: Run, reason: string) returns Result<int, Error>` | Executes pending compensations in LIFO through the governed dispatch; replay-safe; returns the number executed. |
| `token_count` | `(text: string) returns int` | Token count over the existing tokenizer path (no second tokenizer). |
| `T::json_schema` | `() returns string` | Associated function added to the existing JSON derive, emitting the JSON Schema from the same field data that produces `to_json`/`from_json`. |

### 4.3 The tool form

```spectra
#[agent_tool("Cria uma cobrança no provedor de pagamentos")]
public async func cobrar(run: Run, cobranca: Cobranca) returns Result<Recibo, Error> { ... }
```

| Derived | From |
|---|---|
| tool name `cobrar` | function name (project-wide unique; duplicates rejected) |
| `inputSchema` | the parameter that is not the run, through the existing JSON derive |
| effects | host calls reachable in the body (IR) |
| required capabilities | from the host-call names, by the D14 vocabulary |
| model-invocability | the `run` parameter in first position |

A project with `#[agent_tool]` functions gets, synthesized at compile time: one
marshalling wrapper per tool (in the tool's own module) and one static tool table. The
table travels to the runtime as hidden arguments of `agent_start` (D9); nothing in user
code refers to it.

### 4.4 Deliberately absent

- `Agent` as a type; `#[derive(Agent)]`; `#[derive(Tool)]`; `#[effects]`; `#[requires]`;
  `#[intent]`; `journal_step`; `inspect::*` (layer A instead); `rollback` on the panic path.
- Vendor model syntax (`llm claude "..."`) — a provider is data in `AgentSpec`.
- Automatic compensation inference; automatic declassification.
- A second attribute. If a second one is ever needed, ADR 0017 must be revised first.

---

## 5. Milestones

Each milestone has entry criteria, items, and exit criteria that must all pass before the
next milestone's items start. Items inside a milestone parallelize where their
dependencies allow.

### M0 — Decisions and surface

Goal: freeze the architecture, land the fast-path invariant, and ship the developer-loop
surface (the highest-value, lowest-cost part of the concept).

Items: `R-3201` (ADRs 0016–0019 + invariant), `R-3202` (`surface --json`),
`R-3203` (`impact --json`), `R-3204` (diagnostic repair fields + `explain --json`),
`R-3205` (`docs --json`).

Entry: `phase_32` registered in the tracker. No code dependency on this phase.

Exit:
- Four ADRs exist, are Accepted, and record rationale plus consequence.
- The fast-path invariant test fails if a fast host call is reclassified into an
  effect-bearing namespace.
- `surface --json`, `impact --json`, `explain --json` and `docs --json` produce stable,
  byte-identical output across runs on a real project
  (`tests/projects/valid/phase21_async_pipeline` and the repository itself).
- `check --json` emits `expected`, `actual` and `fix` for at least the five highest-value
  codes; SARIF remains schema-valid with and without fixes.
- All five validators are registered in `run_tests.ps1`.

### M1 — Single source of truth

Goal: the catalog generates the midend lowering tables and the Rust host-call table;
the manual copies are deleted with a diff-empty gate at each step.

Items: `R-3206` (catalog schema), `R-3207` (lowering generation), `R-3208` (host-call
table generation).

Entry: M0 complete (ADR 0017 records the migration).

Exit:
- `python scripts/generate_lowering_tables.py --check` and
  `python scripts/generate_host_calls.py --check` pass on a clean tree and fail on a
  manual edit.
- The generated Rust tables carry a do-not-edit header.
- `contract_drift` passes; the full validation suite shows no fixture regression.
- The catalog still parses (1233 entries; new fields populated for `kind=function`).

### M2 — Namespace seam

Goal: `std.agent` exists with exactly one function (`token_count`) proven end to end
through compiler, midend, runtime, catalog, probes, JIT and AOT.

Items: `R-3209`.

Entry: M1 generators in place.

Exit:
- `import std.agent` resolves; `token_count` type-checks, lowers, and returns the same
  value under `spectralang run` and `spectralang compile --emit-exe`.
- The namespace has a probe in `scripts/stdlib_contract.toml`; `validate_r3007` passes
  with `--require-catalog`.
- The cost of adding the next function is measured and documented: the three-edit path.

### M3 — Core library

Goal: an agent can talk to a model, stream chunks, run the project's tools, and remember
context.

Items: `R-3210` (attribute + schema), `R-3211` (gateway, run, ask family, embed),
`R-3222` (tool dispatch + `act`; dispatcher spike first), `R-3212` (memory).

Entry: M2 complete.

Exit:
- The dispatcher spike (R-3222-T1) proves tool invocation by address in JIT
  **and** AOT with two tools in two modules before the loop is built.
- A tool chain against the mock provider completes with at least two tool calls and a
  final answer; malformed tool arguments are repaired by feeding the typed error back to
  the model; unknown tool names never crash.
- `surface --json` reports each tool with derived name, schema, effects and capabilities.
- Memory survives persist/load across processes; recall is deterministic.

### M4 — Governance

Goal: every effect inside a run is bounded, recorded and replayable; untrusted content is
fenced; compensations are declarable.

Items: `R-3213` (run context), `R-3214` (enforcement), `R-3215` (vocabulary),
`R-3216` (budget), `R-3217` (journal, replay, approval, assertions, tracing),
`R-3223` (taint), `R-3224` (compensation).

Entry: M3 run exists.

Exit (each pins its invariant):
- I2: a host call outside the grant fails through all four generic entrypoints,
  including cached and batch; `authorize()` agrees with the dispatch decision.
- I3: killing the process mid-run and resuming completes the run with no duplicated
  effect, proven by a counting test server.
- I5: programs without a run behave exactly as before (regression fixture).
- I8: compensation executes exactly once, LIFO; replay does not duplicate it;
  `agent_end` surfaces pending compensations.
- I9: with untrusted content in context, sensitive sinks are gated by policy
  (block/approve/allow matrix); declassification is journaled.
- I10: no run exceeds a declared ceiling; the report names the ceiling; accounting
  matches the mock provider exactly.

### M5 — Interop, evaluation, release

Goal: the platform is usable from and by other systems, measured as regression, packaged,
and gated by conformance.

Items: `R-3218` (MCP), `R-3219` (A2A/ACP), `R-3220` (eval harness), `R-3221` (packaging,
conformance, integrated project, release gate).

Entry: M4 complete.

Exit:
- MCP: a Spectra agent calls a remote MCP tool through the governed path and the journal
  records it; a Spectra project exposes its tools over MCP and a third-party client can
  call them; stdio transport is documented as unsupported with a follow-up reference.
- A2A/ACP adapters delegate and observe completion; permission requests map to `approve`.
- `agent eval` runs a suite, reports `pass@1`/`pass^k` and cost, and fails the build on
  regression against the checked-in baseline; judge grading is opt-in.
- The package is publishable to the local registry and consumable by another project
  through the normal package flow.
- The integrated project (`tests/projects/valid/integrated_agent_service`) runs in JIT
  and AOT, survives interruption and resume, and stays inside its budget.
- The conformance report is required by the release gate; the phase cannot be marked
  complete while any capability-enforcement, journal-replay or integrated-project
  assertion fails.

---

## 6. Item catalog

Notes that apply to every item:

- I/O-bearing functions are declared `async` in the surface (awaited at call sites,
  e.g. `await ask(run, prompt)?`); validation/counter functions are sync. Signatures in
  section 4.2 name the eventual value type.
- Every item adds its validator `scripts/validate_r32NN_<slug>.py`, registered in
  `run_tests.ps1` with the standard `Invoke-HostCommand` block and `$binary` argument,
  writing its JSON report under `target/`.
- Every item touching the host-call surface extends `scripts/stdlib_contract.toml`
  (namespace entry, classification, probe) and keeps `validate_r3007` passing.
- Fixture numbers start at `364` (`tests/validation/`); error fixtures live in
  `tests/errors/` with descriptive names (no numeric prefixes).

### M0 — Decisions and surface

#### R-3201 — ADR and Invariants for Agent Governance

`runtime` · P0 · risk medium · deps: none · milestone M0

Fix the architecture before code: one dispatch hook, fast-path exclusion, a denial
channel, the run context, the tool-dispatch ABI, and catalog-driven generation.

Production intent: prevent the two expensive failure modes — a policy that can be
bypassed, and a surface that must be edited in four places.

Tasks:
- **T1 — ADR 0016: capability enforcement model** (`docs/adr/0016-agent-capability-enforcement.md`, create).
  Record D2, D3, D4, D5, D14 with the exact entrypoints (`ffi_lifecycle.rs:280` uncached
  single, `:311` cached single, `:344` uncached batch, `:387` cached batch) and the
  `dispatch_generic(name, func_ptr, args, arg_len, results, result_len) -> i32` signature.
  State the fast-path exclusion and its completeness test. Define the denial contract:
  `HOST_STATUS_DENIED`, the fatal runtime symbol, the structured message format
  (host-call name, run goal, the capability that would have allowed it, the allowed set;
  never secrets/payloads/headers), and `authorize(run, host)`. Define nested-run and
  detached-work semantics.
- **T2 — ADR 0017: agent surface ownership and catalog-driven generation** (`docs/adr/0017-agent-surface-generation.md`, create).
  Record D1 (rlib aggregation; one static archive; the existing registration pair
  `spectra_api_register_host_calls` / `spectra_api_host_call_count`), D7, D8 (three-step
  extract → regenerate → diff-empty → delete manual tables), the catalog fields to add
  (`params`, `returns`, `ir_return`, `returns_value`, `rust_symbol`, `cfg_feature`,
  `sink`, `scope_keys`), and that `std.agent` is a `std` namespace whose package is
  `spectra.agent` (mirroring ADR 0011 for `std.api`).
- **T3 — ADR 0018: run, journal and evaluation contract** (`docs/adr/0018-agent-run-contract.md`, create).
  Define `Run` as a handle (new `HandleKind` entries) with lifecycle, budget and journal
  identity; the journal record shape and replay algorithm; approval caching; assertion
  (`require`) semantics; taint ledger and sink gating (D10); compensation semantics (D11);
  span mapping to the OpenTelemetry GenAI conventions with a pinned version; what an eval
  may assert versus what a deterministic test must cover.
- **T4 — ADR 0019: tool dispatch ABI** (`docs/adr/0019-agent-tool-dispatch.md`, create).
  Define the synthesized marshalling-wrapper ABI (arguments, result hand-off, error
  channel), the static tool table format, how the table reaches the runtime (hidden
  lowering-provided `agent_start` arguments), JIT/AOT address materialization and
  lifetime, cross-module resolution rules, and the shielding of synthesized functions
  from inlining/DCE (`Function.suspension_barrier` precedent, `ir.rs:74`).
- **T5 — fast-path invariant test** (`runtime/src/abi.rs` near the completeness test;
  `runtime/src/ffi_tests.rs`). Classify every `FastHostCall` name against the effect
  namespace list; fail on any match; fail if a new variant appears without classification
  (exhaustive match).
- **T6 — allocate the diagnostic family** (`docs/diagnostics/error-code-reference.md`).
  Add the `E3201-E3209` family row (Phase 32 agent platform: capability vocabulary and
  tool declarations). Individual code rows are added by the items that emit them.

Acceptance:
- Four ADRs exist, are Accepted, and each records rationale plus consequence.
- The invariant test fails when a fast host call is temporarily reclassified into an
  effect namespace.
- The `E3201-E3209` family row is present.
- `scripts/validate_r3201_agent_platform_adr.py` checks the ADRs, the invariant test and
  the tracker registration; it passes and is registered in `run_tests.ps1`.

Validation: `python scripts/validate_r3201_agent_platform_adr.py`
Integration: `roadmap.toml` `phase_32` and the item records exist.

#### R-3202 — `spectralang surface --json`

`tooling` · P0 · risk medium · deps: `R-3201` · milestone M0

Emit the project and builtin public surface as machine-readable JSON with a token budget.

Production intent: a coding agent must answer "what does this project expose" in one call
instead of reading every file, and must be able to fit the answer into a context window.

Tasks:
- **T1 — expose the project module registry.** `compiler/src/pipeline.rs`: return the
  registry by `Arc` clone (the field at `:134` is private; `CompilationResult` `:117-121`
  carries no registry); add a public accessor without changing `analyze_module`
  signatures.
- **T2 — serializable surface snapshot.** `compiler/src/semantic/surface.rs`: create
  `SurfaceSnapshot`/`SurfaceModule`/`SurfaceFunction`/`SurfaceType` with
  `serde::Serialize`, mirroring the existing builtin snapshot shape
  (`builtin_contract.rs:16-20`, `:196-232`). Note: that snapshot currently covers only
  `std.api.*`, `std.time` and `std.range`; the new snapshot is the general form. Fields
  per function: path, signature, module, effects, capabilities (nullable here; filled by
  `R-3214`-T4), and `tools` derived from `#[agent_tool]` descriptors (added by `R-3210`).
- **T3 — CLI command.** `tools/spectra-cli/src/cli_surface.rs` (create), dispatch in
  `cli_parse_core.rs`, parsing in the style of `parse_release_info_invocation`
  (`cli_parse_options.rs:468-500`), module include and `CliAction`/`HelpTopic` wiring
  (`lib.rs:287-298`, `:168-227`), help list (`cli_help.rs:9-23`). Flags: `--json`,
  `--tokens N`, `--package NAME`, `--include-builtins`. Exit codes per `lib.rs:77-82`
  (0 success, 64 usage, 65 compile failure, 74 I/O).
- **T4 — token-budget determinism.** Trimming order: drop doc fields, then non-exported
  types, then private members; never drop a type referenced by an included function.
  Ordering stable (module path, then symbol path); report `trimmed` in the payload.

Acceptance:
- `surface --json` on `tests/projects/valid/phase21_async_pipeline` and on the repository
  itself lists every public module, function and type with stable ordering.
- Output is byte-identical across runs; `--tokens` trims deterministically and reports it.
- Unknown module or missing project root fails with exit 64/65 and a JSON diagnostic.
- `scripts/validate_r3202_surface_json.py` passes and is registered.

Validation: `python scripts/validate_r3202_surface_json.py`

#### R-3203 — `spectralang impact --json`

`tooling` · P1 · risk medium · deps: `R-3201` · milestone M0

Answer "what breaks if I change this symbol" from the call graph, not from text search.

Production intent: impact analysis is the single most valuable editing signal for a
coding agent, and text search systematically misses indirect callers.

Tasks:
- **T1 — promote the call graph.** Extract `collect_direct_calls`
  (`midend/src/passes/function_inlining.rs:58-72`, called at `:33`) into
  `midend/src/callgraph.rs` with `pub fn direct_calls(module: &Module) -> HashMap<String, HashSet<String>>`;
  the pass keeps calling the shared function.
- **T2 — reverse index.** `reverse_callers`, `field_readers`, `field_writers`; map field
  access and struct-literal construction to the functions that touch them; record dynamic
  dispatch in a separate bucket (never guess targets).
- **T3 — CLI command.** `tools/spectra-cli/src/cli_impact.rs` (create); symbol paths
  `Module::function` and `Type.field`; output `symbol`, `affected_functions`,
  `affected_types`, `affected_schemas`, `dynamic_dispatch`, `fixtures`, `eval_cases`;
  fixtures from the contract catalog, eval cases from the eval directory when present.
- **T4 — honesty about coverage.** Unresolved call sites (dynamic dispatch, imported
  generics) set `dynamic_dispatch: true` and are listed; an empty affected list is never
  reported as complete while `dynamic_dispatch` is true.

Acceptance:
- Full caller set for a changed function, including cross-module callers.
- Field-level impact returns readers and constructors.
- Unresolved dynamic sites are reported, not omitted.
- `scripts/validate_r3203_impact_json.py` passes and is registered.

Validation: `python scripts/validate_r3203_impact_json.py`

#### R-3204 — Diagnostics with Repair Information

`frontend` · P1 · risk medium · deps: none · milestone M0

Add `expected`, `actual` and `fix` to the JSON diagnostic contract and expose
`explain --json`.

Production intent: repair efficiency dominates one-shot generation quality in agent
workflows; a diagnostic that states the fix removes an investigation round trip.

Tasks:
- **T1 — extend the diagnostic data model.** `compiler/src/error.rs`: add optional
  `expected`/`actual`/`fix` to the error variants that own `context`/`hint`/`code`
  (`LexError`, `ParseError`, `SemanticError`) with builders defaulting to `None`.
  `tools/spectra-cli/src/cli_diagnostics.rs`: extend `JsonDiagnostic` (`:395-403`) and
  `span_error_to_json` (`:316-338`). Map `fix` into a SARIF fix object only when present;
  otherwise emit no fixes array (schema stays valid).
- **T2 — populate five high-value codes.** `E004` (type mismatch in expression/assignment),
  `E003` (function return mismatch), `E033` (unknown stdlib module — reuse the existing
  did-you-mean at `semantic_item_import.rs:85-99`), `E029` (missing user module), `E028`
  (self/circular import). `expected`/`actual` are textual types; `fix` is a concrete
  suggestion (`?`, `if let`, explicit conversion, corrected module path). The set grows by
  observed need; do not attempt every code.
- **T3 — `explain --json`.** `tools/spectra-cli/src/cli_explain.rs` (create): input an
  error code; output description, common causes, fix patterns, and the documented
  reference entry, sourced from `docs/diagnostics/error-code-reference.md` so CLI and docs
  cannot drift. Unknown code: exit 64 with near matches. The existing `fmt --explain` flag
  is untouched and documented as distinct.
- **T4 — documentation and drift guard.** Every code that emits repair information must be
  documented; a validator asserts set equality (documented == emitted-with-fix).

Acceptance:
- `check --json` on a type mismatch emits `expected`, `actual`, `fix`.
- `explain --json E004` returns the documented description; unknown codes exit 64.
- SARIF remains schema-valid with and without fixes.
- `scripts/validate_r3204_diagnostics_repair.py` passes and is registered.

Validation: `python scripts/validate_r3204_diagnostics_repair.py`

#### R-3205 — Version-Matched Language Reference from the CLI

`tooling` · P2 · risk low · deps: none · milestone M0

`spectralang docs --json` serves the reference of the installed binary, not of the
repository.

Production intent: training-data scarcity is the largest single handicap of a young
language; shipping the reference inside the binary removes the stale-knowledge failure
mode.

Tasks:
- **T1 — embed the reference at build time.** `tools/spectra-cli/build.rs` (exists; today
  icon + `/EXPORT` only): read `docs/AI-AGENT-REFERENCE.md`, emit version + sha256 +
  embedded text into `OUT_DIR` with `cargo:rerun-if-changed` (the
  `packages/spectra-contract/build.rs` pattern), included via `include!(concat!(env!("OUT_DIR"), ...))`.
- **T2 — structured sections.** `tools/spectra-cli/src/cli_docs.rs` (create): output
  `version`, `reference_sha256`, `sections` (heading + body), symbol index; `--section NAME`
  returns a strict subset. Changing the reference file changes the reported hash after
  rebuild.

Acceptance:
- `docs --json` reports the version and hash of the embedded reference.
- `--section` returns a strict subset of the full output.
- The hash changes when the reference file changes and the crate is rebuilt.
- `scripts/validate_r3205_embedded_docs.py` passes and is registered.

Validation: `python scripts/validate_r3205_embedded_docs.py`

### M1 — Single source of truth

#### R-3206 — Extended Contract Catalog Schema

`ecosystem` · P0 · risk high · deps: `R-3201` · milestone M1

Add the fields the catalog needs to generate the midend lowering tables, the Rust
host-call table, and the sink/scope classification consumed by governance.

Production intent: until this lands, every agent function costs eight to ten hand edits
across four files — the dominant risk of building the library at all.

Tasks:
- **T1 — extend the typed schema.** `packages/spectra-contract/src/lib.rs`
  (`CatalogEntry` `:18-32`): add `params` (name+type list), `returns`, `ir_return`,
  `returns_value`, `rust_symbol`, `cfg_feature`, `sink` (bool), `scope_keys`
  (list of supported scope predicate keys). Keep new fields optional in the struct for
  one release; extend the integrity tests (`:46-84`) to require them for `kind=function`
  once migration completes.
- **T2 — populate from current sources.** `scripts/generate_stdlib_catalog.py`:
  extract `ir_return`/`returns_value` from the current lowering tables
  (`scripts/validate_r3007_stdlib_contract.py:248-268` already inventories them),
  `rust_symbol` from `packages/spectra-api/src/host_calls.rs`, `cfg_feature` from the
  surrounding `#[cfg]` attributes; classify `sink`/`scope_keys` by namespace rules with an
  explicit override table for the exceptions.
- **T3 — auditor coverage.** `scripts/validate_r3007_stdlib_contract.py`: reject a
  function entry missing any required new field; reject an `ir_return` that does not match
  the module-level type grammar; reject an unknown `scope_key`.

Acceptance:
- Catalog parses with the extended schema; function entries carry the new fields with no
  semantic change.
- The auditor rejects entries missing `ir_return`, `returns_value` or `rust_symbol`.
- `scripts/validate_r3206_catalog_schema.py` passes and is registered.

Validation: `python scripts/validate_r3206_catalog_schema.py`

#### R-3207 — Generate Midend Lowering Tables from the Catalog

`midend` · P0 · risk high · deps: `R-3206` · milestone M1

Replace the hand-written lowering tables with checked-in generated files plus a staleness
check.

Production intent: removes one of the four copies; the remaining manual work is the
semantic signature and the Rust implementation — the two things a human must write anyway.

Tasks:
- **T1 — generator with idempotence proof.** `scripts/generate_lowering_tables.py` (create):
  step 1 generates from the current hand-written tables and must be byte-identical;
  step 2 switches the source to the catalog and must be byte-identical again. Generated
  files carry a header naming the generator and the `--check` command, forbidding manual
  edits.
- **T2 — switch the consumed tables.** Replace the hand-written match arms in
  `midend/src/lowering_std_api.rs` (keeping `std_api_host_call_target`),
  `lowering_std_host_math_io_error.rs`, `lowering_std_host_collections_string.rs`,
  `lowering_std_host_fs_env_result.rs`, `lowering_std_host_convert_time.rs`,
  `lowering_std_host_numeric.rs`, `lowering_std_host_tensor_ml.rs`. Keep the public entry
  points byte-identical (`lowering_std_host.rs:3-35` needs no change). Preserve the
  special-case construction for `Option`/`Result`/`Tensor` returns; the catalog carries
  the descriptor that selects it.
- **T3 — drift guard.** Add a seventh test to
  `packages/spectra-api/tests/contract_drift.rs` that shells out to the generator in
  `--check` mode and fails with a diff hint.

Acceptance:
- The seven lowering files are generated; a manual edit makes `--check` fail.
- All existing `contract_drift` tests pass unchanged; the full validation suite shows no
  fixture regression.
- `scripts/validate_r3207_lowering_generation.py` passes and is registered.

Validation: `python scripts/validate_r3207_lowering_generation.py`

#### R-3208 — Generate the Host-Call Table and Remove the Manual Count

`web` · P1 · risk medium · deps: `R-3206` · milestone M1

Generate the Rust host-call table from the catalog and derive its size instead of
hardcoding it.

Production intent: removes the second copy; the count assertions
(`packages/spectra-api/src/api_tests.rs:93-95`, literals 557/529) become computed,
eliminating a recurring manual edit that silently rots.

Tasks:
- **T1 — generator.** `scripts/generate_host_calls.py` (create): one `HostCallSpec` per
  catalog function entry with a `rust_symbol`, honoring `cfg_feature` with
  cfg-conditional entries, in catalog order, with a do-not-edit header and a `--check`
  mode.
- **T2 — switch the crate and fix the count.** Replace the table in
  `packages/spectra-api/src/host_calls.rs` with the generated include (keep `HostCallSpec`
  at `:2-5`); derive the expected count from `HOST_CALLS.len()` in `api_tests.rs:93-95`.

Acceptance:
- The table is generated; the count assertion is computed; no literal counts remain.
- `spectra_api_host_call_count()` matches `HOST_CALLS.len()` in both feature
  configurations.
- `scripts/validate_r2202_spectra_api_hostcalls.py` still passes.
- `scripts/validate_r3208_hostcall_generation.py` passes and is registered.

Validation: `python scripts/validate_r3208_hostcall_generation.py`

### M2 — Namespace seam

#### R-3209 — `std.agent` Namespace and `packages/spectra-agent` Crate

`runtime` · P0 · risk high · deps: `R-3207`, `R-3208` · milestone M2

Land the namespace and the crate with exactly one function, proving every layer end to end
before scaling.

Production intent: the cost of a native module lives in the seam between compiler,
midend, runtime and catalog. This item measures that cost once so the remaining functions
are mechanical.

Tasks:
- **T1 — rlib crate and aggregated registration.** `Cargo.toml` (add member),
  `packages/spectra-agent/Cargo.toml` (crate-type `rlib`, depends on `spectra-runtime`),
  `packages/spectra-agent/src/lib.rs` (`register() -> usize`),
  `packages/spectra-api/Cargo.toml` (dependency), `packages/spectra-api/src/api_registration.rs`
  (`register()` additionally calls `spectra_agent::register()`). No staticlib, no new C
  symbol, no linker change.
- **T2 — compiler namespace and typed module.**
  `compiler/src/semantic/builtin_std_core.rs` (`make_std_agent` in the shape of
  `make_std_math` at `:70-75`), registration in `builtin_modules.rs` and
  `semantic_init.rs` (`BUILTIN_MODULES` `:5`), snapshot table in `builtin_contract.rs`
  plus `compiler/tests/snapshots/`. First function: `token_count(text: string) returns int`.
- **T3 — midend path and host implementation.** Regenerate the catalog; add the
  generated `midend/src/lowering_std_agent.rs` table; extend the gate in
  `midend/src/lowering_std_host.rs:3-35` with the `[std, agent, function]` shape;
  implement `token_count` in `packages/spectra-agent/src/token.rs` over the existing
  tokenizer path (no second tokenizer).
- **T4 — contract governance.** `scripts/stdlib_contract.toml`: namespace entry with
  owner, classification and a probe; `tests/validation/364_agent_surface.spectra` calling
  `token_count` and asserting a deterministic value.
- **T5 — JIT and AOT execution.** Run the fixture through `spectralang run` and through
  `spectralang compile --emit-exe <out> <src>`; assert identical results.

Acceptance:
- `import std.agent` resolves; `token_count` type-checks, lowers, and returns the same
  value in JIT and AOT.
- Adding a second function afterwards requires the documented three-edit path (recorded
  with a before/after edit count).
- Catalog, snapshot and probe are consistent; `validate_r3007` passes with
  `--require-catalog`.
- `scripts/validate_r3209_agent_namespace.py` passes and is registered.

Validation: `python scripts/validate_r3209_agent_namespace.py`

### M3 — Core library

#### R-3210 — `agent_tool` Attribute and Derived JSON Schema

`semantic` · P0 · risk high · deps: `R-3209` · milestone M3

One function attribute turns a public async function into a model-callable tool, with
name, schema, effects and capabilities derived.

Production intent: a tool declaration a human writes once and a model consumes must not be
able to lie; everything derivable is derived, only the description is authored.

Tasks:
- **T1 — semantic validation point.** `compiler/src/semantic/semantic_agent.rs` (create):
  `validate_function_attributes` called from the `Item::Function` arm
  (`semantic_module_analysis.rs:70`; today function attributes are never validated).
  Accept exactly `agent_tool`; reject: non-public function, non-async function, generic
  function, `dyn` parameter, missing `run` parameter, `run` not first, non-literal
  description, wrong arity. Store descriptors in an analyzer map mirroring
  `json_struct_derives` (`semantic/mod.rs:539`). Diagnostics: `E3203` (declaration misuse,
  one hint per rejection condition) and `E3204` (parameter type not derivable/decodable).
  Project-wide tool names must be unique (duplicate = `E3203` with both source spans).
- **T2 — `json_schema` on the existing derive.** `compiler/src/semantic/semantic_json.rs`:
  register the associated function in `register_json_derived_methods` (`:244`).
  `midend/src/lowering_json_derive.rs`: emit the schema from the same `JsonFieldSchema`
  data and the existing `derive_schema_string` (`:519-553`) — no revalidation. Encoded
  limits must be rejected as tool parameters: exact-width ints/floats and array fields
  (decode-side restrictions), `optional` only on primitives, unit-only enums.
- **T3 — tool descriptor exposure.** `builtin_contract.rs` snapshot includes tools;
  `cli_surface.rs` emits a `tools` array with `name`, `description`, `input_schema`,
  `effects`, `capabilities`, `module` (effects/capabilities empty here; `R-3214`-T4 fills
  them from the IR).
- **T4 — formatter and LSP integration.** `fmt --check` stays idempotent on attributed
  functions; the LSP does not flag `agent_tool` as unknown and offers it on completion.

Acceptance:
- A public async function with the attribute appears in `surface --json` with derived
  name, `input_schema`, effects and capabilities.
- Every rejection case produces its code and a hint; fixtures exist under `tests/errors/`.
- `fmt` is idempotent on attributed functions; the LSP reports no false diagnostics.
- `scripts/validate_r3210_agent_tool.py` passes and is registered.

Validation: `python scripts/validate_r3210_agent_tool.py`

#### R-3211 — Model Gateway, Run and Provider Abstraction

`web` · P0 · risk high · deps: `R-3209` · milestone M3

`AgentSpec`, `Run`, `ask`, `ask_stream`, `ask_json`, `embed`, `agent_start` and `agent_end`
over the existing HTTP client and `std.ml`.

Production intent: the only thing a real agent needs from a language is a governed way to
talk to a model and a place to record what happened; everything else is composition.

Tasks:
- **T1 — run handle and lifecycle.** `runtime/src/handles/mod.rs`: new `HandleKind`
  variants (agent run, chunk stream, tool registry). `packages/spectra-agent/src/run.rs`
  (create): spec validation, handle allocation, journal open, `Report` construction;
  double end / use after end / unknown run return typed errors.
- **T2 — provider abstraction.** `packages/spectra-agent/src/provider/mod.rs` (trait,
  request/response, `Usage`, `ProviderError`), `openai_compatible.rs`, `local.rs` bridging
  to `std.ml.generate_ex` (the only language-reachable generation entry; temperature,
  `top_k`, seed) plus the tokenizer. Transport uses the existing client
  (`packages/spectra-api/src/client_core.rs`) so TLS, pooling and SSRF policy apply
  unchanged. Bounded retries only for idempotent requests; a retried request is journaled
  once.
- **T3 — surface functions.** `builtin_std_core.rs` (`make_std_agent`), catalog
  regeneration, host implementations: `agent_start`, `agent_end`, `ask`, `ask_stream`,
  `stream_next`, `stream_close`, `ask_json`, `embed`. `ask_json` validates the response
  against the schema in the client, independently of provider-side constrained decoding.
  Failures return the `std.error` shape so `match`/`if let`/`?` work. `ask_stream` returns
  a chunk stream backed by the runtime stream facility.
- **T4 — determinism controls.** Record effective sampling parameters (including seed) in
  the journal; fail closed when a run requested deterministic sampling and the provider
  silently ignores the seed.

Acceptance:
- An agent that calls `ask` twice and ends cleanly reports tokens and cost consistent with
  the mock provider.
- `ask_json` rejects a schema-violating response with a typed error even when the provider
  claims compliance; `embed` returns a 1-D float tensor.
- Run lifecycle misuse returns typed errors, never a panic.
- `scripts/validate_r3211_model_gateway.py` passes and is registered.

Validation: `python scripts/validate_r3211_model_gateway.py`

#### R-3222 — Tool Loop and Governed Dispatch (`act`)

`runtime` · P0 · risk high · deps: `R-3210`, `R-3211` · milestone M3

`act(run, prompt)` runs model↔tool turns until a final answer, invoking the project's
`#[agent_tool]` functions through the governed dispatch path.

Production intent: the tool loop is the difference between a chatbot and an agent; it must
exist before governance, so that every later guarantee (capabilities, taint, budget,
journal) is proven against the real execution path.

Tasks:
- **T1 — dispatcher spike (de-risk first).** A minimal two-tool, two-module project:
  synthesized marshalling wrappers and a static tool table, runtime invoking a wrapper by
  address, one async tool with a fake I/O round trip. Prove it in JIT and AOT. Freeze the
  wrapper ABI, table format and address-lifetime rules in ADR 0019 (update the ADR with
  spike evidence).
- **T2 — wrapper and table synthesis.** `midend`: per-tool wrapper in the tool's own
  module — decode JSON arguments through the derived `from_json`, call the tool, encode
  the result through `to_json`; a static table of (name, wrapper address);
  `external_functions` declarations so verification resolves cross-module references;
  `suspension_barrier` (or an equivalent shield) so inlining/DCE cannot remove or
  duplicate synthesized functions; hidden lowering-provided arguments added at every
  `agent_start` call site carrying table address and length.
- **T3 — runtime tool registry and invocation.** `packages/spectra-agent/src/tools.rs`:
  registry keyed by name, wrapper invocation with an out-slot scratch buffer for the
  result, argument and result marshalling, typed errors for unknown tool / invalid
  arguments / tool failure; each invocation journaled.
- **T4 — the `act` loop.** Model turn with the tool schemas; tool call parsed and
  dispatched; result appended to the transcript; repeat until a final answer or a ceiling
  (`max_tool_calls`, budget, cancellation). Works with zero tools (plain answer).
- **T5 — governance integration.** Tool execution flows through the same governed
  dispatch as any host call (capabilities enforced once `R-3214` lands; tool calls
  originating in a run with untrusted content are gated once `R-3223` lands).
  `agent_start` fails fast when a tool's effects exceed the run's grant (integrates with
  `R-3214`-T4).
- **T6 — fixtures.** Multi-step chain (≥2 tool calls then final) against the mock
  provider; malformed arguments repaired by feeding the typed error back to the model;
  unknown tool name returns an error to the model, never crashes; tool defined in another
  module; JIT and AOT parity.

Acceptance:
- The spike proves JIT and AOT invocation before the loop is built; ADR 0019 carries the
  evidence.
- A tool chain completes end to end; every tool invocation is journaled and
  capability-checked; unknown tools and malformed arguments degrade gracefully.
- Cross-module tools work in both execution modes.
- `scripts/validate_r3222_agent_act.py` passes and is registered.

Validation: `python scripts/validate_r3222_agent_act.py`

#### R-3212 — Agent Memory over `std.ml`

`ml` · P1 · risk medium · deps: `R-3211` · milestone M3

`remember` and `recall` as a thin, provenance-carrying layer over the existing vector
index and RAG toolkit (R-1803).

Production intent: long-running agents need memory that survives a restart and can be
audited; the storage primitive already exists and is validated — a second one would be
waste.

Tasks:
- **T1 — tiers and provenance.** `packages/spectra-agent/src/memory.rs` (create): tier
  value (`episodic`/`semantic`/`procedural`); each entry records origin, run identity,
  timestamp and goal; persistence reuses the existing
  `vector_index_persist`/`load` artifact format (`registration.rs:804-813`).
- **T2 — deterministic recall.** Score ties broken by insertion order; payload capped by
  tokens, not only `top_k`.
- **T3 — surface and fixture.** Register `remember`/`recall`; create
  `tests/validation/366_agent_memory.spectra`; run in JIT and AOT.

Acceptance:
- Memory survives persist/load across processes.
- Recall ordering is identical for identical inputs.
- Every recalled entry reports its origin and the run that wrote it.
- `scripts/validate_r3212_agent_memory.py` passes and is registered.

Validation: `python scripts/validate_r3212_agent_memory.py`

### M4 — Governance

#### R-3213 — Run Context and Propagation

`runtime` · P0 · risk high · deps: `R-3209` · milestone M4

An active-run context the runtime can consult on every dispatch, with explicit
propagation and fail-closed behavior for detached work.

Production intent: enforcement without a reliable notion of the current run is theater;
this item is the precondition for `R-3214`.

Tasks:
- **T1 — stacking run context.** `runtime/src/agent/run_context.rs` (create), mirroring
  `runtime/src/tracing/mod.rs:197-200` with a guard type for scope exit. Nested runs push;
  a nested run sees its own chain and inherits the intersection of capabilities; a pop on
  an empty stack is a reported programming error, never ignored.
- **T2 — propagation to worker threads.** `runtime/src/stdlib/concurrent_core.rs:562-584`
  spawn path carries the context (capture at spawn, restore inside the worker, clear on
  completion — the `tracing::with_context` pattern, `tracing/mod.rs:492-508`); coroutine
  resumption restores it too.
- **T3 — fail-closed detached work.** Spawning detached work inside a run requires its own
  capability grant; without it the spawn returns a typed error. A propagation gap becomes
  a visible failure, not a silent bypass.

Acceptance:
- A tool invoked by a run sees the run; a host call inside it is attributable to that run.
- Work spawned by a run carries the context; detached work does not and is rejected
  without a grant.
- No ABI change: the repr(C) call context is untouched.
- `scripts/validate_r3213_run_context.py` passes and is registered.

Validation: `python scripts/validate_r3213_run_context.py`

#### R-3214 — Capability Enforcement at the Dispatch Point

`runtime` · P0 · risk high · deps: `R-3213`, `R-3206` · milestone M4

One policy decision applied at every generic dispatch entrypoint, with a dedicated denial
path and no fast-path hole.

Production intent: the only capability in this phase a library cannot replicate — the
language controls every effect, so the boundary can be total while a run is active.

Tasks:
- **T1 — extract the single dispatch function.** `runtime/src/ffi_host_registry.rs`: add
  `dispatch_generic(name, func_ptr, args, arg_len, results, result_len) -> i32` wrapping
  `invoke_host_function` (`:162-186`). `runtime/src/ffi_lifecycle.rs`: route `:280`
  (uncached single), `:311` (cached single), `:344`/`:387` (batch variants) through it.
  Policy must run on cache hit (the cached path resolves without consulting the registry,
  `ffi_host_registry.rs:78-101`) and per batch item, never panicking inside the outer
  `catch_unwind`.
- **T2 — denial channel.** `runtime/src/ffi_core.rs`: add `HOST_STATUS_DENIED` to the four
  existing codes (`:320-323`). `runtime/src/panic.rs`: add a structured capability-denial
  entry point beside `spectra_rt_panic` (`:63-68`). `runtime/src/abi.rs`: register the new
  symbol in the `RuntimeImport` catalog and bump its COUNT (`:132`).
  `backend/src/codegen_instruction_host.rs`: branch on the new status
  (`:629-635`, `:652-657`). Denial message format per ADR 0016; never secrets, payloads or
  headers.
- **T3 — policy evaluation and `authorize()`.** `packages/spectra-agent/src/policy.rs`
  (create): capability parsing, namespace matching, scope predicates from the catalog's
  `scope_keys` with extractors registered next to the host call (URL host, path prefix,
  table name, HTTP method). `authorize(run, host) -> Result<bool, Error>` reports the same
  decision the dispatch enforces.
- **T4 — effects and capabilities in the surface.** `compiler/src/semantic/surface.rs`
  fills effects and required capabilities from reachable host calls; `cli_surface.rs`
  emits them; `agent_start` fails at startup when a tool's effects exceed the run's grant
  (not on first call).

Acceptance:
- I2: a host call outside the grant fails through all four generic entrypoints, including
  the cached and batch paths.
- `authorize()` agrees with the enforced decision.
- I5: programs without an active run behave exactly as before (dedicated regression
  fixture + unchanged validation suite).
- The fast-path invariant test from `R-3201` still passes.
- `scripts/validate_r3214_capability_enforcement.py` passes and is registered.

Validation: `python scripts/validate_r3214_capability_enforcement.py`

#### R-3215 — Capability Vocabulary Validated by the Compiler

`semantic` · P0 · risk medium · deps: `R-3214`, `R-3206` · milestone M4

Capability strings are validated against the registered host-call namespaces, with
did-you-mean, instead of being free-form strings.

Production intent: the most common failure of permission systems is a grant that matches
nothing and nobody notices; the compiler can catch it at build time because it already
depends on the catalog.

Tasks:
- **T1 — validate the allow list.** `compiler/src/semantic/semantic_agent.rs`:
  validate each capability in `AgentSpec.allow` against catalog host-call paths; reject a
  scoped form for host calls with no registered extractor. Codes: `E3201` (unknown
  capability, did-you-mean using the existing Levenshtein path,
  `semantic_item_import.rs:85-99`), `E3202` (unsupported scope form).
- **T2 — documentation and drift guard.**
  `docs/diagnostics/error-code-reference.md`: rows for `E3201`/`E3202` with hint text;
  `docs/agent-platform.md`: generated capability reference. A generator
  (`scripts/generate_capability_reference.py --check`) fails on drift.

Acceptance:
- An unknown capability fails compilation with a near-match suggestion.
- A scoped capability on a host call without an extractor fails with its own code.
- The generated capability reference matches the catalog.
- `scripts/validate_r3215_capability_vocabulary.py` passes and is registered.

Validation: `python scripts/validate_r3215_capability_vocabulary.py`

#### R-3216 — Budget, Accounting and Cooperative Cancellation

`runtime` · P0 · risk medium · deps: `R-3213` · milestone M4

Hard ceilings on tokens, cost, wall time and tool calls, enforced by cancelling the run.

Production intent: an agent loop without a ceiling is a financial incident waiting to
happen; the ceiling is a default, not an option.

Tasks:
- **T1 — accounting.** `packages/spectra-agent/src/budget.rs` (create): accumulate tokens
  and cost per provider response and per tool call; expose `budget_remaining(run)`; a
  checked-in price table with an explicit unknown-model policy that fails closed in
  production mode.
- **T2 — cooperative cancellation.** Crossing a ceiling marks the run cancelled, cancels
  in-flight tasks (existing cancellation entry points), and makes subsequent calls on that
  run return typed errors. Cancellation is cooperative: no thread is killed; long
  operations observe the flag at their await points.
- **T3 — report integration.** `Report.status` distinguishes `completed`,
  `budget_exceeded`, `failed`; the report names which ceiling was hit.

Acceptance:
- I10: no run exceeds a declared ceiling; the report states which ceiling was hit.
- Cancellation propagates to in-flight model calls and tool executions.
- Accounting matches the mock provider's reported usage exactly.
- `scripts/validate_r3216_agent_budget.py` passes and is registered.

Validation: `python scripts/validate_r3216_agent_budget.py`

#### R-3217 — Journal, Replay, Approval, Assertions and Tracing

`runtime` · P0 · risk high · deps: `R-3213`, `R-3211` · milestone M4

Every side effect recorded once, replayable without repeating effects, with human
approval, governed assertions and OpenTelemetry GenAI spans.

Production intent: durability is what makes an agent operable — a crash resumes instead of
restarting, and behavior can be reproduced instead of argued about.

Tasks:
- **T1 — journal format and writer.** `packages/spectra-agent/src/journal.rs` (create):
  append-only records with run id, step sequence, kind, input digest, output digest,
  idempotency key, seed, usage, timestamp. Digests by default; payload capture is opt-in
  and never captures credentials. Flush before returning from any effect-bearing call so a
  crash cannot lose a completed effect.
- **T2 — replay algorithm.** `packages/spectra-agent/src/replay.rs` (create): return
  recorded outputs for matching step keys instead of executing; a step whose key is absent
  executes normally and appends (partial journals resume forward); idempotency keys
  derived from run, step and input digest so retries after a crash collide instead of
  duplicating.
- **T3 — human approval.** `packages/spectra-agent/src/approval.rs` (create):
  `approve(run, action)` asks a registered approver; the default approver denies when no
  UI is attached; decisions (including allow-once / allow-always) are journaled with who
  and when, so replay never asks twice.
- **T4 — assertions and authority surface.** `require(run, condition, message)`: `false`
  returns a typed error carrying the message and the run goal and marks the run failed.
  Register `approve` and `require` on the surface (semantic table + catalog + fixture).
  Assertion outcomes are journaled.
- **T5 — OpenTelemetry GenAI spans.** `packages/spectra-agent/src/trace.rs` (create),
  reusing `std.api.trace`: spans `invoke_agent`, `plan`, `execute_tool`, `chat {model}`
  with `gen_ai.agent.name` and `gen_ai.conversation.id`; content capture opt-in and off by
  default; the conventions version is pinned and recorded on the span.

Acceptance:
- I3: killing the process mid-run and resuming completes the run without duplicating any
  effect, proven by a counting test server.
- A replayed run produces identical tool ordering and outputs.
- Approval decisions survive replay and are attributed; the default approver denies.
- `require(false)` returns a typed error naming message and goal; the run reports failed.
- Spans validate against the pinned conventions version; content absent unless opted in.
- `scripts/validate_r3217_agent_journal.py` passes and is registered.

Validation: `python scripts/validate_r3217_agent_journal.py`

#### R-3223 — Message and Handle Taint

`runtime` · P0 · risk high · deps: `R-3213`, `R-3214`, `R-3217`, `R-3211` · milestone M4

Provenance for everything that enters a run, explicit declassification, and gating of
sensitive sinks — at the honest granularity the ABI supports.

Production intent: prompt injection cannot be prevented, but damage can be bounded —
capabilities bound what a run may do; taint bounds what it may do *without a human
noticing* once external content is in context.

Tasks:
- **T1 — provenance ledger.** Extend the run/transcript: every message carries origin
  (`user` | `model` | `tool:<name>` | `external:<source>`); tool results and external
  content default to untrusted. `untrusted(run, value, origin)` and
  `trust(run, value, reason)` record ledger entries keyed by content digest; `trust`
  requires a reason and is audited. Both return the value unchanged (documented).
- **T2 — sink classification.** Consume the catalog's `sink`/`scope_keys` fields
  (`R-3206`): a completeness test asserts every production namespace is classified; the
  initial classification covers write-side namespaces (filesystem writes, HTTP requests
  with unsafe methods, database writes, agent compensation, package/host mutations) with
  an explicit override table for exceptions.
- **T3 — gating at dispatch.** Extend `dispatch_generic`: when the active run holds
  untrusted content and the target host call is a sink, apply the run's policy —
  `block` (denial with a `trust_required` reason), `approve` (route through the approval
  primitive; the default approver denies), `allow` (proceed, journaled). Scoped predicates
  (e.g. HTTP method) are evaluated from the call arguments through the extractor registry.
- **T4 — surface and policy.** Register `untrusted`/`trust`; add `AgentSpec.untrusted`
  (default `approve`); journal provenance, declassifications and gated decisions.
- **T5 — fixtures.** Hostile-description injection (a remote tool description carrying
  instructions never becomes control flow); policy matrix (block/approve/allow);
  declassification auditing; sink predicate matrix (GET passes, POST gated); no-run
  programs unaffected.

Acceptance:
- I9: with untrusted content in context, sensitive sinks are never reached silently;
  the policy decides, and the decision is journaled.
- Declassification requires a reason and is attributable.
- The documented limits (message granularity, no string-level flow) appear in
  `docs/agent-platform.md`.
- `scripts/validate_r3223_agent_taint.py` passes and is registered.

Validation: `python scripts/validate_r3223_agent_taint.py`

#### R-3224 — Compensation Declaration and Execution

`runtime` · P1 · risk medium · deps: `R-3214`, `R-3217`, `R-3222` · milestone M4

`compensate` declares how to undo an irreversible step; `rollback` executes declared
compensations in LIFO order through the governed dispatch.

Production intent: the runtime cannot invent how to undo a POST, but it can remember that
the author said how — and it can refuse to lose that declaration across a crash.

Tasks:
- **T1 — declaration.** Register `compensate(run, tool, arguments_json)`: append a
  pending compensation (LIFO) to the journal with an idempotency key. Literal tool names
  are validated at compile time (`E3205` unknown tool); non-literal names are validated
  against the runtime tool registry at declaration time.
- **T2 — execution.** Register `rollback(run, reason)`: execute pending compensations in
  LIFO through the governed dispatch (capabilities and taint still apply), journal each
  attempt and outcome; a failed compensation is recorded and does not mask later ones; the
  command is replay-safe (already-executed compensations are not re-executed).
- **T3 — report integration.** `Report.compensations_pending` surfaces an unrolled-back
  run; `Report.status` reports `rolled_back` after an explicit rollback.
- **T4 — fixtures.** Injected failure after an irreversible step → compensation executes
  exactly once (counting server); crash + replay does not duplicate it; unknown literal
  tool name fails compile with `E3205`; compensation error paths are journaled.

Acceptance:
- I8: compensation executes exactly once, LIFO, replay-safe; pending compensations are
  visible in the report.
- Compensations run through the governed dispatch (denial fixtures apply).
- `docs/diagnostics/error-code-reference.md` gains the `E3205` row.
- `scripts/validate_r3224_agent_compensation.py` passes and is registered.

Validation: `python scripts/validate_r3224_agent_compensation.py`

### M5 — Interop, evaluation, release

#### R-3218 — `std.agent.mcp` Client and Server

`web` · P1 · risk medium · deps: `R-3211`, `R-3214` · milestone M5

Consume and expose Model Context Protocol tools over HTTP transport, with
untrusted-description handling.

Production intent: MCP is where the tool ecosystem already lives; exposing Spectra
functions as MCP tools makes a Spectra service usable by any agent, and consuming MCP
tools removes the need to reimplement integrations.

Tasks:
- **T1 — client over HTTP.** `packages/spectra-agent/src/mcp/client.rs` (create):
  `tools/list` and `tools/call` over the streamable HTTP transport using the existing
  client. Remote descriptions and schemas are untrusted data, never instructions (they
  enter the transcript tagged, feeding `R-3223`). Each remote server maps to a capability
  derived from its identity, so grants are per server.
- **T2 — server exposing Spectra tools.** `packages/spectra-agent/src/mcp/server.rs`
  (create): `tools/list` and `tools/call` from the derived `agent_tool` descriptors;
  `inputSchema` is the derived schema; calls run inside a run with a capability set from
  host configuration and flow through the tool registry/wrapper invocation.
- **T3 — transport limitation recorded.** `docs/agent-platform.md`: stdio transport
  requires subprocess support the language does not have; HTTP is the supported path; a
  follow-up reference instead of an implied promise.

Acceptance:
- I7: a Spectra agent calls a remote MCP tool through the governed dispatch and the
  journal records it; a third-party client can call a Spectra project's tools over MCP.
- A hostile tool description never influences control flow (fixture).
- `scripts/validate_r3218_mcp.py` passes and is registered.

Validation: `python scripts/validate_r3218_mcp.py`

#### R-3219 — `std.agent.protocol`: A2A and ACP Exposure

`web` · P2 · risk medium · deps: `R-3217`, `R-3218` · milestone M5

Expose a Spectra agent as an A2A server and as an ACP agent, reusing the approval
primitive for permission requests.

Production intent: interop is an adapter, not an architecture; it lands last because the
primitives it needs (run, journal, approval) already exist by then.

Tasks:
- **T1 — A2A agent card and task lifecycle.** `packages/spectra-agent/src/protocol/a2a.rs`
  (create): the card is generated from a description record plus the derived tool list;
  long-running tasks map to journaled runs so a client can poll and a restart can resume.
- **T2 — ACP permission bridge.** `packages/spectra-agent/src/protocol/acp.rs` (create):
  `session/request_permission` maps to `approve(run, action)`; allow-once and allow-always
  map to journaled decisions; only implemented capabilities are advertised.

Acceptance:
- A remote A2A client delegates a task and observes completion or failure with a stable
  reason.
- ACP permission denial aborts the action and is journaled.
- Neither adapter bypasses the governed dispatch.
- `scripts/validate_r3219_agent_protocols.py` passes and is registered.

Validation: `python scripts/validate_r3219_agent_protocols.py`

#### R-3220 — Evaluation Harness and `spectralang agent eval`

`tooling` · P0 · risk medium · deps: `R-3217`, `R-3203` · milestone M5

Agent behavior measured as regression, with deterministic graders, judge graders and
`pass@k`.

Production intent: governance is tested deterministically; behavior is evaluated
statistically — mixing them produces suites that fail on model variance and pass by luck.

Tasks:
- **T1 — case format and runner.** `packages/spectra-agent/src/eval/mod.rs` (create):
  cases declare input, expectations (approval required, refusal expected, tool subset,
  output schema), grader list and repeat count; the runner executes each case in a fresh
  run against the configured provider with the mock provider for CI.
- **T2 — graders.** `packages/spectra-agent/src/eval/graders.rs` (create): deterministic
  graders (schema conformance, tool-set constraint, approval presence, budget respected);
  the judge grader is explicit, off by default and its prompt is checked in.
- **T3 — CLI and CI gate.** `tools/spectra-cli/src/cli_agent_eval.rs` (create):
  `spectralang agent eval --json [--suite PATH] [--repeat N]` emits case results,
  `pass@1`, `pass^k` and cost; exit 65 on regression against the checked-in baseline;
  exit 0 while writing the first baseline. `--json` follows the `release-info` pattern
  (its own flag and executor; build-command `--json` restrictions do not apply).
- **T4 — example suite and baseline.** `examples/agent/evals/*.json` plus
  `tests/validation/369_agent_eval_harness.spectra`; a small suite exercising approval,
  refusal, budget and schema conformance against the local provider; baseline checked in.

Acceptance:
- `agent eval` runs a suite, reports `pass@1`/`pass@k`, and fails the build on regression.
- Judge grading never runs in the default CI path.
- Governance assertions are covered by deterministic tests, not by evals.
- `scripts/validate_r3220_agent_eval.py` passes and is registered.

Validation: `python scripts/validate_r3220_agent_eval.py`

#### R-3221 — Agent Platform Integration, Conformance and Release

`ecosystem` · P0 · risk high · deps: `R-3210`, `R-3211`, `R-3212`, `R-3214`, `R-3215`,
`R-3216`, `R-3217`, `R-3218`, `R-3219`, `R-3220`, `R-3222`, `R-3223`, `R-3224` ·
milestone M5

Prove the platform works with the rest of the language, document it, publish it, and gate
the release.

Production intent: a library that is not integrated with the formatter, LSP, package flow,
docs and conformance suite is a prototype, not a product.

Tasks:
- **T1 — package distribution.** `packages/spectra-agent/spectra.toml` (manifest in the
  shape of `packages/spectra-api/spectra.toml`), `src/bindings/*.spectra` (module
  declarations mirroring `packages/spectra-api/src/bindings`), publish to the local
  registry following the `R-2217` flow, and a consumer project that adds it through
  `package add`.
- **T2 — language tooling integration.** LSP: attribute and namespace complete correctly,
  no false unknown-symbol diagnostics. Formatter: idempotent formatting of attributed
  functions. Lint and REPL handle the new surface without special cases.
- **T3 — documentation set.** `docs/agent-platform.md` (concept, surface reference,
  generated capability reference, security model listing what is promised and what is
  not); `docs/api/README.md` link; `docs/book/11-agents.md` (style of
  `docs/book/09-hello-http.md`); `docs/AI-AGENT-REFERENCE.md` agent section plus the async
  essentials the surface uses (`await`, `Task`, `block_on`); `AGENTS.md` validation
  commands updated with the new commands; `.spectra/reference.json` generated-artifact
  note.
- **T4 — runnable examples.** `examples/agent/01-tool-and-run`, `02-approval-and-budget`,
  `03-mcp-and-memory`, `04-durable-replay`: each runs end to end through the normal CLI
  with a mock provider (no credentials required), in JIT and via
  `spectralang compile --emit-exe`.
- **T5 — conformance suite v1.** `packages/spectra-agent/tests/conformance.rs` (create)
  plus `scripts/validate_r3221_agent_conformance.py`: capability denial on all four
  dispatch entrypoints; fast-path invariant; journal replay without duplicate effects;
  budget cancellation; approval default deny; taint policy matrix; compensation exactly
  once; surface determinism; MCP round trip; examples. The report under `target/` is
  required by the release gate.
- **T6 — integrated project gate.** `tests/projects/valid/integrated_agent_service`
  (create): a multi-module service exposing an HTTP endpoint, running an agent with a
  budget, requiring approval for one action, journaling the run, exercised by a test that
  interrupts and resumes; runs in JIT and AOT.

Acceptance:
- The package is publishable to the local registry and consumable through the normal
  package flow.
- Formatter, LSP, lint and REPL handle the attribute and namespace without special cases.
- The conformance suite passes and its report is required by the release gate.
- The integrated project runs in JIT and AOT, including interruption and resume.
- `scripts/validate_r3221_agent_package.py`, `validate_r3221_agent_conformance.py` and
  `validate_r3221_integrated_agent_service.py` pass and are registered.

Validation: `python scripts/validate_r3221_agent_package.py && python scripts/validate_r3221_agent_conformance.py && python scripts/validate_r3221_integrated_agent_service.py`

---

## 7. Engineering standards

### 7.1 Per-item definition of done

An item is complete only when all of the following hold:

1. Code lands in the working tree, comments in English.
2. At least one regression fixture exists (`tests/validation/NNN_*.spectra`, starting at
   364; `tests/errors/` for diagnostics) and passes through the normal CLI path.
3. Runtime-behavior items exercise **both** `spectralang run` and
   `spectralang compile --emit-exe` (AOT parity); an ABI-touching item that only checked
   JIT is not complete.
4. The item's validator `scripts/validate_r32NN_*.py` exists, follows the standard shape
   (require/fail helpers, roadmap registration check, fixture execution, JSON report under
   `target/`), passes, and is registered in `run_tests.ps1` with the standard
   `Invoke-HostCommand` block and `$binary` argument.
5. Host-surface items update `scripts/stdlib_contract.toml` (namespace, classification,
   probe) and keep `validate_r3007` passing with `--require-catalog`.
6. Public-surface items update `compiler/tests/snapshots/` and the catalog, regenerated
   (never hand-edited when generated).
7. Documentation obligations for the item are met (error-code rows, surface reference,
   ADR updates) and the item's acceptance list is fully satisfied.

Partial work stays `in_progress` in the tracker with the remaining criteria visible.

### 7.2 Validation conventions

- Validators live in `scripts/`, are deterministic, take `--binary <path>` where they
  execute Spectra code, and write JSON reports under `target/`.
- `run_tests.ps1` is the single suite entry point: `$binary` is defined at `:20`; blocks
  are appended in the registration region (`:1074-2809`) in the established shape; the
  suite appends status to `TEST_RESULTS.txt`.
- `tests/validation` fixtures are compile-checked by the suite; end-to-end `check` + `run`
  coverage lives in the validators (the repo-wide pattern).
- Error fixtures are descriptive-named under `tests/errors/` and must fail compilation.

### 7.3 Diagnostics conventions

- Phase 32 owns the `E3201-E3209` range. Landed codes:
  `E3201` unknown capability (did-you-mean), `E3202` unsupported capability scope,
  `E3203` invalid `#[agent_tool]` declaration, `E3204` tool parameter type not
  derivable/decodable, `E3205` unknown tool name in a literal `compensate` call.
- A code lands only with: a doc row in `docs/diagnostics/error-code-reference.md`, a
  `tests/errors/` fixture, and (convention) coverage in the item's validator.
- `E320x` names must not collide with the illustrative samples in the concept document
  (which are not real codes).

### 7.4 Dependency and determinism rules

- No new crates without an ADR. `serde`/`serde_json` stay confined to the CLI and
  `packages/`; the midend and backend do not gain serde.
- Every JSON-emitting command orders output deterministically (module path, then symbol
  path; stable key order) and reports any trimming.
- Seeds and effective sampling parameters are recorded in the journal for every model
  call; replay never re-samples.

---

## 8. Validation strategy

### 8.1 Layers

| Layer | Scope | Where |
|---|---|---|
| Crate unit tests | dispatch extraction, policy matching, journal/replay, budget accounting, provider conformance | `cargo test -p spectra-runtime / spectra-agent / spectra-midend / spectra-compiler / spectra-api` |
| Compile fixtures | every new construct compiles (`tests/validation`) | `run_tests.ps1` group 1 |
| Error fixtures | every new diagnostic fails with its code (`tests/errors`) | `run_tests.ps1` group 3 |
| Validators | end-to-end `check`/`run`/AOT, invariant proofs, contract audits | `scripts/validate_r32NN_*.py` |
| Contract drift | generated artifacts and hand tables are consistent | `cargo test -p spectra-api --test contract_drift`, generator `--check` modes |
| Contract governance | namespaces, probes, classifications | `validate_r3007` with `--require-catalog` |
| Conformance | the phase's cross-cutting invariants | `packages/spectra-agent/tests/conformance.rs` + `R-3221` report |
| Evaluation | model-dependent behavior | `spectralang agent eval` (opt-in in CI; baseline-gated) |

### 8.2 Governance versus behavior

Deterministic tests cover everything the runtime must enforce (capability denial, taint
gating, budget cancellation, replay, approval defaults, compensation). The eval harness
covers what the model does inside those bounds (refusal, tool choice, schema
conformance). A behavior claim must never be pinned by a deterministic test that encodes
model output, and a governance claim must never depend on an eval.

### 8.3 Release gate

`R-3221` is the certification gate. The phase cannot be marked complete while any
capability-enforcement, journal-replay, taint, compensation or integrated-project
assertion fails, or while any item's validator is unregistered.

---

## 9. Documentation, packaging and release deliverables

| Deliverable | Owner item |
|---|---|
| ADRs 0016–0019 | `R-3201` |
| Error-code family and per-code rows (`E3201`–`E3205`) | `R-3201`, `R-3210`, `R-3215`, `R-3224` |
| Generated capability reference | `R-3215` |
| `docs/agent-platform.md` (concept, surface, security model, limits) | `R-3221` |
| Surface/impact/explain/docs CLI reference entries + `AI-AGENT-REFERENCE.md` sections | `R-3202`–`R-3205`, `R-3221` |
| Book chapter `docs/book/11-agents.md` | `R-3221` |
| `AGENTS.md` validation-commands block | `R-3221` |
| Examples `examples/agent/01..04` | `R-3221` |
| Package manifest, bindings, registry publish + consumer test | `R-3221` |
| Integrated project `tests/projects/valid/integrated_agent_service` | `R-3221` |
| Conformance report under `target/`, required by the release gate | `R-3221` |

---

## 10. Metrics and measurement

Concept §12 mapped to concrete measurement. Metrics without a command are aspirational;
the table states where each one is measured or why it is deferred.

| Metric | Measurement | Status |
|---|---|---|
| Unauthorized-effect blocking | Injection fixtures + denial matrix across the four dispatch entrypoints, with and without capabilities (`R-3214`, `R-3223`, `R-3221` conformance) | In-phase |
| Resumption | Interrupt-and-resume fixture with a counting server; absence of duplicated effects in replay (`R-3217`) | In-phase |
| Budget enforcement | Ceiling matrix; evidence of cooperative cancellation in the run report (`R-3216`) | In-phase |
| Trace conformance | Span assertions against the pinned GenAI conventions version (`R-3217`) | In-phase |
| Cost | Tokens and cost per resolved task in the eval report, p50/p95 across repeats (`R-3220`) | In-phase (per suite) |
| Legibility by agents | Tokens required to answer "what does this project expose" with and without `surface --json`; measured on the repository and on the integrated project (`R-3202`, `R-3221`) | In-phase (manual protocol, recorded in the item evidence) |
| Generation friction | Task corpus for Spectra agent programs: tokens per task, valid-syntax rate, repair iterations, `pass@1`/`pass^k` | Deferred: needs an external corpus and a stable model target; the eval harness (`R-3220`) is the prerequisite delivered here |

---

## 11. Risk register

| Risk | Mitigation |
|---|---|
| Capability policy bypassed through the fast path | Fast-path invariant test (`R-3201`); fast calls cannot express denial by construction |
| Run context lost across worker threads | Fail-closed default for detached work (`R-3213`) |
| Dispatcher ABI surprises (address lifetime, cross-module relocation, async tools) | Spike before the loop (`R-3222`-T1) proves JIT and AOT with async tools in two modules; ADR 0019 freezes the contract with spike evidence |
| ABI change breaking AOT silently | Every item touching `runtime/src/abi.rs` must exercise a real `--emit-exe` build, not only JIT |
| Catalog migration breaking the contract auditor | Three-step migration with a diff-empty gate at each step (`R-3207`); auditor extended first (`R-3206`) |
| Attribute surface growing beyond one attribute | Recorded as a non-goal; a second attribute requires revising ADR 0017 first |
| Durability implemented as best-effort writing | Flush before returning from any effect-bearing call; crash test kills the process mid-effect |
| Taint over-promising (injection immunity) | Documentation states the message-granularity limit explicitly; the injection fixture measures *bounded damage*, not immunity |
| Compensation read as automatic rollback | Documentation and the report field (`compensations_pending`) make the explicit-rollback model visible; `R-3224` acceptance pins it |
| Eval suites flaky under model variance | Deterministic graders in CI; judge grading opt-in; behavior claims live in evals, governance claims in tests |
| Cross-module tool names colliding | Project-wide uniqueness validation with both spans (`R-3210`) |
| `std.ml` catalog/compiler drift (e.g. `text_embed_model` arity) affecting the gateway | The gateway consumes compiler-declared signatures only; the pre-existing drift is out of scope and tracked in `scripts/stdlib_contract.toml` follow-ups |

---

## 12. Out of scope

Everything in section 2.5, plus:

- Effect rows in the type system; static pre/post-condition verification.
- Filesystem or network rollback; automatic compensation on the panic path.
- String-level information-flow tracking; secret detection.
- Subprocess execution; MCP over stdio; LSP-style multiplexed transports.
- A generic agent framework API beyond the surface in section 4.
- Tool-marketplace or registry features beyond the local registry flow.
- Async `main`; any change to existing language semantics.

---

## 13. Definition of done for the phase

1. Every item `R-3201`–`R-3224` is `complete` in `roadmap/roadmap.toml` with its validator
   registered and passing, and the backlog carrying the same status.
2. Invariants I1–I10 are each pinned by a passing test, and the pins are listed in the
   conformance report.
3. Adding a native function to `std.agent` costs the documented three-edit path, proven by
   a recorded before/after edit count (`R-3209`, `R-3222`).
4. The integrated project runs in JIT and AOT, survives interruption and resume, and stays
   inside its budget.
5. Documentation, examples and package publishing are complete; the AI agent reference
   points to the new surface; the capability reference is generated and cannot drift.
6. The conformance report is required by the release gate; no governance assertion is
   skipped or waived.

## 14. Cross-references

- Vision and rationale: `docs/agent-platform-concept.md`.
- Strategy chapter: `docs/production-ai-implementation-plan.md` ("Agent Platform Vision").
- Tracker: `roadmap/roadmap.toml`, `phase_32`.
- Human backlog: `docs/roadmap-backlog.md`, Phase 32.
- Decisions: `docs/adr/0016`–`docs/adr/0019` (produced by `R-3201`).
- Surface documentation (produced by `R-3221`): `docs/agent-platform.md`,
  `docs/book/11-agents.md`, `docs/AI-AGENT-REFERENCE.md`.

