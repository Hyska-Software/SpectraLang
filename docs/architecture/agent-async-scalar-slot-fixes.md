# Agent-platform gap sweep: defects found while extending the example set

Status: **five defects fixed 2026-09-14**; each carries a regression that fails
without its fix.

While adding examples `13`–`15` and fixtures `398`–`403` for the `std.agent`
surface, using the library rather than reading it turned up four defects. Three
are in the native runtime and the compiler; one is a fail-open direction in a
governance path. This note records what was observed, the root cause, the fix
and the evidence, in the order the defects were found.

## 1. A stalled connection killed the MCP/A2A listener for the rest of the process

**Observed.** After a client connected to a `mcp_serve`/`a2a_serve` endpoint and
sent nothing, every later connection was refused
(`Os { code: 10061, kind: ConnectionRefused }`): the endpoint was dead while the
process still advertised its authority, and the A2A card still named its URL.

**Root cause.** `net.rs`'s accept loop treated a per-connection read error as
terminal:

```rust
let (response, stop) = match read_request(&mut stream) {
    Ok(Some(request)) => …,
    Ok(None) => (400 "malformed HTTP request", false),
    Err(_) => break,          // ← the listener thread returned here
};
if stream.write_all(response.as_bytes()).is_err() {
    break;                    // ← and here
}
```

The read timeout is five seconds (`set_read_timeout`), so a port scanner, a
keep-alive probe, or any client that connects and stalls reached the `Err` arm
and dropped the `TcpListener` — which released the port. The documented
lifecycle says the listener stops for exactly two reasons: a handler that warns
`stop` (the `410 Gone` a run-scoped adapter answers once its run has ended) or
the process exiting. The `Err` arm contradicted that contract, and the adapters'
own module docs (“serves the run's living tools for as long as the run lives”)
were false in the presence of one bad peer.

**Fix.** `packages/spectra-agent/src/net.rs`: a connection that fails mid-read
ends **that connection** — it is answered `408 Request Timeout` and the loop
continues; a peer that vanished while the response was written is likewise that
peer's problem. Only the handler's `stop` and process exit end the loop.

**Evidence.** `mcp::server::tests::a_stalled_connection_does_not_take_the_listener_down`
opens a connection that sends nothing, waits past the read timeout, and then
posts a real `tools/list`. With the two arms reverted the test fails with
`ConnectionRefused` on the second connect; with the fix it passes. The A2A
listener shares the same loop.

## 2. `mcp_handle`, `a2a_handle` and `acp_handle` served a released run

**Observed.** Fixture 398 (dead-run guard matrix) found the three protocol
adapters answering for a run that `agent_end` had already released:
`mcp_handle` returned a `tools/list` document, `acp_handle` returned an
`initialize` result, and `a2a_handle` answered `-32603` carrying the
`unknown_handle` message. Every other entry point refused with the typed
`unknown_handle` error.

**Root cause.** Liveness was checked by the *socket* front-end (`410 Gone` once
the run ended) and by the dispatch, but not by the direct handler the
`*_handle` host calls invoke. The two paths disagreed about whether the
adapter was alive: the socket said the run was gone, the direct call said it was
fine.

**Fix.** Each adapter's `handle` now checks the run first
(`run::with_run(run_handle, |_| ())?`), so the direct entry point and the socket
front-end agree, and the refusal is the same typed error the rest of the surface
produces. Fixing this made fixture 398 assert the whole surface uniformly.

**Evidence.** `tests/validation/398_agent_dead_handle_matrix.spectra` (JIT and
AOT): 26 entry points against a released run, each required to answer
`unknown_handle`. Before the fix the fixture reported the three adapters as
failures.

## 3. `register_tool`'s malformed effect list degraded to “no effects”

**Observed while auditing the governance seam.** `tools::parse_effects` read the
compiler-emitted effect list with
`serde_json::from_str::<Vec<String>>(json).unwrap_or_default()`. A malformed
document therefore registered the tool with **no effects**, and
`enforce_run_grant` — the check that refuses a dispatch whose effects the run
does not authorize — then had nothing to check. Unknown authority was the one
outcome the invariant forbids, and it was the fail-open default.

**Fix.** A malformed list registers the tool with a single un-grantable marker
effect (`"<unreadable effects>"`). Registration still succeeds (the compiler is
the only emitter, so a malformed list is an emitter bug, not a reason to refuse
a module), but no grant can cover the marker, so the dispatch is refused with
`capability_denied` naming it.

**Evidence.** `tools::tests::a_tool_with_an_unreadable_effect_list_is_refused_by_every_grant`
registers a tool with a truncated list and asserts both the marker and the
refusal under a grant that covers `spectra.std` and `spectra`. With
`unwrap_or_default()` restored the test fails.

## 4. Silent wrong answers in coroutine scalar slots (two code-generation bugs)

Found by example 13, which is where a `Result<float, _>`-shaped API and a float
local crossing an `await` first appear together.

### 4a. A float payload in a generic enum did not compile

`fn f() -> Result<float, E> { Result::Ok(1.5) }` failed Cranelift verification:
`call fn3(...): arg 0 has type f64, expected i64`. The enum-construction path
emits `escape_manual_alloc` for each payload value, which lowers to
`spectra_rt_manual_escape(ptr: i64, frame: i64)`; the operand was passed as the
raw `f64`. Every non-`f64` scalar payload (`int`, `string`, `bool`) is already an
`i64` word, which is why only float payloads were unbuildable.

**Fix.** `backend/src/codegen_instruction_memory.rs`: the escape operand is
coerced to the `i64` word the runtime import declares (`CodeGenerator::scalar_word`
— an `f64` by its bit pattern, narrower integers widened). The runtime already
treats a value that is not a tracked allocation as a no-op, so a scalar payload
is unaffected beyond being passed correctly.

### 4b. A call's result read across a suspension became an integer

`let score = half(2)` — a float-returning call — followed by an `await` and then
a comparison compiled to `fcmp.f64 ge v1048, v1042` whose second operand was
`i64`: the coroutine's frame slot for `score` had been typed `Int`.

**Root cause.** Two type tables that feed frame-slot types neither of which
could see a call's result type:

* `lowering_impl_blocks.rs::find_assigned_variables_with_types` hints a slot's
  type from `syntactic_ir_type_hint`, which understands literals only, and
  `allocate_slot` then falls back to `IRType::Int`.
* `lowering_async.rs::collect_value_types` types values from the instructions in
  the body, where `Call`/`HostCall` carry no result type, and the frame layout
  (`AsyncFrameSlot`) resolves the slot type with
  `.unwrap_or(IRType::Int)`.

Inside a coroutine that slot *is* the value's storage, so the value came back as
an integer.

**Fix.** Both tables now learn the missing type: `slot_hint_for` prefers a
scalar declared annotation, then the literal hint, then the expression's
inferred IR type; `call_result_type_hints` supplies the callee's registered
return type and the host call's declared result type. Only scalar results are
hinted — aggregate and pointer results are `i64` words whose slots already carry
them, and re-typing them would move representations the fix has no reason to
touch.

### 4c. A value that is itself a frame load was reloaded from a slot that never existed

`count = count + zero(await one(1))` evaluated to `-1` instead of `0`.

**Root cause.** The uniform suspension pass reloads every operand a block uses
but does not define “from the slot named by the value's own id”, and documents
that “every slot is written at its value's definition site”. The store pass
skips `FrameLoad`/`StateLoad` results (“already frame contents and need no
write-back”), so a value that *is* a frame load has no slot of its own — and the
pre-suspension read of `count` was exactly such a value. The reload read an
uninitialized slot, and the local came back as garbage.

**Fix.** `lowering_async.rs` records where a reloadable value came from
(`ReloadSource::Frame(slot)` for a `FrameLoad` result, `ReloadSource::State` for
a `StateLoad` result) and re-reads it from *there* instead of from its own id.

**Evidence (4a–4c).**
`tests/validation/403_async_scalar_slots_and_float_payloads.spectra` pins all
three shape by shape — `Result<float, E>` in both variants, `Option<float>`, a
declared enum with a float payload, a float local across a suspension, and the
assignment that used to return `-1` — and runs in JIT and AOT. Each part was
minimized before the fix (`Result::Ok(1.5)` alone; a 12-line async function;
`count = count + zero(await one(1))`) and fails without it.

## 5. A delegated A2A task that crossed a ceiling reported `completed`

**Observed.** A serving run declaring `max_tokens: 2` delegated a task whose
prompt cost 22 tokens. The task document said
`"state":"completed"` with `"ceiling":""`, and the run report it carried held
`"status":"completed"` with 22 tokens spent.

**Root cause.** The delegated loop runs through the *crate's* `act`
(`protocol/a2a.rs::execute`), not through the async host that the local
`act(...)` call uses. The host path wraps the work in `write_run_task`, whose
worker settles the turn (`budget::settle_task`) and so marks a run that crossed
a ceiling; the adapter path had no such worker, and `execute` read
`state.cancelled` immediately after the loop — before anything had evaluated
the crossing. The adapter's own contract says the opposite: "a crossed ceiling
as a `failed` task naming it — never a silent success".

**Fix.** `budget::settle` now takes `Option<&CancellationToken>` (`cancel_siblings`
already did), `settle_task` passes its token, and a new `budget::settle_turn`
settles a turn for a caller that has none; `execute` calls it before reading
the outcome. Writing the regression also showed the local path was already
covered — `agent_end` reported the ceiling because a later call's guard marked
the run — which is exactly why the delegated path hid the defect.

**Evidence.**
`a2a::tests::a_delegated_task_that_crosses_a_ceiling_fails_naming_it` fails
with the `settle_turn` call commented out and passes with it; the same
behaviour is pinned from the language by fixture 407 (`failed` naming
`max_tokens`, with the report's `ceiling` agreeing) and example 16.

## Also recorded, not changed

* `ChunkStream` handles are not released by `agent_end`: a stream keeps the
  chunks the run already produced (its work was paid for when the stream
  opened) until it is drained or closed. Fixture 400 pins the boundary.
* `acp_permission`'s granted branch is unreachable from a Spectra program: the
  ACP client seam (`set_acp_client`) is host-only by design, so a language-level
  caller can only observe the default-deny. Fixture 382 exercises the denial and
  example 14 documents the two bridges (ACP client vs host approver) side by
  side.
* The mock's trailing-`spectra:final=` replay arm is unreachable through `act`
  (the loop ends at the first final; each call starts its script at index 0).
  Documented in fixture 402 rather than pinned.
* `budget.rs::charge_tool_call` carried a stale `#[allow(dead_code)]` and a
  comment claiming it was “dead until the dispatch path lands (R-3222)”, which
  landed. Removed; the function is live and load-bearing.
* A tool's effects are derived from its body, so a tool that formats its own
  errors with `error.message` must have `spectra.std.error` granted before the
  run's first dispatch — `enforce_run_grant` refuses the whole run otherwise,
  naming that effect. Found while writing fixture 407; documented in example
  16's tool and in the governance section of the book chapter rather than
  changed, because denying a run whose tool has effects outside its grant is
  the invariant, not a bug.
* The A2A task's request and terminal records live in the *task's* journal
  (keyed by the task id, which is the delegated run's id), not in the serving
  run's. A fixture that asserts "the work happened" therefore has to clear the
  task journals, not the serving one — reusing them replays the recorded tool
  step instead of executing it, which is correct semantics and a wrong starting
  state.

## Found and documented, not fixed: an AOT symbol collision

Writing example 16 and fixture 407 turned up a linkage limitation:

```
ex-16-a2a-task-lifecycle.exe : fatal error LNK1169
spectra_api-<hash>.lib(ws2_32.dll) : error LNK2005: send já definida no module-0000.obj
```

A user function is emitted with `Linkage::Export` under its **bare IR name**
(`backend/src/aot.rs::declare_function`, whose only rename is
`main` → `spectra_user_main`). A program that declares `func send(...)`
therefore defines a symbol the runtime library already imports from winsock on
Windows, and the link fails. The same class reaches any libc/winsock name
(`recv`, `connect`, `bind`, `listen`, `accept`, `select`, `read`, `write`, ...).

Two reasons it is documented rather than fixed here:

* the faithful fix is to mangle *every* user symbol (a reserved prefix, as
  `spectra_user_main` already does for `main`). That is a linkage-wide change,
  and `scripts/validate_r2903_native_debug.py` pins the current names -- it
  asserts `helper` and `spectra_user_main` appear in the PDB symbol stream --
  so the change needs its own validation pass over the native-debug gate, not a
  drive-by edit;
* the failure is loud and late rather than silent and wrong: the linker names
  the symbol and the object, and the workaround is a rename.

Both new files therefore call their helper `send_request` (the naming examples
07 and 382 already used), and the example's header says why.

## Validation

| Check | Result |
|---|---|
| `cargo test -p spectra-agent` | 148 unit + 8 conformance tests pass |
| `spectralang compile` over `tests/validation/*.spectra` | 408 files, 0 failures |
| New examples `13`–`17` and fixtures `398`–`408`, JIT and AOT | pass (registered in the R-3221 gate) |
| `cargo test -p spectra-agent --lib mcp::server::tests::a_stalled_connection…` | passes with the fix, fails with the pre-fix arms |
| `cargo test -p spectra-agent --lib tools::tests::a_tool_with_an_unreadable…` | passes with the fix, fails with `unwrap_or_default()` |
