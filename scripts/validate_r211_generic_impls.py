#!/usr/bin/env python3
"""Validation gate for R-211 generic impl blocks (impl Par<T>) and module-qualified impls."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests" / "validation" / "254_oop_generic_impls.spectra"


def run(binary: Path, args: list[str]) -> subprocess.CompletedProcess[str]:
    command = [str(binary), *args]
    print(f"[R-211] {' '.join(command)}")
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
    parser.add_argument("--report", default=str(ROOT / "target" / "r211-generic-impls" / "report.json"))
    args = parser.parse_args()

    binary = Path(args.binary)
    if not binary.exists():
        print(f"[R-211] binary not found: {binary}", file=sys.stderr)
        return 1

    # 1. Template impl on a generic struct with two instantiations.
    fixture_run = run(binary, ["run", str(FIXTURE)])
    if fixture_run.returncode != 42:
        print("[R-211] fixture did not return 42", file=sys.stderr)
        print(fixture_run.stdout)
        return 1
    for marker in ("boxed=42", "R-211 generic impl ok"):
        if marker not in fixture_run.stdout:
            print(f"[R-211] missing marker '{marker}'", file=sys.stderr)
            return 1

    # 2. Module-qualified impl target (impl point::Point).
    project = ROOT / "target" / "r211-module-impl"
    src = project / "src"
    src.mkdir(parents=True, exist_ok=True)
    (src / "point.spectra").write_text(
        "module point\n\npublic record Point {\n    public x: int,\n}\n",
        encoding="utf-8",
    )
    (src / "main.spectra").write_text(
        "module main\n\nimport point\n\nimpl point::Point {\n"
        "    func double(&self) returns int {\n        self.x * 2\n    }\n}\n\n"
        "func main() returns int {\n    let p = Point { x: 21 }\n    p.double()\n}\n",
        encoding="utf-8",
    )
    project_run = run(binary, ["run", str(project)])
    if project_run.returncode != 42:
        print("[R-211] module-qualified impl fixture did not return 42", file=sys.stderr)
        print(project_run.stdout)
        return 1

    # 3. The specialized methods are emitted per instantiation.
    ir_run = run(binary, ["run", "--dump-ir", str(FIXTURE)])
    for needle in ("fn Boxed_int_get", "fn Boxed_float_get", "fn Boxed_int_set"):
        if needle not in ir_run.stdout:
            print(f"[R-211] missing specialized function '{needle}' in IR dump", file=sys.stderr)
            return 1

    report = {
        "item": "R-211",
        "fixture": str(FIXTURE),
        "fixture_exit": fixture_run.returncode,
        "module_impl_exit": project_run.returncode,
        "specialized_functions": ["Boxed_int_get", "Boxed_float_get", "Boxed_int_set"],
        "status": "passed",
    }
    report_path = Path(args.report)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"[R-211] report written to {report_path}")
    print("[R-211] generic impl blocks validated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
