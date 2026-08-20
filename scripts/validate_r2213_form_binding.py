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
    print(f"R-2213 validation failed: {message}", file=sys.stderr)
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
    form = read("packages/spectra-api/src/form.rs")
    for term in [
        "pub struct Form",
        "pub struct FormSchema",
        "pub struct FormBinding",
        "FormParseErrorKind::InvalidPercentEncoding",
        "FormParseErrorKind::InvalidUtf8",
        "FormParseErrorKind::MalformedKey",
        "FormBindError::DuplicateField",
        "FORM_TYPE_STRING",
        "FORM_TYPE_INT",
        "FORM_TYPE_BOOL",
        "parse_form",
        "form_decode",
        "normalize_form_key",
        "schema_field",
        "binding_int",
        "binding_bool",
        "parses_urlencoded_plus_arrays_and_nested_fields",
        "binds_schema_arrays_nested_fields_and_duplicate_scalar_errors",
        "binding_reports_missing_required_and_type_mismatch_fields",
    ]:
        require(term in form, f"form.rs missing {term}")
    require("b'+'" in form and "decoded.push(b' ')" in form, "form parser must translate plus to space")

    lib = read("packages/spectra-api/src/lib.rs")
    runtime = read("runtime/src/api/mod.rs")
    midend = read("midend/src/lowering.rs")
    builtins = read("compiler/src/semantic/builtin_modules.rs")
    for name in [
        "spectra.api.form.parse",
        "spectra.api.form.len",
        "spectra.api.form.has",
        "spectra.api.form.count",
        "spectra.api.form.first",
        "spectra.api.form.value",
        "spectra.api.form.int",
        "spectra.api.form.bool",
        "spectra.api.form.schema",
        "spectra.api.form.schema_field",
        "spectra.api.form.bind",
        "spectra.api.form.binding_ok",
        "spectra.api.form.binding_error",
        "spectra.api.form.binding_count",
        "spectra.api.form.binding_value",
        "spectra.api.form.binding_int",
        "spectra.api.form.binding_bool",
        "spectra.api.form.error_code",
        "spectra.api.form.error_message",
    ]:
        require(name in lib, f"{name} missing from host-call table")
        require(name in runtime, f"{name} missing from runtime contract")
    for term in [
        '"form", "parse"',
        '"form", "schema_field"',
        '"form", "binding_value"',
        '"form", "error_message"',
        "FormSchema",
        "FormBinding",
    ]:
        require(term in midend, f"midend missing {term}")
    for term in [
        "std.api.form",
        "std.api.form.Form",
        "std.api.form.FormSchema",
        "std.api.form.FormBinding",
        "std.api.form.parse",
        "std.api.form.schema_field",
        "std.api.form.binding_int",
        "std.api.form.binding_bool",
    ]:
        require(term in builtins, f"builtin surface missing {term}")


def validate_fixture_and_docs() -> None:
    fixture = read("tests/validation/137_api_form_binding.spectra")
    for term in [
        "record SignupForm",
        "parse(\"name=Ada+Lovelace",
        "profile[city]",
        "count(form, \"tags\")",
        "schema_field",
        "binding_int",
        "binding_bool",
        "binding_value",
        "duplicate_binding",
        "missing_binding",
        "parse(\"profile[name=Ada\")",
        "error_message",
    ]:
        require(term in fixture, f"fixture missing {term}")

    docs = read("docs/api/std-api-form.md")
    for term in [
        "std.api.form",
        "application/x-www-form-urlencoded",
        "`+` decodes to a space",
        "profile[city]",
        "profile.city",
        "Duplicate keys are accepted for repeated fields only",
        "tests/validation/137_api_form_binding.spectra",
    ]:
        require(term in docs, f"form docs missing {term}")


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2213 = items.get("R-2213")
    require(r2213 is not None, "R-2213 missing from roadmap")
    require(r2213.get("status") == "complete", "R-2213 must be marked complete")
    require(r2213.get("owner") == "web", "R-2213 owner must remain web")
    require(r2213.get("dependencies") == ["R-2212"], "R-2213 dependencies changed")
    acceptance = "\n".join(r2213.get("acceptance", []))
    for term in [
        "typed struct",
        "key-value map",
        "duplicate keys",
        "offending field",
        "missing required fields",
        "137_api_form_binding.spectra",
        "scripts/validate_r2213_form_binding.py",
    ]:
        require(term in acceptance, f"R-2213 acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2213 URL-Encoded Form Binding", 1)[1].split(
        "## R-2214", 1
    )[0]
    for term in [
        "Status: `complete`",
        "packages/spectra-api/src/form.rs",
        "std.api.form",
        "FormSchema",
        "137_api_form_binding.spectra",
        "validate_r2213_form_binding.py",
    ]:
        require(term in block, f"backlog R-2213 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2213` URL-encoded form binding (complete;" in plan,
        "implementation plan must mark R-2213 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2213_form_binding.py" in runner, "run_tests.ps1 must run R-2213")
    require(
        'Teste = "validate_r2213_form_binding"' in runner,
        "run_tests.ps1 must record R-2213",
    )


def validate_commands() -> None:
    binary = ROOT / "target" / "debug" / ("spectralang.exe" if sys.platform.startswith("win") else "spectralang")
    run_command(["cargo", "test", "-q", "-p", "spectra-api", "form", "--offline"])
    run_command(["cargo", "test", "-q", "-p", "spectra-compiler", "--offline"])
    run_command(["cargo", "test", "-q", "-p", "spectra-midend", "--offline"])
    run_command(["cargo", "build", "-q", "-p", "spectra-cli", "--offline"])
    run_command([str(binary), "compile", "tests/validation/137_api_form_binding.spectra"])
    run_command([str(binary), "run", "tests/validation/137_api_form_binding.spectra"])


def main() -> None:
    validate_implementation()
    validate_fixture_and_docs()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2213 URL-encoded form binding")


if __name__ == "__main__":
    main()
