//! Capability policy hook consulted by the generic dispatch seam.
//!
//! R-3214 (ADR 0016 D3): `dispatch_generic` asks one policy hook whether a
//! generic host call is allowed before it invokes the host function. The hook
//! is a process-global slot so the enforcement decision lives in exactly one
//! place; the evaluator itself is installed by `R-3211` (run capabilities) at
//! run start and removed at run end.
//!
//! Until an evaluator is installed the hook defaults to [`PolicyDecision::Allow`],
//! which keeps invariant I5: a program without an active run behaves exactly as
//! before this workstream. The evaluator is called with the host function name
//! (`spectra.api.client.request`, `spectra.std.fs.read`, ...) — the same name
//! the dispatch already resolved, so the decision costs nothing extra.

use std::sync::{Arc, LazyLock, Mutex};

/// The decision returned for a single generic host call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// The call may be dispatched.
    Allow,
    /// The call must not be dispatched; the reason is recorded for the
    /// structured denial message (ADR 0016 D4).
    Deny { reason: String },
}

/// Signature of an installed policy evaluator.
pub type PolicyEvaluator = dyn Fn(&str) -> PolicyDecision + Send + Sync + 'static;

fn evaluator_slot() -> &'static Mutex<Option<Arc<PolicyEvaluator>>> {
    static SLOT: LazyLock<Mutex<Option<Arc<PolicyEvaluator>>>> =
        LazyLock::new(|| Mutex::new(None));
    &SLOT
}

thread_local! {
    /// Reason from the most recent denial on this thread, consumed by
    /// [`crate::panic::spectra_rt_capability_denied`]. Thread-local because a
    /// denial is always immediately followed by the trap on the same thread.
    static LAST_DENIAL_REASON: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// Installs the process-global policy evaluator, replacing any previous one.
///
/// A poisoned slot is recovered rather than propagated: enforcement must never
/// panic at the dispatch seam (ADR 0016 D3).
pub fn set_policy_evaluator<F>(evaluator: F)
where
    F: Fn(&str) -> PolicyDecision + Send + Sync + 'static,
{
    let mut guard = evaluator_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(Arc::new(evaluator));
}

/// Removes the installed evaluator, returning the hook to its default-Allow
/// state. Called when the run that installed it ends.
pub fn clear_policy_evaluator() {
    let mut guard = evaluator_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
}

/// Evaluates the policy for one generic host call.
///
/// Defaults to [`PolicyDecision::Allow`] when no evaluator is installed. The
/// evaluator is cloned out of the slot before being called so it can never
/// deadlock against [`set_policy_evaluator`] or [`clear_policy_evaluator`].
pub(crate) fn evaluate(name: &str) -> PolicyDecision {
    let evaluator = evaluator_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    match evaluator {
        Some(evaluate) => evaluate(name),
        None => PolicyDecision::Allow,
    }
}

/// Records the reason for a denial on this thread.
pub(crate) fn record_denial(reason: &str) {
    LAST_DENIAL_REASON.with(|slot| *slot.borrow_mut() = Some(reason.to_string()));
}

/// Takes (and clears) the recorded denial reason for this thread.
pub(crate) fn take_denial_reason() -> Option<String> {
    LAST_DENIAL_REASON.with(|slot| slot.borrow_mut().take())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::runtime_test_guard()
    }

    #[test]
    fn evaluator_defaults_to_allow() {
        let _lock = test_guard();
        clear_policy_evaluator();

        assert_eq!(evaluate("spectra.test.unrestricted"), PolicyDecision::Allow);
    }

    #[test]
    fn installed_evaluator_decides_and_is_cleared() {
        let _lock = test_guard();
        clear_policy_evaluator();

        set_policy_evaluator(|name| {
            if name == "spectra.test.denied" {
                PolicyDecision::Deny {
                    reason: "not granted".to_string(),
                }
            } else {
                PolicyDecision::Allow
            }
        });

        assert_eq!(
            evaluate("spectra.test.denied"),
            PolicyDecision::Deny {
                reason: "not granted".to_string()
            }
        );
        assert_eq!(evaluate("spectra.test.allowed"), PolicyDecision::Allow);

        clear_policy_evaluator();
        assert_eq!(
            evaluate("spectra.test.denied"),
            PolicyDecision::Allow,
            "clearing the evaluator must restore the default-Allow hook"
        );
    }

    #[test]
    fn denial_reason_is_recorded_and_consumed_once() {
        let _lock = test_guard();
        record_denial("missing capability spectra.std.fs.write");
        assert_eq!(
            take_denial_reason(),
            Some("missing capability spectra.std.fs.write".to_string())
        );
        assert_eq!(take_denial_reason(), None);
    }
}
