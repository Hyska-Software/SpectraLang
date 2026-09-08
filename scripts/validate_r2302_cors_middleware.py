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
    print(f"R-2302 validation failed: {message}", file=sys.stderr)
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
    cors = read("packages/spectra-api/src/cors.rs")
    for term in [
        "pub struct CorsPolicy",
        "pub fn permissive",
        "pub fn allow_origin",
        "pub fn allow_method",
        "pub fn allow_header",
        "pub fn expose_header",
        "pub fn allow_credentials",
        "pub fn max_age",
        "preflight_response",
        "apply_actual_response",
        "Access-Control-Allow-Origin",
        "Access-Control-Allow-Methods",
        "Access-Control-Allow-Headers",
        "Access-Control-Allow-Credentials",
        "Access-Control-Expose-Headers",
        "Access-Control-Max-Age",
        "CorsMiddleware",
        "restrictive_preflight_emits_configured_cors_headers",
        "credentialed_policy_echoes_origin_and_varies",
        "denied_origin_leaves_actual_response_unmodified_and_preflight_forbidden",
    ]:
        require(term in cors, f"cors.rs missing {term}")

    http = read("packages/spectra-api/src/http.rs")
    require("request_with_header" in http, "std.api.http must expose request_with_header")

    middleware = read("packages/spectra-api/src/middleware.rs")
    for term in [
        "register_sync_middleware",
        "cors_origin",
        "set_cors_origin",
        "MiddlewareEntry::Sync",
    ]:
        require(term in middleware, f"middleware integration missing {term}")

    lib = read("packages/spectra-api/src/lib.rs")
    runtime = read("runtime/src/api/mod.rs")
    for name in [
        "spectra.api.http.request_with_header",
        "spectra.api.cors.policy",
        "spectra.api.cors.permissive",
        "spectra.api.cors.allow_origin",
        "spectra.api.cors.allow_method",
        "spectra.api.cors.allow_header",
        "spectra.api.cors.expose_header",
        "spectra.api.cors.allow_credentials",
        "spectra.api.cors.max_age",
        "spectra.api.cors.middleware",
        "spectra.api.cors.is_preflight",
        "spectra.api.cors.preflight",
        "spectra.api.cors.apply",
        "spectra.api.cors.allowed_origin",
    ]:
        require(name in lib, f"{name} missing from spectra-api host table")
        require(name in runtime, f"{name} missing from runtime host-call contract")
    require(
        f"assert_eq!(HOST_CALLS.len(), {PACKAGE_HOST_CALL_COUNT})" in lib,
        f"package host-call count must be {PACKAGE_HOST_CALL_COUNT}",
    )
    require(
        f"assert_eq!(required_host_call_count(), {RUNTIME_REQUIRED_HOST_CALL_COUNT})" in runtime,
        f"runtime required host-call count must be {RUNTIME_REQUIRED_HOST_CALL_COUNT}",
    )


def validate_frontend_and_lowering() -> None:
    builtins = read("compiler/src/semantic/builtin_modules.rs")
    semantic = read("compiler/src/semantic/mod.rs")
    midend = read("midend/src/lowering.rs")
    snapshot = read("compiler/tests/snapshots/std_api_public_function_table.snap")
    for term in [
        "std.api.cors",
        "std.api.cors.CorsPolicy",
        "std.api.cors.permissive",
        "std.api.cors.allow_credentials",
        "std.api.cors.middleware",
        "std.api.http.request_with_header",
    ]:
        require(term in builtins, f"builtin surface missing {term}")
        require(term in snapshot, f"snapshot missing {term}")
    require("std.api.cors" in semantic, "semantic namespace seed missing std.api.cors")
    for term in [
        '"cors", "policy"',
        '"cors", "permissive"',
        '"cors", "middleware"',
        '"cors", "is_preflight"',
        '"http", "request_with_header"',
        "CorsPolicy",
    ]:
        require(term in midend, f"midend lowering missing {term}")


def validate_fixture_and_docs() -> None:
    fixture = read("tests/validation/149_api_cors_middleware.spectra")
    for term in [
        "permissive",
        "allow_origin",
        "allow_method",
        "allow_header",
        "allow_credentials",
        "expose_header",
        "preflight",
        "apply",
        "middleware(configured)",
        "Access-Control-Allow-Origin",
        "Access-Control-Allow-Credentials",
        "trace_short_circuited",
    ]:
        require(term in fixture, f"CORS fixture missing {term}")

    docs = read("docs/api/std-api-cors.md")
    for term in [
        "std.api.cors",
        "preflight",
        "permissive",
        "restrictive",
        "credentials",
        "Access-Control-Allow-Origin",
        "tests/validation/149_api_cors_middleware.spectra",
        "scripts/validate_r2302_cors_middleware.py",
    ]:
        require(term in docs, f"CORS reference missing {term}")
    api_index = read("docs/api/README.md")
    require("std-api-cors.md" in api_index, "API index must link CORS reference")

    http_docs = read("docs/api/std-api-http-types.md")
    require("request_with_header" in http_docs, "HTTP docs must cover request_with_header")


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2302 = items.get("R-2302")
    require(r2302 is not None, "R-2302 missing from roadmap")
    require(r2302.get("status") == "complete", "R-2302 must be complete")
    require(r2302.get("owner") == "web", "R-2302 owner must remain web")
    require(r2302.get("dependencies") == ["R-2301"], "R-2302 dependencies changed")
    acceptance = "\n".join(r2302.get("acceptance", []))
    for term in [
        "Preflight",
        "Access-Control",
        "credentials",
        "permissive",
        "restrictive",
        "149_api_cors_middleware.spectra",
        "scripts/validate_r2302_cors_middleware.py",
    ]:
        require(term in acceptance, f"R-2302 acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2302 CORS Middleware (RFC 7231)", 1)[1].split(
        "## R-2303", 1
    )[0]
    for term in [
        "Status: `complete`",
        "packages/spectra-api/src/cors.rs",
        "std.api.cors",
        "request_with_header",
        "149_api_cors_middleware.spectra",
        "validate_r2302_cors_middleware.py",
    ]:
        require(term in block, f"backlog R-2302 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2302` CORS middleware (complete;" in plan,
        "implementation plan must mark R-2302 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2302_cors_middleware.py" in runner, "run_tests.ps1 must run R-2302")
    require(
        'Teste = "validate_r2302_cors_middleware"' in runner,
        "run_tests.ps1 must record R-2302",
    )


def validate_commands() -> None:
    binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    cargo = cargo_command()
    run_command([cargo, "test", "-q", "-p", "spectra-api", "cors", "--offline"])
    run_command([cargo, "test", "-q", "-p", "spectra-api", "middleware", "--offline"])
    run_command(
        [
            cargo,
            "test",
            "-q",
            "-p",
            "spectra-compiler",
            "std_api_public_function_table_is_snapshotted",
            "--offline",
        ]
    )
    run_command([cargo, "test", "-q", "-p", "spectra-midend", "--offline"])
    run_command([cargo, "build", "-q", "-p", "spectra-cli", "--offline"])
    run_command([str(binary), "compile", "tests/validation/149_api_cors_middleware.spectra"])
    run_command([str(binary), "run", "tests/validation/149_api_cors_middleware.spectra"])


def main() -> None:
    validate_implementation()
    validate_frontend_and_lowering()
    validate_fixture_and_docs()
    validate_planning()
    validate_runner()
    validate_commands()
    print("validated R-2302 CORS middleware")


if __name__ == "__main__":
    main()
