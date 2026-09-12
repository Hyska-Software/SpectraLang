//! Human approval for governed runs (R-3217 T3).
//!
//! `approve(run, action)` asks the registered [`Approver`]. There is no
//! implicit allow: when no approver is attached the decision is a deny, so an
//! unattended run fails closed. Decisions are journaled with the approver's own
//! attribution payload, and replay consults the journal first, so a replayed
//! run never re-asks a decided action.
//!
//! Scoping follows ADR 0018: `allow-once` authorizes one step, `allow-always`
//! the run. The cache lives on the run, so a second `approve` of the same
//! action in one run is answered from the cached decision — journaled again,
//! with the original attribution, so the record is still complete.

use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use crate::error::AgentError;
use crate::journal::StepUsage;
use crate::replay::{self, Kind, Resolved};
use crate::run;

/// What an approver is asked to decide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    /// Run identity the approval belongs to.
    pub run: String,
    /// The run's goal, so a UI can show intent, not just an action string.
    pub goal: String,
    /// Step the decision is recorded at.
    pub step: u64,
    /// The action being authorized.
    pub action: String,
}

impl ApprovalRequest {
    /// The journal input for this request: the action plus its scope.
    ///
    /// Two runs of the same program approve the same action, so the digest is
    /// stable across a crash/resume; the goal is included so an action string
    /// reused across goals cannot replay a decision from another intent.
    pub(crate) fn journal_input(&self) -> String {
        format!("approve\0{}\0{}", self.goal, self.action)
    }
}

/// An approver's decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Authorize exactly this step.
    AllowOnce { by: String },
    /// Authorize this action for the rest of the run.
    AllowAlways { by: String },
    /// Refuse.
    Deny { by: String },
}

impl Decision {
    pub(crate) fn allowed(&self) -> bool {
        !matches!(self, Self::Deny { .. })
    }

    /// The verb recorded in the journal.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::AllowOnce { .. } => "allow-once",
            Self::AllowAlways { .. } => "allow-always",
            Self::Deny { .. } => "deny",
        }
    }

    /// Who made the decision (the approver's own payload).
    pub(crate) fn by(&self) -> &str {
        match self {
            Self::AllowOnce { by } | Self::AllowAlways { by } | Self::Deny { by } => by,
        }
    }

    /// Human-readable attribution for the journal and for denial messages.
    pub(crate) fn attribution(&self) -> String {
        if self.by().is_empty() {
            format!("{} (unattributed)", self.label())
        } else {
            format!("{} by {}", self.label(), self.by())
        }
    }
}

/// A decision maker. Implementations own their own policy (an interactive UI,
/// an allow-list, a fixed test approver).
pub trait Approver: Send + Sync + 'static {
    fn decide(&self, request: &ApprovalRequest) -> Decision;
}

fn slot() -> &'static Mutex<Option<Arc<dyn Approver>>> {
    static SLOT: LazyLock<Mutex<Option<Arc<dyn Approver>>>> = LazyLock::new(|| Mutex::new(None));
    &SLOT
}

fn lock() -> MutexGuard<'static, Option<Arc<dyn Approver>>> {
    slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Attaches (or removes) the process-global approver.
///
/// Replaces any previous approver — a session's UI is not "first installation
/// wins" like the HTTP transport. Returns `true` when an approver is now
/// attached.
pub fn set_approver(approver: Option<Arc<dyn Approver>>) -> bool {
    let attached = approver.is_some();
    *lock() = approver;
    attached
}

/// The installed approver, if any.
pub(crate) fn approver() -> Option<Arc<dyn Approver>> {
    lock().clone()
}

/// Asks the registered approver, or denies when none is attached.
pub(crate) fn decide(request: &ApprovalRequest) -> Decision {
    match approver() {
        Some(approver) => approver.decide(request),
        None => Decision::Deny {
            by: "default-deny (no approver attached)".to_string(),
        },
    }
}

/// `approve(run, action)`: true when the action is authorized.
///
/// A recorded decision is returned without asking again; a missing record asks
/// the approver and appends the decision. An `allow-always` decision is cached
/// on the run and answers later approvals of the same action.
pub(crate) fn approve(run_handle: i64, action: &str) -> Result<bool, AgentError> {
    approve_with(run_handle, action, |request| Ok(decide(request)))
}

/// [`approve`] with an explicit decision supplier (R-3219).
///
/// The supplier is the seam the ACP permission bridge uses: a protocol adapter
/// that must ask a peer instead of the process approver plugs in here, and
/// inherits every property of the primitive — the recorded decision is
/// returned without asking again, `allow-always` is cached on the run, and the
/// decision is journaled with the approver's own attribution before this
/// returns.
///
/// The supplier is invoked **only when a decision is actually needed**: a
/// replayed approval and a cached `allow-always` never reach it, so a remote
/// peer is never re-asked a decided action. It is called with no run lock
/// held, so a supplier that blocks on a peer cannot deadlock the run table.
pub(crate) fn approve_with<F>(
    run_handle: i64,
    action: &str,
    decide_action: F,
) -> Result<bool, AgentError>
where
    F: FnOnce(&ApprovalRequest) -> Result<Decision, AgentError>,
{
    let (run_id, goal) = run::with_run(run_handle, |state| {
        (state.run_id.clone(), state.spec.goal.clone())
    })?;
    let input = ApprovalRequest {
        run: run_id.clone(),
        goal: goal.clone(),
        step: 0,
        action: action.to_string(),
    }
    .journal_input();

    let (step, decision) = match replay::resolve(run_handle, Kind::Approval, &input)? {
        // Replay: the decision was already made; never ask twice.
        Resolved::Recorded(record) => return Ok(record.output == "true"),
        Resolved::Fresh(token) => {
            let cached = run::with_run(run_handle, |state| state.approvals.get(action).copied())?;
            let decision = match cached {
                Some(true) => Decision::AllowAlways {
                    by: "cached allow-always".to_string(),
                },
                _ => decide_action(&ApprovalRequest {
                    run: run_id.clone(),
                    goal: goal.clone(),
                    step: token.step,
                    action: action.to_string(),
                })?,
            };
            (token.step, decision)
        }
    };

    if let Decision::AllowAlways { .. } = &decision {
        run::with_run(run_handle, |state| {
            state.approvals.insert(action.to_string(), true);
        })?;
    }

    let attribution = decision.attribution();
    replay::commit(
        run_handle,
        &replay::Token {
            step,
            kind: Kind::Approval,
            input_digest: crate::digest::of(&input),
        },
        Some(&input),
        if decision.allowed() { "true" } else { "false" },
        StepUsage::default(),
        0,
        Some(attribution),
    )?;
    Ok(decision.allowed())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AlwaysAllow;

    impl Approver for AlwaysAllow {
        fn decide(&self, _request: &ApprovalRequest) -> Decision {
            Decision::AllowAlways {
                by: "test-approver".to_string(),
            }
        }
    }

    #[test]
    fn the_default_decision_denies_and_is_attributed() {
        set_approver(None);
        let request = ApprovalRequest {
            run: "run-x".to_string(),
            goal: "goal".to_string(),
            step: 3,
            action: "spectra.std.fs.fs_write".to_string(),
        };
        let decision = decide(&request);
        assert!(!decision.allowed());
        assert_eq!(decision.label(), "deny");
        assert!(decision.by().contains("no approver"));

        assert!(set_approver(Some(Arc::new(AlwaysAllow))));
        let decision = decide(&request);
        assert!(decision.allowed());
        assert_eq!(decision.attribution(), "allow-always by test-approver");
        assert!(!set_approver(None));
        assert!(decide(&request).by().contains("no approver"));
    }

    #[test]
    fn journal_input_is_stable_and_scope_aware() {
        let base = ApprovalRequest {
            run: "r".to_string(),
            goal: "g".to_string(),
            step: 0,
            action: "net".to_string(),
        };
        assert_eq!(base.journal_input(), base.journal_input());
        let other_goal = ApprovalRequest {
            goal: "other".to_string(),
            ..base.clone()
        };
        assert_ne!(base.journal_input(), other_goal.journal_input());
    }
}
