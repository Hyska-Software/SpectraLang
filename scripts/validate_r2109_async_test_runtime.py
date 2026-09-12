#!/usr/bin/env python3
"""Validation gate for R-2109 async test runtime and test macro support."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str], *, expect_success: bool = True) -> subprocess.CompletedProcess[str]:
    print(f"[R-2109] {' '.join(command)}")
    completed = subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if expect_success and completed.returncode != 0:
        print(completed.stdout)
        raise SystemExit(completed.returncode)
    if not expect_success and completed.returncode == 0:
        print(completed.stdout)
        print("[R-2109] command unexpectedly succeeded", file=sys.stderr)
        raise SystemExit(1)
    return completed


def require_contains(path: Path, needles: list[str]) -> None:
    text = path.read_text(encoding="utf-8")
    missing = [needle for needle in needles if needle not in text]
    if missing:
        stage_text = "\n".join(
            source.read_text(encoding="utf-8")
            for source in sorted(path.parent.rglob("*.rs"))
        )
        missing = [needle for needle in missing if needle not in stage_text]
    if missing:
        for needle in missing:
            print(f"[R-2109] missing marker in {path}: {needle}", file=sys.stderr)
        raise SystemExit(1)


def parse_first_json_object(output: str) -> dict:
    start = -1
    offset = 0
    for line in output.splitlines(keepends=True):
        if line.lstrip().startswith("{"):
            start = offset + line.index("{")
            break
        offset += len(line)
    if start < 0:
        print(output)
        print("[R-2109] no JSON payload found", file=sys.stderr)
        raise SystemExit(1)
    decoder = json.JSONDecoder()
    try:
        payload, _ = decoder.raw_decode(output[start:])
    except json.JSONDecodeError as error:
        print(output)
        print(f"[R-2109] invalid JSON payload: {error}", file=sys.stderr)
        raise SystemExit(1)
    return payload


def main() -> int:
    require_contains(
        ROOT / "compiler" / "src" / "ast" / "mod.rs",
        ["pub struct Attribute", "pub attributes: Vec<Attribute>"],
    )
    require_contains(
        ROOT / "compiler" / "src" / "parser" / "item.rs",
        ["parse_outer_attributes", "Vec<Attribute>"],
    )
    require_contains(
        ROOT / "compiler" / "src" / "semantic" / "mod.rs",
        ["block_on", "Type::Task { output }"],
    )
    require_contains(
        ROOT / "midend" / "src" / "lowering.rs",
        ["block_on", "spectra.async.task.result"],
    )
    require_contains(
        ROOT / "tools" / "spectra-cli" / "src" / "main.rs",
        [
            "discover_async_test_cases",
            "run_async_test_case",
            "PackageTestReport",
            "spectra_async_test",
        ],
    )

    run(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "spectra-cli",
            "--",
            "compile",
            "tests/validation/130_async_test_runtime_block_on.spectra",
        ]
    )

    listing = run(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "spectra-cli",
            "--",
            "package",
            "test",
            "--root",
            "tests/projects/valid/async_test_runtime",
            "--list",
            "--json",
        ]
    ).stdout
    payload = parse_first_json_object(listing)
    names = {test["name"] for test in payload.get("tests", [])}
    expected_names = {
        "async_math_tests::async_addition_passes",
        "async_math_tests::block_on_is_available",
    }
    if names != expected_names or not payload.get("listed") or not payload.get("success"):
        print(listing)
        print("[R-2109] async test listing payload did not match expected tests", file=sys.stderr)
        raise SystemExit(1)

    filtered = run(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "spectra-cli",
            "--",
            "package",
            "test",
            "--root",
            "tests/projects/valid/async_test_runtime",
            "--filter",
            "block_on",
        ]
    ).stdout
    if "async_math_tests::block_on_is_available ... ok" not in filtered:
        print(filtered)
        print("[R-2109] filtered async test did not run", file=sys.stderr)
        raise SystemExit(1)
    if "async_math_tests::async_addition_passes ... ok" in filtered:
        print(filtered)
        print("[R-2109] filter executed an unrelated async test", file=sys.stderr)
        raise SystemExit(1)

    run(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "spectra-cli",
            "--",
            "package",
            "test",
            "--root",
            "tests/projects/valid/async_test_runtime",
        ]
    )

    print("validated R-2109 async test runtime, block_on, list/filter/reporting")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
