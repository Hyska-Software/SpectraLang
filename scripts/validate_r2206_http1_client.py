from __future__ import annotations

import os
import shutil
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
    print(f"R-2206 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def run_command(args: list[str]) -> str:
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
    return completed.stdout


def cargo_cmd() -> str:
    configured = os.environ.get("CARGO")
    if configured:
        return configured
    found = shutil.which("cargo")
    if found:
        return found
    windows_default = Path.home() / ".cargo" / "bin" / "cargo.exe"
    if windows_default.exists():
        return str(windows_default)
    return "cargo"


def parse_toml(path: str):
    with (ROOT / path).open("rb") as fh:
        return tomllib.load(fh)


def validate_client_surface() -> None:
    client = read("packages/spectra-api/src/client.rs")
    for term in [
        "pub struct ClientConfig",
        "pub struct ClientRequest",
        "pub struct ClientResponse",
        "pub enum ClientErrorKind",
        "pub struct ClientError",
        "pub struct ClientStats",
        "pub struct HttpClient",
        "pub fn get",
        "pub fn head",
        "pub fn delete",
        "pub fn post",
        "pub fn put",
        "pub fn patch",
        "pub fn request",
        "fn send_once",
        "fn take_or_connect",
        "fn put_connection",
        "fn redirected_request",
        "fn redirect_target",
        "request_nonblocking",
        "mio::net",
        "spawn_cancellable_io_task_with_token",
        "fn read_head_response",
        "TcpStream::connect",
        "max_redirects",
        "pool_idle_timeout",
        "ClientErrorKind::Timeout",
        "ClientErrorKind::ConnectionFailed",
        "ClientErrorKind::Protocol",
    ]:
        require(term in client, f"missing client implementation term {term}")

    for test in [
        "client_supports_methods_and_arbitrary_bodies",
        "client_reuses_pooled_connection",
        "client_follows_redirects_with_method_semantics",
        "client_enforces_redirect_limit",
        "client_handles_large_bodies",
        "client_reports_explicit_timeout",
        "client_reports_connection_failure",
        "client_reports_protocol_error",
        "client_nonblocking_request_observes_cancellation_during_readiness_wait",
    ]:
        require(test in client, f"missing R-2206 regression test {test}")


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2206 = items.get("R-2206")
    require(r2206 is not None, "R-2206 missing from roadmap")
    require(r2206.get("status") == "complete", "R-2206 must be marked complete")
    require(r2206.get("owner") == "web", "R-2206 owner must remain web")
    require(r2206.get("dependencies") == ["R-2204"], "R-2206 must depend on R-2204")
    acceptance = "\n".join(r2206.get("acceptance", []))
    for term in [
        "GET, POST, PUT, PATCH, DELETE, and HEAD with arbitrary bodies",
        "Redirect chains",
        "configured limit",
        "right method semantics",
        "Timeouts, connection failures, and protocol errors",
        "redirect chains, large bodies, and explicit timeout",
        "cargo test -p spectra-api",
        "scripts/validate_r2206_http1_client.py",
    ]:
        require(term in acceptance, f"R-2206 acceptance must mention {term}")

    backlog = read("docs/roadmap-backlog.md")
    require("## R-2206 HTTP/1.1 Client" in backlog, "backlog R-2206 missing")
    r2206_block = backlog.split("## R-2206 HTTP/1.1 Client", 1)[1].split("## R-2207", 1)[0]
    for term in [
        "Status: `complete`",
        "packages/spectra-api/src/client.rs",
        "HttpClient",
        "ClientConfig",
        "ClientRequest",
        "ClientResponse",
        "ClientError",
        "validate_r2206_http1_client.py",
    ]:
        require(term in r2206_block, f"backlog R-2206 block must mention {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2206` HTTP/1.1 client (complete;" in plan,
        "implementation plan must mark R-2206 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require(
        "validate_r2206_http1_client.py" in runner,
        "run_tests.ps1 must run R-2206 validator",
    )
    require(
        'Teste = "validate_r2206_http1_client"' in runner,
        "run_tests.ps1 must record R-2206 result",
    )


def main() -> None:
    validate_client_surface()
    run_command([cargo_cmd(), "test", "-q", "-p", "spectra-api"])
    validate_planning()
    validate_runner()
    print("validated R-2206 HTTP/1.1 client")


if __name__ == "__main__":
    main()
