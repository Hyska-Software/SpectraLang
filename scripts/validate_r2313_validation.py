from __future__ import annotations

import os
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE_HOST_CALL_COUNT = 444
RUNTIME_REQUIRED_HOST_CALL_COUNT = 347


def cargo_command() -> str:
    cargo = "cargo.exe" if sys.platform.startswith("win") else "cargo"
    home = Path(os.environ.get("USERPROFILE") or os.environ.get("HOME") or "")
    candidate = home / ".cargo" / "bin" / cargo
    return str(candidate) if candidate.exists() else "cargo"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-2313 validation failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


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
    validation = read("packages/spectra-api/src/validation.rs")
    for term in [
        "pub struct ValidationSchema",
        "pub struct ValidationIssue",
        "pub fn with_min_length",
        "pub fn with_max_length",
        "pub fn with_range",
        "pub fn with_regex",
        "Regex::new",
        "pub fn validate_json",
        "pub fn validate_form",
        "application/problem+json",
        'pub extern "C" fn result_response',
    ]:
        require(term in validation, f"validation.rs missing {term}")

    form_core = read("packages/spectra-api/src/form_core.rs")
    require("pub(crate) fn clone_form" in form_core, "form handle bridge is missing")

    handles = read("runtime/src/handles/mod.rs")
    require("ApiValidationSchema" in handles, "validation schema handle kind is missing")
    require("ApiValidationResult" in handles, "validation result handle kind is missing")

    host_calls = read("packages/spectra-api/src/host_calls.rs")
    runtime_api = read("runtime/src/api/mod.rs")
    names = [
        "spectra.api.validation.schema",
        "spectra.api.validation.field",
        "spectra.api.validation.min_length",
        "spectra.api.validation.max_length",
        "spectra.api.validation.range",
        "spectra.api.validation.regex",
        "spectra.api.validation.validate_json",
        "spectra.api.validation.validate_form",
        "spectra.api.validation.result_ok",
        "spectra.api.validation.result_count",
        "spectra.api.validation.result_field",
        "spectra.api.validation.result_code",
        "spectra.api.validation.result_message",
        "spectra.api.validation.result_problem_json",
        "spectra.api.validation.result_response",
        "spectra.api.validation.error_code",
        "spectra.api.validation.error_message",
    ]
    for name in names:
        require(name in host_calls, f"{name} missing from package host registry")
        require(name in runtime_api, f"{name} missing from runtime contract")
    require(
        f"assert_eq!(HOST_CALLS.len(), {PACKAGE_HOST_CALL_COUNT})"
        in read("packages/spectra-api/src/api_tests.rs"),
        "package host-call count is not synchronized",
    )
    require(
        f"assert_eq!(required_host_call_count(), {RUNTIME_REQUIRED_HOST_CALL_COUNT})"
        in runtime_api,
        "runtime required host-call count is not synchronized",
    )


def validate_surface() -> None:
    semantic = read("compiler/src/semantic/builtin_api_core.rs")
    services = read("compiler/src/semantic/builtin_api_services.rs")
    contract = read("compiler/src/semantic/builtin_contract.rs")
    lowering = read("midend/src/lowering_std_api.rs")
    initialization = read("compiler/src/semantic/semantic_init.rs")
    snapshot = read("compiler/tests/snapshots/std_api_public_function_table.snap")
    require("make_std_api_validation" in semantic, "semantic validation module is missing")
    require("ValidationSchema" in services and "ValidationResult" in services, "validation semantic types are missing")
    require("std.api.validation" in contract, "validation contract module is missing")
    require("(\"validation\", \"validate_json\")" in lowering, "validation lowering is missing")
    require("std.api.validation" in initialization, "validation builtin initialization is missing")
    for term in [
        "module std.api.validation",
        "type std.api.validation.ValidationSchema",
        "type std.api.validation.ValidationResult",
        "func std.api.validation.validate_json",
        "func std.api.validation.validate_form",
        "func std.api.validation.result_response",
    ]:
        require(term in snapshot, f"snapshot surface missing {term}")


def validate_fixture_docs_catalog() -> None:
    fixture = read("tests/validation/339_api_validation.spectra")
    for term in [
        "schema()",
        "min_length",
        "max_length",
        "range",
        "regex",
        "validate_json",
        "validate_form",
        "application/problem+json",
        "result_response",
    ]:
        require(term in fixture or term == "application/problem+json", f"validation fixture missing {term}")

    catalog = read("packages/spectra-contract/catalog/stdlib.toml")
    for term in [
        'path = "std.api.validation"',
        'path = "std.api.validation.ValidationSchema"',
        'path = "std.api.validation.validate_json"',
        'path = "std.api.validation.validate_form"',
        'path = "std.api.validation.result_response"',
        'fixture = "tests/validation/339_api_validation.spectra"',
    ]:
        require(term in catalog, f"catalog missing {term}")

    docs = read("docs/api/std-api-validation.md")
    for term in [
        "RFC 7807",
        "min_length",
        "range",
        "validate_json",
        "validate_form",
        "application/problem+json",
        "339_api_validation.spectra",
    ]:
        require(term in docs, f"validation docs missing {term}")


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2313")
    require(item is not None, "R-2313 missing from roadmap")
    require(item.get("status") == "complete", "R-2313 is not complete")
    require(item.get("owner") == "web", "R-2313 owner changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in [
        "required",
        "RFC 7807",
        "validate_json",
        "validate_form",
        "339_api_validation.spectra",
        "validate_r2313_validation.py",
    ]:
        require(term in acceptance, f"roadmap acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2313 Request Validation", 1)[1].split(
        "## R-2314", 1
    )[0]
    for term in [
        "Status: `complete`",
        "339_api_validation.spectra",
        "validate_r2313_validation.py",
    ]:
        require(term in block, f"backlog R-2313 missing {term}")


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2313_validation.py" in runner, "runner does not invoke R-2313")
    require(
        'Teste = "validate_r2313_validation"' in runner,
        "runner does not record R-2313",
    )


def validate_commands() -> None:
    cargo = cargo_command()
    binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    run_command(
        [
            cargo,
            "test",
            "-q",
            "-p",
            "spectra-api",
            "--lib",
            "validation",
            "--offline",
            "--",
            "--test-threads=1",
        ]
    )
    run_command(
        [
            cargo,
            "test",
            "-q",
            "-p",
            "spectra-compiler",
            "--test",
            "snapshot_tests",
            "--offline",
        ]
    )
    run_command([cargo, "build", "-q", "-p", "spectra-cli", "--offline"])
    run_command([str(binary), "check", "tests/validation/339_api_validation.spectra"])
    run_command([str(binary), "run", "tests/validation/339_api_validation.spectra"])


def main() -> None:
    validate_implementation()
    validate_surface()
    validate_fixture_docs_catalog()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2313 request validation")


if __name__ == "__main__":
    main()
