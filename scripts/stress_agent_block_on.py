"""Stress harness for the `block_on` task-tree race (R-3221 flake triage).

Repeats the nested-dispatch fixture in many short-lived processes, each in its
own working directory, and stops at the first non-zero exit with the full output
saved for triage. The fixture (`tests/validation/397_...`) dispatches a tool
whose body nests another `tool_call` and a tool that starts and ends a second
run inside its body: the shape where a background tool worker and the waiting
caller touch the same task tree.

This is a triage tool, not a gate: it runs until it fails or exhausts the
requested iterations, and it is deliberately not registered in `run_tests.ps1`.
See `docs/architecture/agent-block-on-flake-known-failure.md`.

Usage:
    python scripts/stress_agent_block_on.py --iterations 300
    python scripts/stress_agent_block_on.py --iterations 1000 --jobs 8 --engine both
"""

from __future__ import annotations

import argparse
import concurrent.futures
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests" / "validation" / "397_agent_nested_dispatch_stress.spectra"
WORK = ROOT / "target" / "r3290-stress"


def resolve_binary() -> Path:
    """The spectralang binary to drive, mirroring the gate's resolution."""
    for name in ("spectralang.exe", "spectralang"):
        candidate = ROOT / "target" / "debug" / name
        if candidate.is_file():
            return candidate
    raise SystemExit("target/debug/spectralang is missing; build the CLI first")


def build_aot() -> Path:
    """Compiles the fixture once and returns the executable."""
    WORK.mkdir(parents=True, exist_ok=True)
    executable = WORK / ("fixture-397.exe" if os.name == "nt" else "fixture-397")
    completed = subprocess.run(
        [
            str(resolve_binary()),
            "compile",
            "--debug-info=none",
            "--emit-exe",
            str(executable),
            str(FIXTURE.relative_to(ROOT)),
        ],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        timeout=600,
    )
    if completed.returncode != 0 or not executable.is_file():
        raise SystemExit(f"building the stress fixture failed:\n{completed.stdout}")
    return executable


def command(engine: str, executable: Path) -> list[str]:
    if engine == "aot":
        return [str(executable)]
    return [str(resolve_binary()), "run", str(FIXTURE)]


def run_once(engine: str, executable: Path, index: int) -> tuple[int, str, Path]:
    """Runs one iteration in its own working directory.

    The fixture writes its journal under the working directory, so concurrent
    processes must not share one.
    """
    directory = WORK / "work" / str(index)
    directory.mkdir(parents=True, exist_ok=True)
    completed = subprocess.run(
        command(engine, executable),
        cwd=directory,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        timeout=900,
    )
    return completed.returncode, completed.stdout, directory


def save_failure(iteration: int, engine: str, code: int, output: str) -> Path:
    WORK.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
    path = WORK / f"{stamp}-failure-{engine}-{iteration}.txt"
    path.write_text(
        f"iteration: {iteration}\nengine: {engine}\nexit code: {code}\n\n{output}",
        encoding="utf-8",
        newline="\n",
    )
    return path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iterations", type=int, default=300)
    parser.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    parser.add_argument("--engine", choices=("aot", "jit", "both"), default="aot")
    parser.add_argument(
        "--keep-going",
        action="store_true",
        help="run every iteration and keep the first failure file per engine",
    )
    args = parser.parse_args()

    if not FIXTURE.is_file():
        raise SystemExit(f"missing fixture {FIXTURE.relative_to(ROOT)}")

    engines = ("aot", "jit") if args.engine == "both" else (args.engine,)
    executable = build_aot() if "aot" in engines else WORK / "unused"
    work = [(engine, index) for engine in engines for index in range(args.iterations)]

    failures = 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
        futures = {
            pool.submit(run_once, engine, executable, index): (engine, index)
            for engine, index in work
        }
        for future in concurrent.futures.as_completed(futures):
            engine, index = futures[future]
            try:
                code, output, _directory = future.result()
            except subprocess.TimeoutExpired:
                code, output = 124, "iteration timed out"
            if code == 0:
                continue
            failures += 1
            path = save_failure(index, engine, code, output)
            print(f"stress: {engine} iteration {index} exited {code}; output saved to {path}")
            if not args.keep_going:
                for pending in futures:
                    pending.cancel()
                break

    if failures:
        print(f"stress: {failures} failure(s)")
        return 1
    print(f"stress: {len(work)} runs, 0 failures")
    return 0


if __name__ == "__main__":
    sys.exit(main())
