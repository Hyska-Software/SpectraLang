# Goal: Expansão da Standard Library `.spectra` — Onda 2 (Lotes 5–6)

Use Krypton Execution to execute `docs/goals/spectralang-stdlib-expansion-wave2/PLAN.md`.

Core rules:
- Treat PLAN.md as the source plan; preserve intent, ownership, contract,
  cutover, evidence, and kill criteria. A PRE review already ran
  (`partially aligned`) and every finding is addressed in
  §PRE review disposition — no blocker remains open.
- Order: Wave 0 (linguagem: T-01a/b/c/d bitwise, T-02 panic, T-03 stack budget,
  T-04 contrato de bytes) → Wave 1 (`std.unicode`, `std.bytes`, dedupe T-12) →
  Waves 2–3 (10 expansões + 6 módulos novos) → Wave 4 (baseline + gates).
- Parallelism model: a worker owns only `stdlib/src/<módulo>.spectra` and its
  fixture; a single integration owner serializes `scripts/stdlib_contract.toml`,
  catalog regeneration, lowering-table regeneration, `docs/reference/05-stdlib.md`
  and the ledger, then runs the wave gate.
- Do not add a new dominant path without deleting, redirecting, demoting, or
  shimming the displaced path (private UTF-8 helpers in T-12; base-2 arithmetic
  workarounds named in the plan's Cutover).
- Capture acceptance evidence from the target perspective: fixtures JIT+AOT
  (808..828), `--require-catalog` with 0 blockers, regenerated catalog, docs+ledger,
  `tests/execution-baseline.json` recorded, and for `panic`/`std.testing` the
  three exit paths (JIT, AOT, `spectralang package test`).
- Say "implemented but unproven" if that evidence cannot be captured.

Known non-goals: `std.regex`, `std.toml`, `std.json` core, `std.crypto`
(SHA-256/HMAC — Onda 3), a new `bytes` type, native modules
(fs/io/env/random/concurrent/serve/tensor/ml/net/TLS), and rewriting existing
native kernels.
