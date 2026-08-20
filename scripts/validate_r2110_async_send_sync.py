#!/usr/bin/env python3
"""Validation gate for R-2110 async diagnostics and Send/Sync validation."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str], *, expect_success: bool = True) -> subprocess.CompletedProcess[str]:
    print(f"[R-2110] {' '.join(command)}")
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
        print("[R-2110] command unexpectedly succeeded", file=sys.stderr)
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
            print(f"[R-2110] missing marker in {path}: {needle}", file=sys.stderr)
        raise SystemExit(1)


def parse_json(output: str) -> dict:
    for line in output.splitlines():
        line = line.strip()
        if line.startswith("{"):
            return json.loads(line)
    print(output)
    print("[R-2110] no JSON diagnostic payload found", file=sys.stderr)
    raise SystemExit(1)


def require_code(path: str, code: str, message_part: str) -> None:
    completed = run(
        ["cargo", "run", "-q", "-p", "spectra-cli", "--", "check", "--json", path],
        expect_success=False,
    )
    payload = parse_json(completed.stdout)
    found = [
        diagnostic
        for file_entry in payload.get("files", [])
        for diagnostic in file_entry.get("diagnostics", [])
        if diagnostic.get("code") == code and message_part in diagnostic.get("message", "")
    ]
    if not found:
        print(completed.stdout)
        print(f"[R-2110] expected {code} containing {message_part!r}", file=sys.stderr)
        raise SystemExit(1)


def main() -> int:
    require_contains(
        ROOT / "compiler" / "src" / "semantic" / "mod.rs",
        [
            "validate_async_send_sync_function",
            "AsyncSendSyncEvents",
            "type_is_send",
            "E2101",
            "E2102",
            "E2103",
        ],
    )
    require_contains(
        ROOT / "docs" / "diagnostics" / "error-code-reference.md",
        ["E2101", "E2102", "E2103", "E2120"],
    )
    require_contains(
        ROOT / "docs" / "adr" / "0010-async-execution-model.md",
        ["E2101", "E2102", "E2103"],
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
            "tests/validation/131_async_send_sync_valid.spectra",
        ]
    )

    require_code(
        "tests/errors/async_non_send_across_await.spectra",
        "E2101",
        "non-Send type NonSend",
    )
    require_code(
        "tests/errors/async_refcell_across_await.spectra",
        "E2102",
        "RefCell",
    )
    require_code(
        "tests/errors/async_non_send_task_boundary.spectra",
        "E2103",
        "cannot cross a task boundary",
    )

    print("validated R-2110 async Send/Sync diagnostics")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
