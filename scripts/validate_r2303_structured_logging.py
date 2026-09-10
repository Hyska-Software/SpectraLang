from __future__ import annotations

import os
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE_HOST_CALL_COUNT = 555


def cargo_command() -> str:
    cargo = "cargo.exe" if sys.platform.startswith("win") else "cargo"
    home = Path(os.environ.get("USERPROFILE") or os.environ.get("HOME") or "")
    candidate = home / ".cargo" / "bin" / cargo
    return str(candidate) if candidate.exists() else "cargo"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-2303 validation failed: {message}", file=sys.stderr)
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
    middleware = read("packages/spectra-api/src/middleware.rs")
    for term in [
        "pub enum RequestLogFormat",
        "pub struct RequestLogRecord",
        "pub struct StructuredLoggingMiddleware",
        "REQUEST_ID_FALLBACK_SEQ",
        "context.begin_request(&request)",
        "register_logging",
        "logging_len",
        "logging_line",
        "logging_request_id",
        '"request_id"',
        '"latency_us"',
        "structured_logging_emits_json_with_request_identity_and_fields",
        "structured_logging_text_records_short_circuit_response",
    ]:
        require(term in middleware, f"middleware.rs missing {term}")

    host_calls = read("packages/spectra-api/src/host_calls.rs")
    runtime_api = read("packages/spectra-api/src/host_calls.rs")
    api_tests = read("packages/spectra-api/src/api_tests.rs")
    for name in [
        "spectra.api.middleware.register_logging",
        "spectra.api.middleware.logging_len",
        "spectra.api.middleware.logging_line",
        "spectra.api.middleware.logging_request_id",
    ]:
        require(name in host_calls, f"{name} missing from package host registry")
        require(name in runtime_api, f"{name} missing from runtime contract")
    require(
        f"assert_eq!(HOST_CALLS.len(), {PACKAGE_HOST_CALL_COUNT})"
        in read("packages/spectra-api/src/api_tests.rs"),
        "package host-call count is not synchronized",
    )
    require(
        f"assert_eq!(HOST_CALLS.len(), {PACKAGE_HOST_CALL_COUNT})"
        in api_tests,
        "package host-call count is not synchronized",
    )


def validate_frontend_and_lowering() -> None:
    builtins = read("compiler/src/semantic/builtin_api_services.rs")
    contract = read("compiler/src/semantic/builtin_contract.rs")
    lowering = read("midend/src/lowering_std_api.rs")
    snapshot = read("compiler/tests/snapshots/std_api_public_function_table.snap")
    for term in [
        "register_logging",
        "logging_len",
        "logging_line",
        "logging_request_id",
    ]:
        require(term in builtins, f"semantic builtin missing {term}")
        require(term in contract, f"semantic contract missing {term}")
        require(term in lowering, f"midend lowering missing {term}")
        require(term in snapshot, f"public API snapshot missing {term}")


def validate_fixture_docs_catalog() -> None:
    fixture = read("tests/validation/331_api_structured_logging.spectra")
    for term in [
        'register_logging("json")',
        'register_logging("text")',
        "logging_line",
        "logging_request_id",
        "logging_len",
        "req-",
        "status=429",
    ]:
        require(term in fixture, f"logging fixture missing {term}")

    catalog = read("packages/spectra-contract/catalog/stdlib.toml")
    for term in [
        'path = "std.api.middleware.register_logging"',
        'path = "std.api.middleware.logging_len"',
        'path = "std.api.middleware.logging_line"',
        'path = "std.api.middleware.logging_request_id"',
        'fixture = "tests/validation/331_api_structured_logging.spectra"',
    ]:
        require(term in catalog, f"catalog missing {term}")

    docs = read("docs/api/std-api-middleware.md")
    for term in [
        "register_logging",
        "logging_len",
        "logging_line",
        "logging_request_id",
        "latency_us",
        "331_api_structured_logging.spectra",
    ]:
        require(term in docs, f"middleware docs missing {term}")


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2303")
    require(item is not None, "R-2303 missing from roadmap")
    require(item.get("status") in {"in_progress", "complete"}, "invalid R-2303 status")
    require(item.get("owner") == "web", "R-2303 owner changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in [
        "unique request ID",
        "one log line",
        "JSON for production",
        "331_api_structured_logging.spectra",
        "validate_r2303_structured_logging.py",
    ]:
        require(term in acceptance, f"roadmap acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2303 Structured Logging and Request ID Tracing", 1)[1].split(
        "## R-2304", 1
    )[0]
    for term in [
        "Status: `complete`",
        "331_api_structured_logging.spectra",
        "validate_r2303_structured_logging.py",
    ]:
        require(term in block, f"backlog R-2303 missing {term}")


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require(
        "validate_r2303_structured_logging.py" in runner,
        "run_tests.ps1 does not invoke R-2303",
    )
    require(
        'Teste = "validate_r2303_structured_logging"' in runner,
        "run_tests.ps1 does not record R-2303",
    )


def validate_commands() -> None:
    cargo = cargo_command()
    binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    run_command([cargo, "test", "-q", "-p", "spectra-api", "--lib", "middleware", "--offline"])
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
    run_command([str(binary), "check", "tests/validation/331_api_structured_logging.spectra"])
    run_command([str(binary), "run", "tests/validation/331_api_structured_logging.spectra"])


def main() -> None:
    validate_implementation()
    validate_frontend_and_lowering()
    validate_fixture_docs_catalog()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2303 structured logging and request ID tracing")


if __name__ == "__main__":
    main()
