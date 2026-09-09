#!/usr/bin/env python3
"""Capture the K-00 stability baseline without hiding skipped environments."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def git_revision() -> str:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    return result.stdout.strip() if result.returncode == 0 else "unknown"


def run(command: list[str], timeout: int) -> dict[str, object]:
    try:
        result = subprocess.run(
            command,
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        output = "\n".join((exc.stdout or "").splitlines()[-30:])
        return {
            "command": command,
            "exit_code": None,
            "status": "timeout",
            "output_tail": output,
        }

    output = f"{result.stdout}\n{result.stderr}".strip()
    if "skipped_environment" in output:
        status = "skipped_environment"
    elif result.returncode == 0:
        status = "passed"
    else:
        status = "failed"
    return {
        "command": command,
        "exit_code": result.returncode,
        "status": status,
        "output_tail": "\n".join(output.splitlines()[-30:]),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default="target/debug/spectralang.exe")
    parser.add_argument("--report", default="target/stability/baseline.json")
    parser.add_argument("--timeout-seconds", type=int, default=900)
    args = parser.parse_args()

    binary = str((ROOT / args.binary).resolve())
    commands = [
        ["cargo", "test", "--workspace", "--all-targets", "--no-fail-fast"],
        ["cargo", "fmt", "--all", "--", "--check"],
        ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
        ["python", "scripts/validate_r3007_stdlib_contract.py", "--binary", binary, "--report", "target/stability/baseline-r3007.json"],
        ["python", "scripts/validate_stdlib_core_bug_hunt.py", "--binary", binary],
        ["python", "scripts/validate_r2003_base_regression_audit.py", "--binary", binary],
        ["python", "scripts/validate_language_guide.py", "--binary", binary],
        ["python", "scripts/validate_r2107_async_stdlib.py"],
        ["python", "scripts/validate_r2207_tls_rustls.py"],
        ["python", "scripts/validate_r2505_postgres.py", "--report", "target/stability/baseline-postgres.json"],
        ["python", "scripts/validate_r2507_redis.py", "--report", "target/stability/baseline-redis.json"],
    ]

    results = [run(command, args.timeout_seconds) for command in commands]
    report = {
        "schema": "spectralang.stability.baseline.v1",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "git_revision": git_revision(),
        "binary": args.binary,
        "binary_exists": Path(binary).is_file(),
        "platform": os.name,
        "results": results,
        "status": "passed" if all(item["status"] in {"passed", "skipped_environment"} for item in results) else "failed",
        "release_certifying": all(item["status"] == "passed" for item in results),
        "external_note": "skipped_environment is diagnostic evidence only and cannot certify release stability.",
    }
    report_path = (ROOT / args.report).resolve()
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"status": report["status"], "release_certifying": report["release_certifying"], "report": str(report_path)}, indent=2))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
