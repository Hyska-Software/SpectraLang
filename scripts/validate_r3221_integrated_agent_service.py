# R-3221 T6 — the integrated agent-service project gate.
#
# The project `tests/projects/valid/integrated_agent_service` is a multi-module
# Spectra service: it binds a localhost HTTP listener, runs one agent turn
# with a token budget and the `spectra.std.fs` capability, counts two effects
# through the governed tool dispatch, asks for approval of one action, and
# journals the run. This validator proves:
#
#   * the project runs to completion under `spectralang run` (JIT) and as an
#     emitted executable (`compile --debug-info=none --emit-exe`) with the same
#     accounting;
#   * the HTTP endpoint answers real requests over a socket while the run is
#     parked mid-flight (GET /health, /ledger, /status);
#   * an interrupted process (killed after the first effect is journaled) can
#     be re-run with the same run id and resumes the journal without repeating
#     the completed effect — the ledger keeps exactly one line per effect and
#     the journal keeps exactly one tool record per label;
#   * the report and the budget accounting hold and are identical between the
#     fresh and the resumed run.
#
# Interrupt/resume mechanism (printed as part of the verdict):
#   * gate:      `SPECTRA_R3221_GATE=interrupt` makes the run park after the
#                first journaled effect, waiting for the release marker;
#   * release:   `target/r3221-integrated-agent-service/release.txt`; the
#                resumed process finds it and continues without waiting;
#   * kill:      `Popen.kill()` — TerminateProcess on Windows, SIGKILL
#                elsewhere — while the run is parked, never mid-write, so the
#                append-only journal on disk is complete;
#   * run id:    the same `integrated-agent-service` run id and the same
#                journal file for both processes, which is what makes the
#                second process a replay/resume instead of a new run.
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
import tomllib
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPECTRALANG = ROOT / "target" / "debug" / "spectralang.exe"
CARGO = shutil.which("cargo") or "cargo"

PROJECT = "tests/projects/valid/integrated_agent_service"
PROJECT_ROOT = ROOT / PROJECT
WORK = ROOT / "target" / "r3221-integrated-agent-service"
REPORT_DIR = ROOT / "target" / "r3221-integrated-agent-service-validation"
REPORT_PATH = REPORT_DIR / "report.json"

RUN_ID = "integrated-agent-service"
JOURNAL = WORK / "journal" / f"{RUN_ID}.jsonl"
LEDGER = WORK / "ledger.txt"
STATUS = WORK / "status.txt"
PORT_FILE = WORK / "port.txt"
RELEASE = WORK / "release.txt"

GATE_VARIABLE = "SPECTRA_R3221_GATE"
GATE_MODE = "interrupt"
AOT_EXECUTABLE = ROOT / "target" / "r3221-integrated-agent-service.exe"

ROADMAP = "roadmap/roadmap.toml"
ITEM_ID = "R-3221"
PHASE_ID = "phase_32"

# The mock provider's documented cost table (micros per token); the report's
# cost must be exactly this function of its token counts.
COST_PER_INPUT_TOKEN = 1
COST_PER_OUTPUT_TOKEN = 2

JOURNAL_KINDS = ["model", "tool", "approval", "assertion", "tool"]
REPORT_ACCOUNTING_FIELDS = [
    "status",
    "steps",
    "tool_calls",
    "tokens_in",
    "tokens_out",
    "cost_micros",
    "ceiling",
    "compensations_pending",
]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3221 integrated project validation failed: {message}", file=sys.stderr)
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
        fail(f"command {' '.join(str(arg) for arg in args)} failed:\n{completed.stdout}")
    return completed.stdout


def clean_work_dir() -> None:
    shutil.rmtree(WORK, ignore_errors=True)


def journal_records() -> list[dict]:
    require(JOURNAL.exists(), f"the run must journal {JOURNAL}")
    return [
        json.loads(line)
        for line in JOURNAL.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]


def ledger_lines() -> list[str]:
    require(LEDGER.exists(), f"the run must write {LEDGER}")
    return [line for line in LEDGER.read_text(encoding="utf-8").split("\n") if line != ""]


def status_report() -> dict:
    require(STATUS.exists(), f"the run must write its report to {STATUS}")
    return json.loads(STATUS.read_text(encoding="utf-8"))


def launch(source: Path, *, gate: bool) -> subprocess.Popen:
    """Start the project (JIT source or emitted executable) with the gate on."""
    environment = dict(os.environ)
    if gate:
        environment[GATE_VARIABLE] = GATE_MODE
    else:
        environment.pop(GATE_VARIABLE, None)
    command = [str(SPECTRALANG), "run", PROJECT] if source == SPECTRALANG else [str(source)]
    return subprocess.Popen(
        command,
        cwd=ROOT,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )


def wait_until(predicate, timeout: float, message: str) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.05)
    fail(message)


def http_get(port: int, path: str) -> tuple[int, str, str]:
    with urllib.request.urlopen(f"http://127.0.0.1:{port}{path}", timeout=10) as response:
        return response.status, response.headers.get("Content-Type", ""), response.read().decode("utf-8")


def require_accounting(report: dict, label: str) -> None:
    require(report.get("status") == "completed", f"{label}: status {report.get('status')!r}")
    require(report.get("ceiling") == "", f"{label}: the run must not hit a ceiling: {report.get('ceiling')!r}")
    require(report.get("tool_calls") == 2, f"{label}: two governed tool calls, got {report.get('tool_calls')}")
    require(report.get("compensations_pending") == 0, f"{label}: no pending compensations")
    tokens_in = report.get("tokens_in")
    tokens_out = report.get("tokens_out")
    require(isinstance(tokens_in, int) and tokens_in > 0, f"{label}: the model turn must spend input tokens")
    require(isinstance(tokens_out, int) and tokens_out > 0, f"{label}: the model turn must spend output tokens")
    require(
        report.get("cost_micros") == tokens_in * COST_PER_INPUT_TOKEN + tokens_out * COST_PER_OUTPUT_TOKEN,
        f"{label}: cost_micros {report.get('cost_micros')} does not match the mock cost table for "
        f"{tokens_in}/{tokens_out} tokens",
    )


def require_run_files(report: dict, label: str, *, replay: bool) -> None:
    require_accounting(report, label)
    require(report.get("replay") is replay, f"{label}: replay must be {replay}")

    require(
        ledger_lines() == ["first", "second"],
        f"{label}: the ledger must hold exactly one line per effect, got {ledger_lines()!r}",
    )

    records = journal_records()
    require(
        [record["kind"] for record in records] == JOURNAL_KINDS,
        f"{label}: journal kinds {[record['kind'] for record in records]!r}",
    )
    require(
        [record["step"] for record in records] == list(range(len(JOURNAL_KINDS))),
        f"{label}: the journal steps must be a 0..n sequence",
    )
    require(all(record["run"] == RUN_ID for record in records), f"{label}: every record carries the run id")

    # Exactly one durable record per counted effect: the "no effect twice"
    # proof at the journal level.
    for label_text in ("first", "second"):
        matching = [
            record
            for record in records
            if record["kind"] == "tool" and f'"label":"{label_text}"' in (record.get("input") or "")
        ]
        require(
            len(matching) == 1,
            f"{label}: the journal must hold exactly one tool record for '{label_text}', found {len(matching)}",
        )
    require(
        all("notify_release" not in (record.get("input") or "") for record in records),
        f"{label}: the approval-gated action must never be dispatched",
    )

    # The single model step is the budget evidence: the report's accounting is
    # exactly the usage the journal recorded for it.
    model = [record for record in records if record["kind"] == "model"]
    require(len(model) == 1, f"{label}: one model step, found {len(model)}")
    usage = model[0]["usage"]
    require(
        usage["tokens_in"] == report["tokens_in"] and usage["tokens_out"] == report["tokens_out"],
        f"{label}: the report tokens must equal the journaled model usage",
    )
    require(
        usage["cost_micros"] == report["cost_micros"],
        f"{label}: the report cost must equal the journaled model cost",
    )

    # The approval decision is durable and fail-closed: no approver is attached
    # in this process, so the one action requiring approval is refused.
    approval = [record for record in records if record["kind"] == "approval"]
    require(len(approval) == 1, f"{label}: one approval decision, found {len(approval)}")
    require(approval[0]["output"] == "false", f"{label}: the approval must be a deny")
    require(
        "default-deny" in (approval[0].get("attribution") or ""),
        f"{label}: the denial must be attributed to the missing approver",
    )


def validate_project_shape() -> None:
    manifest = read(f"{PROJECT}/spectra.toml")
    for term in ['name = "integrated_agent_service"', 'entry = "src/main.spectra"', 'src_dirs = ["src"]']:
        require(term in manifest, f"spectra.toml missing {term}")

    modules = ["main", "service", "scenario", "tools", "store"]
    for module in modules:
        path = PROJECT_ROOT / "src" / f"{module}.spectra"
        require(path.is_file(), f"missing project module {path.relative_to(ROOT)}")

    main = read(f"{PROJECT}/src/main.spectra")
    for term in [
        "public func main() returns int",
        "listen(server, 0)",
        "block_on(serve(server, routes))",
        "shutdown(server)",
        "perform()",
        "capability_denial()",
    ]:
        require(term in main, f"main.spectra missing {term}")

    service = read(f"{PROJECT}/src/service.spectra")
    for term in ['"/health"', '"/ledger"', '"/status"', "register_sync_callback", '"text/plain"']:
        require(term in service, f"service.spectra missing {term}")

    scenario = read(f"{PROJECT}/src/scenario.spectra")
    for term in [
        "agent_start(",
        "budget_remaining(",
        "block_on(ask(",
        "block_on(tool_call(",
        "approve(",
        "require(",
        "agent_end(",
        "journal_payloads",
        "SPECTRA_R3221_GATE",
        "spectra.std.fs",
        '"mock:',
        "import tools",
    ]:
        require(term in scenario, f"scenario.spectra missing {term}")

    tools = read(f"{PROJECT}/src/tools.spectra")
    for term in [
        '#[agent_tool("',
        "public async func record_effect(",
        "public async func notify_release(",
        "fs_write(",
    ]:
        require(term in tools, f"tools.spectra missing {term}")


def validate_tracker() -> None:
    tracker = tomllib.loads(read(ROADMAP))
    phases = {phase.get("id") for phase in tracker.get("phases", [])}
    require(PHASE_ID in phases, f"{ROADMAP} does not register phase '{PHASE_ID}'")
    items = {item.get("id"): item for item in tracker.get("items", [])}
    require(ITEM_ID in items, f"{ROADMAP} does not register item '{ITEM_ID}'")
    require(items[ITEM_ID].get("phase") == PHASE_ID, f"{ITEM_ID} is not registered under '{PHASE_ID}'")


def compile_aot() -> None:
    run_command(
        [
            str(SPECTRALANG),
            "compile",
            "--debug-info=none",
            "--emit-exe",
            str(AOT_EXECUTABLE),
            PROJECT,
        ]
    )
    require(AOT_EXECUTABLE.is_file(), f"AOT compilation must emit {AOT_EXECUTABLE}")


def complete_run(source: Path, label: str) -> dict:
    """One uninterrupted run: the project's normal mode."""
    clean_work_dir()
    process = launch(source, gate=False)
    output, _ = process.communicate(timeout=120)
    require(process.returncode == 0, f"{label}: exit {process.returncode}\n{output}")
    report = status_report()
    require_run_files(report, label, replay=False)
    return report


def interrupt_resume(source: Path, label: str) -> dict:
    """Kill the run after its first effect and resume it with the same run id."""
    clean_work_dir()
    first = launch(source, gate=True)
    try:
        wait_until(
            lambda: PORT_FILE.exists() and LEDGER.exists() and "first" in LEDGER.read_text(encoding="utf-8"),
            60,
            f"{label}: the run never reached its first journaled effect",
        )
        require(first.poll() is None, f"{label}: the run must still be parked when we reach the kill point")
        port = int(PORT_FILE.read_text(encoding="utf-8").strip())
        require(port > 0, f"{label}: the service must report its bound port")

        # The HTTP surface is live while the run is parked: /ledger observes the
        # one effect already written, /status is still pending.
        health_status, health_type, health_body = http_get(port, "/health")
        require(health_status == 200, f"{label}: GET /health status {health_status}")
        require("text/plain" in health_type, f"{label}: GET /health content type {health_type!r}")
        require(health_body == "ok", f"{label}: GET /health body {health_body!r}")

        ledger_status, _, ledger_body = http_get(port, "/ledger")
        require(ledger_status == 200, f"{label}: GET /ledger status {ledger_status}")
        require(ledger_body == "first\n", f"{label}: GET /ledger must show the parked effect, got {ledger_body!r}")

        status_status, _, status_body = http_get(port, "/status")
        require(status_status == 200, f"{label}: GET /status status {status_status}")
        require(status_body == "pending", f"{label}: GET /status before completion {status_body!r}")

        # The interrupted process must not have completed: four steps are
        # durable and the second effect is not among them.
        records = journal_records()
        require(
            [record["kind"] for record in records] == JOURNAL_KINDS[:4],
            f"{label}: the interrupted journal must hold {JOURNAL_KINDS[:4]}, "
            f"got {[record['kind'] for record in records]!r}",
        )
        require(
            all('"label":"second"' not in (record.get("input") or "") for record in records),
            f"{label}: the second effect must not be recorded before the resume",
        )
        require(ledger_lines() == ["first"], f"{label}: one effect before the kill, got {ledger_lines()!r}")

        # Kill while parked — TerminateProcess on Windows, SIGKILL elsewhere.
        first.kill()
        first.communicate(timeout=30)
        require(first.returncode != 0, f"{label}: the killed process must not report success")
    finally:
        if first.poll() is None:
            first.kill()
            first.communicate(timeout=30)

    require(not STATUS.exists(), f"{label}: the interrupted run must not have written its report")

    # Resume: same run id, same journal file, release marker present so the
    # second process continues past the park point immediately.
    RELEASE.write_text("go", encoding="utf-8")
    second = launch(source, gate=True)
    output, _ = second.communicate(timeout=120)
    require(second.returncode == 0, f"{label}: resume exit {second.returncode}\n{output}")
    report = status_report()
    require_run_files(report, label + " resume", replay=True)
    return report


def validate_behavior() -> dict:
    evidence: dict = {}

    # JIT: fresh complete run, then the interrupt/resume proof.
    jit_report = complete_run(SPECTRALANG, "JIT")
    jit_resumed = interrupt_resume(SPECTRALANG, "JIT")
    evidence["jit"] = jit_report

    # AOT parity: the same two scenarios through the emitted executable.
    aot_report = complete_run(AOT_EXECUTABLE, "AOT")
    aot_resumed = interrupt_resume(AOT_EXECUTABLE, "AOT")
    evidence["aot"] = aot_report

    # The accounting is provider-independent and replay-independent: the
    # interrupted+resumed runs report exactly what the uninterrupted runs did.
    for label, report, baseline in [
        ("AOT fresh vs JIT fresh", aot_report, jit_report),
        ("JIT resume vs JIT fresh", jit_resumed, jit_report),
        ("AOT resume vs AOT fresh", aot_resumed, aot_report),
    ]:
        for field in REPORT_ACCOUNTING_FIELDS:
            require(
                report.get(field) == baseline.get(field),
                f"{label}: {field} differs ({report.get(field)!r} vs {baseline.get(field)!r})",
            )

    evidence["jit_resume"] = jit_resumed
    evidence["aot_resume"] = aot_resumed
    return evidence


def main() -> None:
    run_command([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"])
    validate_project_shape()
    validate_tracker()
    compile_aot()
    evidence = validate_behavior()

    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    REPORT_PATH.write_text(
        json.dumps(
            {
                "schema": "spectralang.r3221_integrated_agent_service.v1",
                "item": ITEM_ID,
                "project": PROJECT,
                "run_id": RUN_ID,
                "gate_variable": GATE_VARIABLE,
                "gate_mode": GATE_MODE,
                "release_marker": str(RELEASE.relative_to(ROOT)),
                "kill": "subprocess.Popen.kill() (TerminateProcess on Windows, SIGKILL elsewhere)",
                "reports": evidence,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )

    print("interrupt/resume mechanism:")
    print(f"  gate          : {GATE_VARIABLE}={GATE_MODE} parks the run after the first journaled effect")
    print(f"  release       : {RELEASE.relative_to(ROOT)} (the resumed run continues without waiting)")
    print("  kill          : Popen.kill() while parked, so the append-only journal on disk is complete")
    print(f"  run id        : {RUN_ID} (same journal file for the interrupted and the resumed process)")
    print("  HTTP proof    : GET /health, /ledger and /status over 127.0.0.1 while the run is parked")
    print("  no-dup proof  : ledger holds one line per effect; the journal holds one tool record per label")
    print(f"  report        : {REPORT_PATH.relative_to(ROOT)}")
    print("validated R-3221 integrated agent service (JIT, AOT, HTTP surface, interrupt/resume)")


if __name__ == "__main__":
    main()
