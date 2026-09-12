//! Budget compilation, accrual evaluation and cooperative cancellation
//! (R-3216 T1-T3).
//!
//! An `AgentSpec` declares four independent ceilings: `max_tokens`,
//! `max_cost_micros`, `max_seconds` and `max_tool_calls`. `0` means "no
//! ceiling" for that dimension; every other value is a hard ceiling.
//!
//! Accrual is read from the run's own counters (`steps`, `tool_calls`,
//! `tokens_in`, `tokens_out`, `cost_micros`, and the wall clock anchored at
//! `agent_start`), so accounting is exactly what the provider reported and no
//! second ledger can drift:
//!
//! - `tokens_in + tokens_out` is compared with `max_tokens`. The same spec
//!   field also caps one provider response's output tokens (it travels in the
//!   request); as a run ceiling it bounds every billable token the run spends.
//! - `cost_micros`, as reported by the provider, is compared with
//!   `max_cost_micros`. A provider that cannot report cost makes the ceiling
//!   unenforceable, so `agent_start` fails closed for that combination.
//! - `elapsed_ms` is compared with `max_seconds * 1000`.
//! - `tool_calls` is compared with `max_tool_calls` *before* each charge, so
//!   the call that would exceed the ceiling is denied rather than executed.
//!
//! Evaluation happens at call boundaries: before a model call, before a tool
//! call, and after a model turn has been accounted. A ceiling reached by work
//! that never calls back into the gateway (plain time passing) is observed by
//! the next boundary, which is the honest granularity for a cooperative
//! runtime.
//!
//! Crossing a ceiling marks the run cancelled (`Report.status ==
//! "budget_exceeded"` plus the ceiling name), sets the cancellation token of
//! every other in-flight task of the run, and makes every subsequent call on
//! the run return the typed `budget_exceeded` error naming the ceiling. The
//! turn that crossed is accounted and still delivers its value: it is the
//! last unit of work the run performs. Tasks already in flight observe the
//! token at their own boundaries and fail with the same typed error instead of
//! delivering a value.
//!
//! Cancellation is cooperative: no thread is killed, and the task objects are
//! not cancelled through `cancel_task_handle` because that would surface a
//! hard cancellation status to compiled `await` instead of the typed error the
//! run contract promises. The runtime's cancellation token
//! (`spectra_runtime::stdlib::CancellationToken`, the hook used by
//! `spawn_cancellable_background_task`) is the flag the workers observe.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use spectra_runtime::stdlib::CancellationToken;

use crate::error::AgentError;
use crate::run::{self, RunState};
use crate::spec::AgentSpec;

/// Which declared ceiling a run hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ceiling {
    Tokens,
    Cost,
    Seconds,
    ToolCalls,
}

impl Ceiling {
    /// The `AgentSpec` field that names this ceiling in errors and reports.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Tokens => "max_tokens",
            Self::Cost => "max_cost_micros",
            Self::Seconds => "max_seconds",
            Self::ToolCalls => "max_tool_calls",
        }
    }

    /// Accrued usage versus the declared ceiling, for the cancellation error.
    pub(crate) fn detail(self, budget: &Budget, state: &RunState) -> String {
        match self {
            Self::Tokens => format!(
                "{} of {} tokens used (input + output)",
                used_tokens(state),
                budget.max_tokens
            ),
            Self::Cost => format!(
                "{} of {} cost micros used",
                state.cost_micros, budget.max_cost_micros
            ),
            Self::Seconds => format!(
                "{} of {} ms elapsed",
                state.elapsed_ms(),
                budget.max_seconds.saturating_mul(1000)
            ),
            Self::ToolCalls => format!(
                "{} of {} tool calls used",
                state.tool_calls, budget.max_tool_calls
            ),
        }
    }
}

/// The four ceilings compiled from one `AgentSpec`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Budget {
    pub(crate) max_tokens: u64,
    pub(crate) max_cost_micros: u64,
    pub(crate) max_seconds: u64,
    pub(crate) max_tool_calls: u64,
}

impl Budget {
    /// Compiles the spec's ceilings. Negative values never reach here
    /// (`AgentSpec::parse` rejects them); `max(0)` keeps the cast total.
    pub(crate) fn compile(spec: &AgentSpec) -> Self {
        Self {
            max_tokens: spec.max_tokens.max(0) as u64,
            max_cost_micros: spec.max_cost_micros.max(0) as u64,
            max_seconds: spec.max_seconds.max(0) as u64,
            max_tool_calls: spec.max_tool_calls.max(0) as u64,
        }
    }

    /// Whether any ceiling is declared (test-only helper for the zero-means-
    /// unlimited contract).
    #[cfg(test)]
    pub(crate) fn bounded(&self) -> bool {
        self.max_tokens > 0
            || self.max_cost_micros > 0
            || self.max_seconds > 0
            || self.max_tool_calls > 0
    }

    /// Pre-call evaluation: a dimension with no room left denies the call.
    ///
    /// `max_seconds` is a deadline (`>=`); the accrued dimensions deny when
    /// the usage has already reached the ceiling, so an exactly-exhausted
    /// budget cannot buy one more call. `max_tool_calls` is not evaluated
    /// here: only the tool gate consumes it.
    fn exhausted(&self, state: &RunState) -> Option<Ceiling> {
        if self.max_seconds > 0 && state.elapsed_ms() >= self.max_seconds.saturating_mul(1000) {
            return Some(Ceiling::Seconds);
        }
        if self.max_tokens > 0 && used_tokens(state) >= self.max_tokens {
            return Some(Ceiling::Tokens);
        }
        if self.max_cost_micros > 0 && state.cost_micros >= self.max_cost_micros {
            return Some(Ceiling::Cost);
        }
        None
    }

    /// Post-accrual evaluation: a dimension strictly over its ceiling
    /// cancels the run after this unit of work.
    fn crossed(&self, state: &RunState) -> Option<Ceiling> {
        if self.max_tokens > 0 && used_tokens(state) > self.max_tokens {
            return Some(Ceiling::Tokens);
        }
        if self.max_cost_micros > 0 && state.cost_micros > self.max_cost_micros {
            return Some(Ceiling::Cost);
        }
        if self.max_seconds > 0 && state.elapsed_ms() >= self.max_seconds.saturating_mul(1000) {
            return Some(Ceiling::Seconds);
        }
        None
    }

    /// Tokens left before `max_tokens`, clamped at zero.
    ///
    /// Returns `i64::MAX` when no token ceiling is declared: the sentinel the
    /// `budget_remaining` binding documents.
    pub(crate) fn remaining_tokens(&self, state: &RunState) -> i64 {
        if self.max_tokens == 0 {
            return i64::MAX;
        }
        let used = used_tokens(state);
        self.max_tokens.saturating_sub(used).min(i64::MAX as u64) as i64
    }
}

/// Billable tokens accrued by the run.
fn used_tokens(state: &RunState) -> u64 {
    state.tokens_in.saturating_add(state.tokens_out)
}

/// Sets the cancellation token of every in-flight task of the run except the
/// one performing the evaluation (when known).
fn cancel_siblings(state: &RunState, current: Option<&CancellationToken>) {
    for token in &state.tasks {
        let is_current = current
            .map(|current| Arc::ptr_eq(current, token))
            .unwrap_or(false);
        if !is_current {
            token.store(true, Ordering::Release);
        }
    }
}

/// Guards a new unit of work: refuses to start when the run is cancelled or
/// when an already-exhausted dimension has no room for it.
pub(crate) fn guard(
    state: &mut RunState,
    current: Option<&CancellationToken>,
) -> Result<(), AgentError> {
    if state.cancelled {
        return Err(state.cancelled_error());
    }
    if let Some(ceiling) = state.budget.exhausted(state) {
        state.mark_cancelled(ceiling);
        cancel_siblings(state, current);
        return Err(state.cancelled_error());
    }
    Ok(())
}

/// Settles a completed model turn: a strictly crossed ceiling cancels the run
/// (this turn keeps its value); a run cancelled by a sibling refuses the
/// value with the typed error.
pub(crate) fn settle(
    state: &mut RunState,
    current: &CancellationToken,
) -> Result<(), AgentError> {
    if state.cancelled {
        return Err(state.cancelled_error());
    }
    if let Some(ceiling) = state.budget.crossed(state) {
        state.mark_cancelled(ceiling);
        cancel_siblings(state, Some(current));
    }
    Ok(())
}

/// Guards the start of a run-scoped background task.
///
/// The token is the cooperative flag a worker observes at its boundaries: a
/// sibling that crossed a ceiling sets it, and this task refuses to start work
/// for the cancelled run instead of delivering a value.
pub(crate) fn guard_task(
    run_handle: i64,
    current: &CancellationToken,
) -> Result<(), AgentError> {
    run::with_run(run_handle, |state| {
        if current.load(Ordering::Acquire) && state.cancelled {
            return Err(state.cancelled_error());
        }
        guard(state, Some(current))
    })?
}

/// Settles a run-scoped background task after its work succeeded.
pub(crate) fn settle_task(
    run_handle: i64,
    current: &CancellationToken,
) -> Result<(), AgentError> {
    run::with_run(run_handle, |state| settle(state, current))?
}

/// Charges one tool call against `max_tool_calls`.
///
/// This is the crate API the governed tool dispatch (R-3222) calls before
/// executing a tool. The call is denied — and the run cancelled — when the
/// ceiling is reached, so with `max_tool_calls = 2` the third call fails.
///
/// Dead until the dispatch path lands (R-3222); the crate tests exercise it
/// directly in the meantime.
#[allow(dead_code)]
pub(crate) fn charge_tool_call(run_handle: i64) -> Result<(), AgentError> {
    run::with_run(run_handle, |state| {
        guard(state, None)?;
        let max = state.budget.max_tool_calls;
        if max > 0 && state.tool_calls >= max {
            state.mark_cancelled(Ceiling::ToolCalls);
            cancel_siblings(state, None);
            return Err(state.cancelled_error());
        }
        state.tool_calls = state.tool_calls.saturating_add(1);
        Ok(())
    })?
}

/// Tokens remaining for the run, or `i64::MAX` when no token ceiling is set.
pub(crate) fn remaining(run_handle: i64) -> Result<i64, AgentError> {
    run::with_run(run_handle, |state| state.budget.remaining_tokens(state))
}

/// Whether a spec with a cost ceiling can be enforced by this spec's provider.
///
/// Fail-closed rule (R-3216 T1): a cost ceiling is only meaningful when every
/// model response carries a cost. `agent_start` therefore refuses the run
/// when `max_cost_micros > 0` and the selected provider cannot report cost,
/// instead of silently accepting a ceiling that could never fire.
pub(crate) fn cost_ceiling_is_enforceable(
    spec: &AgentSpec,
    reports_cost: bool,
) -> Result<(), AgentError> {
    if spec.max_cost_micros > 0 && !reports_cost {
        return Err(AgentError::CostAccountingUnavailable(format!(
            "spec declares a max_cost_micros ceiling of {} but provider '{}' cannot report cost; \
             use a provider that reports per-response cost or remove the ceiling",
            spec.max_cost_micros, spec.model
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::AgentSpec;

    fn spec(extra: &str) -> AgentSpec {
        AgentSpec::parse(&format!(
            r#"{{"goal":"budget-test","model":"mock/echo","endpoint":"mock:"{extra}}}"#
        ))
        .expect("valid spec")
    }

    fn cancelled_token() -> CancellationToken {
        Arc::new(std::sync::atomic::AtomicBool::new(true))
    }

    #[test]
    fn ceilings_compile_from_the_spec_with_zero_meaning_unlimited() {
        let unlimited = Budget::compile(&spec(""));
        assert!(!unlimited.bounded());
        assert_eq!(unlimited.remaining_tokens(&run_state(&spec(""))), i64::MAX);

        let bounded = Budget::compile(&spec(
            r#","max_tokens":10,"max_cost_micros":20,"max_seconds":30,"max_tool_calls":40"#,
        ));
        assert_eq!(bounded.max_tokens, 10);
        assert_eq!(bounded.max_cost_micros, 20);
        assert_eq!(bounded.max_seconds, 30);
        assert_eq!(bounded.max_tool_calls, 40);
    }

    /// A detached run state used to evaluate the pure budget arithmetic.
    fn run_state(spec: &AgentSpec) -> RunState {
        RunState::new(spec.clone(), "run-budget".to_string(), None)
    }

    #[test]
    fn token_ceiling_crossing_cancels_and_names_the_ceiling() {
        let spec = spec(r#","max_tokens":6"#);
        let mut state = run_state(&spec);
        assert_eq!(state.budget.remaining_tokens(&state), 6);

        // 1 input + 3 output tokens: inside the ceiling.
        state.record_turn(1, 3, 0, 0);
        assert_eq!(state.budget.remaining_tokens(&state), 2);
        assert!(state.budget.crossed(&state).is_none());

        // A second turn takes the run to 8 > 6.
        state.record_turn(1, 3, 0, 0);
        assert_eq!(state.budget.remaining_tokens(&state), 0);
        let ceiling = state.budget.crossed(&state).expect("crossed");
        assert_eq!(ceiling, Ceiling::Tokens);
        assert_eq!(ceiling.name(), "max_tokens");

        state.mark_cancelled(ceiling);
        assert_eq!(state.status, "budget_exceeded");
        let error = state.cancelled_error();
        assert_eq!(error.kind(), "budget_exceeded");
        assert!(error.detail().contains("max_tokens"), "{error}");
        assert!(error.detail().contains("8 of 6 tokens used"), "{error}");
    }

    #[test]
    fn pre_call_guard_denies_an_exhausted_dimension() {
        let spec = spec(r#","max_cost_micros":10"#);
        let mut state = run_state(&spec);
        state.record_turn(1, 3, 7, 0);
        // 7 of 10: another turn is still allowed.
        assert!(guard(&mut state, None).is_ok());
        state.record_turn(1, 3, 3, 0);
        // Exactly at the ceiling: the next call is denied, never overrun.
        let error = guard(&mut state, None).expect_err("exhausted");
        assert!(error.detail().contains("max_cost_micros"), "{error}");
        assert!(error.detail().contains("10 of 10 cost micros used"), "{error}");
        assert_eq!(state.status, "budget_exceeded");
        // Subsequent calls are refused with the same typed error.
        assert!(guard(&mut state, None).is_err());
        assert_eq!(state.report_ceiling(), "max_cost_micros");
    }

    #[test]
    fn tool_call_ceiling_allows_two_and_denies_the_third() {
        let handle =
            run::alloc_run(spec(r#","max_tool_calls":2"#), "run-budget".to_string(), None)
                .expect("alloc run");
        for allowed in 0..2 {
            charge_tool_call(handle).unwrap_or_else(|error| panic!("call {allowed}: {error}"));
        }
        assert_eq!(
            run::with_run(handle, |state| state.tool_calls).expect("read"),
            2
        );
        let error = charge_tool_call(handle).expect_err("third call denied");
        assert_eq!(error.kind(), "budget_exceeded");
        assert!(error.detail().contains("max_tool_calls"), "{error}");
        assert!(error.detail().contains("2 of 2 tool calls used"), "{error}");
        let report = run::take_run(handle).expect("end").report_json();
        assert!(report.contains("\"status\":\"budget_exceeded\""), "{report}");
        assert!(report.contains("\"ceiling\":\"max_tool_calls\""), "{report}");
        assert!(report.contains("\"tool_calls\":2"), "{report}");
    }

    #[test]
    fn wall_clock_ceiling_cancels_after_a_slow_call() {
        let spec = spec(r#","max_seconds":1"#);
        let mut state = run_state(&spec);
        state.started = std::time::Instant::now() - std::time::Duration::from_millis(1100);
        assert_eq!(state.budget.crossed(&state), Some(Ceiling::Seconds));
        let ceiling = state.budget.crossed(&state).expect("crossed");
        state.mark_cancelled(ceiling);
        let error = state.cancelled_error();
        assert!(error.detail().contains("max_seconds"), "{error}");
        assert!(error.detail().contains("of 1000 ms elapsed"), "{error}");
        // The next call is refused before it can start.
        assert!(guard(&mut state, None).is_err());
    }

    #[test]
    fn settle_marks_cancelled_on_crossing_and_cancels_in_flight_siblings() {
        let handle = run::alloc_run(spec(r#","max_tokens":6"#), "run-budget".to_string(), None)
            .expect("alloc");
        let token = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sibling = Arc::new(std::sync::atomic::AtomicBool::new(false));
        run::register_task(handle, Arc::clone(&token));
        run::register_task(handle, Arc::clone(&sibling));
        // The first turn stays inside the ceiling and cancels nothing.
        run::with_run(handle, |state| state.record_turn(1, 3, 0, 0)).expect("turn");
        settle_task(handle, &token).expect("first turn stays inside the ceiling");
        assert!(!sibling.load(Ordering::Acquire));
        // The second turn crosses it: the value is delivered (Ok), the run is
        // cancelled, and the *other* task's token is set.
        run::with_run(handle, |state| state.record_turn(1, 3, 0, 0)).expect("turn");
        settle_task(handle, &token).expect("the crossing turn delivers its value");
        assert_eq!(
            run::with_run(handle, |state| state.status).expect("status"),
            "budget_exceeded"
        );
        assert!(!token.load(Ordering::Acquire), "the crossing task is never self-cancelled");
        assert!(sibling.load(Ordering::Acquire), "in-flight siblings are cancelled");
        // Subsequent work on the run is refused with the typed error.
        run::with_run(handle, |state| state.record_turn(1, 3, 0, 0)).expect("turn");
        let error = settle_task(handle, &sibling).expect_err("subsequent turn refused");
        assert!(error.detail().contains("max_tokens"), "{error}");
        let error = guard_task(handle, &sibling).expect_err("guard refuses");
        assert_eq!(error.kind(), "budget_exceeded");
        let state = run::take_run(handle).expect("end");
        assert!(state.report_json().contains("\"ceiling\":\"max_tokens\""));
        assert!(state.report_json().contains("\"status\":\"budget_exceeded\""));
    }

    #[test]
    fn cancellation_tokens_are_registered_and_removed_by_identity() {
        let handle = run::alloc_run(spec(""), "run-budget".to_string(), None).expect("alloc");
        let token = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let other = Arc::new(std::sync::atomic::AtomicBool::new(false));
        assert!(run::register_task(handle, Arc::clone(&token)));
        assert!(run::register_task(handle, Arc::clone(&other)));
        run::unregister_task(handle, &other);
        assert_eq!(run::with_run(handle, |state| state.tasks.len()).expect("len"), 1);
        run::unregister_task(handle, &cancelled_token());
        assert_eq!(run::with_run(handle, |state| state.tasks.len()).expect("len"), 1);
        run::take_run(handle).expect("end");
    }

    #[test]
    fn cost_ceiling_requires_a_provider_that_reports_cost() {
        let bounded = spec(r#","max_cost_micros":5"#);
        assert!(cost_ceiling_is_enforceable(&bounded, true).is_ok());
        let error = cost_ceiling_is_enforceable(&bounded, false).expect_err("fail closed");
        assert_eq!(error.kind(), "cost_accounting_unavailable");
        assert!(error.detail().contains("max_cost_micros"), "{error}");
        // Without a cost ceiling the provider contract is irrelevant.
        assert!(cost_ceiling_is_enforceable(&spec(""), false).is_ok());
    }
}
