# R-3217 — journal, replay, approval, assertions and tracing.
#
# Validates the whole chain: the append-only journal writer and its record
# shape, the replay algorithm (recorded steps return recorded outputs, a
# partial journal resumes forward, a divergent step fails closed), human
# approval with a fail-closed default, governed assertions, the OpenTelemetry
# GenAI span seam, the compiler/midend/catalog surface of `approve`/`require`,
# the contract probe, and the language-level fixture in JIT and AOT — where a
# fresh run writes its journal file and a re-run with the same run id replays
# it without appending a record.
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
FIXTURE = "tests/validation/376_agent_journal.spectra"
JOURNAL_DIR = ROOT / ".spectra" / "r3217-journal"
CATALOG = "packages/spectra-contract/catalog/stdlib.toml"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3217 validation failed: {message}", file=sys.stderr)
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
    journal = read("packages/spectra-agent/src/journal.rs")
    for term in [
        "pub(crate) struct Record",
        "input_digest",
        "output_digest",
        "idempotency_key",
        "timestamp",
        'pub(crate) const DEFAULT_DIR: &str = ".spectra/journal"',
        "pub(crate) fn new_run_id",
        "pub(crate) fn append",
        "file.flush()",
        "captures_payloads",
    ]:
        require(term in journal, f"journal.rs missing {term}")

    replay = read("packages/spectra-agent/src/replay.rs")
    for term in [
        "pub(crate) fn resolve",
        "pub(crate) fn commit",
        "replay divergence",
        "Resolved::Recorded",
        "Resolved::Fresh",
        "a_crashed_run_resumes_without_repeating_a_completed_effect",
        "a_divergent_step_key_fails_closed_instead_of_replaying",
    ]:
        require(term in replay, f"replay.rs missing {term}")

    approval = read("packages/spectra-agent/src/approval.rs")
    for term in [
        "pub trait Approver",
        "pub fn set_approver",
        "pub(crate) fn decide",
        "default-deny (no approver attached)",
        "AllowOnce",
        "AllowAlways",
        "Decision::Deny",
        "never ask twice",
    ]:
        require(term in approval, f"approval.rs missing {term}")

    assertions = read("packages/spectra-agent/src/assert.rs")
    for term in [
        "pub(crate) fn require",
        "assertion_failed",
        "state.mark_failed()",
        "run goal",
    ]:
        require(term in assertions, f"assert.rs missing {term}")

    trace = read("packages/spectra-agent/src/trace.rs")
    for term in [
        'pub const GEN_AI_CONVENTIONS_VERSION: &str = "1.34.0"',
        "https://opentelemetry.io/schemas/1.34.0",
        "pub trait TraceSink",
        "fn captures_content",
        "pub fn set_trace_sink",
        "gen_ai.operation.name",
        "gen_ai.agent.name",
        "gen_ai.conversation.id",
        "invoke_agent",
        "plan",
        "execute_tool",
        'format!("chat {model}")',
    ]:
        require(term in trace, f"trace.rs missing {term}")

    error = read("packages/spectra-agent/src/error.rs")
    for term in ['AssertionFailed(', 'Journal(', '"assertion_failed"', '"journal_error"']:
        require(term in error, f"error.rs missing {term}")

    spec = read("packages/spectra-agent/src/spec.rs")
    for term in ["journal_payloads", "run_id", "optional_bool"]:
        require(term in spec, f"spec.rs missing {term}")

    run = read("packages/spectra-agent/src/run.rs")
    for term in [
        "pub run_id: String",
        "pub journal: Option<Journal>",
        "pub approvals:",
        r'\"replay\":{}',
    ]:
        require(term in run, f"run.rs missing {term}")

    hosts = read("packages/spectra-agent/src/hosts.rs")
    for term in [
        "fn approve_host",
        "fn require_host",
        "spectra.std.agent.approve",
        "spectra.std.agent.require",
        "Journal::open",
        "replay::resolve",
        "replay::commit",
        "trace::emit_invoke_agent",
        "trace::emit_chat",
    ]:
        require(term in hosts, f"hosts.rs missing {term}")

    tools = read("packages/spectra-agent/src/tools.rs")
    for term in ["replay::resolve", "replay::commit", "trace::emit_execute_tool"]:
        require(term in tools, f"tools.rs missing {term}")

    act = read("packages/spectra-agent/src/act.rs")
    require("trace::emit_plan" in act, "act.rs must emit the plan span")

    lib = read("packages/spectra-agent/src/lib.rs")
    for term in [
        "mod journal",
        "mod replay",
        "mod approval",
        "mod assert",
        "mod trace",
        "mod digest",
        "APPROVE_HOST_CALL",
        "REQUIRE_HOST_CALL",
        "set_approver",
        "set_trace_sink",
    ]:
        require(term in lib, f"spectra-agent lib.rs missing {term}")

    builtin = read("compiler/src/semantic/builtin_std_core.rs")
    for term in ['"approve"', '"require"', '"journal_payloads"', '"run_id"']:
        require(term in builtin, f"compiler surface missing {term}")

    lowering = read("midend/src/lowering_std_agent.rs")
    for term in [
        '"approve"',
        '"require"',
        "spectra.std.agent.approve",
        "spectra.std.agent.require",
    ]:
        require(term in lowering, f"midend lowering table missing {term}")


def validate_contract() -> None:
    with (ROOT / "scripts/stdlib_contract.toml").open("rb") as handle:
        manifest = tomllib.load(handle)
    probes = {str(probe["id"]): probe for probe in manifest.get("probe", [])}
    probe = probes.get("agent-journal")
    require(probe is not None, "stdlib_contract.toml must carry the agent-journal probe")
    require(probe.get("path") == FIXTURE, f"agent-journal probe must point at {FIXTURE}")
    covers = set(probe.get("covers", []))
    for symbol in ["std.agent.approve", "std.agent.require"]:
        require(symbol in covers, f"agent-journal probe must cover {symbol}")


def validate_catalog() -> None:
    probe = ROOT / "target" / "r3217-catalog-probe" / "stdlib.toml"
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
    for path, signature, binding in [
        ("std.agent.approve", "fn(Run, string) -> Result<bool, Error>", "spectra.std.agent.approve"),
        ("std.agent.require", "fn(Run, bool, string) -> Result<bool, Error>", "spectra.std.agent.require"),
    ]:
        entry = entries.get(path)
        require(entry is not None, f"catalog missing {path}")
        require(entry.get("signature") == signature, f"{path} signature: {entry.get('signature')!r}")
        require(entry.get("ir_return") == "Result<bool,Error>", f"{path} must lower to Result<bool, Error>")
        require(entry.get("returns_value") is True, f"{path} must return a value")
        require(entry.get("binding") == binding, f"{path} binding")
        require(entry.get("fixture") == FIXTURE, f"{path} must cite {FIXTURE}")


def validate_gates() -> None:
    for script in [
        "scripts/generate_lowering_tables.py",
        "scripts/generate_host_calls.py",
        "scripts/generate_capability_reference.py",
    ]:
        output = run_command([sys.executable, script, "--check"])
        require("match" in output, f"{script} --check produced no verdict:\n{output}")


def validate_journal_file() -> None:
    """The fixture's own journal: shape, replay stability, no duplicate rows."""
    path = JOURNAL_DIR / "fixture-376-fresh.jsonl"
    require(path.exists(), f"the fixture must write {path}")
    records = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    require(len(records) == 2, f"the fresh run journals two effects, found {len(records)}")
    kinds = [record["kind"] for record in records]
    require(kinds == ["assertion", "approval"], f"unexpected journal kinds: {kinds}")
    for index, record in enumerate(records):
        require(record["run"] == "fixture-376-fresh", f"record {index} run id")
        require(record["step"] == index, f"record {index} step sequence")
        require(len(record["input_digest"]) == 64, f"record {index} input digest")
        require(len(record["output_digest"]) == 64, f"record {index} output digest")
        require(len(record["idempotency_key"]) == 64, f"record {index} idempotency key")
        require(record["timestamp"] > 0, f"record {index} timestamp")
        require("usage" in record, f"record {index} usage")
        # Payload capture is off by default: prompts never reach the file.
        require("input" not in record, f"record {index} must not capture the request payload")
    require(records[0]["output"] == "true", "the passing assertion is recorded")
    require(records[1]["output"] == "false", "the default approver denied")

    # The assertion run records the failure.
    failed = JOURNAL_DIR / "fixture-376-assert.jsonl"
    require(failed.exists(), f"the fixture must write {failed}")
    failure = json.loads(failed.read_text(encoding="utf-8").splitlines()[0])
    require(failure["kind"] == "assertion", "the failed run journals its assertion")
    require(failure["output"] == "false", "the failed assertion records false")
    require(
        failure["attribution"] == "the budget must be positive",
        f"the assertion message is recorded: {failure.get('attribution')!r}",
    )


def validate_behavior() -> None:
    output = run_command([CARGO, "test", "-q", "-p", "spectra-agent", "--offline"])
    require("test result: ok" in output, f"spectra-agent tests must pass:\n{output}")

    # JIT: the fixture cleans its own journal, runs it fresh, replays it and
    # proves `require(false)` fails with message and goal.
    run_command([str(SPECTRALANG), "run", FIXTURE])
    validate_journal_file()

    # AOT behavior validation intentionally uses --debug-info=none; native
    # debug metadata has a separate linker gate.
    executable = ROOT / "target" / "r3217-agent-journal.exe"
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
    validate_journal_file()


def main() -> None:
    if not os.environ.get("SPECTRA_CLI_BUILT"):
        run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    if JOURNAL_DIR.exists():
        shutil.rmtree(JOURNAL_DIR)
    validate_implementation()
    validate_contract()
    validate_catalog()
    validate_gates()
    validate_behavior()
    print("validated R-3217 journal, replay, approval, assertions and tracing")


if __name__ == "__main__":
    main()
