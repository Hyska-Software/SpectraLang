#!/usr/bin/env python3
"""Run the executable SpectraLang language bug-hunt round 2 matrix.

The validator intentionally records only stable facts (fixture ids, exit codes,
attempt counts and artifact sizes).  Command output is printed on failure but
is not copied into the report, keeping repeated reports comparable.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = "spectralang.language_bug_hunt.v2"
SOURCE_IDS = list(range(275, 290)) + list(range(293, 317))
PROJECTS = [
    "bug_hunt_v2_290_imported_aggregate",
    "bug_hunt_v2_291_public_reexport",
    "bug_hunt_v2_292_visibility_and_internal",
]
REPEAT_IDS = {
    284,
    285,
    293,
    294,
    295,
    296,
    297,
    299,
    301,
    302,
    303,
    304,
    305,
    306,
    307,
    310,
    313,
}
AOT_IDS = {288, 311, 316}
NEGATIVE_FIXTURES = {"bug_hunt_v2_invalid_json.spectra": {"EJSON001"}}


class ValidationFailure(RuntimeError):
    pass


def relative(path: Path) -> str:
    return path.resolve().relative_to(ROOT.resolve()).as_posix()


def invoke(command: list[str], timeout: int = 180) -> subprocess.CompletedProcess[str]:
    print(f"[bug-hunt-v2] {' '.join(command)}")
    try:
        return subprocess.run(
            command,
            cwd=ROOT,
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as exc:
        raise ValidationFailure(
            f"timeout after {timeout}s: {' '.join(command)}\n{exc.stdout or ''}"
        ) from exc


def require_success(result: subprocess.CompletedProcess[str], label: str) -> None:
    if result.returncode != 0:
        raise ValidationFailure(
            f"{label} failed with exit {result.returncode}\n{result.stdout[-5000:]}"
        )


def source_for(source_id: int) -> Path:
    validation = ROOT / "tests" / "validation"
    matches = sorted(validation.glob(f"{source_id}_*.spectra"))
    if len(matches) != 1:
        raise ValidationFailure(
            f"expected exactly one promoted fixture for {source_id}, found {matches}"
        )
    return matches[0]


def validate_sources(binary: Path) -> dict[str, object]:
    result: dict[str, object] = {}
    for source_id in SOURCE_IDS:
        path = source_for(source_id)
        attempts: list[dict[str, int]] = []
        count = 3 if source_id in REPEAT_IDS else 1
        for attempt in range(1, count + 1):
            checked = invoke([str(binary), "check", str(path)])
            require_success(checked, f"{path.name} check attempt {attempt}")
            executed = invoke([str(binary), "run", str(path)])
            require_success(executed, f"{path.name} run attempt {attempt}")
            attempts.append({"attempt": attempt, "check_exit": 0, "run_exit": 0})
        result[str(source_id)] = {
            "fixture": relative(path),
            "classification": "PASS",
            "attempts": attempts,
        }
    return result


def validate_projects(binary: Path) -> dict[str, object]:
    result: dict[str, object] = {}
    root = ROOT / "tests" / "projects" / "valid"
    for name in PROJECTS:
        path = root / name
        if not (path / "spectra.toml").exists():
            raise ValidationFailure(f"missing multifile project manifest: {path}")
        checked = invoke([str(binary), "check", str(path)])
        require_success(checked, f"{name} check")
        executed = invoke([str(binary), "run", str(path)])
        require_success(executed, f"{name} run")
        result[name] = {
            "classification": "PASS",
            "check_exit": 0,
            "run_exit": 0,
            "files": sorted(relative(item) for item in path.rglob("*.spectra")),
        }
    return result


def validate_aot(binary: Path) -> dict[str, object]:
    output_dir = ROOT / "target" / "language-bug-hunt-v2"
    output_dir.mkdir(parents=True, exist_ok=True)
    result: dict[str, object] = {}
    for source_id in sorted(AOT_IDS):
        path = source_for(source_id)
        object_path = output_dir / f"{source_id}.obj"
        executable_path = output_dir / f"{source_id}.exe"
        object_result = invoke(
            [str(binary), "compile", "--emit-object", str(object_path), str(path)]
        )
        require_success(object_result, f"{path.name} AOT object")
        executable_result = invoke(
            [str(binary), "compile", "--emit-exe", str(executable_path), str(path)]
        )
        require_success(executable_result, f"{path.name} AOT executable")
        if not object_path.exists() or object_path.stat().st_size == 0:
            raise ValidationFailure(f"{path.name}: empty AOT object")
        if not executable_path.exists() or executable_path.stat().st_size == 0:
            raise ValidationFailure(f"{path.name}: empty AOT executable")
        execution = invoke([str(executable_path)])
        require_success(execution, f"{path.name} AOT execution")
        result[str(source_id)] = {
            "fixture": relative(path),
            "classification": "PASS",
            "object_exit": 0,
            "executable_exit": 0,
            "run_exit": 0,
            "object_bytes": object_path.stat().st_size,
            "executable_bytes": executable_path.stat().st_size,
        }
    return result


def first_json_object(output: str) -> dict[str, object]:
    for line in output.splitlines():
        line = line.strip()
        if line.startswith("{") and line.endswith("}"):
            return json.loads(line)
    raise ValidationFailure(f"CLI did not emit JSON diagnostics:\n{output[-5000:]}")


def validate_negative(binary: Path) -> dict[str, object]:
    result: dict[str, object] = {}
    root = ROOT / "tests" / "errors"
    for name, expected in sorted(NEGATIVE_FIXTURES.items()):
        path = root / name
        if not path.exists():
            raise ValidationFailure(f"missing negative fixture: {path}")
        checked = invoke([str(binary), "check", "--json", str(path)])
        if checked.returncode == 0:
            raise ValidationFailure(f"{name}: malformed program unexpectedly passed")
        payload = first_json_object(checked.stdout)
        if payload.get("success") is not False:
            raise ValidationFailure(f"{name}: expected success=false, got {payload}")
        files = payload.get("files", [])
        diagnostics = files[0].get("diagnostics", []) if files else []
        actual = {
            diagnostic.get("code") or "semantic" for diagnostic in diagnostics
        }
        if not actual.intersection(expected):
            raise ValidationFailure(
                f"{name}: expected {sorted(expected)}, got {sorted(actual)}\n"
                f"{checked.stdout[-5000:]}"
            )
        result[name] = {
            "classification": "PASS",
            "check_exit": checked.returncode,
            "success": False,
            "expected_codes": sorted(expected),
            "actual_codes": sorted(actual),
        }
    return result


def validate_cli(binary: Path) -> dict[str, object]:
    path = source_for(314)
    formatted = invoke([str(binary), "fmt", "--check", str(path)])
    require_success(formatted, "314 fmt --check")
    stdout = invoke([str(binary), "fmt", "--stdout", str(path)])
    require_success(stdout, "314 fmt --stdout")
    if not stdout.stdout.strip():
        raise ValidationFailure("314 fmt --stdout returned empty output")
    lint = invoke([str(binary), "lint", "--json", str(path)])
    require_success(lint, "314 lint --json")
    payload = first_json_object(lint.stdout)
    if payload.get("success") is not True:
        raise ValidationFailure(f"314 lint --json was not successful: {payload}")
    return {
        "fixture": relative(path),
        "classification": "PASS",
        "fmt_check_exit": 0,
        "fmt_stdout_exit": 0,
        "lint_exit": 0,
        "lint_success": True,
    }


def validate_no_pending() -> list[str]:
    pending = ROOT / "tests" / "regressions" / "pending"
    if not pending.exists():
        return []
    return sorted(relative(path) for path in pending.rglob("*.spectra"))


def write_report(path: Path, report: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--binary", default=str(ROOT / "target" / "debug" / "spectralang.exe")
    )
    parser.add_argument(
        "--report", default=str(ROOT / "target" / "language-bug-hunt-v2" / "report.json")
    )
    args = parser.parse_args()
    binary = Path(args.binary).resolve()
    report_path = Path(args.report)
    if not report_path.is_absolute():
        report_path = ROOT / report_path

    report: dict[str, object] = {
        "schema": SCHEMA,
        "status": "failed",
        "binary": relative(binary) if binary.is_relative_to(ROOT) else str(binary),
        "sources": {},
        "projects": {},
        "aot": {},
        "cli": {},
        "negative": {},
        "pending": [],
        "classifications": {},
    }
    try:
        if not binary.exists():
            raise ValidationFailure(f"binary not found: {binary}")
        pending = validate_no_pending()
        if pending:
            raise ValidationFailure(f"pending fixtures remain: {pending}")
        report["sources"] = validate_sources(binary)
        report["projects"] = validate_projects(binary)
        report["aot"] = validate_aot(binary)
        report["cli"] = validate_cli(binary)
        report["negative"] = validate_negative(binary)
        report["pending"] = []
        report["classifications"] = {
            **{str(source_id): "PASS" for source_id in SOURCE_IDS},
            **{name: "PASS" for name in PROJECTS},
        }
        report["summary"] = {
            "source_count": len(SOURCE_IDS),
            "project_count": len(PROJECTS),
            "aot_count": len(AOT_IDS),
            "negative_count": len(NEGATIVE_FIXTURES),
            "classification_counts": {"PASS": len(SOURCE_IDS) + len(PROJECTS)},
        }
        report["status"] = "passed"
    except (ValidationFailure, OSError, ValueError, json.JSONDecodeError) as exc:
        report["error"] = str(exc)
        write_report(report_path, report)
        print(f"[bug-hunt-v2] {exc}", file=sys.stderr)
        print(f"[bug-hunt-v2] report written to {report_path}", file=sys.stderr)
        return 1

    write_report(report_path, report)
    print(f"[bug-hunt-v2] report written to {report_path}")
    print("[bug-hunt-v2] 42 promoted candidates and the negative diagnostic gate passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
