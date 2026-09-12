//! Provider abstraction for `std.agent` (R-3211 T2).
//!
//! A provider is selected from the run's `AgentSpec` alone (a provider is
//! data). Three implementations exist: the deterministic mock, the
//! OpenAI-compatible HTTP client, and the `local:` bridge placeholder.

pub mod local;
pub mod mock;
pub mod openai_compatible;
pub mod transport;

use crate::error::AgentError;
use crate::spec::AgentSpec;

/// One conversation message. Roles are closed because the host never invents
/// a role the provider did not define.
///
/// `tool_name` is carried for provenance only (R-3223 T1): the transcript
/// origin of a tool result is `tool:<name>`, and the ledger cannot recover the
/// name once the message has been built. It is never serialized — the wire
/// shape stays `{role, content}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Message {
    pub role: &'static str,
    pub content: String,
    pub tool_name: Option<String>,
}

impl Message {
    pub(crate) fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user",
            content: content.into(),
            tool_name: None,
        }
    }

    pub(crate) fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant",
            content: content.into(),
            tool_name: None,
        }
    }

    /// A completed tool result. The `name` is kept so the run's taint ledger
    /// can tag the message `tool:<name>`; providers still see `{role, content}`
    /// and the mock still keys off the `"tool"` role alone.
    pub(crate) fn tool(name: &str, content: impl Into<String>) -> Self {
        Self {
            role: "tool",
            content: content.into(),
            tool_name: Some(name.to_string()),
        }
    }
}

/// One model-callable tool exposed to the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema object for the tool's single payload argument.
    pub input_schema: String,
}

/// One tool invocation requested by the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolCall {
    pub name: String,
    /// Raw JSON arguments string.
    pub arguments: String,
}

/// Effective sampling parameters. The plan's `AgentSpec` carries only `seed`;
/// deterministic mode pins `temperature` to 0 (the only sampling setting the
/// provider-independent mock and OpenAI-compatible APIs share) and leaves
/// `top_k` unset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SamplingParams {
    pub temperature: Option<f32>,
    pub top_k: Option<u32>,
    pub seed: Option<i64>,
}

impl SamplingParams {
    pub(crate) fn effective(spec: &AgentSpec) -> Self {
        if spec.deterministic() {
            Self {
                temperature: Some(0.0),
                top_k: None,
                seed: Some(spec.seed),
            }
        } else {
            Self {
                temperature: None,
                top_k: None,
                seed: None,
            }
        }
    }
}

/// A provider request. `json_schema` is advisory: the client validates the
/// response regardless of provider-side constrained decoding. `tools` lists
/// the model-callable tools; an empty vec means a text-only turn.
#[derive(Debug, Clone)]
pub(crate) struct ProviderRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub top_k: Option<u32>,
    pub seed: Option<i64>,
    pub json_schema: Option<String>,
    pub max_tokens: Option<u64>,
    pub tools: Vec<ToolDefinition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FinishReason {
    Stop,
    Length,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderResponse {
    pub text: String,
    /// Tool invocations requested by the model; empty for a text-only turn.
    pub tool_calls: Vec<ToolCall>,
    pub usage: Usage,
    /// Provider-reported cost in micros; 0 when the price is unknown (the
    /// checked-in price table and its fail-closed policy are R-3216).
    pub cost_micros: u64,
    /// Number of transport retries this response required (0 when none).
    pub retries: u64,
    pub finish: FinishReason,
}

/// A streamed turn: the chunks plus the usage/cost of the single underlying
/// model call that produced them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderStream {
    pub chunks: Vec<String>,
    pub usage: Usage,
    pub cost_micros: u64,
    pub retries: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProviderError {
    /// No provider can serve the requested endpoint/model.
    NotConfigured(String),
    /// Transport-level failure (no transport installed, connection, timeout).
    Transport(String),
    /// The provider answered with a non-success HTTP status.
    Http { status: i64, message: String },
    /// The provider answered successfully but the payload was unusable.
    InvalidResponse(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured(message)
            | Self::Transport(message)
            | Self::InvalidResponse(message) => f.write_str(message),
            Self::Http { status, message } => write!(f, "HTTP {status}: {message}"),
        }
    }
}

impl From<ProviderError> for AgentError {
    fn from(error: ProviderError) -> Self {
        match error {
            ProviderError::NotConfigured(message) => AgentError::ProviderNotConfigured(message),
            ProviderError::Transport(message)
            | ProviderError::Http {
                message,
                status: _,
            }
            | ProviderError::InvalidResponse(message) => AgentError::Provider(message),
        }
    }
}

/// A model provider. Implementations are thread-safe because a turn runs on
/// the runtime's background executor.
pub(crate) trait Provider: Send + Sync {
    /// Stable provider name used in diagnostics and denial messages.
    fn name(&self) -> &'static str;

    /// Whether this provider can serve requests at all. Checked before the
    /// determinism check so an unconfigured provider reports the more
    /// actionable error.
    fn is_configured(&self) -> bool {
        true
    }

    /// Whether the provider can guarantee the requested seed was applied.
    /// A deterministic run fails closed when this is `false`.
    fn honors_seed(&self) -> bool;

    /// Whether every response from this provider carries a trustworthy
    /// per-response cost. A spec with `max_cost_micros > 0` is refused at
    /// `agent_start` when this is `false`, because the ceiling could never be
    /// enforced (R-3216 T1). Conservative default: no.
    fn reports_cost(&self) -> bool {
        false
    }

    /// One blocking model turn.
    fn complete(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError>;

    /// One model turn delivered as chunks. The default implementation is a
    /// single chunk, which is still a correct (if not progressive) stream.
    fn stream(&self, request: &ProviderRequest) -> Result<ProviderStream, ProviderError> {
        let response = self.complete(request)?;
        Ok(ProviderStream {
            chunks: vec![response.text],
            usage: response.usage,
            cost_micros: response.cost_micros,
            retries: response.retries,
        })
    }

    /// Deterministic embedding vector for `text`.
    fn embed(&self, text: &str) -> Result<Vec<f64>, ProviderError>;
}

/// Selects the provider named by the spec.
///
/// `mock:`/`mock/` select the deterministic mock; `local:`/`local/` select the
/// not-yet-configured local bridge; everything else is treated as an
/// OpenAI-compatible HTTP endpoint (empty endpoint falls back to
/// `SPECTRA_AGENT_ENDPOINT` or `OPENAI_BASE_URL`).
pub(crate) fn provider_for(spec: &AgentSpec) -> Box<dyn Provider> {
    if spec.endpoint.starts_with("mock:") || spec.model.starts_with("mock/") {
        return Box::new(mock::MockProvider::new(spec.model.clone()));
    }
    if spec.endpoint.starts_with("local:") || spec.model.starts_with("local/") {
        return Box::new(local::LocalProvider::default());
    }
    let endpoint = if spec.endpoint.is_empty() {
        std::env::var("SPECTRA_AGENT_ENDPOINT")
            .or_else(|_| std::env::var("OPENAI_BASE_URL"))
            .unwrap_or_default()
    } else {
        spec.endpoint.clone()
    };
    Box::new(openai_compatible::OpenAiCompatibleProvider::new(
        endpoint,
        spec.model.clone(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(json: &str) -> AgentSpec {
        AgentSpec::parse(json).expect("valid spec")
    }

    #[test]
    fn deterministic_specs_pin_sampling_parameters() {
        let deterministic = SamplingParams::effective(&spec(
            r#"{"goal":"g","model":"mock/echo","seed":11}"#,
        ));
        assert_eq!(deterministic.seed, Some(11));
        assert_eq!(deterministic.temperature, Some(0.0));
        assert_eq!(deterministic.top_k, None);

        let default = SamplingParams::effective(&spec(r#"{"goal":"g","model":"mock/echo"}"#));
        assert_eq!(default, SamplingParams { temperature: None, top_k: None, seed: None });
    }

    #[test]
    fn spec_routing_picks_the_mock_and_local_providers() {
        assert_eq!(
            provider_for(&spec(r#"{"goal":"g","model":"m","endpoint":"mock:"}"#)).name(),
            "mock"
        );
        assert_eq!(
            provider_for(&spec(r#"{"goal":"g","model":"mock/echo"}"#)).name(),
            "mock"
        );
        assert_eq!(
            provider_for(&spec(r#"{"goal":"g","model":"local/tiny","endpoint":"local:"}"#)).name(),
            "local"
        );
        assert_eq!(
            provider_for(&spec(r#"{"goal":"g","model":"gpt-x","endpoint":"https://example.invalid"}"#))
                .name(),
            "openai_compatible"
        );
    }
}
