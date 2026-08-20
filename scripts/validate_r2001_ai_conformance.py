#!/usr/bin/env python3
"""Run the R-2001 AI conformance certification suite."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
REPORT_SCHEMA = "spectralang.ai_conformance_report.v1"
CONFORMANCE_VERSION = "R-2001/v1"
REQUIRED_CATEGORIES = {
    "compiler",
    "runtime",
    "tensors",
    "autodiff",
    "graph",
    "interop",
    "package",
    "serving",
    "tooling",
    "docs_examples",
}


@dataclass(frozen=True)
class Gate:
    category: str
    name: str
    command: list[str]
    timeout_seconds: int


def cargo_cmd() -> str:
    configured = os.environ.get("CARGO")
    if configured:
        return configured
    found = shutil.which("cargo")
    if found:
        return found
    windows_default = Path.home() / ".cargo" / "bin" / "cargo.exe"
    if windows_default.exists():
        return str(windows_default)
    return "cargo"


def cargo_cli(*args: str) -> list[str]:
    return [cargo_cmd(), "run", "-q", "-p", "spectra-cli", "--", *args]


def python_script(script: str, *args: str) -> list[str]:
    return [sys.executable, script, *args]


GATES = [
    Gate("compiler", "compiler_unit_tests", [cargo_cmd(), "test", "-q", "-p", "spectra-compiler"], 120),
    Gate("compiler", "midend_unit_tests", [cargo_cmd(), "test", "-q", "-p", "spectra-midend"], 120),
    Gate("runtime", "runtime_unit_tests", [cargo_cmd(), "test", "-q", "-p", "spectra-runtime"], 120),
    Gate("runtime", "cli_runtime_smoke", cargo_cli("run", "tests/validation/01_basic_syntax.spectra"), 30),
    Gate("tensors", "tensor_core_surface", cargo_cli("run", "tests/validation/66_tensor_core_surface.spectra"), 30),
    Gate("tensors", "tensor_float_surface", cargo_cli("run", "tests/validation/67_tensor_float_surface.spectra"), 30),
    Gate("tensors", "tensor_kernel_surface", cargo_cli("run", "tests/validation/68_tensor_phase4_kernels.spectra"), 30),
    Gate("tensors", "numerical_correctness", python_script("scripts/validate_r1503_correctness.py"), 120),
    Gate("autodiff", "autodiff_surface", cargo_cli("run", "tests/validation/71_tensor_phase5_autodiff.spectra"), 30),
    Gate("autodiff", "diff_block_gradient_coverage", cargo_cli("run", "tests/validation/82_diff_block_gradient_coverage.spectra"), 30),
    Gate("graph", "tensor_graph_ir_and_optimization", [cargo_cmd(), "test", "-q", "-p", "spectra-midend", "tensor_graph"], 120),
    Gate("interop", "interop_unit_tests", [cargo_cmd(), "test", "-q", "-p", "spectra-interop"], 120),
    Gate("interop", "lsp_unit_tests", [cargo_cmd(), "test", "-q", "-p", "spectra-lsp"], 120),
    Gate("package", "package_workspace_check", cargo_cli("package", "check", "--root", "tests/projects/valid/package_workspace"), 60),
    Gate("package", "package_workspace_doc", cargo_cli("package", "doc", "--root", "tests/projects/valid/package_workspace"), 60),
    Gate("serving", "serving_foundations", cargo_cli("run", "tests/validation/78_serving_foundations.spectra"), 30),
    Gate("serving", "serving_safety_guardrails", cargo_cli("run", "tests/validation/99_phase19_ai_safety_guardrails.spectra"), 30),
    Gate("serving", "serving_monitoring_drift", cargo_cli("run", "tests/validation/100_phase19_model_monitoring.spectra"), 30),
    Gate("tooling", "cli_help", cargo_cli("--help"), 30),
    Gate("tooling", "diagnostic_standardization", python_script("scripts/validate_diagnostics_standardization.py"), 60),
    Gate("tooling", "feature_maturity_policy", python_script("scripts/validate_feature_maturity.py", "--binary", "target/debug/spectralang.exe"), 60),
    Gate("docs_examples", "ai_book_validation", python_script("scripts/validate_ai_book.py"), 60),
    Gate(
        "docs_examples",
        "ai_examples_benchmark",
        python_script(
            "scripts/ai_examples_benchmark.py",
            "--out",
            "target/r2001-conformance/ai-examples-benchmark.json",
            "--timeout-seconds",
            "20",
        ),
        180,
    ),
]


def gates_for_binary(binary: Path) -> list[Gate]:
    cargo_prefix = [cargo_cmd(), "run", "-q", "-p", "spectra-cli", "--"]
    resolved = str(binary.resolve())
    configured: list[Gate] = []
    for gate in GATES:
        command = gate.command
        if command[: len(cargo_prefix)] == cargo_prefix:
            command = [resolved, *command[len(cargo_prefix):]]
        elif gate.name == "feature_maturity_policy":
            command = [*command[:-1], resolved]
        elif gate.name == "ai_examples_benchmark":
            # The benchmark has an explicit binary mode.  When the
            # conformance runner receives --binary, use it here as well so
            # the gate measures the already-built candidate instead of
            # recompiling the entire workspace once per example.
            command = [*command, "--binary", resolved]
        configured.append(Gate(gate.category, gate.name, command, gate.timeout_seconds))
    return configured


def command_text(command: list[str]) -> str:
    return " ".join(command)


def run_gate(gate: Gate) -> dict[str, Any]:
    print(f"[R-2001] {gate.category}/{gate.name}: {command_text(gate.command)}")
    start = time.perf_counter()
    env = os.environ.copy()
    cargo_dir = str(Path(cargo_cmd()).parent)
    env["PATH"] = cargo_dir + os.pathsep + env.get("PATH", "")
    try:
        completed = subprocess.run(
            gate.command,
            cwd=ROOT,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=gate.timeout_seconds,
            check=False,
        )
        elapsed_ms = int((time.perf_counter() - start) * 1000)
        output = completed.stdout or ""
        status = "passed" if completed.returncode == 0 else "failed"
        return {
            "category": gate.category,
            "name": gate.name,
            "status": status,
            "exit_code": completed.returncode,
            "elapsed_ms": elapsed_ms,
            "timeout_seconds": gate.timeout_seconds,
            "command": gate.command,
            "output_tail": "\n".join(output.splitlines()[-30:]),
        }
    except subprocess.TimeoutExpired as exc:
        elapsed_ms = int((time.perf_counter() - start) * 1000)
        output = exc.stdout if isinstance(exc.stdout, str) else ""
        return {
            "category": gate.category,
            "name": gate.name,
            "status": "timeout",
            "exit_code": None,
            "elapsed_ms": elapsed_ms,
            "timeout_seconds": gate.timeout_seconds,
            "command": gate.command,
            "output_tail": "\n".join(output.splitlines()[-30:]) if output else str(exc),
        }


def git_revision() -> str:
    completed = subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return completed.stdout.strip() if completed.returncode == 0 else "unknown"


def build_report(gate_results: list[dict[str, Any]], release_candidate: str) -> dict[str, Any]:
    categories: dict[str, dict[str, Any]] = {}
    for category in sorted(REQUIRED_CATEGORIES):
        records = [result for result in gate_results if result["category"] == category]
        categories[category] = {
            "status": "passed" if records and all(result["status"] == "passed" for result in records) else "failed",
            "gate_count": len(records),
            "passed": sum(1 for result in records if result["status"] == "passed"),
            "failed": sum(1 for result in records if result["status"] != "passed"),
        }

    overall_passed = all(result["status"] == "passed" for result in gate_results)
    missing_categories = sorted(category for category, summary in categories.items() if summary["gate_count"] == 0)
    certified = overall_passed and not missing_categories
    return {
        "schema": REPORT_SCHEMA,
        "conformance_version": CONFORMANCE_VERSION,
        "release_candidate": release_candidate,
        "candidate_status": "certified" if certified else "rejected",
        "certified": certified,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "git_revision": git_revision(),
        "required_categories": sorted(REQUIRED_CATEGORIES),
        "missing_categories": missing_categories,
        "categories": categories,
        "gates": gate_results,
    }


def validate_report(report: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    if report.get("schema") != REPORT_SCHEMA:
        errors.append("bad report schema")
    if report.get("conformance_version") != CONFORMANCE_VERSION:
        errors.append("bad conformance version")

    categories = report.get("categories")
    if not isinstance(categories, dict):
        errors.append("missing categories object")
        return errors

    category_names = set(categories)
    missing = REQUIRED_CATEGORIES - category_names
    if missing:
        errors.append(f"missing required categories: {', '.join(sorted(missing))}")

    for category in REQUIRED_CATEGORIES:
        summary = categories.get(category, {})
        if summary.get("gate_count", 0) < 1:
            errors.append(f"category has no gates: {category}")

    gates = report.get("gates")
    if not isinstance(gates, list) or not gates:
        errors.append("missing gate results")
    else:
        for gate in gates:
            if gate.get("status") not in {"passed", "failed", "timeout"}:
                errors.append(f"gate has invalid status: {gate.get('name')}")

    if report.get("certified") != all(gate.get("status") == "passed" for gate in gates or []):
        errors.append("certified flag does not match gate results")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", default="target/r2001-conformance/conformance-report.json")
    parser.add_argument("--release-candidate", default="local-working-tree")
    parser.add_argument("--binary", default="target/debug/spectralang.exe")
    parser.add_argument("--keep-going", action="store_true", help="run every gate even after failures")
    args = parser.parse_args()

    binary = (ROOT / args.binary).resolve() if not Path(args.binary).is_absolute() else Path(args.binary)
    if not binary.exists():
        print(f"[R-2001] ERROR: binary not found: {binary}")
        return 2

    gate_results: list[dict[str, Any]] = []
    for gate in gates_for_binary(binary):
        result = run_gate(gate)
        gate_results.append(result)
        if result["status"] != "passed":
            print(f"[R-2001] FAILED: {gate.category}/{gate.name}")
            if not args.keep_going:
                break

    report = build_report(gate_results, args.release_candidate)
    errors = validate_report(report)
    if errors:
        report["report_validation_errors"] = errors
        report["certified"] = False
        report["candidate_status"] = "rejected"

    out = (ROOT / args.out).resolve()
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"[R-2001] wrote {out}")

    if errors:
        for error in errors:
            print(f"[R-2001] ERROR: {error}")
    if not report["certified"]:
        print("[R-2001] conformance certification rejected")
        return 1

    print("[R-2001] conformance certification passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
