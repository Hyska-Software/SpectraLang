#!/usr/bin/env python3
"""Validate the executable language bug-hunt matrix and its promoted contracts."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

POSITIVE_FIXTURES = [
    "258_oop_layout_permutations.spectra",
    "259_oop_drop_nested_lifetimes.spectra",
    "260_oop_dyn_aggregate_overwrite.spectra",
    "261_oop_vtable_inheritance_defaults.spectra",
    "262_oop_ufcs_receiver_matrix.spectra",
    "263_oop_generic_struct_multiarg.spectra",
    "264_oop_generic_trait_substitution.spectra",
    "268_pattern_generic_enum_oop.spectra",
    "273_api_handler_async_trait.spectra",
    "274_exact_width_aggregate_boundary.spectra",
]

EXPECTED_ERROR_CODES = {
    "api_handler_wrong_response.spectra": {"E023"},
    "async_oop_send_boundary.spectra": {"E2104"},
    "oop_duplicate_impl_method.spectra": {"E013"},
    "oop_dyn_generic_trait.spectra": {"E026"},
    "oop_generic_trait_arity.spectra": {"E025"},
    "oop_generic_trait_parameter_mismatch.spectra": {"E023"},
    "oop_generic_trait_return_mismatch.spectra": {"E023"},
    "oop_inherent_impl_wrong_arity.spectra": {"E025"},
    "oop_missing_trait_method.spectra": {"E016"},
    "oop_module_qualified_unknown_type.spectra": {"E027"},
    "oop_self_not_first_trait.spectra": {"E024"},
    "oop_ufcs_missing_method.spectra": {"E017"},
    "oop_ufcs_nonimplementor.spectra": {"E016"},
    "pattern_generic_non_exhaustive.spectra": {"semantic"},
    "tensor_oop_shape_mismatch.spectra": {"E1403"},
}

REPEATED_FIXTURES = {
    "259_oop_drop_nested_lifetimes.spectra": (
        "[R-259 drop] 1:local",
        "[R-259 drop] 2:bundle",
        "[R-259 drop] 3:returned",
    ),
}


def run(binary: Path, args: list[str]) -> subprocess.CompletedProcess[str]:
    command = [str(binary), *args]
    print(f"[bug-hunt] {' '.join(command)}")
    return subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=90,
    )


def first_json_object(output: str) -> dict:
    for line in output.splitlines():
        line = line.strip()
        if line.startswith("{") and line.endswith("}"):
            return json.loads(line)
    raise AssertionError(f"CLI did not emit JSON diagnostics: {output[-2000:]}")


def validate_positive(binary: Path, report: dict) -> None:
    validation_dir = ROOT / "tests" / "validation"
    for name in POSITIVE_FIXTURES:
        path = validation_dir / name
        if not path.exists():
            raise AssertionError(f"missing promoted positive fixture: {path}")

        checks: list[dict[str, object]] = []
        for attempt in range(3 if name in REPEATED_FIXTURES else 1):
            result = run(binary, ["check", str(path)])
            if result.returncode != 0:
                raise AssertionError(f"{name}: check failed (rc={result.returncode})\n{result.stdout}")
            execution = run(binary, ["run", str(path)])
            if execution.returncode != 0:
                raise AssertionError(f"{name}: run failed (rc={execution.returncode})\n{execution.stdout}")
            for marker in REPEATED_FIXTURES.get(name, ()):
                if marker not in execution.stdout:
                    raise AssertionError(f"{name}: missing runtime marker {marker!r}")
            checks.append({"attempt": attempt + 1, "check": 0, "run": 0})
        report[name] = {"status": "passed", "attempts": checks}


def validate_errors(binary: Path, report: dict) -> None:
    errors_dir = ROOT / "tests" / "errors"
    for name, expected_codes in sorted(EXPECTED_ERROR_CODES.items()):
        path = errors_dir / name
        if not path.exists():
            raise AssertionError(f"missing negative fixture: {path}")
        result = run(binary, ["check", "--json", str(path)])
        if result.returncode == 0:
            raise AssertionError(f"{name}: negative fixture unexpectedly succeeded")
        payload = first_json_object(result.stdout)
        if payload.get("success") is not False:
            raise AssertionError(f"{name}: expected success=false, got {payload}")
        diagnostics = payload.get("files", [{}])[0].get("diagnostics", [])
        actual_codes = {diagnostic.get("code") or "semantic" for diagnostic in diagnostics}
        if not actual_codes.intersection(expected_codes):
            raise AssertionError(
                f"{name}: expected one of {sorted(expected_codes)}, got {sorted(actual_codes)}\n"
                f"{result.stdout}"
            )
        report[name] = {
            "status": "passed",
            "exit": result.returncode,
            "expected_codes": sorted(expected_codes),
            "actual_codes": sorted(actual_codes),
        }


def validate_project(binary: Path, report: dict) -> None:
    project = ROOT / "tests" / "projects" / "valid" / "oop_cross_module_dispatch"
    check = run(binary, ["check", str(project)])
    if check.returncode != 0:
        raise AssertionError(f"cross-module project check failed\n{check.stdout}")
    execution = run(binary, ["run", str(project)])
    if execution.returncode != 0:
        raise AssertionError(f"cross-module project run failed\n{execution.stdout}")
    report["oop_cross_module_dispatch"] = {
        "status": "passed",
        "check": check.returncode,
        "run": execution.returncode,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default=str(ROOT / "target" / "debug" / "spectralang.exe"))
    parser.add_argument("--report", default=str(ROOT / "target" / "language-bug-hunt" / "report.json"))
    args = parser.parse_args()

    binary = Path(args.binary)
    if not binary.exists():
        print(f"[bug-hunt] binary not found: {binary}", file=sys.stderr)
        return 1

    report: dict[str, object] = {
        "schema": "spectralang.language_bug_hunt.v1",
        "positive": {},
        "negative": {},
        "project": {},
    }
    try:
        validate_positive(binary, report["positive"])
        validate_errors(binary, report["negative"])
        validate_project(binary, report["project"])
    except (AssertionError, subprocess.TimeoutExpired) as exc:
        print(f"[bug-hunt] {exc}", file=sys.stderr)
        return 1

    report["status"] = "passed"
    report_path = Path(args.report)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"[bug-hunt] report written to {report_path}")
    print("[bug-hunt] promoted positive, negative diagnostic, and cross-module project gates passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
