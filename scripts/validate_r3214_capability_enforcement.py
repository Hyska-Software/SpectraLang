# R-3214 — capability enforcement at the dispatch point.
#
# Validates the single dispatch hook, the four entrypoints, the denial channel,
# the policy evaluation wired to run capabilities, and the invariant that a
# program without an active run is unchanged.
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
    print(f"R-3214 validation failed: {message}", file=sys.stderr)
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
    registry = read("runtime/src/ffi_host_registry.rs")
    require("fn dispatch_generic" in registry, "ffi_host_registry must expose dispatch_generic")

    lifecycle = read("runtime/src/ffi_lifecycle.rs")
    entrypoints = ["spectra_rt_host_invoke", "spectra_rt_host_invoke_cached",
                   "spectra_rt_host_invoke_batch", "spectra_rt_host_invoke_cached_batch"]
    for entrypoint in entrypoints:
        require(entrypoint in lifecycle, f"ffi_lifecycle missing {entrypoint}")
    require(lifecycle.count("dispatch_generic(") >= 4, "all four entrypoints must route through dispatch_generic")

    core = read("runtime/src/ffi_core.rs")
    require("HOST_STATUS_DENIED" in core, "HOST_STATUS_DENIED must exist")

    panic = read("runtime/src/panic.rs")
    require("spectra_rt_capability_denied" in panic, "the denial symbol must exist")

    abi = read("runtime/src/abi.rs")
    require("HostDenied" in abi, "RuntimeImport must register the denial symbol")

    codegen = read("backend/src/codegen_instruction_host.rs")
    require("emit_capability_denied" in codegen, "codegen must branch on the denial status")

    hook = read("runtime/src/agent/policy_hook.rs")
    for term in ["PolicyDecision", "set_policy_evaluator", "fn evaluate"]:
        require(term in hook, f"policy hook missing {term}")

    policy = read("packages/spectra-agent/src/policy.rs")
    for term in ["allow", "authorize"]:
        require(term in policy, f"agent policy missing {term}")

    invariant = read("runtime/src/abi.rs")
    require(
        "fast_host_call_effect_namespace" in invariant,
        "the fast-path effect-namespace invariant must remain",
    )


def validate_behavior() -> None:
    output = run_command(
        [CARGO, "test", "-q", "-p", "spectra-runtime", "--lib", "policy_", "--offline"]
    )
    require("test result: ok" in output, f"policy denial tests must pass:\n{output}")

    output = run_command(
        [CARGO, "test", "-q", "-p", "spectra-runtime", "fast_host_call_effect_namespace", "--offline"]
    )
    require("test result: ok" in output, f"fast-path invariant must pass:\n{output}")

    output = run_command([CARGO, "test", "-q", "-p", "spectra-backend", "--offline"])
    require("test result: ok" in output, f"backend tests must pass:\n{output}")


def validate_planning() -> None:
    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-3214 Capability Enforcement at the Dispatch Point", 1)[1].split(
        "## R-3215", 1
    )[0]
    for term in ["Status: `complete`", "validate_r3214_capability_enforcement.py"]:
        require(term in block, f"backlog R-3214 missing {term}")

    runner = read("run_tests.ps1")
    require("validate_r3214_capability_enforcement.py" in runner, "run_tests.ps1 must run R-3214")


def main() -> None:
    validate_implementation()
    validate_behavior()
    validate_planning()
    print("validated R-3214 capability enforcement at the dispatch point")


if __name__ == "__main__":
    main()
