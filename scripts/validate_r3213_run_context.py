# R-3213 — run context and propagation.
#
# Validates the stacking run context, explicit propagation to workers and
# coroutine resumption, the fail-closed detached-work seam, and the planning
# bookkeeping for the item.
from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CARGO = shutil.which("cargo") or "cargo"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3213 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def run_command(args: list[str]) -> str:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    if completed.returncode != 0:
        fail(f"command {' '.join(args)} failed:\n{completed.stdout}")
    return completed.stdout


def validate_implementation() -> None:
    run_context = read("runtime/src/agent/run_context.rs")
    for term in [
        "RUN_STACK",
        "pub fn push",
        "pub fn pop",
        "RunContextGuard",
        "current_chain",
        "RunContextError",
        "detached_work_allowed",
        "spawn_detached",
    ]:
        require(term in run_context, f"run_context.rs missing {term}")

    lib = read("runtime/src/lib.rs")
    require("pub mod agent;" in lib, "runtime lib.rs must export the agent module")

    concurrent = read("runtime/src/stdlib/concurrent_core.rs")
    require("run_chain" in concurrent and "with_chain" in concurrent, "spawn path must carry the run chain")

    async_stream = read("runtime/src/stdlib/async_task_stream.rs")
    require("with_chain" in async_stream, "coroutine resumption must restore the run chain")


def validate_behavior() -> None:
    output = run_command(
        [CARGO, "test", "-q", "-p", "spectra-runtime", "run_context", "--offline"]
    )
    require("test result: ok" in output, f"run_context tests must pass:\n{output}")

    output = run_command(
        [CARGO, "test", "-q", "-p", "spectra-runtime", "--lib", "concurrent_task_spawn_fn", "--offline"]
    )
    require("test result: ok" in output, f"spawn propagation tests must pass:\n{output}")

    output = run_command(
        [CARGO, "test", "-q", "-p", "spectra-runtime", "--lib", "coroutine", "--offline"]
    )
    require("test result: ok" in output, f"coroutine resumption tests must pass:\n{output}")


def validate_planning() -> None:
    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-3213 Run Context and Propagation", 1)[1].split("## R-3214", 1)[0]
    for term in ["Status: `complete`", "validate_r3213_run_context.py"]:
        require(term in block, f"backlog R-3213 missing {term}")

    runner = read("run_tests.ps1")
    require("validate_r3213_run_context.py" in runner, "run_tests.ps1 must run R-3213")


def main() -> None:
    validate_implementation()
    validate_behavior()
    validate_planning()
    print("validated R-3213 run context and propagation")


if __name__ == "__main__":
    main()
