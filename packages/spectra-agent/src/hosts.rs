//! Host functions for the `std.agent` surface (R-3211 T3).
//!
//! Sync entry points (`agent_start`, `agent_end`) return a tagged
//! `Result<T, Error>`; IO entry points (`ask`, `ask_json`, `ask_stream`,
//! `stream_next`, `stream_close`, `embed`) return a runtime task handle whose
//! value is the same tagged `Result`, so compiled `await`/`block_on` drives
//! them (plan adaptation 3).

use spectra_runtime::ffi::{
    lookup_host_function, SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INTERNAL_ERROR,
    HOST_STATUS_INVALID_ARGUMENT, HOST_STATUS_SUCCESS,
};
use spectra_runtime::stdlib::CancellationToken;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::abi;
use crate::budget;
use crate::error::{self, AgentError};
use crate::provider::{self, Message, ProviderRequest, SamplingParams};
use crate::run;
use crate::schema;
use crate::spec::AgentSpec;

// ── shared helpers ───────────────────────────────────────────────────────

/// Writes one tagged `Result` produced by a sync operation.
pub(crate) fn write_outcome(ctx: *mut SpectraHostCallContext, outcome: Result<i64, AgentError>) -> i32 {
    let value = match outcome {
        Ok(payload) => unsafe { error::tagged_ok(payload) },
        Err(error) => unsafe { error::tagged_err(&error) },
    };
    if value == 0 {
        // Allocation failure inside the runtime arena: fail loudly rather
        // than hand compiled code a null `Result`.
        return HOST_STATUS_INTERNAL_ERROR;
    }
    abi::write_value(ctx, value)
}

/// Spawns `work` on the background executor and writes the task handle.
fn write_task<F>(ctx: *mut SpectraHostCallContext, work: F) -> i32
where
    F: FnOnce() -> Result<i64, AgentError> + Send + 'static,
{
    let spawned = spectra_runtime::stdlib::spawn_background_task(move || {
        match work() {
            Ok(payload) => {
                let value = unsafe { error::tagged_ok(payload) };
                if value == 0 {
                    Err(())
                } else {
                    Ok(value)
                }
            }
            Err(error) => {
                let value = unsafe { error::tagged_err(&error) };
                if value == 0 {
                    Err(())
                } else {
                    Ok(value)
                }
            }
        }
    });
    match spawned {
        Ok(task) => abi::write_value(ctx, task),
        Err(status) => status,
    }
}

/// Spawns run-scoped work on the background executor with the run's budget
/// applied at both boundaries (R-3216 T2).
///
/// The run records the task's cancellation token before the job is queued, a
/// ceiling crossing sets that token, and the work observes the cancellation
/// when it starts (it never begins work for a cancelled run) and when it
/// finishes (it never delivers a value from a cancelled run). The token is
/// removed from the run when the task finishes, by identity, so a later
/// cancellation never touches an unrelated task's token.
fn write_run_task<F>(ctx: *mut SpectraHostCallContext, run_handle: i64, work: F) -> i32
where
    F: FnOnce() -> Result<i64, AgentError> + Send + 'static,
{
    let token = CancellationToken::default();
    run::register_task(run_handle, Arc::clone(&token));
    let worker_token = Arc::clone(&token);
    let spawned = spectra_runtime::stdlib::spawn_cancellable_background_task(
        move || {
            let outcome = (|| -> Result<i64, AgentError> {
                budget::guard_task(run_handle, &worker_token)?;
                let value = work()?;
                budget::settle_task(run_handle, &worker_token)?;
                Ok(value)
            })();
            run::unregister_task(run_handle, &worker_token);
            match outcome {
                Ok(payload) => {
                    let value = unsafe { error::tagged_ok(payload) };
                    if value == 0 {
                        Err(())
                    } else {
                        Ok(value)
                    }
                }
                Err(error) => {
                    let value = unsafe { error::tagged_err(&error) };
                    if value == 0 {
                        Err(())
                    } else {
                        Ok(value)
                    }
                }
            }
        },
        {
            let cancel_token = Arc::clone(&token);
            move || cancel_token.store(true, Ordering::Release)
        },
    );
    match spawned {
        Ok(task) => abi::write_value(ctx, task),
        Err(status) => {
            // The job was never queued: the run must not keep a token that no
            // task will ever remove.
            run::unregister_task(run_handle, &token);
            status
        }
    }
}

pub(crate) fn read_run_and_prompt(
    ctx: *mut SpectraHostCallContext,
    expected: usize,
) -> Option<(i64, Vec<String>)> {
    let args = abi::args(ctx)?;
    if args.len() != expected {
        return None;
    }
    let run = args[0];
    let mut strings = Vec::with_capacity(expected - 1);
    for value in &args[1..] {
        strings.push(abi::read_string_arg(*value)?);
    }
    Some((run, strings))
}

/// Snapshots the run's spec and its effective sampling parameters.
pub(crate) fn snapshot(run_handle: i64) -> Result<(AgentSpec, SamplingParams), AgentError> {
    run::with_run(run_handle, |state| {
        (state.spec.clone(), SamplingParams::effective(&state.spec))
    })
}

/// Resolves the provider and applies the two fail-closed gates: an
/// unconfigured provider, and a deterministic run whose provider cannot honor
/// the seed.
pub(crate) fn resolve_provider(
    spec: &AgentSpec,
) -> Result<Box<dyn provider::Provider>, AgentError> {
    let provider = provider::provider_for(spec);
    if !provider.is_configured() {
        return Err(AgentError::ProviderNotConfigured(format!(
            "provider '{}' is not configured for model '{}'",
            provider.name(),
            spec.model
        )));
    }
    if spec.deterministic() && !provider.honors_seed() {
        return Err(AgentError::DeterministicUnavailable(format!(
            "provider '{}' cannot honor seed {}",
            provider.name(),
            spec.seed
        )));
    }
    Ok(provider)
}

/// One provider request from the run's spec plus an explicit transcript.
///
/// `ask`/`ask_json`/`ask_stream` pass a single user message; the `act` loop
/// (R-3222) appends assistant and tool-result messages as it runs and passes
/// the tool definitions it exposes to the model.
pub(crate) fn build_request(
    spec: &AgentSpec,
    sampling: SamplingParams,
    messages: Vec<Message>,
    json_schema: Option<String>,
    tools: Vec<provider::ToolDefinition>,
) -> ProviderRequest {
    ProviderRequest {
        model: spec.model.clone(),
        messages,
        temperature: sampling.temperature,
        top_k: sampling.top_k,
        seed: sampling.seed,
        json_schema,
        max_tokens: (spec.max_tokens > 0).then_some(spec.max_tokens as u64),
        tools,
    }
}

/// One model turn plus accounting. The turn is accounted for before the caller
/// inspects the payload: the tokens were spent even if the response is later
/// rejected by `ask_json` validation.
fn model_turn(
    run_handle: i64,
    prompt: &str,
    json_schema: Option<String>,
) -> Result<provider::ProviderResponse, AgentError> {
    model_turn_with(
        run_handle,
        vec![Message::user(prompt)],
        json_schema,
        Vec::new(),
    )
}

/// One model turn over an explicit transcript, with the tools exposed to the
/// model (R-3222). Accounting is identical to `model_turn`.
pub(crate) fn model_turn_with(
    run_handle: i64,
    messages: Vec<Message>,
    json_schema: Option<String>,
    tools: Vec<provider::ToolDefinition>,
) -> Result<provider::ProviderResponse, AgentError> {
    let (spec, sampling) = snapshot(run_handle)?;
    let provider = resolve_provider(&spec)?;
    let request = build_request(&spec, sampling, messages, json_schema, tools);
    match provider.complete(&request) {
        Ok(response) => {
            run::with_run(run_handle, |state| {
                state.record_turn(
                    response.usage.input_tokens,
                    response.usage.output_tokens,
                    response.cost_micros,
                    response.retries,
                )
            })?;
            Ok(response)
        }
        Err(error) => {
            let _ = run::with_run(run_handle, |state| state.mark_failed());
            Err(AgentError::from(error))
        }
    }
}

/// Allocates a rank-1 float tensor through the runtime's own tensor
/// constructor (`spectra.std.tensor.literal_f`). The values travel as raw
/// `f64` bit patterns, the same word the compiler emits for a float literal.
fn alloc_float_tensor(values: &[f64]) -> Result<i64, AgentError> {
    if values.is_empty() {
        return Err(AgentError::EmbeddingNotConfigured(
            "the provider returned an empty embedding".to_string(),
        ));
    }
    let Some(literal_f) = lookup_host_function("spectra.std.tensor.literal_f") else {
        return Err(AgentError::EmbeddingNotConfigured(
            "the runtime tensor constructor spectra.std.tensor.literal_f is not registered"
                .to_string(),
        ));
    };
    let mut args: Vec<SpectraHostValue> = Vec::with_capacity(values.len() + 1);
    args.push(values.len() as SpectraHostValue);
    args.extend(values.iter().map(|value| value.to_bits() as SpectraHostValue));
    let mut results = [0 as SpectraHostValue; 1];
    let mut ctx = SpectraHostCallContext {
        args: args.as_ptr(),
        arg_len: args.len(),
        results: results.as_mut_ptr(),
        result_len: results.len(),
        invoke_fn: None,
    };
    if literal_f(&mut ctx) != HOST_STATUS_SUCCESS || results[0] == 0 {
        return Err(AgentError::EmbeddingNotConfigured(
            "the runtime tensor constructor rejected the embedding".to_string(),
        ));
    }
    Ok(results[0])
}

// ── sync lifecycle hosts ─────────────────────────────────────────────────

/// `spectra.std.agent.agent_start(spec_json) -> Result<Run, Error>`.
extern "C" fn agent_start_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some(args) = abi::args(ctx) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if args.len() != 1 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let Some(spec_json) = abi::read_string_arg(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let outcome = (|| -> Result<i64, AgentError> {
        let spec = AgentSpec::parse(&spec_json)?;
        // R-3216 T1: a declared cost ceiling must be enforceable, so the run
        // fails closed here rather than accepting a ceiling that cannot fire.
        let provider = provider::provider_for(&spec);
        budget::cost_ceiling_is_enforceable(&spec, provider.reports_cost())?;
        let handle = run::alloc_run(spec)?;
        crate::policy::install();
        Ok(handle)
    })();
    write_outcome(ctx, outcome)
}

/// `spectra.std.agent.budget_remaining(run) -> Result<int, Error>`.
///
/// The integer is the number of **tokens remaining** before the run's
/// `max_tokens` ceiling (`input + output` tokens accrued, clamped at 0). When
/// the spec declares no token ceiling (`max_tokens == 0`, "unlimited"), the
/// sentinel `i64::MAX` is returned, so a caller can tell "unbounded" from
/// "exhausted" without guessing.
///
/// Introspection never fails because the run is cancelled: a cancelled run
/// reports 0 remaining so the caller can observe the state that cancelled it.
/// Effectful calls (`ask`, `ask_json`, `ask_stream`, `embed`, tool dispatch)
/// are the ones that return the typed `budget_exceeded` error.
extern "C" fn budget_remaining_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some(args) = abi::args(ctx) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if args.len() != 1 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    write_outcome(ctx, budget::remaining(args[0]))
}

/// `spectra.std.agent.agent_end(run) -> Result<string, Error>`.
extern "C" fn agent_end_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some(args) = abi::args(ctx) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if args.len() != 1 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let outcome = (|| -> Result<i64, AgentError> {
        let state = run::take_run(args[0])?;
        let report = state.report_json();
        crate::policy::uninstall_if_idle();
        let pointer = unsafe { abi::alloc_string(&report) };
        if pointer == 0 {
            return Err(AgentError::Internal(
                "could not allocate the report document".to_string(),
            ));
        }
        Ok(pointer)
    })();
    write_outcome(ctx, outcome)
}

// ── async model gateway hosts ────────────────────────────────────────────

/// `spectra.std.agent.ask(run, prompt) -> Task<Result<string, Error>>`.
extern "C" fn ask_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let prompt = strings[0].clone();
    write_run_task(ctx, run_handle, move || {
        let response = model_turn(run_handle, &prompt, None)?;
        let pointer = unsafe { abi::alloc_string(&response.text) };
        if pointer == 0 {
            return Err(AgentError::Internal("could not allocate the response".to_string()));
        }
        Ok(pointer)
    })
}

/// `spectra.std.agent.ask_json(run, prompt, schema) -> Task<Result<string, Error>>`.
extern "C" fn ask_json_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let prompt = strings[0].clone();
    let json_schema = strings[1].clone();
    write_run_task(ctx, run_handle, move || {
        let response = model_turn(run_handle, &prompt, Some(json_schema.clone()))?;
        // Provider-side constrained decoding is never trusted: the response
        // is validated here, independently of what the provider claims.
        schema::validate(&response.text, &json_schema)
            .map_err(AgentError::SchemaViolation)?;
        let pointer = unsafe { abi::alloc_string(&response.text) };
        if pointer == 0 {
            return Err(AgentError::Internal("could not allocate the response".to_string()));
        }
        Ok(pointer)
    })
}

/// `spectra.std.agent.ask_stream(run, prompt) -> Task<Result<ChunkStream, Error>>`.
extern "C" fn ask_stream_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let prompt = strings[0].clone();
    write_run_task(ctx, run_handle, move || {
        let (spec, sampling) = snapshot(run_handle)?;
        let provider = resolve_provider(&spec)?;
        let request = build_request(
            &spec,
            sampling,
            vec![Message::user(prompt.clone())],
            None,
            Vec::new(),
        );
        let stream = match provider.stream(&request) {
            Ok(stream) => stream,
            Err(error) => {
                let _ = run::with_run(run_handle, |state| state.mark_failed());
                return Err(AgentError::from(error));
            }
        };
        run::with_run(run_handle, |state| {
            state.record_turn(
                stream.usage.input_tokens,
                stream.usage.output_tokens,
                stream.cost_micros,
                stream.retries,
            )
        })?;
        run::alloc_stream(stream.chunks)
    })
}

/// `spectra.std.agent.stream_next(stream) -> Task<Result<string, Error>>`.
extern "C" fn stream_next_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some(args) = abi::args(ctx) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if args.len() != 1 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let stream = args[0];
    write_task(ctx, move || {
        let chunk = run::next_chunk(stream)?;
        let pointer = unsafe { abi::alloc_string(&chunk) };
        if pointer == 0 {
            return Err(AgentError::Internal("could not allocate the chunk".to_string()));
        }
        Ok(pointer)
    })
}

/// `spectra.std.agent.stream_close(stream) -> Task<Result<bool, Error>>`.
extern "C" fn stream_close_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some(args) = abi::args(ctx) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if args.len() != 1 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let stream = args[0];
    write_task(ctx, move || Ok(i64::from(run::close_stream(stream)?)))
}

/// `spectra.std.agent.embed(run, text) -> Task<Result<Tensor, Error>>`.
extern "C" fn embed_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let text = strings[0].clone();
    write_run_task(ctx, run_handle, move || {
        let (spec, _sampling) = snapshot(run_handle)?;
        let provider = resolve_provider(&spec)?;
        let vector = provider.embed(&text).map_err(AgentError::from)?;
        alloc_float_tensor(&vector)
    })
}

// ── tool dispatch (R-3222) ───────────────────────────────────────────────

/// `spectra.std.agent.act(run, prompt) -> Task<Result<string, Error>>`.
///
/// Runs model↔tool turns until the model answers without a tool call.
extern "C" fn act_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let prompt = strings[0].clone();
    write_run_task(ctx, run_handle, move || {
        let text = crate::act::act(run_handle, &prompt)?;
        let pointer = unsafe { abi::alloc_string(&text) };
        if pointer == 0 {
            return Err(AgentError::Internal(
                "could not allocate the answer".to_string(),
            ));
        }
        Ok(pointer)
    })
}

/// `spectra.std.agent.tool_call(run, name, args_json) -> Task<Result<string, Error>>`.
///
/// Invokes one registered `#[agent_tool]` through the governed dispatch.
extern "C" fn tool_call_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let name = strings[0].clone();
    let arguments = strings[1].clone();
    write_run_task(ctx, run_handle, move || {
        let result = crate::act::tool_call(run_handle, &name, &arguments)?;
        let pointer = unsafe { abi::alloc_string(&result) };
        if pointer == 0 {
            return Err(AgentError::Internal(
                "could not allocate the tool result".to_string(),
            ));
        }
        Ok(pointer)
    })
}

/// `spectra.std.agent.register_tool(name, wrapper_address, description,
/// input_schema, effects) -> Result<bool, Error>`.
///
/// Emitted only by compiler-synthesized registration functions (one call per
/// declared tool). The registry is idempotent by name, so several dispatch
/// entry points may call it.
extern "C" fn register_tool_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some(args) = abi::args(ctx) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if args.len() != 5 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let outcome = (|| -> Result<i64, AgentError> {
        let name = abi::read_string_arg(args[0])
            .ok_or_else(|| AgentError::ToolFailed("tool registration has no name".to_string()))?;
        let description = abi::read_string_arg(args[2]).unwrap_or_default();
        let input_schema = abi::read_string_arg(args[3]).unwrap_or_default();
        let effects = abi::read_string_arg(args[4]).unwrap_or_default();
        let changed = crate::tools::register(name, args[1], description, input_schema, &effects);
        Ok(i64::from(changed))
    })();
    write_outcome(ctx, outcome)
}

/// Registers every `spectra.std.agent.*` host function and returns the number
/// of newly inserted entries.
pub(crate) fn register() -> usize {
    let mut inserted = 0;
    for (name, function) in [
        ("spectra.std.agent.agent_start", agent_start_host as _),
        ("spectra.std.agent.agent_end", agent_end_host as _),
        ("spectra.std.agent.budget_remaining", budget_remaining_host as _),
        ("spectra.std.agent.ask", ask_host as _),
        ("spectra.std.agent.ask_json", ask_json_host as _),
        ("spectra.std.agent.ask_stream", ask_stream_host as _),
        ("spectra.std.agent.stream_next", stream_next_host as _),
        ("spectra.std.agent.stream_close", stream_close_host as _),
        ("spectra.std.agent.embed", embed_host as _),
        ("spectra.std.agent.act", act_host as _),
        ("spectra.std.agent.tool_call", tool_call_host as _),
        ("spectra.std.agent.register_tool", register_tool_host as _),
    ] {
        if spectra_runtime::ffi::register_host_function(name, function) {
            inserted += 1;
        }
    }
    inserted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::transport::{clear_http_transport, set_http_transport, HttpTransport, TransportResponse};
    use crate::provider::Provider;
    use spectra_runtime::stdlib::block_on_task_value;

    /// Runs one host function through its registered pointer.
    fn call(name: &str, args: &[SpectraHostValue]) -> (i32, SpectraHostValue) {
        let function = lookup_host_function(name).expect("host registered");
        let mut results = [0 as SpectraHostValue; 1];
        let mut ctx = SpectraHostCallContext {
            args: args.as_ptr(),
            arg_len: args.len(),
            results: results.as_mut_ptr(),
            result_len: results.len(),
            invoke_fn: None,
        };
        (function(&mut ctx), results[0])
    }

    /// Resolves a task host's result to the tagged `Result` pointer.
    fn awaited(name: &str, args: &[SpectraHostValue]) -> i64 {
        let (status, task) = call(name, args);
        assert_eq!(status, HOST_STATUS_SUCCESS, "{name} did not return a task");
        assert_ne!(task, 0, "{name} must return a real task handle");
        block_on_task_value(task).expect("task resolves")
    }

    fn tag(value: i64) -> i64 {
        unsafe { *(value as *const i64) }
    }

    fn payload(value: i64) -> i64 {
        unsafe { *(value as *const i64).add(1) }
    }

    fn error_message(value: i64) -> String {
        assert_eq!(tag(value), 1, "expected an Err result");
        let (status, message) = call("spectra.std.error.message", &[payload(value)]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        abi::read_string_arg(message).expect("message string")
    }

    fn spec_json(extra: &str) -> SpectraHostValue {
        let json = format!(r#"{{"goal":"g","model":"mock/echo","endpoint":"mock:"{extra}}}"#);
        unsafe { abi::alloc_string(&json) }
    }

    fn start(extra: &str) -> i64 {
        let (status, result) = call("spectra.std.agent.agent_start", &[spec_json(extra)]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_eq!(tag(result), 0);
        payload(result)
    }

    fn setup() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        spectra_runtime::ffi::clear_host_functions();
        spectra_runtime::register();
        // Nine R-3211 gateway functions plus the three R-3222 dispatch hosts.
        assert_eq!(register(), 12);
        guard
    }

    #[test]
    fn lifecycle_asks_and_report_counters() {
        let _guard = setup();
        let run = start("");
        let alpha = unsafe { abi::alloc_string("alpha") };
        let first = awaited("spectra.std.agent.ask", &[run, alpha]);
        assert_eq!(tag(first), 0);
        assert_eq!(abi::read_string_arg(payload(first)).as_deref(), Some("mock echo: alpha"));

        let beta = unsafe { abi::alloc_string("beta") };
        let second = awaited("spectra.std.agent.ask", &[run, beta]);
        assert_eq!(tag(second), 0);

        let (status, report) = call("spectra.std.agent.agent_end", &[run]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_eq!(tag(report), 0);
        let report = abi::read_string_arg(payload(report)).expect("report string");
        assert!(report.contains("\"status\":\"completed\""), "{report}");
        assert!(report.contains("\"steps\":2"), "{report}");
        assert!(report.contains("\"tokens_in\":2"), "{report}");
        assert!(report.contains("\"tokens_out\":6"), "{report}");
        assert!(report.contains("\"cost_micros\":14"), "{report}");
        assert!(report.contains("\"compensations_pending\":0"), "{report}");
    }

    #[test]
    fn double_end_and_use_after_end_are_typed_errors() {
        let _guard = setup();
        let run = start("");
        let (status, _) = call("spectra.std.agent.agent_end", &[run]);
        assert_eq!(status, HOST_STATUS_SUCCESS);

        let (status, second) = call("spectra.std.agent.agent_end", &[run]);
        assert_eq!(status, HOST_STATUS_SUCCESS, "a typed error, not a host trap");
        assert_eq!(tag(second), 1);
        assert!(
            error_message(second).contains("unknown_handle"),
            "{}",
            error_message(second)
        );

        // Use after end: the same handle is refused, never a panic.
        let prompt = unsafe { abi::alloc_string("hello") };
        let reused = awaited("spectra.std.agent.ask", &[run, prompt]);
        assert!(error_message(reused).contains("unknown_handle"));

        // A malformed handle is a typed error too.
        let (status, bogus) = call("spectra.std.agent.agent_end", &[7]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert!(error_message(bogus).contains("invalid_handle"));
    }

    #[test]
    fn ask_json_validates_client_side() {
        let _guard = setup();
        let run = start("");
        let schema = unsafe {
            abi::alloc_string(
                r#"{"type":"object","properties":{"count":{"type":"integer"},"label":{"type":"string"}},"required":["count","label"],"additionalProperties":false}"#,
            )
        };

        let good_prompt = unsafe { abi::alloc_string("please spectra:json") };
        let good = awaited("spectra.std.agent.ask_json", &[run, good_prompt, schema]);
        assert_eq!(tag(good), 0);
        assert_eq!(
            abi::read_string_arg(payload(good)).as_deref(),
            Some(r#"{"count":3,"label":"mock-response"}"#)
        );

        let bad_prompt = unsafe { abi::alloc_string("please spectra:invalid-json") };
        let bad = awaited("spectra.std.agent.ask_json", &[run, bad_prompt, schema]);
        let message = error_message(bad);
        assert!(message.contains("schema_violation"), "{message}");
        assert!(message.contains("$.count"), "{message}");

        // The rejected turn is still accounted for.
        let (_, report) = call("spectra.std.agent.agent_end", &[run]);
        let report = abi::read_string_arg(payload(report)).expect("report");
        assert!(report.contains("\"steps\":2"), "{report}");
    }

    #[test]
    fn streaming_delivers_chunks_ending_with_the_empty_chunk() {
        let _guard = setup();
        let run = start("");
        let prompt = unsafe { abi::alloc_string("alpha") };
        let stream_result = awaited("spectra.std.agent.ask_stream", &[run, prompt]);
        assert_eq!(tag(stream_result), 0);
        let stream = payload(stream_result);
        assert_ne!(stream, 0);

        let mut text = String::new();
        let mut chunks = 0;
        loop {
            let next = awaited("spectra.std.agent.stream_next", &[stream]);
            assert_eq!(tag(next), 0, "{}", error_message(next));
            let chunk = abi::read_string_arg(payload(next)).expect("chunk string");
            if chunk.is_empty() {
                break;
            }
            chunks += 1;
            text.push_str(&chunk);
        }
        assert!(chunks >= 2, "expected several chunks, got {chunks}");
        assert_eq!(text, "mock echo: alpha");

        let closed = awaited("spectra.std.agent.stream_close", &[stream]);
        assert_eq!(tag(closed), 0);
        assert_eq!(payload(closed), 1);
        // Close is idempotent; reading after close is a typed error.
        let closed_again = awaited("spectra.std.agent.stream_close", &[stream]);
        assert_eq!(payload(closed_again), 1);
        let after = awaited("spectra.std.agent.stream_next", &[stream]);
        assert!(error_message(after).contains("unknown_handle"));

        let (_, report) = call("spectra.std.agent.agent_end", &[run]);
        let report = abi::read_string_arg(payload(report)).expect("report");
        assert!(report.contains("\"steps\":1"), "{report}");
    }

    #[test]
    fn embed_returns_a_rank_one_float_tensor() {
        let _guard = setup();
        let run = start("");
        let text = unsafe { abi::alloc_string("alpha") };
        let embedded = awaited("spectra.std.agent.embed", &[run, text]);
        assert_eq!(tag(embedded), 0, "{}", error_message(embedded));
        let tensor = payload(embedded);
        assert_ne!(tensor, 0);
        // The tensor is a genuine 1-D float tensor carrying the provider's
        // deterministic vector, read back through the public accessors.
        let (status, rank) = call("spectra.std.tensor.rank", &[tensor]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_eq!(rank, 1);
        let expected = crate::provider::mock::MockProvider::new("mock/echo".to_string())
            .embed("alpha")
            .expect("embed");
        for (index, want) in expected.iter().enumerate() {
            let (status, bits) = call("spectra.std.tensor.get_f", &[tensor, index as i64]);
            assert_eq!(status, HOST_STATUS_SUCCESS, "index {index}");
            assert_eq!(
                f64::from_bits(bits as u64),
                *want,
                "embedding element {index} must match the provider"
            );
        }
        let (_, report) = call("spectra.std.agent.agent_end", &[run]);
        let report = abi::read_string_arg(payload(report)).expect("report");
        assert!(report.contains("\"steps\":0"), "{report}");
    }

    #[test]
    fn a_deterministic_run_fails_closed_without_a_seed_honoring_provider() {
        struct NoTransport;
        impl HttpTransport for NoTransport {
            fn post_json(
                &self,
                _url: &str,
                _headers: &[(String, String)],
                _body: &str,
            ) -> Result<TransportResponse, String> {
                Err("must not be called".to_string())
            }
        }
        let _guard = setup();
        clear_http_transport();
        assert!(set_http_transport(NoTransport));

        // The mock honors the seed, so a deterministic mock run works.
        let deterministic_mock = start(",\"seed\":5");
        let prompt = unsafe { abi::alloc_string("alpha") };
        let ok = awaited("spectra.std.agent.ask", &[deterministic_mock, prompt]);
        assert_eq!(tag(ok), 0);
        call("spectra.std.agent.agent_end", &[deterministic_mock]);

        // An OpenAI-compatible endpoint accepts the seed best-effort only and
        // therefore cannot back a deterministic run.
        let json = r#"{"goal":"g","model":"gpt-test","endpoint":"https://provider.invalid","seed":5}"#;
        let (status, result) = call(
            "spectra.std.agent.agent_start",
            &[unsafe { abi::alloc_string(json) }],
        );
        assert_eq!(status, HOST_STATUS_SUCCESS);
        let run = payload(result);
        let prompt = unsafe { abi::alloc_string("alpha") };
        let denied = awaited("spectra.std.agent.ask", &[run, prompt]);
        let message = error_message(denied);
        assert!(
            message.contains("deterministic_sampling_unavailable"),
            "{message}"
        );
        call("spectra.std.agent.agent_end", &[run]);
        clear_http_transport();
    }

    #[test]
    fn invalid_specs_and_wrong_arity_are_rejected() {
        let _guard = setup();
        let (status, result) = call(
            "spectra.std.agent.agent_start",
            &[unsafe { abi::alloc_string("{\"model\":\"mock/echo\"}") }],
        );
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert!(error_message(result).contains("invalid_spec"));
        assert_eq!(
            call("spectra.std.agent.agent_start", &[]).0,
            HOST_STATUS_INVALID_ARGUMENT
        );
        // A run handle passed where a stream is expected is a typed error.
        let run = start("");
        let wrong_kind = awaited("spectra.std.agent.stream_next", &[run]);
        assert!(error_message(wrong_kind).contains("invalid_handle"));
        call("spectra.std.agent.agent_end", &[run]);
    }

    // ── R-3216 budget enforcement ────────────────────────────────────────

    /// Reads `budget_remaining(run)` as a plain integer.
    fn remaining(run: i64) -> i64 {
        let (status, result) = call("spectra.std.agent.budget_remaining", &[run]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_eq!(tag(result), 0, "{}", error_message(result));
        payload(result)
    }

    #[test]
    fn budget_remaining_reports_the_sentinel_and_the_token_ceiling() {
        let _guard = setup();
        // No token ceiling: the documented "unbounded" sentinel.
        let unbounded = start("");
        assert_eq!(remaining(unbounded), i64::MAX);
        call("spectra.std.agent.agent_end", &[unbounded]);

        let run = start(r#","max_tokens":6"#);
        assert_eq!(remaining(run), 6);
        call("spectra.std.agent.agent_end", &[run]);
    }

    #[test]
    fn token_ceiling_crossing_cancels_and_names_the_ceiling() {
        let _guard = setup();
        let run = start(r#","max_tokens":6"#);
        assert_eq!(remaining(run), 6);

        // 1 input + 3 output tokens: inside the ceiling.
        let alpha = unsafe { abi::alloc_string("alpha") };
        let first = awaited("spectra.std.agent.ask", &[run, alpha]);
        assert_eq!(tag(first), 0, "{}", error_message(first));
        assert_eq!(remaining(run), 2);

        // The crossing turn is accounted and still delivers its value; the
        // run is cancelled for everything that follows.
        let beta = unsafe { abi::alloc_string("beta") };
        let second = awaited("spectra.std.agent.ask", &[run, beta]);
        assert_eq!(tag(second), 0, "{}", error_message(second));
        assert_eq!(remaining(run), 0);

        let gamma = unsafe { abi::alloc_string("gamma") };
        let denied = awaited("spectra.std.agent.ask", &[run, gamma]);
        let message = error_message(denied);
        assert!(message.contains("budget_exceeded"), "{message}");
        assert!(message.contains("max_tokens"), "{message}");

        let (_, report) = call("spectra.std.agent.agent_end", &[run]);
        let report = abi::read_string_arg(payload(report)).expect("report");
        assert!(report.contains("\"status\":\"budget_exceeded\""), "{report}");
        assert!(report.contains("\"ceiling\":\"max_tokens\""), "{report}");
        assert!(report.contains("\"steps\":2"), "{report}");
        assert!(report.contains("\"tokens_out\":6"), "{report}");
    }

    #[test]
    fn cost_ceiling_crossing_cancels_the_run() {
        let _guard = setup();
        // The mock reports 1 micro/token in and 2 out: "alpha" costs 7.
        let run = start(r#","max_cost_micros":10"#);
        let alpha = unsafe { abi::alloc_string("alpha") };
        let first = awaited("spectra.std.agent.ask", &[run, alpha]);
        assert_eq!(tag(first), 0, "{}", error_message(first));

        let beta = unsafe { abi::alloc_string("beta") };
        let second = awaited("spectra.std.agent.ask", &[run, beta]);
        assert_eq!(tag(second), 0, "{}", error_message(second));

        let gamma = unsafe { abi::alloc_string("gamma") };
        let denied = awaited("spectra.std.agent.ask", &[run, gamma]);
        assert!(error_message(denied).contains("max_cost_micros"));

        let (_, report) = call("spectra.std.agent.agent_end", &[run]);
        let report = abi::read_string_arg(payload(report)).expect("report");
        assert!(report.contains("\"status\":\"budget_exceeded\""), "{report}");
        assert!(report.contains("\"ceiling\":\"max_cost_micros\""), "{report}");
        assert!(report.contains("\"cost_micros\":14"), "{report}");
    }

    #[test]
    fn a_cost_ceiling_fails_closed_at_agent_start_without_a_cost_reporting_provider() {
        let _guard = setup();
        let json = r#"{"goal":"g","model":"gpt-test","endpoint":"https://provider.invalid","max_cost_micros":5}"#;
        let (status, result) = call(
            "spectra.std.agent.agent_start",
            &[unsafe { abi::alloc_string(json) }],
        );
        assert_eq!(status, HOST_STATUS_SUCCESS, "a typed error, not a host trap");
        assert_eq!(tag(result), 1);
        let message = error_message(result);
        assert!(message.contains("cost_accounting_unavailable"), "{message}");
        // Without the ceiling the same provider starts normally.
        let json =
            r#"{"goal":"g","model":"gpt-test","endpoint":"https://provider.invalid"}"#;
        let (_, result) = call(
            "spectra.std.agent.agent_start",
            &[unsafe { abi::alloc_string(json) }],
        );
        assert_eq!(tag(result), 0);
        call("spectra.std.agent.agent_end", &[payload(result)]);
    }

    #[test]
    fn wall_clock_ceiling_crosses_on_a_slow_call() {
        let _guard = setup();
        let run = start(r#","max_seconds":1"#);
        let slow = unsafe { abi::alloc_string("spectra:sleep-ms=1100") };
        let turn = awaited("spectra.std.agent.ask", &[run, slow]);
        assert_eq!(tag(turn), 0, "{}", error_message(turn));

        let again = unsafe { abi::alloc_string("fast") };
        let denied = awaited("spectra.std.agent.ask", &[run, again]);
        assert!(error_message(denied).contains("max_seconds"));

        let (_, report) = call("spectra.std.agent.agent_end", &[run]);
        let report = abi::read_string_arg(payload(report)).expect("report");
        assert!(report.contains("\"status\":\"budget_exceeded\""), "{report}");
        assert!(report.contains("\"ceiling\":\"max_seconds\""), "{report}");
    }

    #[test]
    fn tool_call_ceiling_allows_two_calls_and_denies_the_third() {
        let _guard = setup();
        let run = start(r#","max_tool_calls":2"#);
        budget::charge_tool_call(run).expect("first tool call");
        budget::charge_tool_call(run).expect("second tool call");
        let error = budget::charge_tool_call(run).expect_err("third tool call denied");
        assert_eq!(error.kind(), "budget_exceeded");
        assert!(error.detail().contains("max_tool_calls"), "{error}");
        // A subsequent model call is refused too: the run is cancelled.
        let prompt = unsafe { abi::alloc_string("gamma") };
        let denied = awaited("spectra.std.agent.ask", &[run, prompt]);
        assert!(error_message(denied).contains("budget_exceeded"));

        let (_, report) = call("spectra.std.agent.agent_end", &[run]);
        let report = abi::read_string_arg(payload(report)).expect("report");
        assert!(report.contains("\"status\":\"budget_exceeded\""), "{report}");
        assert!(report.contains("\"ceiling\":\"max_tool_calls\""), "{report}");
        assert!(report.contains("\"tool_calls\":2"), "{report}");
    }
}
