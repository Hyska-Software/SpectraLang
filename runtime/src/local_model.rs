//! Local model bridge: the runtime's own inference path, callable from an
//! embedding host instead of only from Spectra source.
//!
//! The runtime already owns everything local inference needs — the ONNX
//! session table, the WordPiece tokenizer artifacts, causal-LM generation and
//! the sentence-embedding graph — but until now those were reachable only
//! through `spectra.std.ml.*` host calls, i.e. only from a `.spectra` program.
//! A host that wants a local model to answer *for* a program (the `std.agent`
//! `local:` provider) has no program frame to host-call from, and duplicating
//! model loading in the host would fork the inference path.
//!
//! This module is that seam. It is deliberately small and models the three
//! things a host needs:
//!
//! * [`Tokenizer`] — a WordPiece tokenizer loaded from an artifact
//!   (`spectra.std.ml.tokenizer_load`'s format), with encode/decode and the
//!   artifact's special-token ids;
//! * [`CausalLm`] — a causal language model loaded from an ONNX ModelProto
//!   file, generating text through the *same* engine as
//!   `spectra.std.ml.generate_ex` ([`crate::stdlib`]'s `ml_generate_inner`,
//!   including its KV-cache detection and sampling);
//! * [`EmbeddingModel`] — a sentence-embedding ONNX graph, embedded through
//!   the same masked-mean-pool + L2 path as
//!   `spectra.std.ml.text_embed_model`.
//!
//! Loaded models are cached per `(model, tokenizer)` path pair, because hosts
//! resolve a provider per model turn: without the cache every turn would
//! re-commit an onnxruntime session. The cache holds `Arc<CausalLm>`, so a
//! provider keeps its model alive across turns and the session is released
//! when the last holder drops.
//!
//! # Feature gate
//!
//! ONNX inference is the runtime's opt-in `onnx` feature (it links
//! onnxruntime). Without it, every constructor returns
//! [`LocalModelError::Unavailable`] naming the feature — the same honest
//! degradation `spectra.std.ml.generate_ex` performs, not a fake model.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use crate::stdlib::{ml_parse_artifact_tokenizer, ml_wordpiece_decode, ml_wordpiece_encode};

/// Why a local model could not be loaded or used.
///
/// Each variant names what the caller must fix; the host maps them onto its
/// own typed errors (`std.agent` uses `provider_not_configured` for the first
/// two and `provider_error` for the rest).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalModelError {
    /// The runtime was built without the `onnx` feature.
    Unavailable(String),
    /// A required path was not configured (the message names the setting).
    Missing(String),
    /// A configured path could not be read.
    Io(String),
    /// The file exists but is not the artifact/session this API needs.
    InvalidArtifact(String),
    /// Inference itself failed (shape mismatch, non-finite logits, ...).
    Inference(String),
}

impl std::fmt::Display for LocalModelError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(detail) => write!(formatter, "onnx_unavailable: {detail}"),
            Self::Missing(detail) => write!(formatter, "not_configured: {detail}"),
            Self::Io(detail) => write!(formatter, "io_error: {detail}"),
            Self::InvalidArtifact(detail) => write!(formatter, "invalid_artifact: {detail}"),
            Self::Inference(detail) => write!(formatter, "inference_error: {detail}"),
        }
    }
}

impl std::error::Error for LocalModelError {}

/// Whether this build can run local models.
pub fn inference_available() -> bool {
    cfg!(feature = "onnx")
}

/// Why local inference is unavailable, in the terms a user can act on.
pub fn unavailable_reason() -> String {
    if inference_available() {
        return String::new();
    }
    "this build of spectra-runtime does not include the `onnx` feature, so no local model can be \
     loaded; rebuild with `--features onnx` (it links onnxruntime) or use an OpenAI-compatible \
     endpoint"
        .to_string()
}

/// Sampling parameters for local generation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sampling {
    /// `0` or negative means greedy.
    pub temperature: f64,
    /// `0` or `1` means greedy; otherwise the candidate pool size.
    pub top_k: usize,
    /// Seed for the deterministic splitmix64 stream.
    pub seed: u64,
}

impl Default for Sampling {
    fn default() -> Self {
        Self {
            temperature: 0.0,
            top_k: 0,
            seed: 0,
        }
    }
}

impl Sampling {
    /// The deterministic choice: greedy, fixed seed.
    pub fn greedy(seed: u64) -> Self {
        Self {
            temperature: 0.0,
            top_k: 0,
            seed,
        }
    }
}

/// One generated answer plus the accounting a host needs for its own budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    /// The decoded continuation, excluding the prompt.
    pub text: String,
    /// Tokens the prompt encoded to.
    pub prompt_tokens: usize,
    /// Tokens generated (including a stop token that was not appended, so the
    /// caller's accounting matches what the model actually produced).
    pub output_tokens: usize,
}

/// A WordPiece tokenizer loaded from an artifact.
///
/// The artifact is the one `spectra.std.ml.tokenizer_load` accepts
/// (`kind = "multi_array"`, `tokenizer_type = "wordpiece"`), so a tokenizer
/// prepared for a Spectra program is exactly what a host uses here.
pub struct Tokenizer {
    inner: Arc<crate::stdlib::MlWordpieceTokenizer>,
}

impl std::fmt::Debug for Tokenizer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Tokenizer")
            .field("vocabulary", &self.inner.token_to_id.len())
            .field("eos", &self.eos_id())
            .field("unk", &self.unk_id())
            .finish()
    }
}

impl Tokenizer {
    /// Loads a tokenizer artifact from `path`.
    pub fn load(path: &str) -> Result<Self, LocalModelError> {
        if path.trim().is_empty() {
            return Err(LocalModelError::Missing(
                "no tokenizer artifact path was given".to_string(),
            ));
        }
        let data = crate::artifact::read(Path::new(path))
            .map_err(|error| LocalModelError::Io(format!("'{path}': {error}")))?;
        let tokenizer = ml_parse_artifact_tokenizer(&data).ok_or_else(|| {
            LocalModelError::InvalidArtifact(format!(
                "'{path}' is not a wordpiece tokenizer artifact (expected kind 'multi_array' with \
                 tokenizer_type 'wordpiece' and a 'vocab_json' entry)"
            ))
        })?;
        Ok(Self {
            inner: Arc::new(tokenizer),
        })
    }

    /// Encodes text into token ids.
    pub fn encode(&self, text: &str) -> Vec<i64> {
        ml_wordpiece_encode(&self.inner, text)
    }

    /// Decodes token ids back into text.
    pub fn decode(&self, ids: &[i64]) -> Result<String, LocalModelError> {
        ml_wordpiece_decode(&self.inner, ids).ok_or_else(|| {
            LocalModelError::InvalidArtifact(
                "the tokenizer could not decode the generated ids".to_string(),
            )
        })
    }

    /// The artifact's end-of-sequence id, when it declares one.
    pub fn eos_id(&self) -> Option<i64> {
        ["eos", "</s>", "<eos>", "<|endoftext|>"]
            .iter()
            .find_map(|key| self.inner.special_tokens.get(*key).copied())
    }

    /// The artifact's unknown-token id.
    pub fn unk_id(&self) -> i64 {
        self.inner.unk_id
    }
}

/// A causal language model plus its tokenizer.
///
/// Loading is cached per path pair; the returned `Arc` keeps the session alive
/// for as long as any holder needs it.
pub struct CausalLm {
    session: u64,
    tokenizer: Arc<Tokenizer>,
    model_path: String,
}

impl std::fmt::Debug for CausalLm {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CausalLm")
            .field("session", &self.session)
            .field("model_path", &self.model_path)
            .finish_non_exhaustive()
    }
}

impl CausalLm {
    /// Loads or reuses the model at `model_path`, tokenized by `tokenizer_path`.
    pub fn load(model_path: &str, tokenizer_path: &str) -> Result<Arc<Self>, LocalModelError> {
        if model_path.trim().is_empty() {
            return Err(LocalModelError::Missing(
                "no local model path was given".to_string(),
            ));
        }
        let key = format!("{model_path}\u{0}{tokenizer_path}");
        let cache = causal_cache();
        if let Some(existing) = lock(cache).get(&key) {
            return Ok(Arc::clone(existing));
        }
        let tokenizer = Arc::new(Tokenizer::load(tokenizer_path)?);
        let session = commit_causal_session(model_path)?;
        let loaded = Arc::new(Self {
            session,
            tokenizer,
            model_path: model_path.to_string(),
        });
        lock(cache).insert(key, Arc::clone(&loaded));
        Ok(loaded)
    }

    /// The model's tokenizer.
    pub fn tokenizer(&self) -> &Arc<Tokenizer> {
        &self.tokenizer
    }

    /// The path the model was loaded from.
    pub fn model_path(&self) -> &str {
        &self.model_path
    }

    /// Generates a continuation for `prompt`.
    ///
    /// `max_new_tokens` bounds the continuation; the model's own end-of-
    /// sequence token (when the tokenizer declares one) stops it earlier.
    pub fn generate(
        &self,
        prompt: &str,
        max_new_tokens: usize,
        sampling: Sampling,
    ) -> Result<Generated, LocalModelError> {
        let ids = self.tokenizer.encode(prompt);
        if ids.is_empty() {
            return Err(LocalModelError::InvalidArtifact(
                "the prompt encoded to no tokens".to_string(),
            ));
        }
        let eos = self.tokenizer.eos_id().unwrap_or(-1);
        let generated = self.generate_ids(&ids, max_new_tokens, eos, sampling)?;
        let continuation = &generated[ids.len()..];
        let text = self.tokenizer.decode(continuation)?;
        Ok(Generated {
            text,
            prompt_tokens: ids.len(),
            output_tokens: continuation.len(),
        })
    }

    /// Generates a continuation, reporting text as it is produced.
    ///
    /// `on_text` receives the text each newly generated token adds, decoded
    /// against the tokens produced so far (WordPiece continuations make a
    /// token boundary a partial word boundary, so a chunk is a text delta, not
    /// a token spelling). The returned [`Generated`] is the same value
    /// [`Self::generate`] returns, so a streaming caller can compare the two.
    ///
    /// A callback that returns `false` stops generation after the token it
    /// just received, which is how a caller with an interactive budget ends a
    /// stream early without discarding what was already produced.
    pub fn generate_streaming(
        &self,
        prompt: &str,
        max_new_tokens: usize,
        sampling: Sampling,
        on_text: &mut dyn FnMut(&str) -> bool,
    ) -> Result<Generated, LocalModelError> {
        let ids = self.tokenizer.encode(prompt);
        if ids.is_empty() {
            return Err(LocalModelError::InvalidArtifact(
                "the prompt encoded to no tokens".to_string(),
            ));
        }
        let eos = self.tokenizer.eos_id().unwrap_or(-1);
        let mut continuation: Vec<i64> = Vec::new();
        let mut emitted = String::new();
        let mut keep_going = true;
        let generated = stream_generate(
            self.session,
            &ids,
            max_new_tokens,
            eos,
            crate::stdlib::MlGenerateSampling {
                temperature: sampling.temperature,
                top_k: sampling.top_k,
                seed: sampling.seed,
            },
            &mut |token| {
                if !keep_going {
                    return false;
                }
                continuation.push(token);
                let whole = match self.tokenizer.decode(&continuation) {
                    Ok(text) => text,
                    // A partial id sequence can be undecodable (a continuation
                    // prefix with no base token yet); it is not an error while
                    // more tokens may follow.
                    Err(_) => return true,
                };
                if let Some(delta) = whole.strip_prefix(emitted.as_str()) {
                    if !delta.is_empty() {
                        keep_going = on_text(delta);
                        emitted = whole;
                    }
                    return keep_going;
                }
                // Decoding is not always an extension (a token can rewrite the
                // tail); emit the whole text again rather than a wrong delta.
                keep_going = on_text(&whole);
                emitted = whole;
                keep_going
            },
        )?;
        let produced = &generated[ids.len()..];
        Ok(Generated {
            text: self.tokenizer.decode(produced)?,
            prompt_tokens: ids.len(),
            output_tokens: produced.len(),
        })
    }

    /// Generates raw ids, reporting each token to `on_token` as it is
    /// produced.
    ///
    /// The streaming counterpart of [`Self::generate_ids`], with the same
    /// explicit stop token: a caller that needs a stop token other than the
    /// tokenizer's (a chat template's turn separator, for instance) drives
    /// generation through this entry.
    pub fn generate_ids_streaming(
        &self,
        input_ids: &[i64],
        max_new_tokens: usize,
        eos_id: i64,
        sampling: Sampling,
        on_token: &mut dyn FnMut(i64) -> bool,
    ) -> Result<Vec<i64>, LocalModelError> {
        if max_new_tokens == 0 {
            return Err(LocalModelError::Inference(
                "max_new_tokens must be at least 1".to_string(),
            ));
        }
        stream_generate(
            self.session,
            input_ids,
            max_new_tokens,
            eos_id,
            crate::stdlib::MlGenerateSampling {
                temperature: sampling.temperature,
                top_k: sampling.top_k,
                seed: sampling.seed,
            },
            on_token,
        )
    }

    /// Generates raw ids: the shared engine call every generation path uses.
    pub fn generate_ids(
        &self,
        input_ids: &[i64],
        max_new_tokens: usize,
        eos_id: i64,
        sampling: Sampling,
    ) -> Result<Vec<i64>, LocalModelError> {
        if max_new_tokens == 0 {
            return Err(LocalModelError::Inference(
                "max_new_tokens must be at least 1".to_string(),
            ));
        }
        if !(0.0..=f64::MAX).contains(&sampling.temperature) || !sampling.temperature.is_finite() {
            return Err(LocalModelError::Inference(format!(
                "temperature must be finite and non-negative, found {}",
                sampling.temperature
            )));
        }
        run_generate(
            self.session,
            input_ids,
            max_new_tokens,
            eos_id,
            crate::stdlib::MlGenerateSampling {
                temperature: sampling.temperature,
                top_k: sampling.top_k,
                seed: sampling.seed,
            },
        )
    }
}

impl Drop for CausalLm {
    fn drop(&mut self) {
        free_session(self.session);
    }
}

/// A sentence-embedding model plus its tokenizer.
pub struct EmbeddingModel {
    session: u64,
    tokenizer: Arc<Tokenizer>,
    model_path: String,
}

impl std::fmt::Debug for EmbeddingModel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EmbeddingModel")
            .field("session", &self.session)
            .field("model_path", &self.model_path)
            .finish_non_exhaustive()
    }
}

impl EmbeddingModel {
    /// Loads or reuses the embedding model at `model_path`.
    pub fn load(model_path: &str, tokenizer_path: &str) -> Result<Arc<Self>, LocalModelError> {
        if model_path.trim().is_empty() {
            return Err(LocalModelError::Missing(
                "no local embedding model path was given".to_string(),
            ));
        }
        let key = format!("embed\u{0}{model_path}\u{0}{tokenizer_path}");
        let cache = embedding_cache();
        if let Some(existing) = lock(cache).get(&key) {
            return Ok(Arc::clone(existing));
        }
        let tokenizer = Arc::new(Tokenizer::load(tokenizer_path)?);
        let session = commit_embedding_session(model_path)?;
        let loaded = Arc::new(Self {
            session,
            tokenizer,
            model_path: model_path.to_string(),
        });
        lock(cache).insert(key, Arc::clone(&loaded));
        Ok(loaded)
    }

    /// The model's tokenizer.
    pub fn tokenizer(&self) -> &Arc<Tokenizer> {
        &self.tokenizer
    }

    /// Embeds `text` into a normalized sentence vector.
    pub fn embed(&self, text: &str) -> Result<Vec<f64>, LocalModelError> {
        let ids = self.tokenizer.encode(text);
        if ids.is_empty() {
            return Err(LocalModelError::InvalidArtifact(
                "the text encoded to no tokens".to_string(),
            ));
        }
        run_embedding(self.session, &ids)
    }
}

impl Drop for EmbeddingModel {
    fn drop(&mut self) {
        free_session(self.session);
    }
}

// ── plumbing ─────────────────────────────────────────────────────────────

fn causal_cache() -> &'static Mutex<HashMap<String, Arc<CausalLm>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<CausalLm>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn embedding_cache() -> &'static Mutex<HashMap<String, Arc<EmbeddingModel>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<EmbeddingModel>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(feature = "onnx")]
fn commit_causal_session(model_path: &str) -> Result<u64, LocalModelError> {
    let bytes = std::fs::read(model_path)
        .map_err(|error| LocalModelError::Io(format!("'{model_path}': {error}")))?;
    let session = crate::stdlib::ml_onnx_commit_session(&bytes).map_err(|status| {
        LocalModelError::InvalidArtifact(format!(
            "'{model_path}' could not be committed as an onnxruntime session (host status {status})"
        ))
    })?;
    // A causal language model takes exactly one graph input: the token ids.
    // Checking the arity here (the embedding path checks for two) turns "this
    // is a different graph" into a load-time error instead of a shape failure
    // in the middle of a turn.
    let arity = {
        let sessions = crate::stdlib::ml_onnx_sessions_lock();
        sessions.get(&session).map(|committed| committed.inputs().len())
    };
    match arity {
        Some(1) => Ok(session),
        Some(count) => {
            free_session(session);
            Err(LocalModelError::InvalidArtifact(format!(
                "'{model_path}' has {count} graph inputs; a causal language model takes exactly one"
            )))
        }
        None => Err(LocalModelError::Inference(format!(
            "the committed session for '{model_path}' disappeared before it could be inspected"
        ))),
    }
}

#[cfg(not(feature = "onnx"))]
fn commit_causal_session(model_path: &str) -> Result<u64, LocalModelError> {
    Err(LocalModelError::Unavailable(format!(
        "cannot load '{model_path}': {}",
        unavailable_reason()
    )))
}

#[cfg(feature = "onnx")]
fn commit_embedding_session(model_path: &str) -> Result<u64, LocalModelError> {
    let bytes = std::fs::read(model_path)
        .map_err(|error| LocalModelError::Io(format!("'{model_path}': {error}")))?;
    crate::stdlib::ml_text_embed_commit_session(&bytes).map_err(|status| {
        LocalModelError::InvalidArtifact(format!(
            "'{model_path}' is not an embedding graph this runtime accepts (two int64 inputs; host \
             status {status})"
        ))
    })
}

#[cfg(not(feature = "onnx"))]
fn commit_embedding_session(model_path: &str) -> Result<u64, LocalModelError> {
    Err(LocalModelError::Unavailable(format!(
        "cannot load '{model_path}': {}",
        unavailable_reason()
    )))
}

#[cfg(feature = "onnx")]
fn run_generate(
    session: u64,
    input_ids: &[i64],
    max_new_tokens: usize,
    eos_id: i64,
    sampling: crate::stdlib::MlGenerateSampling,
) -> Result<Vec<i64>, LocalModelError> {
    crate::stdlib::ml_generate_inner(session, input_ids, max_new_tokens, eos_id, Some(sampling))
        .map_err(|status| {
            LocalModelError::Inference(format!("generation failed (host status {status})"))
        })
}

#[cfg(not(feature = "onnx"))]
fn run_generate(
    _session: u64,
    _input_ids: &[i64],
    _max_new_tokens: usize,
    _eos_id: i64,
    _sampling: crate::stdlib::MlGenerateSampling,
) -> Result<Vec<i64>, LocalModelError> {
    Err(LocalModelError::Unavailable(unavailable_reason()))
}

#[cfg(feature = "onnx")]
fn stream_generate(
    session: u64,
    input_ids: &[i64],
    max_new_tokens: usize,
    eos_id: i64,
    sampling: crate::stdlib::MlGenerateSampling,
    on_token: &mut dyn FnMut(i64) -> bool,
) -> Result<Vec<i64>, LocalModelError> {
    crate::stdlib::ml_generate_with(
        session,
        input_ids,
        max_new_tokens,
        eos_id,
        Some(sampling),
        on_token,
    )
    .map_err(|status| LocalModelError::Inference(format!("generation failed (host status {status})")))
}

#[cfg(not(feature = "onnx"))]
fn stream_generate(
    _session: u64,
    _input_ids: &[i64],
    _max_new_tokens: usize,
    _eos_id: i64,
    _sampling: crate::stdlib::MlGenerateSampling,
    _on_token: &mut dyn FnMut(i64) -> bool,
) -> Result<Vec<i64>, LocalModelError> {
    Err(LocalModelError::Unavailable(unavailable_reason()))
}

#[cfg(feature = "onnx")]
fn run_embedding(session: u64, ids: &[i64]) -> Result<Vec<f64>, LocalModelError> {
    let mask = vec![1i64; ids.len()];
    crate::stdlib::ml_text_embed_model_inner(session, ids, &mask).map_err(|status| {
        LocalModelError::Inference(format!("embedding failed (host status {status})"))
    })
}

#[cfg(not(feature = "onnx"))]
fn run_embedding(_session: u64, _ids: &[i64]) -> Result<Vec<f64>, LocalModelError> {
    Err(LocalModelError::Unavailable(unavailable_reason()))
}

/// Releases one session from the shared table. Dropping a model whose session
/// was already freed is not an error.
fn free_session(session: u64) {
    #[cfg(feature = "onnx")]
    {
        crate::stdlib::ml_onnx_sessions_lock().remove(&session);
    }
    #[cfg(not(feature = "onnx"))]
    {
        let _ = session;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes the checked-in local-model fixtures the host-level tests load.
    ///
    /// Ignored by default because it writes into the repository; run it once
    /// with `cargo test -p spectra-runtime --features onnx --lib
    /// local_model::tests::write_local_model_fixtures -- --ignored
    /// --nocapture` after the runtime's model contracts change. It is a
    /// fixture writer, not a test: the model bytes come from the runtime's own
    /// builder, so the fixture and the runtime cannot drift.
    #[cfg(feature = "onnx")]
    #[test]
    #[ignore = "writes tests/fixtures/r3211; run explicitly to regenerate"]
    fn write_local_model_fixtures() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("tests/fixtures/r3211");
        std::fs::create_dir_all(&root).expect("create fixture dir");

        let model = crate::stdlib::ml_generation_fixture_proto();
        std::fs::write(root.join("toy_causal_lm.onnx"), &model).expect("write model");
        let embedding = crate::stdlib::ml_text_embed_fixture_proto();
        std::fs::write(root.join("toy_embedding.onnx"), &embedding).expect("write embedding model");

        // The tokenizer's vocabulary covers every id the toy models can emit,
        // and its stop token is 3 — inside the toy model's 0→1→2→3 cycle — so
        // a generation that does not override the stop token ends after two
        // tokens instead of running to its budget.
        let tokens: Vec<String> = (0..6)
            .map(|id| format!("{{\"id\":{id},\"token\":\"{}\"}}", ["[UNK]", "a", "b", "c", "d", "e"][id]))
            .collect();
        let vocab = format!(
            "{{\"tokens\":[{}],\"special_tokens\":{{\"unk\":0,\"eos\":3}},\"lowercase\":true,\"continuation_prefix\":\"##\"}}",
            tokens.join(",")
        );
        let mut metadata = std::collections::BTreeMap::new();
        metadata.insert("tokenizer_type".to_string(), "wordpiece".to_string());
        metadata.insert("tokenizer_version".to_string(), "v1".to_string());
        metadata.insert("vocab_json".to_string(), vocab);
        // One `token_ids` array, exactly like a tokenizer artifact produced by
        // `spectra.std.ml.tokenizer_load`'s writer path.
        let token_ids: Vec<u8> = (0i64..6).flat_map(|id| id.to_le_bytes()).collect();
        let artifact = crate::artifact::ArtifactData {
            name: "r3211-tokenizer".to_string(),
            model_version: "tokenizer-v1".to_string(),
            kind: "multi_array".to_string(),
            metadata,
            tensors: vec![crate::artifact::TensorPayload {
                name: "token_ids".to_string(),
                dtype: "int".to_string(),
                precision: "f64".to_string(),
                shape: vec![6],
                layout: "contiguous".to_string(),
                bytes: token_ids,
            }],
        };
        crate::artifact::write_atomic(&root.join("tokenizer.spar"), &artifact)
            .expect("write tokenizer artifact");
        println!("wrote fixtures to {}", root.display());
    }

    #[test]
    #[cfg(feature = "onnx")]
    fn the_toy_model_generates_and_stops_at_eos() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("tests/fixtures/r3211");
        let model = CausalLm::load(
            root.join("toy_causal_lm.onnx").to_str().expect("path"),
            root.join("tokenizer.spar").to_str().expect("path"),
        )
        .expect("load the toy causal LM");

        // The tokenizer maps "a" to id 0, and the toy model's next-token
        // function is 0 -> 1 -> 2 -> 3 (the stop token), so one prompt token
        // yields exactly the "b" and "c" tokens.
        // The tokenizer maps "a" to id 1, and the toy model's next-token
        // function walks 1 -> 2 -> 3 -> 0 -> 1 ...
        assert_eq!(model.tokenizer().encode("a"), vec![1]);
        assert_eq!(model.tokenizer().eos_id(), Some(3));

        // The stop token sits inside the cycle, so the default generation is
        // the single "b" token and the budget (two) never runs out.
        let generated = model.generate("a", 2, Sampling::greedy(11)).expect("generate");
        assert_eq!(generated.prompt_tokens, 1);
        assert_eq!(generated.output_tokens, 1);
        assert!(generated.text.contains('b'), "decoded continuation: {:?}", generated.text);

        // Greedy is deterministic; a different seed changes nothing when the
        // temperature is zero.
        let again = model.generate("a", 2, Sampling::greedy(99)).expect("generate");
        assert_eq!(again.text, generated.text);

        // The default stop token (the tokenizer's) ends generation and is not
        // appended: two new tokens, whatever the budget is.
        let ids = model.tokenizer().encode("a");
        let stopped = model
            .generate_ids(&ids, 7, 3, Sampling::greedy(3))
            .expect("generate ids");
        assert_eq!(stopped, vec![1, 2]);

        // With no stop token the whole budget is spent, and the generated ids
        // are exactly the model's cycle.
        let full = model
            .generate_ids(&ids, 7, -1, Sampling::greedy(3))
            .expect("generate ids");
        assert_eq!(full, vec![1, 2, 3, 0, 1, 2, 3, 0]);

        // Paths and errors name what is wrong.
        let missing = CausalLm::load("", "");
        assert!(matches!(missing, Err(LocalModelError::Missing(_))), "{missing:?}");
        let absent = CausalLm::load("/nonexistent/model.onnx", "/nonexistent/tokenizer.spar");
        assert!(matches!(absent, Err(LocalModelError::Io(_))), "{absent:?}");
        let wrong = CausalLm::load(
            root.join("toy_embedding.onnx").to_str().expect("path"),
            root.join("tokenizer.spar").to_str().expect("path"),
        );
        assert!(
            matches!(wrong, Err(LocalModelError::InvalidArtifact(_))),
            "{wrong:?}"
        );
    }

    #[test]
    #[cfg(feature = "onnx")]
    fn streaming_matches_non_streaming_token_for_token() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("tests/fixtures/r3211");
        let model = CausalLm::load(
            root.join("toy_causal_lm.onnx").to_str().expect("path"),
            root.join("tokenizer.spar").to_str().expect("path"),
        )
        .expect("load the toy causal LM");

        let mut deltas: Vec<String> = Vec::new();
        let streamed = model
            .generate_streaming("a", 7, Sampling::greedy(3), &mut |delta| {
                deltas.push(delta.to_string());
                true
            })
            .expect("stream");
        let whole = model.generate("a", 7, Sampling::greedy(3)).expect("generate");
        assert_eq!(streamed, whole, "streaming and one-shot agree");
        assert_eq!(deltas.concat(), whole.text, "the deltas reassemble the answer");
        assert_eq!(deltas.len(), 1, "one delta per generated token");

        // With a stop token the model never emits, the budget bounds the run:
        // seven tokens arrive one at a time, and a callback that stops after
        // two ends the sequence there (keeping the token it saw).
        let ids = model.tokenizer().encode("a");
        let mut streamed_ids: Vec<i64> = Vec::new();
        let full = model
            .generate_ids_streaming(&ids, 7, -1, Sampling::greedy(3), &mut |token| {
                streamed_ids.push(token);
                true
            })
            .expect("stream ids");
        assert_eq!(full, vec![1, 2, 3, 0, 1, 2, 3, 0]);
        assert_eq!(streamed_ids, vec![2, 3, 0, 1, 2, 3, 0], "one callback per token");

        let mut seen = 0;
        let stopped = model
            .generate_ids_streaming(&ids, 7, -1, Sampling::greedy(3), &mut |_token| {
                seen += 1;
                false
            })
            .expect("stream ids");
        assert_eq!(seen, 1);
        assert_eq!(stopped, vec![1, 2], "the token that stopped it is kept");
    }

    #[test]
    #[cfg(feature = "onnx")]
    fn the_toy_embedding_model_embeds_text() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("tests/fixtures/r3211");
        let model = EmbeddingModel::load(
            root.join("toy_embedding.onnx").to_str().expect("path"),
            root.join("tokenizer.spar").to_str().expect("path"),
        )
        .expect("load the toy embedding model");
        let vector = model.embed("a b").expect("embed");
        assert_eq!(vector.len(), 4, "the fixture embeds into four dimensions");
        let norm: f64 = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-9, "L2 normalized, got {norm}");
        assert_eq!(model.embed("a b").expect("embed"), vector, "deterministic");
    }

    #[test]
    fn tokenizers_report_their_own_configuration_errors() {
        let missing = Tokenizer::load("");
        assert!(matches!(missing, Err(LocalModelError::Missing(_))), "{missing:?}");
        let absent = Tokenizer::load("/nonexistent/tokenizer.spar");
        assert!(matches!(absent, Err(LocalModelError::Io(_))), "{absent:?}");
    }
}
