#!/usr/bin/env python3
"""Build and run the Phase 31 cross-language benchmark suite.

For each of 21 scenarios, this script:
1. Builds the Go and Rust binaries.
2. Compiles and runs the Spectra scenario via `spectralang run`.
3. Times 20 iterations per language with 3 warmup rounds.
4. Records median, p95, stddev, ns_per_iter.
5. Writes `target/phase31/cross-lang-report.json` and a `.md` summary.

Correctness is enforced via exit code (0 = pass, anything else = fail).

Usage::

    python scripts/phase31_run_all.py --out target/phase31/cross-lang-report.json

The companion gate `scripts/validate_phase31_cross_lang.py` reads the JSON
report and compares Spectra performance against the checked-in baseline.
"""

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
from functools import lru_cache
import json
import math
import os
import pathlib
import platform
import shutil
import statistics
import subprocess
import sys
import time
from typing import Any

try:
    from scripts.phase31_contract import (
        ASYNC_ECHO_CONTRACT,
        ASYNC_ECHO_ACCEPTED_MAX_GAP_TO_GO,
        ASYNC_ECHO_EXPECTED_RESULT,
        ASYNC_ECHO_ITERATIONS,
        ASYNC_ECHO_MAX_REFERENCE_GAP_PCT,
        ASYNC_ECHO_REFERENCE_RUNTIME,
        ASYNC_ECHO_TASKS_PER_ITERATION,
        LANGUAGES,
        MAX_CONFIRMATION_RUNS,
        MAX_STDDEV_PCT,
        PHASE31_SCHEMA,
        SCENARIOS,
        TIMED_RUNS,
        WARMUP_RUNS,
        validate_scenario_ids,
    )
except ModuleNotFoundError:  # direct `python scripts/phase31_run_all.py`
    from phase31_contract import (  # type: ignore[no-redef]
        ASYNC_ECHO_CONTRACT,
        ASYNC_ECHO_ACCEPTED_MAX_GAP_TO_GO,
        ASYNC_ECHO_EXPECTED_RESULT,
        ASYNC_ECHO_ITERATIONS,
        ASYNC_ECHO_MAX_REFERENCE_GAP_PCT,
        ASYNC_ECHO_REFERENCE_RUNTIME,
        ASYNC_ECHO_TASKS_PER_ITERATION,
        LANGUAGES,
        MAX_CONFIRMATION_RUNS,
        MAX_STDDEV_PCT,
        PHASE31_SCHEMA,
        SCENARIOS,
        TIMED_RUNS,
        WARMUP_RUNS,
        validate_scenario_ids,
    )

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
BENCH_DIR = REPO_ROOT / "benchmarks" / "cross-lang"
TARGET_DIR = REPO_ROOT / "target" / "phase31"
BUILD_DIR = TARGET_DIR / "build"

WARMUP = WARMUP_RUNS
TIMED = TIMED_RUNS
DEFAULT_TIMEOUT_S = 300
MAX_HOST_LOAD_PCT = 70.0
KNOWN_HEAVY_PROCESSES = {"cline.exe"}


def log(msg: str) -> None:
    print(f"[phase31] {msg}", flush=True)


def find_tool(name: str) -> str | None:
    return shutil.which(name)


def host_preflight() -> dict[str, Any]:
    load_pct: float | None = None
    known_processes: list[str] = []
    if os.name == "nt":
        load = subprocess.run(
            [
                "powershell", "-NoProfile", "-Command",
                "(Get-Counter '\\Processor(_Total)\\% Processor Time' "
                "-SampleInterval 1 -MaxSamples 2).CounterSamples[-1].CookedValue",
            ],
            capture_output=True, text=True, encoding="utf-8", errors="replace", check=False, timeout=10,
        )
        try:
            load_pct = float(load.stdout.strip().replace(",", "."))
        except ValueError:
            load_pct = None
        processes = subprocess.run(
            ["tasklist", "/fo", "csv", "/nh"],
            capture_output=True, text=True, encoding="utf-8", errors="replace", check=False, timeout=10,
        )
        for line in processes.stdout.splitlines():
            image = line.split(",", 1)[0].strip('"').lower()
            if image in KNOWN_HEAVY_PROCESSES:
                known_processes.append(image)
    elif hasattr(os, "getloadavg") and os.cpu_count():
        load_pct = os.getloadavg()[0] / max(1, os.cpu_count() or 1) * 100.0
    reasons: list[str] = []
    if load_pct is not None and load_pct > MAX_HOST_LOAD_PCT:
        reasons.append(f"host CPU load {load_pct:.1f}% exceeds {MAX_HOST_LOAD_PCT:.1f}%")
    if known_processes:
        reasons.append(f"known heavy processes active: {sorted(set(known_processes))}")
    return {
        "status": "busy" if reasons else "quiescent",
        "load_pct": load_pct,
        "max_load_pct": MAX_HOST_LOAD_PCT,
        "known_heavy_processes": sorted(set(known_processes)),
        "reasons": reasons,
    }


@lru_cache(maxsize=None)
def build_go(scenario: str) -> pathlib.Path:
    src = BENCH_DIR / scenario / "go" / "bench.go"
    out = BUILD_DIR / scenario / "go" / "bench"
    out.parent.mkdir(parents=True, exist_ok=True)
    cmd = ["go", "build", "-ldflags=-s -w", "-o", str(out), str(src)]
    proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace", check=False)
    if proc.returncode != 0:
        raise RuntimeError(f"go build failed for {scenario}:\n{proc.stderr}")
    return out


@lru_cache(maxsize=None)
def build_rust(scenario: str) -> pathlib.Path:
    src = BENCH_DIR / scenario / "rust" / "bench.rs"
    out = BUILD_DIR / scenario / "rust" / "bench"
    out.parent.mkdir(parents=True, exist_ok=True)
    cmd = ["rustc", "-O", "-o", str(out), str(src)]
    proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace", check=False)
    if proc.returncode != 0:
        raise RuntimeError(f"rustc failed for {scenario}:\n{proc.stderr}")
    return out


def build_spectra(binary: pathlib.Path, scenario: str) -> pathlib.Path:
    # Spectra scenarios are .spectra source files. The compiled CLI runs them.
    return BENCH_DIR / scenario / "spectra" / "bench.spectra"


def time_subprocess(
    cmd: list[str],
    cwd: pathlib.Path | None = None,
    timeout_s: int = DEFAULT_TIMEOUT_S,
    env: dict[str, str] | None = None,
) -> tuple[int, float, bool, str]:
    """Run `cmd` once, returning exit code, duration, status, output tail."""
    start = time.perf_counter_ns()
    try:
        proc = subprocess.run(
            cmd, capture_output=True, text=True, encoding="utf-8", errors="replace", cwd=cwd, check=False,
            timeout=timeout_s, env=env,
        )
    except subprocess.TimeoutExpired:
        return -1, time.perf_counter_ns() - start, False, f"timeout after {timeout_s}s"
    elapsed = time.perf_counter_ns() - start
    output = ((proc.stdout or "") + "\n" + (proc.stderr or "")).strip()
    return proc.returncode, elapsed, proc.returncode == 0, output[-4000:]


def time_runs(
    cmd: list[str],
    cwd: pathlib.Path | None = None,
    timeout_s: int = DEFAULT_TIMEOUT_S,
) -> dict[str, Any]:
    """Run warmup + timed iterations, return stats."""
    for _ in range(WARMUP):
        rc, _, ok, output_tail = time_subprocess(cmd, cwd, timeout_s)
        if not ok:
            return {"elapsed_ns": [], "ok": False, "last_rc": rc, "output_tail": output_tail}
    samples: list[int] = []
    last_output_tail = ""
    for _ in range(TIMED):
        rc, ns, ok, output_tail = time_subprocess(cmd, cwd, timeout_s)
        if not ok:
            return {"elapsed_ns": samples, "ok": False, "last_rc": rc, "output_tail": output_tail}
        samples.append(ns)
        last_output_tail = output_tail
    return {"elapsed_ns": samples, "ok": True, "last_rc": 0, "output_tail": last_output_tail}


def stats_for(samples: list[int]) -> dict[str, int]:
    if not samples:
        return {"median_ns": 0, "p95_ns": 0, "stddev_ns": 0, "ns_per_iter": 0}
    sorted_s = sorted(samples)
    p95_index = max(0, int(len(sorted_s) * 0.95) - 1)
    median = statistics.median(samples)
    stddev = statistics.pstdev(samples) if len(samples) > 1 else 0
    return {
        "median_ns": int(median),
        "p95_ns": int(sorted_s[p95_index]),
        "stddev_ns": int(stddev),
        "ns_per_iter": int(median),
    }


def run_scenario(
    scenario: str,
    spectra_binary: pathlib.Path,
    skip_missing: set[str],
) -> dict[str, Any]:
    log(f"running {scenario}")
    per_lang: dict[str, Any] = {}
    correctness = True
    iterations = 1
    category = "unknown"

    # Determine iterations + category from baseline
    baseline_path = (
        REPO_ROOT
        / "docs"
        / "performance"
        / "phase31-go-comparable"
        / "baseline.json"
    )
    if baseline_path.exists():
        b = json.loads(baseline_path.read_text(encoding="utf-8"))
        b_scenario = b.get("scenarios", {}).get(scenario)
        if b_scenario is not None:
            iterations = b_scenario.get("iterations", 1)
            category = b_scenario.get("category", "unknown")

    # Spectra
    if "spectra" not in skip_missing:
        src = build_spectra(spectra_binary, scenario)
        cmd = [str(spectra_binary), "run", str(src)]
        res = time_runs(cmd, cwd=REPO_ROOT)
        if not res["ok"]:
            log(f"  spectra FAILED rc={res['last_rc']}")
            correctness = False
            per_lang["spectra"] = {
                "error": f"rc={res['last_rc']}",
                "output_tail": res.get("output_tail", ""),
                "command": cmd,
                "exit_code": res["last_rc"],
                "failure_class": "timeout" if res["last_rc"] == -1 else "runtime",
            }
        else:
            per_lang["spectra"] = {
                **stats_for(res["elapsed_ns"]),
                "command": cmd,
                "exit_code": res["last_rc"],
                "failure_class": None,
            }

    # Go
    if "go" not in skip_missing:
        if find_tool("go") is None:
            per_lang["go"] = {"error": "go toolchain not available"}
        else:
            try:
                bin_path = build_go(scenario)
                cmd = [str(bin_path)]
                res = time_runs(cmd)
                if not res["ok"]:
                    correctness = False
                    per_lang["go"] = {
                        "error": f"rc={res['last_rc']}",
                        "output_tail": res.get("output_tail", ""),
                        "command": cmd,
                        "exit_code": res["last_rc"],
                        "failure_class": "timeout" if res["last_rc"] == -1 else "runtime",
                    }
                else:
                    per_lang["go"] = {
                        **stats_for(res["elapsed_ns"]), "command": cmd,
                        "exit_code": res["last_rc"], "failure_class": None,
                    }
            except Exception as e:
                per_lang["go"] = {"error": str(e)}

    # Rust
    if "rust" not in skip_missing:
        if find_tool("rustc") is None:
            per_lang["rust"] = {"error": "rust toolchain not available"}
        else:
            try:
                bin_path = build_rust(scenario)
                cmd = [str(bin_path)]
                res = time_runs(cmd)
                if not res["ok"]:
                    correctness = False
                    per_lang["rust"] = {
                        "error": f"rc={res['last_rc']}",
                        "output_tail": res.get("output_tail", ""),
                        "command": cmd,
                        "exit_code": res["last_rc"],
                        "failure_class": "timeout" if res["last_rc"] == -1 else "runtime",
                    }
                else:
                    per_lang["rust"] = {
                        **stats_for(res["elapsed_ns"]), "command": cmd,
                        "exit_code": res["last_rc"], "failure_class": None,
                    }
            except Exception as e:
                per_lang["rust"] = {"error": str(e)}

    gap_to_go = None
    gap_to_rust = None
    spec = per_lang.get("spectra", {})
    go = per_lang.get("go", {})
    rs = per_lang.get("rust", {})
    if (
        isinstance(spec, dict)
        and "ns_per_iter" in spec
        and isinstance(go, dict)
        and "ns_per_iter" in go
        and go["ns_per_iter"] > 0
    ):
        gap_to_go = round(spec["ns_per_iter"] / go["ns_per_iter"], 3)
    if (
        isinstance(spec, dict)
        and "ns_per_iter" in spec
        and isinstance(rs, dict)
        and "ns_per_iter" in rs
        and rs["ns_per_iter"] > 0
    ):
        gap_to_rust = round(spec["ns_per_iter"] / rs["ns_per_iter"], 3)
    return {
        "id": scenario,
        "category": category,
        "iterations": iterations,
        "results": per_lang,
        "gap_to_go": gap_to_go,
        "gap_to_rust": gap_to_rust,
        "performance_reference": "go" if scenario == "async-echo" else None,
        "max_reference_gap_pct": ASYNC_ECHO_MAX_REFERENCE_GAP_PCT if scenario == "async-echo" else None,
        "reference_performance_passed": (
            gap_to_go is not None and 0 < gap_to_go <= ASYNC_ECHO_ACCEPTED_MAX_GAP_TO_GO
            if scenario == "async-echo"
            else None
        ),
        "correctness_passed": correctness,
    }


def _language_command(
    scenario: str, language: str, spectra_binary: pathlib.Path
) -> tuple[list[str], pathlib.Path | None]:
    if language == "spectra":
        source = build_spectra(spectra_binary, scenario)
        return [str(spectra_binary), "run", str(source)], REPO_ROOT
    if language == "go":
        return [str(build_go(scenario))], None
    if language == "rust":
        return [str(build_rust(scenario))], None
    raise ValueError(f"unsupported benchmark language: {language}")


def _measure_language(
    scenario: str,
    language: str,
    spectra_binary: pathlib.Path,
    timeout_s: int,
    code_validation: bool = False,
) -> dict[str, Any]:
    required_tool = {"go": "go", "rust": "rustc"}.get(language)
    if required_tool and find_tool(required_tool) is None:
        return {
            "error": f"{required_tool} toolchain not available",
            "exit_code": None,
            "failure_class": "environment",
            "command": [],
            "output_tail": "",
        }
    try:
        cmd, cwd = _language_command(scenario, language, spectra_binary)
        if code_validation:
            rc, elapsed, ok, output = time_subprocess(cmd, cwd, timeout_s)
            result = {
                "elapsed_ns": [int(elapsed)] if ok else [],
                "ok": ok,
                "last_rc": rc,
                "output_tail": output,
            }
        else:
            result = time_runs(cmd, cwd=cwd, timeout_s=timeout_s)
    except Exception as exc:
        return {
            "error": str(exc),
            "exit_code": None,
            "failure_class": "build",
            "command": [],
            "output_tail": str(exc)[-4000:],
        }
    if not result["ok"]:
        return {
            "error": f"rc={result['last_rc']}",
            "exit_code": result["last_rc"],
            "failure_class": "timeout" if result["last_rc"] == -1 else "runtime",
            "command": cmd,
            "output_tail": result.get("output_tail", ""),
        }
    return {
        **stats_for(result["elapsed_ns"]),
        "samples_ns": result["elapsed_ns"],
        "command": cmd,
        "exit_code": 0,
        "failure_class": None,
        "output_tail": result.get("output_tail", ""),
    }


def _measure_spectra_go_pair(
    scenario: str,
    spectra_binary: pathlib.Path,
    timeout_s: int,
    attempt_number: int,
) -> tuple[dict[str, Any], dict[str, Any], list[float]]:
    commands = {
        language: _language_command(scenario, language, spectra_binary)
        for language in ("spectra", "go")
    }
    samples: dict[str, list[int]] = {"spectra": [], "go": []}
    outputs = {"spectra": "", "go": ""}
    for warmup in range(WARMUP):
        order = ("spectra", "go") if (attempt_number + warmup) % 2 == 0 else ("go", "spectra")
        for language in order:
            cmd, cwd = commands[language]
            rc, _, ok, output = time_subprocess(cmd, cwd, timeout_s)
            outputs[language] = output
            if not ok:
                failure = {
                    "error": f"rc={rc}", "exit_code": rc,
                    "failure_class": "timeout" if rc == -1 else "runtime",
                    "command": cmd, "output_tail": output,
                }
                return failure, failure, []
    paired_ratios: list[float] = []
    for timed_run in range(TIMED):
        order = ("spectra", "go") if (attempt_number + timed_run) % 2 == 0 else ("go", "spectra")
        round_values: dict[str, int] = {}
        for language in order:
            cmd, cwd = commands[language]
            rc, elapsed, ok, output = time_subprocess(cmd, cwd, timeout_s)
            outputs[language] = output
            if not ok:
                failure = {
                    "error": f"rc={rc}", "exit_code": rc,
                    "failure_class": "timeout" if rc == -1 else "runtime",
                    "command": cmd, "output_tail": output,
                }
                return failure, failure, []
            samples[language].append(int(elapsed))
            round_values[language] = int(elapsed)
        paired_ratios.append(round_values["spectra"] / round_values["go"])

    results: dict[str, dict[str, Any]] = {}
    for language in ("spectra", "go"):
        cmd, _ = commands[language]
        results[language] = {
            **stats_for(samples[language]),
            "samples_ns": samples[language],
            "command": cmd,
            "exit_code": 0,
            "failure_class": None,
            "output_tail": outputs[language],
        }
    return results["spectra"], results["go"], paired_ratios


def _async_echo_concurrency_probe(
    spectra_binary: pathlib.Path, timeout_s: int
) -> tuple[dict[str, Any] | None, str | None]:
    cmd, cwd = _language_command("async-echo", "spectra", spectra_binary)
    env = dict(os.environ)
    env["SPECTRA_CONCURRENT_DIAGNOSTICS"] = "1"
    rc, _, ok, output = time_subprocess(cmd, cwd=cwd, timeout_s=timeout_s, env=env)
    if not ok:
        return None, f"diagnostic probe failed rc={rc}: {output}"
    marker = "SPECTRA_CONCURRENT_DIAGNOSTICS="
    for line in output.splitlines():
        if line.startswith(marker):
            try:
                return json.loads(line[len(marker):]), None
            except json.JSONDecodeError as exc:
                return None, f"invalid concurrency diagnostics: {exc}"
    return None, "concurrency diagnostics marker missing"


def run_scenario_v2(
    scenario: str,
    spectra_binary: pathlib.Path,
    skip_missing: set[str],
    attempt_number: int,
    timeout_s: int,
    code_validation: bool = False,
) -> dict[str, Any]:
    log(f"running {scenario} attempt {attempt_number + 1}")
    baseline_path = REPO_ROOT / "docs" / "performance" / "phase31-go-comparable" / "baseline.json"
    baseline = json.loads(baseline_path.read_text(encoding="utf-8")) if baseline_path.exists() else {}
    baseline_entry = baseline.get("scenarios", {}).get(scenario, {})
    iterations = baseline_entry.get("iterations", 1)
    category = baseline_entry.get("category", "unknown")
    order = ["spectra", "go"] if attempt_number % 2 == 0 else ["go", "spectra"]
    order.append("rust")
    results: dict[str, Any] = {}
    if (
        scenario == "async-echo"
        and not code_validation
        and not ({"spectra", "go"} & skip_missing)
    ):
        log(f"  attempt {attempt_number + 1}: paired spectra/go")
        spectra_result, go_result, paired_samples = _measure_spectra_go_pair(
            scenario, spectra_binary, timeout_s, attempt_number
        )
        results["spectra"] = spectra_result
        results["go"] = go_result
    else:
        paired_samples = []
    pending_languages = [language for language in order if language not in results]
    for language in pending_languages:
        if language in skip_missing:
            results[language] = {
                "error": "explicitly skipped",
                "exit_code": None,
                "failure_class": "environment",
                "command": [],
                "output_tail": "",
            }
    runnable = [language for language in pending_languages if language not in skip_missing]
    if code_validation:
        for language in runnable:
            log(f"  attempt {attempt_number + 1}: {language}")
        with ThreadPoolExecutor(max_workers=len(runnable) or 1) as executor:
            measured = executor.map(
                lambda language: _measure_language(
                    scenario, language, spectra_binary, timeout_s, True
                ),
                runnable,
            )
            for language, result in zip(runnable, measured):
                results[language] = result
    else:
        for language in runnable:
            log(f"  attempt {attempt_number + 1}: {language}")
            results[language] = _measure_language(
                scenario, language, spectra_binary, timeout_s, False
            )

    correctness = all(
        result.get("exit_code") == 0 and "error" not in result
        for result in results.values()
    ) and set(results) == set(LANGUAGES)
    gaps: dict[str, float | None] = {}
    spectra_ns = results.get("spectra", {}).get("ns_per_iter")
    for language in ("go", "rust"):
        reference_ns = results.get(language, {}).get("ns_per_iter")
        gaps[language] = (
            round(spectra_ns / reference_ns, 6)
            if spectra_ns and reference_ns and reference_ns > 0
            else None
        )
    if scenario == "async-echo" and paired_samples:
        gaps["go"] = round(statistics.median(paired_samples), 6)

    entry: dict[str, Any] = {
        "id": scenario,
        "category": category,
        "iterations": iterations,
        "results": results,
        "language_order": order,
        "gap_to_go": gaps["go"],
        "gap_to_rust": gaps["rust"],
        "correctness_passed": correctness,
    }
    if scenario == "async-echo":
        metrics, probe_error = _async_echo_concurrency_probe(spectra_binary, timeout_s)
        gap = gaps["go"]
        entry.update({
            "benchmark_contract": ASYNC_ECHO_CONTRACT,
            "reference_runtime": ASYNC_ECHO_REFERENCE_RUNTIME,
            "performance_reference": ASYNC_ECHO_REFERENCE_RUNTIME,
            "max_reference_gap_pct": ASYNC_ECHO_MAX_REFERENCE_GAP_PCT,
            "tasks_per_iteration": ASYNC_ECHO_TASKS_PER_ITERATION,
            "benchmark_iterations": ASYNC_ECHO_ITERATIONS,
            "expected_result": ASYNC_ECHO_EXPECTED_RESULT,
            "concurrency_metrics": metrics,
            "concurrency_probe_error": probe_error,
            "paired_sample_ratios": paired_samples,
            "reference_performance_passed": (
                gap is not None and 0 < gap <= ASYNC_ECHO_ACCEPTED_MAX_GAP_TO_GO
            ),
        })
        if metrics is None:
            entry["correctness_passed"] = False
    return entry


def aggregate_scenario_attempts(attempts: list[dict[str, Any]]) -> dict[str, Any]:
    """Aggregate independent attempts without hiding correctness failures."""
    if len(attempts) == 1:
        attempts[0]["independent_runs"] = 1
        if attempts[0].get("id") == "async-echo":
            attempts[0]["paired_gap_to_go"] = [attempts[0].get("gap_to_go")]
        return attempts[0]

    first = attempts[0]
    results: dict[str, Any] = {}
    for language in LANGUAGES:
        samples = [
            attempt.get("results", {}).get(language, {}).get("ns_per_iter")
            for attempt in attempts
        ]
        if any(value is None for value in samples):
            results[language] = {"error": "independent attempt failed"}
            continue
        medians = [int(value) for value in samples]
        result = dict(first["results"][language])
        stable_medians = medians
        outliers: list[int] = []
        if len(medians) >= 5:
            ordered = sorted(medians)
            trim_count = max(1, math.ceil(len(ordered) * 0.20))
            candidate = ordered[trim_count:-trim_count]
            if len(candidate) >= 3:
                stable_medians = candidate
                outliers = ordered[:trim_count] + ordered[-trim_count:]
        result["ns_per_iter"] = int(statistics.median(stable_medians))
        result["independent_medians_ns"] = medians
        result["stable_medians_ns"] = stable_medians
        result["outlier_medians_ns"] = outliers
        result["outlier_policy"] = (
            "symmetric 20% trimmed independent medians; raw samples preserved"
        )
        result["independent_stddev_ns"] = int(statistics.pstdev(stable_medians))
        results[language] = result

    first["results"] = results
    first["independent_runs"] = len(attempts)
    first["independent_attempts"] = [
        {
            "results": attempt.get("results", {}),
            "correctness_passed": attempt.get("correctness_passed", False),
            "language_order": attempt.get("language_order", []),
            "gap_to_go": attempt.get("gap_to_go"),
            "concurrency_metrics": attempt.get("concurrency_metrics"),
        }
        for attempt in attempts
    ]
    first["correctness_passed"] = all(
        attempt.get("correctness_passed", False) for attempt in attempts
    )
    spectra = results.get("spectra", {})
    go = results.get("go", {})
    rust = results.get("rust", {})
    if all(
        result.get("ns_per_iter", 0) > 0
        for result in (spectra, go, rust)
    ):
        first["gap_to_go"] = round(spectra["ns_per_iter"] / go["ns_per_iter"], 3)
        first["gap_to_rust"] = round(spectra["ns_per_iter"] / rust["ns_per_iter"], 3)
    if first.get("id") == "async-echo":
        paired_gaps = [
            float(attempt["gap_to_go"])
            for attempt in attempts
            if isinstance(attempt.get("gap_to_go"), (int, float))
        ]
        gap = round(statistics.median(paired_gaps), 6) if paired_gaps else None
        first["paired_gap_to_go"] = paired_gaps
        first["gap_to_go"] = gap
        first["paired_gap_stddev_pct"] = (
            statistics.pstdev(paired_gaps) / gap * 100.0
            if gap and len(paired_gaps) > 1 else 0.0
        )
        first["performance_reference"] = "go"
        first["max_reference_gap_pct"] = ASYNC_ECHO_MAX_REFERENCE_GAP_PCT
        first["reference_performance_passed"] = (
            len(paired_gaps) == len(attempts)
            and gap is not None
            and 0 < gap <= ASYNC_ECHO_ACCEPTED_MAX_GAP_TO_GO
        )
        first["concurrency_metrics"] = attempts[-1].get("concurrency_metrics")
        first["concurrency_probe_error"] = next(
            (attempt.get("concurrency_probe_error") for attempt in attempts
             if attempt.get("concurrency_probe_error")),
            None,
        )
    return first


def write_markdown(report: dict[str, Any], path: pathlib.Path) -> None:
    lines = [
        "# Phase 31 Cross-Language Performance Report",
        "",
        f"Updated: {time.strftime('%Y-%m-%d %H:%M:%S')}",
        "",
        "| scenario | category | spectra ns | go ns | rust ns | gap vs go | gap vs rust |",
        "|---|---|---:|---:|---:|---:|---:|",
    ]
    for s in report.get("scenarios", []):
        results = s.get("results", {})

        def ns_of(lang: str) -> str:
            r = results.get(lang, {})
            if "error" in r:
                return "n/a"
            if "ns_per_iter" in r:
                return f"{r['ns_per_iter']:,}"
            return "n/a"

        gap_go = s.get("gap_to_go")
        gap_rust = s.get("gap_to_rust")
        gap_go_s = f"{gap_go:.3f}x" if gap_go is not None else "n/a"
        gap_rust_s = f"{gap_rust:.3f}x" if gap_rust is not None else "n/a"
        lines.append(
            f"| `{s['id']}` | {s['category']} | {ns_of('spectra')} | "
            f"{ns_of('go')} | {ns_of('rust')} | "
            f"{gap_go_s} | {gap_rust_s} |"
        )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--out",
        default=str(TARGET_DIR / "cross-lang-report.json"),
        help="output JSON report path",
    )
    parser.add_argument(
        "--scenarios",
        nargs="*",
        default=SCENARIOS,
        help="subset of scenarios to run",
    )
    parser.add_argument(
        "--spectra-binary",
        default=str(
            REPO_ROOT / "target" / "debug" / ("spectralang.exe" if os.name == "nt" else "spectralang")
        ),
        help="path to the spectra CLI binary",
    )
    parser.add_argument(
        "--spectra-profile",
        choices=("debug", "release"),
        default=None,
        help="actual build profile of --spectra-binary; inferred when omitted",
    )
    parser.add_argument(
        "--independent-runs",
        type=int,
        default=1,
        help="number of complete independent measurements per scenario",
    )
    parser.add_argument(
        "--baseline",
        default=str(
            REPO_ROOT / "docs" / "performance" / "phase31-go-comparable" / "baseline.json"
        ),
        help="read-only baseline used to trigger confirmation measurements",
    )
    parser.add_argument(
        "--confirm-regressions",
        type=int,
        default=0,
        help="extra independent attempts for scenarios initially over baseline drift",
    )
    parser.add_argument(
        "--timeout-seconds",
        type=int,
        default=DEFAULT_TIMEOUT_S,
        help="per-process timeout",
    )
    parser.add_argument(
        "--allow-busy-host",
        action="store_true",
        help="diagnostic-only override; official full-suite runs reject this flag",
    )
    parser.add_argument(
        "--code-validation",
        action="store_true",
        help="fast functional gate: one execution per language, no performance certification",
    )
    parser.add_argument(
        "--skip",
        nargs="*",
        default=[],
        choices=LANGUAGES,
        help="languages to skip (e.g. when toolchain missing)",
    )
    args = parser.parse_args()
    if (
        args.independent_runs < 1
        or args.confirm_regressions < 0
        or args.confirm_regressions > MAX_CONFIRMATION_RUNS
        or args.timeout_seconds < 1
    ):
        log("ERROR: --independent-runs must be >= 1 and --confirm-regressions >= 0")
        return 2

    spectra_binary = pathlib.Path(args.spectra_binary)
    if not spectra_binary.exists():
        log(f"ERROR: spectra binary not found at {spectra_binary}")
        log("Build it first: cargo build -p spectra-cli")
        return 2

    inferred_profile = "release" if "release" in spectra_binary.parts else "debug"
    profile = args.spectra_profile or inferred_profile
    if args.spectra_profile and args.spectra_profile != inferred_profile:
        log(
            f"ERROR: --spectra-profile={args.spectra_profile} does not match "
            f"binary path ({inferred_profile})"
        )
        return 2

    out_path = pathlib.Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    BUILD_DIR.mkdir(parents=True, exist_ok=True)

    skip_missing = set(args.skip)
    baseline = {}
    baseline_path = pathlib.Path(args.baseline)
    if baseline_path.exists():
        try:
            baseline = json.loads(baseline_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            log(f"ERROR: invalid Phase 31 baseline at {baseline_path}: {exc}")
            return 2
    # Auto-skip languages whose toolchain is missing.
    for lang, tool in (("go", "go"), ("rust", "rustc")):
        if find_tool(tool) is None:
            log(f"auto-skipping {lang}: '{tool}' not on PATH")
            skip_missing.add(lang)

    scenario_errors = validate_scenario_ids(list(args.scenarios))
    if scenario_errors:
        for error in scenario_errors:
            log(f"ERROR: {error}")
        return 2

    official_full_suite = tuple(args.scenarios) == tuple(SCENARIOS)
    if official_full_suite and args.allow_busy_host:
        log("ERROR: --allow-busy-host is diagnostic-only and forbidden for the official gate")
        return 2
    preflight = host_preflight()
    if preflight["status"] != "quiescent" and not args.allow_busy_host:
        log(f"ERROR: host preflight failed: {preflight['reasons']}")
        return 2

    report = {
        "schema": PHASE31_SCHEMA,
        "mode": "code_validation" if args.code_validation else "performance_certification",
        "scenario_matrix": list(SCENARIOS),
        "profile": profile,
        "spectra_binary": str(spectra_binary.resolve()),
        "git_revision": subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=REPO_ROOT,
            capture_output=True, text=True, encoding="utf-8", errors="replace",
            check=False,
        ).stdout.strip() or "unknown",
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "host": platform.platform(),
        "host_details": {
            "cpu_count": os.cpu_count(),
            "python": platform.python_version(),
        },
        "environment_preflight": preflight,
        "complete_scenario_set": official_full_suite,
        "measurement_policy": {
            "warmup_runs": 0 if args.code_validation else WARMUP,
            "timed_runs": 1 if args.code_validation else TIMED,
            "independent_runs": 1 if args.code_validation else args.independent_runs,
            "confirmation_runs": 0 if args.code_validation else args.confirm_regressions,
            "max_stddev_pct": MAX_STDDEV_PCT,
            "per_process_timeout_s": args.timeout_seconds,
        },
        "runtimes": {
            "go": subprocess.run(
                ["go", "version"], capture_output=True, text=True, encoding="utf-8", errors="replace", check=False
            ).stdout.strip()
            if find_tool("go")
            else "missing",
            "rust": subprocess.run(
                ["rustc", "--version"], capture_output=True, text=True, encoding="utf-8", errors="replace", check=False
            ).stdout.strip()
            if find_tool("rustc")
            else "missing",
        },
        "scenarios": [],
    }

    for scenario in args.scenarios:
        attempts: list[dict[str, Any]] = []
        requested_runs = 1 if args.code_validation else args.independent_runs
        for attempt_number in range(requested_runs):
            try:
                attempts.append(run_scenario_v2(
                    scenario, spectra_binary, skip_missing, attempt_number,
                    args.timeout_seconds,
                    args.code_validation,
                ))
            except Exception as e:
                log(f"scenario {scenario} attempt {attempt_number + 1} raised: {e}")
                attempts.append(
                    {
                        "id": scenario,
                        "category": "unknown",
                        "iterations": 0,
                        "results": {},
                        "gap_to_go": None,
                        "gap_to_rust": None,
                        "correctness_passed": False,
                        "error": str(e),
                    }
                )
        entry = aggregate_scenario_attempts(attempts)
        base_entry = baseline.get("scenarios", {}).get(scenario, {})
        base_ns = base_entry.get("spectra_ns_per_iter", 0)
        max_drift = baseline.get("max_drift_pct", 5.0)
        observed_ns = entry.get("results", {}).get("spectra", {}).get("ns_per_iter", 0)
        observed_stddev = entry.get("results", {}).get("spectra", {}).get("independent_stddev_ns", entry.get("results", {}).get("spectra", {}).get("stddev_ns", 0))
        observed_stddev_pct = (observed_stddev / observed_ns * 100.0) if observed_ns else 0.0
        needs_confirmation = (
            not args.code_validation
            and
            args.confirm_regressions > 0
            and (
                observed_stddev_pct > MAX_STDDEV_PCT
                or (
                    not base_entry.get("placeholder", False)
                    and base_ns > 0
                    and observed_ns > base_ns * (1.0 + max_drift / 100.0)
                )
            )
        )
        if needs_confirmation:
            log(
                f"{scenario}: drift or measurement noise exceeds the gate; "
                f"running {args.confirm_regressions} stabilization attempt(s)"
            )
            for confirmation_number in range(args.confirm_regressions):
                try:
                    attempts.append(run_scenario_v2(
                        scenario, spectra_binary, skip_missing,
                        args.independent_runs + confirmation_number,
                        args.timeout_seconds,
                        False,
                    ))
                except Exception as e:
                    log(
                        f"scenario {scenario} confirmation {confirmation_number + 1} raised: {e}"
                    )
                    attempts.append(
                        {
                            "id": scenario,
                            "category": entry.get("category", "unknown"),
                            "iterations": entry.get("iterations", 0),
                            "results": {},
                            "gap_to_go": None,
                            "gap_to_rust": None,
                            "correctness_passed": False,
                            "error": str(e),
                        }
                    )
            entry = aggregate_scenario_attempts(attempts)
        report["scenarios"].append(entry)

    out_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    md_path = out_path.with_suffix(".md")
    write_markdown(report, md_path)
    log(f"wrote {out_path}")
    log(f"wrote {md_path}")

    failed = [s for s in report["scenarios"] if not s.get("correctness_passed", False)]
    unstable = []
    for scenario in report["scenarios"]:
        if args.code_validation or scenario.get("id") == "async-echo":
            continue
        spectra = scenario.get("results", {}).get("spectra", {})
        median_ns = spectra.get("ns_per_iter", 0)
        stddev_ns = spectra.get("independent_stddev_ns", spectra.get("stddev_ns", 0))
        if not median_ns or stddev_ns / median_ns * 100.0 > MAX_STDDEV_PCT:
            unstable.append(scenario["id"])
    reference_failed = [] if args.code_validation else [
        scenario["id"] for scenario in report["scenarios"]
        if scenario["id"] == "async-echo"
        and not scenario.get("reference_performance_passed", False)
    ]
    reference_unstable = [] if args.code_validation else [
        scenario["id"] for scenario in report["scenarios"]
        if scenario["id"] == "async-echo"
        and scenario.get("paired_gap_stddev_pct", 0.0) > MAX_STDDEV_PCT
    ]
    if failed:
        log(f"correctness failures: {[s['id'] for s in failed]}")
    if unstable:
        log(f"inconclusive measurements: {unstable}")
    if reference_failed:
        log(f"reference parity failures: {reference_failed}")
    if reference_unstable:
        log(f"reference parity inconclusive: {reference_unstable}")
    if official_full_suite and len(report["scenarios"]) != len(SCENARIOS):
        log("partial report rejected")
        return 1
    if failed or unstable or reference_failed or reference_unstable:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
