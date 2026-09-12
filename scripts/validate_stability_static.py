#!/usr/bin/env python3
"""Validate the user-visible cross-module static JIT/AOT contract."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import tempfile
from pathlib import Path


SCHEMA = "spectralang.stability.static_aot.v1"


def run(command: list[str], cwd: Path, timeout: int) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=cwd,
        text=True,
        capture_output=True,
        timeout=timeout,
        check=False,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default="target/debug/spectralang.exe")
    parser.add_argument(
        "--project", default="tests/projects/valid/stability_static_cross_module"
    )
    parser.add_argument(
        "--negative",
        default="tests/errors/stability_static_dynamic_initializer.spectra",
    )
    parser.add_argument("--report", default="target/stability/static-aot.json")
    parser.add_argument("--timeout", type=int, default=180)
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    binary = (root / args.binary).resolve()
    project = (root / args.project).resolve()
    report_path = (root / args.report).resolve()
    report_path.parent.mkdir(parents=True, exist_ok=True)

    result: dict[str, object] = {
        "schema": SCHEMA,
        "binary": str(binary),
        "project": str(project),
        "status": "failed",
    }

    if not binary.is_file():
        result["error"] = f"missing CLI binary: {binary}"
    elif not project.is_dir():
        result["error"] = f"missing project fixture: {project}"
    else:
        with tempfile.TemporaryDirectory(
            prefix="spectralang-static-aot-", dir=report_path.parent
        ) as temp_dir:
            executable = Path(temp_dir) / (
                "stability_static_cross_module.exe"
                if os.name == "nt"
                else "stability_static_cross_module"
            )
            jit_result = run(
                [str(binary), "run", str(project)],
                root,
                args.timeout,
            )
            result["jit"] = {
                "exit_code": jit_result.returncode,
                "stdout_tail": jit_result.stdout[-2000:],
                "stderr_tail": jit_result.stderr[-2000:],
            }
            compile_result = run(
                [
                    str(binary),
                    "compile",
                    str(project),
                    "--emit-exe",
                    str(executable),
                ],
                root,
                args.timeout,
            )
            result["compile"] = {
                "exit_code": compile_result.returncode,
                "stdout_tail": compile_result.stdout[-2000:],
                "stderr_tail": compile_result.stderr[-2000:],
            }
            if compile_result.returncode == 0 and executable.is_file():
                run_result = run([str(executable)], root, args.timeout)
                result["execute"] = {
                    "exit_code": run_result.returncode,
                    "stdout_tail": run_result.stdout[-2000:],
                    "stderr_tail": run_result.stderr[-2000:],
                }
                result["equivalence"] = {
                    "exit_code_equal": jit_result.returncode == run_result.returncode,
                    "stdout_equal": jit_result.stdout.strip() == run_result.stdout.strip(),
                }
                if (
                    jit_result.returncode == 0
                    and run_result.returncode == 0
                    and result["equivalence"]["exit_code_equal"]
                    and result["equivalence"]["stdout_equal"]
                ):
                    result["status"] = "passed"
            else:
                result["error"] = "cross-module AOT compilation did not produce an executable"

    negative = (root / args.negative).resolve()
    if negative.is_file():
        negative_result = run([str(binary), "check", "--json", str(negative)], root, args.timeout)
        result["negative"] = {
            "path": str(negative),
            "exit_code": negative_result.returncode,
            "stdout_tail": negative_result.stdout[-2000:],
            "stderr_tail": negative_result.stderr[-2000:],
            "rejected": negative_result.returncode != 0,
            "diagnostic_code_present": "E2902" in negative_result.stdout,
        }
        if (
            negative_result.returncode == 0
            or "E2902" not in negative_result.stdout
        ):
            result["status"] = "failed"
            result["error"] = "dynamic static initializer was not rejected with E2902"
    else:
        result["status"] = "failed"
        result["error"] = f"missing negative fixture: {negative}"

    report_path.write_text(
        json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
