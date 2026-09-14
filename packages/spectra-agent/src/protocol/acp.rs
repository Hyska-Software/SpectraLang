//! ACP exposure: the agent surface and the permission bridge (R-3219 T2).
//!
//! The Agent Client Protocol asks the agent one question the platform already
//! answers — *may I perform this action?* — and this module answers it with the
//! approval primitive instead of a second, protocol-shaped policy:
//!
//! * [`permission`] builds the ACP `session/request_permission` request
//!   (`sessionId`, the action as the tool call, and three options:
//!   allow-once / allow-always / reject), asks the attached [`AcpClient`], maps
//!   the client's selected option onto a [`Decision`], and hands that decision
//!   to [`crate::approval::approve_with`]. Everything the primitive guarantees
//!   therefore holds unchanged: the decision is journaled with its attribution,
//!   `allow-always` is cached on the run, and a replayed run never re-asks a
//!   decided action — the attached client is not even consulted.
//! * [`handle`] serves the client's own requests (`initialize`, `session/new`,
//!   `session/prompt`, `session/cancel`). Only implemented capabilities are
//!   advertised: no session loading, no image/audio/embedded-context prompts,
//!   no MCP-over-ACP transport, and `session/prompt` runs the agent loop
//!   through [`crate::act::act`], so the usual grant, ceiling, journal and
//!   taint enforcement applies to an ACP-driven turn exactly as to a local one.
//!
//! # Fail closed
//!
//! With no client attached the answer is a `deny`, attributed to
//! `default-deny (no ACP client attached)` and journaled like any other
//! decision; there is no implicit allow, matching `approve`'s contract. A
//! client answer the adapter cannot map — a selected option it never offered,
//! or a document with no usable outcome — is a typed failure and **never** an
//! allow: the action stays unauthorized.
//!
//! One session is served per run, and its ACP `sessionId` is the run's id.
//! That is the honest mapping for a host-call adapter: the run *is* the session
//! the platform scopes grants, budget and journal to.

use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use serde_json::{json, Value};

use crate::approval::{self, ApprovalRequest, Decision};
use crate::error::AgentError;
use crate::mcp::wire;
use crate::run;

/// ACP protocol version this adapter speaks (the numeric `v1` binding).
pub(crate) const PROTOCOL_VERSION: i64 = 1;

/// The methods this adapter implements.
///
/// `session/request_permission` is listed because the adapter *sends* it; the
/// rest it answers. Anything absent from this list is not advertised.
const IMPLEMENTED_METHODS: [&str; 5] = [
    "initialize",
    "session/new",
    "session/prompt",
    "session/cancel",
    "session/request_permission",
];

/// JSON-RPC error codes this adapter uses.
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL: i64 = -32603;

// ── the client seam ──────────────────────────────────────────────────────

/// The client end of an ACP session.
///
/// The embedding application owns the pipe — stdio, a socket, a UI — and
/// implements this trait to answer the agent's `session/request_permission`
/// requests. The answer is the JSON-RPC **response document** the client
/// returned; the adapter maps it onto a decision and never treats an
/// unmappable answer as an allow.
///
/// The error type is a string for the same reason
/// [`crate::provider::transport::HttpTransport`] uses one: the trait is
/// implemented outside this crate, which owns no channel for its typed errors.
pub trait AcpClient: Send + Sync + 'static {
    /// Asks the client to decide, returning its JSON-RPC response document.
    fn request_permission(&self, request: &str) -> Result<String, String>;
}

fn slot() -> &'static Mutex<Option<Arc<dyn AcpClient>>> {
    static SLOT: LazyLock<Mutex<Option<Arc<dyn AcpClient>>>> = LazyLock::new(|| Mutex::new(None));
    &SLOT
}

fn lock() -> MutexGuard<'static, Option<Arc<dyn AcpClient>>> {
    slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Attaches (or removes) the process-global ACP client.
///
/// Replaces any previous client — a session's UI is not "first installation
/// wins", the same contract `set_approver` has. Returns `true` when a client
/// is now attached.
pub fn set_acp_client(client: Option<Arc<dyn AcpClient>>) -> bool {
    let attached = client.is_some();
    *lock() = client;
    attached
}

/// The installed client, if any.
fn client() -> Option<Arc<dyn AcpClient>> {
    lock().clone()
}

// ── the permission bridge ────────────────────────────────────────────────

/// `acp_permission(run, action)`: ask the attached client, journal the decision.
///
/// True when the action is authorized. The decision is the client's selected
/// option mapped onto the run's approval decision — `allow_once` authorizes one
/// step, `allow_always` the run, both rejections deny — and it is journaled by
/// [`crate::approval::approve_with`] before this returns, so a denial is as
/// durable and as attributable as an approval. A replayed run returns the
/// recorded decision without asking the client again.
pub(crate) fn permission(run_handle: i64, action: &str) -> Result<bool, AgentError> {
    if action.trim().is_empty() {
        return Err(AgentError::Acp(
            "an ACP permission request needs a non-empty action name".to_string(),
        ));
    }
    let installed = client();
    approval::approve_with(run_handle, action, |request| match &installed {
        Some(client) => {
            let answer = client
                .request_permission(&permission_request(request))
                .map_err(|error| {
                    AgentError::Acp(format!("the ACP client could not answer: {error}"))
                })?;
            decision_from_answer(&answer)
        }
        // No UI attached: deny, attributed, exactly like the default approver.
        None => Ok(Decision::Deny {
            by: "default-deny (no ACP client attached)".to_string(),
        }),
    })
}

/// The ACP `session/request_permission` request for one governed action.
///
/// The action is carried twice: as the tool call's title (what a UI shows) and
/// as `_meta.spectra.action` (the exact host-call name the journal records).
fn permission_request(request: &ApprovalRequest) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "session/request_permission",
        "params": {
            "sessionId": request.run,
            "toolCall": {
                "toolCallId": format!("approve-{}", request.step),
                "title": request.action,
                "kind": "other",
                "status": "pending",
            },
            "options": [
                {
                    "optionId": "allow_once",
                    "name": "Allow once",
                    "kind": "allow_once",
                },
                {
                    "optionId": "allow_always",
                    "name": "Allow always for this run",
                    "kind": "allow_always",
                },
                {
                    "optionId": "reject_once",
                    "name": "Reject",
                    "kind": "reject_once",
                },
            ],
            "_meta": {"spectra": {"action": request.action, "goal": request.goal}},
        },
    })
    .to_string()
}

/// Maps the client's answer document onto a decision.
///
/// Fail closed: only the options this adapter offered are recognized, and a
/// document the adapter cannot read is a typed failure rather than an allow.
fn decision_from_answer(answer: &str) -> Result<Decision, AgentError> {
    let text = answer.trim();
    if text.is_empty() {
        return Err(AgentError::Acp(
            "the ACP client answered the permission request with no document".to_string(),
        ));
    }
    let value: Value = serde_json::from_str(text).map_err(|error| {
        AgentError::Acp(format!(
            "the ACP client's permission answer is not a JSON document: {error}"
        ))
    })?;
    if let Some(error) = value.get("error") {
        // A client that reports an error has not authorized anything; the
        // denial is still a decision, so it is journaled rather than dropped.
        let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
        return Ok(Decision::Deny {
            by: format!("acp-client error {code}"),
        });
    }
    let outcome = value
        .get("result")
        .unwrap_or(&value)
        .get("outcome")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            AgentError::Acp(
                "the ACP client's permission answer carries no outcome".to_string(),
            )
        })?;
    match outcome.get("outcome").and_then(Value::as_str) {
        Some("selected") => match outcome.get("optionId").and_then(Value::as_str) {
            Some("allow_once") => Ok(Decision::AllowOnce {
                by: "acp-client".to_string(),
            }),
            Some("allow_always") => Ok(Decision::AllowAlways {
                by: "acp-client".to_string(),
            }),
            Some("reject_once") | Some("reject_always") => Ok(Decision::Deny {
                by: "acp-client".to_string(),
            }),
            _ => Err(AgentError::Acp(
                "the ACP client selected an option this adapter never offered".to_string(),
            )),
        },
        // The turn was cancelled: no authorization.
        Some("cancelled") => Ok(Decision::Deny {
            by: "acp-client (cancelled)".to_string(),
        }),
        _ => Err(AgentError::Acp(
            "the ACP client's permission answer has no usable outcome".to_string(),
        )),
    }
}

// ── the request handler ──────────────────────────────────────────────────

/// `acp_handle(run, request)`: answer one ACP JSON-RPC request.
///
/// A notification (`session/cancel`) returns the empty string, because a
/// notification has no response. A malformed document is a typed failure,
/// which is what lets a caller wire this handler under any transport.
pub(crate) fn handle(run_handle: i64, request: &str) -> Result<String, AgentError> {
    // The session is the run, so a request against a run that has ended is
    // refused like every other call on a dead handle: an adapter that kept
    // answering `initialize` for a released run would tell a client the
    // session is still there.
    run::with_run(run_handle, |_| ())?;
    let value: Value = serde_json::from_str(request).map_err(|error| {
        AgentError::Acp(format!("the ACP request is not a JSON document: {error}"))
    })?;
    if !value.is_object() {
        return Err(AgentError::Acp(
            "the ACP request must be a JSON-RPC object (batch requests are not supported)"
                .to_string(),
        ));
    }
    Ok(match dispatch(run_handle, &value) {
        Some(document) => document.to_string(),
        None => String::new(),
    })
}

/// Routes one parsed request to its method.
fn dispatch(run_handle: i64, request: &Value) -> Option<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    match method {
        "initialize" => Some(wire::ok(id, capabilities())),
        "session/new" => Some(new_session(run_handle, id)),
        "session/prompt" => Some(prompt(run_handle, id, &params)),
        // A notification: no response document.
        "session/cancel" => None,
        other => Some(wire::failure(
            id,
            METHOD_NOT_FOUND,
            &format!("the ACP method '{other}' is not served by this adapter"),
        )),
    }
}

/// The `initialize` result: only implemented capabilities.
fn capabilities() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "agentCapabilities": {
            // Not implemented: a session is the served run, so there is no
            // stored session to load.
            "loadSession": false,
            // Not implemented: prompts are text.
            "promptCapabilities": {
                "image": false,
                "audio": false,
                "embeddedContext": false,
            },
            // Not implemented: an ACP session reaches tools through the
            // process's own governed registry, not an MCP-over-ACP transport.
            "mcpCapabilities": {"http": false, "sse": false},
        },
        "authMethods": [],
        "_meta": {"spectra": {
            "implementedMethods": IMPLEMENTED_METHODS,
            // The adapter sends session/request_permission; the bridge maps the
            // client's answer onto the run's journaled approval decision.
            "permissionRequests": true,
        }},
    })
}

/// `session/new`: the run's session id.
fn new_session(run_handle: i64, id: Value) -> Value {
    match run::with_run(run_handle, |state| state.run_id.clone()) {
        Ok(run_id) => wire::ok(id, json!({"sessionId": run_id})),
        Err(error) => wire::failure(id, INTERNAL, &error.message()),
    }
}

/// `session/prompt`: run one turn through the governed agent loop.
fn prompt(run_handle: i64, id: Value, params: &Value) -> Value {
    let run_id = match run::with_run(run_handle, |state| state.run_id.clone()) {
        Ok(run_id) => run_id,
        Err(error) => return wire::failure(id, INTERNAL, &error.message()),
    };
    match params.get("sessionId").and_then(Value::as_str) {
        Some(session) if session == run_id => {}
        Some(_) => {
            return wire::failure(
                id,
                INVALID_PARAMS,
                "the ACP session id is not this run's; session/new returns the only session \
                 this adapter serves",
            )
        }
        None => return wire::failure(id, INVALID_PARAMS, "session/prompt needs a 'sessionId'"),
    }
    let Some(text) = prompt_text(params) else {
        return wire::failure(
            id,
            INVALID_PARAMS,
            "session/prompt needs at least one text content block",
        );
    };

    // The same loop a local program runs: grant enforcement, the tool-call
    // ceiling, the journal and the taint gate all apply.
    let outcome = run::in_run_scope(run_handle, || crate::act::act(run_handle, &text));
    match outcome {
        Ok(answer) => wire::ok(id, prompt_response("end_turn", Some(&answer), None)),
        Err(error @ AgentError::CapabilityDenied(_)) => {
            wire::ok(id, prompt_response("refusal", None, Some(&error)))
        }
        Err(error @ AgentError::BudgetExceeded(_)) => {
            let reason = if error.detail().contains("max_tokens") {
                "max_tokens"
            } else {
                "refusal"
            };
            wire::ok(id, prompt_response(reason, None, Some(&error)))
        }
        Err(error @ AgentError::ToolLoopCeiling(_)) => {
            wire::ok(id, prompt_response("max_turn_requests", None, Some(&error)))
        }
        // A transport-level failure is not a stop reason: the session cannot
        // answer this turn at all.
        Err(error) => wire::failure(id, INTERNAL, &error.message()),
    }
}

/// The text of an ACP prompt's text content blocks, in order.
///
/// ACP content blocks carry `type` (`"text"`); a block without one and with a
/// `text` field is accepted as text for the same reason the A2A adapter accepts
/// a part without `kind`. Non-text blocks contribute nothing.
fn prompt_text(params: &Value) -> Option<String> {
    let blocks = params.get("prompt")?.as_array()?;
    let mut text = String::new();
    for block in blocks {
        let is_text = match block.get("type") {
            Some(value) => value.as_str() == Some("text"),
            None => true,
        };
        if !is_text {
            continue;
        }
        let Some(piece) = block.get("text").and_then(Value::as_str) else {
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

/// An ACP `PromptResponse`: the stop reason, plus the adapter's own payload
/// under the protocol's `_meta` extension point.
///
/// A request/response transport cannot stream `session/update` notifications,
/// so the agent's text and the typed failure travel in `_meta` instead of being
/// dropped or replaced by a stream this adapter does not have.
fn prompt_response(
    stop_reason: &str,
    agent_text: Option<&str>,
    error: Option<&AgentError>,
) -> Value {
    let mut spectra = serde_json::Map::new();
    if let Some(text) = agent_text {
        spectra.insert("agentText".to_string(), json!(text));
    }
    if let Some(error) = error {
        spectra.insert("reason".to_string(), json!(error.message()));
    }
    json!({"stopReason": stop_reason, "_meta": {"spectra": Value::Object(spectra)}})
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use super::*;
    use crate::approval::set_approver;
    use crate::provider::transport::clear_http_transport;
    use crate::spec::AgentSpec;

    /// A client that answers from a script and counts how often it was asked.
    struct ScriptedClient {
        answers: Mutex<Vec<String>>,
        calls: AtomicUsize,
        requests: Mutex<Vec<String>>,
    }

    impl ScriptedClient {
        fn new(answers: Vec<String>) -> Arc<Self> {
            Arc::new(Self {
                answers: Mutex::new(answers),
                calls: AtomicUsize::new(0),
                requests: Mutex::new(Vec::new()),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn last_request(&self) -> String {
            self.requests
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .last()
                .cloned()
                .unwrap_or_default()
        }
    }

    impl AcpClient for ScriptedClient {
        fn request_permission(&self, request: &str) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.requests
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(request.to_string());
            let mut answers = self
                .answers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if answers.is_empty() {
                return Err("the script is exhausted".to_string());
            }
            Ok(answers.remove(0))
        }
    }

    fn selected(option_id: &str) -> String {
        json!({"jsonrpc": "2.0", "id": 1, "result": {"outcome": {"outcome": "selected", "optionId": option_id}}})
            .to_string()
    }

    fn journal_dir(tag: &str) -> String {
        std::env::temp_dir()
            .join(format!("spectra-acp-{tag}-{}", crate::journal::new_run_id()))
            .to_string_lossy()
            .to_string()
    }

    /// A journaled run, so the decisions a test makes are observable on disk.
    fn journalled_run(dir: &str, run_id: &str) -> i64 {
        let spec = AgentSpec::parse(&format!(
            r#"{{"goal":"acp-agent","model":"mock/echo","endpoint":"mock:","allow":[],
                 "journal":{},"seed":5,"run_id":{}}}"#,
            serde_json::to_string(dir).expect("dir"),
            serde_json::to_string(run_id).expect("id"),
        ))
        .expect("valid spec");
        let journal = crate::journal::Journal::open(run_id, dir, false).expect("journal");
        run::alloc_run(spec, run_id.to_string(), Some(journal)).expect("alloc")
    }

    fn journal_text(dir: &str, run_id: &str) -> String {
        std::fs::read_to_string(format!("{}/{}", dir, crate::journal::file_name(run_id)))
            .unwrap_or_default()
    }

    /// Installs a scripted client through the trait-object boundary.
    fn install(client: &Arc<ScriptedClient>) {
        let dynamic: Arc<dyn AcpClient> = client.clone();
        assert!(set_acp_client(Some(dynamic)));
    }

    /// Serializes tests that mutate process-global state (the tool registry,
    /// the approver and the ACP client) and clears every seam afterwards. The
    /// registry lock is what keeps `act`'s grant check from observing a tool
    /// another module's test is registering right now.
    fn with_seams<T>(work: impl FnOnce() -> T) -> T {
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
        set_approver(None);
        set_acp_client(None);
        crate::tools::clear();
        let result = work();
        crate::tools::clear();
        set_approver(None);
        set_acp_client(None);
        clear_http_transport();
        result
    }

    #[test]
    fn a_denial_is_journaled_and_never_authorizes_the_action() {
        with_seams(|| {
            let dir = journal_dir("deny");
            let client = ScriptedClient::new(vec![selected("reject_once")]);
            install(&client);
            let served = journalled_run(&dir, "acp-deny");

            assert!(
                !permission(served, "spectra.std.fs.fs_write").expect("a decision"),
                "a denial must not authorize the action"
            );
            assert_eq!(client.calls(), 1);
            // The request the client saw is the protocol's shape: the session
            // is the run, the action is the tool call, and the options are the
            // three the adapter offers.
            let request: Value =
                serde_json::from_str(&client.last_request()).expect("request json");
            assert_eq!(request["method"], "session/request_permission");
            assert_eq!(request["params"]["sessionId"], "acp-deny");
            assert_eq!(
                request["params"]["toolCall"]["title"],
                "spectra.std.fs.fs_write"
            );
            assert_eq!(
                request["params"]["options"][1]["kind"], "allow_always"
            );

            // The denial is a journaled, attributed approval decision.
            let journal = journal_text(&dir, "acp-deny");
            assert!(journal.contains("\"kind\":\"approval\""), "{journal}");
            assert!(journal.contains("\"output\":\"false\""), "{journal}");
            assert!(journal.contains("deny by acp-client"), "{journal}");
            run::take_run(served).expect("end");
        });
    }

    #[test]
    fn an_allow_always_is_journaled_cached_and_never_re_asked() {
        with_seams(|| {
            let dir = journal_dir("allow");
            let client = ScriptedClient::new(vec![selected("allow_always")]);
            install(&client);
            let served = journalled_run(&dir, "acp-allow");

            assert!(permission(served, "spectra.std.fs.fs_write").expect("allowed"));
            // `allow-always` is cached on the run: the second request is
            // answered from the cache, so the client is not asked twice.
            assert!(permission(served, "spectra.std.fs.fs_write").expect("allowed again"));
            assert_eq!(client.calls(), 1);
            let journal = journal_text(&dir, "acp-allow");
            assert_eq!(journal.matches("\"kind\":\"approval\"").count(), 2, "{journal}");
            assert!(journal.contains("allow-always by acp-client"), "{journal}");
            assert!(journal.contains("allow-always by cached allow-always"), "{journal}");
            run::take_run(served).expect("end");

            // A resumed run replays the decision: the client is not consulted.
            let resumed = journalled_run(&dir, "acp-allow");
            assert!(permission(resumed, "spectra.std.fs.fs_write").expect("recorded"));
            assert_eq!(client.calls(), 1, "a replayed decision must not re-ask the client");
            run::take_run(resumed).expect("end");
        });
    }

    #[test]
    fn with_no_client_the_permission_is_denied_and_journaled() {
        with_seams(|| {
            let dir = journal_dir("noclient");
            let served = journalled_run(&dir, "acp-none");
            assert!(!permission(served, "spectra.std.fs.fs_write").expect("a decision"));
            let journal = journal_text(&dir, "acp-none");
            assert!(
                journal.contains("deny by default-deny (no ACP client attached)"),
                "{journal}"
            );
            run::take_run(served).expect("end");
        });
    }

    #[test]
    fn an_answer_the_adapter_cannot_map_never_authorizes() {
        with_seams(|| {
            let dir = journal_dir("malformed");
            for answer in [
                selected("allow_everything"),
                r#"{"jsonrpc":"2.0","id":1,"result":{}}"#.to_string(),
                "not json".to_string(),
            ] {
                let client = ScriptedClient::new(vec![answer]);
                install(&client);
                let served = journalled_run(&dir, "acp-malformed");
                let error = permission(served, "spectra.std.fs.fs_write").expect_err("refused");
                assert_eq!(error.kind(), "acp_error", "{error}");
                run::take_run(served).expect("end");
                set_acp_client(None);
            }
            // Nothing was journaled: an unmappable answer is not a decision.
            let journal = journal_text(&dir, "acp-malformed");
            assert!(!journal.contains("\"kind\":\"approval\""), "{journal}");

            // A client that reports an error is a denial, and it is journaled.
            let client = ScriptedClient::new(vec![
                json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32603, "message": "boom"}})
                    .to_string(),
            ]);
            install(&client);
            let served = journalled_run(&dir, "acp-client-error");
            assert!(!permission(served, "spectra.std.fs.fs_write").expect("denied"));
            let journal = journal_text(&dir, "acp-client-error");
            assert!(journal.contains("deny by acp-client error -32603"), "{journal}");
            run::take_run(served).expect("end");
        });
    }

    #[test]
    fn initialize_advertises_only_implemented_capabilities() {
        with_seams(|| {
            let served = journalled_run(&journal_dir("init"), "acp-init");
            let response: Value = serde_json::from_str(
                &handle(
                    served,
                    &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}).to_string(),
                )
                .expect("served"),
            )
            .expect("json");
            let result = &response["result"];
            assert_eq!(result["protocolVersion"], PROTOCOL_VERSION);
            assert_eq!(result["agentCapabilities"]["loadSession"], false);
            assert_eq!(
                result["agentCapabilities"]["promptCapabilities"]["image"],
                false
            );
            assert_eq!(
                result["agentCapabilities"]["promptCapabilities"]["embeddedContext"],
                false
            );
            assert_eq!(result["agentCapabilities"]["mcpCapabilities"]["http"], false);
            assert_eq!(
                result["_meta"]["spectra"]["implementedMethods"],
                json!(IMPLEMENTED_METHODS)
            );
            // An unsupported method is refused, not silently accepted.
            let unsupported: Value = serde_json::from_str(
                &handle(
                    served,
                    &json!({"jsonrpc": "2.0", "id": 2, "method": "session/load"}).to_string(),
                )
                .expect("served"),
            )
            .expect("json");
            assert_eq!(unsupported["error"]["code"], METHOD_NOT_FOUND);
            // `session/cancel` is a notification: no response document.
            assert_eq!(
                handle(
                    served,
                    &json!({"jsonrpc": "2.0", "method": "session/cancel"}).to_string()
                )
                .expect("served"),
                ""
            );
            run::take_run(served).expect("end");
        });
    }

    #[test]
    fn a_session_prompt_runs_through_the_governed_loop() {
        with_seams(|| {
            let served = journalled_run(&journal_dir("prompt"), "acp-prompt");
            let session: Value = serde_json::from_str(
                &handle(
                    served,
                    &json!({"jsonrpc": "2.0", "id": 1, "method": "session/new",
                            "params": {"cwd": ".", "mcpServers": []}})
                    .to_string(),
                )
                .expect("served"),
            )
            .expect("json");
            let session_id = session["result"]["sessionId"]
                .as_str()
                .expect("session id")
                .to_string();
            assert_eq!(session_id, "acp-prompt");

            let response: Value = serde_json::from_str(
                &handle(
                    served,
                    &json!({"jsonrpc": "2.0", "id": 2, "method": "session/prompt",
                            "params": {"sessionId": session_id,
                                       "prompt": [{"type": "text", "text": "spectra:final=done"}]}})
                    .to_string(),
                )
                .expect("served"),
            )
            .expect("json");
            assert_eq!(response["result"]["stopReason"], "end_turn");
            assert_eq!(response["result"]["_meta"]["spectra"]["agentText"], "done");

            // A prompt for another session is refused: one session per run.
            let foreign: Value = serde_json::from_str(
                &handle(
                    served,
                    &json!({"jsonrpc": "2.0", "id": 3, "method": "session/prompt",
                            "params": {"sessionId": "someone-elses",
                                       "prompt": [{"type": "text", "text": "hi"}]}})
                    .to_string(),
                )
                .expect("served"),
            )
            .expect("json");
            assert_eq!(foreign["error"]["code"], INVALID_PARAMS);
            let error = handle(served, "not json").expect_err("typed failure");
            assert_eq!(error.kind(), "acp_error");
            run::take_run(served).expect("end");
        });
    }

    #[test]
    fn a_capability_denial_traps_through_the_acp_prompt() {
        with_seams(|| {
            // An effectful tool the run does not grant: `session/prompt` reaches
            // `act`, whose grant check refuses before the first dispatch, so the
            // ACP path cannot bypass the capability seam.
            assert!(crate::tools::register(
                "writer".to_string(),
                0x1000,
                "Writes".to_string(),
                r#"{"type":"object"}"#.to_string(),
                r#"["spectra.std.fs.fs_write"]"#,
            ));
            let served = journalled_run(&journal_dir("denied"), "acp-denied");
            let response: Value = serde_json::from_str(
                &handle(
                    served,
                    &json!({"jsonrpc": "2.0", "id": 1, "method": "session/prompt",
                            "params": {"sessionId": "acp-denied",
                                       "prompt": [{"type": "text", "text": "spectra:final=done"}]}})
                    .to_string(),
                )
                .expect("served"),
            )
            .expect("json");
            assert_eq!(response["result"]["stopReason"], "refusal", "{response}");
            assert!(
                response["result"]["_meta"]["spectra"]["reason"]
                    .as_str()
                    .unwrap_or("")
                    .contains("capability_denied"),
                "{response}"
            );
            run::take_run(served).expect("end");
        });
    }
}
