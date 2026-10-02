#!/usr/bin/env python3
"""Run the Phase 33 collection contract, JIT/AOT and release performance gate."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
REPORT = ROOT / "target" / "r3308-collections" / "validation.json"
PERFORMANCE_REPORT = ROOT / "docs" / "performance" / "phase33" / "r3301-collections.json"
FIXTURES = {
    "collection_capacity": "tests/validation/619_stdlib_collection_capacity.spectra",
    "advanced_collections": "tests/validation/620_stdlib_advanced_collections.spectra",
    "vector": "tests/validation/621_stdlib_vector.spectra",
    "collection_growth": "tests/validation/622_stdlib_collection_growth.spectra",
    "vector_growth": "tests/validation/623_stdlib_vector_growth.spectra",
    "hash_set": "tests/validation/624_stdlib_hash_set.spectra",
    "ordered_map": "tests/validation/625_stdlib_ordered_map.spectra",
    "priority_queue": "tests/validation/626_stdlib_priority_queue.spectra",
    "bitset": "tests/validation/627_stdlib_bitset.spectra",
    "disjoint_set": "tests/validation/628_stdlib_disjoint_set.spectra",
    "graph_search": "tests/validation/629_stdlib_collections_graph_search.spectra",
}


def run(command: list[str], *, timeout: int = 600) -> dict[str, Any]:
    try:
        completed = subprocess.run(
            command,
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        return {"command": command, "exit_code": -1, "output": str(exc)[-12000:]}
    return {"command": command, "exit_code": completed.returncode, "output": (completed.stdout or "")[-12000:]}


def require_step(steps: dict[str, Any], name: str, command: list[str], *, timeout: int = 600) -> str:
    result = run(command, timeout=timeout)
    steps[name] = result
    if result["exit_code"] != 0:
        raise RuntimeError(f"{name} failed (exit={result['exit_code']}):\n{result['output']}")
    return str(result["output"])


def require_semantic_negative(binary: Path, steps: dict[str, Any]) -> None:
    source = ROOT / "tests" / "errors" / "620_unsupported_aggregate_collection_key.spectra"
    result = run([str(binary), "check", "--json", str(source)], timeout=30)
    steps["aggregate_key_rejection"] = result
    if result["exit_code"] == 0:
        raise RuntimeError("aggregate HashSet key unexpectedly passed semantic analysis")
    start = result["output"].find("{")
    if start < 0:
        raise RuntimeError(f"semantic diagnostic did not contain JSON:\n{result['output']}")
    try:
        payload = json.loads(result["output"][start:])
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"semantic diagnostic JSON is malformed: {exc}") from exc
    files = payload.get("files")
    if not isinstance(files, list) or len(files) != 1:
        raise RuntimeError(f"expected one diagnostic file, found {files!r}")
    diagnostics = files[0].get("diagnostics")
    if not isinstance(diagnostics, list) or len(diagnostics) != 1:
        raise RuntimeError(f"expected exactly one semantic diagnostic, found {diagnostics!r}")
    diagnostic = diagnostics[0]
    message = str(diagnostic.get("message", ""))
    if (
        diagnostic.get("phase") != "semantic"
        or not ("HashSet" in message or "hash_set" in message)
        or "Point" not in message
    ):
        raise RuntimeError(f"unexpected aggregate-key diagnostic: {diagnostic!r}")
    result["validated_diagnostic"] = {
        "phase": diagnostic.get("phase"),
        "code": diagnostic.get("code"),
        "message": message,
    }


def require_benchmark_report(steps: dict[str, Any], *, samples: int, independent_runs: int) -> dict[str, Any]:
    if not PERFORMANCE_REPORT.is_file():
        raise RuntimeError(f"benchmark report is missing: {PERFORMANCE_REPORT}")
    try:
        payload = json.loads(PERFORMANCE_REPORT.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"benchmark report cannot be read: {exc}") from exc
    if payload.get("schema") != "spectra.phase33.r3301_collections.v1":
        raise RuntimeError("benchmark report has an unexpected schema")
    if payload.get("profile") != "release" or payload.get("correctness_passed") is not True:
        raise RuntimeError("benchmark report does not confirm a correct release-profile run")
    expected_count = samples * independent_runs
    for profile in ("jit", "aot"):
        scenarios = payload.get("results", {}).get(profile)
        if not isinstance(scenarios, dict) or len(scenarios) != 96:
            raise RuntimeError(f"{profile} report must contain 96 collection scenarios")
        for name, result in scenarios.items():
            if result.get("sample_count") != expected_count:
                raise RuntimeError(f"{profile}/{name} has {result.get('sample_count')} samples, expected {expected_count}")
            if not result.get("checksums"):
                raise RuntimeError(f"{profile}/{name} is missing correctness checksums")
    storage = payload.get("results", {}).get("rust_storage")
    if not isinstance(storage, dict) or len(storage) != 9:
        raise RuntimeError("storage probe must contain nine VecDeque/Vec/fixed-array scenarios")
    for name, result in storage.items():
        if result.get("sample_count") != expected_count:
            raise RuntimeError(f"rust_storage/{name} has an unexpected sample count")
        if not result.get("backing_bytes_range"):
            raise RuntimeError(f"rust_storage/{name} is missing backing storage sizes")
    comparisons = payload.get("comparisons", {})
    large_scalar = comparisons.get("hash_set_vs_insertion_ordered_set", {}).get("scalar.large", {})
    if not large_scalar.get("candidate_wins_every_jit_group") or not large_scalar.get("candidate_wins_every_aot_group"):
        raise RuntimeError("HashSet did not beat insertion-ordered Set in every large scalar release group")
    vector = payload.get("vector_t_go_no_go", {})
    if vector.get("decision") not in {"go_candidate", "no_go_keep_current_surface"}:
        raise RuntimeError("benchmark report has no complete Vector<T> go/no-go result")
    vector_vs_list = comparisons.get("vector_vs_list_indexed_append_and_traversal", {})
    if set(vector_vs_list) != {"small", "medium", "large"}:
        raise RuntimeError("benchmark report is missing Vector<T> versus List<T> JIT/AOT measurements")
    for size, result in vector_vs_list.items():
        if not result.get("jit_independent_group_speed_ratios") or not result.get("aot_independent_group_speed_ratios"):
            raise RuntimeError(f"Vector<T> versus List<T> is missing independent JIT/AOT groups at {size}")
    fast_abi = payload.get("vector_fast_abi_comparison", {})
    if fast_abi.get("meets_repeatable_10_percent_threshold") is not True:
        raise RuntimeError("Vector<T> fast ABI does not meet the repeatable 10% release improvement threshold")
    if set(fast_abi.get("profiles", {})) != {"small", "medium", "large"}:
        raise RuntimeError("Vector<T> fast ABI comparison is missing one or more workload sizes")
    for size, profiles in fast_abi["profiles"].items():
        for profile in ("jit", "aot"):
            result = profiles.get(profile, {})
            group_gains = result.get("estimated_independent_group_latency_reductions_percent_normalized_by_list", [])
            median_gain = result.get("estimated_latency_reduction_percent_normalized_by_list_median")
            if len(group_gains) != independent_runs or median_gain is None:
                raise RuntimeError(f"Vector<T> fast ABI comparison is incomplete for {profile}/{size}")
            if median_gain < 10.0 or any(gain < 10.0 for gain in group_gains):
                raise RuntimeError(f"Vector<T> fast ABI improvement is below 10% in {profile}/{size}")
    steps["benchmark_report"] = {
        "path": PERFORMANCE_REPORT.relative_to(ROOT).as_posix(),
        "scenario_count_per_profile": 96,
        "sample_count_per_scenario": expected_count,
        "vector_decision": vector.get("decision"),
        "large_scalar_hashset_wins_jit": large_scalar.get("candidate_wins_every_jit_group"),
        "large_scalar_hashset_wins_aot": large_scalar.get("candidate_wins_every_aot_group"),
        "vector_fast_abi_minimum_estimated_reduction_percent": fast_abi.get(
            "minimum_estimated_reduction_percent_across_medians_and_groups"
        ),
        "vector_fast_abi_threshold_passed": True,
    }
    return payload


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--samples", type=int, default=8)
    parser.add_argument("--warmups", type=int, default=2)
    parser.add_argument("--independent-runs", type=int, default=3)
    args = parser.parse_args()
    if args.samples < 5 or args.warmups < 2 or args.independent_runs < 3:
        parser.error("release gate requires at least 2 warmups, 5 samples and 3 independent groups")

    REPORT.parent.mkdir(parents=True, exist_ok=True)
    steps: dict[str, Any] = {}
    error: str | None = None
    benchmark_payload: dict[str, Any] | None = None
    release_binary = ROOT / "target" / "release" / ("spectralang.exe" if os.name == "nt" else "spectralang")
    try:
        generated = ROOT / "target" / "r3308-collections" / "stdlib.generated.toml"
        catalog = ROOT / "packages" / "spectra-contract" / "catalog" / "stdlib.toml"
        generated.parent.mkdir(parents=True, exist_ok=True)
        generated.write_bytes(catalog.read_bytes())
        require_step(
            steps,
            "stdlib_catalog_generator",
            [
                sys.executable,
                "scripts/generate_stdlib_catalog.py",
                "--output",
                "target/r3308-collections/stdlib.generated.toml",
            ],
        )
        if not generated.is_file() or generated.read_bytes() != catalog.read_bytes():
            raise RuntimeError("generated stdlib catalog differs from checked-in catalog")
        steps["stdlib_catalog_matches_generator"] = {"status": "passed", "path": catalog.relative_to(ROOT).as_posix()}

        catalog_data = tomllib.loads(catalog.read_text(encoding="utf-8"))
        catalog_entries = {entry["path"]: entry for entry in catalog_data.get("entry", [])}
        beta_types = {
            "std.collections.Vector",
            "std.collections.HashSet",
            "std.collections.OrderedMap",
            "std.collections.PriorityQueue",
            "std.collections.BitSet",
            "std.collections.DisjointSet",
        }
        beta_function_prefixes = (
            "std.collections.vector_",
            "std.collections.hash_set_",
            "std.collections.ordered_map_",
            "std.collections.priority_queue_",
            "std.collections.bitset_",
            "std.collections.disjoint_set_",
        )
        beta_capacity_functions = {
            f"std.collections.{collection}_{operation}"
            for collection in ("list", "map", "set", "stack", "queue")
            for operation in ("with_capacity", "reserve", "capacity")
        }
        beta_paths = beta_types | beta_capacity_functions
        beta_paths.update(
            path for path in catalog_entries if path.startswith(beta_function_prefixes)
        )
        missing_beta = sorted(path for path in beta_paths if catalog_entries.get(path, {}).get("maturity") != "beta")
        if missing_beta:
            raise RuntimeError(f"Phase 33 public collection entries are missing beta maturity: {missing_beta}")

        sys.path.insert(0, str(ROOT / "scripts"))
        import validate_r3007_stdlib_contract as stdlib_audit  # noqa: E402

        manifest = tomllib.loads((ROOT / "scripts" / "stdlib_contract.toml").read_text(encoding="utf-8"))
        manifest_errors = stdlib_audit.validate_manifest(ROOT, manifest)
        if manifest_errors:
            raise RuntimeError(f"stdlib contract manifest failed structural validation: {manifest_errors}")
        missing_phase33_probe = [
            path
            for path in sorted(beta_paths)
            if not any(
                probe.get("id", "").startswith("phase33-")
                for probe in stdlib_audit.probe_matches(path, manifest)
            )
        ]
        if missing_phase33_probe:
            raise RuntimeError(f"Phase 33 stdlib contract has no dedicated fixture coverage for {missing_phase33_probe}")
        steps["phase33_contract_manifest"] = {
            "status": "passed",
            "beta_catalog_paths": len(beta_paths),
            "phase33_probes": ["phase33-collection-capacity", "phase33-advanced-collections", "phase33-vector"],
        }

        for name, command in (
            ("lowering_generation", [sys.executable, "scripts/generate_lowering_tables.py", "--check"]),
            ("host_call_generation", [sys.executable, "scripts/generate_host_calls.py", "--check"]),
            (
                "catalog_schema",
                [sys.executable, "scripts/validate_r3206_catalog_schema.py", "--report", "target/r3308-collections/catalog-schema.json"],
            ),
        ):
            require_step(steps, name, command)

        require_step(steps, "runtime_collection_tests", ["cargo", "test", "-p", "spectra-runtime", "--offline", "collection", "--", "--nocapture"], timeout=1800)
        require_step(steps, "release_build", ["cargo", "build", "--release", "-p", "spectra-cli", "--offline"], timeout=3600)
        if not release_binary.is_file():
            raise RuntimeError(f"release CLI is missing after successful build: {release_binary}")

        outputs: list[dict[str, Any]] = []
        for name, relative in FIXTURES.items():
            source = ROOT / relative
            result = run([str(release_binary), "run", str(source)], timeout=120)
            steps[f"jit_{name}"] = result
            if result["exit_code"] != 0:
                raise RuntimeError(f"release JIT fixture {relative} failed:\n{result['output']}")
            outputs.append({"name": name, "mode": "jit", "source": relative, "exit_code": 0})

        aot_dir = ROOT / "target" / "r3308-collections" / "aot"
        aot_dir.mkdir(parents=True, exist_ok=True)
        for name, relative in FIXTURES.items():
            source = ROOT / relative
            output = aot_dir / f"{name}{'.exe' if os.name == 'nt' else ''}"
            compile_result = run(
                [str(release_binary), "compile", "--debug-info=none", "--emit-exe", str(output), str(source)],
                timeout=300,
            )
            steps[f"aot_compile_{name}"] = compile_result
            if compile_result["exit_code"] != 0 or not output.is_file():
                raise RuntimeError(f"AOT compile for {relative} failed:\n{compile_result['output']}")
            run_result = run([str(output)], timeout=120)
            steps[f"aot_run_{name}"] = run_result
            if run_result["exit_code"] != 0:
                raise RuntimeError(f"AOT executable for {relative} failed:\n{run_result['output']}")
            outputs.append({"name": name, "mode": "aot", "source": relative, "exit_code": 0})
        steps["jit_aot_fixtures"] = outputs

        require_semantic_negative(release_binary, steps)
        benchmark_command = [
            sys.executable,
            "scripts/benchmark_r3301_collections.py",
            "--binary",
            str(release_binary),
            "--out",
            "docs/performance/phase33/r3301-collections.json",
            "--warmups",
            str(args.warmups),
            "--samples",
            str(args.samples),
            "--independent-runs",
            str(args.independent_runs),
        ]
        require_step(steps, "release_benchmark", benchmark_command, timeout=7200)
        benchmark_payload = require_benchmark_report(steps, samples=args.samples, independent_runs=args.independent_runs)
        roadmap = tomllib.loads((ROOT / "roadmap" / "roadmap.toml").read_text(encoding="utf-8"))
        phase_items = {item.get("id"): item for item in roadmap.get("items", []) if str(item.get("id", "")).startswith("R-33")}
        expected_ids = {f"R-{number}" for number in range(3301, 3310)}
        if set(phase_items) != expected_ids:
            raise RuntimeError(f"Phase 33 roadmap IDs differ from expected set: {sorted(phase_items)}")
        for item_id, item in phase_items.items():
            if item.get("phase") != "phase_33" or item.get("status") not in {"in_progress", "complete"}:
                raise RuntimeError(f"invalid Phase 33 tracker metadata for {item_id}: {item}")
        steps["roadmap_phase33_integrity"] = {"status": "passed", "item_ids": sorted(phase_items)}
    except (OSError, RuntimeError, subprocess.SubprocessError, tomllib.TOMLDecodeError) as exc:
        error = str(exc)

    report = {
        "schema": "spectra.phase33.r3308_collections_validation.v1",
        "status": "passed" if error is None else "failed",
        "phase": "phase_33",
        "steps": steps,
        "error": error,
        "performance_report": PERFORMANCE_REPORT.relative_to(ROOT).as_posix() if benchmark_payload is not None else None,
    }
    REPORT.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8", newline="\n")
    if error is not None:
        print(f"R-3308 collection release gate failed: {error}", file=sys.stderr)
        print(f"report: {REPORT}", file=sys.stderr)
        return 1
    print("R-3308 collection release gate passed")
    print(json.dumps(steps.get("benchmark_report", {}), sort_keys=True))
    print(f"report: {REPORT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
