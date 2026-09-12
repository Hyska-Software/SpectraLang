# R-3212 — agent memory over the runtime vector index.
#
# Validates the memory module (tiers, provenance, deterministic recall, token
# cap, artifact persist/load), the runtime vector-index seam it reuses, the
# compiler/midend surface, the contract probe and the language-level fixture in
# JIT and AOT.
from __future__ import annotations

import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPECTRALANG = ROOT / "target" / "debug" / "spectralang.exe"
CARGO = shutil.which("cargo") or "cargo"
FIXTURE = "tests/validation/369_agent_memory.spectra"
CATALOG = "packages/spectra-contract/catalog/stdlib.toml"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3212 validation failed: {message}", file=sys.stderr)
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
    memory = read("packages/spectra-agent/src/memory.rs")
    for term in [
        "MemoryTier",
        "episodic",
        "semantic",
        "procedural",
        "RECALL_TOKEN_BUDGET",
        "text_token_count",
        "VectorIndex",
        "set_custom_metadata",
        "write_artifact",
        "read_artifact",
        "fn persist",
        "fn load",
        "remember_host",
        "recall_host",
    ]:
        require(term in memory, f"memory.rs missing {term}")

    vector_index = read("runtime/src/vector_index.rs")
    for term in [
        "pub struct VectorIndex",
        "pub fn write_artifact",
        "pub fn read_artifact",
        "pub fn set_custom_metadata",
    ]:
        require(term in vector_index, f"runtime vector_index.rs missing {term}")
    require(
        "pub mod vector_index" in read("runtime/src/lib.rs"),
        "runtime must export the vector_index seam",
    )

    lib = read("packages/spectra-agent/src/lib.rs")
    for term in ["mod memory", "memory::register()", "REMEMBER_HOST_CALL", "RECALL_HOST_CALL"]:
        require(term in lib, f"spectra-agent lib.rs missing {term}")

    builtin = read("compiler/src/semantic/builtin_std_core.rs")
    for term in ["remember", "recall"]:
        require(term in builtin, f"compiler surface missing {term}")

    lowering = read("midend/src/lowering_std_agent.rs")
    for term in ['"remember"', '"recall"', "spectra.std.agent.recall"]:
        require(term in lowering, f"midend lowering table missing {term}")

    error = read("packages/spectra-agent/src/error.rs")
    require("Memory(" in error, "error.rs must carry the memory variant")


def validate_contract() -> None:
    with (ROOT / "scripts/stdlib_contract.toml").open("rb") as handle:
        manifest = tomllib.load(handle)
    probes = {str(probe["id"]): probe for probe in manifest.get("probe", [])}
    probe = probes.get("agent-memory")
    require(probe is not None, "stdlib_contract.toml must carry the agent-memory probe")
    require(probe.get("path") == FIXTURE, f"agent-memory probe must point at {FIXTURE}")
    covers = set(probe.get("covers", []))
    for entry in ["std.agent.remember", "std.agent.recall"]:
        require(entry in covers, f"agent-memory probe must cover {entry}")


def validate_catalog() -> None:
    first = run_command([sys.executable, "scripts/generate_stdlib_catalog.py"])
    require("generated" in first, f"catalog generator produced no report:\n{first}")
    once = (ROOT / CATALOG).read_bytes()
    run_command([sys.executable, "scripts/generate_stdlib_catalog.py"])
    twice = (ROOT / CATALOG).read_bytes()
    require(once == twice, "catalog generation must be idempotent")

    with (ROOT / CATALOG).open("rb") as handle:
        catalog = tomllib.load(handle)
    entries = {entry["path"]: entry for entry in catalog["entry"]}
    for function in ["std.agent.remember", "std.agent.recall"]:
        entry = entries.get(function)
        require(entry is not None, f"catalog missing {function}")
        require(entry.get("returns_value") is True, f"{function} must return a value")
        require(entry.get("fixture") == FIXTURE, f"{function} must cite {FIXTURE}")
    require(
        entries["std.agent.remember"]["ir_return"] == "Result<bool,Error>",
        "remember must lower to Result<bool, Error>",
    )
    require(
        entries["std.agent.recall"]["ir_return"] == "Result<string,Error>",
        "recall must lower to Result<string, Error>",
    )


def validate_behavior() -> None:
    output = run_command([CARGO, "test", "-q", "-p", "spectra-agent", "--offline"])
    require("test result: ok" in output, f"spectra-agent tests must pass:\n{output}")

    run_command([str(SPECTRALANG), "run", FIXTURE])

    executable = ROOT / "target" / "r3212-agent-memory.exe"
    # --debug-info=none: the default PDB path hits a pre-existing MSVC LNK1318
    # limit on larger fixtures and is unrelated to this item.
    run_command(
        [str(SPECTRALANG), "compile", "--debug-info=none", "--emit-exe", str(executable), FIXTURE]
    )
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
    block = backlog.split("## R-3212 Agent Memory over std.ml", 1)[1].split("## R-3213", 1)[0]
    for term in ["Status: `complete`", "validate_r3212_agent_memory.py"]:
        require(term in block, f"backlog R-3212 missing {term}")

    plan = read("docs/agent-platform-plan.md")
    require(
        "Memory persistence has no path-bearing surface entry" in plan,
        "the plan must record the memory persistence adaptation",
    )
    # Registration in run_tests.ps1 belongs to the phase integrator (R-3221).
    require(Path(ROOT / "scripts" / "validate_r3212_agent_memory.py").is_file(), "validator missing")


def main() -> None:
    run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    validate_implementation()
    validate_contract()
    validate_catalog()
    validate_behavior()
    validate_planning()
    print("validated R-3212 agent memory over the runtime vector index")


if __name__ == "__main__":
    main()
