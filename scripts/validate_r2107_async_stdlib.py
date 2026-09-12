#!/usr/bin/env python3
"""Validation gate for R-2107 async standard-library host calls."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str]) -> subprocess.CompletedProcess[str]:
    print(f"[R-2107] {' '.join(command)}")
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
    require_contains_text(str(path), text, needles)


def require_contains_text(label: str, text: str, needles: list[str]) -> None:
    missing = [needle for needle in needles if needle not in text]
    if missing:
        for needle in missing:
            print(f"[R-2107] missing marker in {label}: {needle}", file=sys.stderr)
        raise SystemExit(1)


def main() -> int:
    stdlib_root = ROOT / "runtime" / "src" / "stdlib"
    stdlib_text = "\n".join(
        path.read_text(encoding="utf-8") for path in sorted(stdlib_root.rglob("*.rs"))
    )
    require_contains_text(
        str(stdlib_root),
        stdlib_text,
        [
            "spectra.async.fs.read_async",
            "spectra.async.fs.write_async",
            "spectra.async.tcp.connect_async",
            "spectra.async.tcp.accept_async",
            "spectra.async.tcp.read_async",
            "spectra.async.tcp.write_async",
            "spectra.async.udp.send_to_async",
            "spectra.async.udp.recv_async",
            "spectra.async.channel.send",
            "spectra.async.channel.recv",
            "async_stdlib_host_calls_cover_fs_tcp_udp_channels_and_cancellation",
        ],
    )

    run(["cargo", "test", "-q", "-p", "spectra-runtime", "async_stdlib_host_calls"])
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
            "tests/validation/126_async_stdlib_surface.spectra",
        ]
    )

    print("validated R-2107 async standard-library surface")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
