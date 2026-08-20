#!/usr/bin/env python3
"""Validation gate for R-2106 Stream<T> and stream adaptors."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str]) -> subprocess.CompletedProcess[str]:
    print(f"[R-2106] {' '.join(command)}")
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
            print(f"[R-2106] missing marker in {path}: {needle}", file=sys.stderr)
        raise SystemExit(1)


def main() -> int:
    require_contains(
        ROOT / "runtime" / "src" / "stdlib" / "mod.rs",
        [
            "spectra.async.stream.new",
            "spectra.async.stream.push",
            "spectra.async.stream.done",
            "spectra.async.stream.next",
            "spectra.async.stream.next_status",
            "spectra.async.stream.cancel",
            "spectra.async.stream.map",
            "spectra.async.stream.filter",
            "spectra.async.stream.fold",
            "spectra.async.stream.take",
            "spectra.async.stream.skip",
            "spectra.async.stream.chunks",
            "spectra.async.stream.fuse",
            "async_stream_host_calls_cover_adaptors_backpressure_done_and_cancellation",
        ],
    )

    run(["cargo", "test", "-q", "-p", "spectra-runtime", "async_stream_host_calls"])
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
            "tests/validation/125_async_stream_surface.spectra",
        ]
    )

    print("validated R-2106 Stream<T> and stream adaptors")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
