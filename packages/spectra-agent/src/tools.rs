//! Tool registry and governed wrapper invocation (R-3222 T3/T5).
//!
//! The compiler synthesizes one marshalling wrapper per `#[agent_tool]`
//! function and a per-module registration function that hands each wrapper's
//! address to `spectra.std.agent.register_tool`. This module owns the
//! resulting process-wide registry and the single invocation path every
//! dispatcher (`act`, `tool_call`, and later the MCP/A2A adapters) uses.
//!
//! Invocation reuses the proven callback ABI (ADR 0019): a wrapper is an
//! `extern "C" fn(run: i64, args_json: *const u8, out_slot: *mut i64) -> i64`
//! reached through a raw address. The wrapper decodes the JSON arguments with
//! the derived `from_json`, calls the tool, encodes the result with
//! `to_json`, and writes a packed string (the JSON result on success, the
//! typed error message on failure) into the out-slot.

use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex, MutexGuard};

use crate::abi;
use crate::budget;
use crate::error::AgentError;
use crate::journal::StepUsage;
use crate::policy;
use crate::provider::ToolDefinition;
use crate::replay::{self, Kind, Resolved};
use crate::run;
use crate::trace;

/// One registered tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegisteredTool {
    pub(crate) name: String,
    /// Address of the synthesized marshalling wrapper.
    pub(crate) address: i64,
    pub(crate) description: String,
    /// JSON Schema of the tool's single payload argument.
    pub(crate) input_schema: String,
    /// Host-call names reachable from the tool body, derived by the compiler.
    pub(crate) effects: Vec<String>,
}

fn registry() -> &'static Mutex<BTreeMap<String, RegisteredTool>> {
    static REGISTRY: LazyLock<Mutex<BTreeMap<String, RegisteredTool>>> =
        LazyLock::new(|| Mutex::new(BTreeMap::new()));
    &REGISTRY
}

fn lock() -> MutexGuard<'static, BTreeMap<String, RegisteredTool>> {
    registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Registers one tool, idempotently by name.
///
/// Returns `true` when the registry changed. Re-registering the same name with
/// the same address (several dispatch entry points call the registration
/// function) is a no-op; a different address replaces the stale entry, because
/// the newest compilation of that module is the one whose code is live.
pub(crate) fn register(
    name: String,
    address: i64,
    description: String,
    input_schema: String,
    effects_json: &str,
) -> bool {
    if name.is_empty() || address == 0 {
        return false;
    }
    let effects = parse_effects(effects_json);
    let mut tools = lock();
    match tools.get_mut(&name) {
        Some(existing) if existing.address == address => {
            // Refresh metadata: the same tool registered from another entry
            // point carries identical compiler-derived values.
            existing.description = description;
            existing.input_schema = input_schema;
            existing.effects = effects;
            false
        }
        _ => {
            tools.insert(
                name.clone(),
                RegisteredTool {
                    name,
                    address,
                    description,
                    input_schema,
                    effects,
                },
            );
            true
        }
    }
}

/// Registered tools in deterministic (name) order.
pub(crate) fn registered() -> Vec<RegisteredTool> {
    lock().values().cloned().collect()
}

/// The model-facing descriptions of every registered tool.
pub(crate) fn definitions() -> Vec<ToolDefinition> {
    registered()
        .into_iter()
        .map(|tool| ToolDefinition {
            name: tool.name,
            description: tool.description,
            input_schema: tool.input_schema,
        })
        .collect()
}

/// Empties the registry. Used by tests that must observe a fresh process.
#[cfg(test)]
pub(crate) fn clear() {
    lock().clear();
}

/// Serializes crate tests that mutate the process-global tool registry.
///
/// Shared with the replay tests, which register a counting wrapper: one lock
/// per process-global resource, so a test in another module cannot clear the
/// registry while a wrapper is being invoked.
#[cfg(test)]
pub(crate) static REGISTRY_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Parses the compiler-emitted JSON array of effect names.
///
/// A malformed document cannot be produced by the compiler (the list is built
/// from literal host-call names), so it degrades to "no effects" instead of
/// failing registration; the grant check then has nothing to enforce rather
/// than blocking a tool that is legitimately exposed.
fn parse_effects(effects_json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(effects_json).unwrap_or_default()
}

/// Resolves a tool by name.
///
/// `pub(crate)` so `compensate` can validate a declared tool name against the
/// registry at declaration time (R-3224 T1), before anything is journaled.
pub(crate) fn lookup(name: &str) -> Result<RegisteredTool, AgentError> {
    let tools = lock();
    match tools.get(name) {
        Some(tool) => Ok(tool.clone()),
        None => {
            let known = tools.keys().cloned().collect::<Vec<_>>().join(", ");
            Err(AgentError::UnknownTool(format!(
                "no tool named '{name}' is registered; registered tools: [{}]",
                known
            )))
        }
    }
}

/// Invokes `name` through its marshalling wrapper.
///
/// The tool call is charged against the run's `max_tool_calls` ceiling
/// **before** the wrapper runs, so the (max+1)-th call is denied without
/// executing. Every failure below the ABI (unknown tool, malformed arguments,
/// a failing tool) is a typed error the caller reports back to the model.
///
/// R-3217 T1/T2: the invocation is a journaled step. A replayed step returns
/// the recorded result without reaching the wrapper — the tool body does not
/// run twice — while the charge is still applied, so a replayed run's report
/// and ceiling behavior are identical to the original.
pub(crate) fn invoke(run_handle: i64, name: &str, arguments: &str) -> Result<String, AgentError> {
    let tool = lookup(name)?;
    budget::charge_tool_call(run_handle)?;
    let (run_id, goal) = run::with_run(run_handle, |state| {
        (state.run_id.clone(), state.spec.goal.clone())
    })?;
    let input = format!("tool\0{}\0{}", tool.name, arguments);
    let token = match replay::resolve(run_handle, Kind::Tool, &input)? {
        Resolved::Recorded(record) => {
            let result = record.output.clone();
            trace::emit_execute_tool(
                &record.run,
                &goal,
                record.step,
                &tool.name,
                None,
                Some(&result),
            );
            return Ok(result);
        }
        Resolved::Fresh(token) => token,
    };

    let message = call_wrapper(run_handle, &tool, arguments)?;
    // The result is durable before it is returned to the dispatcher.
    replay::commit(
        run_handle,
        &token,
        Some(&input),
        &message,
        StepUsage::default(),
        -1,
        None,
    )?;
    trace::emit_execute_tool(
        &run_id,
        &goal,
        token.step,
        &tool.name,
        Some(arguments),
        Some(&message),
    );
    Ok(message)
}

/// The governed dispatch core: resolve the tool, charge the run's tool-call
/// ceiling, and reach its marshalling wrapper (R-3224 T2).
///
/// [`invoke`] is this core wrapped in the R-3217 replay/journal discipline;
/// the compensation runner calls it once per journaled `rollback` step,
/// because a compensation must execute through exactly the same path — the
/// capability check its caller performs, the taint gate inside the wrapper's
/// own host calls, and the budget charge here. It deliberately performs no
/// replay resolution: `rollback` reserves its own step so a *failed*
/// compensation is recorded (unlike a model-driven tool call, which R-3222
/// leaves unrecorded on failure).
pub(crate) fn dispatch(
    run_handle: i64,
    name: &str,
    arguments: &str,
) -> Result<String, AgentError> {
    let tool = lookup(name)?;
    budget::charge_tool_call(run_handle)?;
    call_wrapper(run_handle, &tool, arguments)
}

/// Reaches `tool`'s marshalling wrapper by address and returns its JSON result
/// or typed failure (ADR 0019).
///
/// No accounting, journaling or tracing: those belong to the entry points
/// above, so both share one wrapper-invocation path.
fn call_wrapper(
    run_handle: i64,
    tool: &RegisteredTool,
    arguments: &str,
) -> Result<String, AgentError> {
    let args_pointer = unsafe { abi::alloc_string(arguments) };
    if args_pointer == 0 {
        return Err(AgentError::Internal(
            "could not allocate the tool argument document".to_string(),
        ));
    }
    let out_slot = unsafe { abi::alloc_words(1) };
    if out_slot.is_null() {
        return Err(AgentError::Internal(
            "could not allocate the tool result slot".to_string(),
        ));
    }

    // ADR 0019: the wrapper is reached by address with the same i64 callback
    // convention `CoroutineCreate` uses for poll/drop.
    let wrapper: unsafe extern "C" fn(i64, i64, i64) -> i64 =
        unsafe { std::mem::transmute(tool.address as usize) };
    let status = unsafe { wrapper(run_handle, args_pointer, out_slot as i64) };
    let message = abi::read_string_arg(unsafe { *out_slot }).unwrap_or_default();
    if status == 0 {
        Ok(message)
    } else {
        Err(AgentError::ToolFailed(if message.is_empty() {
            format!("tool '{}' failed without a message", tool.name)
        } else {
            message
        }))
    }
}

/// Fails the dispatch when a registered tool's derived effects are not covered
/// by the run's grant (R-3222 T5 / I7).
///
/// Checked once, before the first dispatch of a run: a spec that exposes tools
/// whose effects it does not grant would otherwise authorize work outside its
/// declared authority, which is exactly the bypass invariant I7 forbids. The
/// agent's own primitives (`spectra.std.agent.*`) are excluded: a run may
/// always call the loop that ends it.
///
/// The compiler's own machinery is excluded as well (R-3223): the coroutine
/// ABI and the JSON/format helpers the compiler emits around the author's code
/// are not effects the author chose, so a tool that compares strings does not
/// make its spec grant the compiler's helper. See
/// [`crate::policy::is_compiler_emitted`].
pub(crate) fn enforce_run_grant(run_handle: i64) -> Result<(), AgentError> {
    let grants = run::with_run(run_handle, |state| state.spec.allow.clone())?;
    for tool in registered() {
        for effect in &tool.effects {
            if policy::is_compiler_emitted(effect) {
                continue;
            }
            if !grants
                .iter()
                .any(|grant| policy::grant_matches(grant, effect))
            {
                return Err(AgentError::CapabilityDenied(format!(
                    "tool '{}' has effect '{}', which the run grant [{}] does not authorize",
                    tool.name,
                    effect,
                    grants.join(", ")
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes tests that mutate the process-global registry.
    fn with_registry<T>(work: impl FnOnce() -> T) -> T {
        let _guard = REGISTRY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear();
        let result = work();
        clear();
        result
    }

    #[test]
    fn registration_is_idempotent_by_name_and_address() {
        with_registry(|| {
            assert!(register(
                "add".to_string(),
                0x1000,
                "adds".to_string(),
                "{}".to_string(),
                r#"["spectra.std.agent.token_count"]"#
            ));
            // Same name and address: no change.
            assert!(!register(
                "add".to_string(),
                0x1000,
                "adds".to_string(),
                "{}".to_string(),
                "[]"
            ));
            // A new address for the same name replaces the stale entry.
            assert!(register(
                "add".to_string(),
                0x2000,
                "adds".to_string(),
                "{}".to_string(),
                "[]"
            ));
            assert_eq!(registered().len(), 1);
            let tool = registered().pop().expect("registered");
            assert_eq!(tool.address, 0x2000);
            // A zero address is never a valid wrapper.
            assert!(!register(
                "bad".to_string(),
                0,
                String::new(),
                "{}".to_string(),
                "[]"
            ));
            assert_eq!(registered().len(), 1);
        });
    }

    #[test]
    fn an_unknown_tool_is_a_typed_error() {
        with_registry(|| {
            let error = invoke(1, "missing", "{}").expect_err("unknown tool");
            assert_eq!(error.kind(), "unknown_tool");
            assert!(error.detail().contains("missing"), "{error}");
        });
    }
}
