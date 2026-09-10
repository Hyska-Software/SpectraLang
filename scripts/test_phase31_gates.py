from __future__ import annotations

import subprocess
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

from scripts import validate_phase31_cross_lang as cross_lang
from scripts import validate_r1603_gpu_backend as gpu_backend
from scripts import phase31_run_all as phase31_runner
from scripts import compare_phase31_reports as report_comparator
from scripts import diagnose_async_echo as async_echo_diagnostics


def report(*, profile: str = "debug", stddev_ns: int = 5, ns_per_iter: int = 100) -> dict:
    scenarios = []
    for scenario_id in cross_lang.REQUIRED_SCENARIOS:
        entry = {
                "id": scenario_id,
                "correctness_passed": True,
                "results": {
                    language: {
                        "ns_per_iter": ns_per_iter,
                        "stddev_ns": stddev_ns,
                        "command": [language, "bench"],
                        "exit_code": 0,
                        "failure_class": None,
                    }
                    for language in phase31_runner.LANGUAGES
                },
        }
        if scenario_id == "async-echo":
            entry.update({
                "benchmark_contract": phase31_runner.ASYNC_ECHO_CONTRACT,
                "performance_reference": "go",
                "reference_runtime": "go",
                "gap_to_go": 1.0,
                "paired_gap_to_go": [1.0] * 5,
                "paired_gap_stddev_pct": 0.0,
                "reference_performance_passed": True,
                "tasks_per_iteration": 10,
                "benchmark_iterations": 10000,
                "expected_result": 550_000,
                "concurrency_metrics": {
                    "max_pending_tasks": 10,
                    "tasks_executed": 100_000,
                    "task_joins": 100_000,
                    "tasks_failed": 0,
                },
            })
        scenarios.append(entry)
    return {
        "schema": "spectra.phase31.bench.v1",
        "mode": "performance_certification",
        "profile": profile,
        "spectra_binary": str(Path("target/debug/spectralang.exe").resolve()),
        "scenario_matrix": list(cross_lang.SCENARIOS),
        "complete_scenario_set": True,
        "environment_preflight": {"status": "quiescent"},
        "measurement_policy": {
            "warmup_runs": 3,
            "timed_runs": 20,
            "max_stddev_pct": 10.0,
            "independent_runs": 5,
            "per_process_timeout_s": 300,
        },
        "scenarios": scenarios,
    }


def baseline() -> dict:
    return {
        "max_drift_pct": 15.0,
        "max_stddev_pct": 10.0,
        "scenarios": {
            scenario_id: {"spectra_ns_per_iter": 100, "placeholder": False}
            for scenario_id in cross_lang.REQUIRED_SCENARIOS
        },
    }


class Phase31GateTests(unittest.TestCase):
    def test_active_benchmark_matrix_excludes_java(self) -> None:
        self.assertEqual(phase31_runner.LANGUAGES, ("spectra", "go", "rust"))
        self.assertNotIn("java", phase31_runner.LANGUAGES)

    def test_independent_attempts_are_aggregated(self) -> None:
        attempts = [
            {
                "id": "sample",
                "category": "cpu",
                "iterations": 1,
                "results": {"spectra": {"ns_per_iter": value, "stddev_ns": 1}},
                "correctness_passed": True,
            }
            for value in (100, 120, 110)
        ]
        aggregated = phase31_runner.aggregate_scenario_attempts(attempts)
        self.assertEqual(aggregated["results"]["spectra"]["ns_per_iter"], 110)
        self.assertEqual(aggregated["results"]["spectra"]["independent_medians_ns"], [100, 120, 110])
        self.assertEqual(aggregated["independent_runs"], 3)

    def test_aggregation_preserves_and_filters_clear_outlier(self) -> None:
        attempts = [
            {
                "id": "sample",
                "category": "cpu",
                "iterations": 1,
                "results": {"spectra": {"ns_per_iter": value, "stddev_ns": 1}},
                "correctness_passed": True,
            }
            for value in (100, 101, 99, 100, 102, 1000, 98)
        ]
        aggregated = phase31_runner.aggregate_scenario_attempts(attempts)
        result = aggregated["results"]["spectra"]
        self.assertEqual(result["independent_medians_ns"], [100, 101, 99, 100, 102, 1000, 98])
        self.assertEqual(result["outlier_medians_ns"], [98, 99, 102, 1000])
        self.assertLess(result["independent_stddev_ns"], 3)

    def test_stable_report_passes(self) -> None:
        failures, inconclusive = cross_lang.check_baseline(baseline(), report())
        self.assertEqual(failures, [])
        self.assertEqual(inconclusive, [])

    def test_noisy_report_is_inconclusive(self) -> None:
        failures, inconclusive = cross_lang.check_baseline(
            baseline(), report(stddev_ns=11)
        )
        self.assertEqual(failures, [])
        self.assertTrue(inconclusive)

    def test_real_regression_remains_failure(self) -> None:
        failures, inconclusive = cross_lang.check_baseline(
            baseline(), report(stddev_ns=5, ns_per_iter=120)
        )
        self.assertTrue(failures)
        self.assertEqual(inconclusive, [])

    def test_async_echo_requires_go_reference_parity(self) -> None:
        value = report()
        async_entry = next(item for item in value["scenarios"] if item["id"] == "async-echo")
        async_entry["performance_reference"] = "go"
        async_entry["gap_to_go"] = 1.30
        async_entry["reference_performance_passed"] = False
        failures, inconclusive = cross_lang.check_baseline(baseline(), value)
        self.assertTrue(any("gap to Go" in failure for failure in failures))
        self.assertEqual(inconclusive, [])

    def test_async_echo_go_reference_parity_passes(self) -> None:
        value = report()
        async_entry = next(item for item in value["scenarios"] if item["id"] == "async-echo")
        async_entry["performance_reference"] = "go"
        async_entry["gap_to_go"] = 1.03
        async_entry["paired_gap_to_go"] = [1.03] * 5
        failures, inconclusive = cross_lang.check_baseline(baseline(), value)
        self.assertEqual(failures, [])
        self.assertEqual(inconclusive, [])

    def test_async_echo_contract_mismatch_is_rejected(self) -> None:
        value = report()
        async_entry = next(item for item in value["scenarios"] if item["id"] == "async-echo")
        async_entry["benchmark_contract"] = "legacy"
        failures, _ = cross_lang.check_baseline(baseline(), value)
        self.assertTrue(any("benchmark contract" in failure for failure in failures))

    def test_async_echo_missing_real_concurrency_is_rejected(self) -> None:
        value = report()
        async_entry = next(item for item in value["scenarios"] if item["id"] == "async-echo")
        async_entry["concurrency_metrics"]["max_pending_tasks"] = 1
        failures, _ = cross_lang.check_baseline(baseline(), value)
        self.assertTrue(any("fan-out concurrency" in failure for failure in failures))

    def test_async_echo_accepts_ten_percent_paired_variance(self) -> None:
        value = report()
        async_entry = next(item for item in value["scenarios"] if item["id"] == "async-echo")
        async_entry["paired_gap_stddev_pct"] = 10.0
        failures, inconclusive = cross_lang.check_baseline(baseline(), value)
        self.assertEqual(failures, [])
        self.assertEqual(inconclusive, [])

    def test_async_echo_rejects_paired_variance_above_ten_percent(self) -> None:
        value = report()
        async_entry = next(item for item in value["scenarios"] if item["id"] == "async-echo")
        async_entry["paired_gap_stddev_pct"] = 10.01
        _, inconclusive = cross_lang.check_baseline(baseline(), value)
        self.assertTrue(any("paired ratio noise" in item for item in inconclusive))

    def test_partial_report_metadata_is_rejected(self) -> None:
        value = report()
        value["complete_scenario_set"] = False
        self.assertTrue(cross_lang.validate_report_metadata(value, "debug", None))

    def test_code_validation_report_skips_performance_statistics(self) -> None:
        value = report(stddev_ns=999, ns_per_iter=100)
        value["mode"] = "code_validation"
        value["measurement_policy"].update({
            "warmup_runs": 0,
            "timed_runs": 1,
            "independent_runs": 1,
        })
        async_entry = next(item for item in value["scenarios"] if item["id"] == "async-echo")
        async_entry["gap_to_go"] = 3.0
        async_entry["reference_performance_passed"] = False
        failures, inconclusive = cross_lang.check_baseline(
            baseline(), value, code_validation=True
        )
        self.assertEqual(failures, [])
        self.assertEqual(inconclusive, [])
        self.assertEqual(
            cross_lang.validate_report_metadata(value, "debug", None, True), []
        )

    def test_current_contract_requires_all_21_scenarios(self) -> None:
        self.assertEqual(len(cross_lang.REQUIRED_SCENARIOS), 21)
        value = report()
        value["scenarios"].append(
            {
                "id": "unknown-scenario",
                "correctness_passed": True,
                "results": {"spectra": {"ns_per_iter": 100, "stddev_ns": 1}},
            }
        )
        failures, _ = cross_lang.check_baseline(baseline(), value)
        self.assertIn("report has unknown scenario: unknown-scenario", failures)

    def test_baseline_missing_scenario_is_failure(self) -> None:
        value = baseline()
        value["scenarios"].pop(cross_lang.REQUIRED_SCENARIOS[-1])
        failures, _ = cross_lang.check_baseline(value, report())
        self.assertIn(
            f"baseline missing scenario: {cross_lang.REQUIRED_SCENARIOS[-1]}",
            failures,
        )

    def test_semantic_report_comparison_ignores_runtime_timestamps(self) -> None:
        first = report()
        second = report()
        first["generated_at"] = "2026-01-01T00:00:00Z"
        second["generated_at"] = "2026-01-02T00:00:00Z"
        self.assertEqual(
            report_comparator.semantic_report(first),
            report_comparator.semantic_report(second),
        )

    def test_profile_metadata_mismatch_is_rejected_by_contract(self) -> None:
        value = report(profile="release")
        self.assertTrue(cross_lang.validate_report_metadata(value, "debug", None))

    def test_measurement_policy_metadata_is_required(self) -> None:
        value = report()
        value["measurement_policy"]["timed_runs"] = 12
        self.assertTrue(cross_lang.validate_report_metadata(value, "debug", None))

    def test_run_tests_phase31_timeout_is_forwarded_to_host_runner(self) -> None:
        source = Path("run_tests.ps1").read_text(encoding="utf-8")
        self.assertIn(
            "[int]$timeoutSeconds = $hostCommandTimeoutSeconds",
            source,
        )
        self.assertIn(
            "$proc.WaitForExit($timeoutSeconds * 1000)",
            source,
        )
        self.assertIn("$proc.StandardOutput.ReadToEndAsync()", source)
        self.assertIn("$proc.StandardError.ReadToEndAsync()", source)
        self.assertIn('$name -eq "phase31_run_all"', source)
        self.assertIn("$proc.Kill($true)", source)
        self.assertIn(
            'Invoke-HostCommand -name "phase31_run_all"',
            source,
        )
        self.assertIn('$phase31BinaryPath = (Join-Path (Get-Location).Path "target\\release\\spectralang.exe")', source)
        self.assertIn('"--spectra-profile", "release"', source)
        self.assertIn('"--code-validation"', source)
        self.assertIn("-timeoutSeconds 600", source)
        self.assertIn("$runPhase31Gpu = $Phase -contains \"phase31_gpu\"", source)
        self.assertIn("$gpuStatus = \"SKIPPED\"", source)

    def test_async_echo_diagnostic_contract_has_required_variants(self) -> None:
        self.assertEqual(
            set(async_echo_diagnostics.FIXTURES),
            {
                "startup", "reset-only", "spawn-only", "join-only", "spawn-join",
                "fused", "full", "batch-reset-only", "batch-spawn-only",
                "batch-join-only", "batch-full", "batch-full-no-reset",
            },
        )
        self.assertEqual(async_echo_diagnostics.BATCH_VARIANTS, {
            "batch-reset-only", "batch-spawn-only", "batch-join-only",
            "batch-full", "batch-full-no-reset",
        })
        self.assertEqual(
            async_echo_diagnostics.VARIANT_CONTRACTS["batch-full"],
            "fanout_fanin_real_concurrency.v2",
        )
        self.assertEqual(
            async_echo_diagnostics.OUTER * async_echo_diagnostics.INNER,
            10_000,
        )
        speedup_source = Path("scripts/validate_r1603_gpu_speedup.py").read_text(encoding="utf-8")
        self.assertIn('report["status"] = "skipped"', speedup_source)
        self.assertIn("no WGPU adapter available", speedup_source)


class R1603ValidatorTests(unittest.TestCase):
    @patch("scripts.validate_r1603_gpu_backend.subprocess.run")
    def test_timeout_is_deterministic(self, run: Mock) -> None:
        run.side_effect = subprocess.TimeoutExpired(["cargo"], 120)
        with self.assertRaises(SystemExit) as raised:
            gpu_backend.run_step("gpu test", ["cargo", "test"], timeout_s=120)
        self.assertEqual(raised.exception.code, 124)

    @patch("scripts.validate_r1603_gpu_backend.subprocess.run")
    def test_step_captures_failure_output(self, run: Mock) -> None:
        run.return_value = subprocess.CompletedProcess(
            ["cargo", "test"], 1, stdout="stdout", stderr="stderr"
        )
        with self.assertRaises(SystemExit) as raised:
            gpu_backend.run_step("gpu test", ["cargo", "test"])
        self.assertEqual(raised.exception.code, 1)
        run.assert_called_once()
        self.assertEqual(run.call_args.kwargs["timeout"], 120)


if __name__ == "__main__":
    unittest.main()
