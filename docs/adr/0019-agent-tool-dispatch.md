# ADR 0019: Agent Tool Dispatch ABI

Status: Accepted (interface frozen; concrete ABI details subject to the R-3222-T1 spike)

Date: 2026-09-12

Roadmap item: R-3201

> **Spike dependency.** The _shape_ of this decision — a runtime-side `act`
> loop, compiler-synthesized marshalling wrappers, and a static tool table
> passed to the runtime as hidden lowering-provided arguments of
> `agent_start` — is frozen here. The _concrete_ wrapper ABI, table layout and
> address-lifetime rules are provisional until `R-3222-T1` (the dispatcher
> spike) proves tool invocation by address in JIT **and** AOT with two tools
> in two modules. `R-3222-T1` updates this ADR with the spike evidence and
> finalizes the details. No implementation may depend on the provisional
> details before that spike lands.

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

The compiler/midend also synthesizes a **static tool table**: for each tool, a
name and the wrapper's address (plus the metadata the loop needs). The table is
passed to the runtime as **hidden lowering-provided arguments of
`agent_start`**. Nothing in user code refers to the table; the user writes only
the attribute and the function.

The runtime invokes a wrapper **by address** through the proven i64 callback
ABI. The wrapper's arguments are the decoded-argument buffer and the result
out-slot; the return value is a status in the same shape host calls use, so a
tool failure is a typed error at the loop, not a trap.

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

Synthesized wrappers and table constructors must not be inlined, duplicated or
eliminated by ordinary optimization passes. The midend already has the
precedent: `Function.suspension_barrier` (`midend/src/ir.rs:74`) marks
generated poll/drop functions opaque to ordinary CFG rewrites. Synthesized
tool-dispatch functions carry the same shield (or an equivalent one), so an
optimizer cannot remove a wrapper the runtime will call by address or
duplicate it into inconsistent copies.

## Provisional details (finalized by R-3222-T1)

The following are intentionally not frozen here and must be recorded as spike
evidence in this ADR before `act` is built:

- the exact wrapper signature and its status encoding;
- the exact table layout (entry struct, alignment, length and address
  arguments to `agent_start`);
- the result hand-off protocol (out-slot scratch buffer ownership and size
  negotiation) and the error channel for unknown tool / invalid arguments /
  tool failure;
- address-lifetime rules across JIT finalization and AOT relocation, including
  what happens when a module is recompiled or a table outlives one compilation;
- how the async tools of the tool set are driven (an async tool is a coroutine;
  the wrapper must create and drive it under the run's context).

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

**Why hidden arguments on `agent_start`.** The table is compiler-known data.
Passing it as hidden lowering-provided arguments keeps user code free of it,
keeps the surface function signature stable (`agent_start(spec)`), and puts the
data where the runtime already is when a run starts.

**Why freeze the shape now.** The shape determines which items can be built in
parallel (the attribute, the run, the surface) without rework. The concrete
ABI is exactly the part that needs a spike, so it is deliberately left open
rather than guessed.

## Consequences

- `R-3210` derives the tool name, input schema, effects and capabilities; the
  input schema is the same JSON schema the wrapper's `from_json` decodes.
- `R-3211` implements `agent_start` accepting hidden lowering-provided
  arguments (table address and length) in addition to `AgentSpec`.
- `R-3222-T1` runs the dispatcher spike and updates this ADR with the frozen
  wrapper ABI, table layout, error channel and lifetime rules.
- `R-3222-T2` synthesizes per-tool wrappers, the static table,
  `external_functions` declarations for cross-module tools, the shield, and the
  hidden arguments at every `agent_start` call site.
- `R-3222-T3` implements the runtime tool registry and address invocation with
  an out-slot scratch buffer and typed errors.
- `R-3222-T5` routes tool execution through the governed dispatch so
  capabilities, taint and journaling apply inside `act` exactly as they do for
  host calls (I7).
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
