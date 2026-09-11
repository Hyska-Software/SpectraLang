from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE_HOST_CALL_COUNT = 557


def read(path: str) -> str:
    source = ROOT / path
    if source.suffix == ".rs":
        return "\n".join(
            sibling.read_text(encoding="utf-8")
            for sibling in sorted(source.parent.rglob("*.rs"))
        )
    return source.read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-2216 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def parse_toml(path: str):
    with (ROOT / path).open("rb") as fh:
        return tomllib.load(fh)


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


def validate_implementation() -> None:
    server = read("packages/spectra-api/src/server.rs")
    for term in [
        "SERVER_STATE_STOPPING",
        "SERVER_SIGNAL_SIGINT",
        "SERVER_SIGNAL_SIGTERM",
        "shutdown_grace_period",
        "drained_connections",
        "cancelled_connections",
        "shutdown_signals",
        "server_listen",
        "server_serve",
        "server_local_port",
        "server_signal",
        "server_stats",
        "routed_handler",
        "ready_task",
        "mio::net",
        "Poll::new",
        "Interest::READABLE",
        "reregister",
        "server_poll_timeout",
        "spectra.async.task.ready",
        "r2216_serve_routes_to_registered_handler_and_shutdowns_cleanly",
        "r2216_shutdown_drains_in_flight_keep_alive_request",
        "r2216_shutdown_cancels_unfinished_connections_after_grace_period",
        "registered_callbacks_execute_sync_and_async_routes_end_to_end",
        "HandlerResult::Pending",
        "poll_task_once",
    ]:
        require(term in server, f"server.rs missing {term}")

    handler = read("packages/spectra-api/src/handler.rs")
    require("response_for_route" in handler, "handler route dispatch helper missing")

    routing = read("packages/spectra-api/src/routing.rs")
    require("clone_router" in routing, "router snapshot helper missing")

    http = read("packages/spectra-api/src/http.rs")
    require("store_request" in http, "HTTP request store helper missing")

    lib = read("packages/spectra-api/src/lib.rs")
    runtime = read("packages/spectra-api/src/host_calls.rs")
    api_tests = read("packages/spectra-api/src/api_tests.rs")
    midend = read("midend/src/lowering.rs")
    for name in [
        "spectra.api.server.listen",
        "spectra.api.server.serve",
        "spectra.api.server.local_port",
        "spectra.api.server.signal",
        "spectra.api.server.stats",
    ]:
        require(name in lib, f"{name} missing from spectra-api host table")
        require(name in runtime, f"{name} missing from runtime API contract")
        require(name in midend, f"{name} missing from midend host lowering")
    require(
        f"assert_eq!(HOST_CALLS.len(), {PACKAGE_HOST_CALL_COUNT})" in lib,
        f"package host-call count must be {PACKAGE_HOST_CALL_COUNT}",
    )
    require(
        f"assert_eq!(HOST_CALLS.len(), {PACKAGE_HOST_CALL_COUNT})" in api_tests,
        f"package host-call count must be {PACKAGE_HOST_CALL_COUNT}",
    )


def validate_surface_fixture_and_docs() -> None:
    builtins = read("compiler/src/semantic/builtin_modules.rs")
    snapshot = read("compiler/tests/snapshots/std_api_public_function_table.snap")
    fixture = read("tests/validation/147_api_server_lifecycle.spectra")
    docs = read("docs/api/std-api-server-lifecycle.md")
    for term in [
        "std.api.server.listen",
        "std.api.server.serve",
        "std.api.server.local_port",
        "std.api.server.signal",
        "std.api.server.stats",
    ]:
        require(term in builtins, f"builtin surface missing {term}")
        require(term in snapshot, f"std.api snapshot missing {term}")

    for term in [
        "listen(server, 0)",
        "serve(server, routes)",
        "state(server) != 2",
        "local_port(server)",
        "signal(signal_server, 15)",
        "serve(signal_server, routes)",
        "stats(signal_server, 10)",
        "shutdown(server)",
    ]:
        require(term in fixture, f"fixture missing {term}")

    for term in [
        "Shutdown stops accepting new connections immediately",
        "default drain timeout",
        "drained",
        "cancelled",
        "register_sync_callback",
        "register_async_callback",
        "330_api_handler_callbacks.spectra",
        "tests/validation/147_api_server_lifecycle.spectra",
        "scripts/validate_r2216_server_lifecycle.py",
    ]:
        require(term in docs, f"server lifecycle docs missing {term}")


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2216 = items.get("R-2216")
    require(r2216 is not None, "R-2216 missing from roadmap")
    require(r2216.get("status") == "complete", "R-2216 must be marked complete")
    require(r2216.get("owner") == "web", "R-2216 owner must remain web")
    require(
        r2216.get("dependencies")
        == [
            "R-2003",
            "R-2004",
            "R-2005",
            "R-2006",
            "R-2007",
            "R-2205",
            "R-2211",
            "R-2215",
        ],
        "R-2216 dependencies changed unexpectedly",
    )
    acceptance = "\n".join(r2216.get("acceptance", []))
    for term in [
        "configured port",
        "SIGINT/SIGTERM",
        "configurable drain timeout",
        "registered sync callbacks",
        "async callback tasks",
        "330_api_handler_callbacks.spectra",
        "147_api_server_lifecycle.spectra",
        "validate_r2216_server_lifecycle.py",
    ]:
        require(term in acceptance, f"R-2216 acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2216 Server Lifecycle, Listen, Serve, and Graceful Shutdown", 1)[
        1
    ].split("## R-2217", 1)[0]
    for term in [
        "Status: `complete`",
        "packages/spectra-api/src/server.rs",
        "docs/api/std-api-server-lifecycle.md",
        "tests/validation/147_api_server_lifecycle.spectra",
        "validate_r2216_server_lifecycle.py",
    ]:
        require(term in block, f"backlog R-2216 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2216` Server lifecycle, listen, serve, graceful shutdown (complete;" in plan,
        "implementation plan must mark R-2216 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2216_server_lifecycle.py" in runner, "run_tests.ps1 must run R-2216")
    require(
        'Teste = "validate_r2216_server_lifecycle"' in runner,
        "run_tests.ps1 must record R-2216",
    )


def validate_commands(binary: Path) -> None:
    cargo = cargo_cmd()
    run_command([cargo, "test", "-q", "-p", "spectra-api", "r2216"])
    run_command([cargo, "test", "-q", "-p", "spectra-api", "host_call_table"])
    run_command([cargo, "test", "-q", "-p", "spectra-runtime", "api_host_call_contract"])
    run_command([cargo, "build", "-q", "-p", "spectra-cli"])
    run_command([str(binary), "compile", "tests/validation/147_api_server_lifecycle.spectra"])
    run_command([str(binary), "run", "tests/validation/147_api_server_lifecycle.spectra"])


def main() -> None:
    parser = argparse.ArgumentParser()
    default_binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    parser.add_argument("--binary", type=Path, default=default_binary)
    args = parser.parse_args()

    validate_implementation()
    validate_surface_fixture_and_docs()
    validate_planning()
    validate_runner()
    validate_commands(args.binary)
    print("validated R-2216 server lifecycle, listen, serve, and graceful shutdown")


if __name__ == "__main__":
    main()
