# Goal: std.agent Example and Verification Expansion

Use the execution plan in `docs/goals/spectra-agent-example-expansion/PLAN.md`.

Core rules:
- Expand `examples/agent` and `tests/validation` with distinct, deterministic contracts.
- Treat compiler signatures and `packages/spectra-agent` runtime behavior as the truth owner.
- Reproduce and document any real defect before fixing it; keep the fix on the actual runtime/compiler path.
- Preserve JIT/AOT parity and register every new artifact in the R-3221 gate.
- Finish with direct scenario evidence and the full certification report.
