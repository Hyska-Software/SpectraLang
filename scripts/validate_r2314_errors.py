from __future__ import annotations

import os
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE_HOST_CALL_COUNT = 555
RUNTIME_REQUIRED_HOST_CALL_COUNT = 439


def cargo_command() -> str:
    cargo = "cargo.exe" if sys.platform.startswith("win") else "cargo"
    home = Path(os.environ.get("USERPROFILE") or os.environ.get("HOME") or "")
    candidate = home / ".cargo" / "bin" / cargo
    return str(candidate) if candidate.exists() else "cargo"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-2314 validation failed: {message}", file=sys.stderr)
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
    errors = read("packages/spectra-api/src/errors.rs")
    for term in [
        "pub struct ApiError",
        "pub fn sanitized",
        "application/problem+json",
        "internal_code",
        "internal_detail",
        "pub(crate) struct ErrorLogEntry",
        "eprintln!(",
        "pub(crate) struct ExceptionMiddleware",
        'pub extern "C" fn internal',
        'pub extern "C" fn exception_middleware',
        "internal_error_logs_full_detail_and_sanitizes_response",
        "exception_middleware_recovers_per_chain_with_custom_public_mapping",
    ]:
        require(term in errors, f"errors.rs missing {term}")

    middleware = read("packages/spectra-api/src/middleware.rs")
    for term in [
        "fn on_error(",
        "fn recover_sync_error",
        "async fn recover_async_error",
        "fn execute_sync_inner",
        "async fn execute_async_inner",
    ]:
        require(term in middleware, f"middleware.rs missing {term}")

    handles = read("runtime/src/handles/mod.rs")
    require("ApiError = 76" in handles, "ApiError handle kind is missing")

    host_calls = read("packages/spectra-api/src/host_calls.rs")
    runtime_api = read("runtime/src/api/mod.rs")
    names = [
        "spectra.api.errors.new",
        "spectra.api.errors.internal_error",
        "spectra.api.errors.status",
        "spectra.api.errors.code",
        "spectra.api.errors.message",
        "spectra.api.errors.response",
        "spectra.api.errors.exception_middleware",
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
    services = read("compiler/src/semantic/builtin_api_services.rs")
    contract = read("compiler/src/semantic/builtin_contract.rs")
    lowering = read("midend/src/lowering_std_api.rs")
    handles = read("midend/src/lowering_handles.rs")
    snapshot = read("compiler/tests/snapshots/std_api_public_function_table.snap")
    require("ApiError" in services and "exception_middleware" in services, "semantic error surface is missing")
    require("std.api.errors.exception_middleware" in contract, "error contract is missing")
    require("spectra.api.errors.exception_middleware" in lowering, "error lowering is missing")
    require("ApiError" in handles, "ApiError handle lowering is missing")
    for term in [
        "type std.api.errors.ApiError",
        "func std.api.errors.new",
        "func std.api.errors.internal_error",
        "func std.api.errors.response",
        "func std.api.errors.exception_middleware",
    ]:
        require(term in snapshot, f"snapshot surface missing {term}")


def validate_fixture_docs_catalog() -> None:
    fixture = read("tests/validation/340_api_errors.spectra")
    for term in [
        "new_error",
        "internal_error",
        "application/problem+json",
        "internal server error",
        "exception_middleware",
        "chain_len",
    ]:
        require(term in fixture, f"error fixture missing {term}")

    catalog = read("packages/spectra-contract/catalog/stdlib.toml")
    for term in [
        'path = "std.api.errors.ApiError"',
        'path = "std.api.errors.new"',
        'path = "std.api.errors.internal_error"',
        'path = "std.api.errors.response"',
        'path = "std.api.errors.exception_middleware"',
        'fixture = "tests/validation/340_api_errors.spectra"',
    ]:
        require(term in catalog, f"catalog missing {term}")

    docs = read("docs/api/std-api-errors.md")
    for term in [
        "ApiError",
        "Problem Details",
        "internal_error",
        "sanitized",
        "exception_middleware",
        "340_api_errors.spectra",
    ]:
        require(term in docs, f"error docs missing {term}")


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2314")
    require(item is not None, "R-2314 missing from roadmap")
    require(item.get("status") == "complete", "R-2314 is not complete")
    require(item.get("owner") == "web", "R-2314 owner changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in [
        "ApiError",
        "RFC 7807",
        "sanitized",
        "exception middleware",
        "340_api_errors.spectra",
        "validate_r2314_errors.py",
    ]:
        require(term in acceptance, f"roadmap acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2314 Unified Error Handling", 1)[1].split(
        "## R-2315", 1
    )[0]
    for term in [
        "Status: `complete`",
        "340_api_errors.spectra",
        "validate_r2314_errors.py",
    ]:
        require(term in block, f"backlog R-2314 missing {term}")


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2314_errors.py" in runner, "runner does not invoke R-2314")
    require(
        'Teste = "validate_r2314_errors"' in runner,
        "runner does not record R-2314",
    )


def validate_commands() -> None:
    cargo = cargo_command()
    binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    for test_filter in ["errors", "middleware"]:
        run_command(
            [
                cargo,
                "test",
                "-q",
                "-p",
                "spectra-api",
                "--lib",
                test_filter,
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
    run_command([str(binary), "check", "tests/validation/340_api_errors.spectra"])
    run_command([str(binary), "run", "tests/validation/340_api_errors.spectra"])


def main() -> None:
    validate_implementation()
    validate_surface()
    validate_fixture_docs_catalog()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2314 unified errors and exception middleware")


if __name__ == "__main__":
    main()
