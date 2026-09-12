#!/usr/bin/env python3
"""Validate the native R-2404 HTTP/2 server transport and evidence gate."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def fail(message: str) -> None:
    print(f"R-2404 validation failed: {message}", file=sys.stderr)
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


def run_command(args: list[str], timeout: int = 180) -> str:
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
    http2 = read("packages/spectra-api/src/http2.rs")
    for term in [
        "use h2::server",
        "pub const ALPN_HTTP2",
        "pub struct Http2Config",
        "max_concurrent_streams",
        "max_header_list_size",
        "flow_control().release_capacity",
        "pub struct Http2Server",
        "plain_http2_multiplexes_requests_and_round_trips_hpack_headers",
        "default_tls_alpn_advertises_h2_alongside_http11",
        "tls_http2_server_negotiates_h2_and_round_trips_request",
    ]:
        require(term in http2, f"HTTP/2 implementation missing {term}")

    cargo = read("packages/spectra-api/Cargo.toml")
    for dependency in ["bytes =", "h2 =", "http =", "tokio =", "tokio-rustls ="]:
        require(dependency in cargo, f"HTTP/2 dependency missing: {dependency}")

    tls = read("packages/spectra-api/src/tls.rs")
    require("DEFAULT_TLS_ALPN_HTTP2" in tls, "TLS module does not expose the h2 ALPN token")
    require("DEFAULT_TLS_ALPN_HTTP11.to_vec()," in tls, "TLS HTTP/1.1 ALPN default disappeared")
    require("DEFAULT_TLS_ALPN_HTTP2.to_vec()" in tls, "TLS h2 ALPN default disappeared")


def validate_docs_and_planning() -> None:
    docs = read("docs/api/std-api-http2.md")
    for term in [
        "HPACK",
        "concurrent requests share one connection",
        "flow-control",
        "ALPN",
        "h2",
        "validate_r2404_http2.py",
    ]:
        require(term in docs, f"HTTP/2 docs missing {term}")
    require("std-api-http2.md" in read("docs/api/README.md"), "API README misses HTTP/2 docs")

    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2404")
    require(item is not None, "R-2404 is missing from roadmap")
    require(item.get("status") == "complete", "R-2404 must be complete after native evidence")
    require(item.get("owner") == "web", "R-2404 owner changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in ["ALPN", "h2", "multiplexes", "HPACK", "validate_r2404_http2.py"]:
        require(term in acceptance, f"R-2404 acceptance misses {term}")

    backlog = read("docs/roadmap-backlog.md")
    require("## R-2404 HTTP/2 Server (h2, ALPN, HPACK)" in backlog, "backlog misses R-2404")
    block = backlog.split("## R-2404 HTTP/2 Server (h2, ALPN, HPACK)", 1)[1].split(
        "## R-2405", 1
    )[0]
    for term in ["Status: `complete`", "ALPN", "HPACK", "multiplex", "validate_r2404_http2.py"]:
        require(term in block, f"backlog R-2404 misses {term}")
    strategic = read("docs/production-ai-implementation-plan.md")
    require("R-2404` HTTP/2 server (complete;" in strategic, "strategic plan misses R-2404 completion")
    require("validate_r2404_http2.py" in read("run_tests.ps1"), "runner does not invoke R-2404")


def main() -> None:
    validate_native_surface()
    validate_docs_and_planning()
    run_command(
        [
            cargo_cmd(),
            "test",
            "-q",
            "-p",
            "spectra-api",
            "--lib",
            "http2",
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
            "tls",
            "--offline",
            "--",
            "--test-threads=1",
        ]
    )
    run_command([cargo_cmd(), "check", "-q", "-p", "spectra-api", "--offline"])
    print("validated R-2404 HTTP/2")


if __name__ == "__main__":
    main()
