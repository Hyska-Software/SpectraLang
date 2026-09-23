"""Execute the full language-fixture corpus through JIT and AOT.

The broad PowerShell runner (run_tests.ps1) compiles every file under
tests/validation and tests/control_flow but never executes most of them, so a
miscompile that still type-checks passes the default suite.  This gate closes
that gap:

- fixtures declaring ``func main`` are EXECUTED through both pipelines:

  - JIT: ``spectralang run <fixture>``
  - AOT: ``spectralang compile --emit-exe --debug-info=none <fixture>`` and
    then running the produced executable

  A fixture passes when both pipelines produce the expected process exit code.

- fixtures without a ``main`` are compile-checked only (``kind: compile``).

Sidecar files next to a fixture customise expectations:

- ``<name>.exit``   — expected process exit code (default 0), e.g. a fixture
  asserting JIT/AOT exit-code parity writes ``7`` for ``main`` returning 7.
- ``<name>.stdout`` — exact expected stdout (line endings normalised).
- ``<name>.skip``   — do not execute; first line is the reason (e.g. fixtures
  that intentionally loop forever).

Expected behaviour is recorded in ``tests/execution-baseline.json``.  Two
modes:

- default (gate): every baselined fixture must reproduce its recorded status;
  a fixture absent from the baseline passes only if it already meets the
  sidecar/default expectations (reported as NEW — run with
  ``--update-baseline`` to record it); anything else fails the gate.
- ``--update-baseline``: execute everything and rewrite the baseline.

Fixtures whose baseline status is a timeout are skipped in gate mode (they are
known to hang and re-running them would only waste wall-clock time).
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BASELINE_PATH = ROOT / "tests" / "execution-baseline.json"
REPORT_DEFAULT = ROOT / "target" / "execution-coverage" / "report.json"
EXE_DIR = ROOT / "target" / "execution-coverage" / "exes"
SCHEMA = "spectralang.execution_coverage.v1"

MAIN_RE = re.compile(r"\bfunc\s+main\s*\(")
SOURCE_DIRS = (
    ROOT / "tests" / "validation",
    ROOT / "tests" / "control_flow",
)

EXE_SUFFIX = ".exe" if sys.platform == "win32" else ""


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        default=str(ROOT / "target" / "debug" / "spectralang.exe"),
        help="path to the SpectraLang CLI binary",
    )
    parser.add_argument(
        "--mode",
        choices=("both", "jit"),
        default="both",
        help="'both' runs JIT + AOT; 'jit' runs the JIT pipeline only (faster CI signal)",
    )
    parser.add_argument(
        "--update-baseline",
        action="store_true",
        help="execute everything and rewrite tests/execution-baseline.json",
    )
    parser.add_argument("--report", default=str(REPORT_DEFAULT), help="JSON report path")
    parser.add_argument(
        "--jobs",
        type=int,
        default=min(8, os.cpu_count() or 2),
        help="parallel fixtures",
    )
    parser.add_argument(
        "--only",
        default="",
        help="substring filter on fixture paths (debugging)",
    )
    parser.add_argument("--jit-timeout", type=float, default=30.0)
    parser.add_argument("--compile-timeout", type=float, default=180.0)
    return parser.parse_args()


def resolve_binary(raw: str) -> Path:
    binary = Path(raw)
    if not binary.is_absolute():
        binary = ROOT / binary
    return binary.resolve()


def run(command: list[str], timeout_seconds: float) -> dict[str, object]:
    """Run a command and classify the outcome as a stable status string."""
    try:
        completed = subprocess.run(
            command,
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return {"status": "timeout", "detail": f"timed out after {timeout_seconds:.0f}s"}
    detail = (completed.stdout + "\n" + completed.stderr).strip()
    return {
        "status": f"exit:{completed.returncode}",
        "stdout": completed.stdout,
        "detail": detail[-2000:],
    }


def read_sidecar_int(fixture: Path, suffix: str, default: int | None) -> int | None:
    sidecar = fixture.with_suffix(suffix)
    if not sidecar.is_file():
        return default
    try:
        return int(sidecar.read_text(encoding="utf-8").strip().splitlines()[0])
    except (ValueError, IndexError):
        return default


def exit_status_to_code(status: object) -> int | None:
    """Parse an ``exit:N`` status string back into N."""
    text = str(status)
    if text.startswith("exit:"):
        try:
            return int(text[len("exit:"):])
        except ValueError:
            return None
    return None


def read_sidecar(fixture: Path, suffix: str) -> str | None:
    sidecar = fixture.with_suffix(suffix)
    if not sidecar.is_file():
        return None
    return sidecar.read_text(encoding="utf-8", errors="replace")


def normalize_stdout(text: str) -> str:
    return text.replace("\r\n", "\n").strip()


def discover_fixtures(only: str) -> list[Path]:
    fixtures: list[Path] = []
    for directory in SOURCE_DIRS:
        if not directory.is_dir():
            continue
        fixtures.extend(sorted(directory.glob("*.spectra")))
    if only:
        fixtures = [f for f in fixtures if only in f.relative_to(ROOT).as_posix()]
    return fixtures


def load_baseline() -> dict[str, object]:
    if not BASELINE_PATH.is_file():
        return {"schema": SCHEMA, "fixtures": {}}
    data = json.loads(BASELINE_PATH.read_text(encoding="utf-8"))
    if data.get("schema") != SCHEMA:
        raise SystemExit(f"unexpected baseline schema in {BASELINE_PATH}")
    return data


def execute_fixture(
    binary: Path, fixture: Path, args: argparse.Namespace
) -> dict[str, object]:
    rel = fixture.relative_to(ROOT).as_posix()
    skip_reason = read_sidecar(fixture, ".skip")
    if skip_reason is not None:
        first_line = skip_reason.strip().splitlines()[0] if skip_reason.strip() else "sidecar skip"
        return {"fixture": rel, "kind": "skip", "reason": first_line}

    # Expected exit code precedence: `.exit` sidecar (explicit intent) wins;
    # otherwise the baseline carries what this fixture has always returned
    # (many fixtures return computed checksums from main by design), and only
    # genuinely new fixtures fall back to 0.
    expected_exit = read_sidecar_int(fixture, ".exit", None)
    expected_stdout_raw = read_sidecar(fixture, ".stdout")
    expected_stdout = (
        normalize_stdout(expected_stdout_raw) if expected_stdout_raw is not None else None
    )

    source = fixture.read_text(encoding="utf-8", errors="replace")
    if not MAIN_RE.search(source):
        compile_result = run([str(binary), "compile", str(fixture)], args.compile_timeout)
        return {
            "fixture": rel,
            "kind": "compile",
            "compile": compile_result["status"],
            "detail": compile_result.get("detail", ""),
        }

    case: dict[str, object] = {
        "fixture": rel,
        "kind": "exec",
    }
    if expected_exit is not None:
        case["expected_exit"] = expected_exit

    jit = run([str(binary), "run", str(fixture)], args.jit_timeout)
    case["jit"] = jit["status"]
    if jit["status"] != "timeout" and expected_stdout is not None:
        if normalize_stdout(str(jit.get("stdout", ""))) != expected_stdout:
            case["jit"] = "stdout_mismatch"
    if jit["status"] != "exit:0" or case["jit"] != jit["status"]:
        case["jit_detail"] = jit.get("detail", "")

    if args.mode == "jit":
        return case

    exe = EXE_DIR / f"{fixture.stem}{EXE_SUFFIX}"
    exe.parent.mkdir(parents=True, exist_ok=True)
    if exe.exists():
        try:
            exe.unlink()
        except OSError:
            pass
    compiled = run(
        [
            str(binary),
            "compile",
            "--emit-exe",
            str(exe),
            "--debug-info=none",
            str(fixture),
        ],
        args.compile_timeout,
    )
    if compiled["status"] != "exit:0" or not exe.is_file():
        case["aot"] = "compile_fail"
        case["aot_detail"] = compiled.get("detail", "")
        return case

    executed = run([str(exe)], args.jit_timeout)
    case["aot"] = executed["status"]
    if executed["status"] != "timeout" and expected_stdout is not None:
        if normalize_stdout(str(executed.get("stdout", ""))) != expected_stdout:
            case["aot"] = "stdout_mismatch"
    if executed["status"] != "exit:0" or case["aot"] != executed["status"]:
        case["aot_detail"] = executed.get("detail", "")
    return case


def expected_exit_of(case: dict[str, object], baseline_entry: dict[str, object] | None) -> int:
    if "expected_exit" in case:
        return int(case["expected_exit"])  # `.exit` sidecar: explicit intent
    if baseline_entry and "expected_exit" in baseline_entry:
        return int(baseline_entry["expected_exit"])  # historically observed
    return 0  # genuinely new fixture without a sidecar


def evaluate(
    case: dict[str, object],
    baseline_entry: dict[str, object] | None,
    update: bool,
    mode: str,
) -> tuple[str, list[str]]:
    """Return (verdict, notes) with verdict in {pass, warn, fail, skip}."""
    notes: list[str] = []
    rel = str(case["fixture"])

    if case["kind"] == "skip":
        if baseline_entry is None:
            return "warn", [f"NEW skip ({case.get('reason', '')}) — run --update-baseline"]
        if baseline_entry.get("kind") != "skip":
            return "fail", [f"sidecar .skip added but baseline kind is {baseline_entry.get('kind')} — run --update-baseline"]
        return "skip", notes

    if update:
        # `--update-baseline` must never enshrine a broken fixture: every
        # validation fixture has to compile, and (in AOT mode) produce a
        # working executable.  Runtime exit codes and hangs are recorded as
        # observed — many fixtures return computed checksums from `main`.
        if case["kind"] == "error":
            return "fail", [f"internal error: {case.get('detail', '')}"]
        if case["kind"] == "compile" and case.get("compile") != "exit:0":
            return "fail", [
                f"refusing to baseline: fixture does not compile ({case.get('compile')}) {case.get('detail', '')}"
            ]
        if (
            case["kind"] == "exec"
            and mode == "both"
            and case.get("aot") == "compile_fail"
        ):
            return "fail", [
                f"refusing to baseline: AOT compile failed — {str(case.get('aot_detail', ''))[:500]}"
            ]
        return "pass", notes  # baseline is rewritten from observations

    if baseline_entry is None:
        # New fixture: must already satisfy expectations, otherwise fail.
        if case["kind"] == "compile":
            observed = case.get("compile")
            ok = observed == "exit:0"
            if not ok:
                return "fail", [f"NEW fixture fails compile: {observed} {case.get('detail', '')}"]
            return "warn", ["NEW compile-only fixture passes — run --update-baseline"]
        expected = int(case.get("expected_exit", 0))
        problems = []
        jit = case.get("jit")
        if mode == "jit":
            if jit != f"exit:{expected}":
                problems.append(f"jit={jit} (expected exit:{expected})")
        else:
            if jit != f"exit:{expected}":
                problems.append(f"jit={jit} (expected exit:{expected})")
            if case.get("aot") != f"exit:{expected}":
                problems.append(f"aot={case.get('aot')} (expected exit:{expected})")
        if problems:
            detail = " ".join(str(case.get(k, "")) for k in ("jit_detail", "aot_detail"))
            return "fail", [
                f"NEW fixture fails: {'; '.join(problems)} {detail} "
                "(if the non-zero exit is by design — e.g. a checksum return — "
                "record it with --update-baseline or add a .exit sidecar)"
            ]
        return "warn", ["NEW executable fixture passes — run --update-baseline"]

    # Baselined fixture: behaviour must be reproduced exactly.
    notes = []
    if case["kind"] != baseline_entry.get("kind"):
        return "fail", [f"kind changed: baseline={baseline_entry.get('kind')} observed={case['kind']} — run --update-baseline if intentional"]

    if case["kind"] == "compile":
        observed = case.get("compile")
        recorded = baseline_entry.get("compile")
        if observed == "timeout":
            return "fail", [f"compile timed out (baseline {recorded})"]
        if observed != recorded:
            return "fail", [f"compile {observed} != baseline {recorded} — run --update-baseline if intentional"]
        return "pass", notes

    expected = expected_exit_of(case, baseline_entry)
    for pipeline in (("jit",) if mode == "jit" else ("jit", "aot")):
        recorded = baseline_entry.get(pipeline)
        observed = case.get(pipeline)
        if recorded == "timeout":
            # Known hang: execution was skipped, nothing to compare.
            if observed is not None and observed != "timeout":
                notes.append(f"{pipeline} no longer times out — run --update-baseline")
            continue
        if observed != f"exit:{expected}":
            detail = str(case.get(f"{pipeline}_detail", ""))
            hint = ""
            if observed == "timeout":
                hint = " (timeout; add a .skip sidecar if it intentionally hangs)"
            return "fail", [f"{pipeline}={observed} expected exit:{expected}{hint} {detail}".strip()]
        if observed != recorded:
            return "fail", [f"{pipeline} {observed} != baseline {recorded} — run --update-baseline if intentional"]
    return "pass", notes


def known_timeout(entry: dict[str, object] | None) -> bool:
    if not entry:
        return False
    return entry.get("jit") == "timeout" or entry.get("aot") == "timeout"


def main() -> int:
    args = parse_args()
    binary = resolve_binary(args.binary)
    if not binary.is_file():
        print(f"binary not found: {binary}", file=sys.stderr)
        return 2

    baseline = load_baseline()
    baseline_fixtures: dict[str, dict[str, object]] = dict(baseline.get("fixtures", {}))
    fixtures = discover_fixtures(args.only)
    if not fixtures:
        print("no fixtures found", file=sys.stderr)
        return 2

    EXE_DIR.mkdir(parents=True, exist_ok=True)

    to_run: list[Path] = []
    results: dict[str, dict[str, object]] = {}
    skipped_known: list[str] = []
    for fixture in fixtures:
        rel = fixture.relative_to(ROOT).as_posix()
        entry = baseline_fixtures.get(rel)
        if (
            not args.update_baseline
            and entry is not None
            and entry.get("kind") == "skip"
        ):
            results[rel] = {"fixture": rel, "kind": "skip", "reason": str(entry.get("reason", "baseline skip"))}
        elif (
            not args.update_baseline
            and not args.only
            and known_timeout(entry)
            and read_sidecar(fixture, ".skip") is None
        ):
            # Known-hang fixture without a .skip sidecar: do not burn the
            # timeout budget again; carry the baseline statuses forward.
            skipped_known.append(rel)
            results[rel] = {
                "fixture": rel,
                "kind": entry.get("kind", "exec"),
                "jit": entry.get("jit"),
                "aot": entry.get("aot"),
                "expected_exit": entry.get("expected_exit", 0),
                "known_timeout": True,
            }
        else:
            to_run.append(fixture)

    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
        futures = {
            pool.submit(execute_fixture, binary, fixture, args): fixture
            for fixture in to_run
        }
        for future in concurrent.futures.as_completed(futures):
            fixture = futures[future]
            rel = fixture.relative_to(ROOT).as_posix()
            try:
                results[rel] = future.result()
            except Exception as error:  # pragma: no cover - defensive
                results[rel] = {"fixture": rel, "kind": "error", "detail": repr(error)}

    new_fixtures: dict[str, dict[str, object]] = {}
    passed: list[str] = []
    warned: list[tuple[str, list[str]]] = []
    failed: list[tuple[str, list[str]]] = []
    skipped: list[str] = []
    update_warnings: list[str] = []
    non_zero_observations = 0

    for rel in sorted(results):
        case = results[rel]
        entry = baseline_fixtures.get(rel)
        verdict, notes = evaluate(case, entry, args.update_baseline, args.mode)
        if verdict == "pass":
            passed.append(rel)
        elif verdict == "warn":
            warned.append((rel, notes))
        elif verdict == "fail":
            failed.append((rel, notes))
        elif verdict == "skip":
            skipped.append(rel)

        if args.update_baseline:
            record: dict[str, object] = {"kind": case.get("kind")}
            if case.get("kind") == "skip":
                record["reason"] = str(case.get("reason", ""))
            elif case.get("kind") == "compile":
                record["compile"] = case.get("compile")
            elif case.get("kind") == "exec":
                if "expected_exit" in case:
                    record["expected_exit"] = case["expected_exit"]  # `.exit` sidecar
                else:
                    # Learn the fixture's historical exit code (checksum-style
                    # mains return computed values by design).
                    derived = exit_status_to_code(case.get("jit"))
                    record["expected_exit"] = derived if derived is not None else 0
                record["jit"] = case.get("jit")
                if args.mode == "both":
                    record["aot"] = case.get("aot")
                if case.get("jit") == "timeout":
                    update_warnings.append(
                        f"{rel}: JIT execution timed out (add a .skip sidecar if it intentionally hangs)"
                    )
                if args.mode == "both" and case.get("aot") == "timeout":
                    update_warnings.append(
                        f"{rel}: AOT execution timed out (add a .skip sidecar if it intentionally hangs)"
                    )
                jit_code = exit_status_to_code(case.get("jit"))
                if (
                    "expected_exit" not in case
                    and jit_code is not None
                    and jit_code != 0
                ):
                    non_zero_observations += 1
            elif case.get("kind") == "error":
                # Never baseline an internal error: kept red via the fail path.
                continue
            new_fixtures[rel] = record

    if args.update_baseline:
        if failed:
            for rel, notes in failed:
                print(f"FAIL {rel}: {'; '.join(notes)}", file=sys.stderr)
            print(
                "refusing to write a baseline containing failures; fix the fixtures first",
                file=sys.stderr,
            )
            return 1
        payload = {"schema": SCHEMA, "mode": args.mode, "fixtures": new_fixtures}
        BASELINE_PATH.parent.mkdir(parents=True, exist_ok=True)
        BASELINE_PATH.write_text(
            json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        print(f"baseline updated: {len(new_fixtures)} fixtures -> {BASELINE_PATH}")
        if non_zero_observations:
            print(
                f"recorded {non_zero_observations} fixture(s) with a non-zero JIT exit "
                "as observed (verify the checksum-style returns are by design)"
            )
        for warning in update_warnings:
            print(f"WARN {warning}")
        return 0

    report = {
        "schema": SCHEMA,
        "mode": args.mode,
        "fixture_count": len(results),
        "passed": len(passed),
        "warned": len(warned),
        "failed": len(failed),
        "skipped": len(skipped) + len(skipped_known),
        "cases": [results[rel] for rel in sorted(results)],
    }
    report_path = Path(args.report)
    if not report_path.is_absolute():
        report_path = ROOT / report_path
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    print(
        f"execution coverage: {len(passed)} passed, {len(warned)} new, "
        f"{len(failed)} failed, {len(skipped) + len(skipped_known)} skipped "
        f"(mode={args.mode})"
    )
    for rel, notes in warned:
        print(f"NEW {rel}: {'; '.join(notes)}")
    if failed:
        for rel, notes in failed:
            print(f"FAIL {rel}: {'; '.join(notes)}", file=sys.stderr)
        print(f"report: {report_path}", file=sys.stderr)
        return 1
    print(f"report: {report_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
