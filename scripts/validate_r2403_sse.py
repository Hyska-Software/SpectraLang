#!/usr/bin/env python3
"""Validate the R-2403 Server-Sent Events surface and local evidence."""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE_HOST_CALL_COUNT = 545
RUNTIME_REQUIRED_HOST_CALL_COUNT = 439
SSE_CALLS = [
    "server_new",
    "server_response",
    "server_listen",
    "server_local_port",
    "server_set_heartbeat_interval",
    "server_set_replay_capacity",
    "server_set_max_event_bytes",
    "server_accept",
    "server_publish",
    "event_new",
    "event_id",
    "event_type",
    "event_data",
    "event_retry_ms",
    "event_release",
    "connection_peer_port",
    "connection_last_event_id",
    "connection_send",
    "connection_heartbeat",
    "connection_close",
]


def fail(message: str) -> None:
    print(f"R-2403 validation failed: {message}", file=sys.stderr)
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
    sse = read("packages/spectra-api/src/sse.rs")
    server_tests = read("packages/spectra-api/src/server_tests.rs")
    for term in [
        "pub struct SseEvent",
        "pub struct SseServer",
        "Last-Event-ID",
        "text/event-stream",
        "send_heartbeat",
        "replay_after",
        "routed_connections",
        "routed_response_for_handle",
        "event_serialization_preserves_multiline_data_and_retry_hint",
        "streams_events_and_automatic_heartbeats",
        "last_event_id_replays_only_new_identified_events",
        'pub extern "C" fn server_publish',
        'pub extern "C" fn server_response',
        'pub extern "C" fn connection_send',
    ]:
        require(term in sse, f"SSE implementation missing {term}")
    require(
        "r2403_routed_sse_response_streams_through_http_server_loop" in server_tests,
        "routed SSE integration test is missing",
    )

    handles = read("runtime/src/handles/mod.rs")
    for term in ["ApiSseServer = 85", "ApiSseConnection = 86", "ApiSseEvent = 87"]:
        require(term in handles, f"SSE handle kind is missing: {term}")

    host_registry = read("packages/spectra-api/src/host_calls.rs")
    package_names = re.findall(r'name:\s*"([^"]+)"', host_registry)
    require(len(package_names) == PACKAGE_HOST_CALL_COUNT, "package host-call count drifted")
    for name in SSE_CALLS:
        require(f'"spectra.api.sse.{name}"' in host_registry, f"missing SSE host call {name}")

    runtime_api = read("runtime/src/api/mod.rs")
    required_block = runtime_api.split("pub const REQUIRED_HOST_CALLS: &[&str] = &[", 1)[1].split("];", 1)[0]
    runtime_names = re.findall(r'"(spectra\.api\.[^"]+)"', required_block)
    require(len(runtime_names) == RUNTIME_REQUIRED_HOST_CALL_COUNT, "runtime host-call count drifted")
    for name in SSE_CALLS:
        require(f'"spectra.api.sse.{name}"' in runtime_api, f"runtime contract misses SSE host call {name}")


def validate_language_surface() -> None:
    core = read("compiler/src/semantic/builtin_api_core.rs")
    services = read("compiler/src/semantic/builtin_api_services.rs")
    contract = read("compiler/src/semantic/builtin_contract.rs")
    lowering = read("midend/src/lowering_std_api.rs")
    handles = read("midend/src/lowering_handles.rs")
    for term in [
        'format!("{prefix}.sse")',
        "make_std_api_sse",
        '"SseServer"',
        '"SseConnection"',
        '"SseEvent"',
        '"std.api.sse"',
        '"std.api.sse.server_response"',
        '"std.api.sse.server_accept"',
    ]:
        require(term in core + services + contract, f"semantic contract missing {term}")
    for name in SSE_CALLS:
        require(f'"sse", "{name}"' in lowering, f"IR lowering misses sse.{name}")
    require('module == "sse"' in handles, "handle lowering misses std.api.sse")
    for name in ["SseServer", "SseConnection", "SseEvent"]:
        require(f'"{name}"' in handles, f"{name} handle type is not lowered")


def validate_docs_and_catalog() -> None:
    docs = read("docs/api/std-api-sse.md")
    for term in [
        "text/event-stream",
        "Last-Event-ID",
        "heartbeat",
        "retry_ms",
        "server_response",
        "345_api_sse.spectra",
        "validate_r2403_sse.py",
    ]:
        require(term in docs, f"SSE docs missing {term}")
    require("std-api-sse.md" in read("docs/api/README.md"), "API README misses SSE docs")

    catalog = tomllib.loads(read("packages/spectra-contract/catalog/stdlib.toml"))
    entries = {entry["path"]: entry for entry in catalog["entry"]}
    require("std.api.sse" in entries, "SSE module is absent from catalog")
    for name in SSE_CALLS:
        path = f"std.api.sse.{name}"
        require(path in entries, f"catalog misses {path}")
        require(entries[path]["maturity"] == "stable", f"catalog does not mark {path} stable")
    for name in ["SseServer", "SseConnection", "SseEvent"]:
        path = f"std.api.sse.{name}"
        require(path in entries, f"catalog misses {path}")
        require(entries[path]["maturity"] == "stable", f"catalog does not mark {path} stable")


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2403")
    require(item is not None, "R-2403 missing from roadmap")
    require(item.get("status") == "complete", "R-2403 must be complete after route integration")
    require(item.get("owner") == "web", "R-2403 owner changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in ["SSE response", "heartbeats", "Last-Event-ID", "streaming", "server_response"]:
        require(term in acceptance, f"roadmap acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    require("## R-2403 Server-Sent Events (SSE)" in backlog, "backlog is missing R-2403")
    block = backlog.split("## R-2403 Server-Sent Events (SSE)", 1)[1].split("## R-2404", 1)[0]
    for term in ["Status: `complete`", "345_api_sse.spectra", "Last-Event-ID", "heartbeat", "server_response"]:
        require(term in block, f"backlog R-2403 missing {term}")
    require("R-2403" in read("docs/production-ai-implementation-plan.md"), "strategic plan misses R-2403")
    require("validate_r2403_sse.py" in read("run_tests.ps1"), "runner does not invoke R-2403")


def main() -> None:
    binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    validate_native_surface()
    validate_language_surface()
    validate_docs_and_catalog()
    run_command(
        [
            cargo_cmd(),
            "test",
            "-q",
            "-p",
            "spectra-api",
            "--lib",
            "sse",
            "--offline",
            "--",
            "--test-threads=1",
        ]
    )
    run_command(
        [
            cargo_cmd(),
            "test",
            "-q",
            "-p",
            "spectra-api",
            "--lib",
            "r2403_routed_sse_response_streams_through_http_server_loop",
            "--offline",
            "--",
            "--test-threads=1",
        ]
    )
    run_command([cargo_cmd(), "test", "-q", "-p", "spectra-compiler", "--test", "snapshot_tests", "--offline"])
    run_command([cargo_cmd(), "build", "-q", "-p", "spectra-cli", "--bin", "spectralang", "--offline"])
    run_command([str(binary), "check", "tests/validation/345_api_sse.spectra"])
    run_command([str(binary), "run", "tests/validation/345_api_sse.spectra"])
    validate_planning()
    print("validated R-2403 SSE")


if __name__ == "__main__":
    main()
