#!/usr/bin/env python3
"""Validate the R-2402 WebSocket client surface and local evidence."""

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
CLIENT_CALLS = [
    "client_new",
    "client_set_per_message_deflate",
    "client_set_max_message_bytes",
    "client_set_reconnect",
    "client_allow_private_networks",
    "client_connect",
]


def fail(message: str) -> None:
    print(f"R-2402 validation failed: {message}", file=sys.stderr)
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
        "pub struct WebSocketClient",
        "parse_websocket_url",
        "Sec-WebSocket-Accept",
        "set_reconnect",
        "allow_private_networks",
        "client_handshake_round_trips_text_and_binary_frames",
        "client_wss_handshake_round_trips_with_explicit_trust_root",
        "client_reconnects_after_a_failed_handshake",
        "pub extern \"C\" fn client_connect",
    ]:
        require(term in websocket, f"WebSocket client implementation missing {term}")

    handles = read("runtime/src/handles/mod.rs")
    require("ApiWebSocketClient = 84" in handles, "WebSocket client handle kind is missing")

    host_registry = read("packages/spectra-api/src/host_calls.rs")
    package_names = re.findall(r'name:\s*"([^"]+)"', host_registry)
    require(len(package_names) == PACKAGE_HOST_CALL_COUNT, "package host-call count drifted")
    for name in CLIENT_CALLS:
        require(f'"spectra.api.websocket.{name}"' in host_registry, f"missing client host call {name}")

    runtime_api = read("runtime/src/api/mod.rs")
    required_block = runtime_api.split("pub const REQUIRED_HOST_CALLS: &[&str] = &[", 1)[1].split("];", 1)[0]
    runtime_names = re.findall(r'"(spectra\.api\.[^"]+)"', required_block)
    require(len(runtime_names) == RUNTIME_REQUIRED_HOST_CALL_COUNT, "runtime host-call count drifted")
    for name in CLIENT_CALLS:
        require(f'"spectra.api.websocket.{name}"' in runtime_api, f"runtime contract misses client host call {name}")


def validate_language_surface() -> None:
    core = read("compiler/src/semantic/builtin_api_core.rs")
    services = read("compiler/src/semantic/builtin_api_services.rs")
    contract = read("compiler/src/semantic/builtin_contract.rs")
    lowering = read("midend/src/lowering_std_api.rs")
    handles = read("midend/src/lowering_handles.rs")
    for term in [
        '"WebSocketClient"',
        '"std.api.websocket.client_connect"',
        "make_std_api_websocket",
    ]:
        require(term in core + services + contract, f"client semantic surface missing {term}")
    for name in CLIENT_CALLS:
        require(f'"websocket", "{name}"' in lowering, f"IR lowering misses websocket.{name}")
    require('module == "websocket"' in handles and '"WebSocketClient"' in handles, "client handle lowering is missing")


def validate_docs_and_catalog() -> None:
    docs = read("docs/api/std-api-websocket.md")
    for term in [
        "WebSocketClient",
        "ws://",
        "Sec-WebSocket-Accept",
        "SSRF",
        "backoff exponencial",
        "344_api_websocket_client.spectra",
        "wss://",
    ]:
        require(term in docs, f"WebSocket client docs missing {term}")

    catalog = tomllib.loads(read("packages/spectra-contract/catalog/stdlib.toml"))
    entries = {entry["path"]: entry for entry in catalog["entry"]}
    require(entries.get("std.api.websocket.WebSocketClient") is not None, "client type is absent from catalog")
    for name in CLIENT_CALLS:
        path = f"std.api.websocket.{name}"
        require(path in entries, f"catalog misses {path}")
        require(entries[path]["maturity"] == "stable", f"catalog does not mark {path} stable")


def validate_planning() -> None:
    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2402")
    require(item is not None and item.get("status") == "in_progress", "R-2402 must remain in_progress before external TLS certification")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in ["external test echo server", "reconnect with backoff", "wss:// TLS support"]:
        require(term in acceptance, f"R-2402 acceptance misses {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2402 WebSocket Client", 1)[1].split("## R-2403", 1)[0]
    for term in ["Status: `in_progress`", "344_api_websocket_client.spectra", "wss://", "external echo-server"]:
        require(term in block, f"backlog R-2402 missing {term}")
    require("R-2402` WebSocket client (in progress;" in read("docs/production-ai-implementation-plan.md"), "strategic plan misses R-2402")
    require("validate_r2402_websocket_client.py" in read("run_tests.ps1"), "runner does not invoke R-2402")


def main() -> None:
    binary = ROOT / "target" / "debug" / (
        "spectralang.exe" if sys.platform.startswith("win") else "spectralang"
    )
    validate_native_surface()
    validate_language_surface()
    validate_docs_and_catalog()
    run_command([cargo_cmd(), "test", "-q", "-p", "spectra-api", "--lib", "websocket", "--offline", "--", "--test-threads=1"])
    run_command([cargo_cmd(), "test", "-q", "-p", "spectra-compiler", "--test", "snapshot_tests", "--offline"])
    run_command([cargo_cmd(), "build", "-q", "-p", "spectra-cli", "--bin", "spectralang", "--offline"])
    run_command([str(binary), "check", "tests/validation/344_api_websocket_client.spectra"])
    run_command([str(binary), "run", "tests/validation/344_api_websocket_client.spectra"])
    validate_planning()
    print("validated R-2402 WebSocket client")


if __name__ == "__main__":
    main()
