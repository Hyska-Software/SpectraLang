//! Tool registry and governed wrapper invocation (R-3222 T3/T5).
//!
//! The compiler synthesizes one marshalling wrapper per `#[agent_tool]`
//! function and a per-module registration function that hands each wrapper's
//! address to `spectra.std.agent.register_tool`. This module owns the
//! process-local registry and the single invocation path every dispatcher
//! (`act`, `tool_call`, and later the MCP/A2A adapters) uses. Compiled local
//! tools are visible to every live run; remote MCP descriptors retain the run
//! that discovered them and are filtered at every model, protocol and dispatch
//! boundary.
//!
//! Invocation reuses the proven callback ABI (ADR 0019): a wrapper is an
//! `extern "C" fn(run: i64, args_json: *const u8, out_slot: *mut i64) -> i64`
//! reached through a raw address. The wrapper decodes the JSON arguments with
//! the derived `from_json`, calls the tool, encodes the result with
//! `to_json`, and writes a packed string (the JSON result on success, the
//! typed error message on failure) into the out-slot.

use std::collections::{BTreeMap, BTreeSet};
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

/// A tool served by a remote MCP server (R-3218 T1).
///
/// The entry carries the endpoint and the peer's own tool name; the runtime
/// never executes it as code, it forwards `tools/call` over the run's injected
/// HTTP transport through [`crate::mcp::client::call_tool`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteTool {
    /// The endpoint URL the tool was discovered at.
    pub(crate) url: String,
    /// The per-server capability derived from the endpoint identity
    /// (`mcp.<authority>`), which is also the tool's derived effect.
    pub(crate) server: String,
    /// The tool name as the remote server knows it (the registry key is the
    /// namespaced name this process dispatches).
    pub(crate) remote_name: String,
}

/// One registered tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegisteredTool {
    pub(crate) name: String,
    /// Address of the synthesized marshalling wrapper. Zero for a remote MCP
    /// tool, whose body lives in another process.
    pub(crate) address: i64,
    pub(crate) description: String,
    /// JSON Schema of the tool's single payload argument.
    pub(crate) input_schema: String,
    /// Host-call names reachable from the tool body, derived by the compiler.
    /// A remote MCP tool derives the per-server capability instead.
    pub(crate) effects: Vec<String>,
    /// Set when the tool is served by a remote MCP server (R-3218).
    pub(crate) remote: Option<RemoteTool>,
    /// Live run handles whose MCP discovery installed this remote descriptor.
    /// Local compiled tools leave this empty and are visible to every run.
    pub(crate) remote_runs: BTreeSet<i64>,
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
///
/// A descriptor that cannot be advertised is refused, naming the field: every
/// consumer of the registry (the MCP server's `tools/list`, the A2A card, the
/// provider request) serves the schema as the tool's argument contract, so a
/// permissive substitute would invite calls the wrapper rejects and hide the
/// broken descriptor behind them.
pub(crate) fn register(
    name: String,
    address: i64,
    description: String,
    input_schema: String,
    effects_json: &str,
) -> Result<bool, AgentError> {
    if name.is_empty() {
        return Err(AgentError::ToolFailed(
            "a tool cannot be registered without a name".to_string(),
        ));
    }
    if address == 0 {
        return Err(AgentError::ToolFailed(format!(
            "tool '{name}' has no wrapper address; the compiler emitted an empty one"
        )));
    }
    match serde_json::from_str::<serde_json::Value>(&input_schema) {
        Ok(serde_json::Value::Object(_)) => {}
        Ok(other) => {
            return Err(AgentError::ToolFailed(format!(
                "tool '{name}' has an input schema that is not a JSON object ({other}); the \
                 descriptor cannot be advertised"
            )))
        }
        Err(error) => {
            return Err(AgentError::ToolFailed(format!(
                "tool '{name}' has an input schema that is not JSON ({error}); the descriptor \
                 cannot be advertised"
            )))
        }
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
            Ok(false)
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
                    remote: None,
                    remote_runs: BTreeSet::new(),
                },
            );
            Ok(true)
        }
    }
}

/// Registers one remote MCP tool under this process's namespaced name
/// (R-3218 T1), idempotently.
///
/// The entry's effect is the per-server capability `mcp.<authority>`, so
/// `enforce_run_grant` refuses a run whose grant does not name that server:
/// a tool discovered by one run is visible only to runs that discovered it and
/// is never callable by a run that did not grant its server. Re-registering the
/// same name refreshes the peer metadata and adds the discovering run; a name
/// already owned by a compiled local tool is never replaced, because the local
/// wrapper is this process's own code and a remote peer must not be able to
/// shadow it.
pub(crate) fn register_remote(
    run_handle: i64,
    name: String,
    remote: RemoteTool,
    description: String,
    input_schema: String,
) -> bool {
    if name.is_empty() || remote.url.is_empty() || remote.remote_name.is_empty() {
        return false;
    }
    let effects = vec![remote.server.clone()];
    let mut remote_runs = BTreeSet::new();
    remote_runs.insert(run_handle);
    let mut tools = lock();
    match tools.get_mut(&name) {
        Some(existing) if existing.remote.is_none() => false,
        Some(existing) => {
            existing.description = description;
            existing.input_schema = input_schema;
            existing.effects = effects;
            existing.remote = Some(remote);
            existing.remote_runs.insert(run_handle);
            false
        }
        None => {
            tools.insert(
                name.clone(),
                RegisteredTool {
                    name,
                    address: 0,
                    description,
                    input_schema,
                    effects,
                    remote: Some(remote),
                    remote_runs,
                },
            );
            true
        }
    }
}

/// Registered tools in deterministic (name) order.
#[cfg(test)]
pub(crate) fn registered() -> Vec<RegisteredTool> {
    lock().values().cloned().collect()
}
/// The model-facing descriptions of tools visible to `run_handle`.
pub(crate) fn definitions_for(run_handle: i64) -> Result<Vec<ToolDefinition>, AgentError> {
    registered_for(run_handle).map(|tools| {
        tools
            .into_iter()
            .map(|tool| ToolDefinition {
                name: tool.name,
                description: tool.description,
                input_schema: tool.input_schema,
            })
            .collect()
    })
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

/// Effect name standing in for an effect list that could not be read.
///
/// Registration must not fail on it (the compiler emits the list, so a
/// malformed one is a bug in the emitter, not a reason to refuse the whole
/// module), but the tool's authority is *unknown* — and unknown authority is
/// the one thing [`enforce_run_grant`] must not authorize. The marker is not a
/// host-call name, so [`policy::grant_matches`] never covers it and the
/// dispatch is refused until the emitter is fixed.
const UNREADABLE_EFFECTS: &str = "<unreadable effects>";

/// Parses the compiler-emitted JSON array of effect names.
///
/// A malformed document cannot be produced by the compiler (the list is built
/// from literal host-call names); when it happens anyway the tool registers
/// with [`UNREADABLE_EFFECTS`], so the grant check refuses it instead of
/// finding nothing to enforce.
fn parse_effects(effects_json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(effects_json)
        .unwrap_or_else(|_| vec![UNREADABLE_EFFECTS.to_string()])
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
/// Resolves a tool that is visible to `run_handle`.
///
/// Local compiled tools are process-wide. A remote descriptor is installed by
/// MCP discovery for a specific run and must not be callable, advertised, or
/// grant-checked from an unrelated run.
pub(crate) fn lookup_for_run(run_handle: i64, name: &str) -> Result<RegisteredTool, AgentError> {
    // Preserve the dispatcher contract: an unknown name is reported before
    // validating the run handle, just as the unscoped lookup did.
    let tool = lookup(name)?;
    let run_id = run::with_run(run_handle, |state| state.run_id.clone())?;
    if !visible_to_run(&tool, run_handle) {
        return Err(AgentError::UnknownTool(format!(
            "remote tool '{name}' was not discovered for run '{run_id}'"
        )));
    }
    Ok(tool)
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
    let tool = lookup_for_run(run_handle, name)?;
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
pub(crate) fn dispatch(run_handle: i64, name: &str, arguments: &str) -> Result<String, AgentError> {
    let tool = lookup_for_run(run_handle, name)?;
    budget::charge_tool_call(run_handle)?;
    call_wrapper(run_handle, &tool, arguments)
}

/// Reaches `tool`'s marshalling wrapper by address and returns its JSON result
/// or typed failure (ADR 0019).
///
/// A remote MCP tool has no wrapper: its body lives in another process, so the
/// invocation is one `tools/call` over the run's injected HTTP transport
/// (R-3218 T1). Journaling, budget and tracing stay with the entry points
/// above, so both kinds of tool share one dispatch path.
///
/// No accounting, journaling or tracing: those belong to the entry points
/// above, so both share one wrapper-invocation path.
fn call_wrapper(
    run_handle: i64,
    tool: &RegisteredTool,
    arguments: &str,
) -> Result<String, AgentError> {
    if let Some(remote) = &tool.remote {
        return crate::mcp::client::call_tool(run_handle, remote, &tool.name, arguments);
    }
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
    for tool in registered_for(run_handle)? {
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
fn visible_to_run(tool: &RegisteredTool, run_handle: i64) -> bool {
    tool.remote
        .as_ref()
        .map(|_| tool.remote_runs.contains(&run_handle))
        .unwrap_or(true)
}

/// Registered tools visible to `run_handle` in deterministic (name) order.
pub(crate) fn registered_for(run_handle: i64) -> Result<Vec<RegisteredTool>, AgentError> {
    run::with_run(run_handle, |_| ())?;
    Ok(lock()
        .values()
        .filter(|tool| visible_to_run(tool, run_handle))
        .cloned()
        .collect())
}

/// Removes a run's remote descriptors after its generational handle is released.
///
/// A replayed run gets its descriptors back when MCP journal replay calls
/// `register_remote` with the new live handle. Removing the last visible run
/// also drops the stale process-global entry instead of retaining unbounded
/// run identities.
pub(crate) fn forget_run(run_handle: i64) {
    let mut tools = lock();
    tools.retain(|_, tool| {
        if tool.remote.is_none() {
            return true;
        }
        tool.remote_runs.remove(&run_handle);
        !tool.remote_runs.is_empty()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::AgentSpec;

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
            ).expect("register"));
            // Same name and address: no change.
            assert!(!register(
                "add".to_string(),
                0x1000,
                "adds".to_string(),
                "{}".to_string(),
                "[]"
            ).expect("register"));
            // A new address for the same name replaces the stale entry.
            assert!(register(
                "add".to_string(),
                0x2000,
                "adds".to_string(),
                "{}".to_string(),
                "[]"
            ).expect("register"));
            assert_eq!(registered().len(), 1);
            let tool = registered().pop().expect("registered");
            assert_eq!(tool.address, 0x2000);
            // A zero address is never a valid wrapper, and the refusal says so
            // instead of reporting "nothing changed".
            let refused = register(
                "bad".to_string(),
                0,
                String::new(),
                "{}".to_string(),
                "[]",
            )
            .expect_err("a wrapper-less tool is refused");
            assert_eq!(refused.kind(), "tool_failed");
            assert!(refused.detail().contains("wrapper address"), "{refused}");
            assert_eq!(registered().len(), 1, "the registry is unchanged");
        });
    }

    /// A descriptor that cannot be advertised is refused at registration: no
    /// consumer ever sees a permissive substitute for a broken schema.
    #[test]
    fn a_tool_with_an_unusable_input_schema_is_refused() {
        with_registry(|| {
            let refused = register(
                "broken".to_string(),
                0x1000,
                "has a schema nobody can read".to_string(),
                "{not json".to_string(),
                "[]",
            )
            .expect_err("a broken schema is refused");
            assert_eq!(refused.kind(), "tool_failed");
            assert!(refused.detail().contains("not JSON"), "{refused}");
            assert!(registered().is_empty(), "nothing entered the registry");

            // Valid JSON that is not an object is refused too: the consumer
            // needs a schema object, not any document.
            let refused = register(
                "array-schema".to_string(),
                0x1000,
                "schema is an array".to_string(),
                "[1,2]".to_string(),
                "[]",
            )
            .expect_err("a non-object schema is refused");
            assert!(refused.detail().contains("not a JSON object"), "{refused}");
            assert!(registered().is_empty());
        });
    }

    /// A tool whose compiled effect list could not be read holds unknown
    /// authority, and unknown authority is never granted.
    #[test]
    fn a_tool_with_an_unreadable_effect_list_is_refused_by_every_grant() {
        with_registry(|| {
            // The compiler emits a JSON array; this one is truncated, which is
            // the shape a broken emitter or a third-party registration would
            // produce.
            assert!(register(
                "mystery".to_string(),
                0x1000,
                "registers with an effect list nobody can read".to_string(),
                "{}".to_string(),
                r#"["spectra.std.fs.fs_write""#,
            ).expect("register"));
            let tool = registered().pop().expect("registered");
            assert_eq!(tool.effects, vec![UNREADABLE_EFFECTS.to_string()]);

            // Even a grant that covers the whole standard library does not
            // cover the marker, so the dispatch is refused rather than
            // silently authorized with no effects to check.
            let spec = AgentSpec::parse(
                r#"{"goal":"unreadable","model":"mock/echo","endpoint":"mock:","allow":["spectra.std","spectra"]}"#,
            )
            .expect("valid spec");
            let run = run::alloc_run(spec, "run-unreadable-effects".to_string(), None)
                .expect("alloc run");
            let error = enforce_run_grant(run).expect_err("unknown authority is not grantable");
            assert_eq!(error.kind(), "capability_denied");
            assert!(error.detail().contains(UNREADABLE_EFFECTS), "{error}");
            run::take_run(run).expect("end");
        });
    }

    #[test]
    fn remote_tools_are_scoped_to_their_discovery_run() {
        with_registry(|| {
            assert!(register(
                "local".to_string(),
                0x1000,
                "local".to_string(),
                "{}".to_string(),
                "[]",
            )
            .expect("register local"));

            let first = run::alloc_run(
                AgentSpec::parse(
                    r#"{"goal":"first","model":"mock/echo","endpoint":"mock:","allow":["mcp.evil.test"]}"#,
                )
                .expect("first spec"),
                "remote-run".to_string(),
                None,
            )
            .expect("first run");
            let second = run::alloc_run(
                AgentSpec::parse(
                    r#"{"goal":"second","model":"mock/echo","endpoint":"mock:","allow":[]}"#,
                )
                .expect("second spec"),
                "local-run".to_string(),
                None,
            )
            .expect("second run");

            assert!(register_remote(
                first,
                "mcp__evil_test__echo".to_string(),
                RemoteTool {
                    url: "http://evil.test/mcp".to_string(),
                    server: "mcp.evil.test".to_string(),
                    remote_name: "echo".to_string(),
                },
                "remote".to_string(),
                "{}".to_string(),
            ));
            assert_eq!(registered_for(first).expect("first registry").len(), 2);
            assert_eq!(registered_for(second).expect("second registry").len(), 1);
            assert_eq!(
                definitions_for(second).expect("second definitions").len(),
                1
            );
            assert!(enforce_run_grant(second).is_ok());
            let hidden =
                lookup_for_run(second, "mcp__evil_test__echo").expect_err("hidden remote tool");
            assert_eq!(hidden.kind(), "unknown_tool");

            run::take_run(first).expect("end first");
            run::take_run(second).expect("end second");
            assert_eq!(registered().len(), 1, "ended runs leave no remote entries");
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

