# Tail-Call Recursion Codegen Failure (Resolved)

Status: **resolved** — the per-block tail-call terminator bug was fixed and
covered by regressions.

Found: 2026-09-16 · CLI 0.3.4 (nightly) · commit `5a08bf9d` · `cranelift-frontend 0.130.0`.
Fixed: 2026-09-16 · same toolchain line.

## Original failure

Backend Cranelift function building (`backend/`) rejected valid recursive
programs or crashed the compiler. Semantic analysis accepted them: the
`error[codegen]` diagnostics below fired on every CLI path (`check`, `run`,
`compile`, `--emit-object`).

### A. Tail call as the whole `return` value in `if` without `else` → exit 65

```spectra
module repro_a
import std.io

func helper(n: int, acc: int) returns int {
    if n > 1 {
        return helper(n - 1, acc + n)
    }
    return acc
}

public func main() returns int {
    println(f"h={helper(5, 0)}")
    return 0
}
```

```text
error[codegen]: Failed to define function 'helper': Compilation(Verifier(VerifierErrors([VerifierError { location: inst3, context: None, message: "invalid block reference block2" }])))
```

### B. Single-parameter tail recursion, same shape → compiler panic, exit 101

```text
thread 'main' panicked at cranelift-frontend-0.130.0/src/frontend.rs:692:21:
FunctionBuilder finalized, but block block2 is not filled
```

No diagnostic was emitted; the process aborted with exit code 101.

### C. Array indexed store + recursive call in one branch → exit 65

With `pilha[topo] = n` in the same branch as the recursive call (even with
`if`/`else`), Cranelift reported `invalid block reference block3`.

## Root cause

`emitted_tail_call` in `backend/src/codegen_block.rs` was a function-scoped
flag. A block that emitted Cranelift's native `return_call` set it to `true`,
and `generate_block` skipped the IR terminator whenever the flag was set. The
flag was only cleared at the start of the next `Call` instruction, so every
later block **without a call** — for example the `if.merge` return that follows
a tail-recursive `if.then` — also skipped its own terminator and was left
unfilled.

The two observed failure modes were the same bug:

- when the unfilled block had instructions (e.g. a `ConstInt 0` before the
  return, as in shape B), `FunctionBuilder::finalize` hit its debug assertion
  and panicked;
- when the unfilled block only had the terminator (shape A), the block stayed
  pristine, finalization passed, and `module.define_function` failed Cranelift
  verification with `invalid block reference`.

Shape C failed for the same reason: the store instruction does not reset the
flag, only a `Call` does.

## Fix

`backend/src/codegen_block.rs`: reset `*emitted_tail_call = false` at the start
of every block, so the flag means "this block already received a native
`return_call` terminator" instead of "some block in this function did".

Both generators share `CodeGenerator::generate_block`, so the JIT path
(`codegen_core.rs`) and the AOT path (`aot.rs`) are fixed together. Tail-fusion
marking (`midend/src/passes/tail_call_marking.rs`) already guarantees the
marked call is the last instruction of a block whose terminator returns its
result, so skipping that terminator remains correct.

## Regressions

- `backend/src/codegen_tests.rs::tail_call_block_does_not_suppress_later_block_return`
  builds the midend block order (tail-call `step` block before the plain-return
  `base` block), asserts the finalized IR still contains exactly one
  `return_call` plus the base `ret`, and runs 100k recursion levels to prove
  the native tail call survives. Reverting the fix makes this test fail with
  the original `invalid block reference block2` error.
- `tests/validation/506_tail_call_recursion_shapes.spectra` covers shapes A, B,
  and C plus a 200k-level deep case; it compiles, lints, formats clean, runs
  under the JIT, and passes as an AOT executable.

## Validation evidence

- `cargo test -p spectra-backend` — 57 passed.
- `spectralang check`, `lint`, `fmt --check` on the new regression — clean.
- `spectralang run tests/validation/506_tail_call_recursion_shapes.spectra` —
  exit 0; `compile --debug-info=none --emit-exe` + produced binary — exit 0.
- The `PythontoSpectra/*.spectra` conversions that motivated the report now
  run byte-identical to their Python originals under JIT and AOT
  (`fatorial_pilha 20 --traco`, `fatorial_pilha_sem_laco 20 --traco`,
  `fibonacci_pilha_sem_laco 30 --traco` and `92`).
