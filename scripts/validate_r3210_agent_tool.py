# R-3210 — `#[agent_tool]` attribute and derived JSON Schema.
#
# Validates the declaration path end to end: parser support for the string
# attribute argument, semantic validation with stable codes and hints, the
# `json_schema()` associated function on the existing derive, IR-derived tool
# effects/capabilities in `surface --json`, cross-module name uniqueness,
# formatter idempotency and the LSP's freedom from false attribute diagnostics.
from __future__ import annotations

import os
import json
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPECTRALANG = Path(
    os.environ.get("SPECTRALANG_BINARY") or (ROOT / "target" / "debug" / "spectralang.exe")
)
CARGO = shutil.which("cargo") or "cargo"
FIXTURE = "tests/validation/371_agent_tool_surface.spectra"
DUPLICATE_PROJECT = "tests/projects/invalid/agent_tool_duplicate"

# One fixture per rejection condition: `E3203` for declaration misuse (one hint
# per condition) and `E3204` for a payload the JSON derive cannot decode.
REJECTION_FIXTURES = {
    "tests/errors/agent_tool_non_public.spectra": "E3203",
    "tests/errors/agent_tool_non_async.spectra": "E3203",
    "tests/errors/agent_tool_generic.spectra": "E3203",
    "tests/errors/agent_tool_dyn_parameter.spectra": "E3203",
    "tests/errors/agent_tool_missing_run.spectra": "E3203",
    "tests/errors/agent_tool_run_not_first.spectra": "E3203",
    "tests/errors/agent_tool_non_literal_description.spectra": "E3203",
    "tests/errors/agent_tool_wrong_arity.spectra": "E3203",
    "tests/errors/agent_tool_undecodable_parameter.spectra": "E3204",
    "tests/errors/agent_tool_exact_width_parameter.spectra": "E3204",
    "tests/errors/agent_tool_non_unit_enum_parameter.spectra": "E3204",
}


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3210 validation failed: {message}", file=sys.stderr)
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
        encoding="utf-8",
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


def validate_implementation() -> None:
    parser = read("compiler/src/parser/item_dispatch.rs")
    require(
        "AttributeArgument::StringLiteral" in parser,
        "the parser must keep a positional string literal distinct from an identifier",
    )
    ast = read("compiler/src/ast/mod.rs")
    require("StringLiteral(String)" in ast, "AttributeArgument::StringLiteral must exist")

    agent = read("compiler/src/semantic/semantic_agent.rs")
    for term in [
        "validate_function_attributes",
        '"E3203"',
        '"E3204"',
        "agent_tools.insert",
        "render_input_schema",
    ]:
        require(term in agent, f"semantic_agent.rs missing {term}")

    module = read("compiler/src/semantic/mod.rs")
    require("agent_tools" in module, "the analyzer must mirror json_struct_derives for tools")

    analysis = read("compiler/src/semantic/semantic_module_analysis.rs")
    require(
        "validate_function_attributes(func)" in analysis,
        "Item::Function must call validate_function_attributes",
    )

    semantic_json = read("compiler/src/semantic/semantic_json.rs")
    require(
        '"json_schema"' in semantic_json,
        "register_json_derived_methods must register json_schema",
    )

    derive = read("midend/src/lowering_json_derive.rs")
    for term in ["lower_derive_json_schema", "derive_json_schema_string", "json_schema_type"]:
        require(term in derive, f"lowering_json_derive.rs missing {term}")
    enum_lowering = read("midend/src/lowering_expr_enum.rs")
    require('"json_schema"' in enum_lowering, "the associated call must lower directly")

    surface = read("compiler/src/semantic/surface.rs")
    require("SurfaceTool" in surface, "the surface snapshot must carry tools")

    cli = read("tools/spectra-cli/src/cli_surface.rs")
    for term in [
        "SurfaceToolJson",
        "derive_tool_effects",
        "host_namespace_prefix",
        "duplicate_tool_name",
        "InstructionKind::HostCall",
    ]:
        require(term in cli, f"cli_surface.rs missing {term}")

    docs = read("docs/diagnostics/error-code-reference.md")
    for code in ["`E3203`", "`E3204`"]:
        require(code in docs, f"error-code-reference.md must document {code}")


def validate_execution() -> None:
    # JIT: the derived schema and the attributed tool both must work.
    run_command([str(SPECTRALANG), "run", FIXTURE])


def validate_check_clean() -> None:
    # The LSP forwards compiler and lint diagnostics and owns no attribute
    # validation of its own, so a clean `check --json` on a valid attributed
    # function is the proof that no false "unknown attribute" is produced.
    exit_code, output = run([str(SPECTRALANG), "check", "--json", FIXTURE])
    require(exit_code == 0, f"check on the valid fixture exited {exit_code}:\n{output}")
    report = parse_json_line(output)
    diagnostics = [d for file in report.get("files", []) for d in file.get("diagnostics", [])]
    require(not diagnostics, f"valid attributed function must be diagnostic-free: {diagnostics}")

    lsp = read("tools/spectra-lsp/src/lsp_analysis.rs")
    require(
        "compiler_error_to_diagnostic" in lsp and "lint_to_diagnostic" in lsp,
        "LSP diagnostics must stay derived from compiler/lint output",
    )


def validate_rejections() -> None:
    for fixture, code in REJECTION_FIXTURES.items():
        exit_code, output = run([str(SPECTRALANG), "check", "--json", fixture])
        require(exit_code != 0, f"{fixture} must fail compilation")
        report = parse_json_line(output)
        diagnostics = [d for file in report.get("files", []) for d in file.get("diagnostics", [])]
        matching = [d for d in diagnostics if d.get("code") == code]
        require(
            len(matching) == 1,
            f"{fixture} must report exactly one {code}, got {[d.get('code') for d in diagnostics]}",
        )
        require(
            matching[0].get("hint"),
            f"{fixture} must carry a hint for {code}",
        )


def validate_surface() -> None:
    exit_code, output = run([str(SPECTRALANG), "surface", "--json", FIXTURE])
    require(exit_code == 0, f"surface on the fixture exited {exit_code}:\n{output}")
    report = parse_json_line(output)

    tools = report.get("tools")
    require(isinstance(tools, list) and len(tools) == 1, f"expected one tool: {tools}")
    tool = tools[0]
    require(tool["name"] == "cobrar", f"tool name must be derived from the function: {tool}")
    require(
        tool["description"] == "Cria uma cobrança no provedor de pagamentos",
        "the description is the only authored string",
    )
    require(tool["module"] == "agent_tool_surface", f"tool module mismatch: {tool}")
    require(tool["payload_param"] == "cobranca", f"payload parameter mismatch: {tool}")
    schema = tool["input_schema"]
    require('"type":"object"' in schema, f"input schema must be an object: {schema}")
    require('"id":{"type":"string"}' in schema, f"schema must carry field types: {schema}")
    require(
        '"required":["id","amount"]' in schema,
        f"optional fields must be absent from required: {schema}",
    )
    require(
        "spectra.std.agent.token_count" in tool["effects"],
        f"effects must be derived from the IR: {tool['effects']}",
    )
    require(
        tool["capabilities"] == ["spectra.std.agent"],
        f"capabilities must be namespace-prefix grants: {tool['capabilities']}",
    )

    _, second = run([str(SPECTRALANG), "surface", "--json", FIXTURE])
    require(output == second, "surface output must be byte-identical across runs")

    # Cross-module duplicates are a project-level error (exit 65), not a
    # per-module semantic one.
    exit_code, duplicate_output = run([str(SPECTRALANG), "surface", "--json", DUPLICATE_PROJECT])
    require(exit_code == 65, f"duplicate tool names must exit 65, got {exit_code}")
    duplicate = parse_json_line(duplicate_output)
    require(duplicate.get("success") is False, "duplicate failure must set success=false")
    require("Duplicate tool name" in duplicate.get("error", ""), duplicate_output)


def validate_formatter() -> None:
    workdir = ROOT / "target" / "r3210-fmt"
    workdir.mkdir(parents=True, exist_ok=True)
    target = workdir / "agent_tool_surface.spectra"
    target.write_text(read(FIXTURE), encoding="utf-8")

    run_command([str(SPECTRALANG), "fmt", str(target)])
    first = target.read_text(encoding="utf-8")
    run_command([str(SPECTRALANG), "fmt", "--check", str(target)])
    run_command([str(SPECTRALANG), "fmt", str(target)])
    second = target.read_text(encoding="utf-8")

    require(first == second, "fmt must be idempotent on attributed functions")
    require('#[agent_tool("' in second, "fmt must preserve the attribute")


def validate_planning() -> None:
    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-3210", 1)[1].split("## R-3211", 1)[0]
    for term in ["Status: `complete`", "validate_r3210_agent_tool.py"]:
        require(term in block, f"backlog R-3210 missing {term}")

    runner = read("run_tests.ps1")
    require("validate_r3210_agent_tool.py" in runner, "run_tests.ps1 must run R-3210")


def main() -> None:
    if not os.environ.get("SPECTRA_CLI_BUILT"):
        run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    validate_implementation()
    validate_execution()
    validate_check_clean()
    validate_rejections()
    validate_surface()
    validate_formatter()
    validate_planning()
    print("validated R-3210 agent_tool attribute and derived JSON schema")


if __name__ == "__main__":
    main()
