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
    print(f"R-2312 validation failed: {message}", file=sys.stderr)
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
    http_types = read("packages/spectra-api/src/http_types.rs")
    http_host = read("packages/spectra-api/src/http_host.rs")
    for term in [
        "pub enum CookieSameSite",
        "pub enum CookieError",
        "pub fn with_options",
        "pub fn sign",
        "pub fn verify",
        "hmac::verify",
        "SameSite=None cookies must set Secure",
        "set-cookie",
        "fn is_expired_at",
    ]:
        require(term in http_types, f"http_types.rs missing {term}")
    for term in [
        'pub extern "C" fn cookie_with_options',
        'pub extern "C" fn response_with_cookie',
        'pub extern "C" fn cookie_sign',
        'pub extern "C" fn cookie_verify',
        'pub extern "C" fn cookie_error_code',
        'pub extern "C" fn cookie_error_message',
    ]:
        require(term in http_host, f"http_host.rs missing {term}")

    host_calls = read("packages/spectra-api/src/host_calls.rs")
    runtime_api = read("runtime/src/api/mod.rs")
    names = [
        "spectra.api.http.cookie_with_options",
        "spectra.api.http.cookie_path",
        "spectra.api.http.cookie_domain",
        "spectra.api.http.cookie_max_age",
        "spectra.api.http.cookie_secure",
        "spectra.api.http.cookie_http_only",
        "spectra.api.http.cookie_same_site",
        "spectra.api.http.cookie_header",
        "spectra.api.http.response_with_cookie",
        "spectra.api.http.cookie_sign",
        "spectra.api.http.cookie_verify",
        "spectra.api.http.cookie_is_expired",
        "spectra.api.http.cookie_error_code",
        "spectra.api.http.cookie_error_message",
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
    contract = read("compiler/src/semantic/builtin_contract.rs")
    lowering = read("midend/src/lowering_std_api.rs")
    snapshot = read("compiler/tests/snapshots/std_api_public_function_table.snap")
    for source, label in [
        (semantic, "semantic cookie module"),
        (contract, "cookie contract"),
        (lowering, "cookie lowering"),
    ]:
        require("cookie_with_options" in source, f"{label} is missing")
        require("cookie_verify" in source, f"{label} does not expose verification")
    for term in [
        "func std.api.http.cookie_with_options",
        "func std.api.http.response_with_cookie",
        "func std.api.http.cookie_verify",
        "func std.api.http.cookie_error_code",
    ]:
        require(term in snapshot, f"snapshot surface missing {term}")


def validate_fixture_docs_catalog() -> None:
    fixture = read("tests/validation/338_api_cookie.spectra")
    for term in [
        "cookie_with_options",
        "cookie_sign",
        "cookie_verify",
        "response_with_cookie",
        "cookie_error_code",
        "cookie_same_site",
    ]:
        require(term in fixture, f"cookie fixture missing {term}")

    catalog = read("packages/spectra-contract/catalog/stdlib.toml")
    for term in [
        'path = "std.api.http.cookie_with_options"',
        'path = "std.api.http.cookie_sign"',
        'path = "std.api.http.cookie_verify"',
        'path = "std.api.http.cookie_error_code"',
        'fixture = "tests/validation/338_api_cookie.spectra"',
    ]:
        require(term in catalog, f"catalog missing {term}")

    docs = read("docs/api/std-api-cookie.md")
    for term in [
        "SameSite",
        "Set-Cookie",
        "HMAC-SHA256",
        "constant-time",
        "338_api_cookie.spectra",
    ]:
        require(term in docs, f"cookie docs missing {term}")


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2312")
    require(item is not None, "R-2312 missing from roadmap")
    require(item.get("status") == "complete", "R-2312 is not complete")
    require(item.get("owner") == "web", "R-2312 owner changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in [
        "Path",
        "constant-time",
        "expired cookies",
        "338_api_cookie.spectra",
        "validate_r2312_cookie.py",
    ]:
        require(term in acceptance, f"roadmap acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2312 Cookie API", 1)[1].split(
        "## R-2313", 1
    )[0]
    for term in [
        "Status: `complete`",
        "338_api_cookie.spectra",
        "validate_r2312_cookie.py",
    ]:
        require(term in block, f"backlog R-2312 missing {term}")


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2312_cookie.py" in runner, "runner does not invoke R-2312")
    require(
        'Teste = "validate_r2312_cookie"' in runner,
        "runner does not record R-2312",
    )


def validate_commands() -> None:
    cargo = cargo_command()
    binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    run_command(
        [cargo, "test", "-q", "-p", "spectra-api", "--lib", "http", "--offline"]
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
    run_command([str(binary), "check", "tests/validation/338_api_cookie.spectra"])
    run_command([str(binary), "run", "tests/validation/338_api_cookie.spectra"])


def main() -> None:
    validate_implementation()
    validate_surface()
    validate_fixture_docs_catalog()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2312 typed cookie API")


if __name__ == "__main__":
    main()
