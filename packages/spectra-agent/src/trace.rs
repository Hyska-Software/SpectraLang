//! OpenTelemetry GenAI span mapping (R-3217 T5).
//!
//! The runtime already emits OpenTelemetry-shaped spans through
//! `std.api.trace`; this module is the seam that maps a governed run's
//! activities onto the GenAI semantic conventions and hands them to a
//! [`TraceSink`]. No sink is a no-op, and this crate deliberately does not
//! depend on `spectra-api`: the adapter that forwards a span into the runtime's
//! tracer is the embedder's (reported as pending; the trait is the seam).
//!
//! Conventions version: **1.34.0** — the minimum version containing both
//! `gen_ai.conversation.id` (added in 1.34.0) and the `invoke_agent` operation
//! (added in 1.33.0). Every span carries the version and its schema URL.
//!
//! Content capture — prompts, completions, tool payloads — is opt-in through
//! [`TraceSink::captures_content`] and off by default: a sink that does not opt
//! in never receives content, and the span records only names, attributes and
//! the run/step identity.

use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

/// Pinned OpenTelemetry GenAI semantic-conventions version.
pub const GEN_AI_CONVENTIONS_VERSION: &str = "1.34.0";

/// Schema URL of the pinned conventions version.
pub const GEN_AI_CONVENTIONS_SCHEMA_URL: &str = "https://opentelemetry.io/schemas/1.34.0";

/// `gen_ai.operation.name` value for a run's agent invocation.
pub const OP_INVOKE_AGENT: &str = "invoke_agent";
/// `gen_ai.operation.name` value for a planning turn.
pub const OP_PLAN: &str = "plan";
/// `gen_ai.operation.name` value for a tool execution.
pub const OP_EXECUTE_TOOL: &str = "execute_tool";
/// `gen_ai.operation.name` value for a model call.
pub const OP_CHAT: &str = "chat";

/// One recorded span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Span name: `invoke_agent`, `plan`, `execute_tool`, `chat {model}`.
    pub name: String,
    /// `gen_ai.operation.name`.
    pub operation: &'static str,
    /// The run this span belongs to (`gen_ai.conversation.id`).
    pub run: String,
    /// The run's effect step, when the span belongs to one.
    pub step: Option<u64>,
    /// `gen_ai.agent.name` — the run's agent identity.
    pub agent_name: String,
    /// Additional `gen_ai.*` attributes.
    pub attributes: Vec<(String, String)>,
    /// Opt-in request content (prompt, tool arguments, ...).
    pub content: Option<String>,
    /// Opt-in result content (completion, tool result, ...).
    pub result: Option<String>,
    /// Pinned conventions version, recorded on every span.
    pub conventions_version: &'static str,
    /// Schema URL of the pinned conventions version.
    pub schema_url: &'static str,
}

impl Span {
    /// One span attribute by key.
    pub fn attribute(&self, key: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    pub(crate) fn attribute_mut(&mut self, key: &str, value: impl Into<String>) {
        self.attributes.push((key.to_string(), value.into()));
    }
}

/// A span consumer. The default captures no content.
pub trait TraceSink: Send + Sync + 'static {
    /// Whether this sink wants prompt/completion/tool content. Off by default:
    /// content can carry secrets, so a sink must ask for it explicitly.
    fn captures_content(&self) -> bool {
        false
    }

    fn record(&self, span: &Span);
}

fn slot() -> &'static Mutex<Option<Arc<dyn TraceSink>>> {
    static SLOT: LazyLock<Mutex<Option<Arc<dyn TraceSink>>>> = LazyLock::new(|| Mutex::new(None));
    &SLOT
}

fn lock() -> MutexGuard<'static, Option<Arc<dyn TraceSink>>> {
    slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Attaches (or removes) the process-global trace sink. No sink means spans
/// are dropped without allocation.
pub fn set_trace_sink(sink: Option<Arc<dyn TraceSink>>) -> bool {
    let attached = sink.is_some();
    *lock() = sink;
    attached
}

/// The installed sink, if any.
pub(crate) fn sink() -> Option<Arc<dyn TraceSink>> {
    lock().clone()
}

/// Builds a span and hands it to the sink. Returns silently when no sink is
/// attached, before any string is built for content.
fn emit(
    operation: &'static str,
    name: String,
    run: &str,
    step: Option<u64>,
    agent_name: &str,
    extra: &[(&str, &str)],
    content: Option<&str>,
    result: Option<&str>,
) {
    let Some(sink) = sink() else {
        return;
    };
    let mut span = Span {
        name,
        operation,
        run: run.to_string(),
        step,
        agent_name: agent_name.to_string(),
        attributes: vec![
            ("gen_ai.operation.name".to_string(), operation.to_string()),
            ("gen_ai.agent.name".to_string(), agent_name.to_string()),
            ("gen_ai.conversation.id".to_string(), run.to_string()),
        ],
        content: None,
        result: None,
        conventions_version: GEN_AI_CONVENTIONS_VERSION,
        schema_url: GEN_AI_CONVENTIONS_SCHEMA_URL,
    };
    for (key, value) in extra {
        span.attribute_mut(key, *value);
    }
    if sink.captures_content() {
        span.content = content.map(str::to_string);
        span.result = result.map(str::to_string);
    }
    sink.record(&span);
}

/// `invoke_agent`: one run's agent invocation.
pub(crate) fn emit_invoke_agent(run: &str, agent_name: &str) {
    emit(
        OP_INVOKE_AGENT,
        OP_INVOKE_AGENT.to_string(),
        run,
        None,
        agent_name,
        &[],
        None,
        None,
    );
}

/// `plan`: the planning turn of a tool loop.
pub(crate) fn emit_plan(run: &str, agent_name: &str, content: Option<&str>) {
    emit(
        OP_PLAN,
        OP_PLAN.to_string(),
        run,
        None,
        agent_name,
        &[],
        content,
        None,
    );
}

/// `chat {model}`: one model call.
pub(crate) fn emit_chat(
    run: &str,
    agent_name: &str,
    step: u64,
    model: &str,
    content: Option<&str>,
    result: Option<&str>,
) {
    emit(
        OP_CHAT,
        format!("chat {model}"),
        run,
        Some(step),
        agent_name,
        &[("gen_ai.request.model", model)],
        content,
        result,
    );
}

/// `execute_tool`: one governed tool execution.
pub(crate) fn emit_execute_tool(
    run: &str,
    agent_name: &str,
    step: u64,
    tool: &str,
    arguments: Option<&str>,
    result: Option<&str>,
) {
    emit(
        OP_EXECUTE_TOOL,
        OP_EXECUTE_TOOL.to_string(),
        run,
        Some(step),
        agent_name,
        &[("gen_ai.tool.name", tool)],
        arguments,
        result,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder {
        spans: Mutex<Vec<Span>>,
        capture: bool,
    }

    impl TraceSink for Recorder {
        fn captures_content(&self) -> bool {
            self.capture
        }

        fn record(&self, span: &Span) {
            self.spans.lock().expect("spans").push(span.clone());
        }
    }

    #[test]
    fn spans_map_to_the_pinned_conventions_and_hide_content_by_default() {
        // The sink is a process-global slot: hold the crate-wide test lock so
        // no other test installs or clears one while this test runs.
        let _guard = crate::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let recorder = Arc::new(Recorder::default());
        assert!(set_trace_sink(Some(recorder.clone())));

        emit_invoke_agent("run-1", "goal");
        emit_plan("run-1", "goal", Some("plan the work"));
        emit_chat("run-1", "goal", 0, "mock/echo", Some("prompt"), Some("answer"));
        emit_execute_tool("run-1", "goal", 1, "add", Some("{\"a\":1}"), Some("2"));

        let spans = recorder.spans.lock().expect("spans");
        assert_eq!(spans.len(), 4);
        let names: Vec<&str> = spans.iter().map(|span| span.name.as_str()).collect();
        assert_eq!(names, ["invoke_agent", "plan", "chat mock/echo", "execute_tool"]);
        let operations: Vec<&str> = spans.iter().map(|span| span.operation).collect();
        assert_eq!(
            operations,
            [OP_INVOKE_AGENT, OP_PLAN, OP_CHAT, OP_EXECUTE_TOOL]
        );
        for span in spans.iter() {
            assert_eq!(span.conventions_version, "1.34.0");
            assert_eq!(span.schema_url, GEN_AI_CONVENTIONS_SCHEMA_URL);
            assert_eq!(span.attribute("gen_ai.agent.name"), Some("goal"));
            assert_eq!(span.attribute("gen_ai.conversation.id"), Some("run-1"));
            assert_eq!(span.attribute("gen_ai.operation.name"), Some(span.operation));
            // Content is absent: the sink did not opt in.
            assert!(span.content.is_none(), "{span:?}");
            assert!(span.result.is_none(), "{span:?}");
        }
        assert_eq!(spans[2].attribute("gen_ai.request.model"), Some("mock/echo"));
        assert_eq!(spans[3].attribute("gen_ai.tool.name"), Some("add"));
        assert_eq!(spans[2].step, Some(0));
        assert_eq!(spans[0].step, None);
        drop(spans);
        set_trace_sink(None);
    }

    #[test]
    fn a_content_sink_receives_content_and_no_sink_is_a_no_op() {
        // The sink is a process-global slot: hold the crate-wide test lock so
        // no other test installs or clears one while this test runs.
        let _guard = crate::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        assert!(!set_trace_sink(None));
        // No sink: emitting must not panic or allocate a span.
        emit_invoke_agent("run-2", "goal");

        let recorder = Arc::new(Recorder {
            spans: Mutex::new(Vec::new()),
            capture: true,
        });
        set_trace_sink(Some(recorder.clone()));
        emit_chat("run-2", "goal", 3, "mock/echo", Some("prompt"), Some("answer"));
        let spans = recorder.spans.lock().expect("spans");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_deref(), Some("prompt"));
        assert_eq!(spans[0].result.as_deref(), Some("answer"));
        drop(spans);
        set_trace_sink(None);
    }
}
