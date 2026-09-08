from __future__ import annotations

import subprocess
import sys
import tomllib
import os
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE_HOST_CALL_COUNT = 444
RUNTIME_REQUIRED_HOST_CALL_COUNT = 347


def cargo_command() -> str:
    cargo = "cargo.exe" if sys.platform.startswith("win") else "cargo"
    home = Path(os.environ.get("USERPROFILE") or os.environ.get("HOME") or "")
    candidate = home / ".cargo" / "bin" / cargo
    if candidate.exists():
        return str(candidate)
    return "cargo"


def read(path: str) -> str:
    source = ROOT / path
    if source.suffix == ".rs":
        return "\n".join(
            sibling.read_text(encoding="utf-8")
            for sibling in sorted(source.parent.rglob("*.rs"))
        )
    return source.read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-2301 validation failed: {message}", file=sys.stderr)
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


def validate_implementation() -> None:
    middleware = read("packages/spectra-api/src/middleware.rs")
    for term in [
        "pub trait Middleware",
        "pub trait AsyncMiddleware",
        "pub enum MiddlewareDecision",
        "pub struct MiddlewareChain",
        "pub struct MiddlewareTrace",
        "execute_sync",
        "execute_async",
        "ShortCircuit",
        "short_circuited",
        "sync_chain_runs_request_order_and_response_reverse_order",
        "short_circuit_stops_remaining_requests_and_unwinds_executed_hooks",
        "async_chain_accepts_sync_and_async_middleware",
    ]:
        require(term in middleware, f"middleware.rs missing {term}")

    lib = read("packages/spectra-api/src/lib.rs")
    runtime = read("runtime/src/api/mod.rs")
    midend = read("midend/src/lowering.rs")
    builtins = read("compiler/src/semantic/builtin_modules.rs")
    semantic = read("compiler/src/semantic/mod.rs")
    for name in [
        "spectra.api.middleware.chain",
        "spectra.api.middleware.chain_new",
        "spectra.api.middleware.chain_len",
        "spectra.api.middleware.register_sync",
        "spectra.api.middleware.register_sync_short_circuit",
        "spectra.api.middleware.register_async",
        "spectra.api.middleware.register_async_short_circuit",
        "spectra.api.middleware.use_sync",
        "spectra.api.middleware.use_async",
        "spectra.api.middleware.execute_sync",
        "spectra.api.middleware.execute_async",
        "spectra.api.middleware.last_trace",
        "spectra.api.middleware.trace_len",
        "spectra.api.middleware.trace_event",
        "spectra.api.middleware.trace_short_circuited",
    ]:
        require(name in lib, f"{name} missing from spectra-api host table")
        require(name in runtime, f"{name} missing from runtime host-call contract")

    for term in [
        '"middleware", "chain"',
        '"middleware", "execute_sync"',
        '"middleware", "execute_async"',
        '"middleware", "trace_event"',
        "MiddlewareChain",
        "MiddlewareHandle",
        "AsyncMiddlewareHandle",
        "MiddlewareTrace",
    ]:
        require(term in midend, f"midend lowering missing {term}")

    for term in [
        "std.api.middleware",
        "std.api.middleware.MiddlewareChain",
        "std.api.middleware.MiddlewareHandle",
        "std.api.middleware.AsyncMiddlewareHandle",
        "std.api.middleware.MiddlewareTrace",
        "std.api.middleware.execute_sync",
        "std.api.middleware.trace_short_circuited",
        "AsyncMiddleware",
    ]:
        require(term in builtins, f"builtin middleware surface missing {term}")
    require("std.api.middleware" in semantic, "semantic namespace seed missing std.api.middleware")
    require(
        f"assert_eq!(HOST_CALLS.len(), {PACKAGE_HOST_CALL_COUNT})" in lib,
        f"package host-call count must be {PACKAGE_HOST_CALL_COUNT}",
    )
    require(
        f"assert_eq!(required_host_call_count(), {RUNTIME_REQUIRED_HOST_CALL_COUNT})" in runtime,
        f"runtime required host-call count must be {RUNTIME_REQUIRED_HOST_CALL_COUNT}",
    )


def validate_fixture_and_docs() -> None:
    fixture = read("tests/validation/148_api_middleware_chain.spectra")
    for term in [
        "impl Middleware for AuditMiddleware",
        "impl AsyncMiddleware for AsyncAuditMiddleware",
        "register_sync_short_circuit",
        "execute_sync",
        "execute_async",
        "trace_short_circuited",
        "first:request",
        "second:response",
        "limit:response",
        "async:response",
    ]:
        require(term in fixture, f"fixture missing {term}")

    docs = read("docs/api/std-api-middleware.md")
    book = read("docs/book/10-middleware-chain.md")
    for term in [
        "Request hooks run in append order",
        "Response hooks run in reverse order",
        "short-circuits",
        "execute_async",
        "tests/validation/148_api_middleware_chain.spectra",
    ]:
        require(term in docs, f"middleware reference missing {term}")
        require(term in book, f"middleware book chapter missing {term}")

    api_index = read("docs/api/README.md")
    book_index = read("docs/book/README.md")
    require("std-api-middleware.md" in api_index, "API index must link middleware reference")
    require("10-middleware-chain.md" in book_index, "book index must link middleware chapter")

    snapshot = read("compiler/tests/snapshots/std_api_public_function_table.snap")
    for term in [
        'module std.api.middleware',
        "type std.api.middleware.MiddlewareChain",
        "func std.api.middleware.execute_sync",
        "func std.api.middleware.trace_event",
    ]:
        require(term in snapshot, f"std.api snapshot missing {term}")


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2301 = items.get("R-2301")
    require(r2301 is not None, "R-2301 missing from roadmap")
    require(r2301.get("status") == "complete", "R-2301 must be complete")
    require(r2301.get("owner") == "web", "R-2301 owner must remain web")
    require(r2301.get("dependencies") == ["R-2215"], "R-2301 dependencies changed")
    acceptance = "\n".join(r2301.get("acceptance", []))
    for term in [
        "async func",
        "synchronous middleware",
        "book chapter",
        "reverse order",
        "148_api_middleware_chain.spectra",
        "scripts/validate_r2301_middleware_chain.py",
    ]:
        require(term in acceptance, f"R-2301 acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2301 Middleware Chain Trait and Deterministic Ordering", 1)[
        1
    ].split("## R-2302", 1)[0]
    for term in [
        "Status: `complete`",
        "packages/spectra-api/src/middleware.rs",
        "std.api.middleware",
        "docs/book/10-middleware-chain.md",
        "148_api_middleware_chain.spectra",
        "validate_r2301_middleware_chain.py",
    ]:
        require(term in block, f"backlog R-2301 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2301` middleware chain trait and deterministic ordering (complete;" in plan,
        "implementation plan must mark R-2301 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2301_middleware_chain.py" in runner, "run_tests.ps1 must run R-2301")
    require(
        'Teste = "validate_r2301_middleware_chain"' in runner,
        "run_tests.ps1 must record R-2301",
    )


def validate_commands() -> None:
    binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    cargo = cargo_command()
    run_command([cargo, "test", "-q", "-p", "spectra-api", "middleware", "--offline"])
    run_command([cargo, "test", "-q", "-p", "spectra-compiler", "--offline"])
    run_command([cargo, "test", "-q", "-p", "spectra-midend", "--offline"])
    run_command([cargo, "build", "-q", "-p", "spectra-cli", "--offline"])
    run_command([str(binary), "compile", "tests/validation/148_api_middleware_chain.spectra"])
    run_command([str(binary), "run", "tests/validation/148_api_middleware_chain.spectra"])


def main() -> None:
    validate_implementation()
    validate_fixture_and_docs()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2301 middleware chain and deterministic ordering")


if __name__ == "__main__":
    main()
