# An aggregate result outlives its coroutine frame

Status: **fixed** (found 2026-09-12 while adding the Phase 32 verification
fixtures; fixed in `midend/src/lowering_async.rs`, regression test
`tests/validation/388_async_aggregate_result_lifetime.spectra`).

The defect was a memory-safety bug, not a diagnostic: a caller could read a
reused block and see a wrong value, or follow a stale pointer and die with
`0xC0000005` (`STATUS_ACCESS_VIOLATION`).

## Symptom

An aggregate returned from an `async func` and read after the await could be
garbage. Ordinary code was affected:

```spectra
record Quote {
    symbol: string
    price: float
}

async func make(symbol: string) returns Quote {
    return Quote { symbol: symbol, price: 512.5 }
}

public func main() returns int {
    let quote = block_on(make("SPX"))
    if quote.symbol != "SPX" { return 3 }   // AOT: intermittent crash (3 of 30 solo runs)
    if quote.price != 512.5 { return 4 }
    return 0
}
```

The agent tool surface hit it too (a tool returning a record, dispatched by
`tool_call`), which is how the certification gate found it: its
`verification_fixtures_jit_and_aot` check failed about half the time before the
fix. JIT runs survived (the released block stayed mapped); AOT runs faulted.

## Cause

1. An aggregate local inside a coroutine lives in storage the coroutine owns:
   the backend lowers an `Alloca` inside a poll to `RuntimeImport::
   CoroutineLocalPtr`, and the runtime hands back a block from the frame's
   `locals` (`AsyncFrame::local_ptr`).
2. `return <aggregate>` makes that block the coroutine payload, which
   `finish_async_coroutine` publishes through `CoroutineComplete` into the task
   registry.
3. On completion the frame is dropped, which releases the `locals` block — but
   the task record still exposes the pointer to whoever reads the result
   (`spectra.async.task.result`, `block_on`). Nothing escapes the payload,
   because the payload never travels through an ordinary `Terminator::Return`
   — and escaping the root return pointer is exactly what the backend does for
   every ordinary function before its manual frame exits.

Scalar results (`int`, `float`, `bool`) and strings were unaffected: a scalar
travels by value, and a string buffer is a tracked manual allocation the caller
frame still owns when it escapes the value.

## Fix

`ASTLowering::materialize_coroutine_payload` copies an aggregate payload into a
tracked manual allocation and escapes it, immediately before the payload is
published to the task registry:

- `Alloca`-shaped payloads are copied word by word into a `ManualAlloc` block
  (`Struct`, `Tuple`, `Array`, `Enum`; scalars, strings and fat pointers travel
  as they are).
- The copy is escaped with `EscapeManualAlloc`, which re-parents it onto the
  base frame — the same guarantee the backend gives a returned string, so the
  value the caller reads outlives the coroutine that produced it.

The escaped copy is process-lifetime, like every other escaped value, and is
counted against the runtime's frame-0 budget (`SPECTRA_FRAME0_BUDGET_MB`,
default 512, `0` disables) — so a long-lived loop that awaits aggregates fails
loudly at the ceiling instead of silently exhausting memory. A future release
seam (`task.release`-style, emitted after the caller has consumed the result)
could hand the copy back early; nothing depends on the current retention.

## Ownership across a frame boundary (same class, also fixed)

The same rule applies to every value a function owns, not only to the coroutine
payload: a function that builds aggregates gets a manual allocation frame, and
the frame exit releases everything allocated inside it. Escaping the root return
pointer only preserves allocations that are *tracked*; the walk now follows the
reachable graph and copies what the frame owns.

Fixed in the same change:

- **Strings.** A string built in the callee and returned as a field, as an
  `Option`/`Result` payload, or as the value itself is escaped with the value
  (`emit_escape_for_value` now walks `String`).
- **Enum payloads.** `Result<T, E>` / `Option<T>` payload slots are escaped, and
  the enum block is copied out of the frame.
- **Nested aggregates.** A coroutine payload is materialized recursively:
  structs, tuples and arrays reachable from it are copied into tracked storage
  (`ASTLowering::materialize_payload`), so `async func f() returns Outer { inner:
  Inner { .. } }` reads live memory. Before this, that shape was wrong in 30 of
  30 runs.
- **Containers.** `collections.list_push`, `list_set`, `list_insert_at`,
  `map_set` (both the generic host call and the fast ABI entry) and the fast map
  store escape the value they store: a container outlives the frame that pushed
  into it.

Fixture: `tests/validation/389_string_and_container_payload_lifetime.spectra`
(covers all four shapes behind deliberate allocation churn).

## Findings fixed in the same change

1. **An enum payload holding a frame-owned aggregate.** `Result<Big, int>`
   returned from an `async func` (with `Big` built in the coroutine) read a
   released block about a third of the time. `materialize_payload` now guards
   each variant's aggregate payload with the tag: only the variant the tag
   names holds a live aggregate, and only that one is copied; the completion
   tail continues in the block the materializer ended in. Pinned by
   `tests/validation/388_async_aggregate_result_lifetime.spectra`.
2. **String-keyed map/set/list lookups with a literal key.** See the section
   above: JIT literals are tracked and AOT literals are registered as borrowed.
   Pinned by `389_string_and_container_payload_lifetime.spectra` and the
   runtime test `registered_literals_are_readable_but_never_freed`.

## Declared generic instantiations (investigated, no defect found)

The investigation produced one scare worth recording: an escape trace showed a
declared `Result<string, int>` lowered as `Result_int_int`, which would have
made the payload layout disagree with the signature. It does not reproduce on
the current tree, and `spectralang run --dump-ir` shows the declared
instantiation for every shape that was suspected:

```text
fn build_result(int seed) -> Result<string, int> [enum Result_string_int] {
fn build_option(int seed) -> Option<string> [enum Option_string] {
fn build_async(int seed) -> Task<Result<struct Big, int> [enum Result_Big_int]> {
```

That includes the sync and coroutine forms, the value bound to a `local` before
matching, and a record payload larger than one word. The intended default for
an *unobserved* constructor parameter (`Option::None`, `Result::Ok(value)`) is
still `int`, so a future change in `fill_builtin_enum_defaults` or in
call-site instantiation is worth re-checking against these three dumps.

## Evidence

`tests/validation/388_async_aggregate_result_lifetime.spectra` awaits records
(single, nested, and one produced by a coroutine that awaited another), churns
same-sized allocations so a premature release is visible, and then reads every
field. AOT, 60 runs under load with two neighbours:

| build | result |
|---|---|
| before the fix | **10 passed / 50 failed** |
| after the fix | **60 passed / 0 failed** |

The plain reproduction above went from 3 failures in 30 solo runs (and 1 in 30
loaded) to 0 in 40.

Debug traces that localized the defect (instrumentation since removed):

```text
# passing run
task_result: task=... -> Ok(1444676404224) first_word=1444663705616   # live string pointer
encode_struct: args=[kinds, "symbol", 1444663705616, "price", 4647719213492862976]

# failing run (same program, same command)
task_result: task=... -> Ok(1503052077216) first_word=8               # block already reused
encode_struct: args=[kinds, "symbol", 8, "price", 1503039643344]
```
