# R-3218 -- MCP client and server over HTTP.
#
# Validates the whole chain: the client's HTTP discovery that registers remote
# tools as governed registry entries and records every peer description and
# schema as untrusted provenance, the server's `tools/list`/`tools/call` over
# the derived `#[agent_tool]` surface (with `tools/call` flowing through the
# governed dispatch), the compiler/midend/catalog surface of
# `mcp_connect`/`mcp_handle`/`mcp_serve`, the contract probe, and the
# language-level fixture in JIT and AOT -- including a third-party HTTP client
# (this script) calling a compiled Spectra project's tools over a real socket,
# and the hostile-description mode where the injected text can only ever add a
# gate.
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
import tomllib
import urllib.error
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPECTRALANG = Path(
    os.environ.get("SPECTRALANG_BINARY") or (ROOT / "target" / "debug" / "spectralang.exe")
)
CARGO = shutil.which("cargo") or "cargo"
FIXTURE = "tests/validation/381_agent_mcp.spectra"
JOURNAL_DIR = ROOT / ".spectra" / "r3218-journal"
SERVE_DIR = ROOT / ".spectra" / "r3218-serve"
CATALOG = "packages/spectra-contract/catalog/stdlib.toml"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3218 validation failed: {message}", file=sys.stderr)
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


# ── implementation ───────────────────────────────────────────────────────


def validate_implementation() -> None:
    client = read("packages/spectra-agent/src/mcp/client.rs")
    for term in [
        '"tools/list"',
        '"tools/call"',
        "Kind::Mcp",
        "mark_untrusted",
        "require_server_grant",
        "register_remote",
    ]:
        require(term in client, f"the MCP client must implement {term}")

    server = read("packages/spectra-agent/src/mcp/server.rs")
    for term in [
        '"tools/list"',
        '"tools/call"',
        "act::tool_call",
        "in_run_scope",
        "tools::registered",
        "readOnlyHint",
        "destructiveHint",
    ]:
        require(term in server, f"the MCP server must implement {term}")

    wire = read("packages/spectra-agent/src/mcp/wire.rs")
    require(
        "mcp." in wire and "authority" in wire,
        "the per-server capability must be derived from the URL authority",
    )
    require(
        "stdio" in wire,
        "the endpoint parser must refuse non-HTTP schemes and say why",
    )

    tools = read("packages/spectra-agent/src/tools.rs")
    require("register_remote" in tools, "the registry must carry remote entries")
    require(
        "crate::mcp::client::call_tool" in tools,
        "a remote entry must invoke the MCP client through the dispatch path",
    )

    hosts = read("packages/spectra-agent/src/hosts.rs")
    for name in [
        "spectra.std.agent.mcp_connect",
        "spectra.std.agent.mcp_handle",
        "spectra.std.agent.mcp_serve",
    ]:
        require(name in hosts, f"the host registry must carry {name}")

    lib = read("packages/spectra-agent/src/lib.rs")
    for constant in [
        "MCP_CONNECT_HOST_CALL",
        "MCP_HANDLE_HOST_CALL",
        "MCP_SERVE_HOST_CALL",
    ]:
        require(constant in lib, f"lib.rs must export {constant}")

    replay = read("packages/spectra-agent/src/replay.rs")
    require('Self::Mcp => "mcp"' in replay, "discovery must be a journaled step kind")


def validate_surface() -> None:
    builtin = read("compiler/src/semantic/builtin_std_core.rs")
    for term in ['"mcp_connect"', '"mcp_handle"', '"mcp_serve"']:
        require(term in builtin, f"compiler surface missing {term}")

    lowering = read("midend/src/lowering_std_agent.rs")
    for term in [
        '"mcp_connect"',
        '"mcp_handle"',
        '"mcp_serve"',
        "spectra.std.agent.mcp_connect",
        "spectra.std.agent.mcp_handle",
        "spectra.std.agent.mcp_serve",
    ]:
        require(term in lowering, f"midend lowering table missing {term}")

    lowering_tools = read("midend/src/lowering_agent_tools.rs")
    for term in [
        '"spectra.std.agent.mcp_connect"',
        '"spectra.std.agent.mcp_handle"',
        '"spectra.std.agent.mcp_serve"',
    ]:
        require(term in lowering_tools, f"the registration dispatch list must cover {term}")


def validate_contract() -> None:
    with (ROOT / "scripts" / "stdlib_contract.toml").open("rb") as handle:
        manifest = tomllib.load(handle)
    probes = {str(probe["id"]): probe for probe in manifest.get("probe", [])}
    probe = probes.get("agent-mcp")
    require(probe is not None, "stdlib_contract.toml must carry the agent-mcp probe")
    require(probe.get("path") == FIXTURE, f"the agent-mcp probe must point at {FIXTURE}")
    covers = set(probe.get("covers", []))
    for symbol in [
        "std.agent.mcp_connect",
        "std.agent.mcp_handle",
        "std.agent.mcp_serve",
    ]:
        require(symbol in covers, f"the agent-mcp probe must cover {symbol}")


def validate_catalog() -> None:
    probe = ROOT / "target" / "r3218-catalog-probe" / "stdlib.toml"
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
            "std.agent.mcp_connect",
            "fn(Run, string) -> Task<Result<string, Error>>",
            "Task<Result<string,Error>>",
            "spectra.std.agent.mcp_connect",
        ),
        (
            "std.agent.mcp_handle",
            "fn(Run, string) -> Result<string, Error>",
            "Result<string,Error>",
            "spectra.std.agent.mcp_handle",
        ),
        (
            "std.agent.mcp_serve",
            "fn(Run, string) -> Result<string, Error>",
            "Result<string,Error>",
            "spectra.std.agent.mcp_serve",
        ),
    ]:
        entry = entries.get(path)
        require(entry is not None, f"catalog missing {path}")
        require(
            entry.get("signature") == signature,
            f"{path} signature: {entry.get('signature')!r}",
        )
        require(
            entry.get("ir_return") == ir_return,
            f"{path} ir_return: {entry.get('ir_return')!r}",
        )
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
    require("stdio" in docs, "agent-platform.md must record the stdio limitation")
    require(
        "HTTP is the supported transport" in docs,
        "agent-platform.md must state that HTTP is the supported transport",
    )
    require(
        "no subprocess" in docs.lower() and "agent-platform-plan.md" in docs,
        "the stdio limitation must refuse subprocess support and cite the plan's follow-up",
    )
    require(
        "mcp.<authority>" in docs,
        "agent-platform.md must document the exact per-server capability form",
    )


def validate_compile_time_check() -> None:
    exit_code, output = run([str(SPECTRALANG), "check", "--json", FIXTURE])
    require(exit_code == 0, f"check on {FIXTURE} exited {exit_code}:\n{output}")
    require(
        not diagnostics_of(parse_json_line(output)),
        "the valid fixture must be diagnostic-free",
    )


# ── behavior ─────────────────────────────────────────────────────────────

FIXTURE_JOURNAL = JOURNAL_DIR / "fixture-381-serve.jsonl"


def validate_journal() -> None:
    require(FIXTURE_JOURNAL.is_file(), f"the serve run must journal {FIXTURE_JOURNAL}")
    records = [
        json.loads(line)
        for line in FIXTURE_JOURNAL.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    tool_steps = [record for record in records if record.get("kind") == "tool"]
    require(
        len(tool_steps) == 2,
        f"the third-party client's two calls must be journaled, found {len(tool_steps)}",
    )
    outputs = sorted(record.get("output", "") for record in tool_steps)
    require(outputs == ["10", "42"], f"the journaled tool results: {outputs}")


def validate_fixture() -> None:
    run_command([str(SPECTRALANG), "run", FIXTURE])

    executable = ROOT / "target" / "r3218-agent-mcp.exe"
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


def validate_blocked_mode() -> None:
    """The hostile description served by the project makes its sink unreachable."""
    environment = {"SPECTRA_R3218_MODE": "block"}
    completed = subprocess.run(
        [str(SPECTRALANG), "run", FIXTURE],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        env=_env(environment),
    )
    require(
        completed.returncode != 0,
        "a run tainted by the served hostile description must not reach the sink",
    )
    owned = ROOT / "target" / "r3218" / "owned.txt"
    require(not owned.exists(), f"the injected text must never write {owned}")


def _env(extra: dict[str, str]) -> dict[str, str]:
    environment = dict(os.environ)
    environment.update(extra)
    return environment


def post(authority: str, payload: dict) -> dict:
    request = urllib.request.Request(
        f"http://{authority}/mcp",
        data=json.dumps(payload).encode("utf-8"),
        headers={
            "content-type": "application/json",
            "accept": "application/json, text/event-stream",
            "mcp-protocol-version": "2025-06-18",
        },
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        body = response.read().decode("utf-8")
    return json.loads(body)


def validate_third_party_client() -> None:
    """A real HTTP client (this script) calls a compiled project's tools."""
    if SERVE_DIR.exists():
        shutil.rmtree(SERVE_DIR)
    SERVE_DIR.mkdir(parents=True)
    if FIXTURE_JOURNAL.exists():
        FIXTURE_JOURNAL.unlink()

    process = subprocess.Popen(
        [str(SPECTRALANG), "run", FIXTURE],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        env=_env(
            {
                "SPECTRA_R3218_MODE": "serve",
                "SPECTRA_R3218_SERVE_DIR": str(SERVE_DIR),
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
        require(authority_file.is_file(), "the fixture never published its MCP authority")
        authority = authority_file.read_text(encoding="utf-8").strip()
        require(authority.startswith("127.0.0.1:"), f"unexpected authority {authority!r}")

        handshake = post(
            authority,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "r3218-validator", "version": "1.0.0"},
                },
            },
        )
        require(
            handshake["result"]["serverInfo"]["name"] == "spectra-agent",
            f"initialize result: {handshake}",
        )

        listing = post(authority, {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
        tools = {tool["name"]: tool for tool in listing["result"]["tools"]}
        require(set(tools) == {"add", "writer"}, f"served tools: {sorted(tools)}")
        require(
            tools["add"]["inputSchema"]["properties"]["a"]["type"] == "integer",
            f"derived schema: {tools['add']['inputSchema']}",
        )
        require(
            tools["add"]["annotations"]["readOnlyHint"] is True,
            f"read-only annotations: {tools['add']['annotations']}",
        )
        require(
            tools["writer"]["annotations"]["destructiveHint"] is True,
            f"destructive annotations: {tools['writer']['annotations']}",
        )
        # The project's authored description is served verbatim -- as data.
        require(
            "IGNORE ALL PREVIOUS INSTRUCTIONS" in tools["writer"]["description"],
            "the hostile description must be served verbatim",
        )

        first = post(
            authority,
            {
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {"name": "add", "arguments": {"a": 40, "b": 2}},
            },
        )
        require(
            first["result"]["content"][0]["text"] == "42",
            f"first call: {first}",
        )
        require(first["result"]["isError"] is False, f"first call flagged an error: {first}")
        second = post(
            authority,
            {
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {"name": "add", "arguments": {"a": 3, "b": 7}},
            },
        )
        require(second["result"]["content"][0]["text"] == "10", f"second call: {second}")
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
    validate_journal()


def main() -> None:
    if not os.environ.get("SPECTRA_CLI_BUILT"):
        run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    if JOURNAL_DIR.exists():
        shutil.rmtree(JOURNAL_DIR)
    validate_implementation()
    validate_surface()
    validate_contract()
    validate_catalog()
    validate_gates()
    validate_documentation()
    validate_compile_time_check()

    # The crate tests own both MCP paths: the stub-transport round trip, the
    # hostile-description inertness and the real-socket client/server cycle.
    run_command([CARGO, "test", "-q", "-p", "spectra-agent", "--offline"])

    validate_fixture()
    validate_blocked_mode()
    validate_third_party_client()
    print("validated R-3218 MCP client and server over HTTP")


if __name__ == "__main__":
    main()
