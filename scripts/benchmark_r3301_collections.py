#!/usr/bin/env python3
"""Capture reproducible JIT/AOT collection baselines and storage prototypes.

The Spectra fixture times collection work internally with std.time, excluding
CLI startup and compilation. Every sample also checks collection contents,
ordering, lengths and checksums. A small Rust probe isolates VecDeque, Vec and
fixed-array storage costs for the conditional Vector<T> decision; its timings
are kept separate from Spectra host-call timings.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import shutil
import statistics
import subprocess
import sys
from pathlib import Path
from typing import Any

try:
    from scripts.benchmark_r3104_codegen import source_tree_fingerprint
except ModuleNotFoundError:  # pragma: no cover
    from benchmark_r3104_codegen import source_tree_fingerprint  # type: ignore[no-redef]


ROOT = Path(__file__).resolve().parents[1]
SPECTRA_SOURCE = ROOT / "benchmarks" / "collections" / "r3301_collections.spectra"
RUST_SOURCE = ROOT / "benchmarks" / "collections" / "r3301_vector_storage.rs"
DEFAULT_OUTPUT = Path("docs/performance/phase33/r3301-collections.json")
VECTOR_FAST_ABI_BASELINE = ROOT / "docs" / "performance" / "phase33" / "r3301-vector-generic-abi-before.json"
WARMUPS = 2
TIMED_SAMPLES = 8
INDEPENDENT_RUNS = 3
TIMEOUT_SECONDS = 300
SEED = 17
SIZES = {"small": 64, "medium": 512, "large": 4096}
STRING_LENGTHS = (8, 64, 256)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def percentile(samples: list[int], fraction: float) -> int:
    ordered = sorted(samples)
    if not ordered:
        return 0
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def run_command(command: list[str], *, cwd: Path = ROOT) -> tuple[int, str]:
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=TIMEOUT_SECONDS,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        return -1, f"timeout after {TIMEOUT_SECONDS}s: {exc}"
    return completed.returncode, (completed.stdout or "")[-12000:]


def parse_spectra_output(output: str) -> dict[str, dict[str, int]]:
    records: dict[str, dict[str, int]] = {}
    for line in output.splitlines():
        if not line.startswith("R3301|"):
            continue
        fields = line.split("|")
        if len(fields) != 9:
            raise RuntimeError(f"malformed collection benchmark record: {line!r}")
        name = fields[1]
        if name in records:
            raise RuntimeError(f"duplicate collection benchmark scenario: {name}")
        record = {
            "construction_ns": int(fields[2]),
            "operation_ns": int(fields[3]),
            "operations": int(fields[4]),
            "checksum": int(fields[5]),
            "capacity_before": int(fields[6]),
            "capacity_after": int(fields[7]),
            "key_length": int(fields[8]),
        }
        if record["operation_ns"] <= 0 or record["operations"] <= 0:
            raise RuntimeError(f"non-positive benchmark duration or operation count: {line}")
        if record["capacity_before"] >= 0 and record["capacity_after"] < record["capacity_before"]:
            raise RuntimeError(f"capacity shrank during a growth workload: {line}")
        records[name] = record
    if not records:
        raise RuntimeError("Spectra collection benchmark emitted no R3301 records")
    return records


def parse_vector_output(output: str) -> dict[str, dict[str, int]]:
    records: dict[str, dict[str, int]] = {}
    for line in output.splitlines():
        if not line.startswith("R3301VECTOR|"):
            continue
        fields = line.split("|")
        if len(fields) != 9:
            raise RuntimeError(f"malformed storage-prototype record: {line!r}")
        name = f"{fields[1]}.{fields[2]}"
        if name in records:
            raise RuntimeError(f"duplicate storage-prototype scenario: {name}")
        record = {
            "operation_ns": int(fields[3]),
            "operations": int(fields[4]),
            "checksum": int(fields[5]),
            "capacity_before": int(fields[6]),
            "capacity_after": int(fields[7]),
            "backing_bytes": int(fields[8]),
        }
        expected_size = int(fields[2])
        expected_checksum = expected_size * (expected_size - 1) // 2
        if record["checksum"] != expected_checksum:
            raise RuntimeError(f"storage prototype produced an incorrect checksum: {line}")
        if record["operation_ns"] <= 0 or record["operations"] != expected_size * 2:
            raise RuntimeError(f"invalid storage-prototype timing record: {line}")
        records[name] = record
    if len(records) != len(SIZES) * 3:
        raise RuntimeError(f"expected {len(SIZES) * 3} storage-prototype records, got {len(records)}")
    return records


def expected_spectra_scenarios() -> set[str]:
    expected: set[str] = set()
    for size_name in SIZES:
        for collection in ("list", "vector", "map-int", "set-int", "stack", "queue"):
            for mode in ("default", "capacity"):
                expected.add(f"{collection}.{size_name}.{mode}")
        for key_length in STRING_LENGTHS:
            for collection in ("map-string", "set-string"):
                for mode in ("default", "capacity"):
                    expected.add(f"{collection}.{key_length}.{size_name}.{mode}")
        expected.add(f"hash-set-int.{size_name}")
        for key_length in STRING_LENGTHS:
            expected.add(f"hash-set-string.{key_length}.{size_name}")
        for collection in ("ordered-map", "priority-queue", "bitset", "disjoint-set"):
            expected.add(f"{collection}.{size_name}")
    return expected


def check_spectra_scenarios(records: dict[str, dict[str, int]]) -> None:
    expected = expected_spectra_scenarios()
    actual = set(records)
    if actual != expected:
        missing = sorted(expected - actual)
        extra = sorted(actual - expected)
        raise RuntimeError(f"benchmark scenario matrix mismatch; missing={missing}, extra={extra}")
    for name, record in records.items():
        size_name = next(key for key in SIZES if f".{key}" in name)
        size = SIZES[size_name]
        if name.startswith(("ordered-map.", "priority-queue.", "vector.")) and record["checksum"] != size * (size - 1) // 2:
            raise RuntimeError(f"unexpected ordering checksum for {name}: {record['checksum']}")
        if name.startswith("bitset.") and record["checksum"] != size:
            raise RuntimeError(f"unexpected BitSet membership checksum for {name}: {record['checksum']}")
        if name.startswith("disjoint-set.") and not 0 <= record["checksum"] <= size:
            raise RuntimeError(f"unexpected union count for {name}: {record['checksum']}")
        if ".capacity" in name and record["capacity_before"] == 0:
            raise RuntimeError(f"capacity-aware constructor reported zero capacity: {name}")


def compile_aot(binary: Path, source: Path, output: Path) -> dict[str, Any]:
    output.parent.mkdir(parents=True, exist_ok=True)
    command = [str(binary), "compile", "--debug-info=none", "--emit-exe", str(output), str(source)]
    code, text = run_command(command)
    if code != 0 or not output.is_file():
        raise RuntimeError(f"AOT compilation failed (exit={code}):\n{text}")
    return {"command": command, "exit_code": code, "output": output.relative_to(ROOT).as_posix(), "sha256": sha256_file(output), "output_tail": text}


def compile_rust(source: Path, output: Path) -> dict[str, Any]:
    rustc = shutil.which("rustc")
    if rustc is None:
        raise RuntimeError("rustc is required for the isolated Vec/VecDeque/fixed-array storage probe")
    output.parent.mkdir(parents=True, exist_ok=True)
    command = [rustc, "--edition=2021", "-C", "opt-level=3", str(source), "-o", str(output)]
    code, text = run_command(command)
    if code != 0 or not output.is_file():
        raise RuntimeError(f"Rust storage probe compilation failed (exit={code}):\n{text}")
    return {"command": command, "exit_code": code, "output": output.relative_to(ROOT).as_posix(), "sha256": sha256_file(output), "output_tail": text}


def run_parsed(command: list[str], parser, *, label: str) -> dict[str, dict[str, int]]:
    code, output = run_command(command)
    if code != 0:
        raise RuntimeError(f"{label} failed (exit={code}):\n{output}")
    return parser(output)


def aggregate_samples(samples: list[dict[str, int]], *, storage_probe: bool = False) -> dict[str, Any]:
    operation = [sample["operation_ns"] for sample in samples]
    throughput = [sample["operations"] * 1_000_000_000 / elapsed for sample, elapsed in zip(samples, operation)]
    summary: dict[str, Any] = {
        "sample_count": len(samples),
        "median_operation_ns": int(statistics.median(operation)),
        "p95_operation_ns": percentile(operation, 0.95),
        "median_throughput_ops_s": round(statistics.median(throughput), 3),
        "p95_throughput_ops_s": round(percentile([int(value) for value in throughput], 0.95), 3),
        "operation_samples_ns": operation,
        "throughput_samples_ops_s": [round(value, 3) for value in throughput],
        "operations_per_sample": samples[0]["operations"],
        "checksums": sorted({sample["checksum"] for sample in samples}),
        "capacity_before_range": [min(s["capacity_before"] for s in samples), max(s["capacity_before"] for s in samples)],
        "capacity_after_range": [min(s["capacity_after"] for s in samples), max(s["capacity_after"] for s in samples)],
    }
    if "construction_ns" in samples[0]:
        construction = [sample["construction_ns"] for sample in samples]
        summary["median_construction_ns"] = int(statistics.median(construction))
        summary["p95_construction_ns"] = percentile(construction, 0.95)
        summary["construction_samples_ns"] = construction
    if storage_probe:
        summary["backing_bytes_range"] = [min(s["backing_bytes"] for s in samples), max(s["backing_bytes"] for s in samples)]
    if "key_length" in samples[0]:
        key_length = samples[0]["key_length"]
        summary["key_length_bytes"] = key_length
        if key_length > 0:
            # Each timed Map/Set/HashSet call normalizes its string argument
            # into an owned key. The copied-byte count is derived from the
            # public workload; allocator metadata is not measured here.
            summary["derived_owned_key_normalizations_per_sample"] = samples[0]["operations"]
            summary["derived_owned_key_bytes_copied_per_sample"] = samples[0]["operations"] * key_length
    return summary


def measure(
    binary: Path,
    output_root: Path,
    *,
    warmups: int,
    timed_samples: int,
    independent_runs: int,
) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any], list[list[str]]]:
    if not binary.is_file():
        raise RuntimeError(f"release CLI does not exist: {binary}")
    if not SPECTRA_SOURCE.is_file() or not RUST_SOURCE.is_file():
        raise RuntimeError("checked-in Phase 33 benchmark sources are missing")
    aot_path = (output_root / "spectra-aot.exe").resolve()
    rust_path = (output_root / "vector-storage.exe").resolve()
    aot_compile = compile_aot(binary, SPECTRA_SOURCE, aot_path)
    rust_compile = compile_rust(RUST_SOURCE, rust_path)
    commands = {
        "jit": [str(binary), "run", str(SPECTRA_SOURCE)],
        "aot": [str(aot_path)],
        "rust_storage": [str(rust_path)],
    }
    parsers = {"jit": parse_spectra_output, "aot": parse_spectra_output, "rust_storage": parse_vector_output}
    per_profile: dict[str, dict[str, list[dict[str, int]]]] = {key: {} for key in commands}
    group_medians: dict[str, dict[str, list[int]]] = {key: {} for key in commands}

    def capture(profile: str) -> dict[str, dict[str, int]]:
        return run_parsed(commands[profile], parsers[profile], label=f"{profile} benchmark")

    profile_orders: list[list[str]] = []
    profile_names = list(commands)
    for independent in range(independent_runs):
        rotation = independent % len(profile_names)
        profile_order = profile_names[rotation:] + profile_names[:rotation]
        profile_orders.append(profile_order)
        warmup_records: dict[str, dict[str, dict[str, int]]] = {}
        for profile in profile_order:
            warmup_records[profile] = capture(profile)
            if profile != "rust_storage":
                check_spectra_scenarios(warmup_records[profile])
        for profile in profile_order:
            reference = warmup_records[profile]
            expected_names = set(reference)
            group: dict[str, list[dict[str, int]]] = {name: [] for name in expected_names}
            for _ in range(warmups - 1):
                current = capture(profile)
                if set(current) != expected_names:
                    raise RuntimeError(f"{profile} warmup scenario set changed")
                for name, sample in current.items():
                    if sample["checksum"] != reference[name]["checksum"]:
                        raise RuntimeError(f"{profile} correctness checksum changed in warmup for {name}")
            for sample_index in range(timed_samples):
                current = capture(profile)
                if set(current) != expected_names:
                    raise RuntimeError(f"{profile} timed scenario set changed")
                if profile != "rust_storage":
                    check_spectra_scenarios(current)
                for name, sample in current.items():
                    if sample["checksum"] != reference[name]["checksum"]:
                        raise RuntimeError(f"{profile} correctness checksum changed for {name}")
                    group[name].append(sample)
            for name, samples in group.items():
                per_profile[profile].setdefault(name, []).extend(samples)
                group_medians[profile].setdefault(name, []).append(int(statistics.median(s["operation_ns"] for s in samples)))
        print(f"[r3301-collections] completed independent group {independent + 1}/{independent_runs}", flush=True)

    summaries: dict[str, Any] = {}
    for profile, scenarios in per_profile.items():
        summaries[profile] = {
            name: {
                **aggregate_samples(samples, storage_probe=profile == "rust_storage"),
                "independent_group_medians_ns": group_medians[profile][name],
            }
            for name, samples in sorted(scenarios.items())
        }
    return summaries, aot_compile, rust_compile, profile_orders


def vector_decision(results: dict[str, Any]) -> dict[str, Any]:
    storage = results["rust_storage"]
    comparisons: dict[str, Any] = {}
    eligible = []
    for size_name, size in SIZES.items():
        deque = storage[f"vec_deque.{size}"]
        vector = storage[f"vec_prototype.{size}"]
        ratio = deque["median_operation_ns"] / vector["median_operation_ns"] if vector["median_operation_ns"] else 0.0
        memory_ratio = (
            vector["backing_bytes_range"][1] / deque["backing_bytes_range"][1]
            if deque["backing_bytes_range"][1]
            else None
        )
        group_ratios = [left / right if right else 0.0 for left, right in zip(deque["independent_group_medians_ns"], vector["independent_group_medians_ns"])]
        qualifies = len(group_ratios) >= INDEPENDENT_RUNS and all(value >= 1.10 for value in group_ratios) and memory_ratio is not None and memory_ratio <= 1.05
        if qualifies:
            eligible.append(size_name)
        comparisons[size_name] = {
            "vecdeque_to_vec_speed_ratio": round(ratio, 4),
            "vec_speedup_percent": round((1.0 - 1.0 / ratio) * 100.0, 2) if ratio else None,
            "vec_to_vecdeque_backing_bytes_ratio": round(memory_ratio, 4) if memory_ratio is not None else None,
            "independent_group_speed_ratios": [round(value, 4) for value in group_ratios],
            "repeatable_10_percent_speed_and_5_percent_memory_gate": qualifies,
        }
    return {
        "method": "isolated optimized Rust storage probe; not Spectra ABI wall-clock",
        "gate": "all three independent groups must show at least 10% Vec speedup and Vec backing bytes must be no more than 5% above VecDeque",
        "decision": "go_candidate" if eligible else "no_go_keep_current_surface",
        "qualifying_sizes": eligible,
        "comparisons": comparisons,
    }


def collection_comparisons(results: dict[str, Any]) -> dict[str, Any]:
    def compare(left: str, right: str) -> dict[str, Any]:
        lhs = results["jit"][left]
        rhs = results["jit"][right]
        aot_lhs = results["aot"][left]
        aot_rhs = results["aot"][right]
        jit_group_ratios = [
            left_ns / right_ns if right_ns else 0.0
            for left_ns, right_ns in zip(lhs["independent_group_medians_ns"], rhs["independent_group_medians_ns"])
        ]
        aot_group_ratios = [
            left_ns / right_ns if right_ns else 0.0
            for left_ns, right_ns in zip(aot_lhs["independent_group_medians_ns"], aot_rhs["independent_group_medians_ns"])
        ]
        return {
            "jit_reference_to_candidate_speed_ratio": round(lhs["median_operation_ns"] / rhs["median_operation_ns"], 4),
            "aot_reference_to_candidate_speed_ratio": round(aot_lhs["median_operation_ns"] / aot_rhs["median_operation_ns"], 4),
            "jit_independent_group_speed_ratios": [round(value, 4) for value in jit_group_ratios],
            "aot_independent_group_speed_ratios": [round(value, 4) for value in aot_group_ratios],
            "candidate_wins_every_jit_group": bool(jit_group_ratios) and all(value > 1.0 for value in jit_group_ratios),
            "candidate_wins_every_aot_group": bool(aot_group_ratios) and all(value > 1.0 for value in aot_group_ratios),
            "jit_candidate_samples_ns": rhs["operation_samples_ns"],
            "jit_reference_samples_ns": lhs["operation_samples_ns"],
            "aot_candidate_samples_ns": aot_rhs["operation_samples_ns"],
            "aot_reference_samples_ns": aot_lhs["operation_samples_ns"],
        }

    hash_set_vs_set: dict[str, Any] = {}
    vector_vs_list: dict[str, Any] = {}
    for size_name in SIZES:
        hash_set_vs_set[f"scalar.{size_name}"] = compare(f"set-int.{size_name}.capacity", f"hash-set-int.{size_name}")
        vector_vs_list[size_name] = compare(f"list.{size_name}.capacity", f"vector.{size_name}.capacity")
        for key_length in STRING_LENGTHS:
            hash_set_vs_set[f"string.{key_length}.{size_name}"] = compare(
                f"set-string.{key_length}.{size_name}.capacity",
                f"hash-set-string.{key_length}.{size_name}",
            )
    bitset_vs_hash: dict[str, Any] = {}
    for size_name, size in SIZES.items():
        bitset = results["jit"][f"bitset.{size_name}"]
        hashed = results["jit"][f"hash-set-int.{size_name}"]
        bitset_vs_hash[size_name] = {
            "hashset_to_bitset_speed_ratio": round(hashed["median_operation_ns"] / bitset["median_operation_ns"], 4),
            "bitset_backing_bytes": bitset["capacity_after_range"][1] // 8,
            "hashset_slot_capacity": hashed["capacity_after_range"],
            "bitset_logical_bytes_per_index": round((bitset["capacity_after_range"][1] // 8) / size, 6),
        }
    capacity_reservation: dict[str, Any] = {}
    for size_name in SIZES:
        for collection in ("list", "vector", "map-int", "set-int", "stack", "queue"):
            default = results["jit"][f"{collection}.{size_name}.default"]
            reserved = results["jit"][f"{collection}.{size_name}.capacity"]
            default_ns = default["median_operation_ns"]
            reserved_ns = reserved["median_operation_ns"]
            aot_default = results["aot"][f"{collection}.{size_name}.default"]
            aot_reserved = results["aot"][f"{collection}.{size_name}.capacity"]
            jit_group_ratios = [
                before / after if after else 0.0
                for before, after in zip(
                    default["independent_group_medians_ns"],
                    reserved["independent_group_medians_ns"],
                )
            ]
            aot_group_ratios = [
                before / after if after else 0.0
                for before, after in zip(
                    aot_default["independent_group_medians_ns"],
                    aot_reserved["independent_group_medians_ns"],
                )
            ]
            capacity_reservation[f"{collection}.{size_name}"] = {
                "default_to_reserved_operation_speed_ratio": round(default_ns / reserved_ns, 4) if reserved_ns else None,
                "aot_default_to_reserved_operation_speed_ratio": round(
                    aot_default["median_operation_ns"] / aot_reserved["median_operation_ns"], 4
                ) if aot_reserved["median_operation_ns"] else None,
                "jit_independent_group_ratios": [round(value, 4) for value in jit_group_ratios],
                "aot_independent_group_ratios": [round(value, 4) for value in aot_group_ratios],
                "default_median_construction_ns": default["median_construction_ns"],
                "reserved_median_construction_ns": reserved["median_construction_ns"],
                "aot_default_median_construction_ns": aot_default["median_construction_ns"],
                "aot_reserved_median_construction_ns": aot_reserved["median_construction_ns"],
                "default_capacity_after_range": default["capacity_after_range"],
                "reserved_capacity_before_range": reserved["capacity_before_range"],
            }
    return {
        "hash_set_vs_insertion_ordered_set": hash_set_vs_set,
        "vector_vs_list_indexed_append_and_traversal": vector_vs_list,
        "bitset_vs_hash_set_dense_membership": bitset_vs_hash,
        "capacity_reservation": capacity_reservation,
    }


def vector_fast_abi_comparison(comparisons: dict[str, Any]) -> dict[str, Any]:
    baseline = json.loads(VECTOR_FAST_ABI_BASELINE.read_text(encoding="utf-8"))
    if baseline.get("schema") != "spectra.phase33.r3301_vector_generic_abi_baseline.v1":
        raise RuntimeError("Vector<T> pre-optimization baseline has an unexpected schema")

    before = baseline.get("measurements", {})
    after = comparisons.get("vector_vs_list_indexed_append_and_traversal", {})
    if set(before) != set(SIZES) or set(after) != set(SIZES):
        raise RuntimeError("Vector<T> fast ABI comparison requires small, medium and large measurements")

    profiles: dict[str, Any] = {}
    reductions: list[float] = []
    for size_name in SIZES:
        previous = before[size_name]
        current = after[size_name]
        profiles[size_name] = {}
        for profile in ("jit", "aot"):
            previous_median = float(previous[f"{profile}_median_list_to_vector_speed_ratio"])
            previous_groups = [float(value) for value in previous[f"{profile}_independent_group_list_to_vector_speed_ratios"]]
            current_median = float(current[f"{profile}_reference_to_candidate_speed_ratio"])
            current_groups = [float(value) for value in current[f"{profile}_independent_group_speed_ratios"]]
            if not previous_groups or len(previous_groups) != len(current_groups):
                raise RuntimeError(f"Vector<T> {profile} group count differs from the checked baseline at {size_name}")
            median_reduction = 100.0 * (1.0 - previous_median / current_median) if current_median else 0.0
            group_reductions = [
                100.0 * (1.0 - before_ratio / after_ratio) if after_ratio else 0.0
                for before_ratio, after_ratio in zip(previous_groups, current_groups)
            ]
            reductions.extend([median_reduction, *group_reductions])
            profiles[size_name][profile] = {
                "baseline_list_to_vector_speed_ratio": round(previous_median, 4),
                "current_list_to_vector_speed_ratio": round(current_median, 4),
                "estimated_latency_reduction_percent_normalized_by_list_median": round(median_reduction, 2),
                "baseline_independent_group_speed_ratios": [round(value, 4) for value in previous_groups],
                "current_independent_group_speed_ratios": [round(value, 4) for value in current_groups],
                "estimated_independent_group_latency_reductions_percent_normalized_by_list": [
                    round(value, 2) for value in group_reductions
                ],
            }

    minimum_reduction = min(reductions)
    return {
        "baseline_path": VECTOR_FAST_ABI_BASELINE.relative_to(ROOT).as_posix(),
        "baseline_provenance": baseline["provenance"],
        "workload": "indexed append and traversal; List<T> is the per-run reference",
        "ratio_semantics": "List median operation latency / Vector median operation latency; values above 1 mean Vector<T> is faster",
        "reduction_method": "estimated from the before/after List-to-Vector ratio; assumes List<T> reference latency is stable between release runs",
        "profiles": profiles,
        "minimum_estimated_reduction_percent_across_medians_and_groups": round(minimum_reduction, 2),
        "meets_repeatable_10_percent_threshold": minimum_reduction >= 10.0,
    }


def capture(*, binary: Path, out: Path, warmups: int, timed_samples: int, independent_runs: int) -> dict[str, Any]:
    binary = binary.resolve()
    output_root = ROOT / "target" / "r3301-collections"
    results, aot_compile, rust_compile, profile_orders = measure(
        binary,
        output_root,
        warmups=warmups,
        timed_samples=timed_samples,
        independent_runs=independent_runs,
    )
    revision = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True, capture_output=True, check=False)
    if revision.returncode != 0 or not revision.stdout.strip():
        raise RuntimeError("unable to resolve repository revision")
    vector = vector_decision(results)
    working_tree = subprocess.run(
        ["git", "status", "--porcelain", "--untracked-files=all"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    if working_tree.returncode != 0:
        raise RuntimeError("unable to determine complete working-tree status")
    comparisons = collection_comparisons(results)
    payload = {
        "schema": "spectra.phase33.r3301_collections.v1",
        "task": "R-3301/R-3308",
        "classification": "release_performance_baseline",
        "git_revision": revision.stdout.strip(),
        "source_tree_fingerprint": source_tree_fingerprint(ROOT),
        "working_tree_dirty": bool(working_tree.stdout.strip()),
        "profile": "release",
        "platform": platform.platform(),
        "architecture": platform.machine(),
        "processor": platform.processor(),
        "logical_cpu_count": os.cpu_count(),
        "spectralang_binary": binary.relative_to(ROOT).as_posix() if binary.is_relative_to(ROOT) else str(binary),
        "spectralang_binary_sha256": sha256_file(binary),
        "spectra_source": SPECTRA_SOURCE.relative_to(ROOT).as_posix(),
        "spectra_source_sha256": sha256_file(SPECTRA_SOURCE),
        "rust_storage_source": RUST_SOURCE.relative_to(ROOT).as_posix(),
        "rust_storage_source_sha256": sha256_file(RUST_SOURCE),
        "measurement_policy": {
            "clock": "std.time.monotonic_nanos inside each Spectra sample; excludes CLI startup and compile",
            "warmup_runs_per_independent_group": warmups,
            "timed_samples_per_independent_group": timed_samples,
            "independent_groups": independent_runs,
            "profile_order_per_group": profile_orders,
            "aggregation": "median and nearest-rank p95 across timed samples; median of per-group medians is exposed through group samples",
            "sizes": SIZES,
            "string_key_lengths_bytes": list(STRING_LENGTHS),
            "workload_seed": SEED,
            "correctness": "each sample checks lengths, keys, order, empty-state behavior and checksums in the timed fixture; Rust storage checks the expected sum",
            "capacity": "actual runtime backing capacity queried before and after operations; BitSet capacity reports addressable bit positions; storage probe reports backing bytes",
            "string_key_normalization": "derived from timed operation count and key length: each non-empty string key operation creates an owned key and copies key bytes; allocator metadata and process-wide allocations are not measured",
        },
        "commands": {
            "release_build": ["cargo", "build", "--release", "-p", "spectra-cli", "--offline"],
            "spectra_jit": [str(binary), "run", str(SPECTRA_SOURCE)],
            "spectra_aot_compile": aot_compile["command"],
            "spectra_aot": [str(output_root / "spectra-aot.exe")],
            "rust_storage_build": rust_compile["command"],
            "rust_storage": [str(output_root / "vector-storage.exe")],
        },
        "aot_compile": aot_compile,
        "rust_storage_compile": rust_compile,
        "results": results,
        "comparisons": comparisons,
        "vector_fast_abi_comparison": vector_fast_abi_comparison(comparisons),
        "vector_t_go_no_go": vector,
        "correctness_passed": True,
    }
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8", newline="\n")
    return payload


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True, help="release-profile spectralang executable")
    parser.add_argument("--out", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--warmups", type=int, default=WARMUPS)
    parser.add_argument("--samples", type=int, default=TIMED_SAMPLES)
    parser.add_argument("--independent-runs", type=int, default=INDEPENDENT_RUNS)
    args = parser.parse_args(argv)
    if args.warmups < 2 or args.samples < 5 or args.independent_runs < INDEPENDENT_RUNS:
        parser.error("require at least 2 warmups, 5 timed samples and 3 independent runs")
    out = args.out if args.out.is_absolute() else ROOT / args.out
    try:
        payload = capture(
            binary=args.binary,
            out=out,
            warmups=args.warmups,
            timed_samples=args.samples,
            independent_runs=args.independent_runs,
        )
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as exc:
        print(f"R-3301 collection benchmark failed: {exc}", file=sys.stderr)
        return 1
    print(f"R-3301 collection benchmark captured: {len(payload['results']['jit'])} scenarios per profile")
    print(f"Vector<T> storage decision: {payload['vector_t_go_no_go']['decision']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
