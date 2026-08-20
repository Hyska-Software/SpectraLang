#!/usr/bin/env python3
"""Validate R-2902 range and iterator production semantics."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def cargo_bin() -> str:
    explicit = os.environ.get("CARGO")
    if explicit:
        return explicit
    discovered = shutil.which("cargo")
    if discovered:
        return discovered
    windows_default = Path.home() / ".cargo" / "bin" / "cargo.exe"
    if windows_default.exists():
        return str(windows_default)
    return "cargo"


def run_checked(command: list[str], *, timeout: int = 180) -> str:
    proc = subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=timeout,
    )
    if proc.returncode != 0:
        sys.stdout.write(proc.stdout)
        raise SystemExit(proc.returncode)
    return proc.stdout


def require_text(path: Path, needles: list[str]) -> None:
    text = path.read_text(encoding="utf-8")
    missing = [needle for needle in needles if needle not in text]
    if missing:
        raise SystemExit(f"{path} missing required text: {', '.join(missing)}")


def validate_roadmap() -> None:
    data = tomllib.loads((ROOT / "roadmap" / "roadmap.toml").read_text(encoding="utf-8"))
    phases = {phase["id"] for phase in data["phases"]}
    if "phase_29" not in phases:
        raise SystemExit("roadmap missing phase_29")
    items = {item["id"]: item for item in data["items"]}
    item = items.get("R-2902")
    if item is None:
        raise SystemExit("roadmap missing R-2902")
    expected = {
        "phase": "phase_29",
        "owner": "midend",
        "priority": "P0",
        "status": "complete",
        "risk": "high",
    }
    for key, value in expected.items():
        if item.get(key) != value:
            raise SystemExit(f"R-2902 {key} expected {value}, got {item.get(key)}")
    acceptance = "\n".join(item.get("acceptance", []))
    for needle in [
        "typed Range handle",
        "tests/validation/151_range_production.spectra",
        "spectra.std.range.create",
        "spectra.std.range.at",
        "validate_r2902_range_production.py",
    ]:
        if needle not in acceptance:
            raise SystemExit(f"R-2902 acceptance missing {needle}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default=str(ROOT / "target" / "debug" / "spectralang.exe"))
    args = parser.parse_args()

    binary = Path(args.binary)
    if not binary.exists():
        raise SystemExit(f"spectralang binary not found: {binary}")

    validate_roadmap()
    require_text(
        ROOT / "compiler" / "tests" / "snapshots" / "std_range_public_function_table.snap",
        ["std.range.Range", "std.range.create", "std.range.is_inclusive"],
    )
    # The lowering stage is split across lowering_impl_loops.rs and
    # lowering_std_host_convert_time.rs.  Check the stage sources as a unit so
    # this gate follows the production implementation after decomposition.
    midend_sources = "\n".join(
        path.read_text(encoding="utf-8")
        for path in sorted((ROOT / "midend" / "src").glob("*.rs"))
    )
    missing = [
        needle
        for needle in [
            "spectra.std.range.create",
            "spectra.std.range.len",
            "spectra.std.range.at",
        ]
        if needle not in midend_sources
    ]
    if missing:
        raise SystemExit(
            "midend/src/**/*.rs missing required text: " + ", ".join(missing)
        )
    stdlib_sources = "\n".join(
        path.read_text(encoding="utf-8")
        for path in sorted((ROOT / "runtime" / "src" / "stdlib").glob("*.rs"))
    )
    missing_runtime = [
        needle
        for needle in ["RANGE_CREATE", "std_range_invalid_handles_and_indexes_return_status"]
        if needle not in stdlib_sources
    ]
    if missing_runtime:
        raise SystemExit(
            "runtime/src/stdlib/**/*.rs missing required text: "
            + ", ".join(missing_runtime)
        )
    require_text(
        ROOT / "docs" / "reference" / "03-tipos-compostos.md",
        ["Range", "std.range", "stored range"],
    )
    require_text(
        ROOT / "docs" / "runtime" / "standard-library.md",
        ["spectra.std.range.create", "spectra.std.range.at"],
    )

    run_checked(
        [cargo_bin(), "test", "-p", "spectra-runtime", "std_range", "--", "--test-threads=1"],
        timeout=240,
    )
    run_checked(
        [cargo_bin(), "test", "-p", "spectra-midend", "stored_range", "--", "--test-threads=1"],
        timeout=180,
    )
    run_checked(
        [cargo_bin(), "test", "-p", "spectra-compiler", "std_range"],
        timeout=180,
    )
    run_checked(
        [str(binary), "run", str(ROOT / "tests" / "validation" / "151_range_production.spectra")],
        timeout=120,
    )

    print("validated R-2902 range production semantics")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
