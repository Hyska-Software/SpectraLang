"""Execute the recursion algorithm regression suite.

The fixtures added for the recursion work (tests/validation/511-526) are
compile-gated by the broad PowerShell runner.  This gate executes each of
them through both pipelines and checks the process exit status:

- JIT:   `spectralang run <fixture>`
- AOT:   `spectralang compile --emit-exe --debug-info=none <fixture>` then
          running the produced executable

A fixture passes only when both pipelines exit 0.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REPORT_DIR = ROOT / "target" / "recursion-suite"
FIXTURE_NAMES = (
    "511_recursion_linear_algorithms.spectra",
    "512_recursion_tree_algorithms.spectra",
    "513_recursion_tail_accumulators.spectra",
    "514_recursion_mutual.spectra",
    "515_recursion_divide_and_conquer.spectra",
    "516_recursion_quicksort_partition.spectra",
    "517_recursion_backtracking_queens.spectra",
    "518_recursion_backtracking_permutations.spectra",
    "519_recursion_tower_of_hanoi.spectra",
    "520_recursion_memoization.spectra",
    "521_recursion_linked_list_enum.spectra",
    "522_recursion_binary_tree_enum.spectra",
    "523_recursion_generic_structural.spectra",
    "524_recursion_recursive_descent_parser.spectra",
    "525_recursion_graph_traversal.spectra",
    "526_recursion_parameter_promotion.spectra",
)
SCHEMA = "spectralang.recursion_suite.v1"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default="target/debug/spectralang.exe")
    parser.add_argument("--report", default=str(REPORT_DIR / "report.json"))
    parser.add_argument("--timeout", type=float, default=180.0)
    return parser.parse_args()


def resolve_binary(raw: str) -> Path:
    binary = Path(raw)
    if not binary.is_absolute():
        binary = ROOT / binary
    return binary.resolve()


def run(command: list[str], timeout_seconds: float) -> subprocess.CompletedProcess[str] | None:
    try:
        return subprocess.run(
            command,
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return None


def main() -> int:
    args = parse_args()
    binary = resolve_binary(args.binary)
    if not binary.is_file():
        print(f"binary not found: {binary}", file=sys.stderr)
        return 2

    fixtures = [ROOT / "tests" / "validation" / name for name in FIXTURE_NAMES]
    missing = [path.relative_to(ROOT).as_posix() for path in fixtures if not path.is_file()]
    if missing:
        print("missing fixtures:", *missing, sep="\n  ", file=sys.stderr)
        return 2

    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    cases: list[dict[str, object]] = []

    for fixture in fixtures:
        case: dict[str, object] = {
            "fixture": fixture.relative_to(ROOT).as_posix(),
        }

        jit = run([str(binary), "run", str(fixture)], args.timeout)
        if jit is None:
            case["jit"] = {"status": "timeout"}
        else:
            case["jit"] = {
                "status": "passed" if jit.returncode == 0 else "failed",
                "exit_code": jit.returncode,
                "detail": (jit.stdout + "\n" + jit.stderr).strip()[-2000:],
            }

        executable = REPORT_DIR / f"{fixture.stem}.exe"
        compiled = run(
            [
                str(binary),
                "compile",
                "--emit-exe",
                str(executable),
                "--debug-info=none",
                str(fixture),
            ],
            args.timeout,
        )
        if compiled is None:
            case["aot"] = {"status": "timeout"}
        elif compiled.returncode != 0 or not executable.is_file():
            case["aot"] = {
                "status": "failed",
                "exit_code": compiled.returncode,
                "detail": (compiled.stdout + "\n" + compiled.stderr).strip()[-2000:],
            }
        else:
            executed = run([str(executable)], args.timeout)
            if executed is None:
                case["aot"] = {"status": "timeout"}
            else:
                case["aot"] = {
                    "status": "passed" if executed.returncode == 0 else "failed",
                    "exit_code": executed.returncode,
                    "detail": (executed.stdout + "\n" + executed.stderr).strip()[-2000:],
                }

        passed = (
            case["jit"]["status"] == "passed" and case["aot"]["status"] == "passed"
        )
        case["status"] = "passed" if passed else "failed"
        cases.append(case)

    failed = [case for case in cases if case["status"] != "passed"]
    report = {
        "schema": SCHEMA,
        "fixture_count": len(cases),
        "passed": len(cases) - len(failed),
        "failed": len(failed),
        "cases": cases,
    }
    report_path = Path(args.report)
    if not report_path.is_absolute():
        report_path = ROOT / report_path
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    print(f"recursion suite: {report['passed']}/{report['fixture_count']} passed (JIT + AOT)")
    if failed:
        for case in failed:
            print(f"FAIL {case['fixture']}", file=sys.stderr)
        return 1
    print(f"report: {report_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
