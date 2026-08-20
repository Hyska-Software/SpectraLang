#!/usr/bin/env python3
"""Validate the R-2401 RFC 6455 WebSocket server surface and evidence."""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGE_HOST_CALL_COUNT = 414
RUNTIME_REQUIRED_HOST_CALL_COUNT = 346
WEBSOCKET_CALLS = [
    "server_new",
    "server_listen",
    "server_local_port",
    "server_set_per_message_deflate",
    "server_set_max_message_bytes",
    "server_accept",
    "connection_peer_port",
    "connection_receive",
    "connection_send_text",
    "connection_send_binary_base64",
    "connection_ping",
    "connection_close",
    "message_kind",
    "message_len",
    "message_text",
    "message_base64",
    "message_release",
]


def fail(message: str) -> None:
    print(f"R-2401 validation failed: {message}", file=sys.stderr)
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
    websocket = read("packages/spectra-api/src/websocket.rs")
    for term in [
        "FrameRole",
        "WebSocketFrame",
        "WebSocketMessage",
        "Sec-WebSocket-Accept",
        "permessage-deflate",
        "fragmented_message",
        "ping_is_answered_with_pong_and_close_is_validated",
        "per_message_deflate_is_negotiated_and_round_trips",
        "pub extern \"C\" fn server_accept",
        "pub extern \"C\" fn connection_receive",
        "pub extern \"C\" fn message_release",
    ]:
        require(term in websocket, f"websocket implementation missing {term}")

    handles = read("runtime/src/handles/mod.rs")
    for term in ["ApiWebSocketServer = 81", "ApiWebSocket = 82", "ApiWebSocketMessage = 83"]:
        require(term in handles, f"WebSocket handle kind is missing: {term}")

    host_registry = read("packages/spectra-api/src/host_calls.rs")
    package_names = re.findall(r'name:\s*"([^"]+)"', host_registry)
    require(len(package_names) == PACKAGE_HOST_CALL_COUNT, "package host-call count drifted")
    for name in WEBSOCKET_CALLS:
        require(f'"spectra.api.websocket.{name}"' in host_registry, f"missing WebSocket host call {name}")

    runtime_api = read("runtime/src/api/mod.rs")
    required_block = runtime_api.split("pub const REQUIRED_HOST_CALLS: &[&str] = &[", 1)[1].split("];", 1)[0]
    runtime_names = re.findall(r'"(spectra\.api\.[^"]+)"', required_block)
    require(len(runtime_names) == RUNTIME_REQUIRED_HOST_CALL_COUNT, "runtime host-call count drifted")
    for name in WEBSOCKET_CALLS:
        require(f'"spectra.api.websocket.{name}"' in runtime_api, f"runtime contract misses WebSocket host call {name}")


def validate_language_surface() -> None:
    core = read("compiler/src/semantic/builtin_api_core.rs")
    services = read("compiler/src/semantic/builtin_api_services.rs")
    contract = read("compiler/src/semantic/builtin_contract.rs")
    lowering = read("midend/src/lowering_std_api.rs")
    handles = read("midend/src/lowering_handles.rs")
    for term in [
        'format!("{prefix}.websocket")',
        "make_std_api_websocket",
        '"WebSocketServer"',
        '"WebSocket"',
        '"WebSocketMessage"',
        '"std.api.websocket"',
        '"std.api.websocket.server_accept"',
    ]:
        require(term in core + services + contract, f"semantic contract missing {term}")
    for name in WEBSOCKET_CALLS:
        require(f'"websocket", "{name}"' in lowering, f"IR lowering misses websocket.{name}")
    require('module == "websocket"' in handles, "handle lowering misses std.api.websocket")
    for name in ["WebSocketServer", "WebSocket", "WebSocketMessage"]:
        require(f'"{name}"' in handles, f"{name} handle type is not lowered")


def validate_docs_and_catalog() -> None:
    docs = read("docs/api/std-api-websocket.md")
    for term in [
        "RFC 6455",
        "Sec-WebSocket-Accept",
        "permessage-deflate",
        "fragment",
        "10 mil conexões",
        "343_api_websocket.spectra",
        "validate_r2401_websocket.py",
    ]:
        require(term in docs, f"WebSocket docs missing {term}")
    require("std-api-websocket.md" in read("docs/api/README.md"), "API README misses WebSocket docs")

    catalog = tomllib.loads(read("packages/spectra-contract/catalog/stdlib.toml"))
    entries = {entry["path"]: entry for entry in catalog["entry"]}
    require("std.api.websocket" in entries, "WebSocket module is absent from catalog")
    for name in WEBSOCKET_CALLS:
        path = f"std.api.websocket.{name}"
        require(path in entries, f"catalog misses {path}")
        require(entries[path]["maturity"] == "stable", f"catalog does not mark {path} stable")
    for name in ["WebSocketServer", "WebSocket", "WebSocketMessage"]:
        path = f"std.api.websocket.{name}"
        require(path in entries, f"catalog misses {path}")
        require(entries[path]["maturity"] == "stable", f"catalog does not mark {path} stable")


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2401")
    require(item is not None, "R-2401 missing from roadmap")
    require(item.get("status") == "in_progress", "R-2401 must remain in_progress before soak/integration")
    require(item.get("owner") == "web", "R-2401 owner changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in ["RFC 6455", "per-message deflate", "10k concurrent connections soak"]:
        require(term in acceptance, f"roadmap acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    require("## R-2401 WebSocket Server" in backlog, "backlog is missing R-2401")
    block = backlog.split("## R-2401 WebSocket Server", 1)[1].split("## R-2402", 1)[0]
    for term in [
        "Status: `in_progress`",
        "RFC 6455",
        "343_api_websocket.spectra",
        "validate_r2401_websocket.py",
        "10k",
    ]:
        require(term in block, f"backlog R-2401 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require("R-2401` WebSocket server" in plan, "strategic plan misses R-2401")

    runner = read("run_tests.ps1")
    require("validate_r2401_websocket.py" in runner, "runner does not invoke R-2401")


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
            "websocket",
            "--offline",
            "--",
            "--test-threads=1",
        ]
    )
    run_command([cargo_cmd(), "test", "-q", "-p", "spectra-compiler", "--test", "snapshot_tests", "--offline"])
    run_command([cargo_cmd(), "build", "-q", "-p", "spectra-cli", "--bin", "spectralang", "--offline"])
    run_command([str(binary), "check", "tests/validation/343_api_websocket.spectra"])
    run_command([str(binary), "run", "tests/validation/343_api_websocket.spectra"])
    validate_planning()
    print("validated R-2401 WebSocket server")


if __name__ == "__main__":
    main()
