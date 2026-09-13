# R-3216 — budget accounting and cooperative cancellation.
#
# Validates the budget module (four ceilings, accrual against the run
# counters, the tool-call gate, fail-closed cost accounting), the cooperative
# cancellation wiring (cancellation tokens registered with the run, guards at
# both task boundaries), the report contract (status + ceiling name), the
# compiler/midend/runtime surface of `budget_remaining`, the contract probe and
# the language-level fixture in JIT and AOT.
from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPECTRALANG = Path(
    os.environ.get("SPECTRALANG_BINARY") or (ROOT / "target" / "debug" / "spectralang.exe")
)
CARGO = shutil.which("cargo") or "cargo"
FIXTURE = "tests/validation/372_agent_budget.spectra"
CATALOG = "packages/spectra-contract/catalog/stdlib.toml"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3216 validation failed: {message}", file=sys.stderr)
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
    budget = read("packages/spectra-agent/src/budget.rs")
    for term in [
        "pub(crate) enum Ceiling",
        "Tokens",
        "Cost",
        "Seconds",
        "ToolCalls",
        "pub(crate) struct Budget",
        "fn compile",
        "fn exhausted",
        "fn crossed",
        "fn remaining_tokens",
        "pub(crate) fn guard",
        "pub(crate) fn settle",
        "pub(crate) fn charge_tool_call",
        "pub(crate) fn remaining",
        "cost_ceiling_is_enforceable",
        "CancellationToken",
        "budget_exceeded",
    ]:
        require(term in budget, f"budget.rs missing {term}")

    error = read("packages/spectra-agent/src/error.rs")
    for term in [
        "BudgetExceeded(",
        "CostAccountingUnavailable(",
        '"budget_exceeded"',
        '"cost_accounting_unavailable"',
    ]:
        require(term in error, f"error.rs missing {term}")

    run = read("packages/spectra-agent/src/run.rs")
    for term in [
        "pub budget: Budget",
        "pub ceiling: Option<Ceiling>",
        "pub cancelled: bool",
        "pub tasks: Vec<CancellationToken>",
        "fn mark_cancelled",
        "fn cancelled_error",
        "fn report_ceiling",
        '"ceiling\\":\\"{}',
        "pub(crate) fn register_task",
        "pub(crate) fn unregister_task",
    ]:
        require(term in run, f"run.rs missing {term}")

    hosts = read("packages/spectra-agent/src/hosts.rs")
    for term in [
        "fn write_run_task",
        "fn budget_remaining_host",
        "spectra.std.agent.budget_remaining",
        "budget::guard_task",
        "budget::settle_task",
        "budget::cost_ceiling_is_enforceable",
        "spawn_cancellable_background_task",
    ]:
        require(term in hosts, f"hosts.rs missing {term}")

    lib = read("packages/spectra-agent/src/lib.rs")
    for term in ["mod budget", "BUDGET_REMAINING_HOST_CALL"]:
        require(term in lib, f"spectra-agent lib.rs missing {term}")

    mock = read("packages/spectra-agent/src/provider/mock.rs")
    for term in ["spectra:sleep-ms=", "requested_sleep_ms", "fn reports_cost"]:
        require(term in mock, f"mock provider missing {term}")

    provider = read("packages/spectra-agent/src/provider/mod.rs")
    require("fn reports_cost" in provider, "provider trait must declare reports_cost")

    builtin = read("compiler/src/semantic/builtin_std_core.rs")
    for term in ["budget_remaining"]:
        require(term in builtin, f"compiler surface missing {term}")

    lowering = read("midend/src/lowering_std_agent.rs")
    for term in ['"budget_remaining"', "spectra.std.agent.budget_remaining"]:
        require(term in lowering, f"midend lowering table missing {term}")


def validate_contract() -> None:
    with (ROOT / "scripts/stdlib_contract.toml").open("rb") as handle:
        manifest = tomllib.load(handle)
    probes = {str(probe["id"]): probe for probe in manifest.get("probe", [])}
    probe = probes.get("agent-budget")
    require(probe is not None, "stdlib_contract.toml must carry the agent-budget probe")
    require(probe.get("path") == FIXTURE, f"agent-budget probe must point at {FIXTURE}")
    require(
        "std.agent.budget_remaining" in set(probe.get("covers", [])),
        "agent-budget probe must cover std.agent.budget_remaining",
    )


def validate_catalog() -> None:
    probe = ROOT / "target" / "r3216-catalog-probe" / "stdlib.toml"
    probe.parent.mkdir(parents=True, exist_ok=True)
    if probe.exists():
        probe.unlink()
    probe_args = ["--output", str(probe.relative_to(ROOT))]
    first = run_command([sys.executable, "scripts/generate_stdlib_catalog.py", *probe_args])
    require("generated" in first, f"catalog generator produced no report:\n{first}")
    once = probe.read_bytes()
    run_command([sys.executable, "scripts/generate_stdlib_catalog.py", *probe_args])
    twice = probe.read_bytes()
    require(once == twice, "catalog generation must be idempotent")
    require(
        once == (ROOT / CATALOG).read_bytes(),
        "the checked-in catalog must be the generator's output",
    )

    with (ROOT / CATALOG).open("rb") as handle:
        catalog = tomllib.load(handle)
    entries = {entry["path"]: entry for entry in catalog["entry"]}
    entry = entries.get("std.agent.budget_remaining")
    require(entry is not None, "catalog missing std.agent.budget_remaining")
    require(entry.get("signature") == "fn(Run) -> Result<int, Error>", "budget_remaining signature")
    require(
        entry.get("ir_return") == "Result<int,Error>",
        "budget_remaining must lower to Result<int, Error>",
    )
    require(entry.get("returns_value") is True, "budget_remaining must return a value")
    require(entry.get("binding") == "spectra.std.agent.budget_remaining", "budget_remaining binding")
    require(entry.get("fixture") == FIXTURE, f"budget_remaining must cite {FIXTURE}")


def validate_gates() -> None:
    for script in [
        "scripts/generate_lowering_tables.py",
        "scripts/generate_host_calls.py",
    ]:
        output = run_command([sys.executable, script, "--check"])
        require("match" in output, f"{script} --check produced no verdict:\n{output}")


def validate_behavior() -> None:
    output = run_command([CARGO, "test", "-q", "-p", "spectra-agent", "--offline"])
    require("test result: ok" in output, f"spectra-agent tests must pass:\n{output}")

    run_command([str(SPECTRALANG), "run", FIXTURE])

    executable = ROOT / "target" / "r3216-agent-budget.exe"
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


def main() -> None:
    if not os.environ.get("SPECTRA_CLI_BUILT"):
        run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    validate_implementation()
    validate_contract()
    validate_catalog()
    validate_gates()
    validate_behavior()
    print("validated R-3216 budget accounting and cooperative cancellation")


if __name__ == "__main__":
    main()
