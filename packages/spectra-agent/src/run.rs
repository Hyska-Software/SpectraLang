//! Run and chunk-stream tables, accounting, and report construction
//! (R-3211 T1).
//!
//! Both domains are generational [`HandleTable`]s keyed by the `std.agent`
//! handle kinds added to `spectra_runtime::handles`. Releasing a handle bumps
//! its generation, so a double `agent_end` / use-after-end can never alias a
//! later run and is reported as a typed error.

use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::Instant;

use spectra_runtime::agent::run_context;
use spectra_runtime::handles::{HandleError, HandleId, HandleKind, HandleTable};
use spectra_runtime::stdlib::CancellationToken;

use crate::budget::{Budget, Ceiling};
use crate::compensate::PendingCompensation;
use crate::error::AgentError;
use crate::journal::Journal;
use crate::spec::{AgentSpec, UntrustedPolicy};
use crate::taint::Ledger;

/// Live state of one `agent_start` -> `agent_end` run.
pub(crate) struct RunState {
    pub spec: AgentSpec,
    /// The run's journal identity: the run id every record carries, and the
    /// journal itself when the spec enabled one (R-3217 T1).
    pub run_id: String,
    pub journal: Option<Journal>,
    /// `approve` decisions scoped to the run: `allow-always` is cached here so
    /// a later step of the same run does not re-ask (R-3217 T3).
    pub approvals: std::collections::BTreeMap<String, bool>,
    /// Ceilings compiled from `spec` and the accrual they are measured
    /// against (R-3216 T1).
    pub budget: Budget,
    pub started: Instant,
    /// Model turns completed (every `ask`, `ask_json` and `ask_stream`).
    pub steps: u64,
    /// Tool dispatches; `charge_tool_call` is the accounting entry point the
    /// governed dispatch (R-3222) uses, so this stays 0 until then.
    pub tool_calls: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_micros: u64,
    /// Transport retries the provider performed (recorded, not journaled yet:
    /// the journal is R-3217).
    pub provider_retries: u64,
    /// Sticky run outcome: `"completed"`, `"budget_exceeded"` (R-3216) or
    /// `"failed"` (provider transport failure).
    pub status: &'static str,
    /// Name of the ceiling that cancelled this run, if any.
    pub ceiling: Option<Ceiling>,
    /// Whether a ceiling cancelled the run; every subsequent call is refused.
    pub cancelled: bool,
    /// Cancellation tokens of this run's in-flight background tasks, removed
    /// by identity when their task finishes.
    pub tasks: Vec<CancellationToken>,
    /// Provenance of everything that entered the run (R-3223 T1).
    pub taint: Ledger,
    /// Declared, not-yet-executed compensations in declaration order
    /// (R-3224). `rollback` drains this stack LIFO; `agent_end` reports its
    /// length as `compensations_pending`, so a silent leak is visible.
    pub pending_compensations: Vec<PendingCompensation>,
}

impl RunState {
    pub(crate) fn new(spec: AgentSpec, run_id: String, journal: Option<Journal>) -> Self {
        let budget = Budget::compile(&spec);
        Self {
            spec,
            run_id,
            journal,
            approvals: std::collections::BTreeMap::new(),
            budget,
            started: Instant::now(),
            steps: 0,
            tool_calls: 0,
            tokens_in: 0,
            tokens_out: 0,
            cost_micros: 0,
            provider_retries: 0,
            status: "completed",
            ceiling: None,
            cancelled: false,
            tasks: Vec::new(),
            taint: Ledger::default(),
            pending_compensations: Vec::new(),
        }
    }

    /// Records one completed model turn.
    pub(crate) fn record_turn(
        &mut self,
        tokens_in: u64,
        tokens_out: u64,
        cost_micros: u64,
        retries: u64,
    ) {
        self.steps += 1;
        self.tokens_in = self.tokens_in.saturating_add(tokens_in);
        self.tokens_out = self.tokens_out.saturating_add(tokens_out);
        self.cost_micros = self.cost_micros.saturating_add(cost_micros);
        self.provider_retries = self.provider_retries.saturating_add(retries);
    }

    /// Marks the run failed. Used for provider transport failures only;
    /// caller-level failures (schema violations, lifecycle misuse) leave the
    /// run outcome untouched. A budget cancellation wins: the resource
    /// decision was made before the failure and the report must name the
    /// ceiling (R-3216 T3).
    pub(crate) fn mark_failed(&mut self) {
        if self.status == "completed" {
            self.status = "failed";
        }
    }

    /// Cancels the run because a ceiling was crossed: the status names the
    /// outcome and the report carries the ceiling that was hit.
    pub(crate) fn mark_cancelled(&mut self, ceiling: Ceiling) {
        self.status = "budget_exceeded";
        self.ceiling = Some(ceiling);
        self.cancelled = true;
    }

    /// Marks the run `rolled_back` after an explicit `rollback` (R-3224 T3).
    ///
    /// A budget cancellation wins, before or during the rollback: the resource
    /// decision must keep naming the ceiling. Every other prior outcome
    /// (`completed`, `failed`) is superseded, because the rollback is the run's
    /// explicit final act and `rolled_back` is exactly what the report must say
    /// about it.
    pub(crate) fn mark_rolled_back(&mut self) {
        if self.status != "budget_exceeded" {
            self.status = "rolled_back";
        }
    }

    /// The typed error every call on a cancelled run returns.
    pub(crate) fn cancelled_error(&self) -> AgentError {
        let ceiling = self.ceiling.map(Ceiling::name).unwrap_or("budget");
        AgentError::BudgetExceeded(format!(
            "run '{}' is cancelled: the {} ceiling was crossed{}",
            self.spec.goal,
            ceiling,
            self.ceiling
                .map(|ceiling| format!(" ({})", ceiling.detail(&self.budget, self)))
                .unwrap_or_default()
        ))
    }

    /// Ceiling name reported by `agent_end`; the empty string means the run
    /// completed without hitting one.
    pub(crate) fn report_ceiling(&self) -> &'static str {
        self.ceiling.map(Ceiling::name).unwrap_or("")
    }

    pub(crate) fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }

    /// Report document in the plan's field order, plus the `ceiling` key the
    /// R-3216 report contract requires ("which ceiling was hit") and the
    /// `replay` key R-3217 adds (whether the run resumed an existing journal).
    /// Both extras are additive: the authored `Report` record keeps its eight
    /// declared fields and the JSON derive ignores keys it does not declare.
    pub(crate) fn report_json(&self) -> String {
        format!(
            concat!(
                "{{\"status\":\"{}\",\"steps\":{},\"tool_calls\":{},\"tokens_in\":{},",
                "\"tokens_out\":{},\"cost_micros\":{},\"elapsed_ms\":{},",
                "\"ceiling\":\"{}\",\"compensations_pending\":{},\"replay\":{}}}"
            ),
            self.status,
            self.steps,
            self.tool_calls,
            self.tokens_in,
            self.tokens_out,
            self.cost_micros,
            self.elapsed_ms(),
            self.report_ceiling(),
            self.pending_compensations.len(),
            self.journal
                .as_ref()
                .map(Journal::replaying)
                .unwrap_or(false),
        )
    }
}

/// Buffered chunks for one `ask_stream` turn.
pub(crate) struct ChunkStreamState {
    pub chunks: Vec<String>,
    pub cursor: usize,
}

/// What the taint gate needs to know about one live run (R-3223 T3).
pub(crate) struct RunTaint {
    /// Raw handle, so the gate can journal its decision and consult the
    /// approval registry for the run that made the call.
    pub handle: i64,
    pub run_id: String,
    pub goal: String,
    /// The run's `AgentSpec.untrusted` policy.
    pub policy: UntrustedPolicy,
    /// Whether any ledger entry is still untrusted.
    pub holds_untrusted: bool,
    /// Origins of the untrusted entries (deduplicated), for the denial text.
    pub untrusted_origins: Vec<String>,
}

fn lock<T>(mutex: &'static Mutex<T>) -> MutexGuard<'static, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn runs() -> &'static Mutex<HandleTable<RunState>> {
    static RUNS: LazyLock<Mutex<HandleTable<RunState>>> =
        LazyLock::new(|| Mutex::new(HandleTable::new(HandleKind::AgentRun)));
    &RUNS
}

fn streams() -> &'static Mutex<HandleTable<ChunkStreamState>> {
    static STREAMS: LazyLock<Mutex<HandleTable<ChunkStreamState>>> =
        LazyLock::new(|| Mutex::new(HandleTable::new(HandleKind::AgentChunkStream)));
    &STREAMS
}

fn kind_label(kind: HandleKind) -> &'static str {
    match kind {
        HandleKind::AgentRun => "Run",
        HandleKind::AgentChunkStream => "ChunkStream",
        _ => "handle",
    }
}

/// Decodes a raw handle and checks its domain tag.
fn decode(handle: i64, expected: HandleKind) -> Result<HandleId, AgentError> {
    let label = kind_label(expected);
    let id = HandleId::from_raw(handle).map_err(|_| {
        AgentError::InvalidHandle(format!("value {handle} is not a valid {label} handle"))
    })?;
    if id.kind() != expected {
        return Err(AgentError::InvalidHandle(format!(
            "expected a {label} handle, found {:?}",
            id.kind()
        )));
    }
    Ok(id)
}

fn unknown(handle: i64, error: HandleError) -> AgentError {
    AgentError::UnknownHandle(format!("handle {handle} is unknown, released or stale ({error})"))
}

/// Allocates a run and returns its raw handle.
pub(crate) fn alloc_run(
    spec: AgentSpec,
    run_id: String,
    journal: Option<Journal>,
) -> Result<i64, AgentError> {
    let mut table = lock(runs());
    let handle = table.insert(RunState::new(spec, run_id, journal));
    Ok(handle.raw())
}

/// Runs `work` with mutable access to a live run.
pub(crate) fn with_run<R>(
    handle: i64,
    work: impl FnOnce(&mut RunState) -> R,
) -> Result<R, AgentError> {
    let id = decode(handle, HandleKind::AgentRun)?;
    let mut table = lock(runs());
    let state = table.get_mut(id).map_err(|error| unknown(handle, error))?;
    Ok(work(state))
}

/// Releases a run and returns its final state.
pub(crate) fn take_run(handle: i64) -> Result<RunState, AgentError> {
    let id = decode(handle, HandleKind::AgentRun)?;
    let mut table = lock(runs());
    table.remove(id).map_err(|error| unknown(handle, error))
}

/// Runs `work` with `run_handle` entered on the active run chain.
///
/// This is how a run becomes *active* for the single policy seam (ADR 0016
/// D3/D5): the chain is pushed, not replaced, so a nested run stacks on its
/// enclosing run and can only narrow authority.
///
/// The run-scoped host calls enter it for their dynamic extent — the model
/// gateway, the dispatch primitives and the taint ledger — which is where the
/// run executes work of its own (a tool wrapper's host calls, work it spawns).
/// The program frame between `agent_start` and `agent_end` is deliberately not
/// entered: a host call the author wrote directly is the author's own action,
/// while the model-driven path is what a run's authority bounds. A program
/// without a run therefore behaves exactly as before (invariant I5).
pub(crate) fn in_run_scope<T>(run_handle: i64, work: impl FnOnce() -> T) -> T {
    let _guard = run_context::push(run_handle as u64);
    work()
}

/// Number of live runs; the policy hook is cleared when this reaches zero.
pub(crate) fn live_run_count() -> usize {
    lock(runs()).len()
}

/// Registers an in-flight background task's cancellation token with its run.
/// Returns `false` when the run handle is no longer live.
pub(crate) fn register_task(handle: i64, token: CancellationToken) -> bool {
    with_run(handle, |state| state.tasks.push(token)).is_ok()
}

/// Removes a task's cancellation token by identity (`Arc::ptr_eq`), so a
/// finished task never makes a later cancellation touch an unrelated token.
pub(crate) fn unregister_task(handle: i64, token: &CancellationToken) {
    let _ = with_run(handle, |state| {
        state.tasks.retain(|candidate| !Arc::ptr_eq(candidate, token));
    });
}

/// Capability allow-list for a raw run identifier, if it is a live run.
///
/// Used by [`crate::policy`] for each identifier on the active run chain.
pub(crate) fn allow_for_raw_id(raw: u64) -> Option<Vec<String>> {
    let id = HandleId::from_raw(raw as i64).ok()?;
    if id.kind() != HandleKind::AgentRun {
        return None;
    }
    let table = lock(runs());
    table.get(id).ok().map(|state| state.spec.allow.clone())
}

/// Taint state of a raw run identifier, if it is a live run.
///
/// Used by [`crate::taint::gate`] for each identifier on the active run chain.
pub(crate) fn taint_for_raw_id(raw: u64) -> Option<RunTaint> {
    let id = HandleId::from_raw(raw as i64).ok()?;
    if id.kind() != HandleKind::AgentRun {
        return None;
    }
    let table = lock(runs());
    let state = table.get(id).ok()?;
    Some(RunTaint {
        handle: raw as i64,
        run_id: state.run_id.clone(),
        goal: state.spec.goal.clone(),
        policy: state.spec.untrusted,
        holds_untrusted: state.taint.holds_untrusted(),
        untrusted_origins: state.taint.untrusted_origins(),
    })
}

/// Allocates a chunk stream and returns its raw handle.
pub(crate) fn alloc_stream(chunks: Vec<String>) -> Result<i64, AgentError> {
    let mut table = lock(streams());
    let handle = table.insert(ChunkStreamState { chunks, cursor: 0 });
    Ok(handle.raw())
}

/// Pops the next chunk; the empty string marks the end of the stream.
pub(crate) fn next_chunk(handle: i64) -> Result<String, AgentError> {
    let id = decode(handle, HandleKind::AgentChunkStream)?;
    let mut table = lock(streams());
    let stream = table.get_mut(id).map_err(|error| unknown(handle, error))?;
    let chunk = stream.chunks.get(stream.cursor).cloned().unwrap_or_default();
    if stream.cursor < stream.chunks.len() {
        stream.cursor += 1;
    }
    Ok(chunk)
}

/// Releases a chunk stream. Idempotent: closing an already-released stream
/// still succeeds, because "close" is a request to reach the closed state.
pub(crate) fn close_stream(handle: i64) -> Result<bool, AgentError> {
    let id = decode(handle, HandleKind::AgentChunkStream)?;
    let mut table = lock(streams());
    match table.remove(id) {
        Ok(_) => Ok(true),
        Err(HandleError::Stale) => Ok(true),
        Err(error) => Err(unknown(handle, error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::AgentSpec;

    fn spec() -> AgentSpec {
        AgentSpec::parse(r#"{"goal":"g","model":"mock/echo","seed":1}"#).expect("valid spec")
    }

    fn run() -> i64 {
        alloc_run(spec(), "run-test".to_string(), None).expect("alloc")
    }

    #[test]
    fn released_run_handle_cannot_be_reused() {
        let handle = run();
        let state = take_run(handle).expect("first end");
        assert_eq!(state.steps, 0);
        let report = state.report_json();
        assert!(report.starts_with("{\"status\":\"completed\""), "{report}");
        assert!(
            report.ends_with("\"compensations_pending\":0,\"replay\":false}"),
            "{report}"
        );
        // Double end and use-after-end are typed errors, never a panic.
        assert!(matches!(take_run(handle), Err(AgentError::UnknownHandle(_))));
        assert!(matches!(
            with_run(handle, |_| ()),
            Err(AgentError::UnknownHandle(_))
        ));
    }

    #[test]
    fn accounting_and_status_are_reported() {
        let handle = run();
        with_run(handle, |state| {
            state.record_turn(3, 5, 13, 1);
            state.record_turn(2, 4, 10, 0);
        })
        .expect("accounting");
        let state = take_run(handle).expect("end");
        assert_eq!(state.steps, 2);
        assert_eq!(state.tokens_in, 5);
        assert_eq!(state.tokens_out, 9);
        assert_eq!(state.cost_micros, 23);
        assert_eq!(state.provider_retries, 1);
        let report = state.report_json();
        assert!(report.contains("\"steps\":2"), "{report}");
        assert!(report.contains("\"tokens_in\":5"), "{report}");
        assert!(report.contains("\"status\":\"completed\""), "{report}");
    }

    #[test]
    fn a_wrong_kind_handle_is_rejected_before_lookup() {
        let stream = alloc_stream(vec!["a".to_string()]).expect("alloc");
        assert!(matches!(
            with_run(stream, |_| ()),
            Err(AgentError::InvalidHandle(_))
        ));
        assert_eq!(next_chunk(stream).expect("first chunk"), "a");
        assert_eq!(next_chunk(stream).expect("end marker"), "");
        assert_eq!(next_chunk(stream).expect("still end"), "");
        assert!(close_stream(stream).expect("close"));
        // Idempotent close; reads after close are typed errors.
        assert!(close_stream(stream).expect("close twice"));
        assert!(matches!(
            next_chunk(stream),
            Err(AgentError::UnknownHandle(_))
        ));
    }

    #[test]
    fn allow_list_is_visible_to_the_policy_layer() {
        let handle = run();
        let grants = allow_for_raw_id(handle as u64).expect("live run");
        assert!(grants.is_empty());
        assert!(allow_for_raw_id(0).is_none());
        take_run(handle).expect("end");
        assert!(allow_for_raw_id(handle as u64).is_none());
    }

    /// The run scope is what makes host calls the run performs attributable to
    /// it; entering is a push, so nested runs stack and cannot widen authority.
    #[test]
    fn the_run_scope_activates_the_run_for_its_dynamic_extent() {
        let handle = run();
        assert!(run_context::current_chain().is_empty());
        let outer = run();
        in_run_scope(handle, || {
            assert_eq!(run_context::current_chain(), vec![handle as u64]);
            in_run_scope(outer, || {
                assert_eq!(
                    run_context::current_chain(),
                    vec![handle as u64, outer as u64]
                );
                let taint = taint_for_raw_id(outer as u64).expect("live run");
                assert!(!taint.holds_untrusted);
                assert_eq!(taint.policy, UntrustedPolicy::Approve);
            });
            assert_eq!(run_context::current_chain(), vec![handle as u64]);
        });
        assert!(
            run_context::current_chain().is_empty(),
            "leaving the scope must restore the caller's chain"
        );
        take_run(handle).expect("end");
        take_run(outer).expect("end");
        assert!(taint_for_raw_id(handle as u64).is_none());
    }
}
