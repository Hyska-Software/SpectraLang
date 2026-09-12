# R-3203 — `spectralang impact --json`.
#
# Validates the midend call-graph extraction, the CLI contract (callers,
# field users, unknown symbols, dynamic-dispatch honesty) and the planning
# bookkeeping for the item.
from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPECTRALANG = ROOT / "target" / "debug" / "spectralang.exe"
CARGO = shutil.which("cargo") or "cargo"
FIXTURE = "tests/validation/364_impact_analysis.spectra"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3203 validation failed: {message}", file=sys.stderr)
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


def run_impact(symbol: str) -> tuple[int, str]:
    completed = subprocess.run(
        [str(SPECTRALANG), "impact", "--json", symbol, FIXTURE],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    return completed.returncode, completed.stdout


def parse_json_line(output: str) -> dict:
    """Parse the first output line as JSON (stderr is merged for diagnostics)."""
    return json.loads(output.splitlines()[0])


def validate_implementation() -> None:
    callgraph = read("midend/src/callgraph.rs")
    for term in [
        "pub fn direct_calls",
        "pub struct ImpactIndex",
        "pub fn callers_of",
        "pub fn field_users_of",
        "pub fn unresolved_dynamic",
    ]:
        require(term in callgraph, f"midend callgraph missing {term}")

    inlining = read("midend/src/passes/function_inlining.rs")
    require(
        "callgraph::direct_calls" in inlining or "crate::callgraph::direct_calls" in inlining,
        "function_inlining must consume the shared call graph",
    )

    lib = read("midend/src/lib.rs")
    require("pub mod callgraph;" in lib, "midend lib.rs must export callgraph")

    cli = read("tools/spectra-cli/src/cli_impact.rs")
    for term in ["fn execute_impact", "pub fn parse_impact_invocation", "spectralang.impact.v1"]:
        require(term in cli.replace("fn parse_impact_invocation", "pub fn parse_impact_invocation"), f"cli_impact missing {term}")

    runner = read("tools/spectra-cli/src/cli_parse_core.rs")
    require('Some("impact")' in runner, "cli_parse_core.rs must dispatch impact")


def validate_cli_contract() -> None:
    exit_code, output = run_impact("Point.x")
    require(exit_code == 0, f"field query exited {exit_code}:\n{output}")
    report = parse_json_line(output)
    require(report.get("schema") == "spectralang.impact.v1", "schema mismatch")
    require(report.get("success") is True, "success must be true")
    functions = report.get("affected_functions", [])
    require(
        any("reader" in name for name in functions),
        f"Point.x must list the field reader: {functions}",
    )
    require(
        any("writer" in name for name in functions),
        f"Point.x must list the constructor: {functions}",
    )
    require("Point" in report.get("affected_types", []), "field query must report the owning type")

    exit_code, output = run_impact("total")
    require(exit_code == 0, f"function query exited {exit_code}:\n{output}")
    report = parse_json_line(output)
    callers = report.get("affected_functions", [])
    require(
        any(name.endswith("::main") for name in callers),
        f"total must list main as caller: {callers}",
    )
    require(report.get("dynamic_dispatch") is False, "fixture has no dynamic calls")

    exit_code, output = run_impact("definitely_not_a_symbol")
    require(exit_code == 65, f"unknown symbol must exit 65, got {exit_code}")
    report = parse_json_line(output)
    require(report.get("success") is False, "unknown symbol must set success=false")
    require(report.get("near_matches") is not None, "unknown symbol must report near matches")

    completed = subprocess.run(
        [str(SPECTRALANG), "impact", "total", FIXTURE],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    require(
        completed.returncode == 64,
        f"impact without --json must exit 64, got {completed.returncode}",
    )


def validate_planning() -> None:
    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-3203 spectralang impact --json", 1)[1].split("## R-3204", 1)[0]
    for term in ["Status: `complete`", "docs/agent-platform-plan.md", "validate_r3203_impact_json.py"]:
        require(term in block, f"backlog R-3203 missing {term}")

    runner = read("run_tests.ps1")
    require("validate_r3203_impact_json.py" in runner, "run_tests.ps1 must run R-3203")


def main() -> None:
    run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    validate_implementation()
    validate_cli_contract()
    validate_planning()
    print("validated R-3203 impact --json")


if __name__ == "__main__":
    main()
