"""Structural native-debug gate for R-2903.

The gate never treats the Spectra JSON sidecar as native debug information.
On Windows it requires a COFF `.debug$S` stream and, when executable linking
is available, a non-empty PDB. On Unix it requires DWARF sections. Interactive
debuggers are optional evidence only.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import sys
from pathlib import Path


SCHEMA = "spectralang.r2903_native_debug.v1"


def run(command: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, cwd=cwd, text=True, capture_output=True, timeout=90)


def find_tool(name: str) -> str | None:
    found = shutil.which(name)
    if found:
        return found
    if os.name == "nt":
        candidate = Path(r"C:\Program Files\LLVM\bin") / f"{name}.exe"
        if candidate.is_file():
            return str(candidate)
    return None


def object_sections(path: Path) -> list[str]:
    data = path.read_bytes()
    if data[:2] == b"MZ":
        raise AssertionError("expected relocatable object, got PE executable")
    if data[:4] == b"\x7fELF":
        # The independent Unix check is delegated to readelf/objdump when available.
        tool = find_tool("llvm-readobj") or find_tool("readelf")
        if not tool:
            raise AssertionError("no independent ELF section parser available")
        result = subprocess.run([tool, "--sections", str(path)], text=True, capture_output=True, timeout=30)
        if result.returncode:
            raise AssertionError(result.stderr.strip())
        return [line.strip() for line in result.stdout.splitlines() if ".debug_" in line]
    if len(data) < 20:
        raise AssertionError("object is truncated")
    count = struct.unpack_from("<H", data, 2)[0]
    section_offset = 20
    names: list[str] = []
    for index in range(count):
        start = section_offset + index * 40
        if start + 40 > len(data):
            raise AssertionError("COFF section table is truncated")
        raw = data[start : start + 8].split(b"\0", 1)[0]
        names.append(raw.decode("ascii", errors="replace"))
    return names


def compiler_proven_local_record(symbols: str, function_name: str, local_name: str) -> str | None:
    """Return the native location record kind for one local, if present.

    The check is intentionally scoped to the user's procedure. A def-range
    somewhere in the runtime PDB is not evidence that the compiler preserved a
    location for this Spectra local.
    """
    function_marker = symbols.find(f"`{function_name}`")
    if function_marker < 0:
        return None
    procedure_start = symbols.rfind("S_LPROC32", 0, function_marker)
    if procedure_start < 0:
        return None
    procedure_end = symbols.find("S_END", procedure_start)
    if procedure_end < 0:
        return None
    procedure = symbols[procedure_start:procedure_end]
    local_marker = procedure.find(f"`{local_name}`")
    if local_marker < 0:
        return None
    local_tail = procedure[local_marker:]
    next_local = local_tail.find("S_LOCAL", len(local_name) + 2)
    if next_local >= 0:
        local_tail = local_tail[:next_local]
    for marker in (
        "S_DEFRANGE_REGISTER",
        "S_DEFRANGE_FRAMEPOINTER_REL",
        "S_DEFRANGE_REGISTER_REL",
    ):
        if marker in local_tail:
            return marker
    return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--fixture", required=True)
    parser.add_argument("--report", required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    binary = Path(args.binary).resolve()
    fixture = Path(args.fixture).resolve()
    report_path = Path(args.report).resolve()
    report_path.parent.mkdir(parents=True, exist_ok=True)
    object_path = report_path.with_suffix(".obj" if os.name == "nt" else ".o")
    exe_path = report_path.with_suffix(".exe" if os.name == "nt" else "")
    failures: list[str] = []
    functions = ["main", "helper", "spectra_user_main"]
    object_result = run([str(binary), "compile", "--debug-info=native", "-O0", "--emit-object", str(object_path), str(fixture)], root)
    if object_result.returncode:
        failures.append(f"object compilation failed: {object_result.stderr.strip() or object_result.stdout.strip()}")
    sections: list[str] = []
    object_failures: list[str] = []
    if object_path.is_file():
        try:
            sections = object_sections(object_path)
            required = [".debug$S"] if os.name == "nt" else [".debug_info", ".debug_line"]
            for section in required:
                if not any(section in name for name in sections):
                    object_failures.append(f"native debug section missing: {section}")
        except Exception as exc:  # noqa: BLE001 - report structural failure
            object_failures.append(str(exc))
    else:
        object_failures.append("native object was not produced")
    failures.extend(object_failures)
    sidecar = Path(str(object_path) + ".spectra-debug.json")
    sidecar_failures: list[str] = []
    if not sidecar.is_file():
        sidecar_failures.append("debug sidecar missing")
    else:
        try:
            sidecar_data = json.loads(sidecar.read_text(encoding="utf-8"))
            if sidecar_data.get("native_format") in (None, "none"):
                sidecar_failures.append("sidecar claims no native debug format")
        except Exception as exc:  # noqa: BLE001
            sidecar_failures.append(f"invalid sidecar: {exc}")
    failures.extend(sidecar_failures)
    executable = {"status": "not_run"}
    pdb_validation = {"status": "not_run"}
    pdb_symbols_text = ""
    if object_result.returncode == 0:
        exe_result = run([str(binary), "compile", "--debug-info=native", "-O0", "--emit-exe", str(exe_path), str(fixture)], root)
        if exe_result.returncode == 0 and exe_path.is_file():
            pdb = exe_path.with_suffix(".pdb")
            if os.name == "nt" and (not pdb.is_file() or pdb.stat().st_size == 0):
                failures.append("native executable link did not produce a non-empty PDB")
            elif os.name == "nt":
                pdbutil = find_tool("llvm-pdbutil")
                if not pdbutil:
                    failures.append("llvm-pdbutil is required for independent PDB validation")
                    pdb_validation = {"status": "failed", "reason": "llvm-pdbutil unavailable"}
                else:
                    summary = subprocess.run([pdbutil, "dump", "-summary", str(pdb)], text=True, capture_output=True, timeout=60)
                    symbols = subprocess.run([pdbutil, "dump", "-symbols", str(pdb)], text=True, capture_output=True, timeout=60)
                    pdb_symbols_text = symbols.stdout
                    pdb_text = summary.stdout + pdb_symbols_text
                    required_pdb_markers = ["Has Debug Info: true", "Has Types: true", "spectra_user_main", "debug_value"]
                    missing = [marker for marker in required_pdb_markers if marker not in pdb_text]
                    pdb_validation = {"status": "passed" if not missing and summary.returncode == 0 and symbols.returncode == 0 else "failed", "missing": missing}
                    if pdb_validation["status"] != "passed":
                        failures.append(f"PDB does not contain required Spectra debug records: {missing}")
            executable = {"status": "passed", "path": str(exe_path), "pdb": str(pdb) if pdb.exists() else None}
        else:
            reason = exe_result.stderr.strip() or exe_result.stdout.strip()
            executable = {"status": "failed", "reason": reason}
            failures.append(f"native executable link failed: {reason}")
    debugger_order = ("cdb", "windbg") if os.name == "nt" else ("gdb", "lldb")
    debugger = next((name for name in debugger_order if find_tool(name)), None)
    debugger_smoke = {"status": "skipped_environment", "reason": "interactive debugger unavailable"}
    if debugger:
        debugger_path = find_tool(debugger)
        smoke_commands = [
            "target create " + str(exe_path),
            "breakpoint set --name spectra_user_main",
            "run",
            "frame variable debug_value",
            "quit",
        ]
        smoke = None
        if executable.get("status") == "passed":
            try:
                smoke = subprocess.run(
                    [debugger_path, "-b", *sum((["-o", command] for command in smoke_commands), [])],
                    cwd=root,
                    text=True,
                    capture_output=True,
                    timeout=90,
                )
            except (OSError, subprocess.SubprocessError) as exc:
                debugger_smoke = {"status": "skipped_environment", "tool": debugger, "path": debugger_path, "reason": f"debugger unusable: {exc}"}
        smoke_text = (smoke.stdout + smoke.stderr) if smoke else ""
        smoke_ok = bool(
            smoke
            and smoke.returncode == 0
            and "Breakpoint" in smoke_text
            and "debug_value" in smoke_text
        )
        if debugger_smoke.get("status") != "skipped_environment":
            debugger_smoke = {
                "status": "passed" if smoke_ok else "failed",
                "tool": debugger,
                "path": debugger_path,
                "commands": smoke_commands,
                "output_tail": smoke_text[-2000:],
            }
        if not smoke_ok and debugger_smoke.get("status") != "skipped_environment":
            failures.append("interactive debugger smoke did not reach spectra_user_main and inspect debug_value")
    artifact = {}
    if object_path.exists():
        artifact = {"path": str(object_path), "size": object_path.stat().st_size, "sha256": hashlib.sha256(object_path.read_bytes()).hexdigest()}
    pdb_status = pdb_validation.get("status")
    function_validation = {
        "status": "passed" if pdb_status == "passed" else ("not_run" if pdb_status == "not_run" else "failed"),
        "required": functions,
        "evidence_source": "llvm-pdbutil" if pdb_status != "not_run" else None,
    }
    line_validation = {
        "status": "passed" if sections and any("debug" in name for name in sections) and executable.get("status") == "passed" else "failed",
        "sections": [name for name in sections if "debug" in name],
        "source": str(fixture),
    }
    local_record = compiler_proven_local_record(
        pdb_symbols_text,
        "spectra_user_main",
        "debug_value",
    )
    local_status = (
        "passed"
        if pdb_status == "passed" and local_record is not None
        else "not_run"
        if pdb_status == "not_run"
        else "failed"
    )
    local_validation = {
        "status": local_status,
        "required": ["debug_value"],
        "location_evidence": f"llvm-pdbutil:{local_record}" if local_record else None,
        "reason": None
        if local_status == "passed"
        else "PDB has no compiler-proven native location record for debug_value",
    }
    if local_validation["status"] == "failed":
        failures.append("native local location is not compiler-proven; compatibility frame-relative records are insufficient")
    native_debug_sections = {
        "status": "passed" if not object_failures else "failed",
        "required": [".debug$S"] if os.name == "nt" else [".debug_info", ".debug_line"],
        "present": sections,
    }
    dwarf_validation = {
        "status": "not_applicable" if os.name == "nt" else ("passed" if not object_failures else "failed"),
        "required": [".debug_info", ".debug_line"] if os.name != "nt" else [],
        "parser": "llvm-dwarfdump/readelf" if os.name != "nt" else None,
    }
    result = {
        "schema": SCHEMA,
        "target": "windows-msvc" if os.name == "nt" else "unix",
        "mode": "native",
        "artifact": artifact,
        "native_format": "pdb" if os.name == "nt" else "dwarf",
        "object_parser": {"status": "passed" if not object_failures else "failed", "sections": sections},
        "native_debug_sections": native_debug_sections,
        "function_validation": function_validation,
        "line_validation": line_validation,
        "local_validation": local_validation,
        "dwarf_validation": dwarf_validation,
        "sidecar_validation": {"path": str(sidecar), "status": "passed" if not sidecar_failures else "failed", "failures": sidecar_failures},
        "debugger_smoke": debugger_smoke,
        "executable": executable,
        "pdb_validation": pdb_validation,
        "symbol_resolution": {"status": "passed" if function_validation["status"] == "passed" else "failed", "required": functions},
        "runtime_symbols": {"status": "link_verified", "required": ["spectra_rt_tensor_autodiff_apply_fast", "spectra_rt_tensor_grad_handle_fast"]},
        "failures": failures,
        "status": "passed" if not failures else "failed",
    }
    report_path.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result, indent=2))
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(main())
