# R-3220 — evaluation harness: case format, graders, the CLI gate and the
# example suite.
#
# Validates the harness end to end: the module and grader surface, the suite
# and baseline documents, the `agent eval` command (pass@1/pass^k, cost, the
# regression gate at exit 65, the first-baseline write at exit 0), that the
# judge grader never runs on the default path, and that fixture 378 drives
# every scenario through both JIT and AOT with args.
from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPECTRALANG = ROOT / "target" / "debug" / "spectralang.exe"
CARGO = shutil.which("cargo") or "cargo"
FIXTURE = "tests/validation/378_agent_eval_harness.spectra"
SUITE = "examples/agent/evals/agent_core.json"
BASELINE = "examples/agent/evals/agent_core.baseline.json"
JOURNAL_DIR = ROOT / ".spectra" / "r3220-eval"
SCRATCH = ROOT / ".spectra" / "r3220-eval-suites"
REPORT_SCHEMA = "spectralang.agent-eval.v1"
SUITE_SCHEMA = "spectralang.agent-eval-suite.v1"
BASELINE_SCHEMA = "spectralang.agent-eval-baseline.v1"
INPUTS = ["approval", "refusal", "budget", "schema"]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def fail(message: str) -> None:
    print(f"R-3220 validation failed: {message}", file=sys.stderr)
    sys.exit(1)


def require(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def run(args: list[str], expect: int | None = 0) -> subprocess.CompletedProcess:
    completed = subprocess.run(
        [str(arg) for arg in args],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if expect is not None and completed.returncode != expect:
        fail(
            f"command {' '.join(str(arg) for arg in args)} exited "
            f"{completed.returncode}, expected {expect}:\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    return completed


def eval_json(*extra: str) -> tuple[dict, subprocess.CompletedProcess]:
    completed = run(
        [SPECTRALANG, "agent", "eval", "--json", "--suite", SUITE, *extra],
        expect=0,
    )
    try:
        return json.loads(completed.stdout), completed
    except json.JSONDecodeError as error:
        fail(f"agent eval --json did not print JSON: {error}\n{completed.stdout}")


def write_suite(path: Path, document: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(document, indent=2), encoding="utf-8")


def fixture_suite(journal: Path, cases: list[dict], judge: dict | None = None) -> dict:
    """A scratch suite pointing at fixture 378 with absolute paths."""
    document = {"schema": SUITE_SCHEMA, "cases": cases}
    if judge is not None:
        document["judge"] = judge
    for case in cases:
        case["program"] = (ROOT / FIXTURE).as_posix()
        case["journal"] = journal.as_posix()
        case.setdefault("repeat", 1)
        case["run_id"] = case.get("run_id", "eval-" + case["input"])
    return document


def journal_records(run_id: str) -> list[dict]:
    path = JOURNAL_DIR / f"{run_id}.jsonl"
    require(path.exists(), f"the fixture must write {path}")
    return [
        json.loads(line)
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]


def validate_implementation() -> None:
    module = read("packages/spectra-agent/src/eval/mod.rs")
    for term in [
        "pub const SUITE_SCHEMA",
        "pub const REPORT_SCHEMA",
        "pub const BASELINE_SCHEMA",
        "pub struct EvalCase",
        "pub struct EvalSuite",
        "pub trait CaseExecutor",
        "pub fn run_suite",
        "pub enum GraderKind",
        "pub fn deterministic()",
        "pass_at_1",
        "pass_at_k",
        "fresh process",
        "journal_payloads",
        "fn path_for",
        "pub fn regressions",
        "the eval program printed no run report",
        "pub struct BaselineStatus",
    ]:
        require(term in module, f"eval/mod.rs missing {term}")
    require(
        "argv[1] = input" in module,
        "eval/mod.rs must document the argv invocation contract",
    )

    graders = read("packages/spectra-agent/src/eval/graders.rs")
    for term in [
        "fn approval(",
        "fn refusal(",
        "fn tool_set(",
        "fn budget(",
        "fn schema(",
        "fn judge_verdict(",
        'include_str!("judge_prompt.md")',
        "the judge grader is opt-in (--judge)",
    ]:
        require(term in graders, f"eval/graders.rs missing {term}")

    prompt = read("packages/spectra-agent/src/eval/judge_prompt.md")
    for term in ["{case}", "{input}", "{rubric}", "{output}", '{"pass": true']:
        require(term in prompt, f"judge_prompt.md missing {term}")

    lib = read("packages/spectra-agent/src/lib.rs")
    for term in ["mod eval;", "pub use eval::{", "run_suite", "CaseExecutor"]:
        require(term in lib, f"spectra-agent lib.rs missing {term}")

    cli = read("tools/spectra-cli/src/cli_agent_eval.rs")
    for term in [
        "--suite",
        "--baseline",
        "--repeat",
        "--judge",
        "current_exe",
        "Command::new",
        "ProcessCaseExecutor",
        "baseline.regressions",
    ]:
        require(term in cli, f"cli_agent_eval.rs missing {term}")

    parse = read("tools/spectra-cli/src/cli_parse_core.rs")
    for term in ["CliAction::AgentEval", "parse_agent_eval_invocation", "HelpTopic::AgentEval"]:
        require(term in parse, f"cli_parse_core.rs missing {term}")

    help_text = read("tools/spectra-cli/src/cli_help.rs")
    for term in ["fn print_agent_eval_help", "agent eval", "--judge"]:
        require(term in help_text, f"cli_help.rs missing {term}")

    manifest = read("tools/spectra-cli/Cargo.toml")
    require(
        "spectra-agent" in manifest,
        "the CLI must depend on spectra-agent for the eval API",
    )

    fixture = read(FIXTURE)
    for term in [
        "journal_payloads",
        "from std.io import println",
        "println(report)",
        "block_on(act(run, script))",
        "ask_json(run",
        "approve(run, WRITE_ACTION)",
        "require(run, false",
    ]:
        require(term in fixture, f"{FIXTURE} missing {term}")


def validate_documents() -> None:
    suite = json.loads(read(SUITE))
    require(suite.get("schema") == SUITE_SCHEMA, "the example suite schema tag")
    cases = suite.get("cases")
    require(isinstance(cases, list) and len(cases) == 4, "the example suite has four cases")
    require(
        [case.get("input") for case in cases] == INPUTS,
        "the example suite must cover approval, refusal, budget and schema",
    )
    names = [case["name"] for case in cases]
    require(len(set(names)) == len(names), "case names must be unique")
    require(
        len({case["run_id"] for case in cases}) == len(cases),
        "case run ids must be unique, or two cases would share a journal",
    )
    root = (ROOT / SUITE).parent
    for case in cases:
        program = (root / case["program"]).resolve()
        require(program.is_file(), f"{case['name']} must point at a real program")
        require(
            (root / case["journal"]).resolve() == JOURNAL_DIR,
            f"{case['name']} must journal into {JOURNAL_DIR}",
        )
        require(
            case.get("graders"),
            f"{case['name']} must select its graders explicitly",
        )

    baseline = json.loads(read(BASELINE))
    require(baseline.get("schema") == BASELINE_SCHEMA, "the baseline schema tag")
    recorded = {case["name"]: case for case in baseline["cases"]}
    require(
        sorted(recorded) == sorted(names),
        "the baseline must record exactly the suite's cases",
    )
    for name, case in recorded.items():
        require(
            case["passed"] == case["attempts"] and case["attempts"] >= 1,
            f"baseline case {name} must record a fully passing run",
        )


def validate_default_run() -> None:
    report, _ = eval_json()
    require(report["schema"] == REPORT_SCHEMA, "the report schema tag")
    require(report["suite"] == "agent_core.json", "the report names the suite")
    require(report["pass_at_1"] == 1.0, f"pass@1 must be 1.0, got {report['pass_at_1']}")
    require(report["pass_at_k"] == 1.0, f"pass@k must be 1.0, got {report['pass_at_k']}")
    repeat_by_name = {}
    for case in report["cases"]:
        repeat_by_name[case["name"]] = case["repeat"]
        require(
            case["passed"] == case["repeat"],
            f"case {case['name']} must pass every attempt",
        )
        for attempt in case["attempts"]:
            require(attempt["passed"], f"{case['name']} attempt {attempt['run_id']} failed")
            require(attempt["exit_code"] == 0, f"{attempt['run_id']} exit code")
            grader_names = {outcome["grader"] for outcome in attempt["graders"]}
            require(
                "judge" not in grader_names,
                "the example suite must not select the judge grader",
            )
    require(repeat_by_name["schema-conformance"] == 3, "the schema case repeats three times")
    require(report["cost"]["tokens_in"] > 0, "the report must account for input tokens")
    require(report["cost"]["cost_micros"] > 0, "the report must account for cost")
    require(report["judge"]["enabled"] is False, "the judge must be off by default")
    require(report["judge"]["cases"] == [], "no case should list the judge by default")
    require(report["baseline"]["written"] is False, "the baseline already exists")
    require(report["baseline"]["regressions"] == [], "the checked-in baseline must not regress")

    # One fresh journal file per attempt: 2 + 2 + 2 + 3.
    files = sorted(JOURNAL_DIR.glob("eval-378-*.jsonl"))
    require(len(files) == 9, f"expected 9 attempt journals, found {len(files)}")
    for path in files:
        records = [
            json.loads(line)
            for line in path.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
        require(records, f"{path.name} must not be empty")
        require(
            records[0]["step"] == 0,
            f"{path.name} must start at step 0: the runner clears the attempt's journal",
        )

    # `--repeat` replaces every case's own count.
    overridden, _ = eval_json("--repeat", "1")
    for case in overridden["cases"]:
        require(case["repeat"] == 1, f"case {case['name']} repeat override")
    require(overridden["pass_at_k"] == 1.0, "the overridden run must still pass")

    # The human output reports the same gate.
    human = run([SPECTRALANG, "agent", "eval", "--suite", SUITE], expect=0).stdout
    require("pass@1 =" in human and "pass@k =" in human, f"human output:\n{human}")
    require("no regressions" in human, f"human output:\n{human}")


def validate_judge_is_opt_in() -> None:
    # A suite without a judge block cannot be asked to judge.
    refused = run(
        [SPECTRALANG, "agent", "eval", "--suite", SUITE, "--judge"],
        expect=64,
    )
    require("judge" in refused.stderr, f"--judge refusal:\n{refused.stderr}")

    scratch = SCRATCH / "judge"
    if scratch.exists():
        shutil.rmtree(scratch)
    journal = JOURNAL_DIR / "judge"
    schema_case = {
        "name": "judged-schema",
        "input": "schema",
        "graders": ["schema", "judge"],
        "expectations": {
            "output_schema": {"type": "object"},
            "judge": {"rubric": "the output is a JSON object"},
        },
        "run_id": "eval-judge-off",
    }
    suite_path = scratch / "judge-off.json"
    write_suite(suite_path, fixture_suite(journal, [dict(schema_case)]))
    completed = run(
        [
            SPECTRALANG,
            "agent",
            "eval",
            "--json",
            "--suite",
            suite_path,
            "--baseline",
            scratch / "judge-off.baseline.json",
        ],
        expect=0,
    )
    report = json.loads(completed.stdout)
    require(
        report["judge"]["cases"] == ["judged-schema"],
        "the report must name the case that listed the judge",
    )
    attempt = report["cases"][0]["attempts"][0]
    judged = [outcome for outcome in attempt["graders"] if outcome["grader"] == "judge"]
    require(len(judged) == 1, "the judge outcome must be reported")
    require(judged[0]["skipped"] is True, "the judge must be skipped while disabled")
    require(judged[0]["passed"] is False, "a skipped judge is not a pass")
    require(
        "opt-in" in judged[0]["detail"],
        f"the skip must name the opt-in rule: {judged[0]['detail']}",
    )
    require(attempt["passed"] is True, "a skipped judge must not fail the attempt")

    # With a judge block and `--judge`, the grader runs — against the mock, so
    # no network is involved and the verdict is parsed strictly.
    enabled_case = dict(schema_case, run_id="eval-judge-on")
    enabled_path = scratch / "judge-on.json"
    write_suite(
        enabled_path,
        fixture_suite(
            journal,
            [enabled_case],
            judge={"model": "mock/echo", "endpoint": "mock:"},
        ),
    )
    completed = run(
        [
            SPECTRALANG,
            "agent",
            "eval",
            "--json",
            "--suite",
            enabled_path,
            "--judge",
            "--baseline",
            scratch / "judge-on.baseline.json",
        ],
        expect=0,
    )
    report = json.loads(completed.stdout)
    require(report["judge"]["enabled"] is True, "the judge must report as enabled")
    judged = [
        outcome
        for outcome in report["cases"][0]["attempts"][0]["graders"]
        if outcome["grader"] == "judge"
    ]
    require(judged[0]["skipped"] is False, "an enabled judge must actually run")
    require(
        "judge" in judged[0]["detail"],
        f"the judge outcome must report why it failed: {judged[0]['detail']}",
    )
    require(
        judged[0]["passed"] is False,
        "the mock's echo is not a verdict, so the judge must fail closed",
    )


def validate_regression_gate() -> None:
    scratch = SCRATCH / "regression"
    if scratch.exists():
        shutil.rmtree(scratch)
    scratch.mkdir(parents=True)
    journal = JOURNAL_DIR / "regression"
    # The schema scenario never asks for an approval, so this expectation can
    # never hold: the case fails deterministically.
    case = {
        "name": "never-satisfied",
        "input": "schema",
        "run_id": "eval-regression",
        "graders": ["approval"],
        "expectations": {"approval_required": ["spectra.std.fs.fs_write"]},
    }
    suite_path = scratch / "suite.json"
    write_suite(suite_path, fixture_suite(journal, [case]))
    baseline_path = scratch / "baseline.json"
    arguments = [
        SPECTRALANG,
        "agent",
        "eval",
        "--json",
        "--suite",
        suite_path,
        "--baseline",
        baseline_path,
    ]

    # No baseline: the first run writes one and exits 0 even though the case
    # fails — a brand-new suite has nothing to regress against.
    first = json.loads(run(arguments, expect=0).stdout)
    require(first["baseline"]["written"] is True, "the first run must write a baseline")
    require(first["pass_at_1"] == 0.0, "the case must fail deterministically")
    require(baseline_path.exists(), "the baseline file must exist after the first run")

    # Same result again: a case that was already failing is not a regression.
    second = json.loads(run(arguments, expect=0).stdout)
    require(second["baseline"]["written"] is False, "the baseline must be reused")
    require(second["baseline"]["regressions"] == [], "an already-failing case must not gate")

    # Pretend the case used to pass: now it is a regression and the command
    # exits 65 after printing its report.
    document = json.loads(baseline_path.read_text(encoding="utf-8"))
    document["cases"][0]["passed"] = document["cases"][0]["attempts"]
    baseline_path.write_text(json.dumps(document, indent=2), encoding="utf-8")
    regressed = run(arguments, expect=65)
    report = json.loads(regressed.stdout)
    require(
        [entry["case"] for entry in report["baseline"]["regressions"]] == ["never-satisfied"],
        "the report must name the regressed case",
    )
    require("regressed" in regressed.stderr, f"stderr:\n{regressed.stderr}")

    # And the non-JSON path reports it too.
    human = run([arg for arg in arguments if arg != "--json"], expect=65).stdout
    require("regression: never-satisfied" in human, f"human output:\n{human}")


def validate_fixture() -> None:
    # JIT: every scenario on its own, with args, exit 0.
    for scenario in INPUTS:
        run_id = f"r3220-jit-{scenario}"
        completed = run(
            [SPECTRALANG, "run", FIXTURE, "--", scenario, run_id, JOURNAL_DIR.as_posix()],
            expect=0,
        )
        report = json.loads(completed.stdout.strip().splitlines()[-1])
        require("status" in report, f"the {scenario} run must print its report")
        require(report["replay"] is False, f"the {scenario} run must not replay")

    approval = journal_records("r3220-jit-approval")
    kinds = [record["kind"] for record in approval]
    require("approval" in kinds, f"the approval scenario must journal the decision: {kinds}")
    decision = next(record for record in approval if record["kind"] == "approval")
    require(decision["output"] == "false", "the approval decision must be a deny")
    require(
        "spectra.std.fs.fs_write" in (decision.get("input") or ""),
        "the approval payload must name the action (journal_payloads)",
    )
    tool = [record for record in approval if record["kind"] == "tool"]
    require(len(tool) == 1, f"the approval scenario dispatches exactly one tool: {kinds}")
    require("add" in (tool[0].get("input") or ""), "the tool payload must name the tool")

    refusal = journal_records("r3220-jit-refusal")
    require(
        not [record for record in refusal if record["kind"] == "tool"],
        "a refusal must not dispatch a tool",
    )
    assertion = [record for record in refusal if record["kind"] == "assertion"]
    require(len(assertion) == 1, "the refusal must be journaled as one assertion")
    require(assertion[0]["output"] == "false", "the refusal assertion must be a failure")

    budget = journal_records("r3220-jit-budget")
    require(
        len([record for record in budget if record["kind"] == "tool"]) == 1,
        "the ceiling must deny the second dispatch before it is journaled",
    )

    # The fixture's contract is enforced: missing args are refused.
    missing = run([SPECTRALANG, "run", FIXTURE], expect=None)
    require(
        missing.returncode != 0,
        "the fixture must refuse to run without the harness arguments",
    )

    # AOT parity.
    executable = ROOT / "target" / "r3220-agent-eval.exe"
    run(
        [
            SPECTRALANG,
            "compile",
            "--debug-info=none",
            "--emit-exe",
            executable,
            FIXTURE,
        ],
        expect=0,
    )
    run_id = "r3220-aot-approval"
    completed = subprocess.run(
        [str(executable), "approval", run_id, JOURNAL_DIR.as_posix()],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    require(
        completed.returncode == 0,
        f"AOT binary must return 0, got {completed.returncode}:\n{completed.stdout}",
    )
    journal_records(run_id)


def validate_behavior() -> None:
    output = run(
        [CARGO, "test", "-q", "-p", "spectra-agent", "--offline", "eval"],
        expect=0,
    ).stdout
    require("test result: ok" in output, f"spectra-agent eval tests must pass:\n{output}")


def main() -> None:
    run([CARGO, "build", "-q", "-p", "spectra-cli", "--offline"], expect=0)
    for path in (JOURNAL_DIR, SCRATCH):
        if path.exists():
            shutil.rmtree(path)
    validate_implementation()
    validate_documents()
    validate_behavior()
    validate_fixture()
    validate_default_run()
    validate_judge_is_opt_in()
    validate_regression_gate()
    shutil.rmtree(SCRATCH, ignore_errors=True)
    print("validated R-3220 evaluation harness: cases, graders, CLI gate and example suite")


if __name__ == "__main__":
    main()
