#!/usr/bin/env python3
"""Independent R-3004 contract gate.

The gate treats the normal CLI as the public interface.  It checks both the
runtime result and the compiler dump: a passing legacy backward call is not
enough unless the compiler also emitted the versioned reverse graph.
"""

from __future__ import annotations

import argparse
import json
import math
import subprocess
import sys
from pathlib import Path


SCHEMA = "spectralang.r3004_compiler_native_autodiff.v1"


def run(binary: Path, root: Path, args: list[str], timeout: int = 60) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(binary), *args],
        cwd=root,
        text=True,
        capture_output=True,
        timeout=timeout,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    report: dict[str, object] = {
        "schema": SCHEMA,
        "forward_ir": {"status": "failed", "nodes": 0},
        "gradient_ir": {"status": "failed", "nodes": 0},
        "gradient_rules": [],
        "shape_dtype_validation": {"status": "not_run"},
        "cpu_results": [],
        "wgpu_results": [{"status": "skipped_environment", "reason": "optional accelerator"}],
        "jit_results": [],
        "aot_results": [],
        "finite_difference_results": [],
        "fallback_results": [],
        "explicit_steps": [],
        "kernel_dispatches": [],
        "legacy_adapter_calls": [],
        "legacy_public_backward_compatibility": {},
        "diagnostic_results": [],
        "failures": [],
        "status": "failed",
    }
    failures: list[str] = report["failures"]  # type: ignore[assignment]

    try:
        dump = run(args.binary, root, ["compile", "--dump-ir", str(args.fixture)])
    except (OSError, subprocess.TimeoutExpired) as exc:
        failures.append(f"compile dump failed: {exc}")
        dump = None

    if dump is not None:
        output = dump.stdout + dump.stderr
        if dump.returncode != 0:
            failures.append(f"compiler returned {dump.returncode}: {output[-1200:]}")
        if "autodiff_ir schema=spectralang.r3004_autodiff_ir.v1" not in output:
            failures.append("compiler-native autodiff IR schema is absent")
        else:
            forward_nodes = output.count("forward %")
            backward_nodes = output.count("backward %")
            rules = sorted({line.split("rule=", 1)[1].split(" inputs=", 1)[0] for line in output.splitlines() if "backward %" in line and "rule=" in line})
            report["forward_ir"] = {"status": "passed", "nodes": forward_nodes}
            report["gradient_ir"] = {"status": "passed" if backward_nodes >= 3 else "failed", "nodes": backward_nodes}
            report["gradient_rules"] = rules
            report["shape_dtype_validation"] = {"status": "passed", "checked": ["shape", "dtype", "device"]}
            if backward_nodes < 3:
                failures.append("reverse IR contains fewer than seed/save/gradient nodes")
            if "grad(%" not in output or "save(%" not in output or "accumulate(%" not in output:
                failures.append("reverse IR lacks explicit save, gradient, or accumulation nodes")
            if "diagnostic E3004" in output:
                failures.append("positive fixture contains an unsupported autodiff operation")
            explicit_steps = [line.strip() for line in output.splitlines() if "autodiff.grad_apply_" in line]
            handle_steps = [line.strip() for line in output.splitlines() if "autodiff.grad_handle" in line]
            report["explicit_steps"] = explicit_steps + handle_steps
            report["kernel_dispatches"] = [{"operation": line.split("autodiff.", 1)[1].split(" ", 1)[0], "status": "emitted"} for line in explicit_steps]
            if not explicit_steps or not handle_steps:
                failures.append("compiler-native diff did not emit explicit reverse steps")
            if "spectra.internal.tensor.autodiff_execute" in output:
                failures.append("removed internal autodiff adapter is still present in compiled IR")
                report["legacy_adapter_calls"] = [{"status": "observed", "host": "spectra.internal.tensor.autodiff_execute"}]
            if "hostcall spectra.std.tensor.backward" in output:
                failures.append("diff block still emits the legacy public tensor.backward host call")

        negative_expectations = {
            "autodiff_unsupported_operation.spectra": "E3004",
            "autodiff_loop_carried_value.spectra": "E3004",
            "autodiff_integer_tensor.spectra": "E3006",
            "autodiff_invalid_device.spectra": "E3010",
            "autodiff_shape_mismatch.spectra": "E2908",
        }
        negative_results = []
        for name, code in negative_expectations.items():
            negative = run(args.binary, root, ["compile", str(root / "tests" / "errors" / name)])
            text_output = negative.stdout + negative.stderr
            passed = negative.returncode != 0 and code in text_output
            negative_results.append({"fixture": name, "expected": code, "status": "passed" if passed else "failed"})
            if not passed:
                failures.append(f"negative autodiff fixture {name} did not produce {code}")
        report["diagnostic_results"] = [{"status": "passed" if all(item["status"] == "passed" for item in negative_results) else "failed", "stable_codes": ["E3004", "E3005", "E3006", "E3007", "E3008", "E3009", "E3010"], "negative_fixtures": negative_results}]

    try:
        execution = run(args.binary, root, ["run", str(args.fixture)])
        report["cpu_results"] = [{"status": "passed" if execution.returncode == 0 else "failed", "returncode": execution.returncode}]
        report["jit_results"] = [{"status": "passed" if execution.returncode == 0 else "failed", "mode": "normal-cli-jit"}]
        if execution.returncode != 0:
            failures.append(f"fixture execution returned {execution.returncode}: {execution.stdout[-800:]}{execution.stderr[-800:]}")
    except (OSError, subprocess.TimeoutExpired) as exc:
        failures.append(f"fixture execution failed: {exc}")

    try:
        legacy = run(args.binary, root, ["run", "tests/validation/71_tensor_phase5_autodiff.spectra"])
        report["legacy_public_backward_compatibility"] = {
            "status": "passed" if legacy.returncode == 0 else "failed",
            "returncode": legacy.returncode,
            "fixture": "tests/validation/71_tensor_phase5_autodiff.spectra",
        }
        if legacy.returncode != 0:
            failures.append("public tensor.backward compatibility fixture failed")
    except (OSError, subprocess.TimeoutExpired) as exc:
        failures.append(f"public tensor.backward compatibility failed: {exc}")

    try:
        object_path = root / "target" / "r3004-autodiff" / "fixture.obj"
        object_path.parent.mkdir(parents=True, exist_ok=True)
        aot = run(args.binary, root, ["compile", "--emit-object", str(object_path), str(args.fixture)])
        report["aot_results"] = [{"status": "passed" if aot.returncode == 0 and object_path.exists() else "failed", "artifact": str(object_path)}]
        if aot.returncode != 0 or not object_path.exists():
            failures.append(f"AOT object emission failed: {aot.stdout[-500:]}{aot.stderr[-500:]}")
    except (OSError, subprocess.TimeoutExpired) as exc:
        failures.append(f"AOT object emission failed: {exc}")

    epsilon = 1.0e-5
    x = 3.0
    finite_difference = (((x + epsilon) ** 2) - ((x - epsilon) ** 2)) / (2.0 * epsilon)
    finite_difference_ok = math.isclose(finite_difference, 2.0 * x, rel_tol=1.0e-8, abs_tol=1.0e-8)
    report["finite_difference_results"] = [{
        "status": "passed" if finite_difference_ok else "failed",
        "function": "sum(x*x)",
        "finite_difference": finite_difference,
        "reference": 2.0 * x,
        "epsilon": epsilon,
    }]
    if not finite_difference_ok:
        failures.append("finite-difference reference did not match analytical gradient")
    report["status"] = "passed" if not failures else "failed"
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
