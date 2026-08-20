from __future__ import annotations

import os
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def cargo_command() -> str:
    cargo = "cargo.exe" if sys.platform.startswith("win") else "cargo"
    home = Path(os.environ.get("USERPROFILE") or os.environ.get("HOME") or "")
    candidate = home / ".cargo" / "bin" / cargo
    return str(candidate) if candidate.exists() else "cargo"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-2315 validation failed: {message}", file=sys.stderr)
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
    tls = read("packages/spectra-api/src/tls.rs")
    for term in [
        "pub fn with_ocsp_response",
        "with_single_cert_with_ocsp",
        "pub fn ocsp_response",
        "pub struct TlsCertificateStore",
        "pub fn current(&self)",
        "pub fn rotate(&self, config: TlsServerConfig)",
        "pub fn serve_single_https_request_rotating",
        "ocsp_response_is_stapled_to_server_handshake",
        "certificate_rotation_changes_future_handshakes_without_listener_restart",
    ]:
        require(term in tls, f"tls.rs missing {term}")

    middleware = read("packages/spectra-api/src/middleware.rs")
    for term in [
        "DEFAULT_HSTS_MAX_AGE",
        "strict-transport-security",
        "includeSubDomains",
        "preload",
        "security_headers_validate_hsts_and_cover_short_circuit_responses",
    ]:
        require(term in middleware, f"security headers implementation missing {term}")


def validate_fixture_docs() -> None:
    fixture = read("tests/validation/334_api_security_headers.spectra")
    for term in [
        "Strict-Transport-Security",
        "includeSubDomains",
        "preload",
    ]:
        require(term in fixture, f"HSTS fixture missing {term}")

    docs = read("docs/api/std-api-https-hardening.md")
    for term in [
        "with_ocsp_response",
        "with_single_cert_with_ocsp",
        "TlsCertificateStore",
        "serve_single_https_request_rotating",
        "334_api_security_headers.spectra",
        "validate_r2315_https_hardening.py",
    ]:
        require(term in docs, f"HTTPS hardening docs missing {term}")
    require(
        "std-api-https-hardening.md" in read("docs/api/README.md"),
        "API docs index does not link HTTPS hardening",
    )


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2315")
    require(item is not None, "R-2315 missing from roadmap")
    require(item.get("status") == "complete", "R-2315 is not complete")
    require(item.get("owner") == "web", "R-2315 owner changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in [
        "Strict-Transport-Security",
        "OCSP",
        "certificate rotation",
        "cargo test -p spectra-api --lib tls",
        "334_api_security_headers.spectra",
        "validate_r2315_https_hardening.py",
    ]:
        require(term in acceptance, f"roadmap acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2315 HTTPS Hardening", 1)[1].split(
        "## R-2316", 1
    )[0]
    for term in [
        "Status: `complete`",
        "with_single_cert_with_ocsp",
        "TlsCertificateStore",
        "334_api_security_headers.spectra",
        "validate_r2315_https_hardening.py",
    ]:
        require(term in block, f"backlog R-2315 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2315` HTTPS hardening (complete;" in plan,
        "implementation plan must mark R-2315 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require(
        "validate_r2315_https_hardening.py" in runner,
        "runner does not invoke R-2315",
    )
    require(
        'Teste = "validate_r2315_https_hardening"' in runner,
        "runner does not record R-2315",
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
            "tls",
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
            "spectra-api",
            "--lib",
            "middleware",
            "--offline",
            "--",
            "--test-threads=1",
        ]
    )
    run_command([cargo, "build", "-q", "-p", "spectra-cli", "--bin", "spectralang", "--offline"])
    run_command([str(binary), "check", "tests/validation/334_api_security_headers.spectra"])
    run_command([str(binary), "run", "tests/validation/334_api_security_headers.spectra"])


def main() -> None:
    validate_implementation()
    validate_fixture_docs()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2315 HTTPS hardening")


if __name__ == "__main__":
    main()
