#!/usr/bin/env python3
"""Validation gate for R-2103 await expression and async lowering."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str], *, expect_success: bool = True) -> subprocess.CompletedProcess[str]:
    print(f"[R-2103] {' '.join(command)}")
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
        raise SystemExit(f"expected command to fail: {' '.join(command)}")
    return completed


def require_output(output: str, needles: list[str]) -> None:
    missing = [needle for needle in needles if needle not in output]
    if missing:
        for needle in missing:
            print(f"[R-2103] missing output marker: {needle}", file=sys.stderr)
        print(output)
        raise SystemExit(1)


def require_contains(path: Path, needles: list[str]) -> None:
    text = path.read_text(encoding="utf-8")
    missing = [needle for needle in needles if needle not in text]
    if missing:
        # The runtime stdlib is decomposed into focused modules; preserve the
        # stage-level contract when a marker moved out of mod.rs.
        stage_text = "\n".join(
            source.read_text(encoding="utf-8")
            for source in sorted(path.parent.rglob("*.rs"))
        )
        missing = [needle for needle in missing if needle not in stage_text]
    if missing:
        for needle in missing:
            print(f"[R-2103] missing marker in {path}: {needle}", file=sys.stderr)
        raise SystemExit(1)


def main() -> int:
    require_contains(
        ROOT / "midend" / "src" / "ir.rs",
        ["AsyncReady", "Task {"],
    )
    require_contains(
        ROOT / "runtime" / "src" / "stdlib" / "mod.rs",
        [
            "spectra.async.task.ready",
            "spectra.async.task.poll",
            "spectra.async.task.wait",
            "std_async_task_wait",
            "spectra.async.task.result",
            "spectra.async.task.cancel",
            "std_async_task_cancel",
        ],
    )

    run(["cargo", "test", "-q", "-p", "spectra-compiler", "async"])
    run(["cargo", "test", "-q", "-p", "spectra-midend", "r2103"])
    run(["cargo", "test", "-q", "-p", "spectra-backend", "async_ready"])
    run(["cargo", "test", "-q", "-p", "spectra-runtime", "async_task_host_calls"])

    dump = run(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "spectra-cli",
            "--",
            "check",
            "--dump-ir",
            "tests/validation/121_async_await_lowering.spectra",
        ]
    )
    # The coroutine lowering shape since the poll/drop split: the ramp allocates a
    # frame and creates a `Task`, child awaits lower to `poll.child`, and poll
    # completion flows through `coroutine.complete` (these replaced the earlier
    # `async.ready` + `spectra.async.task.wait/result` host-call shape).
    require_output(
        dump.stdout,
        [
            "fn add_one() -> Task<int>",
            "coroutine.create",
            "poll.child",
            "coroutine.complete",
        ],
    )

    # Real-wait regression: out-of-order awaits with a ~200 ms task.
    # `spectralang run` propagates the program's exit code, so 0 means the
    # values survived the waits and the slow task really elapsed >= 200 ms
    # (the fixture itself asserts that via time.monotonic_millis).
    run(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "spectra-cli",
            "--",
            "run",
            "tests/validation/349_async_wait_out_of_order.spectra",
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
            "tests/validation/122_async_early_return.spectra",
        ]
    )
    error = run(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "spectra-cli",
            "--",
            "check",
            "tests/errors/await_requires_task.spectra",
        ],
        expect_success=False,
    )
    require_output(error.stdout, ["`await` expects Task<T>, found int"])

    print("validated R-2103 await expression and async lowering")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
