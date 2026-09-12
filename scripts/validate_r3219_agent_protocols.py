# R-3219 -- std.agent.protocol: A2A and ACP exposure.
#
# Validates the whole chain: the A2A agent card built from the authored
# description plus the derived tool surface, the journaled task lifecycle (a
# task id is a run id, `tasks/get` resumes from the journal), the ACP agent
# surface with its permission bridge over the approval primitive, the
# compiler/midend/catalog surface of the five new host calls, the contract
# probe, and the language-level fixture in JIT and AOT -- including a
# third-party HTTP client (this script) fetching the card, delegating a task
# and polling it over a real socket, and the ACP denial that aborts its action
# and is journaled.
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
import tomllib
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPECTRALANG = ROOT / "target" / "debug" / "spectralang.exe"
CARGO = shutil.which("cargo") or "cargo"
FIXTURE = "tests/validation/382_agent_protocol.spectra"
JOURNAL_DIR = ROOT / ".spectra" / "r3219-journal"
SERVE_DIR = ROOT / ".spectra" / "r3219-serve"
CATALOG = "packages/spectra-contract/catalog/stdlib.toml"

# The A2A methods and host-call names this item adds.
A2A_METHODS = ["message/send", "tasks/get", "tasks/cancel"]
SURFACE = {
    "std.agent.a2a_card": ("fn(Run, string) -> Result<string, Error>", "spectra.std.agent.a2a_card"),
    "std.agent.a2a_handle": ("fn(Run, string) -> Result<string, Error>", "spectra.std.agent.a2a_handle"),
    "std.agent.a2a_serve": ("fn(Run, string, string) -> Result<string, Error>", "spectra.std.agent.a2a_serve"),
    "std.agent.acp_handle": ("fn(Run, string) -> Result<string, Error>", "spectra.std.agent.acp_handle"),
    "std.agent.acp_permission": ("fn(Run, string) -> Result<bool, Error>", "spectra.std.agent.acp_permission"),
}


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3219 validation failed: {message}", file=sys.stderr)
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


def diagnostics_of(report: dict) -> list[dict]:
    return [entry for file in report.get("files", []) for entry in file.get("diagnostics", [])]


def records_of(path: Path) -> list[dict]:
    if not path.is_file():
        return []
    return [
        json.loads(line)
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]


# ── implementation ───────────────────────────────────────────────────────


def validate_implementation() -> None:
    a2a = read("packages/spectra-agent/src/protocol/a2a.rs")
    for term in [
        "message/send",
        "tasks/get",
        "tasks/cancel",
        "PROTOCOL_VERSION",
        "card_for",
        "tools::registered",
        "crate::act::act",
        "Kind::Task",
        "run::in_run_scope",
        "CARD_PATH",
    ]:
        require(term in a2a, f"the A2A adapter must implement {term}")

    acp = read("packages/spectra-agent/src/protocol/acp.rs")
    for term in [
        "session/request_permission",
        "session/new",
        "session/prompt",
        "session/cancel",
        "approve_with",
        "AcpClient",
        "set_acp_client",
        "loadSession",
        "default-deny (no ACP client attached)",
    ]:
        require(term in acp, f"the ACP adapter must implement {term}")

    # Both adapters share one listener and one governed dispatch.
    net = read("packages/spectra-agent/src/net.rs")
    require("accept_loop" in net, "the shared listener must own the accept loop")
    server = read("packages/spectra-agent/src/mcp/server.rs")
    require("net::serve" in server, "the MCP listener must reuse the shared listener")
    require("net::bind" in a2a, "the A2A listener must reuse the shared listener")

    approval = read("packages/spectra-agent/src/approval.rs")
    for term in ["pub(crate) fn approve_with", "decide_action"]:
        require(term in approval, f"the approval primitive must expose {term}")

    replay = read("packages/spectra-agent/src/replay.rs")
    require('Self::Task => "task"' in replay, "a task must be a journaled step kind")

    hosts = read("packages/spectra-agent/src/hosts.rs")
    for name in [
        "spectra.std.agent.a2a_card",
        "spectra.std.agent.a2a_handle",
        "spectra.std.agent.a2a_serve",
        "spectra.std.agent.acp_handle",
        "spectra.std.agent.acp_permission",
    ]:
        require(name in hosts, f"the host registry must carry {name}")

    lib = read("packages/spectra-agent/src/lib.rs")
    for constant in [
        "A2A_CARD_HOST_CALL",
        "A2A_HANDLE_HOST_CALL",
        "A2A_SERVE_HOST_CALL",
        "ACP_HANDLE_HOST_CALL",
        "ACP_PERMISSION_HOST_CALL",
    ]:
        require(constant in lib, f"lib.rs must export {constant}")

    # The crate tests are the deterministic half: both adapters have a round
    # trip and the ACP bridge proves denial/allow-once/allow-always journaling.
    for term in [
        "a_delegated_task_completes_and_is_journaled",
        "polling_a_task_resumes_it_from_the_journal_without_repeating_an_effect",
        "a_rejected_task_carries_the_stable_reason",
        "a_repeated_send_with_a_different_message_is_refused",
        "a_third_party_client_reaches_the_card_and_a_task_over_real_http",
    ]:
        require(term in a2a, f"the A2A crate tests must cover {term}")
    for term in [
        "a_denial_is_journaled_and_never_authorizes_the_action",
        "an_allow_always_is_journaled_cached_and_never_re_asked",
        "with_no_client_the_permission_is_denied_and_journaled",
        "initialize_advertises_only_implemented_capabilities",
        "a_capability_denial_traps_through_the_acp_prompt",
    ]:
        require(term in acp, f"the ACP crate tests must cover {term}")


def validate_surface() -> None:
    builtin = read("compiler/src/semantic/builtin_std_core.rs")
    for name in ["a2a_card", "a2a_handle", "a2a_serve", "acp_handle", "acp_permission"]:
        require(f'"{name}"' in builtin, f"compiler surface missing {name}")

    lowering = read("midend/src/lowering_std_agent.rs")
    for path, (_, binding) in SURFACE.items():
        name = path.rsplit(".", 1)[1]
        require(f'"{name}"' in lowering, f"midend lowering table missing {name}")
        require(binding in lowering, f"midend lowering table missing {binding}")

    dispatch = read("midend/src/lowering_agent_tools.rs")
    for binding in [
        "spectra.std.agent.a2a_card",
        "spectra.std.agent.a2a_handle",
        "spectra.std.agent.a2a_serve",
        "spectra.std.agent.acp_handle",
    ]:
        require(binding in dispatch, f"the registration dispatch list must cover {binding}")


def validate_contract() -> None:
    with (ROOT / "scripts" / "stdlib_contract.toml").open("rb") as handle:
        manifest = tomllib.load(handle)
    probes = {str(probe["id"]): probe for probe in manifest.get("probe", [])}
    probe = probes.get("agent-protocol")
    require(probe is not None, "stdlib_contract.toml must carry the agent-protocol probe")
    require(probe.get("path") == FIXTURE, f"the agent-protocol probe must point at {FIXTURE}")
    covers = set(probe.get("covers", []))
    for symbol in SURFACE:
        require(symbol in covers, f"the agent-protocol probe must cover {symbol}")


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
    for path, (signature, binding) in SURFACE.items():
        entry = entries.get(path)
        require(entry is not None, f"catalog missing {path}")
        require(entry.get("signature") == signature, f"{path} signature: {entry.get('signature')!r}")
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


def validate_documentation() -> None:
    docs = read("docs/agent-platform.md")
    for term in [
        "a2a_card",
        "acp_permission",
        "session/request_permission",
        "A task is a run",
        "default-deny",
    ]:
        require(term in docs, f"agent-platform.md must document {term}")
    reference = read("docs/AI-AGENT-REFERENCE.md")
    for term in ["a2a_card", "a2a_serve", "acp_handle", "acp_permission"]:
        require(term in reference, f"AI-AGENT-REFERENCE.md must document {term}")
    # The prose must stay outside the generated markers.
    begin = docs.index("<!-- BEGIN GENERATED CAPABILITY REFERENCE -->")
    require(
        "a2a_card" in docs[:begin],
        "the A2A/ACP prose must stay outside the generated capability reference",
    )


def validate_compile_time_check() -> None:
    exit_code, output = run([str(SPECTRALANG), "check", "--json", FIXTURE])
    require(exit_code == 0, f"check on {FIXTURE} exited {exit_code}:\n{output}")
    require(
        not diagnostics_of(json.loads(output.splitlines()[0])),
        "the valid fixture must be diagnostic-free",
    )


# ── behavior ─────────────────────────────────────────────────────────────

DENIAL_JOURNAL = JOURNAL_DIR / "fixture-382-acp-deny.jsonl"
SERVED_JOURNAL = JOURNAL_DIR / "fixture-382-served.jsonl"
OWNED = ROOT / "target" / "r3219" / "owned.txt"


def validate_fixture() -> None:
    run_command([str(SPECTRALANG), "run", FIXTURE])

    executable = ROOT / "target" / "r3219-agent-protocol.exe"
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
        f"the AOT binary must return 0, got {completed.returncode}:\n{completed.stdout}",
    )


def validate_denial_is_journaled() -> None:
    """ACP permission denial aborts the action and is journaled."""
    records = records_of(DENIAL_JOURNAL)
    approvals = [record for record in records if record.get("kind") == "approval"]
    require(len(approvals) == 1, f"the denial must be journaled once, found {len(approvals)}")
    require(approvals[0].get("output") == "false", f"the decision must be a denial: {approvals[0]}")
    attribution = approvals[0].get("attribution", "")
    require("no ACP client attached" in attribution, f"the attribution: {attribution!r}")
    require(not OWNED.exists(), f"a denied action must not run: {OWNED} exists")


def _env(extra: dict[str, str]) -> dict[str, str]:
    environment = dict(os.environ)
    environment.update(extra)
    return environment


def get(authority: str, path: str) -> dict:
    request = urllib.request.Request(f"http://{authority}{path}", method="GET")
    with urllib.request.urlopen(request, timeout=10) as response:
        return json.loads(response.read().decode("utf-8"))


def post(authority: str, payload: dict) -> dict:
    request = urllib.request.Request(
        f"http://{authority}/",
        data=json.dumps(payload).encode("utf-8"),
        headers={"content-type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        return json.loads(response.read().decode("utf-8"))


def validate_third_party_client() -> None:
    """A real HTTP client (this script) delegates a task to a compiled project."""
    if SERVE_DIR.exists():
        shutil.rmtree(SERVE_DIR)
    SERVE_DIR.mkdir(parents=True)
    if SERVED_JOURNAL.exists():
        SERVED_JOURNAL.unlink()

    process = subprocess.Popen(
        [str(SPECTRALANG), "run", FIXTURE],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        env=_env(
            {
                "SPECTRA_R3219_MODE": "serve",
                "SPECTRA_R3219_SERVE_DIR": str(SERVE_DIR),
            }
        ),
    )
    try:
        authority_file = SERVE_DIR / "authority.txt"
        deadline = time.time() + 60
        while not authority_file.is_file() and time.time() < deadline:
            if process.poll() is not None:
                fail(f"the serving fixture exited early:\n{process.stdout.read()}")
            time.sleep(0.1)
        require(authority_file.is_file(), "the fixture never published its A2A authority")
        authority = authority_file.read_text(encoding="utf-8").strip()
        require(authority.startswith("127.0.0.1:"), f"unexpected authority {authority!r}")

        # The card is derived: authored identity plus the compiled tool surface.
        card = get(authority, "/.well-known/agent-card.json")
        require(card["name"] == "Protocol Agent", f"the authored name: {card}")
        require(card["url"] == f"http://{authority}/", f"the served endpoint: {card['url']!r}")
        require(card["capabilities"]["streaming"] is False, f"streaming must not be advertised: {card}")
        require(
            card["capabilities"]["stateTransitionHistory"] is True,
            f"the journal is the state history: {card}",
        )
        skills = {skill["id"]: skill for skill in card["skills"]}
        require(set(skills) == {"add", "writer"}, f"derived skills: {sorted(skills)}")
        require(skills["add"]["description"] == "Adds two integers", f"skill description: {skills['add']}")

        # A delegated task runs through the governed dispatch on the server.
        sent = post(
            authority,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "message/send",
                "params": {
                    "message": {
                        "messageId": "fixture-382-served",
                        "role": "user",
                        "parts": [
                            {"kind": "text", "text": "spectra:tool=add {\"a\":40,\"b\":2}\nspectra:final=42"}
                        ],
                    }
                },
            },
        )
        task = sent["result"]["task"]
        require(task["status"]["state"] == "completed", f"the delegated task: {sent}")
        require(task["status"]["message"]["parts"][0]["text"] == "completed", f"the reason: {task}")
        require(task["artifacts"][0]["parts"][0]["text"] == "42", f"the result: {task}")
        require(
            task["metadata"]["spectra"]["report"]["tool_calls"] == 1,
            f"the governed dispatch must charge the call: {task}",
        )

        # Polling the task returns the same stored state.
        polled = post(
            authority,
            {"jsonrpc": "2.0", "id": 2, "method": "tasks/get", "params": {"id": "fixture-382-served"}},
        )
        require(polled["result"]["task"]["status"]["state"] == "completed", f"polled: {polled}")

        # An unknown task is the protocol's TaskNotFound, not an empty task.
        missing = post(
            authority,
            {"jsonrpc": "2.0", "id": 3, "method": "tasks/get", "params": {"id": "fixture-382-absent"}},
        )
        require(missing["error"]["code"] == -32001, f"unknown task: {missing}")
    finally:
        (SERVE_DIR / "stop").write_text("stop", encoding="utf-8")
        try:
            stdout, _ = process.communicate(timeout=60)
        except subprocess.TimeoutExpired:
            process.kill()
            fail("the serving fixture did not stop after the sentinel")

    require(
        process.returncode == 0,
        f"the serving fixture must return 0, got {process.returncode}:\n{stdout}",
    )

    # The client's task is a journaled run: two `task` steps and one governed
    # tool step under the task id's own file.
    records = records_of(SERVED_JOURNAL)
    kinds = [record.get("kind") for record in records]
    require(kinds.count("task") == 2, f"the task lifecycle records: {kinds}")
    require(kinds.count("tool") == 1, f"the governed tool step: {kinds}")
    terminal = [record for record in records if "completed" in record.get("output", "")]
    require(terminal, f"the terminal task state must be recorded: {records}")


def main() -> None:
    run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    if JOURNAL_DIR.exists():
        shutil.rmtree(JOURNAL_DIR)
    if OWNED.exists():
        OWNED.unlink()
    validate_implementation()
    validate_surface()
    validate_contract()
    validate_catalog()
    validate_gates()
    validate_documentation()
    validate_compile_time_check()

    # The crate tests own both adapters' round trips and the ACP decision
    # matrix (deny, allow-once, allow-always, replay).
    run_command([CARGO, "test", "-q", "-p", "spectra-agent", "--offline"])

    validate_fixture()
    validate_denial_is_journaled()
    validate_third_party_client()
    print("validated R-3219 A2A and ACP exposure")


if __name__ == "__main__":
    main()
