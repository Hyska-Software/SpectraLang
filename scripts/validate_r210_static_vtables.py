#!/usr/bin/env python3
"""Validation gate for R-210 dyn Trait vtable lifetime (heap vtables escaped to the base frame)."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests" / "validation" / "255_oop_dyn_vtable_lifetime.spectra"


def run(binary: Path, args: list[str]) -> subprocess.CompletedProcess[str]:
    command = [str(binary), *args]
    print(f"[R-210] {' '.join(command)}")
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
    parser.add_argument("--report", default=str(ROOT / "target" / "r210-vtables" / "report.json"))
    args = parser.parse_args()

    binary = Path(args.binary)
    if not binary.exists():
        print(f"[R-210] binary not found: {binary}", file=sys.stderr)
        return 1

    # 1. dyn values outlive their creating scope (function and block).
    for attempt in range(3):
        fixture_run = run(binary, ["run", str(FIXTURE)])
        if fixture_run.returncode != 42:
            print(f"[R-210] fixture attempt {attempt + 1} did not return 42", file=sys.stderr)
            print(fixture_run.stdout)
            return 1
    for marker in ("[circle area=48]", "[square area=36]", "R-210 vtable lifetime ok"):
        if marker not in fixture_run.stdout:
            print(f"[R-210] missing marker '{marker}'", file=sys.stderr)
            return 1

    # 2. Existing dyn tests still pass.
    for fixture in ("129_async_trait_objects_future_stream.spectra", "132_formal_send_sync_bounds.spectra"):
        existing = run(binary, ["run", str(ROOT / "tests" / "validation" / fixture)])
        if existing.returncode != 0:
            print(f"[R-210] existing dyn fixture {fixture} failed", file=sys.stderr)
            print(existing.stdout)
            return 1

    # 3. The dead Module.vtables/VTableDef IR infrastructure was removed.
    ir_text = (ROOT / "midend" / "src" / "ir.rs").read_text(encoding="utf-8")
    if "VTableDef" in ir_text or "pub vtables:" in ir_text:
        print("[R-210] dead VTableDef/Module.vtables infrastructure still present", file=sys.stderr)
        return 1

    report = {
        "item": "R-210",
        "fixture": str(FIXTURE),
        "attempts": 3,
        "dead_code_removed": True,
        "status": "passed",
    }
    report_path = Path(args.report)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"[R-210] report written to {report_path}")
    print("[R-210] dyn vtable lifetime validated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
