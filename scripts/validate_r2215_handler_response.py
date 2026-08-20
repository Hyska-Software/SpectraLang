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
    print(f"R-2215 validation failed: {message}", file=sys.stderr)
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
    handler = read("packages/spectra-api/src/handler.rs")
    for term in [
        "pub trait IntoResponse",
        "pub trait Handler",
        "pub trait AsyncHandler",
        "pub struct HandlerError",
        "impl IntoResponse for Response",
        "impl IntoResponse for String",
        "impl IntoResponse for Vec<u8>",
        "impl<T> IntoResponse for Result<T, HandlerError>",
        "sync_handler_accepts_any_into_response_return",
        "async_handler_accepts_any_into_response_return",
        "host_registration_dispatches_sync_and_async_handlers",
    ]:
        require(term in handler, f"handler.rs missing {term}")

    lib = read("packages/spectra-api/src/lib.rs")
    runtime = read("runtime/src/api/mod.rs")
    midend = read("midend/src/lowering.rs")
    builtins = read("compiler/src/semantic/builtin_modules.rs")
    semantic = read("compiler/src/semantic/mod.rs")
    registry = read("compiler/src/semantic/module_registry.rs")
    for name in [
        "spectra.api.handler.text",
        "spectra.api.handler.json",
        "spectra.api.handler.bytes",
        "spectra.api.handler.status",
        "spectra.api.handler.with_header",
        "spectra.api.handler.into_response",
        "spectra.api.handler.into_text_response",
        "spectra.api.handler.into_status_response",
        "spectra.api.handler.error",
        "spectra.api.handler.error_response",
        "spectra.api.handler.error_code",
        "spectra.api.handler.error_message",
        "spectra.api.handler.last_error_message",
        "spectra.api.handler.register_sync",
        "spectra.api.handler.register_async",
        "spectra.api.handler.register_sync_callback",
        "spectra.api.handler.register_async_callback",
        "spectra.api.handler.dispatch_sync",
        "spectra.api.handler.dispatch_async",
    ]:
        require(name in lib, f"{name} missing from host-call table")
        require(name in runtime, f"{name} missing from runtime contract")
    for term in [
        '"handler", "text"',
        '"handler", "register_sync_callback"',
        '"handler", "register_async_callback"',
        '"handler", "dispatch_sync"',
        '"handler", "dispatch_async"',
        "lower_named_function_value",
        "build_escape_manual_alloc",
        "HandlerHandle",
        "AsyncHandlerHandle",
        "HandlerError",
    ]:
        require(term in midend, f"midend missing {term}")
    for term in [
        "std.api.handler",
        "std.api.handler.HandlerHandle",
        "std.api.handler.AsyncHandlerHandle",
        "std.api.handler.HandlerError",
        "std.api.handler.dispatch_sync",
        "IntoResponse",
        "AsyncHandler",
    ]:
        require(term in builtins, f"builtin surface missing {term}")
    require("ExportedTrait" in registry, 'module registry must export traits')
    require("import_exported_trait" in semantic, "semantic imports must register exported traits")
    require("std.api.handler" in semantic, "semantic namespace seed missing std.api.handler")


def validate_fixture_and_docs() -> None:
    fixture = read("tests/validation/139_api_handler_response_return.spectra")
    callbacks = read("tests/validation/330_api_handler_callbacks.spectra")
    for term in [
        "IntoResponse",
        "impl IntoResponse for TextValue",
        "impl Handler for SyncUserHandler",
        "impl AsyncHandler for AsyncUserHandler",
        "await async_handler.call(request)",
        "register_sync",
        "register_async",
        "dispatch_sync",
        "dispatch_async",
        "error_response",
    ]:
        require(term in fixture, f"fixture missing {term}")
    for term in [
        "register_sync_callback",
        "register_async_callback",
        "dispatch_sync",
        "dispatch_async",
        "async func async_callback",
        "Request",
        "Task<Response>",
    ]:
        require(term in callbacks, f"callback fixture missing {term}")

    docs = read("docs/api/std-api-handler.md")
    for term in [
        "std.api.handler",
        "IntoResponse",
        "Handler",
        "AsyncHandler",
        "HandlerError",
        "register_sync",
        "register_sync_callback",
        "register_async_callback",
        "dispatch_async",
        "tests/validation/139_api_handler_response_return.spectra",
        "tests/validation/330_api_handler_callbacks.spectra",
    ]:
        require(term in docs, f"handler docs missing {term}")

    snapshot = read("compiler/tests/snapshots/std_api_public_function_table.snap")
    for term in [
        'module std.api.handler',
        "type std.api.handler.HandlerHandle",
        "func std.api.handler.text",
        "func std.api.handler.dispatch_async",
    ]:
        require(term in snapshot, f"std.api snapshot missing {term}")


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2215 = items.get("R-2215")
    require(r2215 is not None, "R-2215 missing from roadmap")
    require(r2215.get("status") == "complete", "R-2215 must be marked complete")
    require(r2215.get("owner") == "web", "R-2215 owner must remain web")
    require(r2215.get("dependencies") == ["R-2210"], "R-2215 dependencies changed")
    acceptance = "\n".join(r2215.get("acceptance", []))
    for term in [
        "async func",
        "synchronous handlers",
        "IntoResponse",
        "HandlerError",
        "unified error middleware",
        "139_api_handler_response_return.spectra",
        "330_api_handler_callbacks.spectra",
        "scripts/validate_r2215_handler_response.py",
    ]:
        require(term in acceptance, f"R-2215 acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2215 Handler Trait and Response Return", 1)[1].split(
        "## R-2216", 1
    )[0]
    for term in [
        "Status: `complete`",
        "packages/spectra-api/src/handler.rs",
        "std.api.handler",
        "IntoResponse",
        "register_sync_callback",
        "register_async_callback",
        "139_api_handler_response_return.spectra",
        "330_api_handler_callbacks.spectra",
        "validate_r2215_handler_response.py",
    ]:
        require(term in block, f"backlog R-2215 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2215` `api.handler` trait and response return (complete;" in plan,
        "implementation plan must mark R-2215 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2215_handler_response.py" in runner, "run_tests.ps1 must run R-2215")
    require(
        'Teste = "validate_r2215_handler_response"' in runner,
        "run_tests.ps1 must record R-2215",
    )


def validate_commands() -> None:
    binary = ROOT / "target" / "debug" / ("spectralang.exe" if sys.platform.startswith("win") else "spectralang")
    run_command(["cargo", "test", "-q", "-p", "spectra-api", "handler", "--offline"])
    run_command(["cargo", "test", "-q", "-p", "spectra-compiler", "--offline"])
    run_command(["cargo", "test", "-q", "-p", "spectra-midend", "--offline"])
    run_command(["cargo", "build", "-q", "-p", "spectra-cli", "--offline"])
    run_command([str(binary), "compile", "tests/validation/139_api_handler_response_return.spectra"])
    run_command([str(binary), "run", "tests/validation/139_api_handler_response_return.spectra"])


def main() -> None:
    validate_implementation()
    validate_fixture_and_docs()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2215 handler trait and response return")


if __name__ == "__main__":
    main()
