#!/usr/bin/env python3
"""Validate the executable core-stdlib bug-hunt matrix.

The report contains only deterministic facts: fixture paths, attempt counts,
exit codes, classifications and stable schema values. Runtime output is shown
only when a command fails and is not copied into the report.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = "spectralang.stdlib_core_bug_hunt.v1"
STDLIB_IDS = list(range(317, 330))
ASYNC_IDS = list(range(293, 298))
FORBIDDEN_NAMESPACES = ("std.api", "std.tensor", "std.ml")


class ValidationFailure(RuntimeError):
    pass


def relative(path: Path) -> str:
    resolved = path.resolve()
    if resolved.is_relative_to(ROOT.resolve()):
        return resolved.relative_to(ROOT.resolve()).as_posix()
    return str(resolved)


def invoke(
    command: list[str], *, input_text: str | None = None, timeout: int = 60
) -> subprocess.CompletedProcess[str]:
    print(f"[stdlib-bug-hunt] {' '.join(command)}")
    try:
        return subprocess.run(
            command,
            cwd=ROOT,
            input=input_text,
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as exc:
        output = exc.stdout or ""
        raise ValidationFailure(
            f"timeout after {timeout}s: {' '.join(command)}\n{output}"
        ) from exc


def fixture_for(fixture_id: int) -> Path:
    matches = sorted((ROOT / "tests" / "validation").glob(f"{fixture_id}_*.spectra"))
    if len(matches) != 1:
        raise ValidationFailure(
            f"expected exactly one promoted fixture for {fixture_id}, found {matches}"
        )
    return matches[0]


def first_json_object(output: str) -> dict[str, object]:
    for line in output.splitlines():
        candidate = line.strip()
        if candidate.startswith("{") and candidate.endswith("}"):
            payload = json.loads(candidate)
            if isinstance(payload, dict):
                return payload
    raise ValidationFailure(f"CLI did not emit JSON diagnostics:\n{output[-4000:]}")


def check_fixture(binary: Path, path: Path) -> dict[str, object]:
    result = invoke([str(binary), "check", "--json", str(path)])
    if result.returncode != 0:
        raise ValidationFailure(
            f"{path.name} check failed with exit {result.returncode}\n"
            f"{result.stdout[-4000:]}"
        )
    payload = first_json_object(result.stdout)
    if payload.get("success") is not True:
        raise ValidationFailure(f"{path.name} check reported success != true: {payload}")
    return {"check_exit": result.returncode, "check_success": True}


def run_fixture(binary: Path, path: Path) -> dict[str, object]:
    input_text = "first-line\nsecond-line\n" if path.name.startswith("322_") else None
    result = invoke([str(binary), "run", str(path)], input_text=input_text)
    if result.returncode != 0:
        raise ValidationFailure(
            f"{path.name} run failed with exit {result.returncode}\n"
            f"{result.stdout[-4000:]}"
        )
    return {
        "run_exit": result.returncode,
        "stdin_case": "two_lines" if input_text is not None else "none",
    }


def validate_group(
    binary: Path, ids: list[int], *, attempts: int, group: str
) -> dict[str, object]:
    result: dict[str, object] = {}
    for fixture_id in ids:
        path = fixture_for(fixture_id)
        source = path.read_text(encoding="utf-8")
        forbidden = [name for name in FORBIDDEN_NAMESPACES if name in source]
        if forbidden:
            raise ValidationFailure(
                f"{path.name} references excluded namespace(s): {forbidden}"
            )
        attempt_results: list[dict[str, object]] = []
        for attempt in range(1, attempts + 1):
            check_result = check_fixture(binary, path)
            run_result = run_fixture(binary, path)
            attempt_results.append(
                {"attempt": attempt, **check_result, **run_result}
            )
        result[str(fixture_id)] = {
            "fixture": relative(path),
            "group": group,
            "classification": "PASS",
            "attempts": attempt_results,
        }
    return result


def write_report(path: Path, report: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--binary", default=str(ROOT / "target" / "debug" / "spectralang.exe")
    )
    parser.add_argument(
        "--report",
        default=str(ROOT / "target" / "stdlib-core-bug-hunt" / "report.json"),
    )
    args = parser.parse_args()
    binary = Path(args.binary).resolve()
    report_path = Path(args.report)
    if not report_path.is_absolute():
        report_path = ROOT / report_path

    report: dict[str, object] = {
        "schema": SCHEMA,
        "status": "failed",
        "binary": relative(binary),
        "stdlib": {},
        "async_runtime": {},
        "classifications": {},
        "summary": {},
    }
    try:
        if not binary.exists():
            raise ValidationFailure(f"binary not found: {binary}")
        stdlib = validate_group(binary, STDLIB_IDS, attempts=3, group="stdlib")
        async_runtime = validate_group(
            binary, ASYNC_IDS, attempts=3, group="async_runtime"
        )
        report["stdlib"] = stdlib
        report["async_runtime"] = async_runtime
        report["classifications"] = {
            **{str(fixture_id): "PASS" for fixture_id in STDLIB_IDS},
            **{str(fixture_id): "PASS" for fixture_id in ASYNC_IDS},
        }
        report["summary"] = {
            "stdlib_fixture_count": len(STDLIB_IDS),
            "async_fixture_count": len(ASYNC_IDS),
            "attempts_per_fixture": 3,
            "classification_counts": {"PASS": len(STDLIB_IDS) + len(ASYNC_IDS)},
        }
        report["status"] = "passed"
    except (ValidationFailure, OSError, UnicodeError, ValueError, json.JSONDecodeError) as exc:
        report["error"] = str(exc)
        write_report(report_path, report)
        print(f"[stdlib-bug-hunt] {exc}", file=sys.stderr)
        print(f"[stdlib-bug-hunt] report written to {report_path}", file=sys.stderr)
        return 1

    write_report(report_path, report)
    print(f"[stdlib-bug-hunt] report written to {report_path}")
    print("[stdlib-bug-hunt] stdlib and async runtime matrix passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
