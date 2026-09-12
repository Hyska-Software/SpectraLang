#!/usr/bin/env python3
"""R-3221 -- the host-installed `std.agent` adapters, end to end.

Validates the two seams `spectra_api::register()` now fills (plan adaptation
12):

  * `ApiHttpTransport`, the real `HttpTransport` over the `spectra.api.client`
    client, so the OpenAI-compatible provider leaves the process; and
  * `ApiTraceSink`, the GenAI `TraceSink` over `std.api.trace`, so a run's
    spans reach the runtime's OTLP exporter with content capture off.

The language fixture in `tests/validation/383_agent_host_adapters.spectra` runs
in JIT and AOT against one local HTTP server that answers both the
OpenAI-compatible chat-completions request and the OTLP traces export, so the
validator can assert what actually crossed the socket: the provider request,
the exported `invoke_agent`/`chat` spans, and the absence of prompt content.
"""
from __future__ import annotations

import http.server
import json
import os
import subprocess
import sys
import threading
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPECTRALANG = ROOT / "target" / "debug" / "spectralang.exe"
FIXTURE = "tests/validation/383_agent_host_adapters.spectra"
ADAPTER = "packages/spectra-api/src/agent_transport.rs"
REGISTRATION = "packages/spectra-api/src/api_registration.rs"
LIB = "packages/spectra-api/src/lib.rs"
DOCS = "docs/agent-platform.md"
RUNNER = "run_tests.ps1"

CHAT_RESPONSE = json.dumps(
    {
        "choices": [
            {
                "message": {"role": "assistant", "content": "local answer 383"},
                "finish_reason": "stop",
            }
        ],
        "usage": {"prompt_tokens": 4, "completion_tokens": 3},
    }
).encode("utf-8")

PROMPT = "host adapters"
EXPECTED = "local answer 383"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3221 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def run(args: list[str], environment: dict[str, str] | None = None) -> tuple[int, str]:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        env=environment,
    )
    return completed.returncode, completed.stdout


# ── implementation ───────────────────────────────────────────────────────


def validate_implementation() -> None:
    adapter = read(ADAPTER)
    for term in [
        "pub struct ApiHttpTransport",
        "impl HttpTransport for ApiHttpTransport",
        "pub struct ApiTraceSink",
        "impl TraceSink for ApiTraceSink",
        "install_agent_host_adapters",
        "install_agent_host_adapters_with",
        "set_http_transport",
        "set_trace_sink",
        "span_start",
        "span_end",
        "gen_ai.conventions.version",
    ]:
        require(term in adapter, f"agent_transport.rs must implement {term}")
    require(
        "fn captures_content" not in adapter,
        "the sink must keep the content-capture default (off), never override it",
    )
    require(
        "span.attributes" in adapter,
        "the sink must forward the agent layer's attributes",
    )

    registration = read(REGISTRATION)
    require(
        "install_agent_host_adapters()" in registration,
        "register() must install the adapters before any program runs",
    )
    require(
        "pub mod agent_transport" in read(LIB),
        "the adapter module must be reachable from the crate root",
    )

    # The agent crate stays the owner of the seams: spectra-api implements
    # them, it does not redefine them.
    require(
        "pub trait HttpTransport" not in adapter and "pub trait TraceSink" not in adapter,
        "the adapter must implement the seams, not restate them",
    )


def validate_documentation() -> None:
    docs = read(DOCS)
    for term in [
        "install_agent_host_adapters",
        "ApiHttpTransport",
        "ApiTraceSink",
        "captures_content() == false",
    ]:
        require(term in docs, f"{DOCS} must document {term}")
    require(
        "validate_r3221_host_adapters" in read(RUNNER),
        f"{RUNNER} must register validate_r3221_host_adapters",
    )


# ── local server ─────────────────────────────────────────────────────────

class Recorder(http.server.BaseHTTPRequestHandler):
    """Answers chat completions and OTLP exports, recording every request."""

    protocol_version = "HTTP/1.1"
    requests: list[tuple[str, bytes]] = []

    def do_POST(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler contract
        length = int(self.headers.get("content-length", "0"))
        body = self.rfile.read(length)
        type(self).requests.append((self.path, body))
        if self.path.endswith("/v1/chat/completions"):
            payload = CHAT_RESPONSE
            content_type = "application/json"
        else:
            payload = b""
            content_type = "application/x-protobuf"
        self.send_response(200)
        self.send_header("content-type", content_type)
        self.send_header("content-length", str(len(payload)))
        self.send_header("connection", "close")
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *args) -> None:  # silence the per-request logging
        pass


def start_server() -> tuple[http.server.ThreadingHTTPServer, str]:
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Recorder)
    port = server.server_address[1]
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{port}"
    return server, base


def environment(base: str) -> dict[str, str]:
    values = dict(os.environ)
    values["SPECTRA_R3221_ENDPOINT"] = base
    values["SPECTRA_R3221_OTLP_ENDPOINT"] = f"{base}/v1/traces"
    return values


def assert_exchange(stage: str) -> None:
    chat = [
        body
        for path, body in Recorder.requests
        if path.startswith("/v1/chat/completions")
    ]
    traces = b"".join(
        body for path, body in Recorder.requests if path.startswith("/v1/traces")
    )
    require(
        len(chat) == 1,
        f"{stage}: the provider must send exactly one chat request, got {len(chat)} "
        f"to {[path for path, _ in Recorder.requests]}",
    )
    request = chat[0].decode("utf-8", "replace")
    require('"model":"test/local"' in request, f"{stage}: request body: {request}")
    require(f'"content":"{PROMPT}"' in request, f"{stage}: request body: {request}")

    require(b"invoke_agent" in traces, f"{stage}: the run span was not exported")
    require(b"chat test/local" in traces, f"{stage}: the chat span was not exported")
    require(
        b"gen_ai.conventions.version" in traces,
        f"{stage}: the pinned conventions version was not exported",
    )
    require(
        b"gen_ai.operation.name" in traces,
        f"{stage}: the GenAI operation attribute was not exported",
    )
    require(
        PROMPT.encode() not in traces and EXPECTED.encode() not in traces,
        f"{stage}: content capture leaked into the export",
    )


def run_fixture(stage: str, environment: dict[str, str]) -> None:
    Recorder.requests.clear()
    exit_code, output = run([str(SPECTRALANG), "run", FIXTURE], environment)
    require(exit_code == 0, f"{stage}: fixture exited {exit_code}:\n{output}")
    assert_exchange(stage)


def validate_fixture(environment: dict[str, str]) -> None:
    exit_code, output = run([str(SPECTRALANG), "check", "--json", FIXTURE])
    require(exit_code == 0, f"check on {FIXTURE} exited {exit_code}:\n{output}")
    report = json.loads(output.splitlines()[0])
    diagnostics = [
        entry for file in report.get("files", []) for entry in file.get("diagnostics", [])
    ]
    require(not diagnostics, f"the valid fixture must be diagnostic-free: {diagnostics}")

    run_fixture("JIT", environment)

    executable = ROOT / "target" / "r3221-agent-adapters.exe"
    exit_code, output = run(
        [
            str(SPECTRALANG),
            "compile",
            "--debug-info=none",
            "--emit-exe",
            str(executable),
            FIXTURE,
        ]
    )
    require(exit_code == 0, f"AOT compilation exited {exit_code}:\n{output}")

    Recorder.requests.clear()
    completed = subprocess.run(
        [str(executable)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        env=environment,
    )
    require(
        completed.returncode == 0,
        f"AOT: the binary exited {completed.returncode}:\n{completed.stdout}",
    )
    assert_exchange("AOT")


def main() -> None:
    validate_implementation()
    validate_documentation()
    server, base = start_server()
    try:
        validate_fixture(environment(base))
    finally:
        time.sleep(0.05)
        server.shutdown()
        server.server_close()
    print("R-3221 host adapters validated (JIT and AOT, transport and span sink).")


if __name__ == "__main__":
    main()
