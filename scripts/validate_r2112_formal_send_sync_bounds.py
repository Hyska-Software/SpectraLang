#!/usr/bin/env python3
"""Validation gate for R-2112 formal Send/Sync trait bounds."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str], *, expect_success: bool = True) -> subprocess.CompletedProcess[str]:
    print(f"[R-2112] {' '.join(command)}")
    completed = subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if expect_success and completed.returncode != 0:
        print(completed.stdout)
        raise SystemExit(completed.returncode)
    if not expect_success and completed.returncode == 0:
        print(completed.stdout)
        print("[R-2112] command unexpectedly succeeded", file=sys.stderr)
        raise SystemExit(1)
    return completed


def require_contains(path: Path, needles: list[str]) -> None:
    text = path.read_text(encoding="utf-8")
    missing = [needle for needle in needles if needle not in text]
    if missing:
        stage_text = "\n".join(
            source.read_text(encoding="utf-8")
            for source in sorted(path.parent.rglob("*.rs"))
        )
        missing = [needle for needle in missing if needle not in stage_text]
    if missing:
        for needle in missing:
            print(f"[R-2112] missing marker in {path}: {needle}", file=sys.stderr)
        raise SystemExit(1)


def parse_json(output: str) -> dict:
    for line in output.splitlines():
        stripped = line.strip()
        if stripped.startswith("{"):
            return json.loads(stripped)
    print(output)
    print("[R-2112] no JSON diagnostic payload found", file=sys.stderr)
    raise SystemExit(1)


def require_diagnostic(path: str, code: str, message_part: str) -> None:
    completed = run(
        ["cargo", "run", "-q", "-p", "spectra-cli", "--", "check", "--json", path],
        expect_success=False,
    )
    payload = parse_json(completed.stdout)
    diagnostics = [
        diagnostic
        for file_entry in payload.get("files", [])
        for diagnostic in file_entry.get("diagnostics", [])
    ]
    if not any(
        diagnostic.get("code") == code and message_part in diagnostic.get("message", "")
        for diagnostic in diagnostics
    ):
        print(completed.stdout)
        print(
            f"[R-2112] expected {code} containing {message_part!r} in {path}",
            file=sys.stderr,
        )
        raise SystemExit(1)


def main() -> int:
    require_contains(
        ROOT / "compiler" / "src" / "ast" / "mod.rs",
        ["auto_traits: Vec<String>", "DynTrait"],
    )
    require_contains(
        ROOT / "compiler" / "src" / "parser" / "type_annotation.rs",
        ["Expected trait bound after '+' in dyn type", "auto_traits.push"],
    )
    require_contains(
        ROOT / "compiler" / "src" / "semantic" / "mod.rs",
        ["type_is_sync", "E2104", "auto_trait_bounds_satisfied"],
    )
    require_contains(
        ROOT / "midend" / "src" / "lowering.rs",
        ["ir_type_satisfies_auto_trait", "auto_traits"],
    )
    require_contains(
        ROOT / "docs" / "diagnostics" / "error-code-reference.md",
        ["E2104", "formal `Send`/`Sync` evidence"],
    )

    run(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "spectra-cli",
            "--",
            "run",
            "tests/validation/132_formal_send_sync_bounds.spectra",
        ]
    )

    ir = run(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "spectra-cli",
            "--",
            "check",
            "--dump-ir",
            "tests/validation/132_formal_send_sync_bounds.spectra",
        ]
    ).stdout
    for needle in [
        "fn drive(dyn Worker + Send worker) -> Task<int>",
        "fn inspect(dyn Worker + Send + Sync worker) -> int",
        "call_indirect",
    ]:
        if needle not in ir:
            print(ir)
            print(f"[R-2112] missing IR marker: {needle}", file=sys.stderr)
            raise SystemExit(1)

    require_diagnostic(
        "tests/errors/formal_send_bound_missing_across_await.spectra",
        "E2101",
        "non-Send type T",
    )
    require_diagnostic(
        "tests/errors/formal_task_boundary_missing_send.spectra",
        "E2103",
        "cannot cross a task boundary",
    )
    require_diagnostic(
        "tests/errors/formal_send_bound_rejects_non_send.spectra",
        "E2104",
        "formal Send evidence",
    )
    require_diagnostic(
        "tests/errors/formal_sync_bound_rejects_refcell.spectra",
        "E2104",
        "formal Sync evidence",
    )
    require_diagnostic(
        "tests/errors/dyn_trait_send_bound_missing.spectra",
        "E2104",
        "dyn Worker + Send",
    )

    run(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "spectra-cli",
            "--",
            "compile",
            "tests/validation/129_async_trait_objects_future_stream.spectra",
        ]
    )

    print("validated R-2112 formal Send/Sync trait bounds")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
