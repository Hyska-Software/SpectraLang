#!/usr/bin/env python3
"""R-3221 certification gate: the Phase 32 agent platform release gate.

The checks run in order, stopping at the first failure:

  (a) the prelude: `spectra-cli` and the compiler's contract dump are built
      once and exported to every validator (`SPECTRALANG_BINARY`,
      `SPECTRA_CLI_BUILT`, `SPECTRA_CONTRACT_DUMP`), so no validator rebuilds
      the CLI and no catalog probe re-enters cargo;
  (b) the crate conformance suite
      (`cargo test -p spectra-agent --test conformance`);
  (c) the full sweep of the phase's item validators: every
      `scripts/validate_r32XX_*.py` of the phase, so an item with several
      validators (R-3221 carries the package gate and the host-adapter gate)
      runs all of them;
  (d) surface determinism: two `spectralang surface --json` runs over
      `tests/projects/valid/integrated_agent_service` are byte-identical;
  (e) every runnable example (`examples/agent/01..15`) in JIT and AOT
      (`compile --debug-info=none --emit-exe`, then execute);
  (f) the verification fixtures (`tests/validation/384..403`) in JIT and AOT --
      the language-surface contracts for streaming lifecycle, run-level grant
      enforcement, the journal artifact and run introspection, the
      aggregate-result lifetime regression, the dead-run guard matrix, spec
      rejection, the stream teardown boundary, provider routing, the scripted
      directive grammar and the async scalar-slot code-generation regressions;
  (g) the integrated-project validator
      (`scripts/validate_r3221_integrated_agent_service.py`).

Every check's status is written to
`target/r3221-agent-conformance/report.json`, which the release gate requires;
a single failing check exits non-zero. The gate is intentionally heavy: it
re-runs every item validator, because it is the release gate, not a smoke test.

The share-nothing work -- the item-validator sweep, the examples and the
fixtures -- runs in a bounded process pool (`--jobs`, default 4). Each
validator owns its `target/` and `.spectra/` paths, the crate tests namespace
their work directory by process id, and the catalog probes generate into a
per-item path instead of the repository, so concurrency changes the wall clock
and nothing else: results are reported in input order and a failure is named
with the lowest item index that failed.

Two R-3221 validators are excluded from the sweep for exactly one reason each:
this script itself (recursion) and the integrated-project validator, which check
(f) runs as its dedicated slot. The package validator
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
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from typing import Callable, TypeVar


ROOT = Path(__file__).resolve().parents[1]
SCRIPT_DIR = ROOT / "scripts"
WORK = ROOT / "target" / "r3221-agent-conformance"
REPORT_PATH = WORK / "report.json"

# Parallelism for the independent work: the item-validator sweep, the examples
# and the fixtures are share-nothing processes (each owns its `target/` and
# `.spectra/` paths, and the crate tests namespace their work directory by
# process id), so they run in a bounded pool instead of one after another. Four
# leaves the six-core reference host room for the linker, which is the
# heaviest single operation; `--jobs` overrides it.
MAX_JOBS = 4
JOBS = max(1, min(MAX_JOBS, os.cpu_count() or 1))

T = TypeVar("T")
R = TypeVar("R")

SELF = "scripts/validate_r3221_agent_conformance.py"
INTEGRATED_VALIDATOR = "scripts/validate_r3221_integrated_agent_service.py"
SWEEP_EXCLUDED = {SELF, INTEGRATED_VALIDATOR}

INTEGRATED_PROJECT = "tests/projects/valid/integrated_agent_service"

EXAMPLES = {
    "01-tool-and-run": "examples/agent/01-tool-and-run",
    "02-approval-and-budget": "examples/agent/02-approval-and-budget",
    "03-mcp-and-memory": "examples/agent/03-mcp-and-memory",
    "04-durable-replay": "examples/agent/04-durable-replay",
    "05-streaming-and-schema": "examples/agent/05-streaming-and-schema",
    "06-capabilities-and-taint": "examples/agent/06-capabilities-and-taint",
    "07-protocol-surface": "examples/agent/07-protocol-surface",
    "08-memory-and-recall": "examples/agent/08-memory-and-recall",
    "09-compensation-saga": "examples/agent/09-compensation-saga",
    "10-list-payloads": "examples/agent/10-list-payloads",
    "11-tool-call-budget": "examples/agent/11-tool-call-budget",
    "12-structured-output": "examples/agent/12-structured-output",
    "13-embeddings-and-ranking": "examples/agent/13-embeddings-and-ranking",
    "14-acp-and-permissions": "examples/agent/14-acp-and-permissions",
    "15-mcp-service-surface": "examples/agent/15-mcp-service-surface",
}

# The language-surface contracts that back the example set: the streaming
# handle lifecycle, run-level grant enforcement, the journal artifact and run
# introspection. They are fixtures, not examples: each asserts its contract and
# exits non-zero on the first mismatch.
VERIFICATION_FIXTURES = {
    "stream_lifecycle": "tests/validation/384_agent_stream_lifecycle.spectra",
    "capabilities_in_practice": "tests/validation/385_agent_capabilities_in_practice.spectra",
    "journal_artifact": "tests/validation/386_agent_journal_artifact.spectra",
    "introspection": "tests/validation/387_agent_introspection.spectra",
    "aggregate_result_lifetime": "tests/validation/388_async_aggregate_result_lifetime.spectra",
    "string_and_container_payload_lifetime": "tests/validation/389_string_and_container_payload_lifetime.spectra",
    "nested_dispatch": "tests/validation/390_agent_nested_dispatch.spectra",
    "concurrent_runs": "tests/validation/391_agent_concurrent_runs.spectra",
    "payload_scale": "tests/validation/392_agent_payload_scale.spectra",
    "tool_call_ceiling": "tests/validation/393_agent_tool_call_ceiling.spectra",
    "structured_output": "tests/validation/394_agent_structured_output.spectra",
    "memory_limits": "tests/validation/395_agent_memory_limits.spectra",
    "stream_in_tool": "tests/validation/396_agent_stream_in_tool.spectra",
    "nested_dispatch_stress": "tests/validation/397_agent_nested_dispatch_stress.spectra",
    "dead_handle_matrix": "tests/validation/398_agent_dead_handle_matrix.spectra",
    "spec_rejection": "tests/validation/399_agent_spec_rejection.spectra",
    "stream_teardown": "tests/validation/400_agent_stream_teardown.spectra",
    "provider_routing": "tests/validation/401_agent_provider_routing.spectra",
    "script_directives": "tests/validation/402_agent_script_directives.spectra",
    "async_scalar_slots": "tests/validation/403_async_scalar_slots_and_float_payloads.spectra",
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


def parallel_map(items: list[T], worker: Callable[[T], R], label: str) -> list[R]:
    """Maps `worker` over `items` with at most [`JOBS`] in flight.

    Results come back in input order. A worker raises `CheckFailure` with its
    own item named in the message; when more than one fails, the failure
    reported is the one with the lowest input index, and everything not yet
    started is cancelled, so a failing run names the same item it would name if
    the work had been serial.
    """
    if not items:
        return []
    results: list[R | None] = [None] * len(items)
    failures: list[tuple[int, str]] = []
    workers = max(1, min(JOBS, len(items)))
    with ThreadPoolExecutor(max_workers=workers) as pool:
        futures = {pool.submit(worker, item): index for index, item in enumerate(items)}
        for future in as_completed(futures):
            index = futures[future]
            try:
                results[index] = future.result()
            except CheckFailure as error:
                failures.append((index, str(error)))
                for pending in futures:
                    pending.cancel()
            except Exception as error:  # noqa: BLE001 - reported with the item's failure
                failures.append((index, f"{type(error).__name__}: {error}"))
                for pending in futures:
                    pending.cancel()
    if failures:
        index, detail = min(failures, key=lambda failure: failure[0])
        raise CheckFailure(f"{label} stopped at item {index + 1}: {detail}")
    return [result for result in results if result is not None]  # type: ignore[misc]


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


# ── (a) the prelude: build once, share with every validator ──────────────


def check_cli_build() -> dict:
    """Builds what the rest of the gate consumes and publishes it in the env.

    Both builds happen once, here, in cargo's non-test feature set:

      * `spectra-cli` -- the binary every validator would otherwise build for
        itself. Validators skip their own build when `SPECTRA_CLI_BUILT` is
        set, which also keeps them from flipping the shared build directory
        between cargo's test and non-test feature sets (a flip rebuilds the
        world and costs minutes).
      * `dump_stdlib_contract` -- the compiler's contract dump, exported as
        `SPECTRA_CONTRACT_DUMP` so the eleven catalog probes run it directly
        instead of re-entering cargo.
    """
    evidence: dict = {}
    if BINARY is None:
        output = cargo(["build", "-q", "-p", "spectra-cli", "--offline"], 1200, "spectra-cli build")
        evidence["output"] = tail(output, 5)
    binary = resolve_binary()

    dump = ROOT / "target" / "debug" / ("dump_stdlib_contract.exe" if sys.platform.startswith("win") else "dump_stdlib_contract")
    dump_output = cargo(
        ["build", "-q", "-p", "spectra-compiler", "--bin", "dump_stdlib_contract", "--offline"],
        1200,
        "contract dump build",
    )
    if not dump.is_file():
        raise CheckFailure(f"contract dump binary missing after its build: {dump}")
    evidence["dump"] = tail(dump_output, 3)

    os.environ["SPECTRALANG_BINARY"] = str(binary)
    os.environ["SPECTRA_CLI_BUILT"] = "1"
    os.environ["SPECTRA_CONTRACT_DUMP"] = str(dump)
    evidence["binary"] = str(binary)
    evidence["reused_binary"] = BINARY is not None and "output" not in evidence
    return evidence


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
        # Every validator of the item runs: an item with several scripts
        # (R-3221 has the package gate and the host-adapter gate) must not
        # silently drop the ones that sort after the first.
        found.extend((f"R-{number}", script) for script in matches)
    return found


def check_item_validators() -> dict:
    entries = expected_validators()

    def validate(entry: tuple[str, Path]) -> dict:
        item, script = entry
        started = time.monotonic()
        code, output = run([sys.executable, str(script.relative_to(ROOT))], timeout=2400)
        record = {
            "item": item,
            "script": str(script.relative_to(ROOT)),
            "status": "passed" if code == 0 else "failed",
            "seconds": round(time.monotonic() - started, 2),
        }
        if code != 0:
            raise CheckFailure(f"{item} validator {script.name} failed (exit {code}):\n{tail(output, 30)}")
        return record

    results = parallel_map(entries, validate, "validator sweep")
    return {"validators": results, "count": len(results), "jobs": JOBS}


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
    example_work = WORK / "examples"
    example_work.mkdir(parents=True, exist_ok=True)

    def check(entry: tuple[str, str]) -> tuple[str, dict]:
        name, project = entry
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
        return name, {"jit": jit_tail, "aot": tail(output, 5)}

    pairs = parallel_map(list(EXAMPLES.items()), check, "example set")
    return dict(pairs)


# ── (e) the verification fixtures ────────────────────────────────────────


def check_verification_fixtures() -> dict:
    binary = resolve_binary()
    fixture_work = WORK / "fixtures"
    fixture_work.mkdir(parents=True, exist_ok=True)

    def check(entry: tuple[str, str]) -> tuple[str, dict]:
        name, fixture = entry
        source = ROOT / fixture
        if not source.is_file():
            raise CheckFailure(f"verification fixture {name} has no {fixture}")

        code, output = run([binary, "run", fixture], timeout=600)
        if code != 0:
            raise CheckFailure(f"fixture {name} (JIT) exited {code}:\n{tail(output)}")
        jit_tail = tail(output, 5)

        executable = fixture_work / (name + (".exe" if sys.platform.startswith("win") else ""))
        if executable.exists():
            executable.unlink()
        code, output = run(
            [
                binary,
                "compile",
                "--debug-info=none",
                "--emit-exe",
                str(executable),
                fixture,
            ],
            timeout=900,
        )
        if code != 0 or not executable.is_file():
            raise CheckFailure(f"fixture {name} (AOT compile) exited {code}:\n{tail(output)}")

        code, output = run([executable], timeout=600)
        if code != 0:
            raise CheckFailure(f"fixture {name} (AOT) exited {code}:\n{tail(output)}")
        return name, {"jit": jit_tail, "aot": tail(output, 5)}

    pairs = parallel_map(list(VERIFICATION_FIXTURES.items()), check, "fixture set")
    return dict(pairs)


# ── (f) the integrated project ───────────────────────────────────────────


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
        ("verification_fixtures_jit_and_aot", check_verification_fixtures),
        ("integrated_project", check_integrated_project),
    ]


def main() -> None:
    global JOBS

    parser = argparse.ArgumentParser(description="R-3221 agent platform certification gate")
    parser.add_argument(
        "--binary",
        default=None,
        help="spectralang binary to use (default: target/debug/spectralang[.exe], built if absent)",
    )
    parser.add_argument(
        "--jobs",
        type=int,
        default=JOBS,
        help=f"parallel processes for the share-nothing work (default: {JOBS}; the sweep, the "
        "examples and the fixtures run in a pool of this size, 1 restores the serial order)",
    )
    arguments = parser.parse_args()

    JOBS = max(1, arguments.jobs)

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
