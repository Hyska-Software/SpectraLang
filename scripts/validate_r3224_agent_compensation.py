# R-3224 — compensation declaration and execution.
#
# Validates the whole chain: the declaration-time registry check and the LIFO
# journaling (spectra-agent), the explicit rollback through the governed
# dispatch with replay-safe per-attempt records, the report integration
# (`compensations_pending`, `rolled_back`), the compiler/midend/catalog surface
# of `compensate`/`rollback`, the E3205 compile-time tool-name check, the
# contract probe, and the language-level fixture in JIT and AOT — where a
# counting tool proves the compensation executes exactly once and a replayed
# run does not execute it again.
from __future__ import annotations

import os
import json
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
FIXTURE = "tests/validation/380_agent_compensation.spectra"
ERROR_FIXTURE = "tests/errors/agent_compensate_unknown_tool.spectra"
JOURNAL_DIR = ROOT / ".spectra" / "r3224-compensation"
CATALOG = "packages/spectra-contract/catalog/stdlib.toml"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3224 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def run(args: list[str]) -> tuple[int, str]:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    return completed.returncode, completed.stdout


def run_command(args: list[str]) -> str:
    exit_code, output = run(args)
    if exit_code != 0:
        fail(f"command {' '.join(args)} failed with {exit_code}:\n{output}")
    return output


def parse_json_line(output: str) -> dict:
    """Parse the first output line as JSON (stderr is merged for diagnostics)."""
    return json.loads(output.splitlines()[0])


def diagnostics_of(report: dict) -> list[dict]:
    return [entry for file in report.get("files", []) for entry in file.get("diagnostics", [])]


def validate_implementation() -> None:
    compensate = read("packages/spectra-agent/src/compensate.rs")
    for term in [
        "pub(crate) fn compensate",
        "pub(crate) fn rollback",
        "PendingCompensation",
        "tools::lookup",
        "tools::enforce_run_grant",
        "tools::dispatch",
        "budget::charge_tool_call",
        "replay::resolve",
        "replay::commit",
        "Kind::Compensation",
        "Kind::Rollback",
        "state.pending_compensations",
        "mark_rolled_back",
        '"ok": false',
    ]:
        require(term in compensate, f"compensate.rs missing {term}")

    tools = read("packages/spectra-agent/src/tools.rs")
    for term in [
        "pub(crate) fn dispatch",
        "fn call_wrapper",
        "pub(crate) fn lookup",
        "budget::charge_tool_call",
        "replay::commit",
    ]:
        require(term in tools, f"tools.rs missing {term}")

    run_state = read("packages/spectra-agent/src/run.rs")
    for term in [
        "pub pending_compensations",
        "pub(crate) fn mark_rolled_back",
        "self.pending_compensations.len()",
    ]:
        require(term in run_state, f"run.rs missing {term}")

    replay = read("packages/spectra-agent/src/replay.rs")
    for term in [
        "Compensation,",
        "Rollback,",
        'Self::Compensation => "compensation"',
        'Self::Rollback => "rollback"',
    ]:
        require(term in replay, f"replay.rs missing {term}")

    hosts = read("packages/spectra-agent/src/hosts.rs")
    for term in [
        "fn compensate_host",
        "fn rollback_host",
        "spectra.std.agent.compensate",
        "spectra.std.agent.rollback",
    ]:
        require(term in hosts, f"hosts.rs missing {term}")

    lib = read("packages/spectra-agent/src/lib.rs")
    for term in [
        "mod compensate",
        "COMPENSATE_HOST_CALL",
        "ROLLBACK_HOST_CALL",
    ]:
        require(term in lib, f"spectra-agent lib.rs missing {term}")

    semantic = read("compiler/src/semantic/semantic_agent.rs")
    for term in [
        '"E3205"',
        "validate_compensate_tool_name",
        "closest_agent_tool",
        "levenshtein_distance",
    ]:
        require(term in semantic, f"semantic_agent.rs missing {term}")

    calls = read("compiler/src/semantic/semantic_expression_calls.rs")
    require(
        'name == "compensate"' in calls,
        "the call analyzer must dispatch the literal compensate tool-name check",
    )

    builtin = read("compiler/src/semantic/builtin_std_core.rs")
    for term in ['"compensate"', '"rollback"']:
        require(term in builtin, f"compiler surface missing {term}")

    lowering = read("midend/src/lowering_std_agent.rs")
    for term in [
        '"compensate"',
        '"rollback"',
        "spectra.std.agent.compensate",
        "spectra.std.agent.rollback",
    ]:
        require(term in lowering, f"midend lowering table missing {term}")

    lowering_tools = read("midend/src/lowering_agent_tools.rs")
    require(
        '"spectra.std.agent.compensate"' in lowering_tools
        and '"spectra.std.agent.rollback"' in lowering_tools,
        "the registration dispatch list must cover compensate and rollback",
    )

    docs = read("docs/diagnostics/error-code-reference.md")
    require("`E3205`" in docs, "error-code-reference.md must document E3205")


def validate_contract() -> None:
    with (ROOT / "scripts/stdlib_contract.toml").open("rb") as handle:
        manifest = tomllib.load(handle)
    probes = {str(probe["id"]): probe for probe in manifest.get("probe", [])}
    probe = probes.get("agent-compensation")
    require(probe is not None, "stdlib_contract.toml must carry the agent-compensation probe")
    require(probe.get("path") == FIXTURE, f"agent-compensation probe must point at {FIXTURE}")
    covers = set(probe.get("covers", []))
    for symbol in ["std.agent.compensate", "std.agent.rollback"]:
        require(symbol in covers, f"agent-compensation probe must cover {symbol}")


def validate_catalog() -> None:
    probe = ROOT / "target" / "r3224-catalog-probe" / "stdlib.toml"
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
    for path, signature, ir_return, binding in [
        (
            "std.agent.compensate",
            "fn(Run, string, string) -> Result<bool, Error>",
            "Result<bool,Error>",
            "spectra.std.agent.compensate",
        ),
        (
            "std.agent.rollback",
            "fn(Run, string) -> Result<int, Error>",
            "Result<int,Error>",
            "spectra.std.agent.rollback",
        ),
    ]:
        entry = entries.get(path)
        require(entry is not None, f"catalog missing {path}")
        require(entry.get("signature") == signature, f"{path} signature: {entry.get('signature')!r}")
        require(entry.get("ir_return") == ir_return, f"{path} ir_return: {entry.get('ir_return')!r}")
        require(entry.get("binding") == binding, f"{path} binding: {entry.get('binding')!r}")
        require(entry.get("fixture") == FIXTURE, f"{path} must cite {FIXTURE}")


def validate_gates() -> None:
    for script in [
        "scripts/generate_lowering_tables.py",
        "scripts/generate_host_calls.py",
        "scripts/generate_capability_reference.py",
    ]:
        output = run_command([sys.executable, script, "--check"])
        require("match" in output, f"{script} --check produced no verdict:\n{output}")


def validate_compile_time_check() -> None:
    # The valid fixture must stay diagnostic-free.
    exit_code, output = run([str(SPECTRALANG), "check", "--json", FIXTURE])
    require(exit_code == 0, f"check on {FIXTURE} exited {exit_code}:\n{output}")
    require(not diagnostics_of(parse_json_line(output)), "the valid fixture must be diagnostic-free")

    # A literal unknown tool name fails the build with E3205 and a
    # did-you-mean against the declared `refund`.
    exit_code, output = run([str(SPECTRALANG), "check", "--json", ERROR_FIXTURE])
    require(exit_code != 0, f"{ERROR_FIXTURE} must fail compilation")
    diagnostics = diagnostics_of(parse_json_line(output))
    matching = [d for d in diagnostics if d.get("code") == "E3205"]
    require(
        len(matching) == 1,
        f"{ERROR_FIXTURE} must report exactly one E3205, got {[d.get('code') for d in diagnostics]}",
    )
    hint = matching[0].get("hint") or ""
    require("refund" in hint, f"E3205 must suggest the closest tool name: {hint!r}")
    require(
        "runtime" in hint,
        f"E3205 must document that the runtime registry covers the cross-module case: {hint!r}",
    )


def read_records(run_id: str) -> list[dict]:
    path = JOURNAL_DIR / f"{run_id}.jsonl"
    require(path.exists(), f"the fixture must write {path}")
    return [
        json.loads(line)
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]


def validate_journal() -> None:
    """The fixture's journals: LIFO order, journaled outcomes, replay stability."""
    # The fresh run: one tool step, one declaration, one executed rollback.
    once = read_records("fixture-380-once")
    kinds = [record["kind"] for record in once]
    require(kinds == ["tool", "compensation", "rollback"], f"unexpected once kinds: {kinds}")
    attempt = once[-1]
    require(attempt["output"].find('"ok":true') >= 0, f"the rollback succeeded: {attempt['output']}")
    require(attempt["output"].find('"tool":"refund"') >= 0, f"the rollback names its tool: {attempt['output']}")
    require(
        sum(1 for record in once if record["kind"] == "rollback") == 1,
        "the compensation must be recorded exactly once after the replay",
    )

    # The pending run: a declaration with no rollback, so the leak is visible.
    pending = read_records("fixture-380-pending")
    require([record["kind"] for record in pending] == ["compensation"], "the pending run declares once")

    # The LIFO run: the compensation declared last executes first, fails at the
    # wrapper's schema check, and does not mask the one declared before it.
    lifo = read_records("fixture-380-lifo")
    kinds = [record["kind"] for record in lifo]
    require(
        kinds == ["compensation", "compensation", "rollback", "rollback"],
        f"unexpected lifo kinds: {kinds}",
    )
    first, second = lifo[-2], lifo[-1]
    require('"tool":"refund"' in first["output"] and '"ok":false' in first["output"], f"first attempt: {first['output']}")
    require('"tool":"release"' in second["output"] and '"ok":true' in second["output"], f"second attempt: {second['output']}")


def validate_behavior() -> None:
    output = run_command([CARGO, "test", "-q", "-p", "spectra-agent", "--offline"])
    require("test result: ok" in output, f"spectra-agent tests must pass:\n{output}")

    # JIT: the fixture cleans its journal, proves exactly-once plus replay, the
    # pending report and the LIFO failure.
    run_command([str(SPECTRALANG), "run", FIXTURE])
    validate_journal()

    # AOT parity: --debug-info=none avoids the pre-existing MSVC PDB limit.
    executable = ROOT / "target" / "r3224-agent-compensation.exe"
    run_command(
        [
            str(SPECTRALANG),
            "compile",
            "--debug-info=none",
            "--emit-exe",
            str(executable),
            FIXTURE,
        ]
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
    validate_journal()


def main() -> None:
    if not os.environ.get("SPECTRA_CLI_BUILT"):
        run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    if JOURNAL_DIR.exists():
        shutil.rmtree(JOURNAL_DIR)
    validate_implementation()
    validate_contract()
    validate_catalog()
    validate_gates()
    validate_compile_time_check()
    validate_behavior()
    print("validated R-3224 compensation declaration and execution")


if __name__ == "__main__":
    main()
