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
    print(f"R-2309 validation failed: {message}", file=sys.stderr)
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
    oauth = read("packages/spectra-api/src/oauth.rs")
    for term in [
        "pub enum OAuthError",
        "PKCE_VERIFIER_BYTES",
        "SystemRandom",
        "SHA256",
        "code_challenge_method",
        "constant_time_equal",
        "grant_type",
        "refresh_token",
        "token_type_hint",
        "pub extern \"C\" fn client_new",
        "pub extern \"C\" fn exchange_code",
        "pub extern \"C\" fn refresh",
        "pub extern \"C\" fn revoke",
        "oauth_pkce_code_exchange_refresh_and_revocation_are_end_to_end",
        "oauth_client_rejects_invalid_configuration_and_empty_state",
    ]:
        require(term in oauth, f"oauth.rs missing {term}")

    handles = read("runtime/src/handles/mod.rs")
    require("ApiOAuthClient" in handles, "OAuth client handle kind is missing")
    require("ApiOAuthToken" in handles, "OAuth token handle kind is missing")

    host_calls = read("packages/spectra-api/src/host_calls.rs")
    runtime_api = read("runtime/src/api/mod.rs")
    for name in [
        "spectra.api.oauth.client_new",
        "spectra.api.oauth.client_set_revocation_url",
        "spectra.api.oauth.authorization_url",
        "spectra.api.oauth.exchange_code",
        "spectra.api.oauth.refresh",
        "spectra.api.oauth.revoke",
        "spectra.api.oauth.token_access_token",
        "spectra.api.oauth.token_refresh_token",
        "spectra.api.oauth.token_type",
        "spectra.api.oauth.token_expires_at_ms",
        "spectra.api.oauth.token_scope",
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
    semantic = read("compiler/src/semantic/builtin_api_core.rs")
    contract = read("compiler/src/semantic/builtin_contract.rs")
    lowering = read("midend/src/lowering_std_api.rs")
    handles = read("midend/src/lowering_handles.rs")
    snapshot = read("compiler/tests/snapshots/std_api_public_function_table.snap")
    require("make_std_api_oauth" in semantic, "semantic OAuth module is missing")
    require("std.api.oauth" in contract, "OAuth contract module is missing")
    require("spectra.api.oauth.exchange_code" in lowering, "OAuth lowering is missing")
    require("OAuthClient" in handles and "OAuthToken" in handles, "OAuth handle lowering is missing")
    for term in [
        "module std.api.oauth",
        "type std.api.oauth.OAuthClient",
        "func std.api.oauth.client_new",
        "func std.api.oauth.exchange_code",
        "func std.api.oauth.revoke",
    ]:
        require(term in snapshot, f"snapshot surface missing {term}")


def validate_fixture_docs_catalog() -> None:
    fixture = read("tests/validation/337_api_oauth.spectra")
    for term in [
        "client_new",
        "authorization_url",
        "code_challenge_method=S256",
        "fixture-state",
        "client_set_revocation_url",
    ]:
        require(term in fixture, f"OAuth fixture missing {term}")

    catalog = read("packages/spectra-contract/catalog/stdlib.toml")
    for term in [
        'path = "std.api.oauth"',
        'path = "std.api.oauth.OAuthClient"',
        'path = "std.api.oauth.exchange_code"',
        'path = "std.api.oauth.refresh"',
        'path = "std.api.oauth.revoke"',
        'fixture = "tests/validation/337_api_oauth.spectra"',
    ]:
        require(term in catalog, f"catalog missing {term}")

    docs = read("docs/api/std-api-oauth.md")
    for term in [
        "PKCE",
        "code_challenge_method=S256",
        "exchange_code",
        "refresh",
        "revocation",
        "337_api_oauth.spectra",
    ]:
        require(term in docs, f"OAuth docs missing {term}")


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2309")
    require(item is not None, "R-2309 missing from roadmap")
    require(item.get("status") == "complete", "R-2309 is not complete")
    require(item.get("owner") == "web", "R-2309 owner changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in [
        "authorization code flow",
        "PKCE",
        "refresh tokens",
        "337_api_oauth.spectra",
        "validate_r2309_oauth.py",
    ]:
        require(term in acceptance, f"roadmap acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2309 OAuth2 Client", 1)[1].split(
        "## R-2310", 1
    )[0]
    for term in [
        "Status:",
        "complete",
        "337_api_oauth.spectra",
        "validate_r2309_oauth.py",
    ]:
        require(term in block, f"backlog R-2309 missing {term}")


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2309_oauth.py" in runner, "runner does not invoke R-2309")
    require(
        'Teste = "validate_r2309_oauth"' in runner,
        "runner does not record R-2309",
    )


def validate_commands() -> None:
    cargo = cargo_command()
    binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    run_command(
        [cargo, "test", "-q", "-p", "spectra-api", "--lib", "oauth", "--offline"]
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
    run_command([str(binary), "check", "tests/validation/337_api_oauth.spectra"])
    run_command([str(binary), "run", "tests/validation/337_api_oauth.spectra"])


def main() -> None:
    validate_implementation()
    validate_surface()
    validate_fixture_docs_catalog()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2309 OAuth2 client")


if __name__ == "__main__":
    main()
