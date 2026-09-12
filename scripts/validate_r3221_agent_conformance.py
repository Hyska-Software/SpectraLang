#!/usr/bin/env python3
"""R-3221 certification gate: the Phase 32 agent platform release gate.

Five checks, in order, stopping at the first failure:

  (a) the crate conformance suite
      (`cargo test -p spectra-agent --test conformance`);
  (b) the full sweep of the phase's item validators
      (`scripts/validate_r3201_*.py` ... `scripts/validate_r3224_*.py`);
  (c) surface determinism: two `spectralang surface --json` runs over
      `tests/projects/valid/integrated_agent_service` are byte-identical;
  (d) every runnable example (`examples/agent/01..04`) in JIT and AOT
      (`compile --debug-info=none --emit-exe`, then execute);
  (e) the integrated-project validator
      (`scripts/validate_r3221_integrated_agent_service.py`).

Every check's status is written to
`target/r3221-agent-conformance/report.json`, which the release gate requires;
a single failing check exits non-zero. The gate is intentionally heavy: it
re-runs every item validator, because it is the release gate, not a smoke test.

Two R-3221 validators are excluded from the sweep for exactly one reason each:
this script itself (recursion) and the integrated-project validator, which check
(e) runs as its dedicated slot. The package validator
(`validate_r3221_agent_package.py`) stays in the sweep, so no item validator is
skipped silently; a validator that is expected and missing is a failure.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Callable


ROOT = Path(__file__).resolve().parents[1]
SCRIPT_DIR = ROOT / "scripts"
WORK = ROOT / "target" / "r3221-agent-conformance"
REPORT_PATH = WORK / "report.json"

SELF = "scripts/validate_r3221_agent_conformance.py"
INTEGRATED_VALIDATOR = "scripts/validate_r3221_integrated_agent_service.py"
SWEEP_EXCLUDED = {SELF, INTEGRATED_VALIDATOR}

INTEGRATED_PROJECT = "tests/projects/valid/integrated_agent_service"

EXAMPLES = {
    "01-tool-and-run": "examples/agent/01-tool-and-run",
    "02-approval-and-budget": "examples/agent/02-approval-and-budget",
    "03-mcp-and-memory": "examples/agent/03-mcp-and-memory",
    "04-durable-replay": "examples/agent/04-durable-replay",
}

CARGO = os.environ.get("CARGO") or shutil.which("cargo") or "cargo"

# The resolved spectralang binary; set by `--binary` or discovered after build.
BINARY: Path | None = None


class CheckFailure(Exception):
    """A check failed; the gate stops and reports it."""


def tail(output: str, lines: int = 25) -> str:
    kept = [line for line in output.splitlines() if line.strip()]
    return "\n".join(kept[-lines:])


def run(args: list[str], timeout: int) -> tuple[int, str]:
    completed = subprocess.run(
        [str(arg) for arg in args],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        timeout=timeout,
    )
    return completed.returncode, completed.stdout


def resolve_binary(override: str | None = None) -> Path:
    """The spectralang binary the gate shells out to."""
    global BINARY
    if override:
        path = Path(override)
        if not path.is_absolute():
            path = ROOT / path
        if not path.is_file():
            raise CheckFailure(f"--binary {override} does not exist")
        BINARY = path
        return path
    if BINARY is not None:
        return BINARY
    for name in ("spectralang.exe", "spectralang"):
        candidate = ROOT / "target" / "debug" / name
        if candidate.is_file():
            BINARY = candidate
            return candidate
    raise CheckFailure("target/debug/spectralang is missing; the CLI must be built first")


def cargo(args: list[str], timeout: int, label: str) -> str:
    """Runs cargo, waiting out a transient build-directory lock."""
    attempts = 3
    output = ""
    for attempt in range(1, attempts + 1):
        code, output = run([CARGO, *args], timeout=timeout)
        if code == 0:
            return output
        if attempt < attempts and "file lock" in output.lower():
            time.sleep(5 * attempt)
            continue
        break
    raise CheckFailure(f"{label} failed (exit {code}):\n{tail(output)}")


# ── (a) the conformance suite ────────────────────────────────────────────


def check_cli_build() -> dict:
    if BINARY is not None:
        return {"binary": str(BINARY), "reused": True}
    output = cargo(["build", "-q", "-p", "spectra-cli", "--offline"], 1200, "spectra-cli build")
    return {"binary": str(resolve_binary()), "output": tail(output, 5)}


def check_conformance_suite() -> dict:
    output = cargo(
        ["test", "-p", "spectra-agent", "--test", "conformance", "--offline"],
        1800,
        "spectra-agent conformance suite",
    )
    passed = output.count("test result: ok")
    return {"summary": tail(output, 12), "ok_lines": passed}


# ── (b) the item-validator sweep ─────────────────────────────────────────


def expected_validators() -> list[tuple[str, Path]]:
    """One (item id, script) per phase-32 item, in item order."""
    found: list[tuple[str, Path]] = []
    for number in range(3201, 3225):
        prefix = f"validate_r{number}_"
        matches = sorted(
            path
            for path in SCRIPT_DIR.glob(f"{prefix}*.py")
            if path.name not in {Path(entry).name for entry in SWEEP_EXCLUDED}
        )
        if not matches:
            raise CheckFailure(
                f"R-{number}: no validator in scripts/ (expected {prefix}*.py); "
                "an item validator may not be skipped silently"
            )
        found.append((f"R-{number}", matches[0]))
    return found


def check_item_validators() -> dict:
    results: list[dict] = []
    for item, script in expected_validators():
        started = time.monotonic()
        code, output = run([sys.executable, str(script.relative_to(ROOT))], timeout=2400)
        entry = {
            "item": item,
            "script": str(script.relative_to(ROOT)),
            "status": "passed" if code == 0 else "failed",
            "seconds": round(time.monotonic() - started, 2),
        }
        if code != 0:
            entry["output"] = tail(output, 30)
            results.append(entry)
            raise CheckFailure(
                f"{item} validator {script.name} failed (exit {code}):\n{tail(output, 30)}"
            )
        results.append(entry)
    return {"validators": results, "count": len(results)}


# ── (c) surface determinism ──────────────────────────────────────────────


def check_surface_determinism() -> dict:
    binary = resolve_binary()
    first_code, first = run([binary, "surface", "--json", INTEGRATED_PROJECT], timeout=300)
    if first_code != 0:
        raise CheckFailure(f"surface --json exited {first_code}:\n{tail(first)}")
    second_code, second = run([binary, "surface", "--json", INTEGRATED_PROJECT], timeout=300)
    if second_code != 0:
        raise CheckFailure(f"the second surface --json run exited {second_code}:\n{tail(second)}")
    if not first.strip():
        raise CheckFailure("surface --json produced no output")
    if first != second:
        raise CheckFailure(
            "two surface --json runs over the same project differ; the surface must be \
deterministic (module/symbol order and key order)"
        )
    report = json.loads(first)
    return {
        "project": INTEGRATED_PROJECT,
        "schema": report.get("schema"),
        "bytes": len(first),
        "modules": len(report.get("modules", [])),
    }


# ── (d) the examples in JIT and AOT ──────────────────────────────────────


def check_examples() -> dict:
    binary = resolve_binary()
    evidence: dict[str, dict] = {}
    example_work = WORK / "examples"
    example_work.mkdir(parents=True, exist_ok=True)

    for name, project in EXAMPLES.items():
        source = ROOT / project / "src" / "main.spectra"
        if not source.is_file():
            raise CheckFailure(f"example {name} has no {source.relative_to(ROOT)}")

        code, output = run([binary, "run", str(source.relative_to(ROOT))], timeout=600)
        if code != 0:
            raise CheckFailure(f"example {name} (JIT) exited {code}:\n{tail(output)}")
        jit_tail = tail(output, 5)

        executable = example_work / (name + (".exe" if sys.platform.startswith("win") else ""))
        if executable.exists():
            executable.unlink()
        code, output = run(
            [
                binary,
                "compile",
                "--debug-info=none",
                "--emit-exe",
                str(executable),
                project,
            ],
            timeout=900,
        )
        if code != 0 or not executable.is_file():
            raise CheckFailure(f"example {name} (AOT compile) exited {code}:\n{tail(output)}")

        code, output = run([executable], timeout=600)
        if code != 0:
            raise CheckFailure(f"example {name} (AOT) exited {code}:\n{tail(output)}")
        evidence[name] = {"jit": jit_tail, "aot": tail(output, 5)}
    return evidence


# ── (e) the integrated project ───────────────────────────────────────────


def check_integrated_project() -> dict:
    script = ROOT / INTEGRATED_VALIDATOR
    if not script.is_file():
        raise CheckFailure(f"{INTEGRATED_VALIDATOR} is missing")
    code, output = run([sys.executable, INTEGRATED_VALIDATOR], timeout=2400)
    if code != 0:
        raise CheckFailure(f"{INTEGRATED_VALIDATOR} failed (exit {code}):\n{tail(output, 30)}")
    return {"script": INTEGRATED_VALIDATOR, "summary": tail(output, 5)}


# ── the gate ─────────────────────────────────────────────────────────────


def checks() -> list[tuple[str, Callable[[], dict]]]:
    # The CLI build is the prelude for every check that shells out; a
    # --binary override replaces the build.
    return [
        ("cli_build", check_cli_build),
        ("conformance_suite", check_conformance_suite),
        ("item_validators", check_item_validators),
        ("surface_determinism", check_surface_determinism),
        ("examples_jit_and_aot", check_examples),
        ("integrated_project", check_integrated_project),
    ]


def main() -> None:
    parser = argparse.ArgumentParser(description="R-3221 agent platform certification gate")
    parser.add_argument(
        "--binary",
        default=None,
        help="spectralang binary to use (default: target/debug/spectralang[.exe], built if absent)",
    )
    arguments = parser.parse_args()

    WORK.mkdir(parents=True, exist_ok=True)
    if arguments.binary:
        try:
            resolve_binary(arguments.binary)
        except CheckFailure as error:
            parser.error(str(error))

    records: list[dict] = []
    failure: str | None = None
    for name, runner in checks():
        if failure is not None:
            records.append(
                {
                    "name": name,
                    "status": "not_run",
                    "detail": f"stopped: check '{failure}' failed",
                    "seconds": 0.0,
                }
            )
            continue
        started = time.monotonic()
        try:
            evidence = runner()
        except CheckFailure as error:
            records.append(
                {
                    "name": name,
                    "status": "failed",
                    "detail": str(error),
                    "seconds": round(time.monotonic() - started, 2),
                }
            )
            failure = name
            continue
        records.append(
            {
                "name": name,
                "status": "passed",
                "detail": "",
                "seconds": round(time.monotonic() - started, 2),
                "evidence": evidence,
            }
        )

    report = {
        "schema": "spectralang.r3221.conformance_report.v1",
        "item": "R-3221",
        "status": "failed" if failure else "passed",
        "failed_check": failure,
        "binary": str(BINARY) if BINARY else str(ROOT / "target" / "debug" / "spectralang"),
        "checks": records,
    }
    REPORT_PATH.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    if failure:
        detail = next(record["detail"] for record in records if record["name"] == failure)
        print(
            f"R-3221 certification gate failed at '{failure}':\n{detail}\nreport: {REPORT_PATH}",
            file=sys.stderr,
        )
        sys.exit(1)

    print(f"R-3221 certification gate passed ({len(records)} checks); report: {REPORT_PATH}")


if __name__ == "__main__":
    main()
