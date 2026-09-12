# ADR 0019: Agent Tool Dispatch ABI

Status: Accepted. The wrapper ABI, registration protocol and address-lifetime
rules are **frozen** by the R-3222-T1 spike, which proved tool invocation by
address in JIT and AOT with two tools in two modules (one async with a real
provider round trip); see "Frozen ABI" below for the contract and the evidence.

Date: 2026-09-12

Roadmap item: R-3201 (shape), R-3222 (implementation and spike)

> **Implementation note (R-3222).** The shape below is what landed. Two details
> changed against the original planning text and are recorded with their
> evidence in "Frozen ABI": the tool table is not passed as hidden arguments of
> `agent_start` — each module registers its own wrappers through
> `spectra.std.agent.register_tool` — and the wrapper receives the raw argument
> document rather than a decoded buffer (it decodes with the derived
> `from_json`).

## Context

A `#[agent_tool]` function is an ordinary compiled Spectra function. A model
turn returns a tool call as a name plus JSON arguments, and the runtime must
invoke the corresponding compiled function. The naive implementation — a pure
IR loop that parses JSON and matches names inside IR — would require JSON
parsing, schema validation and dynamic name resolution to be expressible in
IR, none of which the midend currently provides.

The backend and runtime already have a proven mechanism for reaching compiled
code from the runtime: function addresses passed as `i64` through the
extern "C" callback ABI. `CoroutineCreate` passes the generated `poll` and
`drop` addresses this way in both JIT and AOT
(`backend/src/codegen_instruction_async.rs:109-121`,
`runtime/src/async_abi.rs:130-149`). The `act` loop should reuse that
mechanism rather than invent a second one.

## Decision

### D9 — Runtime-side tool loop over synthesized marshalling wrappers

The `act` loop lives in the **runtime**, not in IR. For each tool, the
compiler/midend synthesizes a **marshalling wrapper** in the tool's own module.
The wrapper:

1. decodes the JSON arguments through the derived `from_json`;
2. calls the tool function;
3. encodes the result through `to_json`;
4. writes the encoded result to a caller-provided out-slot and signals success
   or failure through a status.

The compiler/midend also synthesizes a **registration function per module** that
hands the runtime each tool's name, wrapper address and metadata through
`spectra.std.agent.register_tool`. Nothing in user code refers to any of it; the
user writes only the attribute and the function. (The originally planned "static
tool table passed to `agent_start`" was replaced by registration during the
spike; the rationale and evidence are in "Frozen ABI".)

The runtime invokes a wrapper **by address** through the proven i64 callback
ABI. The wrapper's arguments are the model's JSON argument document, the run
handle and the result out-slot; the return value is a status, so a tool failure
is a typed error at the loop, not a trap.

Rationale for putting the loop in the runtime: the loop, budget, journal,
approval and transcript all live in one place, and the same wrapper invocation
serves `act`, `rollback`, the MCP server and A2A/ACP adapters. Pure-IR
orchestration would duplicate JSON parsing and name matching in IR and still
need a runtime seam for governance.

### Proven callback ABI

The wrapper invocation reuses the two mechanisms the backend already
implements:

- **Coroutine callbacks.** `CoroutineCreate` materializes the `poll` and `drop`
  function addresses with `func_addr` and passes them to the runtime, which
  transmutes them back to `extern "C" fn` pointers
  (`backend/src/codegen_instruction_async.rs:109-121`,
  `runtime/src/async_abi.rs:130-149`). Tool wrappers use the same
  address-as-`i64` contract.
- **JIT and AOT address materialization.** In JIT, `FuncAddr` prefers the
  addresses finalized by the JIT (`finalized_function_ptrs`,
  `backend/src/codegen_instruction_indirect.rs:21-35`) so the runtime receives
  a callable pointer. In AOT, function addresses come from the linker's
  relocation of the function symbol. The tool table is populated per execution
  mode: JIT fills it with finalized pointers; AOT fills it with relocated
  symbol addresses.

Because the table is data, the runtime never needs to resolve a tool name to a
symbol. Address materialization is the backend's existing job.

### Cross-module resolution

A tool may be defined in another module than the `agent_start` call site. The
wrapper is synthesized in the **tool's own module**, and the tool table entry
references it across modules:

- the midend declares the wrapper in its defining module and records it in
  `external_functions` so module-level verification resolves the reference;
- a table entry that cannot be resolved is a compile error, never a runtime
  lookup miss;
- project-wide tool name uniqueness is validated at compile time (ADR 0017,
  D6), so a table cannot contain two entries for one name.

### Shielding synthesized functions

Synthesized wrappers and the registration function must not be inlined away or
eliminated by ordinary optimization passes. `Function.suspension_barrier`
(`midend/src/ir.rs:74`) is the precedent for opaque generated functions, but it
is **not** the right shield here: it also selects coroutine frame-local lowering
for allocas, and the wrapper is a plain function. The equivalent shield, and why
it holds, is recorded in "Frozen ABI" → "Equivalent shield".

## Frozen ABI (finalized by R-3222-T1, with spike evidence)

Status: **frozen**. The spike ran on 2026-09-12 with two tools in two modules
(one async with a real provider round trip) and passed in JIT and AOT.

### Wrapper signature and status encoding

```
extern "C" fn __spectra_agent_tool_<name>(
    run:       i64,   // the live agent-run handle
    args_json: i64,   // pointer to the model's JSON argument document,
                      // packed UTF-8 with a single trailing NUL, arena-owned
    out_slot:  i64,   // pointer to one arena word written by the wrapper
) -> i64              // 0 = success, 1 = the call was rejected
```

- All four values are machine-word integers, i.e. exactly the callback shape
  `CoroutineCreate` already passes for `poll`/`drop`; the runtime transmutes the
  address to `unsafe extern "C" fn(i64, i64, i64) -> i64` at the call site
  (`packages/spectra-agent/src/tools.rs`).
- The `Run` parameter is typed as the opaque `Run` handle in IR, so the wrapper
  is declared `(Run, string, int) -> int`.
- **Result hand-off.** `out_slot` always receives a pointer to a packed UTF-8
  string: the JSON result document on success, the typed error message on
  failure. No size negotiation is needed because the payload is always a JSON
  document, and no memory is owned by the runtime beyond the arena block it
  allocates for the slot. This replaces the provisional "decoded-argument
  buffer" wording: the wrapper receives the raw document and decodes it, which
  lets the wrapper reuse the derived `from_json` lowering.
- **Error channel.** Malformed arguments and tool failures are *values*
  (`status = 1` plus the message), never a trap: `tools::invoke` turns them into
  `AgentError::ToolFailed`, which the loop feeds back to the model. An unknown
  name is `AgentError::UnknownTool`, raised before the wrapper is reached. A
  crossed `max_tool_calls` ceiling is `AgentError::BudgetExceeded`, raised
  before the call, and is terminal.

### Registration (adapted from "hidden arguments on `agent_start`")

The SPIKE disproved the provisional table-passing shape: a static table would
need a relocated data blob, which the IR has no facility for. The sanctioned
fallback from the item description is what landed:

- Each module that declares tools synthesizes
  `__spectra_agent_register_tools_<module path>() -> int`, which emits one
  `spectra.std.agent.register_tool(name, wrapper_address, description,
  input_schema, effects_json) -> Result<bool, Error>` host call per tool. The
  address is a `FuncAddr` on the synthesized wrapper, so it materializes through
  the existing JIT/AOT mechanism.
- Every function that contains a dispatch call (`spectra.std.agent.act` or
  `spectra.std.agent.tool_call`) gets a call to its own module's registration
  function *and* to the registration function of every imported module that
  declares tools (the symbol name is derived from the module path). This is how
  a cross-module tool is registered without a second dispatch path and without
  cross-module `FuncAddr`: the defining module owns its wrappers and its
  metadata, the importing module only calls a known symbol.
- The runtime registry is process-global and idempotent by tool name; the same
  name at the same address refreshes metadata without duplicating an entry, and
  a different address replaces the stale entry.
- Registration also carries the tool's derived **effects** (host-call names
  reachable from the tool body), so `act`/`tool_call` can fail the run before
  the first dispatch when the tool's effects exceed `AgentSpec.allow`
  (`tools::enforce_run_grant`, reusing `policy::grant_matches`).

### Address lifetime

- **JIT**: `FuncAddr` prefers `finalized_function_ptrs`
  (`backend/src/codegen_instruction_indirect.rs:21-35`); a wrapper compiled in
  an earlier module of the same run is therefore already callable. Project
  builds compile dependencies first, which is the same ordering cross-module
  calls already rely on.
- **AOT**: the module that declares a tool also defines it, so no cross-module
  wrapper symbol is needed; the only cross-module symbol is the registration
  function, declared as an ordinary `external_functions` entry and resolved by
  the native linker.
- The registry lives for the process. Re-registering a name replaces its
  address, so a recompiled module never leaves a stale pointer reachable.

### Equivalent shield

`Function.suspension_barrier` (`midend/src/ir.rs:74`) is **not** used for the
synthesized functions: the flag selects coroutine frame-local lowering for
allocas, and the wrapper is a plain function whose allocas must stay ordinary.
The equivalent shield is structural: the wrapper body contains `Call`/`HostCall`
instructions, which excludes it from `function_inlining` (an inline candidate
may contain neither), dead-code elimination only removes instructions and never
whole functions, and the `FuncAddr` inside the registration function is what
makes the wrapper reachable. The registration function is reached by `Call`
from the dispatch entry points. A regression here would surface as an AOT
linker error for the missing wrapper symbol, not as a silent miscompile.

### Async tools

An `#[agent_tool]` function is async by validation. The wrapper calls the
tool's public ramp (which returns a `Task`), then emits
`spectra.async.task.block_on` + `spectra.async.task.result` — the same pair the
language's `block_on` lowers to — so the coroutine is created and driven
entirely inside the wrapper, under the run's context. The spike's `echo` tool
awaits the run's provider inside the tool body, proving a wrapper drives a
coroutine that itself awaits a host call.

### Spike evidence

| Evidence | Result |
| --- | --- |
| `tests/validation/375_agent_act.spectra` (two tools, one async with a provider round trip; multi-step chain of two calls then a final answer; malformed arguments repaired through the typed error; unknown tool reported to the model; `max_tool_calls` denial) | `spectralang run` exit 0; `compile --debug-info=none --emit-exe` binary exit 0 |
| `tests/projects/valid/agent_act` (tools in module `tools`, dispatcher in module `main`) | JIT exit 0; AOT binary exit 0 |
| IR inspection | `func_addr __spectra_agent_tool_<name>` + `hostcall spectra.std.agent.register_tool` in the synthesized registration function |
| `cargo test -p spectra-agent`, `-p spectra-midend`, `-p spectra-compiler`, `-p spectra-api` | all green |

Two defects found by the spike are fixed alongside it:

1. **String comparison in synthesized IR.** The first wrapper used the raw `ne`
   instruction to test the JSON validator's verdict against `""`, which compares
   pointers and was therefore always true. The wrapper now uses
   `spectra.std.string.eq`, the same codec the language's `!=` uses on strings.
   This is a general hazard for any synthesized IR: comparisons on aggregate
   values must go through the runtime codec.
2. **Cross-module JSON derive methods were declared as linker imports.** An
   importing module received `Record_to_json`/`_from_json`/`_json_schema`/
   `_json_error_field` in `imported_function_signatures`, and AOT emitted them
   as undefined symbols although no module ever defines them (they are always
   lowered inline). Any project importing a module that declares a
   `#[derive(Serialize)]` record failed to link. Lowering now skips those
   names (`ASTLowering::is_inlined_derive_method`).

## Rationale

**Why runtime-side.** Governance is runtime state: run context, budget,
journal, approval, transcript. A loop in IR cannot consult or update it
without a runtime call per step anyway. Keeping the loop in the runtime means
one implementation serves `act`, `rollback`, MCP and A2A/ACP, and every tool
call flows through the same governed dispatch.

**Why wrappers instead of generic reflection.** The derived `from_json` and
`to_json` operate on statically known types. Synthesizing one wrapper per tool
turns dynamic JSON dispatch into a direct static call, with no runtime type
registry and no schema interpreter. The model-facing schema comes from the same
derive that produces `to_json`/`from_json`, so the schema and the codec cannot
disagree.

**Why by address.** The callback ABI is already exercised by coroutines in both
execution modes. Reusing it avoids a second dynamic-call mechanism, a symbol
registry, or a `dlsym`-style lookup, all of which would have their own JIT/AOT
lifetime rules.

**Why registration instead of a table pointer.** Registration gives each module
ownership of the data only it knows (the wrapper address, the tool's description
and schema, its derived effects) and keeps user code free of any of it. The
grant check in `act`/`tool_call` runs over the registry the same file populated,
so a tool whose effects exceed `AgentSpec.allow` fails before it can run. The
provisional "hidden arguments on `agent_start`" shape would have required a
relocated static data blob, which the IR has no facility for; it is recorded as
an adaptation in "Frozen ABI" with the spike's evidence.

**Why freeze the shape now.** The shape determines which items can be built in
parallel (the attribute, the run, the surface) without rework. The concrete
ABI is exactly the part that needs a spike, so it is deliberately left open
rather than guessed.

## Consequences

- `R-3210` derives the tool name, input schema, effects and capabilities; the
  input schema is the same JSON schema the wrapper's `from_json` decodes.
- `R-3222-T1` ran the dispatcher spike and froze the wrapper ABI, the
  registration protocol, the error channel and the lifetime rules (see "Frozen
  ABI").
- `R-3222-T2` synthesizes the per-tool wrappers and the per-module registration
  function, wires registration into every dispatch entry point, declares the
  cross-module registration symbol, and derives each tool's effects
  (`midend/src/lowering_agent_tools.rs`).
- `R-3222-T3` implements the runtime tool registry and address invocation with
  an out-slot word and typed errors (`packages/spectra-agent/src/tools.rs`).
- `R-3222-T4` implements the `act` loop and the `tool_call` dispatcher
  primitive (`packages/spectra-agent/src/act.rs`).
- `R-3222-T5` routes tool execution through `tools::invoke`, which charges the
  budget before the call and enforces the run's grant over each tool's derived
  effects before the first dispatch (I7). Journaling is the documented seam
  R-3217 fills.
- `R-3218` (MCP) and `R-3219` (A2A/ACP) reuse the same wrapper invocation; they
  do not introduce a second dispatch path.
- No tool call can bypass the governed dispatch; a second path would violate
  invariant I7.

## Rejected Alternatives

### A pure-IR `act` loop

Rejected. It requires JSON parsing, schema validation and name matching in IR,
none of which the midend provides, and it still needs a runtime seam for
governance. Runtime-side orchestration keeps the loop in one place.

### A runtime symbol registry resolved by name

Rejected. It invents a second dynamic-call mechanism with its own JIT/AOT
lifetime rules, when the proven callback ABI already reaches compiled code by
address in both modes.

### A schema interpreter at run time

Rejected. The derived `from_json`/`to_json` already know the type statically.
A wrapper per tool turns dynamic dispatch into a direct call and removes any
chance of the model-facing schema diverging from the codec.

### Passing the tool table through user-visible `AgentSpec`

Rejected. The table is compiler data and not part of the authored surface.
Hidden lowering-provided arguments keep `agent_start(spec)` stable.
