from __future__ import annotations

import os
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE_HOST_CALL_COUNT = 414
RUNTIME_REQUIRED_HOST_CALL_COUNT = 346


def cargo_command() -> str:
    cargo = "cargo.exe" if sys.platform.startswith("win") else "cargo"
    home = Path(os.environ.get("USERPROFILE") or os.environ.get("HOME") or "")
    candidate = home / ".cargo" / "bin" / cargo
    return str(candidate) if candidate.exists() else "cargo"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-2307 validation failed: {message}", file=sys.stderr)
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
        "enum ApiKeySource",
        "query_parameter",
        "struct ApiKeyRecord",
        "pub struct ApiKeyMiddleware",
        "invalid_api_key",
        "application/problem+json",
        "ApiKeyRejection::Missing",
        "ApiKeyRejection::Unknown",
        "ApiKeyRejection::Revoked",
        "ApiKeyRejection::Expired",
        "x-spectra-api-key",
        "pub extern \"C\" fn register_api_key",
        "pub extern \"C\" fn api_key_add",
        "pub extern \"C\" fn api_key_revoke",
        "api_key_auth_handles_valid_missing_expired_revoked_and_query_keys",
        "api_key_identity_can_feed_api_key_rate_limit_scope",
    ]:
        require(term in middleware, f"middleware.rs missing {term}")

    host_calls = read("packages/spectra-api/src/host_calls.rs")
    runtime_api = read("runtime/src/api/mod.rs")
    for name in [
        "spectra.api.middleware.register_api_key",
        "spectra.api.middleware.api_key_add",
        "spectra.api.middleware.api_key_revoke",
    ]:
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
    sources = [
        read("compiler/src/semantic/builtin_api_services.rs"),
        read("compiler/src/semantic/builtin_contract.rs"),
        read("midend/src/lowering_std_api.rs"),
        read("compiler/tests/snapshots/std_api_public_function_table.snap"),
    ]
    for term in ["register_api_key", "api_key_add", "api_key_revoke"]:
        for source in sources:
            require(term in source, f"public surface missing {term}")


def validate_fixture_docs_catalog() -> None:
    fixture = read("tests/validation/333_api_key_auth.spectra")
    for term in [
        'register_api_key("header")',
        'register_api_key("query")',
        "api_key_add",
        "api_key_revoke",
        'register_rate_limit(',
        '"api_key"',
        "problem+json",
        "429",
    ]:
        require(term in fixture, f"API-key fixture missing {term}")

    catalog = read("packages/spectra-contract/catalog/stdlib.toml")
    for term in [
        'path = "std.api.middleware.register_api_key"',
        'path = "std.api.middleware.api_key_add"',
        'path = "std.api.middleware.api_key_revoke"',
        'fixture = "tests/validation/333_api_key_auth.spectra"',
    ]:
        require(term in catalog, f"catalog missing {term}")

    docs = read("docs/api/std-api-middleware.md")
    for term in [
        "register_api_key",
        "api_key_add",
        "api_key_revoke",
        "invalid_api_key",
        "application/problem+json",
        "333_api_key_auth.spectra",
    ]:
        require(term in docs, f"middleware docs missing {term}")


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2307")
    require(item is not None, "R-2307 missing from roadmap")
    require(item.get("status") in {"in_progress", "complete"}, "invalid R-2307 status")
    require(item.get("owner") == "web", "R-2307 owner changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in [
        "configured source",
        "401",
        "rate limiting",
        "333_api_key_auth.spectra",
        "validate_r2307_api_key.py",
    ]:
        require(term in acceptance, f"roadmap acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2307 API Key Authentication", 1)[1].split(
        "## R-2308", 1
    )[0]
    for term in [
        "Status: `complete`",
        "333_api_key_auth.spectra",
        "validate_r2307_api_key.py",
    ]:
        require(term in block, f"backlog R-2307 missing {term}")


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2307_api_key.py" in runner, "runner does not invoke R-2307")
    require(
        'Teste = "validate_r2307_api_key"' in runner,
        "runner does not record R-2307",
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
    run_command([str(binary), "check", "tests/validation/333_api_key_auth.spectra"])
    run_command([str(binary), "run", "tests/validation/333_api_key_auth.spectra"])


def main() -> None:
    validate_implementation()
    validate_surface()
    validate_fixture_docs_catalog()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2307 API key authentication")


if __name__ == "__main__":
    main()
