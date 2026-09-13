# R-3204 — diagnostics with repair information and `explain --json`.
#
# Validates the error model (expected/actual/fix), the JSON/SARIF emission,
# the five populated codes against real CLI output, the `explain` command and
# the documentation set-equality guard.
from __future__ import annotations

import os
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPECTRALANG = Path(
    os.environ.get("SPECTRALANG_BINARY") or (ROOT / "target" / "debug" / "spectralang.exe")
)
CARGO = shutil.which("cargo") or "cargo"
WORKDIR = ROOT / "target" / "r3204-diagnostics"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3204 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def run_command(args: list[str]) -> str:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    if completed.returncode != 0:
        fail(f"command {' '.join(args)} failed:\n{completed.stdout}")
    return completed.stdout


CASES = {
    "e004": (
        'module ret_mismatch\n\nfunc f() returns int {\n    return "s"\n}\n\n'
        "public func main() returns int {\n    return f()\n}\n",
        "E004",
    ),
    "e003": (
        'module assign_mismatch\n\npublic func main() returns int {\n    let x: int = "s"\n    return x\n}\n',
        "E003",
    ),
    "e033": (
        "module unknown_std\n\nimport std.nonexistent_module\n\npublic func main() returns int {\n    return 0\n}\n",
        "E033",
    ),
    "e029": (
        "module missing_user\n\nimport missing_module_xyz\n\npublic func main() returns int {\n    return 0\n}\n",
        "E029",
    ),
    "e028": (
        "module self_import_check\n\nimport self_import_check\n\npublic func main() returns int {\n    return 0\n}\n",
        "E028",
    ),
}


def check_json(path: Path) -> dict:
    completed = subprocess.run(
        [str(SPECTRALANG), "check", "--json", str(path)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    require(
        completed.returncode == 65,
        f"check --json on {path.name} must exit 65, got {completed.returncode}:\n{completed.stdout}",
    )
    return json.loads(completed.stdout.splitlines()[0])


def validate_implementation() -> None:
    error_rs = read("compiler/src/error.rs")
    for term in ["with_expected", "with_actual", "with_fix", "pub expected", "pub actual", "pub fix"]:
        require(term in error_rs, f"compiler error model missing {term}")

    diagnostics = read("tools/spectra-cli/src/cli_diagnostics.rs")
    for term in [
        "expected: Option<String>",
        "actual: Option<String>",
        "fix: Option<String>",
        "project_error_diagnostic",
        '"E029"',
        '"E028"',
    ]:
        require(term in diagnostics, f"cli_diagnostics missing {term}")

    explain = read("tools/spectra-cli/src/cli_explain.rs")
    for term in ["fn execute_explain", "spectralang.explain.v1", "ERROR_CODES_TEXT"]:
        require(term in explain, f"cli_explain missing {term}")

    lib = read("tools/spectra-cli/src/lib.rs")
    require('include!("cli_explain.rs");' in lib, "lib.rs must include cli_explain.rs")


def validate_populated_codes() -> None:
    WORKDIR.mkdir(parents=True, exist_ok=True)
    for name, (source, code) in CASES.items():
        path = WORKDIR / f"{name}.spectra"
        path.write_text(source, encoding="utf-8")
        report = check_json(path)
        diagnostics = report["files"][0]["diagnostics"]
        match = next((entry for entry in diagnostics if entry.get("code") == code), None)
        require(match is not None, f"{name}: expected diagnostic {code}, got {diagnostics}")
        require(
            match.get("fix"),
            f"{name}: {code} must carry a fix field: {match}",
        )
        if code in {"E003", "E004"}:
            require(match.get("expected"), f"{name}: {code} must carry expected")
            require(match.get("actual"), f"{name}: {code} must carry actual")


def validate_explain() -> None:
    completed = subprocess.run(
        [str(SPECTRALANG), "explain", "--json", "E004"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    require(completed.returncode == 0, f"explain --json E004 exited {completed.returncode}")
    report = json.loads(completed.stdout.splitlines()[0])
    require(report.get("schema") == "spectralang.explain.v1", "explain schema mismatch")
    require(report.get("code") == "E004", "explain must echo the code")
    require(report.get("description"), "explain must include a description")
    require(report.get("reference_sha256"), "explain must report the reference hash")

    completed = subprocess.run(
        [str(SPECTRALANG), "explain", "--json", "E9999"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    require(completed.returncode == 64, f"unknown code must exit 64, got {completed.returncode}")
    failure = json.loads(completed.stdout.splitlines()[0])
    require(failure.get("success") is False, "unknown code must set success=false")

    completed = subprocess.run(
        [str(SPECTRALANG), "explain", "--list", "--json"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    require(completed.returncode == 0, "explain --list must succeed")
    listing = json.loads(completed.stdout.splitlines()[0])
    codes = listing.get("codes", [])
    for code in ["E004", "E003", "E033", "E029", "E028"]:
        require(code in codes, f"explain --list must include {code}")


def validate_documentation_guard() -> None:
    reference = read("docs/diagnostics/error-code-reference.md")
    emitted: set[str] = set()
    for path in (ROOT / "compiler" / "src").rglob("*.rs"):
        text = path.read_text(encoding="utf-8", errors="ignore")
        for match in re.finditer(
            r'\.with_code\("(E\d+)"\)(?:(?!\n\s*\n).){0,600}?\.with_fix\(',
            text,
            flags=re.DOTALL,
        ):
            emitted.add(match.group(1))
    require(emitted, "no codes emit repair fields; the guard would be vacuous")
    for code in sorted(emitted):
        require(
            f"`{code}`" in reference,
            f"{code} carries repair information but is not documented in the error-code reference",
        )


def validate_planning() -> None:
    backlog = read("docs/roadmap-backlog.md")
    block = backlog.split("## R-3204 Diagnostics with Repair Information", 1)[1].split(
        "## R-3205", 1
    )[0]
    for term in ["Status: `complete`", "docs/agent-platform-plan.md", "validate_r3204_diagnostics_repair.py"]:
        require(term in block, f"backlog R-3204 missing {term}")

    runner = read("run_tests.ps1")
    require(
        "validate_r3204_diagnostics_repair.py" in runner,
        "run_tests.ps1 must run R-3204",
    )


def main() -> None:
    if not os.environ.get("SPECTRA_CLI_BUILT"):
        run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    validate_implementation()
    validate_populated_codes()
    validate_explain()
    validate_documentation_guard()
    validate_planning()
    print("validated R-3204 diagnostics repair information and explain")


if __name__ == "__main__":
    main()
