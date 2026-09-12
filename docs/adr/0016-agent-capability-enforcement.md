# ADR 0016: Agent Capability Enforcement

Status: Accepted

Date: 2026-09-12

Roadmap item: R-3201

## Context

Phase 32 introduces governed agent runs: a `std.agent` code path where every
effect the program performs is attributable to an active run and bounded by
that run's capability grant. The repository has no capability model today.
There is exactly one place where such a model can be total: host-call
dispatch. The language routes every external effect through a host call, and
every host call passes through the runtime dispatch entrypoints.

Two execution paths reach host calls:

- the **generic path**, where the runtime resolves a registered `extern "C"`
  function by name or through a module-owned cache slot and invokes it; and
- the **fast path**, the 28 `FastHostCall` variants declared in
  `runtime/src/abi.rs:439-501`, which the backend lowers directly to dedicated
  runtime imports (`backend/src/codegen_instruction_host.rs:24`).

The fast path exists for in-process compute (arithmetic-shaped string,
collection, tensor and concurrency operations). Several fast calls return raw
`i64` values through their own ABI rather than a `SpectraHostCallContext`
status; they have no channel through which a policy decision could be
reported. The generic path has a status channel, but its cached variant never
consults the registry (`runtime/src/ffi_host_registry.rs:78-101`) and its
batch variants dispatch several calls behind one outer `catch_unwind`.

Enforcement cannot be bolted onto one entrypoint. A policy that runs on the
uncached single-call path but not on the cache hit, the batch, or the cached
batch is bypassable by construction. This ADR freezes the enforcement model
before any enforcement code exists.

## Decision

### D2 — Enforcement applies to the generic dispatch path only

Capability policy is evaluated for every **generic** host call. Fast host
calls are excluded by construction because they are in-process compute with no
observable external effect and no denial channel.

The exclusion is not a promise; it is pinned by a completeness test. Invariant
I1 states that no host call touching the outside world is in the fast path.
`runtime/src/abi.rs` gains `fast_host_call_effect_namespace`, which iterates
`FastHostCall::ALL`, asserts the catalog matches `FastHostCall::COUNT`, and
fails if any `host_name()` falls under an effect-bearing namespace. The test
carries an exhaustive `match` over every `FastHostCall` variant with no
wildcard arm, so adding a variant forces a compile error until the variant is
classified.

Effect-bearing namespaces are the host-call prefixes that reach outside the
process: `spectra.api.*`, `spectra.agent.*`, `spectra.async.*`,
`spectra.std.env.*`, `spectra.std.fs.*`, `spectra.std.io.*`,
`spectra.std.random.*`, `spectra.std.serve.*` and `spectra.std.time.*`.
`spectra.agent.*` is included preemptively, before the namespace lands. The
list is derived from the registered host functions in
`runtime/src/stdlib/registration.rs` and
`runtime/src/stdlib/stdlib_bindings.rs`.

### D3 — One extracted dispatch function

Policy is evaluated by one function:

```text
dispatch_generic(name: &str, func_ptr: *const (), args: *const SpectraHostValue,
                 arg_len: usize, results: *mut SpectraHostValue, result_len: usize) -> i32
```

`dispatch_generic` wraps the existing `invoke_host_function`
(`runtime/src/ffi_host_registry.rs:162-186`), consults the active run context,
evaluates the capability decision once, and either returns
`HOST_STATUS_DENIED` or delegates to the host function.

All four generic entrypoints in `runtime/src/ffi_lifecycle.rs` call it:

| Entrypoint | Line | Path |
| --- | --- | --- |
| `spectra_rt_host_invoke` | `:280` | uncached single |
| `spectra_rt_host_invoke_cached` | `:311` | cached single |
| `spectra_rt_host_invoke_batch` | `:344` | uncached batch |
| `spectra_rt_host_invoke_cached_batch` | `:387` | cached batch |

Policy therefore runs on a cache hit and per batch item, never once per batch.
`dispatch_generic` must not panic inside the outer `catch_unwind`; a denied
call returns a status like any other host failure, and the batch dispatchers
stop at the first non-success status exactly as they already do.

### D4 — Denial contract

Denial is reported as `HOST_STATUS_DENIED`, a new status code added beside the
existing four in `runtime/src/ffi_core.rs:320-323`
(`SUCCESS` 0, `INVALID_ARGUMENT` 1, `NOT_FOUND` 2, `INTERNAL_ERROR` 3). The
lowering already turns any non-zero status into a fatal trap
(`backend/src/codegen_instruction_host.rs:629-657`); the dedicated code makes
that trap distinguishable from an ordinary host failure.

Alongside the status, the runtime exposes a dedicated fatal runtime symbol
registered in the `RuntimeImport` catalog
(`runtime/src/abi.rs`, count at `:132`). The lowering emits a call to that
symbol on `HOST_STATUS_DENIED`, so the process terminates with a structured
message instead of the generic panic text.

The denial message is machine-greppable and contains only:

- the host-call name that was denied;
- the run goal (`AgentSpec.goal`);
- the capability that would have allowed the call;
- the run's allowed capability set.

It never contains secrets, request or response payloads, headers, prompts or
model output. The format is frozen here so denial tests can assert on it
without parsing free-form prose.

Callers that want a decision before performing work, rather than a trap, use:

```text
authorize(run, host) -> Result<bool, Error>
```

`authorize` reports the same decision `dispatch_generic` enforces. It is a
convenience over the same policy evaluation, not a second policy engine, and a
test asserts the two agree.

Because there is no typed error channel on the lowering path, everything a run
may need to handle gracefully must be expressed as host-call return values,
not as `HOST_STATUS_DENIED`. The denial code is for effects the run was never
granted, which are programming errors in a well-formed program.

### D5 — Run context stacking and detached work

The active run is a stacking thread-local, mirroring the tracing context
(`runtime/src/tracing/mod.rs:197-200`), with a guard type that restores the
previous entry on scope exit. Nested runs push a new frame; a nested run
inherits the intersection of the enclosing run's capabilities and its own
grant, so a nested run can never widen authority.

The repr(C) `SpectraHostCallContext` is unchanged. No ABI change is required
or permitted; the context is a separate, thread-local concept.

Propagation to workers is explicit and uses the proven tracing pattern:
capture the context at spawn, restore it inside the worker, clear it on
completion (`runtime/src/tracing/mod.rs:492-508`, used at
`runtime/src/stdlib/async_task_stream.rs:314-328`). Coroutine resumption
restores the context too.

Detached work is fail-closed. Spawning work that does not inherit the run
context from inside a run requires its own explicit capability grant; without
one the spawn returns a typed error. A propagation gap must surface as a
visible failure, never as a silent bypass in which a host call executes with
no active run.

### D14 — Capability vocabulary

A capability is a host-call namespace prefix:

```text
std.api.client.request
std.api.client.request:host=api.example.com
```

The vocabulary is exactly the set of registered host-call paths, which the
contract catalog already knows. A grant that matches no registered host call
must fail the build; a permission system whose most common failure is a grant
that silently matches nothing is worse than no permission system.

Grants are validated at compile time:

- `E3201` — unknown capability. The diagnostic uses the existing
  did-you-mean/Levenshtein path (`semantic_item_import.rs:85-99`) to suggest a
  near match.
- `E3202` — unsupported scope form. A scoped capability
  (`prefix:key=value`) is accepted only where the host call registers an
  extractor for that scope key. Scope keys come from the catalog's
  `scope_keys` field (ADR 0017).

The vocabulary and its error codes are allocated here; `R-3215` implements the
validation.

## Rationale

**Why the generic path only.** The fast path cannot express denial: it returns
raw values with no status, and its calls are pure in-process compute. Routing
them through policy would require an ABI change, a per-call policy lookup on
the hottest path, and a way to report a decision that does not exist. Excluding
them is honest, and the exclusion is enforced by a test rather than by
convention. If a future fast call would touch the outside world, the invariant
test fails and the call must move to the generic path.

**Why one function.** The four entrypoints differ only in how the function
pointer is obtained and whether one or many calls are dispatched. The decision
itself must be identical. Extracting `dispatch_generic` makes "one decision,
four call sites" structurally true and gives enforcement exactly one place to
change. Running policy on the cached path is mandatory because the cached path
resolves pointers without consulting the registry; running it per batch item
is mandatory because a batch is a sequence of separate effects.

**Why a status plus a symbol.** The lowering has no typed error channel on the
host-call path and already traps on any non-zero status. Adding a distinct code
costs nothing and makes the trap diagnosable; the fatal symbol carries the
structured context the trap cannot. Two paths, one decision.

**Why a thread-local.** There is no ambient run identity today, and threading a
run parameter through the ABI would change the repr(C) context that every host
call and the AOT shim depend on. The tracing context proves the stacking
thread-local pattern works, including explicit worker propagation, at the cost
of requiring propagation discipline. Fail-closed detached work converts the
weakness of thread-locals (loss across threads) into a loud error.

**Why catalog-derived grants.** The catalog already enumerates every host call
the runtime can dispatch. Deriving the vocabulary from it means grants cannot
drift out of sync with the surface, and the compiler can reject a grant that
would never match.

## Consequences

- `R-3213` lands the run context with the stacking and propagation semantics
  above; enforcement depends on it.
- `R-3214` extracts `dispatch_generic`, routes the four entrypoints through it,
  adds `HOST_STATUS_DENIED` and the fatal symbol, and implements
  `authorize(run, host)`.
- `R-3215` implements compile-time vocabulary validation and emits `E3201` and
  `E3202`.
- `R-3214` must prove I2: a host call outside the grant fails through all four
  generic entrypoints, including cached and batch.
- `R-3214` must prove I5: programs without an active run behave exactly as
  before this workstream; the enforcement path is inert when no run is active.
- The fast-path invariant test from `R-3201` remains the guard against a
  new fast call acquiring an external effect.
- No fast host call receives a capability check, and none may be moved into the
  fast path once it has an external effect.

## Rejected Alternatives

### Enforce on every host call including the fast path

Rejected. Fast host calls do not carry a `SpectraHostCallContext` status in
every variant, several return raw `i64` results, and their lowering is chosen
precisely to avoid dispatch overhead. There is no correct way to report denial,
and forcing one would change the ABI and slow the compute path.

### Thread the run as an explicit host-call argument

Rejected. It changes the repr(C) `SpectraHostCallContext` contract shared with
the backend, the AOT shim and every registered host function. The thread-local
mirrors the proven tracing model at no ABI cost.

### A separate policy check inside each host function

Rejected. It duplicates one decision across hundreds of functions, drifts, and
cannot be proven complete. Enforcement belongs at the dispatch seam.

### Free-form capability strings validated at runtime

Rejected. A grant that matches nothing is the dominant failure mode of
permission systems. The compiler already has the catalog; validation belongs at
build time.
