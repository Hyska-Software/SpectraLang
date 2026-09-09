#!/usr/bin/env python3
"""Validate the R-2317 authenticated REST/JWT example end to end."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE_HOST_CALL_COUNT = 555
RUNTIME_REQUIRED_HOST_CALL_COUNT = 439


def fail(message: str) -> None:
    print(f"R-2317 validation failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def parse_toml(path: str):
    with (ROOT / path).open("rb") as handle:
        return tomllib.load(handle)


def cargo_cmd() -> str:
    configured = os.environ.get("CARGO")
    if configured:
        return configured
    return shutil.which("cargo") or str(Path.home() / ".cargo" / "bin" / "cargo.exe")


def run_command(args: list[str], timeout: int = 120) -> str:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=timeout,
        check=False,
    )
    if completed.returncode != 0:
        fail(f"command {' '.join(args)} failed:\n{completed.stdout}")
    return completed.stdout


def validate_body_surface() -> None:
    host_calls = read("packages/spectra-api/src/host_calls.rs")
    runtime_api = read("runtime/src/api/mod.rs")
    http_host = read("packages/spectra-api/src/http_host.rs")
    for name in [
        "spectra.api.http.request_body",
        "spectra.api.http.request_with_body",
    ]:
        require(name in host_calls, f"{name} missing from package host registry")
        require(name in runtime_api, f"{name} missing from runtime contract")
    for term in ["pub extern \"C\" fn request_body", "pub extern \"C\" fn request_with_body"]:
        require(term in http_host, f"HTTP body host implementation missing {term}")
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


def validate_example() -> None:
    example = read("examples/api/02_jwt_auth_crud.spectra")
    for term in [
        "module jwt_auth_crud_api",
        "sign(\"HS256\"",
        "verify(token",
        "token + \"x\"",
        "register_sync_callback",
        "dispatch_sync",
        "request_with_body",
        "request_body(req)",
        "validate_json",
        "result_response",
        "new_error(401",
        "exception_middleware",
        "execute_sync",
        'get(routes, \"/users\")',
        'post(routes, \"/users\")',
        'get(routes, \"/users/{id:\\\\d+}\")',
        'put(routes, \"/users/{id:\\\\d+}\")',
        'delete(routes, \"/users/{id:\\\\d+}\")',
        "tampered.token.value",
        "validation_failure",
        "serve(server, routes)",
    ]:
        require(term in example, f"authenticated CRUD example missing {term}")


def validate_docs_and_catalog() -> None:
    api_index = read("docs/api/README.md")
    require(
        "02_jwt_auth_crud.spectra" in api_index,
        "docs/api/README.md must reference the authenticated CRUD example",
    )
    auth_docs = read("docs/api/std-api-jwt-auth-crud.md")
    for term in [
        "02_jwt_auth_crud.spectra",
        "Bearer",
        "HS256",
        "exception_middleware",
        "result_response",
        "request_body",
        "request_with_body",
    ]:
        require(term in auth_docs, f"JWT CRUD docs missing {term}")
    http_docs = read("docs/api/std-api-http-types.md")
    for term in ["request_body(request)", "request_with_body(request, body)"]:
        require(term in http_docs, f"HTTP body docs missing {term}")

    catalog = read("packages/spectra-contract/catalog/stdlib.toml")
    for term in [
        'path = "std.api.http.request_body"',
        'path = "std.api.http.request_with_body"',
        'fixture = "tests/validation/134_http_core_types.spectra"',
    ]:
        require(term in catalog, f"catalog missing {term}")


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2317")
    require(item is not None, "R-2317 missing from roadmap")
    require(item.get("status") == "complete", "R-2317 is not complete")
    require(item.get("owner") == "ecosystem", "R-2317 owner changed")
    require(item.get("dependencies") == ["R-2308", "R-2219"], "R-2317 dependencies changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in [
        "02_jwt_auth_crud.spectra",
        "issues, validates, and rejects JWTs",
        "unified error middleware",
        "request body",
        "validate_r2317_jwt_auth_crud_example.py",
    ]:
        require(term in acceptance, f"R-2317 roadmap acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2317 API Example: Authenticated REST API (JWT)", 1)[1].split(
        "## R-2318", 1
    )[0]
    for term in [
        "Status: `complete`",
        "02_jwt_auth_crud.spectra",
        "request body",
        "validate_r2317_jwt_auth_crud_example.py",
    ]:
        require(term in block, f"backlog R-2317 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2317` API example: authenticated REST API (JWT) (complete;" in plan,
        "implementation plan must mark R-2317 complete",
    )

    runner = read("run_tests.ps1")
    require("validate_r2317_jwt_auth_crud_example.py" in runner, "runner does not invoke R-2317")
    require(
        'Teste = "validate_r2317_jwt_auth_crud_example"' in runner,
        "runner does not record R-2317",
    )


def main() -> None:
    binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    validate_body_surface()
    validate_example()
    validate_docs_and_catalog()
    run_command([cargo_cmd(), "test", "-q", "-p", "spectra-api", "--lib", "--offline"])
    run_command([cargo_cmd(), "test", "-q", "-p", "spectra-runtime", "--lib", "--offline"])
    run_command([cargo_cmd(), "build", "-q", "-p", "spectra-cli", "--offline"])
    run_command([str(binary), "compile", "examples/api/02_jwt_auth_crud.spectra"])
    run_command([str(binary), "run", "examples/api/02_jwt_auth_crud.spectra"])
    run_command([str(binary), "compile", "tests/validation/134_http_core_types.spectra"])
    run_command([str(binary), "run", "tests/validation/134_http_core_types.spectra"])
    validate_planning()
    print("validated R-2317 authenticated JWT CRUD example")


if __name__ == "__main__":
    main()
