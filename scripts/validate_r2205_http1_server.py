from __future__ import annotations

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
    print(f"R-2205 validation failed: {message}", file=sys.stderr)
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


def parse_toml(path: str):
    with (ROOT / path).open("rb") as fh:
        return tomllib.load(fh)


def validate_server_surface() -> None:
    server = read("packages/spectra-api/src/server.rs")
    for term in [
        "pub struct ServerConfig",
        "pub struct ServerResponse",
        "pub struct ServerStats",
        "pub enum ServerError",
        "pub struct HttpServer",
        "pub fn start",
        "pub fn local_addr",
        "pub fn stats",
        "pub fn shutdown",
        "TcpListener",
        "TcpStream",
        "set_nonblocking(true)",
        "run_accept_loop",
        "accept_ready_connections",
        "service_connections",
        "max_body_bytes",
        "read_timeout",
        "idle_timeout",
        "max_connections",
        "queue_error_response(connection, 413",
        "queue_error_response(connection, 408",
        "queue_error_response(connection, 400",
    ]:
        require(term in server, f"missing server implementation term {term}")

    for test in [
        "end_to_end_get_post_chunked_and_head",
        "body_limit_violation_returns_413_and_cleans_up",
        "slowloris_timeout_closes_connection",
        "parse_error_returns_400_and_cleans_up",
        "connection_limiter_survives_10k_concurrent_slots_without_threads",
    ]:
        require(test in server, f"missing R-2205 regression test {test}")


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2205 = items.get("R-2205")
    require(r2205 is not None, "R-2205 missing from roadmap")
    require(r2205.get("status") == "complete", "R-2205 must be marked complete")
    require(r2205.get("owner") == "web", "R-2205 owner must remain web")
    require(r2205.get("dependencies") == ["R-2204"], "R-2205 must depend on R-2204")
    acceptance = "\n".join(r2205.get("acceptance", []))
    for term in [
        "GET, POST with body, chunked responses, and HEAD",
        "Body size limits and slowloris protections",
        "timeout, body-limit violation, and parse error",
        "10k concurrent connections",
        "cargo test -p spectra-api",
        "scripts/validate_r2205_http1_server.py",
    ]:
        require(term in acceptance, f"R-2205 acceptance must mention {term}")

    backlog = read("docs/roadmap-backlog.md")
    require("## R-2205 HTTP/1.1 Server" in backlog, "backlog R-2205 missing")
    r2205_block = backlog.split("## R-2205 HTTP/1.1 Server", 1)[1].split("## R-2206", 1)[0]
    for term in [
        "Status: `complete`",
        "packages/spectra-api/src/server.rs",
        "HttpServer",
        "ServerConfig",
        "ServerResponse",
        "ServerStats",
        "validate_r2205_http1_server.py",
    ]:
        require(term in r2205_block, f"backlog R-2205 block must mention {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2205` HTTP/1.1 server (complete;" in plan,
        "implementation plan must mark R-2205 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require(
        "validate_r2205_http1_server.py" in runner,
        "run_tests.ps1 must run R-2205 validator",
    )
    require(
        'Teste = "validate_r2205_http1_server"' in runner,
        "run_tests.ps1 must record R-2205 result",
    )


def main() -> None:
    validate_server_surface()
    run_command(["cargo", "test", "-q", "-p", "spectra-api"])
    validate_planning()
    validate_runner()
    print("validated R-2205 HTTP/1.1 server")


if __name__ == "__main__":
    main()
