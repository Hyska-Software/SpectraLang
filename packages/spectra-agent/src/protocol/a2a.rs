//! A2A exposure: the agent card and the journaled task lifecycle (R-3219 T1).
//!
//! The Agent2Agent protocol has two halves, and this module implements both
//! against data the platform already owns:
//!
//! * **Discovery.** [`card`] renders an AgentCard from an authored description
//!   record (name, description, version, url — the strings a human writes) plus
//!   the **derived** tool surface: every registered `#[agent_tool]` becomes a
//!   skill whose id, description and tags come from the compiler's own
//!   descriptor and effect set. Nothing about a skill is re-derived here, so
//!   the advertised surface and the compiled surface cannot drift.
//! * **Task lifecycle.** A delegated task *is* a run: the task id is the run
//!   id of a run created with the serving run's spec (the host's grants, model
//!   and ceilings), and the task's request and terminal state are journaled as
//!   `task` steps. `message/send` creates that run and drives the governed
//!   agent loop ([`crate::act::act`]); `tasks/get` returns the recorded state,
//!   resuming an interrupted task forward from its journal instead of
//!   duplicating an effect; `tasks/cancel` closes a task that never reached a
//!   terminal step.
//!
//! # Governed by construction
//!
//! A delegated task cannot bypass anything the local path enforces: it runs
//! through the same `act` loop, which enforces the run's grant before its first
//! dispatch (`enforce_run_grant`), charges the run's tool-call ceiling, journals
//! every model turn and tool invocation, and applies the taint gate inside each
//! tool's own host calls. A capability denial is therefore observable to the
//! A2A client as a `rejected` task carrying the typed reason, and a crossed
//! ceiling as a `failed` task naming it — never a silent success.
//!
//! # Matrix
//!
//! The card's `capabilities` are honest: `streaming` and `pushNotifications`
//! are `false` because no streaming or webhook path is implemented, and
//! `stateTransitionHistory` is `true` because `tasks/get` reconstructs a task
//! from its journal. The JSON-RPC binding is the widely deployed `0.3` method
//! names (`message/send`, `tasks/get`, `tasks/cancel`); task states are the
//! lowercase strings of that binding.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::error::AgentError;
use crate::journal::{Journal, StepUsage};
use crate::mcp::wire;
use crate::net;
use crate::replay::{self, Kind, Resolved};
use crate::run;
use crate::spec::AgentSpec;
use crate::tools;

/// The A2A protocol version this adapter speaks.
pub(crate) const PROTOCOL_VERSION: &str = "0.3.0";

/// Where a card is served, per the specification.
pub(crate) const CARD_PATH: &str = "/.well-known/agent-card.json";

/// JSON-RPC error codes this adapter uses.
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL: i64 = -32603;
/// A2A `TaskNotFoundError`.
const TASK_NOT_FOUND: i64 = -32001;
/// A2A `TaskNotCancelableError`.
const TASK_NOT_CANCELABLE: i64 = -32002;

// ── the card ─────────────────────────────────────────────────────────────

/// `a2a_card(run, description_json)`: the A2A AgentCard document.
///
/// The authored record supplies `name`, `description`, `version` and `url`;
/// an absent field defaults from the run (its goal as the name, the crate
/// version as the version) rather than being invented. A present-but-non-string
/// field is a typed failure, because guessing identity strings would make the
/// served card differ from what the author wrote.
pub(crate) fn card(run_handle: i64, description_json: &str) -> Result<String, AgentError> {
    card_for(run_handle, description_json, None)
}

/// [`card`] with the endpoint the card is being served from.
fn card_for(
    run_handle: i64,
    description_json: &str,
    endpoint: Option<&str>,
) -> Result<String, AgentError> {
    let goal = run::with_run(run_handle, |state| state.spec.goal.clone())?;
    let authored = authored(description_json)?;
    let name = string_field(&authored, "name")?.unwrap_or_else(|| goal.clone());
    let description = string_field(&authored, "description")?.unwrap_or_default();
    let version = string_field(&authored, "version")?
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

    // The skill list is the derived `#[agent_tool]` surface: the compiler's
    // name, description and effect set, exactly what the process dispatches.
    let skills: Vec<Value> = tools::registered()
        .into_iter()
        .map(|tool| {
            let tags: Vec<String> = if tool.effects.is_empty() {
                vec!["read-only".to_string()]
            } else {
                tool.effects
            };
            json!({
                "id": tool.name,
                "name": tool.name,
                "description": tool.description,
                "tags": tags,
                "inputModes": ["application/json"],
                "outputModes": ["application/json"],
            })
        })
        .collect();

    let mut document = json!({
        "protocolVersion": PROTOCOL_VERSION,
        "name": name,
        "description": description,
        "version": version,
        "capabilities": {
            "streaming": false,
            "pushNotifications": false,
            "stateTransitionHistory": true,
        },
        "defaultInputModes": ["text/plain"],
        "defaultOutputModes": ["text/plain", "application/json"],
        "skills": skills,
    });
    let url = match endpoint {
        Some(endpoint) => endpoint.to_string(),
        None => string_field(&authored, "url")?.unwrap_or_default(),
    };
    if !url.is_empty() {
        document["url"] = json!(url);
    }
    Ok(document.to_string())
}

/// The authored description record, validated field by field.
fn authored(description_json: &str) -> Result<serde_json::Map<String, Value>, AgentError> {
    let trimmed = description_json.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::Map::new());
    }
    let value: Value = serde_json::from_str(trimmed).map_err(|error| {
        AgentError::A2a(format!("the A2A description record is not valid JSON: {error}"))
    })?;
    value.as_object().cloned().ok_or_else(|| {
        AgentError::A2a(
            "the A2A description record must be a JSON object with the authored string fields \
             name, description, version and url"
                .to_string(),
        )
    })
}

/// One authored string field; a non-string value is refused, not coerced.
fn string_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<String>, AgentError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(AgentError::A2a(format!(
            "the A2A description field '{key}' must be a string"
        ))),
    }
}

// ── the request handler ──────────────────────────────────────────────────

/// `a2a_handle(run, request)`: answer one A2A JSON-RPC request.
///
/// A malformed document is a typed failure of the host call, which is what
/// lets a caller wire this handler under any HTTP stack; a well-formed request
/// always produces a JSON-RPC document, including for an unsupported method.
pub(crate) fn handle(run_handle: i64, request: &str) -> Result<String, AgentError> {
    let value: Value = serde_json::from_str(request).map_err(|error| {
        AgentError::A2a(format!("the A2A request is not a JSON document: {error}"))
    })?;
    if !value.is_object() {
        return Err(AgentError::A2a(
            "the A2A request must be a JSON-RPC object (batch requests are not supported)"
                .to_string(),
        ));
    }
    Ok(dispatch(run_handle, &value).to_string())
}

/// Routes one parsed request to its method.
fn dispatch(run_handle: i64, request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    match method {
        "message/send" => send_message(run_handle, id, &params),
        "tasks/get" => get_task(run_handle, id, &params),
        "tasks/cancel" => cancel_task(run_handle, id, &params),
        other => wire::failure(
            id,
            METHOD_NOT_FOUND,
            &format!("the A2A method '{other}' is not served by this project"),
        ),
    }
}

/// `message/send`: delegate a task and answer with its (terminal) Task.
fn send_message(run_handle: i64, id: Value, params: &Value) -> Value {
    let Some(message) = params.get("message") else {
        return wire::failure(id, INVALID_PARAMS, "message/send needs a 'message' object");
    };
    let Some(text) = message_text(message) else {
        return wire::failure(
            id,
            INVALID_PARAMS,
            "message/send needs a message with at least one text part",
        );
    };
    // The client may name the task; otherwise the id is fresh. A named id is
    // what makes polling and resume possible, and the adapter never invents a
    // second identity for the same request.
    let task_id = message
        .get("messageId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(crate::journal::new_run_id);
    match run_task(run_handle, &task_id, TaskQuery::Send(&text)) {
        Ok(TaskOutcome::Task(task)) => wire::ok(id, json!({"task": task})),
        Ok(TaskOutcome::NotFound) => wire::failure(
            id,
            TASK_NOT_FOUND,
            &format!("no A2A task '{task_id}' is journaled by this project"),
        ),
        // A send always carries the request, so it is never "not cancelable";
        // the arm keeps the mapping total.
        Ok(TaskOutcome::NotCancelable) => wire::failure(
            id,
            INTERNAL,
            &format!("A2A task '{task_id}' could not be created"),
        ),
        Err(error) => wire::failure(id, failure_code(&error), &error.message()),
    }
}

/// `tasks/get`: the task's current state, resuming an interrupted task.
fn get_task(run_handle: i64, id: Value, params: &Value) -> Value {
    let Some(task_id) = task_id_of(params) else {
        return wire::failure(id, INVALID_PARAMS, "tasks/get needs a task 'id'");
    };
    match run_task(run_handle, &task_id, TaskQuery::Poll) {
        Ok(TaskOutcome::Task(task)) => wire::ok(id, json!({"task": task})),
        Ok(TaskOutcome::NotFound) => wire::failure(
            id,
            TASK_NOT_FOUND,
            &format!("no A2A task '{task_id}' is journaled by this project"),
        ),
        Ok(TaskOutcome::NotCancelable) => wire::failure(
            id,
            INTERNAL,
            &format!("A2A task '{task_id}' could not be read"),
        ),
        Err(error) => wire::failure(id, failure_code(&error), &error.message()),
    }
}

/// `tasks/cancel`: close a task that never reached a terminal state.
fn cancel_task(run_handle: i64, id: Value, params: &Value) -> Value {
    let Some(task_id) = task_id_of(params) else {
        return wire::failure(id, INVALID_PARAMS, "tasks/cancel needs a task 'id'");
    };
    match run_task(run_handle, &task_id, TaskQuery::Cancel) {
        Ok(TaskOutcome::Task(task)) => wire::ok(id, json!({"task": task})),
        Ok(TaskOutcome::NotFound) => wire::failure(
            id,
            TASK_NOT_FOUND,
            &format!("no A2A task '{task_id}' is journaled by this project"),
        ),
        Ok(TaskOutcome::NotCancelable) => wire::failure(
            id,
            TASK_NOT_CANCELABLE,
            &format!("A2A task '{task_id}' already reached a terminal state"),
        ),
        Err(error) => wire::failure(id, failure_code(&error), &error.message()),
    }
}

/// The task id a `tasks/*` request names.
fn task_id_of(params: &Value) -> Option<String> {
    params
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// The text of an A2A message's text parts, in order.
///
/// A part is text when its `kind` is `"text"` (the `0.3` binding) or absent
/// and it carries a `text` field. Anything else is skipped rather than
/// guessed at, so a non-text part contributes nothing to the prompt.
fn message_text(message: &Value) -> Option<String> {
    let parts = message.get("parts")?.as_array()?;
    let mut text = String::new();
    for part in parts {
        let is_text = match part.get("kind") {
            Some(value) => value.as_str() == Some("text"),
            None => true,
        };
        if !is_text {
            continue;
        }
        let Some(piece) = part.get("text").and_then(Value::as_str) else {
            continue;
        };
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(piece);
    }
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// A JSON-RPC error code for a failure that never reached a task.
fn failure_code(error: &AgentError) -> i64 {
    match error {
        AgentError::A2a(_) => INVALID_REQUEST,
        _ => INTERNAL,
    }
}

// ── the task lifecycle ───────────────────────────────────────────────────

/// What a caller wants from a task.
enum TaskQuery<'a> {
    /// Delegate a task (or resume an already-journaled one).
    Send(&'a str),
    /// Read the task's current state, resuming it when it was interrupted.
    Poll,
    /// Close a task that never reached a terminal state.
    Cancel,
}

/// The result of resolving a task.
enum TaskOutcome {
    Task(Value),
    /// No task is journaled under that id.
    NotFound,
    /// The task already reached a terminal state.
    NotCancelable,
}

/// Resolves a task from its journal, driving it forward when it is interrupted.
///
/// The task id **is** the run id and the task's request and terminal state are
/// `task` journal steps, so this single path serves delegation, polling, resume
/// and cancellation: a recorded terminal state is returned as-is, a recorded
/// request with a missing terminal state resumes the run (recorded model and
/// tool steps return their recorded outputs, so no effect executes twice), and
/// a missing request means the task does not exist.
fn run_task(serving: i64, task_id: &str, query: TaskQuery<'_>) -> Result<TaskOutcome, AgentError> {
    let mut spec: AgentSpec = run::with_run(serving, |state| state.spec.clone())?;
    spec.run_id = task_id.to_string();
    let journal = if spec.journal.is_empty() {
        // Without a journal a task has no durable identity: it can be delegated
        // but not polled, resumed or canceled. Recorded, not silently degraded.
        None
    } else {
        Some(Journal::open(
            task_id,
            &spec.journal,
            spec.journal_payloads,
        )?)
    };
    let handle = run::alloc_run(spec, task_id.to_string(), journal)?;
    let outcome = run_task_body(handle, task_id, query);
    // The delegated run is released whatever the outcome: a rejected or failed
    // task must not leak its handle.
    let released = run::take_run(handle);
    match (outcome, released) {
        (Ok(outcome), Ok(_)) => Ok(outcome),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

/// [`run_task`] with the delegated run allocated.
fn run_task_body(handle: i64, task_id: &str, query: TaskQuery<'_>) -> Result<TaskOutcome, AgentError> {
    let supplied = match &query {
        TaskQuery::Send(prompt) => Some(*prompt),
        TaskQuery::Poll | TaskQuery::Cancel => None,
    };
    let cancel = matches!(&query, TaskQuery::Cancel);
    let prompt = match replay::resolve(handle, Kind::Task, &request_input(task_id))? {
        Resolved::Recorded(record) => {
            let recorded = recorded_prompt(&record.output)?;
            // A task id is the run id: a send that names a journaled task must
            // repeat its request. A different message is refused rather than
            // silently answered with the recorded task's outcome.
            if let Some(supplied) = supplied {
                if supplied != recorded {
                    return Err(AgentError::A2a(format!(
                        "A2A task '{task_id}' already holds a different request; a task id is a \
                         run id and is not reused"
                    )));
                }
            }
            recorded
        }
        Resolved::Fresh(token) => match supplied {
            Some(prompt) => {
                // The request is durable before the task runs, so a crash
                // resumes the same prompt instead of losing or inventing one.
                let document = json!({"task": task_id, "message": prompt}).to_string();
                replay::commit(
                    handle,
                    &token,
                    Some(&request_input(task_id)),
                    &document,
                    StepUsage::default(),
                    -1,
                    None,
                )?;
                prompt.to_string()
            }
            // Polling or canceling a task with no recorded request: the task
            // this journal would hold does not exist.
            None => return Ok(TaskOutcome::NotFound),
        },
    };

    match replay::resolve(handle, Kind::Task, &result_input(task_id))? {
        // A recorded terminal state is the task's answer: a poll returns it and
        // a repeated send is idempotent, without executing anything again.
        Resolved::Recorded(record) => {
            let task = serde_json::from_str::<Value>(&record.output).map_err(|error| {
                AgentError::A2a(format!("the recorded A2A task state is invalid: {error}"))
            })?;
            if cancel {
                return Ok(TaskOutcome::NotCancelable);
            }
            Ok(TaskOutcome::Task(task))
        }
        Resolved::Fresh(token) => match cancel {
            true => {
                let task = canceled_task(task_id);
                replay::commit(
                    handle,
                    &token,
                    Some(&result_input(task_id)),
                    &task.to_string(),
                    StepUsage::default(),
                    -1,
                    None,
                )?;
                Ok(TaskOutcome::Task(task))
            }
            false => {
                let task = execute(handle, task_id, &prompt)?;
                replay::commit(
                    handle,
                    &token,
                    Some(&result_input(task_id)),
                    &task.to_string(),
                    StepUsage::default(),
                    -1,
                    None,
                )?;
                Ok(TaskOutcome::Task(task))
            }
        },
    }
}

/// The journal input that identifies a task's request step.
fn request_input(task_id: &str) -> String {
    format!("a2a-request\0{task_id}")
}

/// The journal input that identifies a task's terminal-state step.
fn result_input(task_id: &str) -> String {
    format!("a2a-result\0{task_id}")
}

/// The prompt a recorded request carries.
fn recorded_prompt(document: &str) -> Result<String, AgentError> {
    let value: Value = serde_json::from_str(document).map_err(|error| {
        AgentError::A2a(format!("the recorded A2A task request is invalid: {error}"))
    })?;
    value
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            AgentError::A2a("the recorded A2A task request carries no message".to_string())
        })
}

/// Runs one delegated task through the governed agent loop.
///
/// `act` is the same loop a local program uses: it enforces the run's grant
/// before its first dispatch, charges the tool-call ceiling, journals every
/// model turn and tool invocation, and applies the taint gate inside each
/// tool's host calls. Its outcome is mapped onto an A2A state with a stable
/// reason; a refusal or a crossed ceiling is a task state, never a panic and
/// never a silent success.
fn execute(handle: i64, task_id: &str, prompt: &str) -> Result<Value, AgentError> {
    let outcome = run::in_run_scope(handle, || crate::act::act(handle, prompt));
    let (report, cancelled) = run::with_run(handle, |state| {
        (
            state.report_json(),
            state.cancelled.then(|| state.cancelled_error()),
        )
    })?;
    let (state, reason, answer) = match outcome {
        Ok(answer) => match cancelled {
            // The last turn crossed a ceiling: the run's resource decision is
            // the task's outcome, and it names the ceiling.
            Some(error) => ("failed", error.message(), None),
            None => ("completed", "completed".to_string(), Some(answer)),
        },
        Err(error @ AgentError::CapabilityDenied(_)) => ("rejected", error.message(), None),
        Err(error) => ("failed", error.message(), None),
    };
    Ok(task_document(
        task_id,
        state,
        &reason,
        answer.as_deref(),
        &report,
    ))
}

/// One A2A Task document.
fn task_document(
    task_id: &str,
    state: &str,
    reason: &str,
    answer: Option<&str>,
    report: &str,
) -> Value {
    let metadata = serde_json::from_str::<Value>(report).unwrap_or(Value::Null);
    json!({
        "kind": "task",
        "id": task_id,
        "contextId": task_id,
        "status": {
            "state": state,
            "message": {
                "messageId": format!("{task_id}-status"),
                "role": "agent",
                "parts": [{"kind": "text", "text": reason}],
            },
            "timestamp": crate::journal::now_ms(),
        },
        "artifacts": match answer {
            Some(text) => json!([{
                "artifactId": format!("{task_id}-result"),
                "name": "result",
                "parts": [{"kind": "text", "text": text}],
            }]),
            None => json!([]),
        },
        // The run report travels as task metadata: it is how a client observes
        // that the delegated work went through the governed path (steps,
        // tool_calls) and whether the run replayed a journal.
        "metadata": {"spectra": {"report": metadata}},
    })
}

/// The terminal state a `tasks/cancel` records.
fn canceled_task(task_id: &str) -> Value {
    json!({
        "kind": "task",
        "id": task_id,
        "contextId": task_id,
        "status": {
            "state": "canceled",
            "message": {
                "messageId": format!("{task_id}-status"),
                "role": "agent",
                "parts": [{"kind": "text", "text": "canceled before completion"}],
            },
            "timestamp": crate::journal::now_ms(),
        },
        "artifacts": [],
        "metadata": {"spectra": {"report": Value::Null}},
    })
}

// ── the in-crate listener ────────────────────────────────────────────────

/// `a2a_serve(run, bind, description_json)`: start the in-process listener.
///
/// Returns the bound `host:port`. `POST` at any path answers one A2A JSON-RPC
/// request; `GET /.well-known/agent-card.json` (or `/`) answers the card, whose
/// `url` is the bound endpoint. The listener serves the run for as long as it
/// lives and stops on the first request that arrives after `agent_end`.
pub(crate) fn serve(
    run_handle: i64,
    bind: &str,
    description_json: &str,
) -> Result<String, AgentError> {
    run::with_run(run_handle, |_| ())?;
    // The authored record is validated before the socket exists, so a malformed
    // card is refused at serve time rather than on the first client.
    let _ = card(run_handle, description_json)?;
    let (listener, authority) = net::bind(bind)
        .map_err(|error| AgentError::A2a(format!("could not bind the A2A listener: {error}")))?;
    let endpoint = format!("http://{authority}/");
    let description = description_json.to_string();
    let handler: net::Handler = Arc::new(move |request: &net::Request| {
        if run::with_run(run_handle, |_| ()).is_err() {
            // The serving run has ended: its surface is gone with it.
            return net::Response::text(410, "Gone", "the serving run has ended").stop();
        }
        if request.method == "GET" {
            if request.path == CARD_PATH || request.path == "/" {
                return match card_for(run_handle, &description, Some(&endpoint)) {
                    Ok(card) => net::Response::json(card),
                    Err(error) => net::Response::text(400, "Bad Request", error.message()),
                };
            }
            return net::Response::text(
                404,
                "Not Found",
                format!(
                    "the A2A endpoint serves the agent card at {CARD_PATH} and JSON-RPC at POST"
                ),
            );
        }
        if request.method != "POST" {
            return net::Response::text(
                405,
                "Method Not Allowed",
                format!(
                    "the A2A endpoint accepts GET and POST, found {}",
                    request.method
                ),
            );
        }
        let body = match handle(run_handle, &request.body) {
            Ok(body) => body,
            Err(error) => wire::failure(Value::Null, INVALID_REQUEST, &error.message()).to_string(),
        };
        net::Response::json(body)
    });
    net::spawn(listener, "spectra-a2a", handler)
        .map_err(|error| AgentError::A2a(format!("could not start the A2A listener: {error}")))?;
    Ok(authority)
}

#[cfg(test)]
mod tests {
    use std::sync::{LazyLock, Mutex};

    use super::*;
    use crate::approval;
    use crate::provider::transport::clear_http_transport;
    use crate::spec::AgentSpec;

    /// The tool bodies an execution invoked, in order.
    fn tool_log() -> &'static Mutex<Vec<String>> {
        static LOG: LazyLock<Mutex<Vec<String>>> = LazyLock::new(|| Mutex::new(Vec::new()));
        &LOG
    }

    /// How many tool bodies ran.
    fn tools_called() -> usize {
        tool_log()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// A tool wrapper with the ADR 0019 ABI: it echoes `n` and records the call.
    extern "C" fn counting_tool(_run: i64, args: i64, out: i64) -> i64 {
        let arguments = crate::abi::read_string_arg(args).unwrap_or_default();
        tool_log()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(arguments.clone());
        let n = serde_json::from_str::<Value>(&arguments)
            .ok()
            .and_then(|value| value.get("n").and_then(Value::as_i64))
            .unwrap_or(0);
        let pointer = unsafe { crate::abi::alloc_string(&n.to_string()) };
        unsafe { *(out as *mut i64) = pointer };
        0
    }

    fn journal_dir(tag: &str) -> String {
        std::env::temp_dir()
            .join(format!("spectra-a2a-{tag}-{}", crate::journal::new_run_id()))
            .to_string_lossy()
            .to_string()
    }

    /// A serving run with the mock provider and the given grants.
    fn serving_run(journal: &str, allow: &[&str]) -> i64 {
        let allow: Vec<String> = allow.iter().map(|value| value.to_string()).collect();
        let spec = AgentSpec::parse(&format!(
            r#"{{"goal":"a2a-agent","model":"mock/echo","endpoint":"mock:","allow":{},
                 "journal":{},"seed":7}}"#,
            serde_json::to_string(&allow).expect("grants"),
            serde_json::to_string(journal).expect("dir"),
        ))
        .expect("valid spec");
        run::alloc_run(spec, crate::journal::new_run_id(), None).expect("alloc")
    }

    /// Serializes tests that mutate the process-global registry and tools.
    fn with_registry<T>(work: impl FnOnce() -> T) -> T {
        let _guard = crate::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _registry = crate::tools::REGISTRY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        spectra_runtime::ffi::clear_host_functions();
        spectra_runtime::register();
        crate::register();
        clear_http_transport();
        approval::set_approver(None);
        tools::clear();
        tool_log()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        let result = work();
        tools::clear();
        clear_http_transport();
        approval::set_approver(None);
        result
    }

    fn register_counting_tool() {
        assert!(tools::register(
            "count".to_string(),
            counting_tool as *const () as usize as i64,
            "Counts to the given integer".to_string(),
            r#"{"type":"object","properties":{"n":{"type":"integer"}}}"#.to_string(),
            "[]",
        ));
    }

    fn send(authority: i64, body: &str) -> Value {
        serde_json::from_str(&handle(authority, body).expect("served")).expect("json")
    }

    fn message(task_id: &str, text: &str) -> String {
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "message/send",
            "params": {"message": {"messageId": task_id, "role": "user",
                                   "parts": [{"kind": "text", "text": text}]}},
        })
        .to_string()
    }

    #[test]
    fn the_card_advertises_the_derived_tool_surface() {
        with_registry(|| {
            tools::register(
                "count".to_string(),
                0x1000,
                "Counts to the given integer".to_string(),
                r#"{"type":"object","properties":{"n":{"type":"integer"}}}"#.to_string(),
                r#"["spectra.std.fs.fs_write"]"#,
            );
            let serving = serving_run("", &[]);
            let document: Value = serde_json::from_str(
                &card(serving, r#"{"name":"Counter","description":"Counts","version":"9.9.9"}"#)
                    .expect("card"),
            )
            .expect("json");
            assert_eq!(document["name"], "Counter");
            assert_eq!(document["description"], "Counts");
            assert_eq!(document["version"], "9.9.9");
            assert_eq!(document["protocolVersion"], PROTOCOL_VERSION);
            // Only implemented capabilities are advertised.
            assert_eq!(document["capabilities"]["streaming"], false);
            assert_eq!(document["capabilities"]["pushNotifications"], false);
            assert_eq!(document["capabilities"]["stateTransitionHistory"], true);
            // The skill is the compiler-derived descriptor plus its effects.
            assert_eq!(document["skills"][0]["id"], "count");
            assert_eq!(
                document["skills"][0]["description"],
                "Counts to the given integer"
            );
            assert_eq!(document["skills"][0]["tags"][0], "spectra.std.fs.fs_write");

            // An empty description defaults the identity to the run's goal; a
            // non-string authored field is refused, never coerced.
            let default: Value =
                serde_json::from_str(&card(serving, "").expect("card")).expect("json");
            assert_eq!(default["name"], "a2a-agent");
            assert!(default.get("url").is_none());
            let error = card(serving, r#"{"name":42}"#).expect_err("refused");
            assert_eq!(error.kind(), "a2a_error");
            let error = card(serving, "[]").expect_err("refused");
            assert_eq!(error.kind(), "a2a_error");
            run::take_run(serving).expect("end");
        });
    }

    #[test]
    fn a_delegated_task_completes_and_is_journaled() {
        with_registry(|| {
            register_counting_tool();
            let dir = journal_dir("complete");
            let serving = serving_run(&dir, &[]);
            let response = send(
                serving,
                &message("task-complete", "spectra:tool=count {\"n\":5}\nspectra:final=done"),
            );
            let task = &response["result"]["task"];
            assert_eq!(task["id"], "task-complete");
            assert_eq!(task["status"]["state"], "completed");
            assert_eq!(
                task["status"]["message"]["parts"][0]["text"], "completed",
                "{response}"
            );
            // The delegated work ran through the governed dispatch: the tool
            // executed once and the report counted it.
            assert_eq!(tools_called(), 1);
            assert_eq!(task["metadata"]["spectra"]["report"]["tool_calls"], 1);
            assert_eq!(
                task["artifacts"][0]["parts"][0]["text"], "done",
                "{response}"
            );

            // The task is a journaled run: the request and the terminal state
            // are `task` steps under the task id's own file.
            let journal = std::fs::read_to_string(format!(
                "{}/{}",
                dir,
                crate::journal::file_name("task-complete")
            ))
            .expect("journal written");
            assert_eq!(journal.matches("\"kind\":\"task\"").count(), 2, "{journal}");
            assert!(journal.contains("\"kind\":\"tool\""), "{journal}");
            run::take_run(serving).expect("end");
        });
    }

    #[test]
    fn a_rejected_task_carries_the_stable_reason() {
        with_registry(|| {
            register_counting_tool();
            let dir = journal_dir("rejected");
            // The serving run grants nothing, and `count`'s derived effect list
            // is empty, so no tool is ungranted. Register an effectful tool for
            // this test so the delegated loop's grant check has something to
            // refuse.
            tools::clear();
            assert!(tools::register(
                "writer".to_string(),
                counting_tool as *const () as usize as i64,
                "Writes".to_string(),
                r#"{"type":"object"}"#.to_string(),
                r#"["spectra.std.fs.fs_write"]"#,
            ));
            let serving = serving_run(&dir, &[]);
            let response = send(serving, &message("task-rejected", "spectra:final=done"));
            let task = &response["result"]["task"];
            assert_eq!(task["status"]["state"], "rejected", "{response}");
            let reason = task["status"]["message"]["parts"][0]["text"]
                .as_str()
                .unwrap_or("");
            assert!(reason.contains("capability_denied"), "{response}");
            // Nothing executed: the refusal happens before the first dispatch.
            assert_eq!(tools_called(), 0);
            run::take_run(serving).expect("end");
        });
    }

    #[test]
    fn polling_a_task_resumes_it_from_the_journal_without_repeating_an_effect() {
        with_registry(|| {
            register_counting_tool();
            let dir = journal_dir("resume");
            let serving = serving_run(&dir, &[]);
            let response = send(
                serving,
                &message("task-resume", "spectra:tool=count {\"n\":3}\nspectra:final=done"),
            );
            assert_eq!(response["result"]["task"]["status"]["state"], "completed");
            assert_eq!(tools_called(), 1);
            run::take_run(serving).expect("end");

            // A different process (here: a fresh serving run) polls the task by
            // its id. The recorded terminal state answers, and the recorded
            // tool step is not executed again.
            let fresh = serving_run(&dir, &[]);
            let polled = send(
                fresh,
                &json!({"jsonrpc": "2.0", "id": 2, "method": "tasks/get",
                        "params": {"id": "task-resume"}})
                .to_string(),
            );
            assert_eq!(polled["result"]["task"]["status"]["state"], "completed");
            assert_eq!(
                polled["result"]["task"]["artifacts"][0]["parts"][0]["text"], "done"
            );
            assert_eq!(tools_called(), 1, "a poll must not repeat a recorded effect");
            run::take_run(fresh).expect("end");

            // An unknown id is a typed TaskNotFound, not an empty task.
            let fresh = serving_run(&dir, &[]);
            let missing = send(
                fresh,
                &json!({"jsonrpc": "2.0", "id": 3, "method": "tasks/get",
                        "params": {"id": "task-absent"}})
                .to_string(),
            );
            assert_eq!(missing["error"]["code"], TASK_NOT_FOUND, "{missing}");
            run::take_run(fresh).expect("end");
        });
    }

    #[test]
    fn a_repeated_send_with_a_different_message_is_refused() {
        with_registry(|| {
            let dir = journal_dir("reuse");
            let serving = serving_run(&dir, &[]);
            let first = send(serving, &message("task-reuse", "spectra:final=one"));
            assert_eq!(first["result"]["task"]["status"]["state"], "completed", "{first}");
            // The identical request is idempotent: the recorded task answers.
            let again = send(serving, &message("task-reuse", "spectra:final=one"));
            assert_eq!(
                again["result"]["task"]["artifacts"][0]["parts"][0]["text"],
                "one",
                "{again}"
            );
            // A different message under a taken id is refused, never silently
            // answered with the recorded task's outcome.
            let conflicting = send(serving, &message("task-reuse", "spectra:final=two"));
            assert_eq!(conflicting["error"]["code"], INVALID_REQUEST, "{conflicting}");
            run::take_run(serving).expect("end");
        });
    }

    #[test]
    fn an_unnamed_message_still_produces_a_task() {
        with_registry(|| {
            let dir = journal_dir("unnamed");
            let serving = serving_run(&dir, &[]);
            let body = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "message/send",
                "params": {"message": {"role": "user", "parts": [{"kind": "text", "text": "spectra:final=hi"}]}},
            })
            .to_string();
            let response = send(serving, &body);
            let task = &response["result"]["task"];
            assert_eq!(task["status"]["state"], "completed", "{response}");
            assert!(task["id"].as_str().unwrap_or("").starts_with("run-"), "{response}");
            run::take_run(serving).expect("end");
        });
    }

    #[test]
    fn canceling_and_unknown_methods_follow_the_protocol() {
        with_registry(|| {
            let dir = journal_dir("cancel");
            let serving = serving_run(&dir, &[]);
            // Delegate, then cancel: a terminal task is not cancelable.
            let done = send(serving, &message("task-done", "spectra:final=ok"));
            assert_eq!(done["result"]["task"]["status"]["state"], "completed");
            let cancel = send(
                serving,
                &json!({"jsonrpc": "2.0", "id": 2, "method": "tasks/cancel",
                        "params": {"id": "task-done"}})
                .to_string(),
            );
            assert_eq!(cancel["error"]["code"], TASK_NOT_CANCELABLE, "{cancel}");

            // A task that was interrupted before its terminal step can be
            // canceled: write only the request step by hand, then cancel it.
            let task_id = "task-interrupted";
            let path = dir.clone() + "/" + &crate::journal::file_name(task_id);
            std::fs::create_dir_all(&dir).expect("dir");
            let run_id = task_id.to_string();
            let input = request_input(task_id);
            let record = json!({
                "run": run_id, "step": 0, "kind": "task",
                "input_digest": crate::digest::of(&input),
                "output_digest": crate::digest::of("{}"),
                "idempotency_key": crate::digest::step_key(&run_id, 0, &crate::digest::of(&input)),
                "seed": 7, "usage": {"tokens_in": 0, "tokens_out": 0, "cost_micros": 0},
                "timestamp": 0, "output": "{\"task\":\"task-interrupted\",\"message\":\"spectra:final=ok\"}",
            });
            std::fs::write(&path, format!("{record}\n")).expect("journal");
            let cancel = send(
                serving,
                &json!({"jsonrpc": "2.0", "id": 3, "method": "tasks/cancel",
                        "params": {"id": task_id}})
                .to_string(),
            );
            assert_eq!(cancel["result"]["task"]["status"]["state"], "canceled", "{cancel}");
            // And the canceled state is what a later poll reads.
            let polled = send(
                serving,
                &json!({"jsonrpc": "2.0", "id": 4, "method": "tasks/get",
                        "params": {"id": task_id}})
                .to_string(),
            );
            assert_eq!(polled["result"]["task"]["status"]["state"], "canceled", "{polled}");

            // Unsupported methods and malformed documents are typed results.
            let unsupported = send(
                serving,
                &json!({"jsonrpc": "2.0", "id": 5, "method": "tasks/pushNotificationConfig/set"}).to_string(),
            );
            assert_eq!(unsupported["error"]["code"], METHOD_NOT_FOUND, "{unsupported}");
            let error = handle(serving, "not json").expect_err("typed failure");
            assert_eq!(error.kind(), "a2a_error");
            run::take_run(serving).expect("end");
        });
    }

    #[test]
    fn a_third_party_client_reaches_the_card_and_a_task_over_real_http() {
        with_registry(|| {
            use std::io::{Read, Write};
            use std::net::TcpStream;

            register_counting_tool();
            let dir = journal_dir("http");
            let serving = serving_run(&dir, &[]);
            let authority =
                serve(serving, "127.0.0.1:0", r#"{"name":"Counter"}"#).expect("served");
            assert!(authority.starts_with("127.0.0.1:"), "{authority}");

            let mut stream = TcpStream::connect(&authority).expect("connect");
            let request = format!(
                "GET {CARD_PATH} HTTP/1.1\r\nhost: {authority}\r\nconnection: close\r\n\r\n"
            );
            stream.write_all(request.as_bytes()).expect("write");
            let mut raw = String::new();
            stream.read_to_string(&mut raw).expect("read");
            let card: Value =
                serde_json::from_str(raw.split_once("\r\n\r\n").expect("body").1).expect("json");
            assert_eq!(card["name"], "Counter");
            assert_eq!(card["url"], format!("http://{authority}/"));
            assert_eq!(card["skills"][0]["id"], "count");

            let payload = message("task-http", "spectra:tool=count {\"n\":1}\nspectra:final=done");
            let mut stream = TcpStream::connect(&authority).expect("connect");
            let request = format!(
                "POST / HTTP/1.1\r\nhost: {authority}\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n{payload}",
                payload.len()
            );
            stream.write_all(request.as_bytes()).expect("write");
            let mut raw = String::new();
            stream.read_to_string(&mut raw).expect("read");
            let response: Value =
                serde_json::from_str(raw.split_once("\r\n\r\n").expect("body").1).expect("json");
            assert_eq!(
                response["result"]["task"]["status"]["state"], "completed",
                "{response}"
            );
            assert_eq!(tools_called(), 1);

            // After the run ends the listener answers 410 and stops.
            run::take_run(serving).expect("end");
            let mut stream = TcpStream::connect(&authority).expect("connect");
            let request = format!(
                "POST / HTTP/1.1\r\nhost: {authority}\r\ncontent-length: {}\r\n\
                 connection: close\r\n\r\n{payload}",
                payload.len()
            );
            stream.write_all(request.as_bytes()).expect("write");
            let mut raw = String::new();
            stream.read_to_string(&mut raw).expect("read");
            assert!(raw.starts_with("HTTP/1.1 410"), "{raw}");
        });
    }

    /// The counter is shared: a test that forgot to clear it would make the
    /// "executed once" assertions meaningless, so the harness clears it and
    /// this pins the starting state.
    #[test]
    fn the_tool_log_is_cleared_by_the_harness() {
        with_registry(|| {
            assert_eq!(tools_called(), 0);
        });
    }
}
