#!/usr/bin/env python3
"""Defined stress/soak suites for SpectraLang Phase 12.

The default profile is intentionally CI-friendly. Increase --iterations and
--timeout-seconds for local soak runs.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from dataclasses import dataclass, asdict
from pathlib import Path




@dataclass
class CaseResult:
    suite: str
    name: str
    command: list[str]
    elapsed_ms: int
    exit_code: int
    timed_out: bool
    peak_rss_bytes: int | None
    output_excerpt: str


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def find_binary(root: Path) -> Path:
    exe = "spectralang.exe" if os.name == "nt" else "spectralang"
    candidate = root / "target" / "debug" / exe
    if candidate.exists():
        return candidate
    subprocess.run(["cargo", "build", "-p", "spectra-cli"], cwd=root, check=True)
    if not candidate.exists():
        raise SystemExit(f"spectralang binary not found after build: {candidate}")
    return candidate


def peak_rss(pid: int) -> int | None:
    try:
        import psutil  # type: ignore

        process = psutil.Process(pid)
        rss = process.memory_info().rss
        for child in process.children(recursive=True):
            try:
                rss += child.memory_info().rss
            except psutil.Error:
                pass
        return rss
    except Exception:
        return None


def run_command(
    root: Path,
    suite: str,
    name: str,
    command: list[str],
    timeout_seconds: int,
    memory_limit_mb: int | None,
) -> CaseResult:
    started = time.perf_counter()
    proc = subprocess.Popen(
        command,
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    timed_out = False
    max_rss: int | None = None
    stdout = ""
    stderr = ""
    while True:
        if proc.poll() is not None:
            stdout, stderr = proc.communicate()
            break
        rss = peak_rss(proc.pid)
        if rss is not None:
            max_rss = max(max_rss or 0, rss)
            if memory_limit_mb is not None and rss > memory_limit_mb * 1024 * 1024:
                proc.kill()
                stdout, stderr = proc.communicate()
                timed_out = True
                stderr += f"\nMEMORY_LIMIT_EXCEEDED rss={rss}"
                break
        if time.perf_counter() - started > timeout_seconds:
            proc.kill()
            stdout, stderr = proc.communicate()
            timed_out = True
            break
        time.sleep(0.05)
    elapsed_ms = int((time.perf_counter() - started) * 1000)
    output = (stdout + "\n" + stderr).strip()
    excerpt = output[:1000]
    return CaseResult(
        suite=suite,
        name=name,
        command=command,
        elapsed_ms=elapsed_ms,
        exit_code=124 if timed_out else proc.returncode,
        timed_out=timed_out,
        peak_rss_bytes=max_rss,
        output_excerpt=excerpt,
    )


def compile_cases(root: Path) -> list[tuple[str, list[str]]]:
    files = [
        root / "tests" / "validation" / "20_all_features.spectra",
        root / "tests" / "validation" / "68_tensor_phase4_kernels.spectra",
        root / "tests" / "validation" / "71_tensor_phase5_autodiff.spectra",
        root / "tests" / "validation" / "77_concurrency_pipeline.spectra",
        root / "tests" / "validation" / "78_serving_foundations.spectra",
    ]
    return [(path.name, ["compile", str(path)]) for path in files]


def runtime_cases(root: Path) -> list[tuple[str, list[str]]]:
    files = [
        root / "tests" / "validation" / "68_tensor_phase4_kernels.spectra",
        root / "tests" / "validation" / "70_tensor_phase3_production.spectra",
        root / "tests" / "validation" / "71_tensor_phase5_autodiff.spectra",
        root / "tests" / "validation" / "77_concurrency_pipeline.spectra",
        root / "tests" / "validation" / "78_serving_foundations.spectra",
    ]
    return [(path.name, ["run", str(path)]) for path in files]


def package_cases(root: Path) -> list[tuple[str, list[str]]]:
    package_root = root / "tests" / "projects" / "valid" / "package_workspace"
    return [
        ("package_lock", ["package", "lock", "--root", str(package_root)]),
        ("package_check", ["package", "check", "--root", str(package_root)]),
        ("package_build", ["package", "build", "--root", str(package_root)]),
    ]


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="SpectraLang stress/soak runner")
    parser.add_argument("--iterations", type=int, default=2)
    parser.add_argument("--timeout-seconds", type=int, default=20)
    parser.add_argument("--memory-limit-mb", type=int, default=1024)
    parser.add_argument("--json-out", default="target/stress-soak-report.json")
    parser.add_argument(
        "--suite",
        action="append",
        choices=["compile", "runtime", "package"],
        help="suite to run; repeatable; default runs all",
    )
    args = parser.parse_args(argv)

    root = repo_root()
    binary = find_binary(root)
    suite_names = args.suite or ["compile", "runtime", "package"]
    suite_builders = {
        "compile": compile_cases,
        "runtime": runtime_cases,
        "package": package_cases,
    }

    results: list[CaseResult] = []
    failures: list[CaseResult] = []
    for iteration in range(args.iterations):
        for suite_name in suite_names:
            for case_name, case_args in suite_builders[suite_name](root):
                name = f"{case_name}#{iteration + 1}"
                command = [str(binary), *case_args]
                result = run_command(
                    root=root,
                    suite=suite_name,
                    name=name,
                    command=command,
                    timeout_seconds=args.timeout_seconds,
                    memory_limit_mb=args.memory_limit_mb,
                )
                results.append(result)
                if result.exit_code != 0 or result.timed_out:
                    failures.append(result)
                status = "PASS" if result not in failures else "FAIL"
                rss = "" if result.peak_rss_bytes is None else f" rss={result.peak_rss_bytes}"
                print(f"{status} {suite_name}:{name} {result.elapsed_ms}ms{rss}")

    report = {
        "schema": "spectralang.stress-soak-report.v1",
        "iterations": args.iterations,
        "timeout_seconds": args.timeout_seconds,
        "memory_limit_mb": args.memory_limit_mb,
        "total": len(results),
        "failed": len(failures),
        "results": [asdict(result) for result in results],
    }
    out = root / args.json_out
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"stress report written to {out}")
    if failures:
        print(f"{len(failures)} stress case(s) failed", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
