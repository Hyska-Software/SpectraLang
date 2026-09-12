#!/usr/bin/env python3
"""Validate Phase 10 debugger and stack trace artifacts."""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
from pathlib import Path


def run(command: list[str], timeout: int = 20) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
    )


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def normalized_path(value: str) -> Path:
    if value.startswith("\\\\?\\"):
        value = value[4:]
    return Path(value).resolve()


def validate_runtime_trace(binary: Path, repo_root: Path) -> None:
    fixture = repo_root / "tests" / "cli" / "runtime_nonzero.spectra"
    result = run([str(binary), "run", str(fixture)], timeout=10)
    output = result.stdout + result.stderr

    require(result.returncode == 7, f"expected exit code 7, got {result.returncode}\n{output}")
    require("error[runtime]" in output, "runtime diagnostic code missing")
    require("program exited with status 7" in output, "runtime exit status missing")
    require("= stack:" in output, "runtime stack header missing")
    require("0: main()" in output, "main stack frame missing")
    require(str(fixture) in output or str(fixture.resolve()) in output, "source path missing")


def validate_aot_debug_map(binary: Path, repo_root: Path) -> None:
    fixture = repo_root / "tests" / "cli" / "runtime_nonzero.spectra"
    out_dir = repo_root / "target" / "r1002-debug"
    if out_dir.exists():
        shutil.rmtree(out_dir)
    out_dir.mkdir(parents=True)

    object_path = out_dir / "runtime_nonzero.obj"
    result = run([str(binary), "compile", "--emit-object", str(object_path), str(fixture)], timeout=20)
    output = result.stdout + result.stderr

    require(result.returncode == 0, f"AOT object compilation failed\n{output}")
    require(object_path.exists(), "AOT object was not written")
    require(object_path.stat().st_size > 0, "AOT object is empty")
    require("Written debug map" in output, "CLI did not report debug map emission")

    debug_map_path = Path(str(object_path) + ".spectra-debug.json")
    require(debug_map_path.exists(), "AOT debug map was not written")

    debug_map = json.loads(debug_map_path.read_text(encoding="utf-8"))
    require(debug_map["schema"] == "spectra-aot-debug-map", "unexpected debug map schema")
    require(debug_map["schema_version"] == 1, "unexpected debug map schema version")
    require(debug_map["artifact"]["kind"] == "object", "unexpected artifact kind")
    require(normalized_path(debug_map["artifact"]["path"]) == object_path.resolve(), "artifact path mismatch")
    require(normalized_path(debug_map["source"]["path"]) == fixture.resolve(), "source path mismatch")
    require(debug_map["entrypoint"]["function"] == "main", "entrypoint function missing")
    require(debug_map["entrypoint"]["exported_symbol"] == "main", "object symbol strategy missing")
    require(debug_map["entrypoint"]["source_line"] == 5, "entrypoint line mismatch")
    require(debug_map["entrypoint"]["source_column"] > 0, "entrypoint column missing")
    require("gdb" in debug_map["native_debuggers"], "gdb strategy missing")
    require("lldb" in debug_map["native_debuggers"], "lldb strategy missing")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/spectralang.exe"))
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[1]
    binary = (repo_root / args.binary).resolve() if not args.binary.is_absolute() else args.binary

    validate_runtime_trace(binary, repo_root)
    validate_aot_debug_map(binary, repo_root)
    print("debugger_stack_traces validation passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
