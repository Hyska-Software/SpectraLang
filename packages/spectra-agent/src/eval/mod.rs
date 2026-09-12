//! Evaluation harness: cases, a fresh-run runner, graders and `pass@k`
//! reporting (R-3220 T1/T3).
//!
//! Governance is tested deterministically (R-3214, R-3217); *behavior* is
//! evaluated statistically. Mixing the two produces suites that fail on model
//! variance and pass by luck, so this module keeps the instruments apart:
//! every default grader reads the run's own journal and report, and the only
//! model-graded instrument is the opt-in judge.
//!
//! # Case execution contract
//!
//! A case names a Spectra program and an input. The runner executes the
//! program once per attempt in a **fresh process** with
//!
//! ```text
//! argv[1] = input
//! argv[2] = run id
//! argv[3] = journal directory
//! ```
//!
//! (argv[0] is the program path, as `spectralang run` forwards it). The
//! program must:
//!
//! 1. perform its whole scenario in one `agent_start`/`agent_end` run using
//!    the given run id and journal directory, with `journal_payloads` enabled
//!    so the governed actions and tool names are readable from the journal;
//! 2. delete nothing after the run — the journal is the record the harness
//!    grades; and
//! 3. print the `agent_end` report as the **last non-empty line of stdout**.
//!
//! The runner grades `<journal>/<run_id>.jsonl` plus that report. Exit status
//! 0 means "the scenario ran"; the graders decide whether it behaved. A stale
//! journal for the attempt's run id is removed first, so every attempt is a
//! fresh run and can never replay a previous attempt.
//!
//! A fresh process per attempt is deliberate. The runtime's program-argument
//! store is set once per process (`spectra_runtime::set_program_args`), the
//! agent run handles and the policy hook are process-global, and the run
//! report is a value inside the program — all three make a genuinely fresh run
//! the honest reading of "each case in a fresh run", and they keep one
//! crashing case from contaminating the next.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{json, Value};

use crate::journal::Record;

mod graders;

pub use graders::GraderOutcome;
pub(crate) use graders::JudgeGrader;

use graders::evaluate;

/// JSON schema tag of a suite document.
pub const SUITE_SCHEMA: &str = "spectralang.agent-eval-suite.v1";
/// JSON schema tag of the CLI report.
pub const REPORT_SCHEMA: &str = "spectralang.agent-eval.v1";
/// JSON schema tag of a checked-in baseline.
pub const BASELINE_SCHEMA: &str = "spectralang.agent-eval-baseline.v1";

/// One grader a case can select.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GraderKind {
    Approval,
    Refusal,
    ToolSet,
    Budget,
    Schema,
    Judge,
}

impl GraderKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Approval => "approval",
            Self::Refusal => "refusal",
            Self::ToolSet => "tool_set",
            Self::Budget => "budget",
            Self::Schema => "schema",
            Self::Judge => "judge",
        }
    }

    /// The deterministic graders: the default set for a case that declares no
    /// `graders`, and the only graders the default CI path ever runs.
    pub fn deterministic() -> Vec<Self> {
        vec![
            Self::Approval,
            Self::Refusal,
            Self::ToolSet,
            Self::Budget,
            Self::Schema,
        ]
    }

    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "approval" => Self::Approval,
            "refusal" => Self::Refusal,
            "tool_set" | "tool-set" => Self::ToolSet,
            "budget" => Self::Budget,
            "schema" => Self::Schema,
            "judge" => Self::Judge,
            _ => return None,
        })
    }
}

/// Budget expectations, all optional and all measured against the report's
/// own accounting.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BudgetExpectation {
    pub max_tokens: Option<u64>,
    pub max_cost_micros: Option<u64>,
    pub max_tool_calls: Option<u64>,
    pub status: Option<String>,
    pub exceeded: Option<bool>,
}

/// Refusal expectation: the run declined `action` and none of
/// `forbidden_tools` ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusalExpectation {
    pub expected: bool,
    pub action: Option<String>,
    pub forbidden_tools: Vec<String>,
}

/// Everything a case asserts about a run. Every field is optional: a case that
/// omits an expectation makes no claim, and the grader for that expectation
/// passes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Expectations {
    pub approval_required: Vec<String>,
    pub approval_denied: Vec<String>,
    pub approval_allowed: Vec<String>,
    pub refusal: Option<RefusalExpectation>,
    pub tool_subset: Option<Vec<String>>,
    pub tool_required: Vec<String>,
    pub output_schema: Option<String>,
    pub budget: Option<BudgetExpectation>,
    pub judge_rubric: Option<String>,
}

/// One evaluation case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalCase {
    pub name: String,
    /// Absolute program path (resolved against the suite's directory).
    pub program: PathBuf,
    pub input: String,
    /// Base run id; attempt N runs as `<run_id>-<N>`.
    pub run_id: String,
    /// Absolute journal directory (resolved against the suite's directory).
    pub journal: PathBuf,
    pub repeat: usize,
    pub graders: Vec<GraderKind>,
    pub expectations: Expectations,
}

impl EvalCase {
    /// Test-only constructor with an empty program and the given graders.
    #[cfg(test)]
    pub(crate) fn for_test(name: &str, graders: Vec<GraderKind>) -> Self {
        Self {
            name: name.to_string(),
            program: PathBuf::new(),
            input: String::new(),
            run_id: "test".to_string(),
            journal: PathBuf::new(),
            repeat: 1,
            graders,
            expectations: Expectations::default(),
        }
    }
}

/// A loaded suite. `root` is the suite file's directory; `program` and
/// `journal` are resolved against it, so a suite behaves the same whatever
/// directory the CLI is invoked from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalSuite {
    path: PathBuf,
    root: PathBuf,
    repeat: usize,
    judge: Option<JudgeConfig>,
    cases: Vec<EvalCase>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JudgeConfig {
    model: String,
    endpoint: String,
}

impl EvalSuite {
    /// Loads and validates a suite. Unknown keys anywhere are rejected: a typo
    /// in an expectation is exactly the failure a governance harness must not
    /// hide behind a silent default.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read suite '{}': {error}", path.display()))?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|error| format!("suite '{}' is not valid JSON: {error}", path.display()))?;
        let object = value
            .as_object()
            .ok_or_else(|| format!("suite '{}' must be a JSON object", path.display()))?;
        reject_unknown(
            object,
            &["schema", "repeat", "judge", "cases"],
            "suite",
        )?;
        match object.get("schema").and_then(Value::as_str) {
            Some(SUITE_SCHEMA) => {}
            Some(other) => {
                return Err(format!(
                    "suite '{}' declares schema '{other}', expected '{SUITE_SCHEMA}'",
                    path.display()
                ))
            }
            None => {
                return Err(format!(
                    "suite '{}' is missing the '{SUITE_SCHEMA}' schema tag",
                    path.display()
                ))
            }
        }
        let repeat = optional_positive(object, "repeat")?.unwrap_or(1);
        let judge = match object.get("judge") {
            None => None,
            Some(value) => {
                let judge = value.as_object().ok_or_else(|| {
                    "the suite's 'judge' field must be an object".to_string()
                })?;
                reject_unknown(judge, &["model", "endpoint"], "suite.judge")?;
                Some(JudgeConfig {
                    model: required_string(judge, "model", "suite.judge")?,
                    endpoint: optional_string(judge, "endpoint", "suite.judge")?
                        .unwrap_or_default(),
                })
            }
        };
        let cases_value = object
            .get("cases")
            .and_then(Value::as_array)
            .ok_or_else(|| "the suite must declare a 'cases' array".to_string())?;
        if cases_value.is_empty() {
            return Err("the suite must declare at least one case".to_string());
        }
        let path = path.to_path_buf();
        let root = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let mut cases = Vec::with_capacity(cases_value.len());
        let mut seen = BTreeMap::new();
        for case_value in cases_value {
            let case = parse_case(case_value, &root, repeat)?;
            if seen.insert(case.name.clone(), ()).is_some() {
                return Err(format!("duplicate case name '{}'", case.name));
            }
            cases.push(case);
        }
        Ok(Self {
            path,
            root,
            repeat,
            judge,
            cases,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    pub fn cases(&self) -> &[EvalCase] {
        &self.cases
    }

    pub fn judge_configured(&self) -> bool {
        self.judge.is_some()
    }
}

fn parse_case(value: &Value, root: &Path, default_repeat: usize) -> Result<EvalCase, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "every case must be a JSON object".to_string())?;
    reject_unknown(
        object,
        &[
            "name",
            "program",
            "input",
            "run_id",
            "journal",
            "repeat",
            "graders",
            "expectations",
        ],
        "case",
    )?;
    let name = required_string(object, "name", "case")?;
    let program_raw = required_string(object, "program", &format!("case '{name}'"))?;
    let program = root.join(&program_raw);
    if !program.is_file() {
        return Err(format!(
            "case '{name}' names program '{}', which does not exist",
            program.display()
        ));
    }
    let input = object
        .get("input")
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("case '{name}' field 'input' must be a string"))
        })
        .transpose()?
        .unwrap_or_default();
    let run_id = required_string(object, "run_id", &format!("case '{name}'"))?;
    validate_run_id(&name, &run_id)?;
    let journal_raw = required_string(object, "journal", &format!("case '{name}'"))?;
    let journal = root.join(&journal_raw);
    let repeat = optional_positive(object, "repeat")?.unwrap_or(default_repeat);
    let graders = match object.get("graders") {
        None => GraderKind::deterministic(),
        Some(value) => {
            let array = value.as_array().ok_or_else(|| {
                format!("case '{name}' field 'graders' must be an array")
            })?;
            if array.is_empty() {
                return Err(format!("case '{name}' declares an empty 'graders' list"));
            }
            let mut graders = Vec::with_capacity(array.len());
            for entry in array {
                let raw = entry.as_str().ok_or_else(|| {
                    format!("case '{name}' grader names must be strings")
                })?;
                let grader = GraderKind::parse(raw).ok_or_else(|| {
                    format!(
                        "case '{name}' names unknown grader '{raw}' (known: approval, refusal, \
                         tool_set, budget, schema, judge)"
                    )
                })?;
                if graders.contains(&grader) {
                    return Err(format!("case '{name}' lists grader '{raw}' twice"));
                }
                graders.push(grader);
            }
            graders
        }
    };
    let expectations = match object.get("expectations") {
        None => Expectations::default(),
        Some(value) => parse_expectations(value, &name)?,
    };
    Ok(EvalCase {
        name,
        program,
        input,
        run_id,
        journal,
        repeat,
        graders,
        expectations,
    })
}

fn parse_expectations(value: &Value, case: &str) -> Result<Expectations, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("case '{case}' field 'expectations' must be an object"))?;
    reject_unknown(
        object,
        &[
            "approval_required",
            "approval_denied",
            "approval_allowed",
            "refusal",
            "tool_subset",
            "tool_required",
            "output_schema",
            "budget",
            "judge",
        ],
        &format!("case '{case}' expectations"),
    )?;
    let mut expectations = Expectations {
        approval_required: string_list(object, "approval_required", case)?,
        approval_denied: string_list(object, "approval_denied", case)?,
        approval_allowed: string_list(object, "approval_allowed", case)?,
        tool_required: string_list(object, "tool_required", case)?,
        ..Expectations::default()
    };
    if let Some(value) = object.get("tool_subset") {
        expectations.tool_subset = Some(
            value
                .as_array()
                .ok_or_else(|| format!("case '{case}' expectations.tool_subset must be an array"))?
                .iter()
                .map(|entry| {
                    entry.as_str().map(str::to_string).ok_or_else(|| {
                        format!("case '{case}' expectations.tool_subset members must be strings")
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    if let Some(value) = object.get("output_schema") {
        expectations.output_schema = Some(value.to_string());
    }
    if let Some(value) = object.get("refusal") {
        let refusal = value
            .as_object()
            .ok_or_else(|| format!("case '{case}' expectations.refusal must be an object"))?;
        reject_unknown(
            refusal,
            &["expected", "action", "forbidden_tools"],
            &format!("case '{case}' expectations.refusal"),
        )?;
        let expected = match refusal.get("expected") {
            None => true,
            Some(value) => value.as_bool().ok_or_else(|| {
                format!("case '{case}' expectations.refusal.expected must be a boolean")
            })?,
        };
        expectations.refusal = Some(RefusalExpectation {
            expected,
            action: optional_string(refusal, "action", case)?,
            forbidden_tools: string_list(refusal, "forbidden_tools", case)?,
        });
    }
    if let Some(value) = object.get("budget") {
        let budget = value
            .as_object()
            .ok_or_else(|| format!("case '{case}' expectations.budget must be an object"))?;
        reject_unknown(
            budget,
            &[
                "max_tokens",
                "max_cost_micros",
                "max_tool_calls",
                "status",
                "exceeded",
            ],
            &format!("case '{case}' expectations.budget"),
        )?;
        expectations.budget = Some(BudgetExpectation {
            max_tokens: optional_u64(budget, "max_tokens", case)?,
            max_cost_micros: optional_u64(budget, "max_cost_micros", case)?,
            max_tool_calls: optional_u64(budget, "max_tool_calls", case)?,
            status: optional_string(budget, "status", case)?,
            exceeded: match budget.get("exceeded") {
                None => None,
                Some(value) => Some(value.as_bool().ok_or_else(|| {
                    format!("case '{case}' expectations.budget.exceeded must be a boolean")
                })?),
            },
        });
    }
    if let Some(value) = object.get("judge") {
        let judge = value
            .as_object()
            .ok_or_else(|| format!("case '{case}' expectations.judge must be an object"))?;
        reject_unknown(
            judge,
            &["rubric"],
            &format!("case '{case}' expectations.judge"),
        )?;
        expectations.judge_rubric = Some(required_string(judge, "rubric", case)?);
    }
    Ok(expectations)
}

fn validate_run_id(case: &str, run_id: &str) -> Result<(), String> {
    if run_id.len() > 96 {
        return Err(format!("case '{case}' run_id is longer than 96 characters"));
    }
    if !run_id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
    {
        return Err(format!(
            "case '{case}' run_id '{run_id}' must be filesystem-safe (letters, digits, '-', \
             '_' and '.') so the journal file name is exactly '<run_id>.jsonl'"
        ));
    }
    Ok(())
}

/// What the runner hands the executor for one attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseInvocation {
    pub case: String,
    /// 1-based attempt index within the case.
    pub attempt: usize,
    pub program: PathBuf,
    pub input: String,
    pub run_id: String,
    /// Journal directory exactly as the program must receive it: forward
    /// slashes, so a program can embed it in its spec JSON verbatim.
    pub journal: String,
}

/// What the executor observed from outside the attempt: the process outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseExecution {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Runs one case attempt. The harness owns the loop, the fresh-run bookkeeping
/// and the grading; the executor owns "how a Spectra program is launched",
/// which is the one part that needs the compiler toolchain.
pub trait CaseExecutor {
    fn execute(&mut self, invocation: &CaseInvocation) -> Result<CaseExecution, String>;
}

/// Runs every case of `suite` `repeat` times (the override wins over each
/// case's own count) and grades each attempt.
pub fn run_suite(
    suite: &EvalSuite,
    repeat_override: Option<usize>,
    judge_enabled: bool,
    executor: &mut dyn CaseExecutor,
) -> SuiteReport {
    let judge = suite
        .judge
        .as_ref()
        .filter(|_| judge_enabled)
        .map(|config| JudgeGrader::new(&config.model, &config.endpoint));
    let mut cases = Vec::with_capacity(suite.cases.len());
    let mut passed_attempts = 0usize;
    let mut total_attempts = 0usize;
    let mut tokens_in = 0u64;
    let mut tokens_out = 0u64;
    let mut cost_micros = 0u64;
    for case in &suite.cases {
        let repeat = repeat_override.unwrap_or(case.repeat).max(1);
        let mut attempts = Vec::with_capacity(repeat);
        let mut passed = 0usize;
        for attempt in 1..=repeat {
            let report = run_attempt(case, attempt, judge.as_ref(), judge_enabled, executor);
            if report.passed {
                passed += 1;
            }
            tokens_in = tokens_in.saturating_add(report.tokens_in);
            tokens_out = tokens_out.saturating_add(report.tokens_out);
            cost_micros = cost_micros.saturating_add(report.cost_micros);
            attempts.push(report);
        }
        passed_attempts += passed;
        total_attempts += repeat;
        cases.push(CaseReport {
            name: case.name.clone(),
            repeat,
            passed,
            attempts,
        });
    }
    let fully_passing = cases.iter().filter(|case| case.passed == case.repeat).count();
    let pass_at_1 = ratio(passed_attempts, total_attempts);
    let pass_at_k = ratio(fully_passing, cases.len());
    let suite_name = suite
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| suite.path.to_string_lossy().into_owned());
    SuiteReport {
        suite: suite_name,
        suite_path: suite.path.to_string_lossy().into_owned(),
        judge_enabled,
        cases,
        pass_at_1,
        pass_at_k,
        tokens_in,
        tokens_out,
        cost_micros,
    }
}

fn run_attempt(
    case: &EvalCase,
    attempt: usize,
    judge: Option<&JudgeGrader>,
    judge_enabled: bool,
    executor: &mut dyn CaseExecutor,
) -> AttemptReport {
    let run_id = format!("{}-{attempt}", case.run_id);
    // A fresh run cannot replay: the attempt's journal file is removed before
    // the program starts, so the run always begins at step 0.
    let journal_file = crate::journal::file_path(&case.journal, &run_id);
    if let Err(error) = std::fs::remove_file(&journal_file) {
        if error.kind() != std::io::ErrorKind::NotFound {
            return AttemptReport::failed(
                &run_id,
                format!("cannot clear the previous journal {}: {error}", journal_file.display()),
            );
        }
    }
    let invocation = CaseInvocation {
        case: case.name.clone(),
        attempt,
        program: case.program.clone(),
        input: case.input.clone(),
        run_id: run_id.clone(),
        journal: journal_argument(&case.journal),
    };
    let started = Instant::now();
    let execution = match executor.execute(&invocation) {
        Ok(execution) => execution,
        Err(error) => {
            return AttemptReport::failed(&run_id, error);
        }
    };
    let observation = Observation::new(&execution, &case.journal, &run_id);
    let outcomes = evaluate(case, &observation, judge, judge_enabled);
    let passed = observation.error.is_none()
        && outcomes
            .iter()
            .all(|outcome| outcome.passed || outcome.skipped);
    let report = observation.report.clone();
    AttemptReport {
        run_id,
        passed,
        duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        exit_code: Some(execution.exit_code),
        status: report.as_ref().map(|report| report.status.clone()),
        tool_calls: report.as_ref().map(|report| report.tool_calls).unwrap_or(0),
        tokens_in: report.as_ref().map(|report| report.tokens_in).unwrap_or(0),
        tokens_out: report.as_ref().map(|report| report.tokens_out).unwrap_or(0),
        cost_micros: report.as_ref().map(|report| report.cost_micros).unwrap_or(0),
        error: observation.error.clone(),
        stdout_tail: tail(&execution.stdout, 400),
        stderr_tail: tail(&execution.stderr, 400),
        graders: outcomes,
    }
}

/// The journal directory in the form a program can embed in its spec JSON.
fn journal_argument(path: &Path) -> String {
    let text = path.to_string_lossy();
    if std::path::MAIN_SEPARATOR == '\\' {
        text.replace('\\', "/")
    } else {
        text.into_owned()
    }
}

/// Everything the harness could observe about one attempt.
#[derive(Debug, Clone, Default)]
pub(crate) struct Observation {
    report: Option<RunReport>,
    records: Vec<Record>,
    final_output: Option<String>,
    tool_calls: Vec<ToolObservation>,
    approvals: Vec<ApprovalObservation>,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolObservation {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApprovalObservation {
    pub action: Option<String>,
    pub allowed: bool,
    pub attribution: Option<String>,
}

impl Observation {
    fn new(execution: &CaseExecution, journal: &Path, run_id: &str) -> Self {
        let mut observation = Self::default();
        match crate::journal::read_records(journal, run_id) {
            Ok(records) => observation.records = records,
            Err(error) => observation.error = Some(error.to_string()),
        }
        observation.report = last_report(&execution.stdout);
        observation.final_output = final_output(&observation.records);
        observation.tool_calls = tool_observations(&observation.records);
        observation.approvals = approval_observations(&observation.records);
        if observation.error.is_none() {
            if execution.exit_code != 0 {
                observation.error = Some(format!(
                    "the eval program exited with status {}{}",
                    execution.exit_code,
                    stderr_hint(&execution.stderr)
                ));
            } else if observation.report.is_none() {
                observation.error = Some(
                    "the eval program printed no run report (the last non-empty stdout line must \
                     be the agent_end report)"
                        .to_string(),
                );
            }
        }
        observation
    }

    pub(crate) fn report(&self) -> Option<&RunReport> {
        self.report.as_ref()
    }

    pub(crate) fn records(&self) -> &[Record] {
        &self.records
    }

    pub(crate) fn final_output(&self) -> Option<&str> {
        self.final_output.as_deref()
    }

    pub(crate) fn tool_calls(&self) -> &[ToolObservation] {
        &self.tool_calls
    }

    pub(crate) fn tool_names(&self) -> impl Iterator<Item = Option<&str>> {
        self.tool_calls.iter().map(|call| call.name.as_deref())
    }

    pub(crate) fn approvals(&self) -> &[ApprovalObservation] {
        &self.approvals
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub(crate) fn push_approval(&mut self, action: Option<String>, allowed: bool) {
        self.approvals.push(ApprovalObservation {
            action,
            allowed,
            attribution: None,
        });
    }

    #[cfg(test)]
    pub(crate) fn push_tool(&mut self, name: Option<String>) {
        self.tool_calls.push(ToolObservation {
            name,
            arguments: None,
        });
    }
}

/// The `agent_end` report, as the harness needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    pub status: String,
    pub steps: u64,
    pub tool_calls: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_micros: u64,
    pub elapsed_ms: u64,
    pub ceiling: String,
    pub replay: bool,
}

impl RunReport {
    /// Parses the run report. The last non-empty stdout line must be the JSON
    /// object `agent_end` returned; anything else is "no report", which the
    /// attempt reports as an execution failure.
    fn parse(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text.trim()).ok()?;
        let object = value.as_object()?;
        let status = object.get("status").and_then(Value::as_str)?.to_string();
        Some(Self {
            status,
            steps: number(object, "steps"),
            tool_calls: number(object, "tool_calls"),
            tokens_in: number(object, "tokens_in"),
            tokens_out: number(object, "tokens_out"),
            cost_micros: number(object, "cost_micros"),
            elapsed_ms: number(object, "elapsed_ms"),
            ceiling: object
                .get("ceiling")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            replay: object.get("replay").and_then(Value::as_bool).unwrap_or(false),
        })
    }
}

fn number(object: &serde_json::Map<String, Value>, key: &str) -> u64 {
    object.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn last_report(stdout: &str) -> Option<RunReport> {
    stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .and_then(RunReport::parse)
}

fn final_output(records: &[Record]) -> Option<String> {
    records
        .iter()
        .rev()
        .find(|record| record.kind == "model")
        .and_then(|record| serde_json::from_str::<Value>(&record.output).ok())
        .and_then(|value| {
            value
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn tool_observations(records: &[Record]) -> Vec<ToolObservation> {
    records
        .iter()
        .filter(|record| record.kind == "tool")
        .map(|record| match record.input.as_deref() {
            // `tool\0<name>\0<arguments>` is the dispatch's journal input.
            Some(input) => {
                let mut parts = input.split('\0');
                parts.next();
                ToolObservation {
                    name: parts.next().map(str::to_string),
                    arguments: parts.next().map(str::to_string),
                }
            }
            None => ToolObservation {
                name: None,
                arguments: None,
            },
        })
        .collect()
}

fn approval_observations(records: &[Record]) -> Vec<ApprovalObservation> {
    records
        .iter()
        .filter(|record| record.kind == "approval")
        .map(|record| ApprovalObservation {
            // `approve\0<goal>\0<action>` is the approval's journal input.
            action: record
                .input
                .as_deref()
                .and_then(|input| input.rsplit('\0').next())
                .filter(|action| !action.is_empty())
                .map(str::to_string),
            allowed: record.output == "true",
            attribution: record.attribution.clone(),
        })
        .collect()
}

fn stderr_hint(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("; stderr: {}", tail(trimmed, 200))
    }
}

fn tail(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        text.to_string()
    } else {
        let mut start = text.len() - limit;
        while !text.is_char_boundary(start) {
            start += 1;
        }
        format!("…{}", &text[start..])
    }
}

/// One attempt's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptReport {
    pub run_id: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub exit_code: Option<i32>,
    pub status: Option<String>,
    pub tool_calls: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_micros: u64,
    pub error: Option<String>,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub graders: Vec<GraderOutcome>,
}

impl AttemptReport {
    fn failed(run_id: &str, error: String) -> Self {
        Self {
            run_id: run_id.to_string(),
            passed: false,
            duration_ms: 0,
            exit_code: None,
            status: None,
            tool_calls: 0,
            tokens_in: 0,
            tokens_out: 0,
            cost_micros: 0,
            error: Some(error),
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            graders: Vec::new(),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "run_id": self.run_id,
            "passed": self.passed,
            "duration_ms": self.duration_ms,
            "exit_code": self.exit_code,
            "status": self.status,
            "tool_calls": self.tool_calls,
            "tokens_in": self.tokens_in,
            "tokens_out": self.tokens_out,
            "cost_micros": self.cost_micros,
            "error": self.error,
            "stdout_tail": self.stdout_tail,
            "stderr_tail": self.stderr_tail,
            "graders": self
                .graders
                .iter()
                .map(GraderOutcome::to_json)
                .collect::<Vec<_>>(),
        })
    }
}

/// One case's results across its repeats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseReport {
    pub name: String,
    pub repeat: usize,
    pub passed: usize,
    pub attempts: Vec<AttemptReport>,
}

impl CaseReport {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "repeat": self.repeat,
            "passed": self.passed,
            "attempts": self
                .attempts
                .iter()
                .map(AttemptReport::to_json)
                .collect::<Vec<_>>(),
        })
    }
}

/// The whole suite's results.
#[derive(Debug, Clone, PartialEq)]
pub struct SuiteReport {
    pub suite: String,
    pub suite_path: String,
    pub judge_enabled: bool,
    pub cases: Vec<CaseReport>,
    /// Fraction of attempts that passed.
    pub pass_at_1: f64,
    /// Fraction of cases every one of whose attempts passed.
    pub pass_at_k: f64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_micros: u64,
}

impl SuiteReport {
    pub fn passed(&self) -> usize {
        self.cases.iter().filter(|case| case.passed == case.repeat).count()
    }

    /// The CLI report, plus the baseline verdict the CLI computed.
    pub fn to_json(&self, baseline: &BaselineStatus) -> Value {
        json!({
            "schema": REPORT_SCHEMA,
            "suite": self.suite,
            "suite_path": self.suite_path,
            "judge": {
                "enabled": self.judge_enabled,
                "cases": self
                    .cases
                    .iter()
                    .filter(|case| {
                        case.attempts.iter().any(|attempt| {
                            attempt.graders.iter().any(|outcome| {
                                outcome.grader == GraderKind::Judge && outcome.skipped
                            })
                        })
                    })
                    .map(|case| case.name.clone())
                    .collect::<Vec<_>>(),
            },
            "cases": self.cases.iter().map(CaseReport::to_json).collect::<Vec<_>>(),
            "pass_at_1": round6(self.pass_at_1),
            "pass_at_k": round6(self.pass_at_k),
            "cost": {
                "tokens_in": self.tokens_in,
                "tokens_out": self.tokens_out,
                "cost_micros": self.cost_micros,
            },
            "baseline": baseline.to_json(),
        })
    }
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

/// One case's recorded result in a baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CaseBaseline {
    name: String,
    passed: usize,
    attempts: usize,
}

/// A checked-in baseline: what each case achieved when it was last accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baseline {
    suite: String,
    cases: Vec<CaseBaseline>,
}

impl Baseline {
    /// The default baseline path for a suite: `agent_core.json` ->
    /// `agent_core.baseline.json`, next to the suite.
    pub fn path_for(suite_path: &Path) -> PathBuf {
        let stem = suite_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "suite".to_string());
        suite_path.with_file_name(format!("{stem}.baseline.json"))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read baseline '{}': {error}", path.display()))?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|error| format!("baseline '{}' is not valid JSON: {error}", path.display()))?;
        let object = value
            .as_object()
            .ok_or_else(|| format!("baseline '{}' must be a JSON object", path.display()))?;
        reject_unknown(object, &["schema", "suite", "cases"], "baseline")?;
        match object.get("schema").and_then(Value::as_str) {
            Some(BASELINE_SCHEMA) => {}
            other => {
                return Err(format!(
                    "baseline '{}' declares schema {other:?}, expected '{BASELINE_SCHEMA}'",
                    path.display()
                ))
            }
        }
        let suite = object
            .get("suite")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let entries = object
            .get("cases")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("baseline '{}' has no 'cases' array", path.display()))?;
        let mut cases = Vec::with_capacity(entries.len());
        for entry in entries {
            let case = entry.as_object().ok_or_else(|| {
                format!("baseline '{}' case entries must be objects", path.display())
            })?;
            reject_unknown(case, &["name", "passed", "attempts"], "baseline.case")?;
            let name = case
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| "baseline case entry has no 'name'".to_string())?
                .to_string();
            let passed = case.get("passed").and_then(Value::as_u64).unwrap_or(0) as usize;
            let attempts = case.get("attempts").and_then(Value::as_u64).unwrap_or(0) as usize;
            cases.push(CaseBaseline {
                name,
                passed,
                attempts,
            });
        }
        Ok(Self { suite, cases })
    }

    pub fn from_report(report: &SuiteReport) -> Self {
        Self {
            suite: report.suite.clone(),
            cases: report
                .cases
                .iter()
                .map(|case| CaseBaseline {
                    name: case.name.clone(),
                    passed: case.passed,
                    attempts: case.repeat,
                })
                .collect(),
        }
    }

    /// The regression gate: a case that fully passed in the baseline and no
    /// longer does. A case that was already failing is not a regression —
    /// otherwise a known-flaky case would gate every build forever.
    pub fn regressions(&self, report: &SuiteReport) -> Vec<Regression> {
        let recorded: BTreeMap<&str, &CaseBaseline> = self
            .cases
            .iter()
            .map(|case| (case.name.as_str(), case))
            .collect();
        report
            .cases
            .iter()
            .filter_map(|case| {
                let baseline = recorded.get(case.name.as_str())?;
                if baseline.attempts == 0 || baseline.passed < baseline.attempts {
                    return None;
                }
                if case.passed == case.repeat {
                    return None;
                }
                Some(Regression {
                    case: case.name.clone(),
                    baseline_passed: baseline.passed,
                    baseline_attempts: baseline.attempts,
                    passed: case.passed,
                    attempts: case.repeat,
                })
            })
            .collect()
    }

    pub fn to_pretty_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&json!({
            "schema": BASELINE_SCHEMA,
            "suite": self.suite,
            "cases": self
                .cases
                .iter()
                .map(|case| {
                    json!({
                        "name": case.name,
                        "passed": case.passed,
                        "attempts": case.attempts,
                    })
                })
                .collect::<Vec<_>>(),
        }))
        .map_err(|error| format!("cannot serialize the baseline: {error}"))
    }
}

/// One case that passed in the baseline and does not pass now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Regression {
    pub case: String,
    pub baseline_passed: usize,
    pub baseline_attempts: usize,
    pub passed: usize,
    pub attempts: usize,
}

impl Regression {
    pub fn to_json(&self) -> Value {
        json!({
            "case": self.case,
            "baseline": format!("{}/{}", self.baseline_passed, self.baseline_attempts),
            "current": format!("{}/{}", self.passed, self.attempts),
        })
    }
}

/// What the CLI did with the baseline: read it, or wrote the first one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineStatus {
    pub path: String,
    pub written: bool,
    pub regressions: Vec<Regression>,
}

impl BaselineStatus {
    pub fn to_json(&self) -> Value {
        json!({
            "path": self.path,
            "written": self.written,
            "regressions": self
                .regressions
                .iter()
                .map(Regression::to_json)
                .collect::<Vec<_>>(),
        })
    }
}

fn reject_unknown(
    object: &serde_json::Map<String, Value>,
    known: &[&str],
    context: &str,
) -> Result<(), String> {
    for key in object.keys() {
        if !known.contains(&key.as_str()) {
            return Err(format!(
                "{context} has unknown field '{key}' (known: {})",
                known.join(", ")
            ));
        }
    }
    Ok(())
}

fn required_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<String, String> {
    match object.get(key) {
        Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        Some(Value::String(_)) => Err(format!("{context} field '{key}' must not be empty")),
        Some(_) => Err(format!("{context} field '{key}' must be a string")),
        None => Err(format!("{context} field '{key}' is required")),
    }
}

fn optional_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<Option<String>, String> {
    match object.get(key) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(format!("{context} field '{key}' must be a string")),
    }
}

fn string_list(
    object: &serde_json::Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<Vec<String>, String> {
    match object.get(key) {
        None => Ok(Vec::new()),
        Some(value) => value
            .as_array()
            .ok_or_else(|| format!("case '{context}' field '{key}' must be an array"))?
            .iter()
            .map(|entry| {
                entry.as_str().map(str::to_string).ok_or_else(|| {
                    format!("case '{context}' field '{key}' members must be strings")
                })
            })
            .collect(),
    }
}

fn optional_u64(
    object: &serde_json::Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<Option<u64>, String> {
    match object.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("case '{context}' field '{key}' must be a non-negative integer")),
    }
}

fn optional_positive(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<usize>, String> {
    match object.get(key) {
        None => Ok(None),
        Some(value) => match value.as_u64() {
            Some(count) if count >= 1 => Ok(Some(count as usize)),
            _ => Err(format!("field '{key}' must be a positive integer")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_suite(dir: &Path, program: &Path, body: &str) -> PathBuf {
        let path = dir.join("suite.json");
        // JSON needs the Windows separators escaped.
        let program = program.to_string_lossy().replace('\\', "\\\\");
        let document = body.replace("{program}", &program);
        std::fs::write(&path, document).expect("suite file");
        path
    }

    /// An executor that writes the journal a real program would have written:
    /// one approval denial and one tool call, plus a run report on stdout.
    struct FakeExecutor {
        records: String,
        report: String,
    }

    impl CaseExecutor for FakeExecutor {
        fn execute(&mut self, invocation: &CaseInvocation) -> Result<CaseExecution, String> {
            let journal = PathBuf::from(&invocation.journal);
            std::fs::create_dir_all(&journal).expect("journal dir");
            let path = crate::journal::file_path(&journal, &invocation.run_id);
            std::fs::write(&path, &self.records).expect("journal file");
            Ok(CaseExecution {
                exit_code: 0,
                stdout: format!("noise\n{}\n", self.report),
                stderr: String::new(),
            })
        }
    }

    fn record(step: u64, kind: &str, input: Option<&str>, output: &str) -> String {
        let mut object = json!({
            "run": "run",
            "step": step,
            "kind": kind,
            "input_digest": "a".repeat(64),
            "output_digest": "b".repeat(64),
            "idempotency_key": "c".repeat(64),
            "seed": 3,
            "usage": {"tokens_in": 1, "tokens_out": 2, "cost_micros": 5},
            "timestamp": 1,
            "output": output,
        });
        if let Some(input) = input {
            object["input"] = json!(input);
        }
        object.to_string()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "spectra-eval-{tag}-{}-{}",
            std::process::id(),
            crate::journal::new_run_id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn a_case_runs_in_a_fresh_run_and_scores_one() {
        let dir = temp_dir("pass");
        let program = dir.join("program.spectra");
        std::fs::write(&program, "// fixture").expect("program");
        let path = write_suite(
            &dir,
            &program,
            r#"{
              "schema": "spectralang.agent-eval-suite.v1",
              "cases": [{
                "name": "approval-and-tools",
                "program": "{program}",
                "input": "spectra:final=done",
                "run_id": "eval-pass",
                "journal": "journal",
                "repeat": 2,
                "graders": ["approval", "tool_set", "budget"],
                "expectations": {
                  "approval_required": ["spectra.std.fs.fs_write"],
                  "approval_denied": ["spectra.std.fs.fs_write"],
                  "tool_subset": ["add"],
                  "tool_required": ["add"],
                  "budget": {"max_tool_calls": 1, "status": "completed"}
                }
              }]
            }"#,
        );
        let suite = EvalSuite::load(&path).expect("suite");
        let mut executor = FakeExecutor {
            records: format!(
                "{}\n{}\n",
                record(0, "approval", Some("approve\0goal\0spectra.std.fs.fs_write"), "false"),
                record(1, "tool", Some("tool\0add\0{\"a\":1,\"b\":2}"), "3"),
            ),
            report: r#"{"status":"completed","steps":1,"tool_calls":1,"tokens_in":1,"tokens_out":2,"cost_micros":5,"elapsed_ms":1,"ceiling":"","compensations_pending":0,"replay":false}"#.to_string(),
        };
        let report = run_suite(&suite, None, false, &mut executor);
        assert_eq!(report.cases.len(), 1);
        assert_eq!(report.cases[0].repeat, 2);
        assert_eq!(report.cases[0].passed, 2);
        assert_eq!(report.pass_at_1, 1.0);
        assert_eq!(report.pass_at_k, 1.0);
        assert_eq!(report.tokens_in, 2);
        assert_eq!(report.cost_micros, 10);
        // Each attempt used its own run id, so the two attempts could not
        // replay one another's journal.
        assert_eq!(report.cases[0].attempts[0].run_id, "eval-pass-1");
        assert_eq!(report.cases[0].attempts[1].run_id, "eval-pass-2");
    }

    #[test]
    fn the_attempt_journal_is_cleared_before_each_run() {
        let dir = temp_dir("fresh");
        let program = dir.join("program.spectra");
        std::fs::write(&program, "// fixture").expect("program");
        let path = write_suite(
            &dir,
            &program,
            r#"{
              "schema": "spectralang.agent-eval-suite.v1",
              "cases": [{
                "name": "fresh",
                "program": "{program}",
                "input": "",
                "run_id": "eval-fresh",
                "journal": "journal",
                "graders": ["budget"],
                "expectations": {"budget": {"status": "completed"}}
              }]
            }"#,
        );
        let suite = EvalSuite::load(&path).expect("suite");
        let stale = crate::journal::file_path(&dir.join("journal"), "eval-fresh-1");
        std::fs::create_dir_all(dir.join("journal")).expect("journal dir");
        std::fs::write(
            &stale,
            record(0, "model", None, "{\"text\":\"stale\"}"),
        )
        .expect("stale journal");
        let mut executor = FakeExecutor {
            records: String::new(),
            report: r#"{"status":"completed","tool_calls":0,"tokens_in":0,"tokens_out":0,"cost_micros":0}"#
                .to_string(),
        };
        let report = run_suite(&suite, None, false, &mut executor);
        assert!(report.cases[0].attempts[0].passed);
        // The stale record is gone, so the attempt did not replay it.
        let contents = std::fs::read_to_string(&stale).expect("journal exists");
        assert!(contents.is_empty());
    }

    #[test]
    fn a_missing_report_fails_the_attempt() {
        let dir = temp_dir("noreport");
        let program = dir.join("program.spectra");
        std::fs::write(&program, "// fixture").expect("program");
        let path = write_suite(
            &dir,
            &program,
            r#"{
              "schema": "spectralang.agent-eval-suite.v1",
              "cases": [{
                "name": "silent",
                "program": "{program}",
                "input": "",
                "run_id": "eval-silent",
                "journal": "journal"
              }]
            }"#,
        );
        let suite = EvalSuite::load(&path).expect("suite");
        struct Silent;
        impl CaseExecutor for Silent {
            fn execute(&mut self, _invocation: &CaseInvocation) -> Result<CaseExecution, String> {
                Ok(CaseExecution {
                    exit_code: 0,
                    stdout: "just some output".to_string(),
                    stderr: String::new(),
                })
            }
        }
        let report = run_suite(&suite, None, false, &mut Silent);
        assert_eq!(report.cases[0].passed, 0);
        assert!(report.cases[0].attempts[0]
            .error
            .as_deref()
            .expect("error")
            .contains("no run report"));
        assert_eq!(report.pass_at_1, 0.0);
        assert_eq!(report.pass_at_k, 0.0);
    }

    #[test]
    fn a_nonzero_exit_fails_the_attempt_even_when_the_graders_pass() {
        let dir = temp_dir("exit");
        let program = dir.join("program.spectra");
        std::fs::write(&program, "// fixture").expect("program");
        let path = write_suite(
            &dir,
            &program,
            r#"{
              "schema": "spectralang.agent-eval-suite.v1",
              "cases": [{
                "name": "broken",
                "program": "{program}",
                "input": "",
                "run_id": "eval-broken",
                "journal": "journal"
              }]
            }"#,
        );
        let suite = EvalSuite::load(&path).expect("suite");
        struct Broken;
        impl CaseExecutor for Broken {
            fn execute(&mut self, _invocation: &CaseInvocation) -> Result<CaseExecution, String> {
                Ok(CaseExecution {
                    exit_code: 7,
                    stdout: "{\"status\":\"completed\"}".to_string(),
                    stderr: "boom".to_string(),
                })
            }
        }
        let report = run_suite(&suite, None, false, &mut Broken);
        assert_eq!(report.cases[0].passed, 0);
        assert!(report.cases[0].attempts[0]
            .error
            .as_deref()
            .expect("error")
            .contains("status 7"));
    }

    #[test]
    fn the_repeat_override_replaces_every_case_count() {
        let dir = temp_dir("repeat");
        let program = dir.join("program.spectra");
        std::fs::write(&program, "// fixture").expect("program");
        let path = write_suite(
            &dir,
            &program,
            r#"{
              "schema": "spectralang.agent-eval-suite.v1",
              "repeat": 2,
              "cases": [
                {"name": "a", "program": "{program}", "input": "", "run_id": "eval-a", "journal": "j"},
                {"name": "b", "program": "{program}", "input": "", "run_id": "eval-b", "journal": "j", "repeat": 3}
              ]
            }"#,
        );
        let suite = EvalSuite::load(&path).expect("suite");
        let mut executor = FakeExecutor {
            records: String::new(),
            report: r#"{"status":"completed","tool_calls":0,"tokens_in":0,"tokens_out":0,"cost_micros":0}"#
                .to_string(),
        };
        let report = run_suite(&suite, Some(4), false, &mut executor);
        assert_eq!(report.cases[0].repeat, 4);
        assert_eq!(report.cases[1].repeat, 4);
        // Without the override each case keeps its own count (suite default 2,
        // case override 3).
        let report = run_suite(&suite, None, false, &mut executor);
        assert_eq!(report.cases[0].repeat, 2);
        assert_eq!(report.cases[1].repeat, 3);
    }

    #[test]
    fn unknown_fields_are_rejected_everywhere() {
        let dir = temp_dir("strict");
        let program = dir.join("program.spectra");
        std::fs::write(&program, "// fixture").expect("program");
        let base = |extra: &str| {
            format!(
                r#"{{
                  "schema": "spectralang.agent-eval-suite.v1",
                  "cases": [{{
                    "name": "a",
                    "program": "{program}",
                    "input": "",
                    "run_id": "eval-a",
                    "journal": "j"
                    {extra}
                  }}]
                }}"#,
                program = program.to_string_lossy().replace('\\', "\\\\"),
                extra = extra
            )
        };
        for extra in [
            r#","expectations": {"aprov": ["x"]}"#,
            r#","graders": ["nope"]"#,
            r#","extra": 1"#,
            r#","run_id": "bad id""#,
        ] {
            let path = dir.join("suite-strict.json");
            std::fs::write(&path, base(extra)).expect("suite");
            assert!(
                EvalSuite::load(&path).is_err(),
                "suite with {extra} must be rejected"
            );
        }
        // A missing program is caught at load rather than mid-run.
        let path = dir.join("suite-missing.json");
        std::fs::write(
            &path,
            r#"{"schema":"spectralang.agent-eval-suite.v1","cases":[{"name":"a","program":"nope.spectra","input":"","run_id":"eval-a","journal":"j"}]}"#,
        )
        .expect("suite");
        let error = EvalSuite::load(&path).expect_err("missing program");
        assert!(error.contains("does not exist"));
    }

    #[test]
    fn only_a_fully_passing_baseline_case_can_regress() {
        let dir = temp_dir("baseline");
        let program = dir.join("program.spectra");
        std::fs::write(&program, "// fixture").expect("program");
        let path = write_suite(
            &dir,
            &program,
            r#"{
              "schema": "spectralang.agent-eval-suite.v1",
              "cases": [
                {"name": "good", "program": "{program}", "input": "", "run_id": "eval-good", "journal": "j", "repeat": 2},
                {"name": "flaky", "program": "{program}", "input": "", "run_id": "eval-flaky", "journal": "j"}
              ]
            }"#,
        );
        let suite = EvalSuite::load(&path).expect("suite");
        let passing = r#"{"status":"completed","tool_calls":0,"tokens_in":0,"tokens_out":0,"cost_micros":0}"#;
        let mut executor = FakeExecutor {
            records: String::new(),
            report: passing.to_string(),
        };
        let report = run_suite(&suite, None, false, &mut executor);
        // A run that matches its own baseline never regresses, and the
        // baseline round-trips through its file unchanged.
        let recorded = Baseline::from_report(&report);
        assert!(recorded.regressions(&report).is_empty());
        assert!(recorded
            .to_pretty_json()
            .expect("baseline json")
            .contains("spectralang.agent-eval-baseline.v1"));
        let baseline_path = dir.join("baseline.json");
        std::fs::write(&baseline_path, recorded.to_pretty_json().expect("json")).expect("write");
        assert_eq!(
            Baseline::load(&baseline_path).expect("reload"),
            recorded
        );
        assert_eq!(
            Baseline::path_for(Path::new("target/suite.json")),
            Path::new("target/suite.baseline.json")
        );

        // A baseline where `good` fully passed and `flaky` was already
        // failing: only `good` can regress.
        let baseline_path = dir.join("mixed.json");
        std::fs::write(
            &baseline_path,
            r#"{
              "schema": "spectralang.agent-eval-baseline.v1",
              "suite": "suite.json",
              "cases": [
                {"name": "good", "passed": 2, "attempts": 2},
                {"name": "flaky", "passed": 0, "attempts": 1}
              ]
            }"#,
        )
        .expect("baseline");
        let baseline = Baseline::load(&baseline_path).expect("baseline");

        // The same suite with no report: `good` regresses (it was 2/2),
        // `flaky` does not (it was already 0/1).
        let mut broken = FakeExecutor {
            records: String::new(),
            report: "not a report".to_string(),
        };
        let broken_report = run_suite(&suite, None, false, &mut broken);
        let regressions = baseline.regressions(&broken_report);
        assert_eq!(regressions.len(), 1);
        assert_eq!(regressions[0].case, "good");
        assert_eq!(regressions[0].to_json()["baseline"], "2/2");
    }

    #[test]
    fn the_report_json_is_deterministic_and_carries_the_gate() {
        let dir = temp_dir("json");
        let program = dir.join("program.spectra");
        std::fs::write(&program, "// fixture").expect("program");
        let path = write_suite(
            &dir,
            &program,
            r#"{
              "schema": "spectralang.agent-eval-suite.v1",
              "judge": {"model": "gpt-judge", "endpoint": "https://judge.invalid"},
              "cases": [{
                "name": "judged",
                "program": "{program}",
                "input": "spectra:json",
                "run_id": "eval-judged",
                "journal": "j",
                "graders": ["schema", "judge"],
                "expectations": {"output_schema": {"type": "object"}, "judge": {"rubric": "is JSON"}}
              }]
            }"#,
        );
        let suite = EvalSuite::load(&path).expect("suite");
        let mut executor = FakeExecutor {
            records: record(0, "model", None, "{\"text\":\"{\\\"count\\\":3}\"}"),
            report: r#"{"status":"completed","tool_calls":0,"tokens_in":0,"tokens_out":0,"cost_micros":0}"#
                .to_string(),
        };
        let report = run_suite(&suite, None, false, &mut executor);
        let status = BaselineStatus {
            path: "baseline.json".to_string(),
            written: false,
            regressions: Vec::new(),
        };
        let first = report.to_json(&status).to_string();
        let second = report.to_json(&status).to_string();
        assert_eq!(first, second);
        let value: Value = serde_json::from_str(&first).expect("json");
        assert_eq!(value["schema"], REPORT_SCHEMA);
        assert_eq!(value["judge"]["enabled"], false);
        assert_eq!(value["judge"]["cases"][0], "judged");
        assert_eq!(value["pass_at_1"], 1.0);
        let graders = value["cases"][0]["attempts"][0]["graders"]
            .as_array()
            .expect("graders");
        assert_eq!(graders[0]["grader"], "schema");
        assert_eq!(graders[0]["passed"], true);
        assert_eq!(graders[1]["grader"], "judge");
        assert_eq!(graders[1]["skipped"], true);
    }
}
