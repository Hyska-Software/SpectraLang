# R-3211 — model gateway, run and provider abstraction.
#
# Validates the run lifecycle, the provider abstraction (mock-first), the
# client-side ask_json validation, streaming, embedding, the compiler/midend
# surface and the language-level end-to-end fixture in JIT and AOT.
from __future__ import annotations

import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPECTRALANG = ROOT / "target" / "debug" / "spectralang.exe"
CARGO = shutil.which("cargo") or "cargo"
FIXTURE = "tests/validation/366_agent_model_surface.spectra"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3211 validation failed: {message}", file=sys.stderr)
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
    handles = read("runtime/src/handles/mod.rs")
    for term in ["AgentRun", "AgentChunkStream"]:
        require(term in handles, f"handles/mod.rs must declare {term}")

    builtin = read("compiler/src/semantic/builtin_std_core.rs")
    for term in [
        "agent_start",
        "agent_end",
        "ask_json",
        "ask_stream",
        "stream_next",
        "stream_close",
        "embed",
        "AgentSpec",
        "Report",
    ]:
        require(term in builtin, f"compiler surface missing {term}")

    lowering = read("midend/src/lowering_std_agent.rs")
    for term in ["agent_start", "ask_json", "stream_next", "embed"]:
        require(term in lowering, f"midend lowering table missing {term}")

    crate = ROOT / "packages/spectra-agent/src"
    for relative in [
        "run.rs",
        "spec.rs",
        "schema.rs",
        "policy.rs",
        "hosts.rs",
        "provider/mod.rs",
        "provider/mock.rs",
        "provider/openai_compatible.rs",
        "provider/transport.rs",
    ]:
        require((crate / relative).is_file(), f"spectra-agent missing {relative}")

    hosts = (crate / "hosts.rs").read_text(encoding="utf-8")
    for host in ["agent_start", "agent_end", "ask", "ask_json", "ask_stream", "stream_next", "stream_close", "embed"]:
        require(host in hosts, f"hosts.rs missing {host}")


def validate_catalog() -> None:
    with (ROOT / "packages/spectra-contract/catalog/stdlib.toml").open("rb") as handle:
        catalog = tomllib.load(handle)
    entries = {entry["path"]: entry for entry in catalog["entry"]}
    for function in [
        "std.agent.agent_start",
        "std.agent.agent_end",
        "std.agent.ask",
        "std.agent.ask_json",
        "std.agent.ask_stream",
        "std.agent.stream_next",
        "std.agent.stream_close",
        "std.agent.embed",
    ]:
        entry = entries.get(function)
        require(entry is not None, f"catalog missing {function}")
        require(entry.get("returns_value") is True, f"{function} must return a value")


def validate_behavior() -> None:
    output = run_command([CARGO, "test", "-q", "-p", "spectra-agent", "--offline"])
    require("test result: ok" in output, f"spectra-agent tests must pass:\n{output}")

    run_command([str(SPECTRALANG), "run", FIXTURE])

    executable = ROOT / "target" / "r3211-agent-model-surface.exe"
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
    block = backlog.split("## R-3211 Model Gateway, Run and Provider Abstraction", 1)[1].split(
        "## R-3212", 1
    )[0]
    for term in ["Status: `complete`", "validate_r3211_model_gateway.py"]:
        require(term in block, f"backlog R-3211 missing {term}")

    runner = read("run_tests.ps1")
    require("validate_r3211_model_gateway.py" in runner, "run_tests.ps1 must run R-3211")


def main() -> None:
    run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    validate_implementation()
    validate_catalog()
    validate_behavior()
    validate_planning()
    print("validated R-3211 model gateway, run and provider abstraction")


if __name__ == "__main__":
    main()
