# R-3202 — `spectralang surface --json`.
#
# Validates the implementation seams, the CLI contract (schema, determinism,
# token-budget trimming, failure payloads, exit codes) and the planning
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
PROJECT = "tests/projects/valid/phase21_async_pipeline"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3202 validation failed: {message}", file=sys.stderr)
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


def run_surface(extra_args: list[str]) -> tuple[int, str]:
    completed = subprocess.run(
        [str(SPECTRALANG), "surface", "--json", *extra_args],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    return completed.returncode, completed.stdout


def parse_json_line(output: str) -> dict:
    """Parse the first output line as JSON.

    Stderr is merged into stdout for diagnostics, so failure runs append an
    `error: ...` line after the JSON payload.
    """
    return json.loads(output.splitlines()[0])


def validate_implementation() -> None:
    surface = read("compiler/src/semantic/surface.rs")
    for term in [
        "pub struct SurfaceSnapshot",
        "pub struct SurfaceFunction",
        "pub struct SurfaceModule",
        "pub fn snapshot_from_registry",
        "is_builtin",
    ]:
        require(term in surface, f"compiler surface snapshot missing {term}")

    pipeline = read("compiler/src/pipeline.rs")
    require(
        "pub fn registry(&self) -> Arc<RwLock<ModuleRegistry>>" in pipeline,
        "CompilationPipeline must expose registry()",
    )

    cli_surface = read("tools/spectra-cli/src/cli_surface.rs")
    for term in [
        "fn parse_surface_invocation",
        "fn execute_surface",
        "fn apply_surface_token_budget",
        "spectralang.surface.v1",
        "type_members",
        "function_tail",
    ]:
        require(term in cli_surface, f"cli_surface.rs missing {term}")

    lib = read("tools/spectra-cli/src/lib.rs")
    require('include!("cli_surface.rs");' in lib, "lib.rs must include cli_surface.rs")
    require("Surface(SurfaceOptions)" in lib, "lib.rs must wire CliAction::Surface")

    parse_core = read("tools/spectra-cli/src/cli_parse_core.rs")
    require('Some("surface")' in parse_core, "cli_parse_core.rs must dispatch surface")
    require("execute_surface(options)" in parse_core, "surface action must execute")

    help_text = read("tools/spectra-cli/src/cli_help.rs")
    require("fn print_surface_help" in help_text, "help must document surface")


def validate_cli_contract() -> None:
    exit_code, output = run_surface([PROJECT])
    require(exit_code == 0, f"surface on {PROJECT} exited {exit_code}:\n{output}")

    report = parse_json_line(output)

    require(report.get("schema") == "spectralang.surface.v1", "schema field mismatch")
    require(report.get("success") is True, "success must be true for a valid project")
    require(report.get("trimmed", {}).get("applied") == [], "unbudgeted run must not trim")

    modules = report.get("modules")
    require(isinstance(modules, list) and modules, "modules must be a non-empty list")
    paths = {module["path"] for module in modules}
    require("main" in paths, f"project modules missing 'main': {sorted(paths)}")

    main_module = next(module for module in modules if module["path"] == "main")
    function_names = {fn["name"] for fn in main_module["functions"]}
    require(
        "phase21_total" in function_names,
        f"main module must expose phase21_total: {sorted(function_names)}",
    )
    require(
        all(fn["visibility"] == "public" for fn in main_module["functions"]),
        "surface must list public exports only",
    )
    require(
        any(fn["is_async"] for fn in main_module["functions"]),
        "async exports must carry is_async=true",
    )

    _, second = run_surface([PROJECT])
    require(output == second, "surface output must be byte-identical across runs")

    exit_code, trimmed_output = run_surface(["--tokens", "120", PROJECT])
    require(exit_code == 0, f"trimmed surface run exited {exit_code}:\n{trimmed_output}")
    trimmed = parse_json_line(trimmed_output)
    require(
        trimmed.get("trimmed", {}).get("applied"),
        "a 120-token budget must report which trim tiers were applied",
    )
    require(
        trimmed.get("estimated_tokens", 10**9) <= 400,
        "trimmed payload must roughly respect the requested budget",
    )

    exit_code, failure_output = run_surface(["does/not/exist"])
    require(exit_code == 64, f"missing path must exit 64, got {exit_code}")
    failure = parse_json_line(failure_output)
    require(failure.get("success") is False, "failure payload must set success=false")
    require(failure.get("error"), "failure payload must carry an error message")

    completed = subprocess.run(
        [str(SPECTRALANG), "surface", PROJECT],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    require(
        completed.returncode == 64,
        f"surface without --json must exit 64, got {completed.returncode}",
    )


def validate_planning() -> None:
    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-3202 spectralang surface --json", 1)[1].split("## R-3203", 1)[0]
    for term in [
        "Status: `complete`",
        "docs/agent-platform-plan.md",
        "validate_r3202_surface_json.py",
    ]:
        require(term in block, f"backlog R-3202 missing {term}")

    runner = read("run_tests.ps1")
    require(
        "validate_r3202_surface_json.py" in runner,
        "run_tests.ps1 must run R-3202",
    )


def main() -> None:
    run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    validate_implementation()
    validate_cli_contract()
    validate_planning()
    print("validated R-3202 surface --json")


if __name__ == "__main__":
    main()
