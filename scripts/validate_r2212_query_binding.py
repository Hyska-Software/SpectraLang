from __future__ import annotations

import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    source = ROOT / path
    if source.suffix == ".rs":
        return "\n".join(
            sibling.read_text(encoding="utf-8")
            for sibling in sorted(source.parent.rglob("*.rs"))
        )
    return source.read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-2212 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def parse_toml(path: str):
    with (ROOT / path).open("rb") as fh:
        return tomllib.load(fh)


def run_command(args: list[str]) -> None:
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


def validate_implementation() -> None:
    # Query parsing is still implemented in this focused module.  Do not
    # aggregate unrelated API modules here: HTTP form decoding legitimately
    # handles `+`, while RFC 3986 query decoding must preserve it.
    query = (ROOT / "packages/spectra-api/src/query.rs").read_text(encoding="utf-8")
    for term in [
        "pub struct Query",
        "pub struct QuerySchema",
        "pub struct QueryBinding",
        "QueryParseErrorKind::InvalidPercentEncoding",
        "QueryParseErrorKind::InvalidUtf8",
        "QueryParseErrorKind::ControlCharacter",
        "QUERY_TYPE_STRING",
        "QUERY_TYPE_INT",
        "QUERY_TYPE_BOOL",
        "parse_query",
        "percent_decode",
        "schema_field",
        "binding_int",
        "binding_bool",
        "parses_simple_repeated_and_reserved_query_values",
        "binds_typed_schema_and_reports_type_errors",
    ]:
        require(term in query, f"query.rs missing {term}")
    require("b'+'" not in query, "query parser must not translate plus to space")

    lib = read("packages/spectra-api/src/lib.rs")
    runtime = read("runtime/src/api/mod.rs")
    midend = read("midend/src/lowering.rs")
    builtins = read("compiler/src/semantic/builtin_modules.rs")
    for name in [
        "spectra.api.query.parse",
        "spectra.api.query.len",
        "spectra.api.query.has",
        "spectra.api.query.count",
        "spectra.api.query.first",
        "spectra.api.query.value",
        "spectra.api.query.int",
        "spectra.api.query.bool",
        "spectra.api.query.schema",
        "spectra.api.query.schema_field",
        "spectra.api.query.bind",
        "spectra.api.query.binding_ok",
        "spectra.api.query.binding_error",
        "spectra.api.query.binding_count",
        "spectra.api.query.binding_value",
        "spectra.api.query.binding_int",
        "spectra.api.query.binding_bool",
        "spectra.api.query.error_code",
        "spectra.api.query.error_message",
    ]:
        require(name in lib, f"{name} missing from host-call table")
        require(name in runtime, f"{name} missing from runtime contract")
    for term in [
        '"query", "parse"',
        '"query", "schema_field"',
        '"query", "binding_value"',
        '"query", "error_message"',
        "QuerySchema",
        "QueryBinding",
    ]:
        require(term in midend, f"midend missing {term}")
    for term in [
        "std.api.query",
        "std.api.query.Query",
        "std.api.query.QuerySchema",
        "std.api.query.QueryBinding",
        "std.api.query.parse",
        "std.api.query.schema_field",
        "std.api.query.binding_int",
        "std.api.query.binding_bool",
    ]:
        require(term in builtins, f"builtin surface missing {term}")


def validate_fixture_and_docs() -> None:
    fixture = read("tests/validation/136_api_query_binding.spectra")
    for term in [
        "record SearchQuery",
        "parse(\"/search?page=2",
        "count(query, \"tag\")",
        "schema_field",
        "binding_int",
        "binding_bool",
        "binding_value",
        "parse(\"bad=%GG\")",
        "binding_error",
    ]:
        require(term in fixture, f"fixture missing {term}")

    docs = read("docs/api/std-api-query.md")
    for term in [
        "std.api.query",
        "RFC 3986",
        "Repeated keys are arrays",
        "Typed Binding",
        "type_int()",
        "tests/validation/136_api_query_binding.spectra",
    ]:
        require(term in docs, f"query docs missing {term}")


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2212 = items.get("R-2212")
    require(r2212 is not None, "R-2212 missing from roadmap")
    require(r2212.get("status") == "complete", "R-2212 must be marked complete")
    require(r2212.get("owner") == "web", "R-2212 owner must remain web")
    require(r2212.get("dependencies") == ["R-2210"], "R-2212 dependencies changed")
    acceptance = "\n".join(r2212.get("acceptance", []))
    for term in [
        "structured map",
        "typed struct",
        "repeated keys",
        "typed errors",
        "RFC 3986",
        "136_api_query_binding.spectra",
        "scripts/validate_r2212_query_binding.py",
    ]:
        require(term in acceptance, f"R-2212 acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2212 Query String Parser and Binding", 1)[1].split(
        "## R-2213", 1
    )[0]
    for term in [
        "Status: `complete`",
        "packages/spectra-api/src/query.rs",
        "std.api.query",
        "QuerySchema",
        "136_api_query_binding.spectra",
        "validate_r2212_query_binding.py",
    ]:
        require(term in block, f"backlog R-2212 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2212` Query string parser and binding (complete;" in plan,
        "implementation plan must mark R-2212 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2212_query_binding.py" in runner, "run_tests.ps1 must run R-2212")
    require(
        'Teste = "validate_r2212_query_binding"' in runner,
        "run_tests.ps1 must record R-2212",
    )


def validate_commands() -> None:
    binary = ROOT / "target" / "debug" / ("spectralang.exe" if sys.platform.startswith("win") else "spectralang")
    run_command(["cargo", "test", "-q", "-p", "spectra-api", "query", "--offline"])
    run_command(["cargo", "test", "-q", "-p", "spectra-compiler", "--offline"])
    run_command(["cargo", "test", "-q", "-p", "spectra-midend", "--offline"])
    run_command(["cargo", "build", "-q", "-p", "spectra-cli", "--offline"])
    run_command([str(binary), "compile", "tests/validation/136_api_query_binding.spectra"])
    run_command([str(binary), "run", "tests/validation/136_api_query_binding.spectra"])


def main() -> None:
    validate_implementation()
    validate_fixture_and_docs()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2212 query string parser and binding")


if __name__ == "__main__":
    main()
