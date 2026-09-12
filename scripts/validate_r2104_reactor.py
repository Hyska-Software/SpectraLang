#!/usr/bin/env python3
"""Validation gate for R-2104 event loop multiplexer."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str]) -> subprocess.CompletedProcess[str]:
    print(f"[R-2104] {' '.join(command)}")
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
            print(f"[R-2104] missing marker in {path}: {needle}", file=sys.stderr)
        raise SystemExit(1)


def main() -> int:
    require_contains(
        ROOT / "runtime" / "src" / "reactor" / "mod.rs",
        [
            "Linux selects the `epoll` backend label.",
            "Windows selects the `IOCP` backend label.",
            "macOS and the BSD family select the `kqueue` backend label.",
            'target_os = "freebsd"',
            'target_os = "openbsd"',
            'target_os = "netbsd"',
            'target_os = "dragonfly"',
            "`mio::Poll` maps to the platform readiness backend",
            "pub enum EventKind",
            "TaskWake",
            "Timer",
            "Io",
            "register_source",
            "deregister_source",
            "bsd_kqueue_backend_is_selected",
            "real_tcp_listener_readiness_reaches_the_shared_queue",
            "linux_epoll_backend_handles_10k_suspended_task_wakeups",
            "10_000",
        ],
    )
    require_contains(
        ROOT / ".github" / "workflows" / "r2104-freebsd-kqueue.yml",
        [
            "vmactions/freebsd-vm@v1",
            "cargo test -q -p spectra-runtime reactor -- --nocapture",
            "async_stdlib_host_calls_cover_fs_tcp_udp_channels_and_cancellation",
        ],
    )
    require_contains(
        ROOT / "runtime" / "src" / "stdlib" / "mod.rs",
        [
            "spectra.async.reactor.backend",
            "spectra.async.reactor.wake",
            "spectra.async.reactor.timer",
            "spectra.async.reactor.io_register",
            "spectra.async.reactor.io_notify",
            "spectra.async.reactor.poll",
            "async_reactor_host_calls_cover_backend_wake_timer_and_io",
        ],
    )

    run(["cargo", "test", "-q", "-p", "spectra-runtime", "reactor"])
    run(["cargo", "test", "-q", "-p", "spectra-runtime", "async_reactor"])
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
            "tests/validation/123_async_reactor_ready_tasks.spectra",
        ]
    )

    print("validated R-2104 event loop multiplexer")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
