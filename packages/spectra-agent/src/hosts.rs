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

use serde_json::{json, Value};

use crate::abi;
use crate::budget;
use crate::error::{self, AgentError};
use crate::journal::StepUsage;
use crate::provider::{self, FinishReason, Message, ProviderRequest, ProviderResponse, SamplingParams};
use crate::replay::{self, Kind, Resolved, Token};
use crate::run;
use crate::schema;
use crate::spec::AgentSpec;
use crate::trace;

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

/// Runs `work` with the run entered on the active run chain.
///
/// R-3223 T3: the run-scoped hosts — the model gateway (`ask`, `ask_json`,
/// `ask_stream`, `embed`) and the dispatch primitives (`act`, `tool_call`) —
/// execute with the run active. That is the dynamic extent in which the run
/// performs work of its own: the chain is captured when the host spawns its
/// worker (R-3213 T2), and a tool wrapper invoked by `act`/`tool_call`
/// dispatches its host calls while the run is on the chain, so the policy seam
/// can enforce the run's capabilities and its taint policy there.
///
/// The program frame between `agent_start` and `agent_end` is deliberately not
/// entered: a host call the author wrote directly is the author's own action,
/// while the model-driven path is what a run's authority bounds. A program
/// without a run therefore behaves exactly as before (invariant I5).
fn in_run<F>(run_handle: i64, work: F) -> i32
where
    F: FnOnce() -> i32,
{
    run::in_run_scope(run_handle, work)
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

/// Canonical fingerprint of a provider request, used as the model step's
/// journal input. Two runs of the same program produce the same fingerprint,
/// which is what makes a post-crash retry collide with the recorded step.
fn request_fingerprint(
    spec: &AgentSpec,
    sampling: SamplingParams,
    messages: &[Message],
    json_schema: Option<&str>,
    tools: &[provider::ToolDefinition],
) -> String {
    let messages: Vec<Value> = messages
        .iter()
        .map(|message| json!([message.role, message.content]))
        .collect();
    let tools: Vec<Value> = tools
        .iter()
        .map(|tool| json!([tool.name, tool.input_schema]))
        .collect();
    json!({
        "model": spec.model,
        "endpoint": spec.endpoint,
        "messages": messages,
        "tools": tools,
        "schema": json_schema,
        "temperature": sampling.temperature,
        "top_k": sampling.top_k,
        "seed": sampling.seed,
        "max_tokens": (spec.max_tokens > 0).then_some(spec.max_tokens),
    })
    .to_string()
}

fn finish_label(finish: &FinishReason) -> &str {
    match finish {
        FinishReason::Stop => "stop",
        FinishReason::Length => "length",
        FinishReason::Other(reason) => reason,
    }
}

fn finish_from_label(label: &str) -> FinishReason {
    match label {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::Length,
        other => FinishReason::Other(other.to_string()),
    }
}

/// The recorded output of a model step: everything the caller needs to
/// reconstruct the response (text, tool calls, usage, cost, finish).
fn response_json(response: &ProviderResponse) -> String {
    json!({
        "text": response.text,
        "tool_calls": response
            .tool_calls
            .iter()
            .map(|call| json!({"name": call.name, "arguments": call.arguments}))
            .collect::<Vec<_>>(),
        "usage": {
            "input_tokens": response.usage.input_tokens,
            "output_tokens": response.usage.output_tokens,
        },
        "cost_micros": response.cost_micros,
        "retries": response.retries,
        "finish": finish_label(&response.finish),
    })
    .to_string()
}

fn response_from_json(output: &str) -> Result<ProviderResponse, AgentError> {
    let value: Value = serde_json::from_str(output)
        .map_err(|error| AgentError::Journal(format!("recorded model output is invalid: {error}")))?;
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| AgentError::Journal("recorded model output has no text".to_string()))?
        .to_string();
    let tool_calls = value
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .filter_map(|call| {
                    Some(provider::ToolCall {
                        name: call.get("name")?.as_str()?.to_string(),
                        arguments: call.get("arguments")?.as_str()?.to_string(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(ProviderResponse {
        text,
        tool_calls,
        usage: provider::Usage {
            input_tokens: value
                .pointer("/usage/input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            output_tokens: value
                .pointer("/usage/output_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        },
        cost_micros: value.get("cost_micros").and_then(Value::as_u64).unwrap_or(0),
        retries: value.get("retries").and_then(Value::as_u64).unwrap_or(0),
        finish: value
            .get("finish")
            .and_then(Value::as_str)
            .map(finish_from_label)
            .unwrap_or(FinishReason::Stop),
    })
}

/// The recorded output of a streamed model step.
fn stream_json(stream: &provider::ProviderStream) -> String {
    json!({
        "chunks": stream.chunks,
        "usage": {
            "input_tokens": stream.usage.input_tokens,
            "output_tokens": stream.usage.output_tokens,
        },
        "cost_micros": stream.cost_micros,
        "retries": stream.retries,
    })
    .to_string()
}

/// `(tokens_in, tokens_out)` plus cost and retries from a recorded stream.
#[allow(clippy::type_complexity)]
fn stream_from_json(
    output: &str,
) -> Result<(Vec<String>, (u64, u64), u64, u64), AgentError> {
    let value: Value = serde_json::from_str(output).map_err(|error| {
        AgentError::Journal(format!("recorded stream output is invalid: {error}"))
    })?;
    let chunks = value
        .get("chunks")
        .and_then(Value::as_array)
        .map(|chunks| {
            chunks
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| AgentError::Journal("recorded stream output has no chunks".to_string()))?;
    let usage = (
        value
            .pointer("/usage/input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        value
            .pointer("/usage/output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    );
    Ok((
        chunks,
        usage,
        value.get("cost_micros").and_then(Value::as_u64).unwrap_or(0),
        value.get("retries").and_then(Value::as_u64).unwrap_or(0),
    ))
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
/// model (R-3222) and the journal/replay discipline of R-3217.
///
/// A recorded step returns the recorded response without calling the provider:
/// that is I3's "no effect twice across a replay". A fresh step calls the
/// provider, accounts the turn, journals the response, and only then returns —
/// the journal flush precedes the value, so a crash cannot lose a completed
/// effect.
pub(crate) fn model_turn_with(
    run_handle: i64,
    messages: Vec<Message>,
    json_schema: Option<String>,
    tools: Vec<provider::ToolDefinition>,
) -> Result<provider::ProviderResponse, AgentError> {
    let (spec, sampling) = snapshot(run_handle)?;
    let provider = resolve_provider(&spec)?;
    // R-3223 T1: every message in the transcript carries its origin before it
    // reaches the provider; a tool result is untrusted, and observation is
    // monotone so re-sending the transcript never launders earlier content.
    crate::taint::observe_transcript(run_handle, &messages)?;
    let fingerprint = request_fingerprint(&spec, sampling, &messages, json_schema.as_deref(), &tools);

    let token: Token = match replay::resolve(run_handle, Kind::Model, &fingerprint)? {
        Resolved::Recorded(record) => {
            let response = response_from_json(&record.output)?;
            run::with_run(run_handle, |state| {
                state.record_turn(
                    record.usage.tokens_in,
                    record.usage.tokens_out,
                    record.usage.cost_micros,
                    response.retries,
                )
            })?;
            trace::emit_chat(
                &record.run,
                &spec.goal,
                record.step,
                &spec.model,
                None,
                Some(&response.text),
            );
            return Ok(response);
        }
        Resolved::Fresh(token) => token,
    };

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
            replay::commit(
                run_handle,
                &token,
                Some(&fingerprint),
                &response_json(&response),
                StepUsage {
                    tokens_in: response.usage.input_tokens,
                    tokens_out: response.usage.output_tokens,
                    cost_micros: response.cost_micros,
                },
                sampling.seed.unwrap_or(-1),
                None,
            )?;
            let run_id = run::with_run(run_handle, |state| state.run_id.clone())?;
            let prompt = request
                .messages
                .iter()
                .rev()
                .find(|message| message.role == "user")
                .map(|message| message.content.as_str());
            trace::emit_chat(
                &run_id,
                &spec.goal,
                token.step,
                &spec.model,
                prompt,
                Some(&response.text),
            );
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
        // R-3217: the journal identity is fixed at start. A non-empty `run_id`
        // that names an existing journal resumes that run in replay mode.
        let run_id = if spec.run_id.is_empty() {
            crate::journal::new_run_id()
        } else {
            spec.run_id.clone()
        };
        let journal = if spec.journal.is_empty() {
            None
        } else {
            Some(crate::journal::Journal::open(
                &run_id,
                &spec.journal,
                spec.journal_payloads,
            )?)
        };
        let goal = spec.goal.clone();
        let handle = run::alloc_run(spec, run_id.clone(), journal)?;
        crate::policy::install();
        trace::emit_invoke_agent(&run_id, &goal);
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
    in_run(run_handle, || {
        write_run_task(ctx, run_handle, move || {
            let response = model_turn(run_handle, &prompt, None)?;
            let pointer = unsafe { abi::alloc_string(&response.text) };
            if pointer == 0 {
                return Err(AgentError::Internal("could not allocate the response".to_string()));
            }
            Ok(pointer)
        })
    })
}

/// `spectra.std.agent.ask_json(run, prompt, schema) -> Task<Result<string, Error>>`.
extern "C" fn ask_json_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let prompt = strings[0].clone();
    let json_schema = strings[1].clone();
    in_run(run_handle, || {
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
    })
}

/// `spectra.std.agent.ask_stream(run, prompt) -> Task<Result<ChunkStream, Error>>`.
extern "C" fn ask_stream_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let prompt = strings[0].clone();
    in_run(run_handle, || {
        write_run_task(ctx, run_handle, move || {
            let (spec, sampling) = snapshot(run_handle)?;
            let provider = resolve_provider(&spec)?;
            let fingerprint = format!(
                "stream\0{}",
                request_fingerprint(
                    &spec,
                    sampling,
                    &[Message::user(prompt.clone())],
                    None,
                    &[]
                )
            );
            let token = match replay::resolve(run_handle, Kind::Model, &fingerprint)? {
                Resolved::Recorded(record) => {
                    let (chunks, usage, cost_micros, retries) = stream_from_json(&record.output)?;
                    run::with_run(run_handle, |state| {
                        state.record_turn(usage.0, usage.1, cost_micros, retries)
                    })?;
                    trace::emit_chat(
                        &record.run,
                        &spec.goal,
                        record.step,
                        &spec.model,
                        None,
                        None,
                    );
                    return run::alloc_stream(chunks);
                }
                Resolved::Fresh(token) => token,
            };
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
            replay::commit(
                run_handle,
                &token,
                Some(&fingerprint),
                &stream_json(&stream),
                StepUsage {
                    tokens_in: stream.usage.input_tokens,
                    tokens_out: stream.usage.output_tokens,
                    cost_micros: stream.cost_micros,
                },
                sampling.seed.unwrap_or(-1),
                None,
            )?;
            let run_id = run::with_run(run_handle, |state| state.run_id.clone())?;
            trace::emit_chat(
                &run_id,
                &spec.goal,
                token.step,
                &spec.model,
                Some(&prompt),
                None,
            );
            run::alloc_stream(stream.chunks)
        })
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
    in_run(run_handle, || {
        write_run_task(ctx, run_handle, move || {
            let (spec, _sampling) = snapshot(run_handle)?;
        let provider = resolve_provider(&spec)?;
        let fingerprint = format!("embed\0{}\0{}", spec.model, text);
        let token = match replay::resolve(run_handle, Kind::Embed, &fingerprint)? {
            // The recorded vector is re-materialized as the tensor the caller
            // expects; the provider is not called again.
            Resolved::Recorded(record) => return alloc_float_tensor(&vector_from_json(&record.output)?),
            Resolved::Fresh(token) => token,
        };
        let vector = provider.embed(&text).map_err(AgentError::from)?;
        replay::commit(
            run_handle,
            &token,
            Some(&fingerprint),
            &vector_to_json(&vector),
            StepUsage::default(),
            -1,
            None,
        )?;
            alloc_float_tensor(&vector)
        })
    })
}

/// Serializes an embedding so a replayed step can rebuild its tensor.
fn vector_to_json(vector: &[f64]) -> String {
    Value::Array(vector.iter().map(|value| json!(value)).collect()).to_string()
}

fn vector_from_json(output: &str) -> Result<Vec<f64>, AgentError> {
    let value: Value = serde_json::from_str(output)
        .map_err(|error| AgentError::Journal(format!("recorded embedding is invalid: {error}")))?;
    value
        .as_array()
        .map(|values| values.iter().filter_map(Value::as_f64).collect())
        .filter(|values: &Vec<f64>| !values.is_empty())
        .ok_or_else(|| AgentError::Journal("recorded embedding is empty".to_string()))
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
    in_run(run_handle, || {
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
    in_run(run_handle, || {
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
        // Every field is required: a tool whose descriptor cannot be read is
        // not registered at all, instead of entering the registry with an
        // empty description or a permissive schema that hides the bug.
        let description = abi::read_string_arg(args[2]).ok_or_else(|| {
            AgentError::ToolFailed(format!("tool '{name}' has an unreadable description"))
        })?;
        let input_schema = abi::read_string_arg(args[3]).ok_or_else(|| {
            AgentError::ToolFailed(format!("tool '{name}' has an unreadable input schema"))
        })?;
        let effects = abi::read_string_arg(args[4]).ok_or_else(|| {
            AgentError::ToolFailed(format!("tool '{name}' has an unreadable effect list"))
        })?;
        let changed =
            crate::tools::register(name, args[1], description, input_schema, &effects)?;
        Ok(i64::from(changed))
    })();
    write_outcome(ctx, outcome)
}

// ── governance hosts (R-3217) ────────────────────────────────────────────

/// `spectra.std.agent.approve(run, action) -> Result<bool, Error>`.
///
/// True when the action is authorized. Without an attached approver the
/// decision is a deny, so an unattended run fails closed.
extern "C" fn approve_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let action = strings[0].clone();
    write_outcome(
        ctx,
        crate::approval::approve(run_handle, &action).map(i64::from),
    )
}

/// `spectra.std.agent.require(run, condition, message) -> Result<bool, Error>`.
///
/// The condition crosses as the ABI's boolean word; `false` returns a typed
/// error carrying the message and the run goal, and marks the run failed.
extern "C" fn require_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some(args) = abi::args(ctx) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if args.len() != 3 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let Some(message) = abi::read_string_arg(args[2]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let run_handle = args[0];
    let condition = args[1] != 0;
    write_outcome(
        ctx,
        crate::assert::require(run_handle, condition, &message).map(i64::from),
    )
}

// ── taint hosts (R-3223 T4) ──────────────────────────────────────────────

/// `spectra.std.agent.untrusted(run, value, origin) -> Result<string, Error>`.
///
/// Records that `value` entered the run from `origin`; the value is returned
/// unchanged, so a caller can tag content inline. The ledger entry is keyed by
/// the value's content digest and journaled.
extern "C" fn untrusted_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (value, origin) = (strings[0].clone(), strings[1].clone());
    let outcome = (|| -> Result<i64, AgentError> {
        crate::taint::mark_untrusted(run_handle, &value, &origin)?;
        let pointer = unsafe { abi::alloc_string(&value) };
        if pointer == 0 {
            return Err(AgentError::Internal(
                "could not allocate the untrusted value".to_string(),
            ));
        }
        Ok(pointer)
    })();
    write_outcome(ctx, outcome)
}

/// `spectra.std.agent.trust(run, value, reason) -> Result<string, Error>`.
///
/// Declassifies the digest of `value` and returns the value unchanged. The
/// reason is mandatory and is the audit record; without one the call fails
/// with a typed `taint_error`.
extern "C" fn trust_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (value, reason) = (strings[0].clone(), strings[1].clone());
    let outcome = (|| -> Result<i64, AgentError> {
        crate::taint::declassify(run_handle, &value, &reason)?;
        let pointer = unsafe { abi::alloc_string(&value) };
        if pointer == 0 {
            return Err(AgentError::Internal(
                "could not allocate the declassified value".to_string(),
            ));
        }
        Ok(pointer)
    })();
    write_outcome(ctx, outcome)
}

// ── compensation hosts (R-3224) ──────────────────────────────────────────

/// `spectra.std.agent.compensate(run, tool, arguments_json)
/// -> Result<bool, Error>`.
///
/// Journals a pending compensation (LIFO) after validating the tool name
/// against the run's registered registry. Synchronous: the declaration is
/// durable before it returns.
extern "C" fn compensate_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (tool, arguments) = (strings[0].clone(), strings[1].clone());
    write_outcome(
        ctx,
        crate::compensate::compensate(run_handle, &tool, &arguments).map(i64::from),
    )
}

/// `spectra.std.agent.rollback(run, reason) -> Result<int, Error>`.
///
/// Executes the pending compensations in LIFO order through the governed
/// dispatch and returns the number executed. Synchronous, and executed with
/// the run entered on the active run chain so the tools' own host calls pass
/// the policy and taint seams (R-3223).
extern "C" fn rollback_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let reason = strings[0].clone();
    in_run(run_handle, || {
        write_outcome(ctx, crate::compensate::rollback(run_handle, &reason))
    })
}

// ── MCP hosts (R-3218) ───────────────────────────────────────────────────

/// `spectra.std.agent.mcp_connect(run, url) -> Task<Result<string, Error>>`.
///
/// Discovers a remote MCP server's tools over the run's injected HTTP
/// transport, registers each one as a governed registry entry namespaced by
/// the server identity, and records every description and schema in the run's
/// taint ledger. Returns the descriptor document, which is untrusted data.
extern "C" fn mcp_connect_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let url = strings[0].clone();
    in_run(run_handle, || {
        write_run_task(ctx, run_handle, move || {
            let document = crate::mcp::client::connect(run_handle, &url)?;
            allocate_outcome("the MCP descriptor document", &document)
        })
    })
}

/// `spectra.std.agent.mcp_handle(run, request) -> Result<string, Error>`.
///
/// Serves one MCP JSON-RPC request from the project's registered tools and
/// executes `tools/call` inside the run through the governed dispatch. The
/// empty string is the response to a notification.
extern "C" fn mcp_handle_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let request = strings[0].clone();
    let outcome = (|| -> Result<i64, AgentError> {
        let response = crate::mcp::server::handle(run_handle, &request)?;
        allocate_outcome("the MCP response", &response)
    })();
    write_outcome(ctx, outcome)
}

/// `spectra.std.agent.mcp_serve(run, bind) -> Result<string, Error>`.
///
/// Starts the in-process HTTP listener and returns the bound `host:port`, so a
/// third-party MCP client can call the run's tools.
extern "C" fn mcp_serve_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let bind = strings[0].clone();
    let outcome = (|| -> Result<i64, AgentError> {
        let authority = crate::mcp::server::serve(run_handle, &bind)?;
        allocate_outcome("the MCP listener address", &authority)
    })();
    write_outcome(ctx, outcome)
}

// ── A2A and ACP hosts (R-3219) ───────────────────────────────────────────

/// `spectra.std.agent.a2a_card(run, description_json) -> Result<string, Error>`.
///
/// The A2A AgentCard: the authored strings from `description_json` plus the
/// derived `#[agent_tool]` surface as skills.
extern "C" fn a2a_card_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let description = strings[0].clone();
    let outcome = (|| -> Result<i64, AgentError> {
        let document = crate::protocol::a2a::card(run_handle, &description)?;
        allocate_outcome("the A2A agent card", &document)
    })();
    write_outcome(ctx, outcome)
}

/// `spectra.std.agent.a2a_handle(run, request) -> Result<string, Error>`.
///
/// Serves one A2A JSON-RPC request. A delegated task runs inside its own
/// journaled run through the governed dispatch; the response is the Task.
extern "C" fn a2a_handle_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let request = strings[0].clone();
    let outcome = (|| -> Result<i64, AgentError> {
        let response = crate::protocol::a2a::handle(run_handle, &request)?;
        allocate_outcome("the A2A response", &response)
    })();
    write_outcome(ctx, outcome)
}

/// `spectra.std.agent.a2a_serve(run, bind, description_json)
/// -> Result<string, Error>`.
///
/// Starts the in-process A2A listener (the agent card and JSON-RPC over HTTP)
/// and returns the bound `host:port`.
extern "C" fn a2a_serve_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let bind = strings[0].clone();
    let description = strings[1].clone();
    let outcome = (|| -> Result<i64, AgentError> {
        let authority = crate::protocol::a2a::serve(run_handle, &bind, &description)?;
        allocate_outcome("the A2A listener address", &authority)
    })();
    write_outcome(ctx, outcome)
}

/// `spectra.std.agent.acp_handle(run, request) -> Result<string, Error>`.
///
/// Answers one ACP JSON-RPC request from the client (`initialize`,
/// `session/new`, `session/prompt`, `session/cancel`). A notification returns
/// the empty string.
extern "C" fn acp_handle_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let request = strings[0].clone();
    let outcome = (|| -> Result<i64, AgentError> {
        let response = crate::protocol::acp::handle(run_handle, &request)?;
        allocate_outcome("the ACP response", &response)
    })();
    write_outcome(ctx, outcome)
}

/// `spectra.std.agent.acp_permission(run, action) -> Result<bool, Error>`.
///
/// Asks the attached ACP client through `session/request_permission` and
/// journals the mapped decision through the approval primitive. False — a
/// denial, a cancellation, no client attached — means the caller must not
/// perform the action.
extern "C" fn acp_permission_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = read_run_and_prompt(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let action = strings[0].clone();
    write_outcome(
        ctx,
        crate::protocol::acp::permission(run_handle, &action).map(i64::from),
    )
}

/// Allocates the packed string a sync host returns, or a typed internal error.
fn allocate_outcome(what: &str, document: &str) -> Result<i64, AgentError> {
    let pointer = unsafe { abi::alloc_string(document) };
    if pointer == 0 {
        return Err(AgentError::Internal(format!(
            "could not allocate {what}"
        )));
    }
    Ok(pointer)
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
        ("spectra.std.agent.approve", approve_host as _),
        ("spectra.std.agent.require", require_host as _),
        ("spectra.std.agent.untrusted", untrusted_host as _),
        ("spectra.std.agent.trust", trust_host as _),
        ("spectra.std.agent.compensate", compensate_host as _),
        ("spectra.std.agent.rollback", rollback_host as _),
        ("spectra.std.agent.mcp_connect", mcp_connect_host as _),
        ("spectra.std.agent.mcp_handle", mcp_handle_host as _),
        ("spectra.std.agent.mcp_serve", mcp_serve_host as _),
        ("spectra.std.agent.a2a_card", a2a_card_host as _),
        ("spectra.std.agent.a2a_handle", a2a_handle_host as _),
        ("spectra.std.agent.a2a_serve", a2a_serve_host as _),
        ("spectra.std.agent.acp_handle", acp_handle_host as _),
        ("spectra.std.agent.acp_permission", acp_permission_host as _),
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
        // `journal:""` disables the run journal: these tests exercise the
        // gateway, not durability, and must not write into the process cwd.
        let json = format!(
            r#"{{"goal":"g","model":"mock/echo","endpoint":"mock:","journal":""{extra}}}"#
        );
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
        // Nine R-3211 gateway functions, the three R-3222 dispatch hosts, the
        // two R-3217 governance hosts, the two R-3223 taint hosts, the two
        // R-3224 compensation hosts, the three R-3218 MCP hosts and the five
        // R-3219 protocol hosts.
        assert_eq!(register(), 26);
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
        let json = r#"{"goal":"g","model":"gpt-test","endpoint":"https://provider.invalid","journal":"","seed":5}"#;
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
        let json = r#"{"goal":"g","model":"gpt-test","endpoint":"https://provider.invalid","journal":"","max_cost_micros":5}"#;
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
            r#"{"goal":"g","model":"gpt-test","endpoint":"https://provider.invalid","journal":""}"#;
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
