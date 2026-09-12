#!/usr/bin/env python3
"""Validation gate for R-212 UFCS Trait::method(obj, args)."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests" / "validation" / "256_oop_ufcs.spectra"


def run(binary: Path, args: list[str]) -> subprocess.CompletedProcess[str]:
    command = [str(binary), *args]
    print(f"[R-212] {' '.join(command)}")
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
    parser.add_argument("--report", default=str(ROOT / "target" / "r212-ufcs" / "report.json"))
    args = parser.parse_args()

    binary = Path(args.binary)
    if not binary.exists():
        print(f"[R-212] binary not found: {binary}", file=sys.stderr)
        return 1

    # 1. UFCS on concrete receivers, default methods, and dyn receivers.
    fixture_run = run(binary, ["run", str(FIXTURE)])
    if fixture_run.returncode != 42:
        print("[R-212] fixture did not return 42", file=sys.stderr)
        print(fixture_run.stdout)
        return 1
    for marker in ("<btn>", "<btn>!", "[7]", "R-212 UFCS ok"):
        if marker not in fixture_run.stdout:
            print(f"[R-212] missing marker '{marker}'", file=sys.stderr)
            return 1

    # 2. Missing trait method is rejected.
    payload = None
    for line in run(
        binary,
        ["check", "--json", str(ROOT / "tests" / "validation" / "256_oop_ufcs.spectra")],
    ).stdout.splitlines():
        pass  # valid fixture compiles; nothing to assert here
    missing = ROOT / "target" / "r212-missing.spectra"
    missing.write_text(
        "module t\n\ntrait T {\n    func m(&self) returns int\n}\n\n"
        "record R {\n    x: int,\n}\n\nimpl T for R {\n    func m(&self) returns int { self.x }\n}\n\n"
        "func main() returns int {\n    let r = R { x: 1 }\n    T::missing(r)\n    0\n}\n",
        encoding="utf-8",
    )
    payload = None
    for line in run(binary, ["check", "--json", str(missing)]).stdout.splitlines():
        line = line.strip()
        if line.startswith("{") and line.endswith("}"):
            payload = json.loads(line)
            break
    if payload is None or payload.get("success", True):
        print("[R-212] missing-trait-method fixture unexpectedly compiled", file=sys.stderr)
        return 1
    codes = {d.get("code") for d in payload["files"][0]["diagnostics"]}
    if "E017" not in codes:
        print("[R-212] missing trait method did not produce E017", file=sys.stderr)
        return 1

    report = {
        "item": "R-212",
        "fixture": str(FIXTURE),
        "fixture_exit": fixture_run.returncode,
        "markers": ["<btn>", "<btn>!", "[7]", "R-212 UFCS ok"],
        "missing_method_code": "E017",
        "status": "passed",
    }
    report_path = Path(args.report)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"[R-212] report written to {report_path}")
    print("[R-212] UFCS validated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
