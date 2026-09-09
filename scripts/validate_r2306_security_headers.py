from __future__ import annotations

import os
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE_HOST_CALL_COUNT = 550
RUNTIME_REQUIRED_HOST_CALL_COUNT = 439


def cargo_command() -> str:
    cargo = "cargo.exe" if sys.platform.startswith("win") else "cargo"
    home = Path(os.environ.get("USERPROFILE") or os.environ.get("HOME") or "")
    candidate = home / ".cargo" / "bin" / cargo
    return str(candidate) if candidate.exists() else "cargo"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-2306 validation failed: {message}", file=sys.stderr)
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
        "DEFAULT_SECURITY_CSP",
        "DEFAULT_SECURITY_PERMISSIONS",
        "struct SecurityHeadersConfig",
        "struct SecurityRouteOverride",
        "pub struct SecurityHeadersMiddleware",
        "strict-transport-security",
        "includeSubDomains",
        "content-security-policy",
        "permissions-policy",
        "pub extern \"C\" fn register_security_headers",
        "pub extern \"C\" fn security_headers_configure",
        "pub extern \"C\" fn security_headers_route",
        "security_headers_apply_defaults_and_longest_route_policy",
        "security_headers_validate_hsts_and_cover_short_circuit_responses",
    ]:
        require(term in middleware, f"middleware.rs missing {term}")

    host_calls = read("packages/spectra-api/src/host_calls.rs")
    runtime_api = read("runtime/src/api/mod.rs")
    for name in [
        "spectra.api.middleware.register_security_headers",
        "spectra.api.middleware.security_headers_configure",
        "spectra.api.middleware.security_headers_route",
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
    for term in [
        "register_security_headers",
        "security_headers_configure",
        "security_headers_route",
    ]:
        for source in sources:
            require(term in source, f"public surface missing {term}")


def validate_fixture_docs_catalog() -> None:
    fixture = read("tests/validation/334_api_security_headers.spectra")
    for term in [
        "register_security_headers",
        "security_headers_configure",
        "security_headers_route",
        "Content-Security-Policy",
        "Permissions-Policy",
        "X-Frame-Options",
        "X-Content-Type-Options",
        "Referrer-Policy",
        "Strict-Transport-Security",
        "includeSubDomains",
        "preload",
    ]:
        require(term in fixture, f"security-header fixture missing {term}")

    catalog = read("packages/spectra-contract/catalog/stdlib.toml")
    for term in [
        'path = "std.api.middleware.register_security_headers"',
        'path = "std.api.middleware.security_headers_configure"',
        'path = "std.api.middleware.security_headers_route"',
        'fixture = "tests/validation/334_api_security_headers.spectra"',
    ]:
        require(term in catalog, f"catalog missing {term}")

    docs = read("docs/api/std-api-middleware.md")
    for term in [
        "register_security_headers",
        "security_headers_configure",
        "security_headers_route",
        "Content-Security-Policy",
        "includeSubDomains",
        "334_api_security_headers.spectra",
    ]:
        require(term in docs, f"middleware docs missing {term}")


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2306")
    require(item is not None, "R-2306 missing from roadmap")
    require(item.get("status") == "complete", "R-2306 is not complete")
    require(item.get("owner") == "web", "R-2306 owner changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in [
        "every response",
        "per route",
        "HSTS",
        "334_api_security_headers.spectra",
        "validate_r2306_security_headers.py",
    ]:
        require(term in acceptance, f"roadmap acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2306 Security Headers Middleware", 1)[1].split(
        "## R-2307", 1
    )[0]
    for term in [
        "Status: `complete`",
        "334_api_security_headers.spectra",
        "validate_r2306_security_headers.py",
    ]:
        require(term in block, f"backlog R-2306 missing {term}")


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2306_security_headers.py" in runner, "runner does not invoke R-2306")
    require(
        'Teste = "validate_r2306_security_headers"' in runner,
        "runner does not record R-2306",
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
    run_command([str(binary), "check", "tests/validation/334_api_security_headers.spectra"])
    run_command([str(binary), "run", "tests/validation/334_api_security_headers.spectra"])


def main() -> None:
    validate_implementation()
    validate_surface()
    validate_fixture_docs_catalog()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2306 security headers")


if __name__ == "__main__":
    main()
