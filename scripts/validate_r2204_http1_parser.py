from __future__ import annotations

import os
import shutil
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
    print(f"R-2204 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def run_command(args: list[str]) -> str:
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
    return completed.stdout


def cargo_cmd() -> str:
    configured = os.environ.get("CARGO")
    if configured:
        return configured
    found = shutil.which("cargo")
    if found:
        return found
    windows_default = Path.home() / ".cargo" / "bin" / "cargo.exe"
    if windows_default.exists():
        return str(windows_default)
    return "cargo"


def parse_toml(path: str):
    with (ROOT / path).open("rb") as fh:
        return tomllib.load(fh)


def validate_parser_surface() -> None:
    http = read("packages/spectra-api/src/http.rs")
    required_terms = [
        "pub struct Http1Parser",
        "pub struct ParsedRequest",
        "pub struct ParsedResponse",
        "pub struct Header",
        "pub struct BodyChunk",
        "pub struct HttpBody",
        "pub enum ParseErrorKind",
        "pub struct ParseError",
        "pub fn parse_request",
        "pub fn parse_response",
        "pub fn serialize_request",
        "pub fn serialize_response",
        "parse_chunked_body",
        "parse_trailer_section",
        "determine_keep_alive",
        "ParserConfig",
        "max_header_bytes",
        "max_body_bytes",
        "max_chunk_bytes",
    ]
    for term in required_terms:
        require(term in http, f"missing parser implementation term {term}")

    test_terms = [
        "request_parser_streams_headers_then_body",
        "parser_keeps_pipelined_request_bytes_for_next_message",
        "response_parser_accepts_rfc_7230_style_sample",
        "chunked_request_round_trips_with_extensions_and_trailers",
        "chunked_response_round_trips_without_trailers",
        "malformed_header_reports_typed_position",
        "malformed_chunk_size_reports_typed_position",
        "rejects_conflicting_content_length",
        "rejects_unsupported_transfer_encoding",
    ]
    for term in test_terms:
        require(term in http, f"missing parser regression test {term}")


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2204 = items.get("R-2204")
    require(r2204 is not None, "R-2204 missing from roadmap")
    require(r2204.get("status") == "complete", "R-2204 must be marked complete")
    require(r2204.get("owner") == "web", "R-2204 owner must remain web")
    require(
        r2204.get("dependencies") == ["R-2107", "R-2202"],
        "R-2204 dependencies must remain R-2107 and R-2202",
    )
    acceptance = "\n".join(r2204.get("acceptance", []))
    for term in [
        "structured request and response values",
        "body chunks",
        "chunked transfer encoding round-trips",
        "typed parse error",
        "offending position",
        "RFC 7230",
        "scripts/validate_r2204_http1_parser.py",
    ]:
        require(term in acceptance, f"R-2204 acceptance must mention {term}")

    backlog = read("docs/roadmap-backlog.md")
    require("## R-2204 HTTP/1.1 Parser" in backlog, "backlog R-2204 missing")
    r2204_block = backlog.split("## R-2204 HTTP/1.1 Parser", 1)[1].split("## R-2205", 1)[0]
    for term in [
        "Status: `complete`",
        "packages/spectra-api/src/http.rs",
        "Http1Parser",
        "ParsedRequest",
        "ParsedResponse",
        "ParseError",
        "validate_r2204_http1_parser.py",
    ]:
        require(term in r2204_block, f"backlog R-2204 block must mention {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2204` HTTP/1.1 parser (complete;" in plan,
        "implementation plan must mark R-2204 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require(
        "validate_r2204_http1_parser.py" in runner,
        "run_tests.ps1 must run R-2204 validator",
    )
    require(
        'Teste = "validate_r2204_http1_parser"' in runner,
        "run_tests.ps1 must record R-2204 result",
    )


def main() -> None:
    validate_parser_surface()
    run_command([cargo_cmd(), "test", "-q", "-p", "spectra-api"])
    validate_planning()
    validate_runner()
    print("validated R-2204 HTTP/1.1 parser")


if __name__ == "__main__":
    main()
