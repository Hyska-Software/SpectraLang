#!/usr/bin/env python3
"""Validate the R-2311 server-side session implementation and language fixture."""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE_HOST_CALL_COUNT = 557
SESSION_CALLS = [
    "memory_store",
    "redis_store",
    "store_kind",
    "create",
    "lookup",
    "id",
    "value",
    "created_at_ms",
    "expires_at_ms",
    "is_valid",
    "revoke",
    "error_code",
    "error_message",
]


def fail(message: str) -> None:
    print(f"R-2311 validation failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


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


def validate_native_surface() -> None:
    session = read("packages/spectra-api/src/session.rs")
    for term in [
        "trait SessionBackend",
        "MemorySessionBackend",
        "RedisSessionBackend",
        "RedisConnection",
        "set_blocking",
        "SystemRandom",
        "sliding",
        "max_expires_at_ms",
        "pub extern \"C\" fn memory_store",
        "pub extern \"C\" fn redis_store",
        "pub extern \"C\" fn lookup",
        "pub extern \"C\" fn revoke",
        "sliding_expiration_extends_activity_but_respects_maximum_lifetime",
        "revoke_invalidates_an_active_session_immediately",
    ]:
        require(term in session, f"session implementation missing {term}")

    handles = read("runtime/src/handles/mod.rs")
    require("ApiSessionStore = 79" in handles, "session-store handle kind is missing")
    require("ApiSession = 80" in handles, "session handle kind is missing")

    host_registry = read("packages/spectra-api/src/host_calls.rs")
    package_names = re.findall(r'name:\s*"([^"]+)"', host_registry)
    require(len(package_names) == PACKAGE_HOST_CALL_COUNT, "package host-call count drifted")
    for name in SESSION_CALLS:
        require(f'"spectra.api.session.{name}"' in host_registry, f"missing session host call {name}")



def validate_language_surface() -> None:
    core = read("compiler/src/semantic/builtin_api_core.rs")
    services = read("compiler/src/semantic/builtin_api_services.rs")
    contract = read("compiler/src/semantic/builtin_contract.rs")
    lowering = read("midend/src/lowering_std_api.rs")
    handles = read("midend/src/lowering_handles.rs")
    for term in [
        'format!("{prefix}.session")',
        "make_std_api_session",
        '"SessionStore"',
        '"Session"',
        '"std.api.session"',
        '"std.api.session.create"',
    ]:
        require(term in core + services + contract, f"semantic contract missing {term}")
    for name in SESSION_CALLS:
        require(f'"session", "{name}"' in lowering, f"IR lowering misses session.{name}")
    require('module == "session"' in handles, "handle lowering misses std.api.session")
    require('"SessionStore"' in handles and '"Session"' in handles, "session handle types are not lowered")


def validate_docs_and_catalog() -> None:
    docs = read("docs/api/std-api-session.md")
    for term in [
        "memory_store()",
        "redis_store(redis_connection, prefix)",
        "sliding",
        "max_lifetime_ms",
        "revoke(store, id)",
        "342_api_session.spectra",
        "R-2507",
    ]:
        require(term in docs, f"session docs missing {term}")
    require("std-api-session.md" in read("docs/api/README.md"), "API README misses session docs")

    catalog = tomllib.loads(read("packages/spectra-contract/catalog/stdlib.toml"))
    entries = {entry["path"]: entry for entry in catalog["entry"]}
    require("std.api.session" in entries, "session module is absent from catalog")
    for name in SESSION_CALLS:
        path = f"std.api.session.{name}"
        require(path in entries, f"catalog misses {path}")
        require(entries[path]["maturity"] == "stable", f"catalog does not mark {path} stable")
    require(entries["std.api.session.Session"]["maturity"] == "stable", "Session type is not stable")


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2311")
    require(item is not None and item.get("status") == "complete", "R-2311 is not complete")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in ["pluggable", "sliding", "invalidation", "342_api_session.spectra", "validate_r2311_session.py"]:
        require(term in acceptance, f"R-2311 acceptance misses {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2311 Session Management", 1)[1].split("## R-2312", 1)[0]
    require("Status: `complete`" in block, "backlog does not mark R-2311 complete")
    require("validate_r2311_session.py" in block, "backlog misses R-2311 validator")
    require("R-2311` Sessions (complete;" in read("docs/production-ai-implementation-plan.md"), "strategic plan misses R-2311 completion")
    runner = read("run_tests.ps1")
    require("validate_r2311_session.py" in runner, "runner does not invoke R-2311")


def main() -> None:
    binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    validate_native_surface()
    validate_language_surface()
    validate_docs_and_catalog()
    run_command([cargo_cmd(), "test", "-q", "-p", "spectra-api", "--lib", "session", "--offline"])
    run_command([cargo_cmd(), "test", "-q", "-p", "spectra-compiler", "--test", "snapshot_tests", "--offline"])
    run_command([cargo_cmd(), "build", "-q", "-p", "spectra-cli", "--offline"])
    run_command([str(binary), "compile", "tests/validation/342_api_session.spectra"])
    run_command([str(binary), "run", "tests/validation/342_api_session.spectra"])
    validate_planning()
    print("validated R-2311 server-side sessions")


if __name__ == "__main__":
    main()
