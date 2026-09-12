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
//! before this workstream.
//!
//! # Hook signature
//!
//! ```text
//! evaluate(host_call: &str, args: &[SpectraHostValue]) -> PolicyDecision
//! ```
//!
//! * `host_call` is the host function name the dispatch already resolved
//!   (`spectra.api.client.request`, `spectra.std.fs.fs_read`, ...).
//! * `args` borrows the caller's argument buffer for the duration of the call;
//!   a host call without arguments passes an empty slice. The evaluator MUST
//!   NOT retain the borrow (the buffer belongs to the generated frame) and MUST
//!   NOT read past its length. R-3214 decides from the name alone; R-3223 adds
//!   the taint gate, which reads a declared scope key (a sink's `scope_keys`)
//!   from the arguments while the arguments are still live, so the decision
//!   costs nothing beyond the one lookup the dispatch already performed.
//!
//! The repr(C) host-call ABI is unchanged: `args` is a borrow of the same
//! buffer the host function itself receives, never a new channel.

use std::sync::{Arc, LazyLock, Mutex};

use crate::ffi::SpectraHostValue;

/// The decision returned for a single generic host call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// The call may be dispatched.
    Allow,
    /// The call must not be dispatched; the reason is recorded for the
    /// structured denial message (ADR 0016 D4).
    Deny { reason: String },
}

/// Signature of an installed policy evaluator (see the module docs).
pub type PolicyEvaluator =
    dyn Fn(&str, &[SpectraHostValue]) -> PolicyDecision + Send + Sync + 'static;

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
    F: Fn(&str, &[SpectraHostValue]) -> PolicyDecision + Send + Sync + 'static,
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
/// `args` is forwarded unchanged: see the module docs for its borrow contract.
pub(crate) fn evaluate(name: &str, args: &[SpectraHostValue]) -> PolicyDecision {
    let evaluator = evaluator_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    match evaluator {
        Some(evaluate) => evaluate(name, args),
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

        assert_eq!(
            evaluate("spectra.test.unrestricted", &[]),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn installed_evaluator_decides_and_is_cleared() {
        let _lock = test_guard();
        clear_policy_evaluator();

        set_policy_evaluator(|name, args| {
            if name == "spectra.test.denied" {
                PolicyDecision::Deny {
                    reason: format!("not granted ({} arg(s))", args.len()),
                }
            } else {
                PolicyDecision::Allow
            }
        });

        assert_eq!(
            evaluate("spectra.test.denied", &[7]),
            PolicyDecision::Deny {
                reason: "not granted (1 arg(s))".to_string()
            },
            "the evaluator must observe the dispatch arguments"
        );
        assert_eq!(
            evaluate("spectra.test.allowed", &[]),
            PolicyDecision::Allow
        );

        clear_policy_evaluator();
        assert_eq!(
            evaluate("spectra.test.denied", &[]),
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
