# Security and Operations

This document describes the Phase 12 production baseline for supply-chain
security and stress/soak validation.

## Release Security Evidence

Use `scripts/release_security.py` to create and verify release evidence.

Generated files:

- `release-manifest.json`: artifact paths, SHA-256 hashes, sizes, version,
  commit, builder metadata, and dirty-tree flag.
- `release-manifest.json.sig`: HMAC-SHA256 signature over the canonical
  manifest JSON.
- `release-provenance.json`: release provenance with commit, workflow, run ID,
  version, and artifact subjects.
- `release-sbom.cdx.json`: CycloneDX-compatible SBOM from `Cargo.lock` and
  `tools/vscode-extension/package-lock.json`.
- `SHA256SUMS`: user-facing artifact checksum list.

Signing requires the environment variable `SPECTRA_RELEASE_SIGNING_KEY`; the
local `--allow-dev-key` flag is only for validation tests and must not be used
for published evidence.

The release workflow only builds, packages, and publishes assets. It no longer
runs a signing preflight or generates signing evidence, so published GitHub
release assets are unsigned unless `scripts/release_security.py` is run on
demand (it is also exercised by `run_tests.ps1`).

Example:

```powershell
python scripts\release_security.py create `
  --artifact release-assets `
  --out release-evidence `
  --version 0.2.0

python scripts\release_security.py verify --evidence release-evidence
```

Local validation with a deterministic non-secret key:

```powershell
python scripts\release_security.py create `
  --artifact target\phase12-validation\artifacts `
  --out target\phase12-validation\evidence `
  --version 0.0.0-local `
  --allow-dev-key

python scripts\release_security.py verify `
  --evidence target\phase12-validation\evidence `
  --allow-dev-key
```

## Dependency Scanning

CI includes a dependency scan job:

- `cargo audit` for Rust dependencies.
- `npm audit --audit-level=high` for the VS Code extension.

The release workflow builds, packages, and publishes installable artifacts only.
Signed release evidence is generated and verified manually with
`scripts/release_security.py`; it is not produced or uploaded by the release
workflow.

## Stress and Soak Testing

Use `scripts/stress_soak.py` for defined compiler/runtime stress suites.

Default suites:

- `compile`: repeated compilation of representative language, tensor,
  autodiff, concurrency, and serving examples.
- `runtime`: repeated execution of representative tensor, autodiff,
  concurrency, and serving examples through the CLI/JIT runtime path.
- `package`: package lock/check/build stress over the package workspace.

Example smoke run:

```powershell
python scripts\stress_soak.py `
  --iterations 1 `
  --timeout-seconds 20 `
  --memory-limit-mb 1024 `
  --json-out target\stress-soak-smoke.json
```

Longer local soak run:

```powershell
python scripts\stress_soak.py `
  --iterations 50 `
  --timeout-seconds 60 `
  --memory-limit-mb 2048 `
  --json-out target\stress-soak-local.json
```

The stress runner fails on:

- non-zero process exit
- timeout
- memory limit breach when process RSS is available

When `psutil` is available, the report includes observed peak RSS. Without
`psutil`, timeout and exit-code checks still run.

## Runtime Invariants and Host Interop Containment

The runtime exposes `spectra_rt_debug_invariants_check()` for low-cost invariant
checks over:

- host function registry state
- manual allocation frame stack
- manual allocation ownership consistency

`spectra_rt_host_invoke(...)` validates host-call buffers and wraps host
function invocation in a panic guard. If a host implementation unwinds through a
compatible ABI boundary, the runtime reports `HOST_STATUS_INTERNAL_ERROR`
instead of propagating the panic into caller code.

Runtime unit tests cover:

- invariant checks across host registry and manual allocation lifecycle
- host invocation status/result handling
- missing host function error handling

## Integrated Validation

`run_tests.ps1` validates the Phase 12 baseline by:

- creating and verifying local release evidence with a deterministic test key
- running the stress/soak smoke profile
- including Phase 12 results in `TEST_RESULTS.txt`

Full command:

```powershell
.\run_tests.ps1
```

## Current Limits

- Release signatures use HMAC-SHA256 evidence signing. Public-key signing or
  Sigstore/cosign can be added as future hardening.
- CI dependency scanning is present; vulnerability policy thresholds beyond
  `npm audit --audit-level=high` and `cargo audit` defaults should be refined
  as the project matures.
- Stress/soak suites are defined and automated. Very long soak windows remain
  an operational choice and should run outside the fast regression path.
