#!/usr/bin/env python3
"""Validation gate for R-207 struct layout with padding and cumulative offsets."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests" / "validation" / "253_oop_struct_layout_drop.spectra"
DROP_EXAMPLE = ROOT / "examples" / "test_oop_drop.spectra"


def run(command: list[str], *, expect_success: bool = True) -> subprocess.CompletedProcess[str]:
    print(f"[R-207] {' '.join(command)}")
    completed = subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if expect_success and completed.returncode != 0:
        print(completed.stdout)
        raise SystemExit(completed.returncode)
    if not expect_success and completed.returncode == 0:
        print(completed.stdout)
        print("[R-207] command unexpectedly succeeded", file=sys.stderr)
        raise SystemExit(1)
    return completed


def require_contains(text: str, needles: list[str], what: str) -> None:
    missing = [needle for needle in needles if needle not in text]
    if missing:
        for needle in missing:
            print(f"[R-207] missing marker in {what}: {needle}", file=sys.stderr)
        raise SystemExit(1)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default=str(ROOT / "target" / "debug" / "spectralang.exe"))
    parser.add_argument("--report", default=str(ROOT / "target" / "r207-struct-layout" / "report.json"))
    args = parser.parse_args()

    binary = Path(args.binary)
    if not binary.exists():
        print(f"[R-207] binary not found: {binary}", file=sys.stderr)
        return 1

    # 1. The historical crash fixture (Drop + mixed-size fields) executes fully.
    drop_run = run([str(binary), "run", str(DROP_EXAMPLE)])
    require_contains(
        drop_run.stdout,
        [
            "[Buffer] Abrindo 'log.txt' (1024 bytes)",
            "[Buffer] Fechando 'log.txt' e liberando 1024 bytes",
            "[Connection] Encerrando conexão com db.local:5432",
            "[TempFile] Removendo arquivo temporário: /tmp/spec_12345.tmp",
            "=== Todos os destrutores executados ===",
        ],
        "examples/test_oop_drop.spectra",
    )

    # 2. The R-207 regression fixture: mixed-size fields, mutation, nesting, drops.
    fixture_run = run([str(binary), "run", str(FIXTURE)])
    require_contains(
        fixture_run.stdout,
        [
            "[buffer] size=1024 flag=true code=65 score=2.25",
            "[buffer] size=2048 flag=false code=66 score=2.25",
            "[R-207 drop] buffer (2048)",
            "[inner/81/false]",
            "=== R-207 done ===",
        ],
        "tests/validation/253_oop_struct_layout_drop.spectra",
    )

    # 3. The IR dump must use byte-offset field pointers (padded layout).
    ir_run = run([str(binary), "run", "--dump-ir", str(FIXTURE)])
    require_contains(
        ir_run.stdout,
        ["field_ptr %v", "alloca struct Mixed"],
        "--dump-ir output",
    )

    # 4. Mixed-size fields use distinct padded offsets: bool@16, char@20, float@24.
    for expected_offset in (16, 20, 24):
        if not re.search(rf"field_ptr %v\d+, {expected_offset}\b", ir_run.stdout):
            print(
                f"[R-207] missing padded offset {expected_offset} in --dump-ir output",
                file=sys.stderr,
            )
            return 1

    # 5. Compile-level check of the fixture (harness equivalence).
    compile_run = run([str(binary), "compile", str(FIXTURE)])

    report = {
        "item": "R-207",
        "fixture": str(FIXTURE),
        "drop_example": str(DROP_EXAMPLE),
        "drop_example_exit": drop_run.returncode,
        "fixture_exit": fixture_run.returncode,
        "compile_exit": compile_run.returncode,
        "ir_has_field_ptr": "field_ptr" in ir_run.stdout,
        "ir_has_padded_offsets": all(
            re.search(rf"field_ptr %v\d+, {off}\b", ir_run.stdout)
            for off in (16, 20, 24)
        ),
        "status": "passed",
    }
    report_path = Path(args.report)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"[R-207] report written to {report_path}")
    print("[R-207] struct layout and drop semantics validated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
