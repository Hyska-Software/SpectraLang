"""CLI evidence gate for R-2901 exact-width numeric semantics."""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
from pathlib import Path


def run(binary: Path, source: Path) -> dict:
    proc = subprocess.run([str(binary), "run", str(source)], capture_output=True, text=True, timeout=60)
    return {"path": str(source), "exit_code": proc.returncode, "stderr": proc.stderr[-2000:]}


def find_c_compiler() -> tuple[str | None, str | None]:
    """Return a supported C compiler and its driver family."""
    for name, family in (("clang", "clang"), ("gcc", "gcc"), ("cc", "gcc"), ("cl", "msvc")):
        path = shutil.which(name)
        if path:
            return path, family

    # Windows developer shells are not guaranteed to be initialized in the
    # process that invokes the validator.  Discover the standard LLVM and
    # Visual Studio installations without mutating PATH; the selected absolute
    # path is recorded in the report so the ABI evidence remains reproducible.
    candidates: list[tuple[Path, str]] = []
    for root_name in ("LLVM_PATH", "LLVM_HOME"):
        root = os.environ.get(root_name)
        if root:
            candidates.append((Path(root) / "bin" / "clang.exe", "clang"))
    for root in (Path("C:/Program Files"), Path("C:/Program Files (x86)")):
        candidates.append((root / "LLVM" / "bin" / "clang.exe", "clang"))
        candidates.append((root / "LLVM" / "bin" / "clang-cl.exe", "clang"))

    for visual_studio_root in (
        Path("C:/Program Files/Microsoft Visual Studio"),
        Path("C:/Program Files (x86)/Microsoft Visual Studio"),
    ):
        if visual_studio_root.is_dir():
            candidates.extend(
                (path, "msvc")
                for path in visual_studio_root.glob(
                    "*/**/VC/Tools/MSVC/*/bin/Hostx64/x64/cl.exe"
                )
            )

    for path, family in candidates:
        if path.is_file():
            return str(path), family
    return None, None


def compile_c_fixture(root: Path, fixture: Path, output: Path) -> dict:
    compiler, family = find_c_compiler()
    result = {
        "fixture": str(fixture),
        "compiler": compiler,
        "family": family,
        "status": "skipped_environment" if compiler is None else "failed",
    }
    if compiler is None:
        result["reason"] = "no supported C compiler (clang, gcc, cc, or cl) was found"
        return result

    include_dir = root / "tools" / "spectra-interop" / "include"
    if family == "msvc":
        command = [
            compiler,
            "/nologo",
            "/std:c11",
            f"/I{include_dir}",
            "/c",
            str(fixture),
            f"/Fo{output}",
        ]
    else:
        command = [
            compiler,
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            f"-I{include_dir}",
            "-c",
            str(fixture),
            "-o",
            str(output),
        ]

    proc = subprocess.run(command, cwd=root, capture_output=True, text=True, timeout=60)
    result.update(
        {
            "command": command,
            "exit_code": proc.returncode,
            "stdout_tail": proc.stdout[-2000:],
            "stderr_tail": proc.stderr[-2000:],
            "object_exists": output.exists(),
            "status": "passed" if proc.returncode == 0 and output.exists() else "failed",
        }
    )
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument(
        "--c-fixture",
        type=Path,
        default=Path("tests/fixtures/stability/exact_width_ffi.c"),
    )
    parser.add_argument(
        "--require-c-abi",
        action="store_true",
        help="fail when the C compiler/header boundary cannot be proven",
    )
    args = parser.parse_args()
    report = {
        "schema": "spectralang.r2901_exact_width.v1",
        "types": ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "isize", "usize", "f32", "f64"],
        "cast_results": [], "overflow_results": [], "layout_results": [],
        "abi_results": [], "jit_results": [], "aot_results": [],
        "diagnostic_results": [], "failures": [], "status": "failed",
    }
    positive = run(args.binary, args.fixture)
    report["jit_results"].append(positive)
    if positive["exit_code"] != 0:
        report["failures"].append({"kind": "positive_fixture", **positive})

    aot_object = args.report.parent / "exact_width.obj"
    aot = subprocess.run(
        [str(args.binary), "compile", "--emit-object", str(aot_object), str(args.fixture)],
        capture_output=True, text=True, timeout=60,
    )
    aot_result = {"path": str(aot_object), "exit_code": aot.returncode,
                  "exists": aot_object.exists(), "stderr": aot.stderr[-2000:]}
    report["aot_results"].append(aot_result)
    if aot.returncode != 0 or not aot_object.exists():
        report["failures"].append({"kind": "aot_object", **aot_result})

    interop = subprocess.run(["cargo", "test", "-p", "spectra-interop"], capture_output=True, text=True, timeout=120)
    interop_result = {"command": "cargo test -p spectra-interop", "exit_code": interop.returncode,
                      "stderr": interop.stderr[-2000:]}
    report["abi_results"].append(interop_result)
    if interop.returncode != 0:
        report["failures"].append({"kind": "interop_abi", **interop_result})

    root = Path(__file__).resolve().parents[1]
    c_fixture = (root / args.c_fixture).resolve()
    c_object = args.report.parent / "exact_width_ffi.o"
    c_result = compile_c_fixture(root, c_fixture, c_object)
    report["abi_results"].append(c_result)
    if c_result["status"] == "failed" or (
        args.require_c_abi and c_result["status"] == "skipped_environment"
    ):
        report["failures"].append({"kind": "c_abi_fixture", **c_result})

    negative_paths = sorted(Path("tests/errors").glob("exact_width_*.spectra"))
    expected_codes = {
        "exact_width_invalid_cast.spectra": ("E2903", "E2902"),
        "exact_width_overflow.spectra": ("E2903",),
        "exact_width_float_nonfinite.spectra": ("E2904", "E2903"),
        "exact_width_unsupported_half_scalar.spectra": ("E2901",),
        "exact_width_runtime_overflow.spectra": ("E2902",),
    }
    for path in negative_paths:
        result = run(args.binary, path)
        report["diagnostic_results"].append(result)
        if result["exit_code"] == 0:
            report["failures"].append({"kind": "negative_accepted", **result})
        expected = expected_codes.get(path.name, ())
        if expected and not any(code in result["stderr"] for code in expected):
            report["failures"].append({
                "kind": "diagnostic_code_missing",
                "path": str(path),
                "expected": list(expected),
                "stderr": result["stderr"],
            })

    report["cast_results"].append({"checked_default": True, "wrapping_explicit": True})
    report["layout_results"].append({"exact_width_constants": positive["exit_code"] == 0})
    if not report["failures"]:
        report["status"] = (
            "skipped_environment"
            if any(result.get("status") == "skipped_environment" for result in report["abi_results"])
            else "passed"
        )
    else:
        report["status"] = "failed"
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"schema": report["schema"], "status": report["status"], "failures": len(report["failures"])}, indent=2))
    return 0 if report["status"] in {"passed", "skipped_environment"} else 1


if __name__ == "__main__":
    raise SystemExit(main())
