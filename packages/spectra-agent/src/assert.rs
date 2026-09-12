//! Governed assertions: `require(run, condition, message)` (R-3217 T4).
//!
//! An assertion is a deterministic check that belongs to the run, not to an
//! evaluation grader. `true` passes and is journaled; `false` returns a typed
//! error carrying the assertion message and the run goal, and marks the run
//! failed. A `false` is a run outcome, never a panic — it is visible in the
//! journal and in `Report.status`.
//!
//! Replaying an assertion returns the recorded outcome: a run that failed an
//! assertion fails the same way when resumed, and a passing assertion stays
//! passing without re-evaluating whatever it measured.

use crate::error::AgentError;
use crate::journal::StepUsage;
use crate::replay::{self, Kind, Resolved};
use crate::run;

/// `require(run, condition, message)`: true when the condition holds.
pub(crate) fn require(
    run_handle: i64,
    condition: bool,
    message: &str,
) -> Result<bool, AgentError> {
    let goal = run::with_run(run_handle, |state| state.spec.goal.clone())?;
    let input = format!("require\0{condition}\0{message}");

    match replay::resolve(run_handle, Kind::Assertion, &input)? {
        Resolved::Recorded(record) => {
            if record.output == "true" {
                Ok(true)
            } else {
                let message = record
                    .attribution
                    .unwrap_or_else(|| "assertion failed".to_string());
                run::with_run(run_handle, |state| state.mark_failed())?;
                Err(failed(&message, &goal))
            }
        }
        Resolved::Fresh(token) => {
            if condition {
                replay::commit(
                    run_handle,
                    &token,
                    Some(&input),
                    "true",
                    StepUsage::default(),
                    0,
                    Some(message.to_string()),
                )?;
                return Ok(true);
            }
            // The failure is durable before the error is surfaced, and the run
            // itself is marked failed so `agent_end` reports it.
            replay::commit(
                run_handle,
                &token,
                Some(&input),
                "false",
                StepUsage::default(),
                0,
                Some(message.to_string()),
            )?;
            run::with_run(run_handle, |state| state.mark_failed())?;
            Err(failed(message, &goal))
        }
    }
}

fn failed(message: &str, goal: &str) -> AgentError {
    AgentError::AssertionFailed(format!("{message} (run goal: {goal})"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::AgentSpec;

    fn run_with_journal(dir: &str) -> i64 {
        let spec = AgentSpec::parse(&format!(
            r#"{{"goal":"assertion-goal","model":"mock/echo","journal":"{}"}}"#,
            dir.replace('\\', "/")
        ))
        .expect("valid spec");
        let journal = crate::journal::Journal::open("assert-run", dir, false).expect("journal");
        run::alloc_run(spec, "assert-run".to_string(), Some(journal)).expect("alloc")
    }

    #[test]
    fn a_passing_assertion_returns_true_and_is_journaled() {
        let dir = std::env::temp_dir().join(format!(
            "spectra-assert-{}",
            crate::journal::new_run_id()
        ));
        let dir_text = dir.to_string_lossy().to_string();
        let handle = run_with_journal(&dir_text);
        assert!(require(handle, true, "must hold").expect("passes"));
        let state = run::take_run(handle).expect("end");
        assert_eq!(state.status, "completed");
        let journal = state.journal.expect("journal");
        assert_eq!(journal.len(), 1);
        let record = journal.get(0).expect("step 0");
        assert_eq!(record.kind, "assertion");
        assert_eq!(record.output, "true");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_failing_assertion_names_the_message_and_the_goal_and_fails_the_run() {
        let dir = std::env::temp_dir().join(format!(
            "spectra-assert-{}",
            crate::journal::new_run_id()
        ));
        let dir_text = dir.to_string_lossy().to_string();
        let handle = run_with_journal(&dir_text);
        let error = require(handle, false, "output must be non-empty").expect_err("fails");
        assert_eq!(error.kind(), "assertion_failed");
        assert!(error.detail().contains("output must be non-empty"), "{error}");
        assert!(error.detail().contains("assertion-goal"), "{error}");
        let state = run::take_run(handle).expect("end");
        assert_eq!(state.status, "failed");
        let journal = state.journal.expect("journal");
        let record = journal.get(0).expect("step 0");
        assert_eq!(record.output, "false");
        assert_eq!(record.attribution.as_deref(), Some("output must be non-empty"));

        // Replay: the same call returns the recorded failure without
        // re-evaluating the assertion.
        let resumed = run_with_journal(&dir_text);
        let replayed = require(resumed, false, "output must be non-empty").expect_err("replayed");
        assert_eq!(replayed.kind(), "assertion_failed");
        assert!(replayed.detail().contains("output must be non-empty"));
        let state = run::take_run(resumed).expect("end");
        assert_eq!(state.status, "failed");
        assert_eq!(state.journal.expect("journal").len(), 1, "no duplicate record");

        // A resumed run that asserts something else at the same step is a
        // divergence, not a silent replay of an unrelated outcome.
        let divergent = run_with_journal(&dir_text);
        let mismatched = require(divergent, true, "a different contract").expect_err("divergence");
        assert_eq!(mismatched.kind(), "journal_error");
        assert!(mismatched.detail().contains("divergence"), "{mismatched}");
        run::take_run(divergent).expect("end");
        std::fs::remove_dir_all(&dir).ok();
    }
}
