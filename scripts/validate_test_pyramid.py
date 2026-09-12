#!/usr/bin/env python3
"""Validate the R-104 compiler test pyramid structure."""

from __future__ import annotations

from pathlib import Path


REQUIRED_FILES = [
    "compiler/tests/snapshot_tests.rs",
    "compiler/tests/snapshots/parser_ast.snap",
    "compiler/tests/snapshots/semantic_diagnostic.snap",
    "midend/tests/ir_snapshot_tests.rs",
    "midend/tests/snapshots/lowering_ir.snap",
    "backend/src/codegen_tests.rs",
    "tools/spectra-cli/tests/integration_tests.rs",
    "fuzz/Cargo.toml",
    "fuzz/fuzz_targets/parser.rs",
    "fuzz/fuzz_targets/semantic.rs",
    "fuzz/fuzz_targets/pipeline.rs",
    "fuzz/fuzz_targets/lowering.rs",
    "fuzz/README.md",
    "docs/testing-regression-policy.md",
]


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    errors: list[str] = []

    for rel in REQUIRED_FILES:
        if not (root / rel).is_file():
            errors.append(f"missing required test pyramid file: {rel}")

    backend = (root / "backend/src/codegen_tests.rs").read_text(encoding="utf-8")
    if "#[test]" not in backend:
        errors.append("backend/src/codegen_tests.rs has no stage-local tests")

    fuzz_manifest = (root / "fuzz/Cargo.toml").read_text(encoding="utf-8")
    for target in ("parser", "semantic", "pipeline", "lowering"):
        if f'name = "{target}"' not in fuzz_manifest:
            errors.append(f"fuzz manifest does not declare target: {target}")

    policy = (root / "docs/testing-regression-policy.md").read_text(encoding="utf-8")
    for phrase in ("AST snapshots", "Diagnostic snapshots", "IR snapshots", "Fuzz Workflow"):
        if phrase not in policy:
            errors.append(f"regression policy missing section phrase: {phrase}")

    if errors:
        for error in errors:
            print(f"ERROR: {error}")
        return 1

    print(f"validated R-104 test pyramid with {len(REQUIRED_FILES)} required files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
