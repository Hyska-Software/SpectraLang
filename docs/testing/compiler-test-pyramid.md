# Compiler Test Pyramid

Updated: 2026-09-11  
Roadmap item: `R-104`

## Goal

Phase 1 requires a production-oriented test shape for the compiler stack. The goal is not only to have passing tests, but to make regressions land in the smallest possible layer.

## Test Layers

### 1. Stage-local unit tests

Purpose: validate a subsystem in isolation with tight failure localization.

Current crate coverage:

- `compiler`
  - lexer unit tests in [D:\Lang\SpectraLang\compiler\src\lexer\mod.rs](D:\Lang\SpectraLang\compiler\src\lexer\mod.rs)
  - parser unit tests in [D:\Lang\SpectraLang\compiler\src\parser\mod.rs](D:\Lang\SpectraLang\compiler\src\parser\mod.rs)
  - workspace parser tests in [D:\Lang\SpectraLang\compiler\src\parser\workspace.rs](D:\Lang\SpectraLang\compiler\src\parser\workspace.rs)
  - pipeline unit tests in [D:\Lang\SpectraLang\compiler\src\pipeline.rs](D:\Lang\SpectraLang\compiler\src\pipeline.rs)
  - integration smoke/resilience tests in [D:\Lang\SpectraLang\compiler\tests\stage_smoke.rs](D:\Lang\SpectraLang\compiler\tests\stage_smoke.rs) and [D:\Lang\SpectraLang\compiler\tests\frontend_resilience.rs](D:\Lang\SpectraLang\compiler\tests\frontend_resilience.rs)
- `midend`
  - lowering tests in [D:\Lang\SpectraLang\midend\tests\lowering_tests.rs](D:\Lang\SpectraLang\midend\tests\lowering_tests.rs)
  - optimization and verifier tests in [D:\Lang\SpectraLang\midend\tests\optimization_tests.rs](D:\Lang\SpectraLang\midend\tests\optimization_tests.rs) and [D:\Lang\SpectraLang\midend\src\passes\verification.rs](D:\Lang\SpectraLang\midend\src\passes\verification.rs)
- `backend`
  - codegen tests in [D:\Lang\SpectraLang\backend\src\codegen.rs](D:\Lang\SpectraLang\backend\src\codegen.rs)
- `runtime`
  - runtime, memory, stdlib, and FFI tests in [D:\Lang\SpectraLang\runtime\src\lib.rs](D:\Lang\SpectraLang\runtime\src\lib.rs), [D:\Lang\SpectraLang\runtime\src\memory\mod.rs](D:\Lang\SpectraLang\runtime\src\memory\mod.rs), [D:\Lang\SpectraLang\runtime\src\stdlib\mod.rs](D:\Lang\SpectraLang\runtime\src\stdlib\mod.rs), and [D:\Lang\SpectraLang\runtime\src\ffi.rs](D:\Lang\SpectraLang\runtime\src\ffi.rs)
- `tools/spectra-cli`
  - unit tests in [D:\Lang\SpectraLang\tools\spectra-cli\src\main.rs](D:\Lang\SpectraLang\tools\spectra-cli\src\main.rs)
  - integration tests in [D:\Lang\SpectraLang\tools\spectra-cli\tests\integration_tests.rs](D:\Lang\SpectraLang\tools\spectra-cli\tests\integration_tests.rs)

Acceptance mapping: every compiler crate now has stage-local automated tests.

### 2. Regression corpus

Purpose: validate language behavior across real `.spectra` fixtures.

Primary corpus:

- `tests/validation`: must compile
- `tests/control_flow`: must compile
- `tests/projects/valid`: must compile as projects
- `tests/errors`: must fail quickly
- `tests/semantic`: informative semantic behavior surface
- `tests/cli`: CLI fixture inputs
- `examples/`: language examples and demos

Primary runner:

- [D:\Lang\SpectraLang\run_tests.ps1](D:\Lang\SpectraLang\run_tests.ps1)

Regression policy:

- every bug fixed from an example or issue must add or tighten a fixture in one of the directories above
- frontend crashes and infinite-loop regressions must add a minimal malformed-input case
- lowering/backend regressions must add a minimized `.spectra` fixture or crate-local IR test
- tests that are intentionally invalid belong in `tests/errors` or explicitly informational buckets, never mixed into the positive validation bucket

### 3. Resilience and fuzz-style testing

Purpose: catch parser/semantic instability from malformed input.

Current Phase 1 targets:

- malformed corpus resilience in [D:\Lang\SpectraLang\compiler\tests\frontend_resilience.rs](D:\Lang\SpectraLang\compiler\tests\frontend_resilience.rs)
- feature-gate regression coverage in the same file
- lexical diagnostic-code stability in the same file

This is a deterministic fuzz layer rather than a `cargo-fuzz` integration. It is sufficient for Phase 1 because it gives executable, CI-friendly malformed-input coverage without introducing another toolchain dependency.

### 4. Local execution

Primary runner:

- [D:\Lang\SpectraLang\run_tests.ps1](D:\Lang\SpectraLang\run_tests.ps1)

Coverage:

- crate-level `cargo test` lanes (interop, LSP, midend tensor graph tests)
- full scripted Spectra regression suite, including the validator scripts under `scripts/`

The repository keeps a single GitHub workflow,
[D:\Lang\SpectraLang\.github\workflows\release.yml](D:\Lang\SpectraLang\.github\workflows\release.yml),
which builds and publishes release artifacts. The per-platform CI matrix that
previously enforced `cargo test --workspace` on every push was removed;
`run_tests.ps1` is the supported regression entry point.

## Exit Criteria for New Compiler Work

Any future compiler change is only complete when:

1. the smallest affected crate gains or updates a stage-local test
2. a user-facing regression adds or updates a fixture in the corpus
3. `cargo test --workspace` passes
4. `run_tests.ps1` passes when the change affects language or CLI behavior
