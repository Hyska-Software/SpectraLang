//! Declared compensation and explicit rollback (R-3224).
//!
//! ADR 0018 D11 freezes the semantics: compensation is declared, not inferred.
//! `compensate(run, tool, arguments_json)` appends a pending compensation (LIFO)
//! to the run; `rollback(run, reason)` executes the pending compensations in
//! LIFO order through the governed dispatch — the same path `act`/`tool_call`
//! use, so capabilities, taint gating and budget still apply — and is
//! replay-safe: a compensation that already executed is not executed again
//! when the run replays, and its recorded outcome is returned instead.
//!
//! Two journal step kinds carry the feature (ADR 0018's `kind` field):
//!
//! * `compensation` — one declaration. `output` is `"true"` and `attribution`
//!   is the JSON `{tool, arguments}` a replay re-reads to rebuild the pending
//!   stack. The declaration is journaled and flushed before `compensate`
//!   returns, so a crash cannot lose the memory of how to undo the step.
//! * `rollback` — one executed compensation attempt. Unlike a model-driven
//!   tool step (R-3222 leaves a wrapper failure unrecorded), the outcome is
//!   recorded even when the compensation fails: a failed compensation must
//!   not be re-executed on replay, and the record is the audit trail of the
//!   attempt. A failure never masks the compensations that follow it — the
//!   LIFO walk continues and `rollback` reports how many were executed.
//!
//! The declaration-time name check is the runtime half of E3205: a literal
//! tool name is validated by the compiler against the tools the compilation
//! unit knows, and *every* name is validated here against the run's registered
//! tool registry before anything is journaled (so a computed name cannot
//! declare an unexecutable compensation).

use serde_json::{json, Value};

use crate::budget;
use crate::error::AgentError;
use crate::journal::StepUsage;
use crate::replay::{self, Kind, Resolved};
use crate::run;
use crate::tools;

/// One declared, not-yet-executed compensation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingCompensation {
    /// Registered tool name that undoes the step.
    pub(crate) tool: String,
    /// The JSON argument document the tool will be invoked with.
    pub(crate) arguments: String,
}

impl PendingCompensation {
    fn new(tool: &str, arguments: &str) -> Self {
        Self {
            tool: tool.to_string(),
            arguments: arguments.to_string(),
        }
    }

    /// The declaration's recorded `attribution`: everything a replay needs to
    /// rebuild this pending entry without re-reading the caller's strings.
    fn to_attribution(&self) -> String {
        json!({"tool": self.tool, "arguments": self.arguments}).to_string()
    }

    fn from_attribution(attribution: &str) -> Result<Self, AgentError> {
        let value: Value = serde_json::from_str(attribution).map_err(|error| {
            AgentError::Journal(format!(
                "recorded compensation declaration is invalid: {error}"
            ))
        })?;
        let tool = value.get("tool").and_then(Value::as_str).ok_or_else(|| {
            AgentError::Journal("recorded compensation declaration has no tool".to_string())
        })?;
        Ok(Self {
            tool: tool.to_string(),
            arguments: value
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        })
    }
}

/// `compensate(run, tool, arguments_json) -> Result<bool, Error>`.
///
/// Validates `tool` against the run's registered tool registry at declaration
/// time, then journals the pending compensation (LIFO) and returns `true`. The
/// journal write is flushed before returning; a replay of the declaration
/// rebuilds the pending entry instead of appending a second one.
pub(crate) fn compensate(
    run_handle: i64,
    tool: &str,
    arguments: &str,
) -> Result<bool, AgentError> {
    // Declaration-time validation against the registry: a computed name is
    // validated nowhere else, so this must happen before the journal write.
    tools::lookup(tool)?;
    let input = format!("compensate\0{tool}\0{arguments}");
    match replay::resolve(run_handle, Kind::Compensation, &input)? {
        Resolved::Recorded(record) => {
            let pending = match &record.attribution {
                Some(attribution) => PendingCompensation::from_attribution(attribution)?,
                // The record's input digest matched, so the caller's own
                // strings are the same declaration (a record written by an
                // earlier build that predates the attribution).
                None => PendingCompensation::new(tool, arguments),
            };
            run::with_run(run_handle, |state| state.pending_compensations.push(pending))?;
            Ok(true)
        }
        Resolved::Fresh(token) => {
            let pending = PendingCompensation::new(tool, arguments);
            replay::commit(
                run_handle,
                &token,
                Some(&input),
                "true",
                StepUsage::default(),
                0,
                Some(pending.to_attribution()),
            )?;
            run::with_run(run_handle, |state| state.pending_compensations.push(pending))?;
            Ok(true)
        }
    }
}

/// `rollback(run, reason) -> Result<int, Error>`.
///
/// Executes the pending compensations in LIFO order through the governed
/// dispatch, journaling each attempt and its outcome, and returns the number
/// executed. Already-executed compensations are not executed again on replay;
/// the recorded outcome stands. A compensation that fails is recorded and does
/// not stop the compensations declared before it.
///
/// The run is marked `rolled_back` (unless a budget ceiling cancelled it, in
/// which case the report keeps naming the ceiling) so `agent_end` reports the
/// explicit rollback.
pub(crate) fn rollback(run_handle: i64, reason: &str) -> Result<i64, AgentError> {
    // I7: compensations reach their tools through the governed dispatch, so
    // the run's grant must cover the tools it exposes — the check `act` and
    // `tool_call` perform before their first dispatch.
    tools::enforce_run_grant(run_handle)?;
    run::with_run(run_handle, |state| state.mark_rolled_back())?;

    let pending = run::with_run(run_handle, |state| {
        // LIFO: the most recently declared compensation executes first.
        std::mem::take(&mut state.pending_compensations)
    })?;

    let mut executed = 0i64;
    for (ordinal, item) in pending.iter().rev().enumerate() {
        let input = format!(
            "rollback\0{ordinal}\0{reason}\0{}\0{}",
            item.tool, item.arguments
        );
        match replay::resolve(run_handle, Kind::Rollback, &input)? {
            Resolved::Recorded(_) => {
                // The attempt already happened: return its recorded outcome
                // without reaching the wrapper. The charge is mirrored so a
                // replayed run's accounting matches the original, exactly as
                // [`crate::tools::invoke`] charges a replayed tool step. A
                // refused charge is swallowed rather than propagated: the
                // original charge was refused the same way (and also did not
                // increment the counter), and `rollback`'s contract is to
                // complete the walk and report the recorded outcomes.
                let _ = budget::charge_tool_call(run_handle);
            }
            Resolved::Fresh(token) => {
                // The outcome is recorded even on failure, so a replay does
                // not re-execute a compensation whose side effect may have
                // been partially applied.
                let output = match tools::dispatch(run_handle, &item.tool, &item.arguments) {
                    Ok(result) => json!({
                        "tool": item.tool,
                        "ok": true,
                        "result": result,
                    }),
                    Err(error) => json!({
                        "tool": item.tool,
                        "ok": false,
                        "error": error.message(),
                    }),
                }
                .to_string();
                replay::commit(
                    run_handle,
                    &token,
                    Some(&input),
                    &output,
                    StepUsage::default(),
                    -1,
                    Some(item.tool.clone()),
                )?;
            }
        }
        executed += 1;
    }
    Ok(executed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::AgentSpec;

    fn spec_json(run_id: &str, journal: &str) -> String {
        format!(
            r#"{{"goal":"compensation-goal","model":"mock/echo","endpoint":"mock:","journal":"{}","run_id":"{}"}}"#,
            journal.replace('\\', "/"),
            run_id
        )
    }

    fn run_with_journal(dir: &str, run_id: &str) -> i64 {
        let spec = AgentSpec::parse(&spec_json(run_id, dir)).expect("valid spec");
        let journal = crate::journal::Journal::open(run_id, dir, false).expect("journal");
        run::alloc_run(spec, run_id.to_string(), Some(journal)).expect("alloc")
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("spectra-compensate-{tag}-{}", crate::journal::new_run_id()))
    }

    /// A wrapper that succeeds with an empty JSON document.
    extern "C" fn ok_wrapper(_run: i64, _args: i64, _out: i64) -> i64 {
        0
    }

    /// A wrapper that fails, so the compensation's failure path is exercised.
    extern "C" fn failing_wrapper(_run: i64, _args: i64, _out: i64) -> i64 {
        1
    }

    fn lock_registry() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::tools::REGISTRY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        tools::clear();
        guard
    }

    #[test]
    fn an_unknown_tool_is_refused_before_anything_is_journaled() {
        let _guard = lock_registry();
        let dir = temp_dir("unknown");
        let dir_text = dir.to_string_lossy().to_string();
        let handle = run_with_journal(&dir_text, "compensate-unknown");
        let error = compensate(handle, "no_such_tool", "{}").expect_err("unknown tool");
        assert_eq!(error.kind(), "unknown_tool");
        let state = run::take_run(handle).expect("end");
        assert!(state.pending_compensations.is_empty());
        assert_eq!(state.journal.expect("journal").len(), 0);
        tools::clear();
    }

    #[test]
    fn a_declaration_is_pending_and_flushed() {
        let _guard = lock_registry();
        assert!(tools::register(
            "ok_tool".to_string(),
            ok_wrapper as *const () as usize as i64,
            "test tool".to_string(),
            "{}".to_string(),
            "[]"
        ));
        let dir = temp_dir("pending");
        let dir_text = dir.to_string_lossy().to_string();
        let handle = run_with_journal(&dir_text, "compensate-pending");
        assert!(compensate(handle, "ok_tool", "{\"a\":1}").expect("declare"));
        assert!(compensate(handle, "ok_tool", "{\"a\":2}").expect("declare"));
        let state = run::take_run(handle).expect("end");
        assert_eq!(state.pending_compensations.len(), 2);
        let report = state.report_json();
        assert!(report.contains("\"compensations_pending\":2"), "{report}");
        assert_eq!(state.journal.expect("journal").len(), 2);
        tools::clear();
    }

    #[test]
    fn rollback_executes_lifo_and_a_failure_does_not_mask_the_rest() {
        let _guard = lock_registry();
        assert!(tools::register(
            "ok_tool".to_string(),
            ok_wrapper as *const () as usize as i64,
            "test tool".to_string(),
            "{}".to_string(),
            "[]"
        ));
        assert!(tools::register(
            "failing".to_string(),
            failing_wrapper as *const () as usize as i64,
            "failing test tool".to_string(),
            "{}".to_string(),
            "[]"
        ));
        let dir = temp_dir("lifo");
        let dir_text = dir.to_string_lossy().to_string();
        let handle = run_with_journal(&dir_text, "compensate-lifo");
        // Declared "ok_tool" first, then "failing": LIFO runs "failing" first.
        assert!(compensate(handle, "ok_tool", "{}").expect("declare"));
        assert!(compensate(handle, "failing", "{}").expect("declare"));
        let executed = rollback(handle, "undo everything").expect("rollback");
        assert_eq!(executed, 2);
        let state = run::take_run(handle).expect("end");
        assert_eq!(state.status, "rolled_back");
        assert!(state.pending_compensations.is_empty());
        let report = state.report_json();
        assert!(report.contains("\"status\":\"rolled_back\""), "{report}");
        assert!(report.contains("\"compensations_pending\":0"), "{report}");
        let journal = state.journal.expect("journal");
        // Two declarations plus two attempt records, in execution order: the
        // failing compensation first (LIFO), the successful one second.
        assert_eq!(journal.len(), 4);
        assert!(journal.get(2).expect("first attempt").output.contains("\"ok\":false"));
        assert!(journal.get(3).expect("second attempt").output.contains("\"ok\":true"));
        tools::clear();
    }

    #[test]
    fn rollback_enforces_the_run_grant_before_dispatching() {
        let _guard = lock_registry();
        assert!(tools::register(
            "writer".to_string(),
            ok_wrapper as *const () as usize as i64,
            "writes".to_string(),
            "{}".to_string(),
            r#"["spectra.std.fs.fs_write"]"#
        ));
        let dir = temp_dir("grant");
        let dir_text = dir.to_string_lossy().to_string();
        let handle = run_with_journal(&dir_text, "compensate-grant");
        assert!(compensate(handle, "writer", "{}").expect("declare"));
        // The run grants nothing, so the tool's derived effect is outside its
        // authority: rollback refuses before the first dispatch (I7).
        let error = rollback(handle, "undo").expect_err("grant denied");
        assert_eq!(error.kind(), "capability_denied");
        // Nothing executed and nothing was drained, so the author can widen
        // the grant and roll back again.
        let state = run::take_run(handle).expect("end");
        assert_eq!(state.pending_compensations.len(), 1);
        assert_eq!(state.status, "completed");
        tools::clear();
    }

    #[test]
    fn a_replayed_declaration_and_rollback_are_not_duplicated() {
        let _guard = lock_registry();
        assert!(tools::register(
            "ok_tool".to_string(),
            ok_wrapper as *const () as usize as i64,
            "test tool".to_string(),
            "{}".to_string(),
            "[]"
        ));
        let dir = temp_dir("replay");
        let dir_text = dir.to_string_lossy().to_string();
        // First execution declares and rolls back.
        let handle = run_with_journal(&dir_text, "compensate-replay");
        assert!(compensate(handle, "ok_tool", "{}").expect("declare"));
        assert_eq!(rollback(handle, "undo").expect("rollback"), 1);
        let state = run::take_run(handle).expect("end");
        assert_eq!(state.journal.expect("journal").len(), 2);

        // A second run with the same id and journal replays both steps: the
        // declaration is rebuilt, the rollback counts the recorded attempt
        // without dispatching again.
        let replayed = run_with_journal(&dir_text, "compensate-replay");
        assert!(compensate(replayed, "ok_tool", "{}").expect("replay declaration"));
        assert_eq!(rollback(replayed, "undo").expect("replay rollback"), 1);
        let state = run::take_run(replayed).expect("end");
        let journal = state.journal.expect("journal");
        assert!(journal.replaying());
        assert_eq!(journal.len(), 2, "replay must not append a record");
        assert_eq!(state.status, "rolled_back");
        tools::clear();
    }
}
