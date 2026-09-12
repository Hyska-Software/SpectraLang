// Agent evaluation: `spectralang agent eval` (R-3220 T3).
//
// The command runs a checked-in suite with the deterministic mock provider,
// reports `pass@1`/`pass^k` and cost, and gates on a checked-in baseline. It
// never runs the judge grader unless the caller passes `--judge`, so the
// default CI path makes no model calls at all.
//
// A case is executed through the normal compile+run path by re-invoking this
// binary as `run <program> -- <input> <run_id> <journal>` in a fresh process.
// That is deliberate: the runtime's program arguments are set once per
// process, the agent run handles and policy hook are process-global, and "a
// fresh run" is then literally a fresh process — one crashing case cannot
// contaminate the next, and the attempt's run cannot replay an earlier one.

use std::process::{Command, Stdio};

use spectra_agent::{
    run_suite, Baseline, BaselineStatus, CaseExecution, CaseExecutor, CaseInvocation, EvalSuite,
    GraderKind,
};

/// The suite the command runs when `--suite` is not given.
const DEFAULT_AGENT_EVAL_SUITE: &str = "examples/agent/evals/agent_core.json";

fn parse_agent_eval_invocation<I>(
    args: &mut std::iter::Peekable<I>,
) -> CliResult<AgentEvalOptions>
where
    I: Iterator<Item = String>,
{
    let mut suite = PathBuf::from(DEFAULT_AGENT_EVAL_SUITE);
    let mut baseline: Option<PathBuf> = None;
    let mut repeat: Option<usize> = None;
    let mut json = false;
    let mut judge = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--judge" => judge = true,
            "--suite" => {
                let value = args
                    .next()
                    .ok_or_else(|| usage_error("Missing path after '--suite'."))?;
                suite = PathBuf::from(value);
            }
            "--baseline" => {
                let value = args
                    .next()
                    .ok_or_else(|| usage_error("Missing path after '--baseline'."))?;
                baseline = Some(PathBuf::from(value));
            }
            "--repeat" => {
                let value = args
                    .next()
                    .ok_or_else(|| usage_error("Missing count after '--repeat'."))?;
                let count: usize = value.parse().map_err(|_| {
                    usage_error(&format!(
                        "'--repeat' expects a positive integer, got '{}'.",
                        value
                    ))
                })?;
                if count == 0 {
                    return Err(usage_error("'--repeat' expects a positive integer (>= 1)."));
                }
                repeat = Some(count);
            }
            flag if flag.starts_with('-') => {
                return Err(usage_error(&format!("Unknown agent eval option: {}", flag)));
            }
            value => {
                // A bare path is the suite, matching `release-info [root]`.
                suite = PathBuf::from(value);
            }
        }
    }

    Ok(AgentEvalOptions {
        suite,
        baseline,
        repeat,
        json,
        judge,
    })
}

/// Runs each case by re-invoking this binary's `run` command.
struct ProcessCaseExecutor;

impl CaseExecutor for ProcessCaseExecutor {
    fn execute(&mut self, invocation: &CaseInvocation) -> Result<CaseExecution, String> {
        let binary = std::env::current_exe()
            .map_err(|error| format!("cannot locate the running CLI binary: {error}"))?;
        let output = Command::new(&binary)
            .arg("run")
            .arg(&invocation.program)
            .arg("--")
            .arg(&invocation.input)
            .arg(&invocation.run_id)
            .arg(&invocation.journal)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| {
                format!(
                    "cannot run '{}': {error}",
                    invocation.program.display()
                )
            })?;
        Ok(CaseExecution {
            // A process killed by a signal has no code; -1 can never collide
            // with a real exit status.
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn execute_agent_eval(options: AgentEvalOptions) -> CliResult<()> {
    let suite = EvalSuite::load(&options.suite).map_err(CliError::io)?;
    if options.judge && !suite.judge_configured() {
        return Err(usage_error(
            "'--judge' requires the suite to declare a 'judge' block (its model and endpoint); \
             the suite has none.",
        ));
    }

    if !options.json {
        println!(
            "Evaluating {} ({} case(s), judge {})",
            suite.path().display(),
            suite.case_count(),
            if options.judge { "enabled" } else { "off" }
        );
    }

    let mut executor = ProcessCaseExecutor;
    let report = run_suite(&suite, options.repeat, options.judge, &mut executor);

    // The baseline is the gate, not a scoreboard: it records what the suite
    // achieved when it was last accepted, and only a case that fully passed
    // there and no longer does can fail the build.
    let baseline_path = options
        .baseline
        .clone()
        .unwrap_or_else(|| Baseline::path_for(suite.path()));
    let baseline = if baseline_path.exists() {
        let baseline = Baseline::load(&baseline_path).map_err(CliError::io)?;
        BaselineStatus {
            path: baseline_path.to_string_lossy().into_owned(),
            written: false,
            regressions: baseline.regressions(&report),
        }
    } else {
        let fresh = Baseline::from_report(&report);
        let text = fresh.to_pretty_json().map_err(CliError::io)?;
        fs::write(&baseline_path, text).map_err(|error| {
            CliError::io(format!(
                "Cannot write baseline '{}': {}",
                baseline_path.display(),
                error
            ))
        })?;
        BaselineStatus {
            path: baseline_path.to_string_lossy().into_owned(),
            written: true,
            regressions: Vec::new(),
        }
    };

    if options.json {
        let payload = report.to_json(&baseline);
        println!(
            "{}",
            serde_json::to_string_pretty(&payload)
                .map_err(|error| CliError::io(error.to_string()))?
        );
    } else {
        print_agent_eval_human(&report, &baseline);
    }

    if !baseline.regressions.is_empty() {
        let names: Vec<&str> = baseline
            .regressions
            .iter()
            .map(|regression| regression.case.as_str())
            .collect();
        return Err(CliError::compilation(format!(
            "{} eval case(s) regressed against {}: {}",
            names.len(),
            baseline.path,
            names.join(", ")
        )));
    }
    Ok(())
}

fn print_agent_eval_human(report: &spectra_agent::SuiteReport, baseline: &BaselineStatus) {
    for case in &report.cases {
        let verdict = if case.passed == case.repeat {
            "PASS"
        } else {
            "FAIL"
        };
        println!("  {verdict} {} ({}/{})", case.name, case.passed, case.repeat);
        for attempt in case.attempts.iter().filter(|attempt| !attempt.passed) {
            if let Some(error) = attempt.error.as_deref() {
                println!("       {}: {}", attempt.run_id, error);
                continue;
            }
            for outcome in attempt
                .graders
                .iter()
                .filter(|outcome| !outcome.passed && !outcome.skipped)
            {
                println!(
                    "       {}: {} grader: {}",
                    attempt.run_id,
                    outcome.grader.name(),
                    outcome.detail
                );
            }
        }
        for attempt in &case.attempts {
            for outcome in &attempt.graders {
                if outcome.skipped && outcome.grader == GraderKind::Judge {
                    println!("       judge: skipped ({})", outcome.detail);
                    break;
                }
            }
        }
    }
    println!(
        "pass@1 = {:.6}  pass@k = {:.6}",
        report.pass_at_1, report.pass_at_k
    );
    println!(
        "cost: tokens_in={} tokens_out={} cost_micros={}",
        report.tokens_in, report.tokens_out, report.cost_micros
    );
    if baseline.written {
        println!("wrote first baseline {}", baseline.path);
    } else if baseline.regressions.is_empty() {
        println!("baseline {}: no regressions", baseline.path);
    } else {
        for regression in &baseline.regressions {
            println!(
                "regression: {} was {}/{}, now {}/{}",
                regression.case,
                regression.baseline_passed,
                regression.baseline_attempts,
                regression.passed,
                regression.attempts
            );
        }
    }
}
