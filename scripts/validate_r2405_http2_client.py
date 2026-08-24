#!/usr/bin/env python3
"""Validate the native R-2405 HTTP/2 client transport and evidence gate."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path
from urllib.parse import urlsplit


ROOT = Path(__file__).resolve().parents[1]


def fail(message: str) -> None:
    print(f"R-2405 validation failed: {message}", file=sys.stderr)
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


def run_command(
    args: list[str], timeout: int = 180, env: dict[str, str] | None = None
) -> str:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        env=env,
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
        "pub struct Http2ClientConfig",
        "pub struct Http2Client",
        "pub enum Http2ClientError",
        "pub type Http2PushCallback",
        "pub async fn connect(url: &str",
        "pub async fn request(&self, request: Http2Request)",
        "set_push_callback",
        "builder.initial_max_send_streams",
        "builder.enable_push(config.enable_push)",
        "stream.reserve_capacity(requested)",
        "stream.poll_capacity",
        "collect_h2_body",
        "resolve_http2_endpoint",
        "is_private_or_link_local",
        "client_reuses_one_connection_for_concurrent_requests",
        "client_https_negotiates_h2_with_explicit_trust_root",
        "client_sends_request_body_with_stream_flow_control",
        "known_external_http2_endpoint_round_trips",
        "client_accepts_server_push_and_invokes_callback",
    ]:
        require(term in http2, f"HTTP/2 client implementation missing {term}")

    cargo = read("packages/spectra-api/Cargo.toml")
    for dependency in ["bytes =", "h2 =", "http =", "tokio =", "tokio-rustls ="]:
        require(dependency in cargo, f"HTTP/2 dependency missing: {dependency}")

    tls = read("packages/spectra-api/src/tls.rs")
    require("DEFAULT_TLS_ALPN_HTTP2" in tls, "TLS module does not expose the h2 ALPN token")


def validate_docs_and_planning() -> None:
    docs = read("docs/api/std-api-http2-client.md")
    for term in [
        "Http2Client",
        "http://",
        "https://",
        "ALPN `h2`",
        "SSRF",
        "allow_private_networks",
        "concurrent requests",
        "Server push",
        "external h2 endpoint",
        "validate_r2405_http2_client.py",
    ]:
        require(term in docs, f"HTTP/2 client docs missing {term}")
    require(
        "std-api-http2-client.md" in read("docs/api/README.md"),
        "API README misses HTTP/2 client docs",
    )

    roadmap = tomllib.loads(read("roadmap/roadmap.toml"))
    items = {item["id"]: item for item in roadmap["items"]}
    item = items.get("R-2405")
    require(item is not None, "R-2405 is missing from roadmap")
    require(item.get("status") == "complete", "R-2405 must be complete after native and external evidence")
    require(item.get("owner") == "web", "R-2405 owner changed")
    acceptance = "\n".join(item.get("acceptance", []))
    for term in ["external h2 endpoint", "multiplexed", "server push", "HTTPS", "validate_r2405_http2_client.py"]:
        require(term.lower() in acceptance.lower(), f"R-2405 acceptance misses {term}")

    backlog = read("docs/roadmap-backlog.md")
    require("## R-2405 HTTP/2 Client" in backlog, "backlog misses R-2405")
    block = backlog.split("## R-2405 HTTP/2 Client", 1)[1].split("## R-2406", 1)[0]
    for term in ["Status: `complete`", "external h2", "multiplex", "server push", "validate_r2405_http2_client.py", "nghttp2.org"]:
        require(term.lower() in block.lower(), f"backlog R-2405 misses {term}")
    strategic = read("docs/production-ai-implementation-plan.md")
    require("R-2405` HTTP/2 client (complete;" in strategic, "strategic plan misses R-2405")
    require("validate_r2405_http2_client.py" in read("run_tests.ps1"), "runner does not invoke R-2405")


def main() -> None:
    import argparse

    parser = argparse.ArgumentParser()
    parser.add_argument("--require-external", action="store_true")
    parser.add_argument("--external-url", default=None)
    args = parser.parse_args()

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
    run_command([cargo_cmd(), "check", "-q", "-p", "spectra-api", "--offline"])
    external_url = args.external_url or os.environ.get("SPECTRA_HTTP2_EXTERNAL_URL")
    if args.require_external and not external_url:
        fail("external h2 endpoint is required; set --external-url or SPECTRA_HTTP2_EXTERNAL_URL")
    if external_url:
        parsed = urlsplit(external_url)
        require(
            parsed.scheme in {"http", "https"} and parsed.hostname,
            "external h2 endpoint must be an http(s) URL",
        )
        external_env = os.environ.copy()
        external_env["SPECTRA_HTTP2_EXTERNAL_URL"] = external_url
        run_command(
            [
                cargo_cmd(),
                "test",
                "-q",
                "-p",
                "spectra-api",
                "--lib",
                "http2::tests::known_external_http2_endpoint_round_trips",
                "--offline",
                "--",
                "--ignored",
                "--test-threads=1",
            ],
            env=external_env,
        )
        print("validated R-2405 HTTP/2 client including external h2 evidence")
    else:
        print("validated R-2405 HTTP/2 client; recorded external evidence not rerun")


if __name__ == "__main__":
    main()
