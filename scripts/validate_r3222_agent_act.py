# R-3222 — the tool loop `act` and governed tool dispatch.
#
# Validates the whole chain: the synthesized marshalling wrappers and the
# per-module registration (midend), the tool registry and invocation-by-address
# (spectra-agent), the `act` loop and its ceilings, the compiler/midend/catalog
# surface of `act`/`tool_call`, the contract probe and the language-level
# fixtures in JIT and AOT — including the cross-module project, where the
# wrappers live in a different module than the dispatcher.
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
FIXTURE = "tests/validation/375_agent_act.spectra"
PROJECT = "tests/projects/valid/agent_act"
CATALOG = "packages/spectra-contract/catalog/stdlib.toml"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3222 validation failed: {message}", file=sys.stderr)
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
    midend = read("midend/src/lowering_agent_tools.rs")
    for term in [
        "AGENT_TOOL_WRAPPER_PREFIX",
        "AGENT_REGISTER_TOOLS_PREFIX",
        "AGENT_REGISTER_TOOL_HOST_CALL",
        "spectra.std.agent.register_tool",
        "spectra.std.agent.act",
        "spectra.std.agent.tool_call",
        "fn synthesize_agent_tools",
        "fn synthesize_tool_wrapper",
        "fn synthesize_agent_registration",
        "fn insert_registration_calls",
        "lower_derive_from_json",
        "lower_derive_error_field",
        "lower_derive_encode_struct",
        "spectra.async.task.block_on",
    ]:
        require(term in midend, f"lowering_agent_tools.rs missing {term}")

    tools = read("packages/spectra-agent/src/tools.rs")
    for term in [
        "pub(crate) fn register",
        "pub(crate) fn invoke",
        "pub(crate) fn definitions",
        "pub(crate) fn enforce_run_grant",
        "budget::charge_tool_call",
        "std::mem::transmute(tool.address as usize)",
        "AgentError::UnknownTool",
        "AgentError::ToolFailed",
        "spectra.std.agent.register_tool",
    ]:
        require(term in tools, f"tools.rs missing {term}")

    act = read("packages/spectra-agent/src/act.rs")
    for term in [
        "pub(crate) fn act",
        "pub(crate) fn tool_call",
        "MAX_ACT_STEPS",
        "tools::enforce_run_grant",
        "tools::invoke",
        "AgentError::ToolLoopCeiling",
        "budget::guard",
    ]:
        require(term in act, f"act.rs missing {term}")

    hosts = read("packages/spectra-agent/src/hosts.rs")
    for term in [
        "fn act_host",
        "fn tool_call_host",
        "fn register_tool_host",
        "spectra.std.agent.act",
        "spectra.std.agent.tool_call",
        "spectra.std.agent.register_tool",
        "fn build_request",
        "provider::ToolDefinition",
    ]:
        require(term in hosts, f"hosts.rs missing {term}")

    error = read("packages/spectra-agent/src/error.rs")
    for term in [
        "UnknownTool(",
        "ToolFailed(",
        "CapabilityDenied(",
        "ToolLoopCeiling(",
        '"unknown_tool"',
        '"tool_failed"',
        '"capability_denied"',
    ]:
        require(term in error, f"error.rs missing {term}")

    lib = read("packages/spectra-agent/src/lib.rs")
    for term in ["mod tools", "mod act", "ACT_HOST_CALL", "TOOL_CALL_HOST_CALL", "REGISTER_TOOL_HOST_CALL"]:
        require(term in lib, f"spectra-agent lib.rs missing {term}")

    provider = read("packages/spectra-agent/src/provider/mod.rs")
    for term in ["pub(crate) struct ToolDefinition", "pub(crate) struct ToolCall", "tool_calls", "tools: Vec<ToolDefinition>"]:
        require(term in provider, f"provider/mod.rs missing {term}")

    mock = read("packages/spectra-agent/src/provider/mock.rs")
    for term in ["spectra:tool=", "spectra:final="]:
        require(term in mock, f"mock provider missing {term}")

    builtin = read("compiler/src/semantic/builtin_std_core.rs")
    for term in ['"act"', '"tool_call"', '"register_tool"']:
        require(term in builtin, f"compiler surface missing {term}")

    lowering = read("midend/src/lowering_std_agent.rs")
    for term in [
        '"act"',
        '"tool_call"',
        '"register_tool"',
        "spectra.std.agent.act",
        "spectra.std.agent.tool_call",
        "spectra.std.agent.register_tool",
    ]:
        require(term in lowering, f"midend lowering table missing {term}")

    ast = read("compiler/src/ast/mod.rs")
    require("imported_agent_tools" in ast, "AST module must carry imported_agent_tools")
    require("pub struct ImportedAgentTool" in ast, "AST module must declare ImportedAgentTool")


def validate_contract() -> None:
    with (ROOT / "scripts/stdlib_contract.toml").open("rb") as handle:
        manifest = tomllib.load(handle)
    probes = {str(probe["id"]): probe for probe in manifest.get("probe", [])}
    probe = probes.get("agent-act")
    require(probe is not None, "stdlib_contract.toml must carry the agent-act probe")
    require(probe.get("path") == FIXTURE, f"agent-act probe must point at {FIXTURE}")
    covers = set(probe.get("covers", []))
    for symbol in ["std.agent.act", "std.agent.tool_call"]:
        require(symbol in covers, f"agent-act probe must cover {symbol}")


def validate_catalog() -> None:
    probe = ROOT / "target" / "r3222-catalog-probe" / "stdlib.toml"
    probe.parent.mkdir(parents=True, exist_ok=True)
    if probe.exists():
        probe.unlink()
    run_command(
        [
            sys.executable,
            "scripts/generate_stdlib_catalog.py",
            "--output",
            str(probe.relative_to(ROOT)),
        ]
    )
    require(
        probe.read_bytes() == (ROOT / CATALOG).read_bytes(),
        "the checked-in catalog must be the generator's output",
    )
    with (ROOT / CATALOG).open("rb") as handle:
        catalog = tomllib.load(handle)
    entries = {entry["path"]: entry for entry in catalog["entry"]}
    act = entries.get("std.agent.act")
    require(act is not None, "catalog missing std.agent.act")
    require(
        act.get("signature") == "fn(Run, string) -> Task<Result<string, Error>>",
        f"act signature: {act.get('signature')!r}",
    )
    require(act.get("ir_return") == "Task<Result<string,Error>>", "act must lower to a Task")
    require(act.get("binding") == "spectra.std.agent.act", "act binding")
    require(act.get("fixture") == FIXTURE, f"act must cite {FIXTURE}")

    call = entries.get("std.agent.tool_call")
    require(call is not None, "catalog missing std.agent.tool_call")
    require(
        call.get("signature") == "fn(Run, string, string) -> Task<Result<string, Error>>",
        f"tool_call signature: {call.get('signature')!r}",
    )

    register = entries.get("std.agent.register_tool")
    require(register is not None, "catalog missing std.agent.register_tool")
    require(
        register.get("signature")
        == "fn(string, int, string, string, string) -> Result<bool, Error>",
        f"register_tool signature: {register.get('signature')!r}",
    )


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

    # JIT: the single-module chain (two tools, one of them async with a real
    # provider round trip) and the cross-module project.
    run_command([str(SPECTRALANG), "run", FIXTURE])
    run_command([str(SPECTRALANG), "run", PROJECT])

    # AOT parity: --debug-info=none avoids the pre-existing MSVC PDB limit.
    for fixture, executable in [
        (FIXTURE, ROOT / "target" / "r3222-agent-act.exe"),
        (PROJECT, ROOT / "target" / "r3222-agent-act-project.exe"),
    ]:
        run_command(
            [
                str(SPECTRALANG),
                "compile",
                "--debug-info=none",
                "--emit-exe",
                str(executable),
                fixture,
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
            f"AOT binary for {fixture} must return 0, got {completed.returncode}:\n{completed.stdout}",
        )

    # The ABI proof itself: the wrapper of a tool declared in another module is
    # registered and invoked by address in both execution modes (covered by the
    # project fixture above), and the dispatched result is the derived JSON.
    ir = run_command([str(SPECTRALANG), "run", "--dump-ir", FIXTURE])
    require("__spectra_agent_tool_add" in ir, "the tool wrapper must be synthesized")
    require("func_addr __spectra_agent_tool_add" in ir, "the wrapper address must be taken")
    require(
        "hostcall spectra.std.agent.register_tool" in ir,
        "registration must hand the wrapper address to the runtime",
    )


def main() -> None:
    if not os.environ.get("SPECTRA_CLI_BUILT"):
        run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    validate_implementation()
    validate_contract()
    validate_catalog()
    validate_gates()
    validate_behavior()
    print("validated R-3222 tool loop and governed tool dispatch")


if __name__ == "__main__":
    main()
