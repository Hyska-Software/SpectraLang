//! Native implementation boundary for the `std.agent` package.
//!
//! R-3209 landed the namespace seam with one function (`std.agent.token_count`);
//! R-3211 adds the model gateway and run lifecycle on top of it, R-3212
//! adds provenance-carrying memory (`remember`/`recall`), R-3216 adds
//! budget accounting, cooperative cancellation and `budget_remaining`, and
//! R-3217 adds the durable journal with replay, human approval, governed
//! assertions and the OpenTelemetry GenAI span seam, and R-3223 adds the
//! message/handle taint ledger with `untrusted`/`trust` and the sink gate at
//! the dispatch seam, and R-3224 adds declared compensation
//! (`compensate`/`rollback`). This crate is an `rlib`
//! aggregated by `spectra_api::register` (see
//! `packages/spectra-api/src/api_registration.rs`): it declares no staticlib,
//! no `#[no_mangle]` symbol, and no linker entry.

mod abi;
mod act;
mod approval;
mod assert;
mod budget;
mod compensate;
mod digest;
mod error;
mod eval;
mod hosts;
mod journal;
mod memory;
mod policy;
mod provider;
mod replay;
mod run;
mod schema;
mod spec;
mod taint;
mod token;
mod tools;
mod trace;

pub use approval::{set_approver, ApprovalRequest, Approver, Decision};
pub use eval::{
    run_suite, Baseline, BaselineStatus, CaseExecution, CaseExecutor, CaseInvocation, EvalSuite,
    GraderKind, Regression, SuiteReport,
};
pub use provider::transport::{
    clear_http_transport, set_http_transport, HttpTransport, TransportResponse,
};
pub use trace::{
    set_trace_sink, Span, TraceSink, GEN_AI_CONVENTIONS_SCHEMA_URL, GEN_AI_CONVENTIONS_VERSION,
};

/// Runtime host-call name the midend lowers `std.agent.token_count` to.
pub const TOKEN_COUNT_HOST_CALL: &str = "spectra.std.agent.token_count";

/// Runtime host-call name the midend lowers `std.agent.remember` to.
pub const REMEMBER_HOST_CALL: &str = "spectra.std.agent.remember";

/// Runtime host-call name the midend lowers `std.agent.recall` to.
pub const RECALL_HOST_CALL: &str = "spectra.std.agent.recall";

/// Runtime host-call name the midend lowers `std.agent.budget_remaining` to.
pub const BUDGET_REMAINING_HOST_CALL: &str = "spectra.std.agent.budget_remaining";

/// Runtime host-call name the midend lowers `std.agent.act` to.
pub const ACT_HOST_CALL: &str = "spectra.std.agent.act";

/// Runtime host-call name the midend lowers `std.agent.tool_call` to.
pub const TOOL_CALL_HOST_CALL: &str = "spectra.std.agent.tool_call";

/// Runtime host-call name a synthesized registration function emits per tool.
pub const REGISTER_TOOL_HOST_CALL: &str = "spectra.std.agent.register_tool";

/// Runtime host-call name the midend lowers `std.agent.approve` to.
pub const APPROVE_HOST_CALL: &str = "spectra.std.agent.approve";

/// Runtime host-call name the midend lowers `std.agent.require` to.
pub const REQUIRE_HOST_CALL: &str = "spectra.std.agent.require";

/// Runtime host-call name the midend lowers `std.agent.untrusted` to.
pub const UNTRUSTED_HOST_CALL: &str = "spectra.std.agent.untrusted";

/// Runtime host-call name the midend lowers `std.agent.trust` to.
pub const TRUST_HOST_CALL: &str = "spectra.std.agent.trust";

/// Runtime host-call name the midend lowers `std.agent.compensate` to.
pub const COMPENSATE_HOST_CALL: &str = "spectra.std.agent.compensate";

/// Runtime host-call name the midend lowers `std.agent.rollback` to.
pub const ROLLBACK_HOST_CALL: &str = "spectra.std.agent.rollback";

/// Register this crate's host functions into the process-wide runtime
/// registry and return the number of newly inserted entries.
pub fn register() -> usize {
    token::register() + hosts::register() + memory::register()
}

/// Serializes tests that mutate process-global state (the host registry, the
/// injected HTTP transport). Cargo runs a crate's tests in parallel threads,
/// and these slots are intentionally process-wide.
#[cfg(test)]
pub(crate) static GLOBAL_STATE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_inserts_the_agent_surface_once() {
        // Registration is idempotent per process: the first call inserts every
        // entry, a second call observes them already present.
        let _guard = GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        spectra_runtime::ffi::clear_host_functions();
        let first = register();
        let second = register();
        assert_eq!(
            first, 21,
            "token_count + the eight R-3211 host functions + remember/recall + \
             budget_remaining + act/tool_call/register_tool + approve/require + \
             untrusted/trust + compensate/rollback"
        );
        assert_eq!(second, 0);
    }
}
