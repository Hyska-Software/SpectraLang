//! Typed `std.agent` failures and the two-word tagged `Result` encoding.
//!
//! Every fallible agent host function returns `Result<T, Error>`; the runtime
//! representation is `[tag, payload]` (tag 0 = `Ok`, tag 1 = `Err`) and the
//! error payload is a `std.error.Error` record. Both shapes are produced
//! through the runtime's own allocators — `spectra.std.error.new` for the
//! record and the manual arena for the pair — so no layout knowledge is
//! duplicated here.

use spectra_runtime::ffi::{
    lookup_host_function, SpectraHostCallContext, SpectraHostValue, HOST_STATUS_SUCCESS,
};

use crate::abi;

/// `ErrorCode` indices from `std.error` (declaration order in
/// `compiler/src/semantic/builtin_text_system.rs`). The full enum also has
/// `PermissionDenied`, which this layer never produces: a capability denial is
/// the dispatch seam's fatal path (R-3214), not a `Result` value.
pub(crate) const ERROR_CODE_INVALID_ARGUMENT: i64 = 0;
pub(crate) const ERROR_CODE_NOT_FOUND: i64 = 1;
/// Capability denial: the dispatch seam (R-3214) raises this for a host call
/// outside the run's grant; R-3222 raises it for a tool whose effects exceed
/// the grant.
pub(crate) const ERROR_CODE_PERMISSION_DENIED: i64 = 2;
pub(crate) const ERROR_CODE_IO: i64 = 3;
pub(crate) const ERROR_CODE_INTERNAL: i64 = 4;
pub(crate) const ERROR_CODE_UNSUPPORTED: i64 = 5;

/// A failure surfaced through `Result<T, Error>` instead of a host trap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentError {
    /// The spec JSON is malformed or violates a documented constraint.
    InvalidSpec(String),
    /// A handle argument is not a well-formed agent handle.
    InvalidHandle(String),
    /// The run/stream handle is unknown, released, or of the wrong kind.
    UnknownHandle(String),
    /// No provider is reachable for the requested endpoint/model.
    ProviderNotConfigured(String),
    /// The provider call failed (transport, HTTP status, malformed response).
    Provider(String),
    /// The response did not satisfy the caller's JSON schema.
    SchemaViolation(String),
    /// The run requested deterministic sampling and the provider cannot honor it.
    DeterministicUnavailable(String),
    /// No embedding backend is configured for this run.
    EmbeddingNotConfigured(String),
    /// An agent-memory operation failed (dimension mismatch, malformed ledger,
    /// corrupt artifact).
    Memory(String),
    /// A declared budget ceiling was crossed; the message names the ceiling.
    /// The run is cancelled and every subsequent call returns this error.
    BudgetExceeded(String),
    /// The model asked for a tool that is not registered in this process
    /// (R-3222). Reported back to the model as a tool result, never a trap.
    UnknownTool(String),
    /// A registered tool's marshalling wrapper rejected the call: malformed
    /// arguments or a failure inside the tool. The message is the wrapper's
    /// typed error, which the `act` loop feeds back to the model.
    ToolFailed(String),
    /// A tool's derived effects are not covered by the run's grant (R-3222
    /// T5). Raised before the first dispatch of a run, so a spec can never
    /// authorize less than the tools it exposes.
    CapabilityDenied(String),
    /// The `act` loop exhausted its internal step bound without a final
    /// answer. A run with a declared `max_tool_calls` hits that ceiling
    /// first; this bound only protects against a model that never stops.
    ToolLoopCeiling(String),
    /// The spec declares a cost ceiling that the selected provider cannot
    /// enforce because it does not report per-response cost (R-3216 T1).
    CostAccountingUnavailable(String),
    /// A governed `require(run, condition, message)` assertion failed
    /// (R-3217 T4). The message carries the assertion text and the run goal.
    AssertionFailed(String),
    /// The run journal could not be read, written or replayed (R-3217 T1/T2).
    Journal(String),
    /// A taint ledger operation was rejected (R-3223): an empty or control-
    /// bearing origin/reason, or a malformed ledger input. A declassification
    /// without a reason is the audit gap the ledger exists to close.
    Taint(String),
    /// An MCP exchange failed (R-3218): a malformed endpoint URL, a missing
    /// HTTP transport, a non-2xx response, or a peer that answered with a
    /// JSON-RPC error instead of a result. The remote peer's text is carried
    /// as data, never interpreted.
    Mcp(String),
    /// An A2A exchange failed (R-3219): a malformed request document, or a
    /// task that is not journaled. A task-level outcome (a refused or failed
    /// delegated run) is a Task state, not this error; this variant is the
    /// adapter being handed something it cannot serve.
    A2a(String),
    /// An ACP exchange failed (R-3219): a malformed request document, or a
    /// permission answer the adapter cannot map onto a decision. A client that
    /// answers nothing usable never authorizes an action.
    Acp(String),
    /// A bug or an exhausted runtime resource.
    Internal(String),
}

impl AgentError {
    /// Stable machine-readable kind, prefixed to the human message so callers
    /// can branch on it without parsing free text.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::InvalidSpec(_) => "invalid_spec",
            Self::InvalidHandle(_) => "invalid_handle",
            Self::UnknownHandle(_) => "unknown_handle",
            Self::ProviderNotConfigured(_) => "provider_not_configured",
            Self::Provider(_) => "provider_error",
            Self::SchemaViolation(_) => "schema_violation",
            Self::DeterministicUnavailable(_) => "deterministic_sampling_unavailable",
            Self::EmbeddingNotConfigured(_) => "embedding_not_configured",
            Self::Memory(_) => "memory_error",
            Self::BudgetExceeded(_) => "budget_exceeded",
            Self::UnknownTool(_) => "unknown_tool",
            Self::ToolFailed(_) => "tool_failed",
            Self::CapabilityDenied(_) => "capability_denied",
            Self::ToolLoopCeiling(_) => "tool_loop_ceiling",
            Self::CostAccountingUnavailable(_) => "cost_accounting_unavailable",
            Self::AssertionFailed(_) => "assertion_failed",
            Self::Journal(_) => "journal_error",
            Self::Taint(_) => "taint_error",
            Self::Mcp(_) => "mcp_error",
            Self::A2a(_) => "a2a_error",
            Self::Acp(_) => "acp_error",
            Self::Internal(_) => "internal",
        }
    }

    /// `std.error.ErrorCode` index for this failure.
    pub(crate) fn code(&self) -> i64 {
        match self {
            Self::InvalidSpec(_) | Self::InvalidHandle(_) | Self::SchemaViolation(_) => {
                ERROR_CODE_INVALID_ARGUMENT
            }
            Self::AssertionFailed(_) => ERROR_CODE_INVALID_ARGUMENT,
            Self::Taint(_) => ERROR_CODE_INVALID_ARGUMENT,
            Self::UnknownHandle(_) => ERROR_CODE_NOT_FOUND,
            Self::UnknownTool(_) => ERROR_CODE_NOT_FOUND,
            Self::CapabilityDenied(_) => ERROR_CODE_PERMISSION_DENIED,
            Self::ToolFailed(_) => ERROR_CODE_IO,
            Self::Journal(_) => ERROR_CODE_IO,
            Self::ProviderNotConfigured(_)
            | Self::DeterministicUnavailable(_)
            | Self::EmbeddingNotConfigured(_)
            | Self::BudgetExceeded(_)
            | Self::ToolLoopCeiling(_)
            | Self::CostAccountingUnavailable(_) => ERROR_CODE_UNSUPPORTED,
            Self::Provider(_) => ERROR_CODE_IO,
            Self::Mcp(_) => ERROR_CODE_IO,
            Self::A2a(_) | Self::Acp(_) => ERROR_CODE_IO,
            Self::Memory(_) | Self::Internal(_) => ERROR_CODE_INTERNAL,
        }
    }

    /// Operation that reported the failure, recorded in the `Error` record.
    pub(crate) fn operation(&self) -> &'static str {
        match self {
            Self::InvalidSpec(_) => "agent_start",
            Self::InvalidHandle(_) | Self::UnknownHandle(_) => "agent_run",
            Self::ProviderNotConfigured(_) | Self::Provider(_) => "agent_provider",
            Self::SchemaViolation(_) => "ask_json",
            Self::DeterministicUnavailable(_) => "agent_provider",
            Self::EmbeddingNotConfigured(_) => "embed",
            Self::Memory(_) => "agent_memory",
            Self::BudgetExceeded(_) => "agent_budget",
            Self::UnknownTool(_) | Self::ToolFailed(_) | Self::ToolLoopCeiling(_) => "agent_tool",
            Self::CapabilityDenied(_) => "agent_dispatch",
            Self::CostAccountingUnavailable(_) => "agent_start",
            Self::AssertionFailed(_) => "require",
            Self::Journal(_) => "agent_journal",
            Self::Taint(_) => "agent_taint",
            Self::Mcp(_) => "agent_mcp",
            Self::A2a(_) => "agent_a2a",
            Self::Acp(_) => "agent_acp",
            Self::Internal(_) => "agent",
        }
    }

    pub(crate) fn detail(&self) -> &str {
        match self {
            Self::InvalidSpec(detail)
            | Self::InvalidHandle(detail)
            | Self::UnknownHandle(detail)
            | Self::ProviderNotConfigured(detail)
            | Self::Provider(detail)
            | Self::SchemaViolation(detail)
            | Self::DeterministicUnavailable(detail)
            | Self::EmbeddingNotConfigured(detail)
            | Self::Memory(detail)
            | Self::BudgetExceeded(detail)
            | Self::UnknownTool(detail)
            | Self::ToolFailed(detail)
            | Self::CapabilityDenied(detail)
            | Self::ToolLoopCeiling(detail)
            | Self::CostAccountingUnavailable(detail)
            | Self::AssertionFailed(detail)
            | Self::Journal(detail)
            | Self::Taint(detail)
            | Self::Mcp(detail)
            | Self::A2a(detail)
            | Self::Acp(detail)
            | Self::Internal(detail) => detail,
        }
    }

    pub(crate) fn message(&self) -> String {
        format!("{}: {}", self.kind(), self.detail())
    }

    /// `true` when a retry of the identical call could plausibly succeed.
    fn retryable(&self) -> bool {
        matches!(self, Self::Provider(_))
    }
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

/// Allocates `[tag, payload]` for an `Ok` value.
pub(crate) unsafe fn tagged_ok(payload: SpectraHostValue) -> SpectraHostValue {
    write_tagged(0, payload)
}

/// Allocates `[tag, payload]` for an `Err` value carrying an `Error` record.
///
/// Returns 0 when the runtime cannot materialize the record; callers surface
/// that as a host status failure rather than an untyped zero handle.
pub(crate) unsafe fn tagged_err(error: &AgentError) -> SpectraHostValue {
    let record = alloc_error_record(error);
    if record == 0 {
        return 0;
    }
    write_tagged(1, record)
}

unsafe fn write_tagged(tag: i64, payload: SpectraHostValue) -> SpectraHostValue {
    let raw = abi::alloc_words(2);
    if raw.is_null() {
        return 0;
    }
    *raw = tag;
    *raw.add(1) = payload;
    raw as SpectraHostValue
}

/// Materializes a `std.error.Error` record through the runtime's own
/// constructor, so field access (`error.code(err)`, `err.message`) works
/// exactly as it does for `std.fs` results.
unsafe fn alloc_error_record(error: &AgentError) -> SpectraHostValue {
    let Some(new_error) = lookup_host_function("spectra.std.error.new") else {
        return 0;
    };
    let message = abi::alloc_string(&error.message());
    let operation = abi::alloc_string(error.operation());
    let context = abi::alloc_string("");
    let origin = abi::alloc_string("std.agent");
    let args = [
        error.code(),
        message,
        operation,
        context,
        origin,
        i64::from(error.retryable()),
    ];
    let mut results = [0 as SpectraHostValue; 1];
    let mut ctx = SpectraHostCallContext {
        args: args.as_ptr(),
        arg_len: args.len(),
        results: results.as_mut_ptr(),
        result_len: results.len(),
        invoke_fn: None,
    };
    if new_error(&mut ctx) != HOST_STATUS_SUCCESS {
        return 0;
    }
    results[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_codes_are_stable() {
        assert_eq!(AgentError::InvalidSpec("x".into()).kind(), "invalid_spec");
        assert_eq!(
            AgentError::InvalidSpec("x".into()).code(),
            ERROR_CODE_INVALID_ARGUMENT
        );
        assert_eq!(
            AgentError::UnknownHandle("x".into()).code(),
            ERROR_CODE_NOT_FOUND
        );
        assert_eq!(
            AgentError::EmbeddingNotConfigured("x".into()).kind(),
            "embedding_not_configured"
        );
        assert_eq!(
            AgentError::Provider("x".into()).code(),
            ERROR_CODE_IO
        );
        assert!(AgentError::Provider("x".into()).retryable());
        assert!(!AgentError::InvalidSpec("x".into()).retryable());
    }

    /// Invokes a registered scalar host accessor on one record argument.
    fn accessor(name: &str, record: SpectraHostValue) -> SpectraHostValue {
        let function = lookup_host_function(name).expect("runtime accessor registered");
        let args = [record];
        let mut results = [0 as SpectraHostValue; 1];
        let mut ctx = SpectraHostCallContext {
            args: args.as_ptr(),
            arg_len: args.len(),
            results: results.as_mut_ptr(),
            result_len: results.len(),
            invoke_fn: None,
        };
        assert_eq!(function(&mut ctx), HOST_STATUS_SUCCESS);
        results[0]
    }

    #[test]
    fn error_records_round_trip_through_std_error() {
        // The record layout is owned by the runtime's `std.error.new`; the
        // agent error must be readable through the public accessors, exactly
        // like a `std.fs` failure.
        let _guard = crate::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        spectra_runtime::register();
        let value = unsafe { tagged_err(&AgentError::UnknownHandle("run 7".into())) };
        assert_ne!(value, 0);
        let record = unsafe { *(value as *const i64).add(1) };
        assert_ne!(record, 0);
        assert_eq!(accessor("spectra.std.error.code", record), ERROR_CODE_NOT_FOUND);
        let message = accessor("spectra.std.error.message", record);
        let message = abi::read_string_arg(message).expect("message is a string");
        assert!(message.contains("unknown_handle"), "message: {message}");
    }
}
