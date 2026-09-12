# R-3209 — `std.agent` namespace and `packages/spectra-agent` crate.
#
# Validates the namespace seam end to end: crate aggregation, compiler
# declaration, midend gate, runtime implementation, contract catalog, probe,
# and identical results in JIT and AOT.
from __future__ import annotations

import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPECTRALANG = ROOT / "target" / "debug" / "spectralang.exe"
CARGO = shutil.which("cargo") or "cargo"
FIXTURE = "tests/validation/365_agent_surface.spectra"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3209 validation failed: {message}", file=sys.stderr)
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
    manifest = read("packages/spectra-agent/Cargo.toml")
    require('crate-type = ["rlib"]' in manifest, "spectra-agent must be an rlib")
    require('"staticlib"' not in manifest, "spectra-agent must not declare a staticlib crate type")

    lib = read("packages/spectra-agent/src/lib.rs")
    for term in ["pub fn register() -> usize", "spectra.std.agent.token_count"]:
        require(term in lib, f"spectra-agent lib.rs missing {term}")

    token = read("packages/spectra-agent/src/token.rs")
    require("text_token_count" in token, "token.rs must reuse the runtime tokenizer path")

    registration = read("packages/spectra-api/src/api_registration.rs")
    require("spectra_agent::register()" in registration, "api registration must aggregate spectra-agent")

    builtin = read("compiler/src/semantic/builtin_std_core.rs")
    require("make_std_agent" in builtin, "compiler must declare make_std_agent")
    require("token_count" in builtin, "make_std_agent must export token_count")

    modules = read("compiler/src/semantic/builtin_api_core.rs")
    require('"std.agent"' in modules, "std.agent must be registered as a builtin module")

    lowering = read("midend/src/lowering_std_agent.rs")
    require("token_count" in lowering, "midend lowering table must map token_count")
    gate = read("midend/src/lowering_std_host.rs")
    require("agent" in gate, "the std.agent gate arm must exist in lowering_std_host.rs")

    contract = read("scripts/stdlib_contract.toml")
    require("std.agent" in contract, "stdlib contract manifest must declare std.agent")

    workspace = read("Cargo.toml")
    require("packages/spectra-agent" in workspace, "workspace must include packages/spectra-agent")


def validate_catalog() -> None:
    run_command([sys.executable, "scripts/generate_stdlib_catalog.py"])
    with (ROOT / "packages/spectra-contract/catalog/stdlib.toml").open("rb") as handle:
        catalog = tomllib.load(handle)
    entries = {entry["path"]: entry for entry in catalog["entry"]}
    require("std.agent" in entries, "catalog must declare the std.agent module")
    function = entries.get("std.agent.token_count")
    require(function is not None, "catalog must declare std.agent.token_count")
    require(function.get("ir_return") == "int", "token_count must lower to int")
    require(function.get("returns_value") is True, "token_count must return a value")


def validate_execution() -> None:
    run_command([str(SPECTRALANG), "run", FIXTURE])

    executable = ROOT / "target" / "r3209-agent-surface.exe"
    run_command([str(SPECTRALANG), "compile", "--emit-exe", str(executable), FIXTURE])
    completed = subprocess.run(
        [str(executable)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    require(
        completed.returncode == 0,
        f"AOT binary must return 0, got {completed.returncode}:\n{completed.stdout}",
    )


def validate_planning() -> None:
    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-3209 std.agent Namespace and packages/spectra-agent Crate", 1)[1].split(
        "## R-3210", 1
    )[0]
    for term in ["Status: `complete`", "validate_r3209_agent_namespace.py"]:
        require(term in block, f"backlog R-3209 missing {term}")

    runner = read("run_tests.ps1")
    require("validate_r3209_agent_namespace.py" in runner, "run_tests.ps1 must run R-3209")


def main() -> None:
    run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    validate_implementation()
    validate_catalog()
    validate_execution()
    validate_planning()
    print("validated R-3209 std.agent namespace and crate seam")


if __name__ == "__main__":
    main()
