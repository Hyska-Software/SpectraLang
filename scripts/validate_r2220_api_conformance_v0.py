#!/usr/bin/env python3
"""Validate R-2220 API conformance suite v0."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REPORT = ROOT / "target" / "api-conformance-v0.json"


def fail(message: str) -> None:
    print(f"R-2220 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def parse_toml(path: str):
    with (ROOT / path).open("rb") as fh:
        return tomllib.load(fh)


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


def validate_sources() -> None:
    lib = read("packages/spectra-api/src/lib.rs")
    conformance = read("packages/spectra-api/src/conformance.rs")
    example = read("packages/spectra-api/examples/conformance_v0.rs")
    docs = read("docs/api/api-conformance-v0.md")

    require("pub mod conformance;" in lib, "spectra-api must export conformance module")
    for term in [
        "pub const SUITE_ID",
        "spectra.api.conformance.v0",
        "pub fn run_v0_suite",
        "pub fn conformance_v0_cases",
        "ConformanceReport",
        "to_json_string",
        "http1.request.get_minimal",
        "http1.request.chunked_round_trip",
        "http1.error.conflicting_content_length",
        "json.kind.matrix",
        "json.round_trip.nested_object",
        "json.encode.non_finite_rejected",
        "routing.literal.match",
        "routing.regex.constraint",
        "routing.conflict.overlap",
        "api_conformance_v0_suite_passes",
    ]:
        require(term in conformance, f"conformance module missing {term}")

    require("run_v0_suite()" in example, "conformance example must run the suite")
    require("--output" in example, "conformance example must support --output")

    for term in [
        "target/api-conformance-v0.json",
        "HTTP/1.1 Must-Pass Cases",
        "JSON Must-Pass Cases",
        "Router Must-Pass Cases",
        "http1.request.get_minimal",
        "json.kind.matrix",
        "routing.literal.match",
    ]:
        require(term in docs, f"conformance docs missing {term}")

    api_index = read("docs/api/README.md")
    require("api-conformance-v0.md" in api_index, "docs/api README must link conformance v0")


def load_report() -> dict:
    require(REPORT.is_file(), f"missing report {REPORT}")
    try:
        return json.loads(REPORT.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        fail(f"report is not valid JSON: {exc}")


def validate_report(report: dict) -> None:
    require(report.get("suite") == "spectra.api.conformance.v0", "wrong suite id")
    require(report.get("version") == "0.1.0", "wrong suite version")
    cases = report.get("cases")
    require(isinstance(cases, list), "report cases must be a list")
    require(report.get("total") == len(cases), "report total does not match cases")
    require(report.get("failed") == 0, "conformance report has failed cases")
    require(report.get("passed") == report.get("total"), "passed count mismatch")
    require(len(cases) >= 25, "v0 conformance suite must contain at least 25 cases")

    ids = [case.get("id") for case in cases]
    require(len(ids) == len(set(ids)), "conformance case ids must be unique")
    categories = {case.get("category") for case in cases}
    require({"http1", "json", "routing"} <= categories, "missing conformance categories")

    for required in [
        "http1.request.get_minimal",
        "http1.request.content_length_body",
        "http1.request.pipelined_streaming",
        "http1.request.chunked_round_trip",
        "http1.response.rfc7230_sample",
        "http1.response.chunked_round_trip",
        "http1.connection.http10_keep_alive",
        "http1.error.malformed_header_position",
        "http1.error.invalid_chunk_size",
        "http1.error.unsupported_transfer_encoding",
        "http1.error.conflicting_content_length",
        "http1.types.method_status_matrix",
        "http1.types.header_validation",
        "json.kind.matrix",
        "json.round_trip.nested_object",
        "json.escape.unicode",
        "json.error.invalid_syntax_offset",
        "json.encode.non_finite_rejected",
        "json.number.exponent_round_trip",
        "routing.literal.match",
        "routing.param.extract",
        "routing.wildcard.extract",
        "routing.regex.constraint",
        "routing.method.separation",
        "routing.conflict.overlap",
        "routing.invalid.path",
    ]:
        require(required in ids, f"report missing case {required}")

    for case in cases:
        require(case.get("passed") is True, f"case did not pass: {case}")
        require(isinstance(case.get("description"), str) and case["description"], "case missing description")
        require(isinstance(case.get("detail"), str), "case missing detail")


def validate_planning() -> None:
    roadmap = parse_toml("roadmap/roadmap.toml")
    items = {item["id"]: item for item in roadmap["items"]}
    r2220 = items.get("R-2220")
    require(r2220 is not None, "R-2220 missing from roadmap")
    require(r2220.get("status") == "complete", "R-2220 must be complete")
    require(r2220.get("owner") == "tooling", "R-2220 owner changed")
    require(
        r2220.get("dependencies") == ["R-2216", "R-2217", "R-2208"],
        "R-2220 dependencies changed",
    )
    acceptance = "\n".join(r2220.get("acceptance", []))
    for term in [
        "scripts/validate_r2220_api_conformance_v0.py",
        "machine-readable report",
        "HTTP/1.1",
        "JSON conformance matrix",
        "run_tests.ps1",
        "target/api-conformance-v0.json",
    ]:
        require(term in acceptance, f"R-2220 acceptance missing {term}")

    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-2220 API Conformance Suite v0 (HTTP/1.1)", 1)[1].split(
        "## R-2301", 1
    )[0]
    for term in [
        "Status: `complete`",
        "packages/spectra-api/src/conformance.rs",
        "packages/spectra-api/examples/conformance_v0.rs",
        "docs/api/api-conformance-v0.md",
        "target/api-conformance-v0.json",
        "scripts/validate_r2220_api_conformance_v0.py",
    ]:
        require(term in block, f"backlog R-2220 missing {term}")

    plan = read("docs/production-ai-implementation-plan.md")
    require(
        "R-2220` API conformance suite v0 (HTTP/1.1) (complete;" in plan,
        "implementation plan must mark R-2220 complete",
    )


def validate_runner() -> None:
    runner = read("run_tests.ps1")
    require("validate_r2220_api_conformance_v0.py" in runner, "run_tests.ps1 must run R-2220")
    require(
        'Teste = "validate_r2220_api_conformance_v0"' in runner,
        "run_tests.ps1 must record R-2220",
    )


def main() -> None:
    validate_sources()
    if REPORT.exists():
        REPORT.unlink()
    # Cargo invocations get a generous timeout: a cold rebuild of spectra-api
    # under a loaded machine exceeds the generic 120s budget, which would turn
    # the R-3007 release gate into a false negative.
    run_command(
        [cargo_cmd(), "test", "-q", "-p", "spectra-api", "conformance_v0", "--offline"],
        timeout=900,
    )
    run_command(
        [
            cargo_cmd(),
            "run",
            "-q",
            "-p",
            "spectra-api",
            "--example",
            "conformance_v0",
            "--offline",
            "--",
            "--output",
            str(REPORT),
        ],
        timeout=900,
    )
    validate_report(load_report())
    validate_planning()
    validate_runner()
    print(f"validated R-2220 API conformance v0 report at {REPORT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
