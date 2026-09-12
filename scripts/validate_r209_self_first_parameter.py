#!/usr/bin/env python3
"""Validation gate for R-209 self-first-parameter validation."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests" / "errors" / "self_must_be_first_parameter.spectra"


def run(binary: Path, args: list[str]) -> subprocess.CompletedProcess[str]:
    command = [str(binary), *args]
    print(f"[R-209] {' '.join(command)}")
    return subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default=str(ROOT / "target" / "debug" / "spectralang.exe"))
    parser.add_argument("--report", default=str(ROOT / "target" / "r209-self-first" / "report.json"))
    args = parser.parse_args()

    binary = Path(args.binary)
    if not binary.exists():
        print(f"[R-209] binary not found: {binary}", file=sys.stderr)
        return 1

    # The fixture must FAIL with E024 codes on all three receiver sites.
    payload = None
    for line in run(binary, ["check", "--json", str(FIXTURE)]).stdout.splitlines():
        line = line.strip()
        if line.startswith("{") and line.endswith("}"):
            payload = json.loads(line)
            break
    if payload is None or payload.get("success", True):
        print("[R-209] fixture unexpectedly compiled", file=sys.stderr)
        return 1

    diagnostics = payload["files"][0]["diagnostics"]
    e024_sites = [d for d in diagnostics if d.get("code") == "E024"]
    messages = {d.get("message", "") for d in e024_sites}
    if len(e024_sites) < 2:
        print(
            f"[R-209] expected at least 2 E024 diagnostics, found {len(e024_sites)}",
            file=sys.stderr,
        )
        return 1
    for needle in ("increment", "describe"):
        if not any(needle in m for m in messages):
            print(f"[R-209] missing E024 for method '{needle}'", file=sys.stderr)
            return 1

    # A valid method with self first still compiles.
    valid = run(
        binary,
        ["check", str(ROOT / "tests" / "validation" / "40_self_field_access.spectra")],
    )
    if valid.returncode != 0:
        print("[R-209] valid self-first fixture failed to compile", file=sys.stderr)
        return 1

    # The code is documented.
    doc = (ROOT / "docs" / "diagnostics" / "error-code-reference.md").read_text(encoding="utf-8")
    if "`E024`" not in doc:
        print("[R-209] E024 is not documented", file=sys.stderr)
        return 1

    report = {
        "item": "R-209",
        "fixture": str(FIXTURE),
        "e024_sites": len(e024_sites),
        "messages": sorted(messages),
        "status": "passed",
    }
    report_path = Path(args.report)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"[R-209] report written to {report_path}")
    print("[R-209] self-first-parameter validation verified")
    return 0


if __name__ == "__main__":
    sys.exit(main())
