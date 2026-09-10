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
    print(f"R-2316 validation failed: {message}", file=sys.stderr)
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
    security = read("packages/spectra-api/src/security.rs")
    for term in [
        "pub struct CsrfPolicy",
        "pub struct SsrfPolicy",
        "csrf_origin_rejected",
        "is_private_or_link_local",
        "ApiCsrfPolicy",
        "ApiSsrfPolicy",
        'pub extern "C" fn csrf_middleware',
        'pub extern "C" fn ssrf_allow_private_networks',
        "ssrf_policy_blocks_private_and_link_local_addresses_by_default",
    ]:
        require(term in security, f"security.rs missing {term}")

    client = read("packages/spectra-api/src/client_core.rs")
    for term in [
        "pub ssrf_policy: SsrfPolicy",
        "SsrfBlocked",
        "fn resolve_destination",
        "TcpStream::connect(addresses)",
        "allow_private_networks",
    ]:
        require(term in client, f"client_core.rs missing {term}")
    async_client = read("packages/spectra-api/src/client_async.rs")
    require("resolve_destination" in async_client, "async client does not validate destinations")

    server = read("packages/spectra-api/src/server_core.rs")
    for term in [
        "pub max_body_bytes: usize",
        "pub read_timeout: Duration",
        "pub idle_timeout: Duration",
        "ParseErrorKind::BodyTooLarge",
        "queue_error_response(connection, 408",
    ]:
        require(term in server, f"server_core.rs missing {term}")
    server_host = read("packages/spectra-api/src/server_host.rs")
    for term in [
        'pub extern "C" fn server_set_max_body_bytes',
        'pub extern "C" fn server_set_read_timeout',
        'pub extern "C" fn server_set_idle_timeout',
    ]:
        require(term in server_host, f"server_host.rs missing {term}")

    handles = read("runtime/src/handles/mod.rs")
    require("ApiCsrfPolicy = 77" in handles, "CSRF handle kind is missing")
    require("ApiSsrfPolicy = 78" in handles, "SSRF handle kind is missing")

    host_calls = read("packages/spectra-api/src/host_calls.rs")
    runtime_api = read("packages/spectra-api/src/host_calls.rs")
    names = [
        "spectra.api.server.set_max_body_bytes",
        "spectra.api.server.set_read_timeout",
        "spectra.api.server.set_idle_timeout",
        "spectra.api.client.set_ssrf_policy",
        "spectra.api.security.csrf_policy",
        "spectra.api.security.csrf_allow_origin",
        "spectra.api.security.csrf_origin_count",
        "spectra.api.security.csrf_middleware",
        "spectra.api.security.ssrf_policy",
        "spectra.api.security.ssrf_allow_private_networks",
        "spectra.api.security.ssrf_allows",
    ]
    for name in names:
        require(name in host_calls, f"{name} missing from package host registry")
        require(name in runtime_api, f"{name} missing from runtime contract")
    require(
        f"assert_eq!(HOST_CALLS.len(), {PACKAGE_HOST_CALL_COUNT})"
        in read("packages/spectra-api/src/api_tests.rs"),
        "package host-call count is not synchronized",
    )


def validate_surface() -> None:
    services = read("compiler/src/semantic/builtin_api_services.rs")
    core = read("compiler/src/semantic/builtin_api_core.rs")
    contract = read("compiler/src/semantic/builtin_contract.rs")
    lowering = read("midend/src/lowering_std_api.rs")
    handles = read("midend/src/lowering_handles.rs")
    init = read("compiler/src/semantic/semantic_init.rs")
    snapshot = read("compiler/tests/snapshots/std_api_public_function_table.snap")
    require("make_std_api_security" in services, "semantic security module is missing")
    require("make_std_api_security" in core, "security module is not registered")
    require("std.api.security" in contract, "security contract is missing")
    require('("security", "csrf_middleware")' in lowering, "CSRF lowering is missing")
    require('("client", "set_ssrf_policy")' in lowering, "client SSRF lowering is missing")
    require("CsrfPolicy" in handles and "SsrfPolicy" in handles, "security handles are not lowered")
    require("std.api.security" in init, "security namespace is not seeded")
    for term in [
        "module std.api.security",
        "type std.api.security.CsrfPolicy",
        "type std.api.security.SsrfPolicy",
        "func std.api.security.csrf_policy",
        "func std.api.security.csrf_middleware",
        "func std.api.security.ssrf_policy",
        "func std.api.client.set_ssrf_policy",
        "func std.api.server.set_max_body_bytes",
    ]:
        require(term in snapshot, f"snapshot surface missing {term}")


def validate_fixture_docs_catalog() -> None:
    fixture = read("tests/validation/341_api_security.spectra")
    for term in [
        "csrf_policy",
        "csrf_allow_origin",
        "csrf_middleware",
        "ssrf_policy",
        "ssrf_allow_private_networks",
        "set_max_body_bytes",
        "set_read_timeout",
        "set_idle_timeout",
    ]:
        require(term in fixture, f"security fixture missing {term}")

    catalog = read("packages/spectra-contract/catalog/stdlib.toml")
    for term in [
        'path = "std.api.security"',
        'path = "std.api.security.CsrfPolicy"',
        'path = "std.api.security.SsrfPolicy"',
        'path = "std.api.security.csrf_middleware"',
        'path = "std.api.security.ssrf_allow_private_networks"',
        'path = "std.api.client.set_ssrf_policy"',
        'path = "std.api.server.set_max_body_bytes"',
        'fixture = "tests/validation/341_api_security.spectra"',
    ]:
        require(term in catalog, f"catalog missing {term}")

    docs = read("docs/api/std-api-security.md")
    for term in [
        "CSRF",
        "SSRF",
        "Content-Length",
        "408",
        "504",
        "341_api_security.spectra",
    ]:
        require(term in docs, f"security docs missing {term}")


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2316")
    require(item is not None, "R-2316 missing from roadmap")
    require(item.get("status") == "complete", "R-2316 is not complete")
    require(item.get("owner") == "web", "R-2316 owner changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in [
        "CSRF",
        "SSRF",
        "body size",
        "timeouts",
        "341_api_security.spectra",
        "validate_r2316_security.py",
    ]:
        require(term.lower() in acceptance.lower(), f"roadmap acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2316 Threat Mitigations", 1)[1].split(
        "## R-2317", 1
    )[0]
    for term in [
        "Status: `complete`",
        "341_api_security.spectra",
        "validate_r2316_security.py",
    ]:
        require(term in block, f"backlog R-2316 missing {term}")


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2316_security.py" in runner, "runner does not invoke R-2316")
    require(
        'Teste = "validate_r2316_security"' in runner,
        "runner does not record R-2316",
    )


def validate_commands() -> None:
    cargo = cargo_command()
    binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    for test_filter in ["security", "client", "server"]:
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
    run_command([str(binary), "check", "tests/validation/341_api_security.spectra"])
    run_command([str(binary), "run", "tests/validation/341_api_security.spectra"])


def main() -> None:
    validate_implementation()
    validate_surface()
    validate_fixture_docs_catalog()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2316 threat mitigations")


if __name__ == "__main__":
    main()
