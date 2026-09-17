# Tail-Call Recursion Codegen Failure (Known Failure)

Status: **open** — valid recursive programs are rejected by the backend or crash the compiler.

Found: 2026-09-16 · CLI 0.3.4 (nightly) · commit `5a08bf9d` · `cranelift-frontend 0.130.0`.

## Scope

Backend Cranelift function building (`backend/`, via `cranelift-frontend`).
Semantic analysis accepts these programs: `spectralang check` itself emits the
`error[codegen]` diagnostics below, so the failure fires on every CLI path
(`check`, `run`, `compile`, `--emit-object`).

## Failing shapes

### A. Tail call as the whole return value in `if` without `else` → exit 65

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

```powershell
spectralang check repro_a_tail_if_no_else.spectra
```

```text
error[codegen]: Failed to define function 'helper': Compilation(Verifier(VerifierErrors([VerifierError { location: inst3, context: None, message: "invalid block reference block2" }])))
```

### B. Single-parameter tail recursion, same shape → compiler panic, exit 101

```spectra
module repro_b
import std.io

func down(n: int) returns int {
    if n > 0 {
        return down(n - 1)
    }
    return 0
}

public func main() returns int {
    println(f"d={down(5)}")
    return 0
}
```

```text
thread 'main' panicked at cranelift-frontend-0.130.0/src/frontend.rs:692:21:
FunctionBuilder finalized, but block block2 is not filled
```

No diagnostic is emitted; the process aborts with exit code 101.

### C. Array indexed store + recursive call in one branch → exit 65

Even with `if`/`else`, an indexed store to an array parameter in the same
branch as the recursive call fails:

```spectra
module repro_c
import std.io

func empilha(n: int, pilha: [int], topo: int) returns int {
    if n > 1 {
        pilha[topo] = n
        return empilha(n - 1, pilha, topo + 1)
    } else {
        return topo
    }
}

public func main() returns int {
    let pilha = [0, 0, 0, 0, 0, 0, 0, 0]
    println(f"t={empilha(5, pilha, 0)}")
    return 0
}
```

```text
error[codegen]: Failed to define function 'empilha': Compilation(Verifier(VerifierErrors([VerifierError { location: inst3, context: None, message: "invalid block reference block3" }])))
```

Each half in isolation compiles: an indexed store inside `if` with a plain
trailing return works, and a non-recursive call in the same position works.
Only the store + recursive-call combination fails.

## Shapes that work (controls, verified on the same toolchain)

- Non-tail recursion: `return n * fat_rec(n - 1)` with a trailing `return 1`
  (the shape used by `examples/fibonacci.spectra` and `console_demo.spectra`).
- Pure-scalar tail recursion with `if`/`else` and a return in **both** branches
  (no trailing return after the `if`).
- Recursion threading `List<T>` handles (`std.collections`) instead of array
  parameters, with strict `if`/`else` returns.
- `unit`-returning recursion with the recursive call as a statement plus a
  bare `return`.

The `PythontoSpectra/*_sem_laco.spectra` conversions were written against the
working shapes (recursive `List<int>` stack, strict `if`/`else`).

## Hypothesis (unconfirmed)

A branch that ends in a call-as-return-value never seals/fills its successor
block in the Cranelift builder, so verification fails (`invalid block
reference`) or finalization panics (`block is not filled`). The array-store
variant suggests the store + call instruction sequence leaves the fallthrough
block unreferenced the same way. Requires backend minimization to confirm.

## Impact

- Valid programs rejected (exit 65) or compiler crash (exit 101, no diagnostics).
- Blocks the natural accumulator-recursion style and array-backed explicit
  stacks inside recursive functions; workarounds exist (see above).

## Suggested follow-up

- Backend-owner minimization and fix in block sealing for call-terminated branches.
- Add `tests/validation/` regressions for shapes A–C once fixed (they must stay
  out for now: they fail `check`).
- Re-evaluate the `PythontoSpectra/*_sem_laco.spectra` workarounds after the fix.
