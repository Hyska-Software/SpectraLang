//! Run capability policy: the evaluator behind the runtime's policy hook
//! (R-3214 D3 / R-3211 run grant).
//!
//! `agent_start` stores `AgentSpec.allow` in the run state and installs one
//! process-global evaluator; `agent_end` clears it once the last run ends.
//! The evaluator reads the thread's active run chain, so a host call made
//! while a run is active is decided against that run's grant.
//!
//! Decision rules (v1, matching the dispatch seam's expectations):
//! * no active run -> `Allow`, so a program without a run behaves exactly as
//!   before this workstream (invariant I5);
//! * an active run may always call the agent's own primitives
//!   (`spectra.std.agent.*`), otherwise a run could never end;
//! * every other host call requires a grant from *every* run on the chain
//!   (grants intersect, so a nested run can only narrow authority);
//! * an empty grant list denies every non-agent host call, except the
//!   compiler-emitted execution machinery in
//!   [`COMPILER_EMITTED_NAMESPACES`] (the coroutine ABI and the JSON
//!   marshalling a tool wrapper is built from).
//!
//! R-3223 adds the taint gate *after* the capability check: a call the run was
//! not granted is refused as a capability denial whatever the taint state, and
//! a granted call that targets a catalog-classified sink is additionally gated
//! when the chain holds untrusted content ([`crate::taint::gate`]).

use spectra_runtime::agent::policy_hook::{
    clear_policy_evaluator, set_policy_evaluator, PolicyDecision,
};
use spectra_runtime::agent::run_context;
use spectra_runtime::ffi::SpectraHostValue;

/// Namespace of the run's own primitives.
const AGENT_NAMESPACE: &str = "spectra.std.agent.";

/// Host-call namespaces the compiler emits on the run's own execution path and
/// that never reach outside the process, so a run's grant does not govern
/// them.
///
/// A capability bounds effects: a tool body's host calls are the tool's
/// declared effects, and `tools::enforce_run_grant` already refuses to start a
/// dispatch unless every registered tool's effects are inside the run's grant.
/// What is left ungranted on that path is the language's own machinery — the
/// coroutine/task protocol the compiler emits for `await`, and the JSON
/// derive/format helpers that marshal a tool's payload and encode its result.
/// Requiring a grant for those would mean every tool-bearing spec grants the
/// compiler its own marshalling code, which is not an author decision.
///
/// This is an exception list, not a default: every other namespace still
/// requires a grant, and none of these namespaces contains a catalog sink, so
/// the taint gate (which keys on sinks) is unaffected.
const COMPILER_EMITTED_NAMESPACES: &[&str] = &[
    // `await`/`block_on` over a Task (the coroutine ABI).
    "spectra.async.",
    // JSON derive: parse/decode_field/typed_error_field/quote_string/...
    "spectra.api.json.",
    // The encoding helpers the same marshalling emits.
    "spectra.std.convert.",
    "spectra.std.string.",
];

/// Whether the host call is part of the compiler's own machinery on the run's
/// path (see [`COMPILER_EMITTED_NAMESPACES`]).
///
/// Shared with the tool grant check (R-3222 T5), so a tool's *derived effects*
/// do not include the coroutine ABI or the JSON/format helpers the compiler
/// emits around the author's code: a tool body may use `==` on strings without
/// its spec having to grant the compiler's helper.
pub(crate) fn is_compiler_emitted(host_call: &str) -> bool {
    COMPILER_EMITTED_NAMESPACES
        .iter()
        .any(|namespace| host_call.starts_with(namespace))
}

/// Installs the evaluator. Idempotent: installing twice replaces the slot with
/// an equivalent closure.
///
/// The installed closure calls [`authorize`], so the predicate an out-of-band
/// caller sees and the decision the dispatch seam enforces are the same code.
pub(crate) fn install() {
    set_policy_evaluator(|host_call, args| evaluate(host_call, args));
}

/// Removes the evaluator once no run is live.
pub(crate) fn uninstall_if_idle() {
    if crate::run::live_run_count() == 0 {
        clear_policy_evaluator();
    }
}

/// Whether the active run chain authorizes `host_call` (ADR 0016 D4).
///
/// The same predicate the installed evaluator applies, so an out-of-band
/// `authorize()` query cannot disagree with enforcement. The capability
/// decision does not depend on the call's arguments, so the query passes the
/// empty argument list; the taint gate is not part of this predicate.
#[allow(dead_code)] // the ADR's out-of-band predicate; the dispatch seam is the enforcement path
pub(crate) fn authorize(host_call: &str) -> bool {
    matches!(evaluate(host_call, &[]), PolicyDecision::Allow)
}

/// The full decision for one host call: capabilities first, then taint.
fn evaluate(host_call: &str, args: &[SpectraHostValue]) -> PolicyDecision {
    let chain = run_context::current_chain();
    let grant = evaluate_grants(&chain, host_call);
    if !matches!(grant, PolicyDecision::Allow) {
        return grant;
    }
    crate::taint::gate(&chain, host_call, args)
}

/// The capability decision alone. `Allow` when no run is active, the call is
/// the run's own primitive, or a grant matches.
fn evaluate_grants(chain: &[u64], host_call: &str) -> PolicyDecision {
    if chain.is_empty() {
        return PolicyDecision::Allow;
    }

    // Intersect the grants of every active run; an identifier that is not a
    // live agent run contributes nothing (it belongs to another subsystem).
    let mut grants: Option<Vec<String>> = None;
    for id in chain {
        let Some(allowed) = crate::run::allow_for_raw_id(*id) else {
            continue;
        };
        grants = Some(match grants {
            None => allowed,
            Some(existing) => existing
                .into_iter()
                .filter(|grant| allowed.contains(grant))
                .collect(),
        });
    }
    let Some(grants) = grants else {
        return PolicyDecision::Allow;
    };

    if host_call.starts_with(AGENT_NAMESPACE) {
        return PolicyDecision::Allow;
    }
    if is_compiler_emitted(host_call) {
        return PolicyDecision::Allow;
    }
    if grants.iter().any(|grant| grant_matches(grant, host_call)) {
        return PolicyDecision::Allow;
    }
    PolicyDecision::Deny {
        reason: format!("run does not grant '{host_call}'"),
    }
}

/// A grant authorizes a host call when it names it exactly or when it names an
/// enclosing namespace (`spectra.std.fs` grants `spectra.std.fs.fs_read`).
///
/// Shared with the tool dispatcher (R-3222 T5), so a tool's effects are
/// checked against the grant with the same predicate the dispatch seam uses.
pub(crate) fn grant_matches(grant: &str, host_call: &str) -> bool {
    host_call == grant
        || (host_call.len() > grant.len()
            && host_call.starts_with(grant)
            && host_call.as_bytes()[grant.len()] == b'.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::{alloc_run, take_run};
    use crate::spec::AgentSpec;

    fn run_with(allow: &[&str]) -> i64 {
        let grants = allow
            .iter()
            .map(|grant| format!("\"{grant}\""))
            .collect::<Vec<_>>()
            .join(",");
        let json = format!(r#"{{"goal":"g","model":"mock/echo","allow":[{grants}]}}"#);
        alloc_run(AgentSpec::parse(&json).expect("spec"), "run-policy".to_string(), None)
            .expect("alloc")
    }

    #[test]
    fn no_active_run_allows_everything() {
        assert!(authorize("spectra.std.fs.fs_write"));
    }

    #[test]
    fn an_active_run_denies_ungranted_calls_and_allows_its_own_primitives() {
        let run = run_with(&[]);
        let _guard = run_context::push(run as u64);
        assert!(authorize("spectra.std.agent.ask"));
        assert!(!authorize("spectra.std.fs.fs_write"));
        assert!(matches!(
            evaluate("spectra.std.fs.fs_write", &[]),
            PolicyDecision::Deny { .. }
        ));
        drop(_guard);
        take_run(run).expect("end");
    }

    /// Pins the wiring, not just the predicate: the installed evaluator must
    /// deny through the runtime's single generic dispatch entrypoint.
    #[test]
    fn the_installed_evaluator_denies_through_the_dispatch_seam() {
        let _global = crate::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Use a host the runtime itself registers, so the test observes the
        // real dispatch path (and introduces no new contract symbol).
        spectra_runtime::register();
        install();
        let run = run_with(&[]);
        let name = "spectra.std.env.env_get";
        let mut results = [0 as i64; 1];

        let invoke = |results: &mut [i64; 1]| {
            spectra_runtime::ffi::spectra_rt_host_invoke(
                name.as_ptr(),
                name.len(),
                std::ptr::null(),
                0,
                results.as_mut_ptr(),
                results.len(),
            )
        };

        // Outside a run the hook stays default-Allow (invariant I5): the call
        // reaches the host, which reports its own argument error.
        let outside = invoke(&mut results);
        assert_ne!(
            outside,
            spectra_runtime::ffi::HOST_STATUS_DENIED,
            "no active run must not deny"
        );

        let denied = {
            let _guard = run_context::push(run as u64);
            invoke(&mut results)
        };
        assert_eq!(
            denied,
            spectra_runtime::ffi::HOST_STATUS_DENIED,
            "an ungranted host call must be denied at dispatch"
        );

        take_run(run).expect("end");
        uninstall_if_idle();
    }

    #[test]
    fn the_compilers_own_machinery_needs_no_grant() {
        let run = run_with(&[]);
        let _guard = run_context::push(run as u64);
        // The coroutine ABI and the tool-payload marshalling the compiler
        // emits are not author effects.
        assert!(authorize("spectra.async.task.block_on"));
        assert!(authorize("spectra.api.json.typed_error_field"));
        assert!(authorize("spectra.std.convert.int_to_string"));
        assert!(authorize("spectra.std.string.eq"));
        // An outward call in the same run is still denied.
        assert!(!authorize("spectra.std.env.env_get"));
        assert!(!authorize("spectra.api.client.request"));
        drop(_guard);
        take_run(run).expect("end");
    }

    #[test]
    fn grants_match_exactly_or_by_namespace() {
        let run = run_with(&["spectra.std.fs"]);
        let _guard = run_context::push(run as u64);
        assert!(authorize("spectra.std.fs.fs_read"));
        assert!(authorize("spectra.std.fs"));
        // A prefix that is not a namespace boundary is not a grant.
        assert!(!authorize("spectra.std.fsx.fs_read"));
        drop(_guard);
        take_run(run).expect("end");
    }

    #[test]
    fn nested_runs_intersect_grants() {
        let outer = run_with(&["spectra.std.fs", "spectra.std.env"]);
        let inner = run_with(&["spectra.std.fs"]);
        let outer_guard = run_context::push(outer as u64);
        let inner_guard = run_context::push(inner as u64);
        assert!(authorize("spectra.std.fs.fs_read"));
        assert!(!authorize("spectra.std.env.env_get"));
        drop(inner_guard);
        assert!(authorize("spectra.std.env.env_get"));
        drop(outer_guard);
        take_run(inner).expect("end inner");
        take_run(outer).expect("end outer");
    }

    #[test]
    fn a_released_run_stops_authorizing() {
        let run = run_with(&["spectra.std.fs"]);
        take_run(run).expect("end");
        let _guard = run_context::push(run as u64);
        // The chain entry is no longer a live run, so it contributes no grant
        // and no denial.
        assert!(authorize("spectra.std.env.env_get"));
    }
}
