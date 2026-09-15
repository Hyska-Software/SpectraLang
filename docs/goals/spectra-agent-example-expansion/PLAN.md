# std.agent Example and Verification Expansion Plan

**Intent:** Expand the executable `std.agent` corpus with realistic projects and narrow `.spectra` contract fixtures, then fix any implementation defect the new scenarios expose.
**Current Behavior:** `examples/agent/01..37` and `tests/validation/384..428` cover the current agent surface, but cross-module usage is only represented by an internal project and the process-global remote-tool registry has no language-level isolation scenario.
**Expected Outcome:** Add examples `38..42` and fixtures `429..432`, register every artifact in the certification gate, document the observable contracts, and leave the new scenarios passing in JIT and AOT.
**Target-Perspective Output:** A contributor can run each new example with `spectralang run` or AOT and see deterministic output; each fixture exits non-zero on a contract regression and is executed by R-3221.
**Truth Owner:** `compiler/src/semantic/builtin_std_core.rs` owns the source-visible `std.agent` signatures; `packages/spectra-agent` owns runtime behavior; `scripts/validate_r3221_agent_conformance.py` owns example/fixture registration.
**Contract Boundary:** Spectra programs call `std.agent` through the compiler lowering and host-call ABI; tools cross the boundary through derived JSON wrappers; remote MCP descriptors cross it as run-scoped governed registry entries.
**Cutover:** Extend the existing example/fixture maps and documentation; add no second runner or parallel registry path. If remote visibility is process-global today, scope it to the run that discovered it while preserving the existing shared registry and grant checks.
**Displaced Path:** Stale notes claiming that cross-module dispatch or source-visible tool-call ceilings are unavailable; those comments must be updated after the new examples prove the current path.
**Value Density:** Prefer scenarios that exercise a distinct boundary: cross-module wrappers, remote-tool visibility across runs, empty stream termination, protocol defaults/error handling, and escaped Unicode JSON payloads.
**Acceptance Evidence:** Every new example and fixture passes JIT and AOT under the R-3221 gate; the remote-scope fixture proves a later run with no MCP grant can still invoke its local tool; backend/runtime regressions cover the changed registry behavior; findings document the reproduced defect and correction.
**Evidence Lane:** `cargo test` for changed Rust crates, `cargo build -p spectra-cli`, direct JIT/AOT executions, `python scripts/validate_r3221_agent_conformance.py --jobs 1` for the final serial certification run (bounded parallel mode remains available), and targeted reports from each new scenario.
**Kill Criteria:** Do not add a new example if it only repeats an existing contract; do not keep a global remote descriptor visible to unrelated runs; do not claim protocol behavior without an executable `.spectra` assertion.
**Non-goals:** No new provider, protocol version, public host-call signature, roadmap status change, or interactive network dependency.
**Risk if wrong:** A plausible example can hide a process-global capability leak, AOT-only registration failure, or JSON lifetime/escaping bug that unit tests do not exercise through the language.
**Architecture Slice:**

- **Files to create:** `examples/agent/38-cross-module-tools/**`, `39-remote-scope-isolation/**`, `40-empty-stream-boundary/**`, `41-a2a-card-defaults/**`, `42-unicode-tool-payload/**`; `tests/validation/429_agent_remote_scope.spectra` through `432_agent_unicode_tool_payload.spectra`; `docs/goals/spectra-agent-example-expansion/GOAL.md`; `docs/goals/spectra-agent-example-expansion/FINDINGS.md` when a defect is reproduced.
- **Files to avoid:** generated contract outputs, unrelated API examples, roadmap files, and broad workspace formatting changes.
- **Source of truth:** Existing `std.agent` bindings, compiler signatures, ADR 0018/0019, and current runtime behavior; examples and fixtures assert the consumer-visible contract.
- **Read path:** source project -> compiler lowering -> synthesized tool registration -> `spectra.std.agent.*` host calls -> runtime registry/provider/journal.
- **Write path:** new source projects and fixtures first; implementation changes only when a failing consumer scenario identifies a real defect; documentation follows verified behavior.
- **Integration points:** example/fixture maps, JIT/AOT gate, JSON derive wrappers, MCP discovery, `act`/`tool_call`, stream handles, A2A card validation.
- **Acceptance evidence gate:** R-3221 report has all checks passed and the new names appear in the example/fixture evidence; no failing direct reproduction remains.

**Plan Review Gate:** Requires PRE review before execution.

## Tasks

1. Map current agent signatures, examples, fixtures, and gate registration.
2. Add cross-module and boundary-focused executable examples 38 through 42.
3. Add verification fixtures 429 through 432 with distinct assertions.
4. Run new scenarios and isolate any implementation failure.
5. Document each reproduced defect with a concrete reproducer and root cause.
6. Correct implementation defects and add permanent regression coverage.
7. Update catalogs and docs to match the verified surface.
8. Run crate tests, JIT/AOT examples, and the full certification gate.

Parallelism: examples whose source trees are independent may be authored together; implementation fixes and gate registration are serialized after the first reproducer result. Validation runs only after all edits are complete.
