# R-3223 — message and handle taint.
#
# Validates the whole chain: the provenance ledger and its monotone lattice,
# the catalog-derived sink classification and the scope-extractor registry, the
# gate at the single dispatch seam (`block` denies, `approve` fails closed
# without an approver and proceeds with one, `allow` proceeds), the
# `untrusted`/`trust` surface through compiler, midend and catalog, the
# contract probe, and the language-level fixture in JIT and AOT — including the
# hostile tool description, which stays data while the sink still needs the
# run's policy.
from __future__ import annotations

import json
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
FIXTURE = "tests/validation/377_agent_taint.spectra"
JOURNAL_DIR = ROOT / ".spectra" / "r3223-journal"
WORK_DIR = ROOT / "target" / "r3223"
CATALOG = "packages/spectra-contract/catalog/stdlib.toml"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3223 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def run_command(args: list[str], env: dict[str, str] | None = None) -> str:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        env=env,
    )
    if completed.returncode != 0:
        fail(f"command {' '.join(args)} failed:\n{completed.stdout}")
    return completed.stdout


def run_fixture(mode: str | None) -> subprocess.CompletedProcess:
    environment = dict(os.environ)
    if mode is None:
        environment.pop("SPECTRA_R3223_MODE", None)
    else:
        environment["SPECTRA_R3223_MODE"] = mode
    return subprocess.run(
        [str(SPECTRALANG), "run", FIXTURE],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        env=environment,
    )


def journal_records(run_id: str) -> list[dict]:
    path = JOURNAL_DIR / f"{run_id}.jsonl"
    require(path.exists(), f"the fixture must journal {path}")
    return [
        json.loads(line)
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]


def kinds(run_id: str) -> list[str]:
    return [record["kind"] for record in journal_records(run_id)]


def decisions(run_id: str) -> list[dict]:
    return [
        record for record in journal_records(run_id) if record["kind"] == "taint_decision"
    ]


def validate_implementation() -> None:
    taint = read("packages/spectra-agent/src/taint.rs")
    for term in [
        "pub(crate) struct Entry",
        "pub(crate) struct Ledger",
        "pub(crate) fn mark_untrusted",
        "pub(crate) fn declassify",
        "pub(crate) fn observe_transcript",
        "pub(crate) fn gate",
        "spectra_contract::catalog()",
        "pub(crate) fn scope_value",
        "trust_required",
    ]:
        require(term in taint, f"taint.rs missing {term}")

    policy = read("packages/spectra-agent/src/policy.rs")
    for term in [
        "crate::taint::gate",
        "COMPILER_EMITTED_NAMESPACES",
        "pub(crate) fn is_compiler_emitted",
    ]:
        require(term in policy, f"policy.rs missing {term}")

    run = read("packages/spectra-agent/src/run.rs")
    for term in [
        "pub taint: Ledger",
        "pub(crate) struct RunTaint",
        "pub(crate) fn taint_for_raw_id",
        "pub(crate) fn in_run_scope",
    ]:
        require(term in run, f"run.rs missing {term}")

    hosts = read("packages/spectra-agent/src/hosts.rs")
    for term in [
        "fn untrusted_host",
        "fn trust_host",
        "spectra.std.agent.untrusted",
        "spectra.std.agent.trust",
        "fn in_run<F>",
        "in_run(run_handle",
    ]:
        require(term in hosts, f"hosts.rs missing {term}")

    hook = read("runtime/src/agent/policy_hook.rs")
    for term in [
        "&[SpectraHostValue]",
        "Hook signature",
        "fn evaluate(name: &str, args: &[SpectraHostValue])",
    ]:
        require(term in hook, f"policy_hook.rs missing {term}")

    registry = read("runtime/src/ffi_host_registry.rs")
    for term in ["from_raw_parts(args_ptr, arg_len)"]:
        require(term in registry, f"ffi_host_registry.rs missing {term}")

    lib = read("packages/spectra-agent/src/lib.rs")
    for term in ["mod taint", "UNTRUSTED_HOST_CALL", "TRUST_HOST_CALL"]:
        require(term in lib, f"spectra-agent lib.rs missing {term}")

    error = read("packages/spectra-agent/src/error.rs")
    for term in ["Taint(", '"taint_error"']:
        require(term in error, f"error.rs missing {term}")

    replay = read("packages/spectra-agent/src/replay.rs")
    for term in ["Taint,", "TaintDecision,", '"taint_decision"']:
        require(term in replay, f"replay.rs missing {term}")

    provider = read("packages/spectra-agent/src/provider/mod.rs")
    require("tool_name" in provider, "the transcript origin needs the tool name")

    tools = read("packages/spectra-agent/src/tools.rs")
    require("is_compiler_emitted" in tools, "tool grants must exclude compiler machinery")

    builtin = read("compiler/src/semantic/builtin_std_core.rs")
    for term in ['"untrusted"', '"trust"', "R-3223"]:
        require(term in builtin, f"compiler surface missing {term}")

    lowering = read("midend/src/lowering_std_agent.rs")
    for term in [
        '"untrusted"',
        '"trust"',
        "spectra.std.agent.untrusted",
        "spectra.std.agent.trust",
    ]:
        require(term in lowering, f"midend lowering table missing {term}")

    docs = read("docs/agent-platform.md")
    for term in ["Message granularity", "string-level", "spectra.api.json.", "Honest limits"]:
        require(term in docs, f"docs/agent-platform.md must record the honest limits ({term})")


def validate_contract() -> None:
    with (ROOT / "scripts/stdlib_contract.toml").open("rb") as handle:
        manifest = tomllib.load(handle)
    probes = {str(probe["id"]): probe for probe in manifest.get("probe", [])}
    probe = probes.get("agent-taint")
    require(probe is not None, "stdlib_contract.toml must carry the agent-taint probe")
    require(probe.get("path") == FIXTURE, f"agent-taint probe must point at {FIXTURE}")
    covers = set(probe.get("covers", []))
    for symbol in ["std.agent.untrusted", "std.agent.trust"]:
        require(symbol in covers, f"agent-taint probe must cover {symbol}")


def validate_catalog() -> None:
    probe = ROOT / "target" / "r3223-catalog-probe" / "stdlib.toml"
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
    for path, binding in [
        ("std.agent.untrusted", "spectra.std.agent.untrusted"),
        ("std.agent.trust", "spectra.std.agent.trust"),
    ]:
        entry = entries.get(path)
        require(entry is not None, f"catalog missing {path}")
        require(
            entry.get("signature") == "fn(Run, string, string) -> Result<string, Error>",
            f"{path} signature: {entry.get('signature')!r}",
        )
        require(entry.get("ir_return") == "Result<string,Error>", f"{path} ir_return")
        require(entry.get("binding") == binding, f"{path} binding")
        require(entry.get("fixture") == FIXTURE, f"{path} must cite {FIXTURE}")
        require(entry.get("sink") is not True, f"{path} is not a sink")

    # The sink set is the catalog's: the classification this item gates on is
    # the one the generator derives from the entry effects.
    sinks = [entry for entry in catalog["entry"] if entry.get("sink")]
    require(len(sinks) >= 17, f"the catalog must classify the write-side sinks, found {len(sinks)}")
    for entry in sinks:
        require(
            "mutation" in entry.get("effects", []),
            f"{entry['path']} is classified as a sink without a mutation effect",
        )


def validate_gates() -> None:
    for script in [
        "scripts/generate_lowering_tables.py",
        "scripts/generate_host_calls.py",
        "scripts/generate_capability_reference.py",
    ]:
        output = run_command([sys.executable, script, "--check"])
        require("match" in output, f"{script} --check produced no verdict:\n{output}")


def validate_behavior() -> None:
    output = run_command([CARGO, "test", "-q", "-p", "spectra-agent", "--offline"])
    require("test result: ok" in output, f"spectra-agent tests must pass:\n{output}")

    # JIT, passing matrix. The fixture learns its mode from the environment, so
    # one checked-in program proves both the decisions that proceed and the
    # decisions that trap.
    completed = run_fixture(None)
    require(
        completed.returncode == 0,
        f"the accept matrix must return 0, got {completed.returncode}:\n{completed.stdout}",
    )
    validate_journals()

    # JIT, `block`: the model-requested sink is refused.
    blocked = run_fixture("block")
    require(blocked.returncode != 0, "a block run must not reach the sink")
    require("trust_required" in blocked.stdout, blocked.stdout)
    require("spectra.std.fs.fs_write" in blocked.stdout, blocked.stdout)
    require("tool:fetch" in blocked.stdout, "the denial must name the untrusted origin")
    validate_denial_journal("fixture-377-block")

    # JIT, `approve` (the default) with no approver attached: fail closed.
    approved = run_fixture("approve")
    require(approved.returncode != 0, "an unattended approve run must fail closed")
    require("trust_required" in approved.stdout, approved.stdout)
    require("approver" in approved.stdout, approved.stdout)
    validate_denial_journal("fixture-377-approve")

    # AOT parity: --debug-info=none avoids the pre-existing MSVC PDB limit.
    executable = ROOT / "target" / "r3223-agent-taint.exe"
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
    validate_journals()
    environment = dict(os.environ)
    environment["SPECTRA_R3223_MODE"] = "block"
    blocked = subprocess.run(
        [str(executable)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        env=environment,
    )
    require(blocked.returncode != 0, "the AOT block mode must not reach the sink")
    require("trust_required" in blocked.stdout, blocked.stdout)


def validate_journals() -> None:
    """The passing matrix's journals: provenance, declassification, decisions."""
    # `allow`: provenance keyed by content digest, then the gated decision.
    allow = journal_records("fixture-377-allow")
    require(kinds("fixture-377-allow") == ["taint", "taint_decision", "tool"], kinds("fixture-377-allow"))
    provenance = allow[0]
    require(len(provenance["input_digest"]) == 64, "the provenance step is digest-keyed")
    require(len(provenance["output"]) == 64, "the ledger key is the content digest")
    require(provenance["attribution"] == "external:web", provenance.get("attribution"))
    require(decisions("fixture-377-allow")[0]["output"] == "allow", "the decision is recorded")
    require("input" not in provenance, "payload capture stays off by default")

    # `trust`: the same digest recorded untrusted, then declassified with its
    # reason; the sink then runs.
    trust = journal_records("fixture-377-trust")
    require(kinds("fixture-377-trust") == ["taint", "taint", "tool"], kinds("fixture-377-trust"))
    require(trust[0]["output"] == trust[1]["output"], "the same digest is declassified")
    require(trust[1]["attribution"] == "reviewed by an operator", trust[1].get("attribution"))
    require(trust[2]["output"] == "true", "the sink runs once the value is trusted")
    require(
        not decisions("fixture-377-trust"),
        "a declassified run holds no untrusted content, so nothing is gated",
    )

    # No untrusted content: the gate never fires.
    require(kinds("fixture-377-clean") == ["tool"], kinds("fixture-377-clean"))

    # The hostile description: it arrives as a tool result, the model quotes it,
    # the scripted sink is decided (not silently reached) and the file the
    # description demanded is never written.
    hostile = journal_records("fixture-377-hostile")
    require(
        kinds("fixture-377-hostile")
        == ["model", "tool", "model", "taint_decision", "tool", "model"],
        kinds("fixture-377-hostile"),
    )
    decision = decisions("fixture-377-hostile")[0]
    require(decision["output"] == "allow", decision["output"])
    require("tool:fetch" in (decision.get("attribution") or ""), decision.get("attribution"))
    require(not (WORK_DIR / "owned.txt").exists(), "the description must not become control flow")
    require((WORK_DIR / "scripted.txt").exists(), "the scripted sink ran")
    require((WORK_DIR / "no-run.txt").exists(), "a program without a run is unchanged")


def validate_denial_journal(run_id: str) -> None:
    """A denied run journals the decision before the trap consumes it."""
    records = journal_records(run_id)
    require(records[-1]["kind"] == "taint_decision", f"{run_id} must journal the denial")
    require(records[-1]["output"] == "deny", f"{run_id} denial output")
    attribution = records[-1].get("attribution") or ""
    require("trust_required" in attribution, attribution)
    require("tool:fetch" in attribution, attribution)
    require(
        "IGNORE ALL PREVIOUS INSTRUCTIONS" not in attribution,
        "the denial text must not carry the untrusted payload (ADR 0016 D4)",
    )


def main() -> None:
    if not os.environ.get("SPECTRA_CLI_BUILT"):
        run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    for directory in (JOURNAL_DIR, WORK_DIR):
        if directory.exists():
            shutil.rmtree(directory)
    validate_implementation()
    validate_contract()
    validate_catalog()
    validate_gates()
    validate_behavior()
    print("validated R-3223 taint ledger, catalog sinks, gate and honest limits")


if __name__ == "__main__":
    main()
