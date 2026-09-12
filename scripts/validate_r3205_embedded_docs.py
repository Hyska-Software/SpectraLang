# R-3205 — version-matched language reference from the CLI.
#
# Validates the build-time embedding, the `docs` command contract (version,
# sha256, sections, strict-subset selection, determinism) and the planning
# bookkeeping for the item.
from __future__ import annotations

import hashlib
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPECTRALANG = ROOT / "target" / "debug" / "spectralang.exe"
CARGO = shutil.which("cargo") or "cargo"
REFERENCE = ROOT / "docs" / "AI-AGENT-REFERENCE.md"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3205 validation failed: {message}", file=sys.stderr)
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


def run_docs(extra_args: list[str]) -> tuple[int, str]:
    completed = subprocess.run(
        [str(SPECTRALANG), "docs", *extra_args],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    return completed.returncode, completed.stdout


def validate_implementation() -> None:
    build = read("tools/spectra-cli/build.rs")
    for term in [
        "embed_language_reference",
        "AI-AGENT-REFERENCE.md",
        "cargo:rerun-if-changed",
        "sha256_hex",
    ]:
        require(term in build, f"build.rs missing {term}")

    cli_docs = read("tools/spectra-cli/src/cli_docs.rs")
    for term in ["fn execute_docs", "spectralang.docs.v1", "REFERENCE_SHA256", "fn reference_sections"]:
        require(term in cli_docs, f"cli_docs missing {term}")

    lib = read("tools/spectra-cli/src/lib.rs")
    require('include!("cli_docs.rs");' in lib, "lib.rs must include cli_docs.rs")


def parse_json_line(output: str) -> dict:
    """Parse the first output line as JSON (stderr is merged for diagnostics)."""
    return json.loads(output.splitlines()[0])


def validate_cli_contract() -> None:
    exit_code, output = run_docs(["--json"])
    require(exit_code == 0, f"docs --json exited {exit_code}:\n{output}")
    report = parse_json_line(output)

    require(report.get("schema") == "spectralang.docs.v1", "schema mismatch")
    require(report.get("success") is True, "success must be true")

    version_match = re.search(
        r'^version = "([^"]+)"', read("tools/spectra-cli/Cargo.toml"), flags=re.MULTILINE
    )
    require(version_match is not None, "could not read the CLI crate version")
    require(
        report.get("version") == version_match.group(1),
        f"docs version {report.get('version')} must match the crate version {version_match.group(1)}",
    )

    expected_hash = hashlib.sha256(REFERENCE.read_bytes()).hexdigest()
    require(
        report.get("reference_sha256") == expected_hash,
        f"reference hash mismatch: {report.get('reference_sha256')} != {expected_hash}",
    )

    sections = report.get("sections", [])
    require(len(sections) >= 25, f"expected the numbered reference sections, got {len(sections)}")
    headings = [section["heading"] for section in sections]
    require("1. Language Overview" in headings, f"missing section 1: {headings[:5]}")

    exit_code, section_output = run_docs(["--json", "--section", "stdlib"])
    require(exit_code == 0, f"docs --section stdlib exited {exit_code}:\n{section_output}")
    section_report = parse_json_line(section_output)
    section_titles = [section["heading"] for section in section_report.get("sections", [])]
    require(
        section_titles == ["23. Standard Library"],
        f"--section stdlib must select exactly one section, got {section_titles}",
    )
    require(
        len(section_output) < len(output),
        "--section output must be a strict subset of the full output",
    )

    _, second = run_docs(["--json"])
    require(output == second, "docs output must be byte-identical across runs")

    exit_code, failure_output = run_docs(["--json", "--section", "not-a-section"])
    require(exit_code == 64, f"unknown section must exit 64, got {exit_code}")
    failure = parse_json_line(failure_output)
    require(failure.get("success") is False, "unknown section must set success=false")
    require(failure.get("available"), "unknown section must list available headings")


def validate_planning() -> None:
    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-3205 Version-Matched Language Reference from the CLI", 1)[1].split(
        "## R-3206", 1
    )[0]
    for term in ["Status: `complete`", "docs/agent-platform-plan.md", "validate_r3205_embedded_docs.py"]:
        require(term in block, f"backlog R-3205 missing {term}")

    runner = read("run_tests.ps1")
    require("validate_r3205_embedded_docs.py" in runner, "run_tests.ps1 must run R-3205")


def main() -> None:
    run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    validate_implementation()
    validate_cli_contract()
    validate_planning()
    print("validated R-3205 embedded version-matched docs")


if __name__ == "__main__":
    main()
