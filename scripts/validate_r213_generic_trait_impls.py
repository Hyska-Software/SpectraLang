#!/usr/bin/env python3
"""Validation gate for R-213 generic traits and generic trait impls."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests" / "validation" / "257_oop_generic_traits.spectra"


def run(binary: Path, args: list[str]) -> subprocess.CompletedProcess[str]:
    command = [str(binary), *args]
    print(f"[R-213] {' '.join(command)}")
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
    print("[R-213] no JSON diagnostics payload found", file=sys.stderr)
    raise SystemExit(1)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default=str(ROOT / "target" / "debug" / "spectralang.exe"))
    parser.add_argument("--report", default=str(ROOT / "target" / "r213-gen-traits" / "report.json"))
    args = parser.parse_args()

    binary = Path(args.binary)
    if not binary.exists():
        print(f"[R-213] binary not found: {binary}", file=sys.stderr)
        return 1

    # 1. Generic trait + concrete impls + generic impl clause + bound dispatch.
    fixture_run = run(binary, ["run", str(FIXTURE)])
    if fixture_run.returncode != 42:
        print("[R-213] fixture did not return 42", file=sys.stderr)
        print(fixture_run.stdout)
        return 1
    for marker in ("42/hi/int-box/gen-box/gen-box", "R-213 generic traits ok"):
        if marker not in fixture_run.stdout:
            print(f"[R-213] missing marker '{marker}'", file=sys.stderr)
            return 1

    # 2. Object safety: dyn of a generic trait is rejected with E026.
    dyn_fixture = ROOT / "target" / "r213-dyn.spectra"
    dyn_fixture.write_text(
        "module t\n\ntrait BoxOf<T> {\n    func get(&self) returns T\n}\n\n"
        "record IntBox {\n    v: int,\n}\n\nimpl BoxOf<int> for IntBox {\n"
        "    func get(&self) returns int { self.v }\n}\n\n"
        "func main() returns int {\n    let ib = IntBox { v: 42 }\n"
        "    let d: dyn BoxOf = ib as dyn BoxOf\n    0\n}\n",
        encoding="utf-8",
    )
    payload = first_json_object(run(binary, ["check", "--json", str(dyn_fixture)]).stdout)
    if payload.get("success", True):
        print("[R-213] dyn-of-generic-trait fixture unexpectedly compiled", file=sys.stderr)
        return 1
    codes = {d.get("code") for d in payload["files"][0]["diagnostics"]}
    if "E026" not in codes:
        print("[R-213] missing E026 for dyn of generic trait", file=sys.stderr)
        return 1

    # 3. Arity mismatch on the trait's type arguments is rejected with E025.
    arity_fixture = ROOT / "target" / "r213-arity.spectra"
    arity_fixture.write_text(
        "module t\n\ntrait BoxOf<T> {\n    func get(&self) returns T\n}\n\n"
        "record IntBox {\n    v: int,\n}\n\nimpl BoxOf<int, string> for IntBox {\n"
        "    func get(&self) returns int { self.v }\n}\n\n"
        "func main() returns int { 0 }\n",
        encoding="utf-8",
    )
    payload = first_json_object(run(binary, ["check", "--json", str(arity_fixture)]).stdout)
    if payload.get("success", True):
        print("[R-213] arity-mismatch fixture unexpectedly compiled", file=sys.stderr)
        return 1
    codes = {d.get("code") for d in payload["files"][0]["diagnostics"]}
    if "E025" not in codes:
        print("[R-213] missing E025 for trait arity mismatch", file=sys.stderr)
        return 1

    report = {
        "item": "R-213",
        "fixture": str(FIXTURE),
        "fixture_exit": fixture_run.returncode,
        "object_safety_code": "E026",
        "arity_code": "E025",
        "status": "passed",
    }
    report_path = Path(args.report)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"[R-213] report written to {report_path}")
    print("[R-213] generic traits validated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
