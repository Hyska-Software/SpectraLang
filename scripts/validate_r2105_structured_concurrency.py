#!/usr/bin/env python3
"""Validation gate for R-2105 cancellation, timeout, and structured concurrency."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str]) -> subprocess.CompletedProcess[str]:
    print(f"[R-2105] {' '.join(command)}")
    completed = subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if completed.returncode != 0:
        print(completed.stdout)
        raise SystemExit(completed.returncode)
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
            print(f"[R-2105] missing marker in {path}: {needle}", file=sys.stderr)
        raise SystemExit(1)


def main() -> int:
    require_contains(
        ROOT / "runtime" / "src" / "stdlib" / "mod.rs",
        [
            "spectra.async.task.join",
            "spectra.async.task.join_status",
            "spectra.async.task.cancel_handle",
            "spectra.async.cancel_handle.cancel",
            "spectra.async.task.with_timeout",
            "spectra.async.scheduler.advance_time",
            "spectra.async.scope.new",
            "spectra.async.scope.child",
            "spectra.async.scope.spawn_ready",
            "spectra.async.scope.cancel",
            "spectra.async.scope.join",
            "spectra.async.scope.failures",
            "async_structured_concurrency_host_calls_cover_cascade_timeout_and_join_order",
        ],
    )

    run(
        [
            "cargo",
            "test",
            "-q",
            "-p",
            "spectra-runtime",
            "async_structured_concurrency",
        ]
    )
    run(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "spectra-cli",
            "--",
            "check",
            "--dump-ir",
            "tests/validation/124_async_structured_concurrency_surface.spectra",
        ]
    )

    print("validated R-2105 cancellation, timeout, and structured concurrency")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
