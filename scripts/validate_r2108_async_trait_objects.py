#!/usr/bin/env python3
"""Validation gate for R-2108 async trait objects and dyn Future/Stream."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str], *, expect_success: bool = True) -> subprocess.CompletedProcess[str]:
    print(f"[R-2108] {' '.join(command)}")
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
        print("[R-2108] command unexpectedly succeeded", file=sys.stderr)
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
            print(f"[R-2108] missing marker in {path}: {needle}", file=sys.stderr)
        raise SystemExit(1)


def parse_first_json_object(output: str) -> dict:
    for line in output.splitlines():
        line = line.strip()
        if line.startswith("{") and line.endswith("}"):
            return json.loads(line)
    print(output)
    print("[R-2108] no JSON diagnostics payload found", file=sys.stderr)
    raise SystemExit(1)


def main() -> int:
    require_contains(
        ROOT / "compiler" / "src" / "semantic" / "mod.rs",
        [
            "register_builtin_async_traits",
            "Future",
            "Stream",
            "Box",
            "validate_async_trait_method_object_safety",
            "E2108",
        ],
    )
    require_contains(
        ROOT / "midend" / "src" / "lowering.rs",
        [
            "register_builtin_async_traits",
            "IRType::Task",
            "TypeAnnotationKind::DynTrait",
            "trait_method_signatures",
        ],
    )
    require_contains(
        ROOT / "compiler" / "src" / "parser" / "mod.rs",
        ["register_builtin_async_traits", "Future", "Stream"],
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
            "tests/validation/129_async_trait_objects_future_stream.spectra",
        ]
    ).stdout
    for needle in [
        "fn drive_future(dyn Future future) -> Task<int>",
        "fn drive_stream(dyn Stream stream) -> Task<int>",
        "fn drive_worker(dyn AsyncWorker worker) -> Task<int>",
        "call_indirect",
        # Real suspension (no eager blocking wait): child polling and the
        # generated poll/dispatch state machine replace task.result markers.
        "poll.child",
        "coroutine.dispatch",
        "__poll",
    ]:
        if needle not in ir:
            print(ir)
            print(f"[R-2108] missing IR marker: {needle}", file=sys.stderr)
            raise SystemExit(1)

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

    diagnostics = run(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "spectra-cli",
            "--",
            "check",
            "--json",
            "tests/errors/async_trait_object_safety.spectra",
        ],
        expect_success=False,
    ).stdout
    payload = parse_first_json_object(diagnostics)
    found = [
        diagnostic
        for file_entry in payload.get("files", [])
        for diagnostic in file_entry.get("diagnostics", [])
        if diagnostic.get("code") == "E2108"
        and "BadFuture::poll" in diagnostic.get("message", "")
    ]
    if not found:
        print(diagnostics)
        print("[R-2108] expected E2108 diagnostic for BadFuture::poll", file=sys.stderr)
        raise SystemExit(1)

    print("validated R-2108 async trait objects and dyn Future/Stream")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
