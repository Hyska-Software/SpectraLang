#!/usr/bin/env python3
"""Validation gate for R-208 stable OOP diagnostic codes."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
ERRORS = ROOT / "tests" / "errors"

EXPECTED_CODES = {
    "method_not_found.spectra": "E017",
    "method_wrong_args.spectra": "E023",
    "trait_incomplete.spectra": "E016",
    "trait_inheritance_incomplete.spectra": "E016",
    "trait_wrong_signature.spectra": "E023",
    "dyn_trait_send_bound_missing.spectra": "E2104",
    "async_trait_object_safety.spectra": "E2108",
    "trait_bound_missing_method_stress.spectra": "E010",
    "struct_literal_shorthand_undefined_binding.spectra": "E001",
}


def run(binary: Path, args: list[str]) -> subprocess.CompletedProcess[str]:
    command = [str(binary), *args]
    print(f"[R-208] {' '.join(command)}")
    return subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )


def first_json_object(output: str) -> dict:
    for line in output.splitlines():
        line = line.strip()
        if line.startswith("{") and line.endswith("}"):
            return json.loads(line)
    print(output)
    print("[R-208] no JSON diagnostics payload found", file=sys.stderr)
    raise SystemExit(1)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default=str(ROOT / "target" / "debug" / "spectralang.exe"))
    parser.add_argument("--report", default=str(ROOT / "target" / "r208-oop-diagnostics" / "report.json"))
    args = parser.parse_args()

    binary = Path(args.binary)
    if not binary.exists():
        print(f"[R-208] binary not found: {binary}", file=sys.stderr)
        return 1

    results: dict[str, str] = {}
    for fixture, expected_code in sorted(EXPECTED_CODES.items()):
        fixture_path = ERRORS / fixture
        if not fixture_path.exists():
            print(f"[R-208] missing fixture {fixture_path}", file=sys.stderr)
            return 1
        payload = first_json_object(run(binary, ["check", "--json", str(fixture_path)]).stdout)
        diagnostics = payload["files"][0]["diagnostics"]
        actual_code = diagnostics[0].get("code")
        results[fixture] = actual_code or "semantic"
        if actual_code != expected_code:
            print(
                f"[R-208] {fixture}: expected code {expected_code}, got {actual_code} "
                f"(message: {diagnostics[0].get('message')})",
                file=sys.stderr,
            )
            return 1

    # Stable-code documentation is present in the error-code reference.
    doc = (ROOT / "docs" / "diagnostics" / "error-code-reference.md").read_text(encoding="utf-8")
    for code in ("E012", "E016", "E017", "E019", "E020", "E021", "E022", "E023"):
        if f"`{code}`" not in doc:
            print(f"[R-208] code {code} is not documented", file=sys.stderr)
            return 1

    report = {
        "item": "R-208",
        "fixtures": results,
        "documented_codes": ["E012", "E013", "E014", "E015", "E016", "E017", "E018", "E019", "E020", "E021", "E022", "E023"],
        "status": "passed",
    }
    report_path = Path(args.report)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"[R-208] report written to {report_path}")
    print("[R-208] OOP diagnostic codes validated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
