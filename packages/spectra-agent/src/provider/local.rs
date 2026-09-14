//! Local generation bridge (R-3211 T2).
//!
//! A local model answers *in process*: the provider tokenizes the prompt with
//! a WordPiece artifact, runs the runtime's own ONNX generation engine, and
//! decodes the continuation — the same engine `spectra.std.ml.generate_ex`
//! drives, reached through [`spectra_runtime::local_model`] instead of a host
//! call, because a provider has no program frame to host-call from.
//!
//! # Configuration
//!
//! A provider is data (the run's `AgentSpec`), so the paths come from the spec
//! first and the environment second:
//!
//! | Setting | Spec | Environment |
//! |---|---|---|
//! | model | `endpoint: "local:<path>"` | `SPECTRA_AGENT_LOCAL_MODEL` |
//! | tokenizer | — | `SPECTRA_AGENT_LOCAL_TOKENIZER`, else `<model>.spar`, else `<dir>/tokenizer.spar` |
//! | embedding model | — | `SPECTRA_AGENT_LOCAL_EMBEDDING` (+ its tokenizer, resolved the same way) |
//!
//! `endpoint: "local:"` with no path is the documented "local, configured from
//! the environment" spelling. A missing path is a typed `not_configured` error
//! that names every candidate it tried — never a silent fallback to another
//! provider, and never a fabricated answer.
//!
//! # What a local model does and does not do
//!
//! * Sampling is ours: `temperature`, `top_k` and `seed` reach the runtime's
//!   sampler, so [`Provider::honors_seed`] is `true` and a deterministic spec
//!   replays identically.
//! * Cost is not ours: local inference has no price, so [`Provider::reports_cost`]
//!   stays `false` and `agent_start` refuses a `max_cost_micros` ceiling on
//!   this provider rather than pretending a cost of zero is a measurement.
//! * Token usage *is* ours (the tokenizer counts exactly what was fed and
//!   produced), so token ceilings are enforceable in a way they are not for an
//!   endpoint that omits `usage`.
//! * Tool calls are not: a raw causal language model has no tool-call protocol,
//!   so a local turn answers with text and the `act` loop sees no tool
//!   requests. Teaching a model a tool protocol is the model's business (a
//!   chat template), not something this bridge may invent.

use std::path::Path;

use spectra_runtime::local_model::{CausalLm, EmbeddingModel, LocalModelError, Sampling};

use super::{FinishReason, Provider, ProviderError, ProviderRequest, ProviderResponse, ProviderStream, Usage};

/// Continuations are bounded when the request declares no `max_tokens`, so a
/// local turn cannot run away with a CPU.
const DEFAULT_MAX_NEW_TOKENS: usize = 256;

/// The environment variable naming the local causal-LM file.
pub(crate) const ENV_MODEL: &str = "SPECTRA_AGENT_LOCAL_MODEL";
/// The environment variable naming the local tokenizer artifact.
pub(crate) const ENV_TOKENIZER: &str = "SPECTRA_AGENT_LOCAL_TOKENIZER";
/// The environment variable naming the local embedding model file.
pub(crate) const ENV_EMBEDDING: &str = "SPECTRA_AGENT_LOCAL_EMBEDDING";

/// Which files a local provider answers from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalConfig {
    pub model: String,
    pub tokenizer: String,
    pub embedding: String,
    pub embedding_tokenizer: String,
}

impl LocalConfig {
    /// Resolves the configuration: spec first, environment second.
    pub(crate) fn resolve(endpoint: &str) -> Self {
        let from_endpoint = endpoint
            .strip_prefix("local:")
            .unwrap_or(endpoint)
            .trim()
            .to_string();
        let model = if from_endpoint.is_empty() {
            env_value(ENV_MODEL)
        } else {
            from_endpoint
        };
        let tokenizer = match env_value(ENV_TOKENIZER) {
            explicit if !explicit.is_empty() => explicit,
            _ => resolve_tokenizer(&model),
        };
        let embedding = env_value(ENV_EMBEDDING);
        let embedding_tokenizer = if embedding.is_empty() {
            String::new()
        } else {
            match env_value(ENV_TOKENIZER) {
                explicit if !explicit.is_empty() => explicit,
                _ => resolve_tokenizer(&embedding),
            }
        };
        Self {
            model,
            tokenizer,
            embedding,
            embedding_tokenizer,
        }
    }

    /// Whether a model file is named at all: the cheapest check a run can make
    /// before its first turn.
    pub(crate) fn names_a_model(&self) -> bool {
        !self.model.is_empty()
    }
}

fn env_value(key: &str) -> String {
    std::env::var(key).unwrap_or_default().trim().to_string()
}

/// The tokenizer artifact for `model`: the model path with a `.spar`
/// extension, else a `tokenizer.spar` beside it.
///
/// When neither exists the first candidate is returned anyway, so the error a
/// user reads names a concrete path instead of an empty string.
fn resolve_tokenizer(model: &str) -> String {
    if model.is_empty() {
        return String::new();
    }
    let path = Path::new(model);
    let sibling = path.with_extension("spar");
    if sibling.is_file() {
        return sibling.to_string_lossy().to_string();
    }
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let conventional = directory.join("tokenizer.spar");
    if conventional.is_file() {
        return conventional.to_string_lossy().to_string();
    }
    sibling.to_string_lossy().to_string()
}

/// The local provider: an ONNX causal LM plus its tokenizer.
pub(crate) struct LocalProvider {
    config: LocalConfig,
}

impl LocalProvider {
    pub(crate) fn new(config: LocalConfig) -> Self {
        Self { config }
    }

    /// Names what is missing, so the refusal is actionable.
    fn model_error(&self) -> ProviderError {
        ProviderError::NotConfigured(format!(
            "the local provider has no model: set `endpoint: \"local:<model.onnx>\"` or {ENV_MODEL}; \
             the tokenizer is taken from {ENV_TOKENIZER}, then '<model>.spar', then \
             '<model-dir>/tokenizer.spar' (looked for '{}')",
            self.config.tokenizer
        ))
    }

    fn load_causal(&self) -> Result<std::sync::Arc<CausalLm>, ProviderError> {
        if !self.config.names_a_model() {
            return Err(self.model_error());
        }
        CausalLm::load(&self.config.model, &self.config.tokenizer).map_err(local_error)
    }

    fn load_embedding(&self) -> Result<std::sync::Arc<EmbeddingModel>, ProviderError> {
        if self.config.embedding.is_empty() {
            return Err(ProviderError::NotConfigured(format!(
                "the local provider has no embedding model: set {ENV_EMBEDDING} to an ONNX graph \
                 that embeds token ids (the runtime's `text_embed_model` contract)"
            )));
        }
        EmbeddingModel::load(&self.config.embedding, &self.config.embedding_tokenizer)
            .map_err(local_error)
    }

    /// The sampling a request asks for, in the runtime's terms.
    fn sampling(request: &ProviderRequest) -> Sampling {
        Sampling {
            // The request carries f32 sampling parameters (the provider
            // contract); the runtime's sampler is f64.
            temperature: f64::from(request.temperature.unwrap_or(0.0)).max(0.0),
            top_k: request.top_k.unwrap_or(0).max(0) as usize,
            seed: request.seed.unwrap_or(-1).max(0) as u64,
        }
    }

    /// A causal LM has no chat template of its own, so the prompt is the
    /// transcript rendered in a documented, minimal form: a single user
    /// message is its own text, and anything longer is one `role: content`
    /// line per message.
    fn prompt_of(request: &ProviderRequest) -> String {
        if request.messages.len() == 1 {
            return request.messages[0].content.clone();
        }
        let mut prompt = String::new();
        for message in &request.messages {
            prompt.push_str(message.role);
            prompt.push_str(": ");
            prompt.push_str(&message.content);
            prompt.push('\n');
        }
        prompt
    }
}

/// Maps the runtime's error taxonomy onto the provider's.
///
/// Everything that means "local inference is not usable as configured" is a
/// `not_configured` refusal (the run refuses the turn and the message names
/// the file or the setting); a genuine inference failure is a provider error.
fn local_error(error: LocalModelError) -> ProviderError {
    match error {
        LocalModelError::Unavailable(message)
        | LocalModelError::Missing(message)
        | LocalModelError::Io(message)
        | LocalModelError::InvalidArtifact(message) => ProviderError::NotConfigured(message),
        LocalModelError::Inference(message) => ProviderError::InvalidResponse(message),
    }
}

impl Provider for LocalProvider {
    fn name(&self) -> &'static str {
        "local"
    }

    fn is_configured(&self) -> bool {
        // The cheapest check that can refuse a run before its first turn: a
        // model is named and this build can run one at all.
        self.config.names_a_model() && spectra_runtime::local_model::inference_available()
    }

    fn honors_seed(&self) -> bool {
        // The sampler is the runtime's, driven by the request's seed.
        true
    }

    fn complete(&self, request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let model = self.load_causal()?;
        let max_new_tokens = request
            .max_tokens
            .map(|value| value.max(1) as usize)
            .unwrap_or(DEFAULT_MAX_NEW_TOKENS);
        let generated = model
            .generate(&Self::prompt_of(request), max_new_tokens, Self::sampling(request))
            .map_err(local_error)?;
        Ok(ProviderResponse {
            text: generated.text,
            // A raw causal LM has no tool-call protocol; see the module docs.
            tool_calls: Vec::new(),
            usage: Usage {
                input_tokens: generated.prompt_tokens as u64,
                output_tokens: generated.output_tokens as u64,
            },
            // Local inference has no price; `reports_cost()` says so, and a
            // cost ceiling is refused at `agent_start` rather than measured as
            // zero.
            cost_micros: 0,
            retries: 0,
            finish: FinishReason::Stop,
        })
    }

    fn stream(&self, request: &ProviderRequest) -> Result<ProviderStream, ProviderError> {
        let model = self.load_causal()?;
        let max_new_tokens = request
            .max_tokens
            .map(|value| value.max(1) as usize)
            .unwrap_or(DEFAULT_MAX_NEW_TOKENS);
        let mut chunks: Vec<String> = Vec::new();
        let generated = model
            .generate_streaming(
                &Self::prompt_of(request),
                max_new_tokens,
                Self::sampling(request),
                &mut |delta| {
                    chunks.push(delta.to_string());
                    true
                },
            )
            .map_err(local_error)?;
        Ok(ProviderStream {
            chunks,
            usage: Usage {
                input_tokens: generated.prompt_tokens as u64,
                output_tokens: generated.output_tokens as u64,
            },
            cost_micros: 0,
            retries: 0,
        })
    }

    fn embed(&self, text: &str) -> Result<Vec<f64>, ProviderError> {
        let model = self.load_embedding()?;
        model.embed(text).map_err(local_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(prompt: &str) -> ProviderRequest {
        ProviderRequest {
            model: "local/toy".to_string(),
            messages: vec![super::super::Message::user(prompt)],
            temperature: Some(0.0),
            top_k: None,
            seed: Some(7),
            json_schema: None,
            max_tokens: Some(4),
            tools: Vec::new(),
        }
    }

    /// The paths come from the spec first and the environment second, and a
    /// missing model is named rather than guessed.
    #[test]
    fn configuration_resolution_is_explicit() {
        // Spec path wins; the tokenizer falls back to the conventional name.
        let from_spec = LocalConfig::resolve("local:models/toy.onnx");
        assert_eq!(from_spec.model, "models/toy.onnx");
        assert_eq!(from_spec.tokenizer, "models/toy.spar");
        assert_eq!(from_spec.embedding, "");

        // An empty path means "from the environment" in both directions.
        let bare = LocalConfig::resolve("local:");
        assert!(!bare.names_a_model());
        let blank = LocalConfig::resolve("");
        assert!(!blank.names_a_model());
    }

    /// An unconfigured provider refuses with the setting's name, and nothing
    /// else: no fallback provider, no empty answer.
    #[test]
    fn an_unconfigured_local_provider_refuses_by_name() {
        let provider = LocalProvider::new(LocalConfig::resolve("local:"));
        assert!(!provider.is_configured());
        let refusal = provider
            .complete(&request("hello"))
            .expect_err("refuses without a model");
        let message = refusal.to_string();
        assert!(message.contains(ENV_MODEL), "{message}");
        assert!(message.contains(ENV_TOKENIZER), "{message}");

        // Embeddings are refused separately, naming their own setting.
        let embeddings = provider.embed("hello").expect_err("refuses embeddings");
        let message = embeddings.to_string();
        assert!(message.contains(ENV_EMBEDDING), "{message}");
    }

    /// A single user message is the prompt; a conversation is rendered with
    /// role prefixes (a causal LM has no chat template of its own).
    #[test]
    fn the_transcript_is_rendered_predictably() {
        let single = request("alpha beta");
        assert_eq!(LocalProvider::prompt_of(&single), "alpha beta");

        let mut conversation = request("alpha");
        conversation.messages.push(super::super::Message::assistant("beta"));
        conversation.messages.push(super::super::Message::tool("add", "42"));
        assert_eq!(
            LocalProvider::prompt_of(&conversation),
            "user: alpha\nassistant: beta\ntool: 42\n"
        );
    }

    /// Sampling is carried through, and a missing seed stays the greedy
    /// default rather than becoming a fixed one.
    #[test]
    fn sampling_follows_the_request() {
        let greedy = LocalProvider::sampling(&request("x"));
        assert_eq!(greedy.seed, 7);
        assert_eq!(greedy.temperature, 0.0);
        assert_eq!(greedy.top_k, 0);

        let mut sampled = request("x");
        sampled.temperature = Some(0.8);
        sampled.top_k = Some(40);
        sampled.seed = None;
        let resolved = LocalProvider::sampling(&sampled);
        // The request's f32 0.8 widened to f64 is 0.800000011920929.
        assert!((resolved.temperature - 0.8).abs() < 1e-6);
        assert_eq!(resolved.top_k, 40);
        assert_eq!(resolved.seed, 0, "no seed is the deterministic stream's zero");
    }
}
