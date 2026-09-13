//! `std.agent` memory: `remember`/`recall` over the runtime vector index
//! (R-3212).
//!
//! The storage primitive is the validated `std.ml` HNSW vector index
//! ([`spectra_runtime::vector_index::VectorIndex`]): `remember` embeds the text
//! through the run's provider and appends it, `recall` embeds the query and
//! retrieves the nearest entries. This module adds only what a memory needs on
//! top of retrieval:
//!
//! - **Tiers.** Every entry is classified as [`MemoryTier::Episodic`],
//!   [`MemoryTier::Semantic`] or [`MemoryTier::Procedural`]. The run surface
//!   records the run's own experience, so `remember(run, text)` writes
//!   `episodic`; the store API accepts every tier and recall reports it.
//! - **Provenance.** Each entry records its origin, the run identity that
//!   wrote it, the run goal and the write timestamp, so a recall result is
//!   explainable without consulting a separate log.
//! - **Deterministic recall.** Entries are ranked by cosine score with ties
//!   broken by insertion order, and the payload is capped by the tokenizer's
//!   token budget rather than only by `top_k`.
//! - **Scope.** A store is keyed by the run goal that wrote it, so a restarted
//!   agent with the same goal recalls earlier memory while every entry still
//!   names the run that produced it: its durable `run_id` (the journal's key)
//!   and the process-local handle that wrote it.
//!
//! Persistence reuses the vector-index artifact format
//! ([`spectra_runtime::vector_index::write_artifact`]/`read_artifact`): the
//! provenance ledger travels in the artifact metadata, so a store written by
//! one process loads byte-identically in another with no side state.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use spectra_runtime::ffi::{SpectraHostCallContext, HOST_STATUS_INVALID_ARGUMENT};
use spectra_runtime::stdlib::text_token_count;
use spectra_runtime::vector_index::{self, VectorIndex};

use crate::abi;
use crate::error::AgentError;
use crate::hosts;

/// Token budget of one recall payload.
///
/// The payload stops accepting entries once the sum of the tokenizer counts of
/// the included texts would exceed this value, independently of `top_k`, so a
/// single long memory cannot flood the caller.
pub(crate) const RECALL_TOKEN_BUDGET: usize = 512;

/// Metadata key carrying the provenance ledger inside the vector-index
/// artifact; it is the only key this module adds to the shared format.
const ENTRIES_METADATA_KEY: &str = "agent_memory_entries";

/// `model_version` required by the vector-index artifact encoding.
const MEMORY_MODEL_VERSION: &str = "agent-memory/v1";

/// Memory tier of one entry.
///
/// `remember` writes `episodic` (the run's own experience); the other two
/// tiers and the parsing path belong to the persistence API exercised by the
/// memory regression tests, so they are dead code in a surface-only build.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryTier {
    /// What happened during a run: the run surface's own writes.
    Episodic,
    /// Durable facts the agent learned.
    Semantic,
    /// How to do something, recorded for reuse.
    Procedural,
}

#[cfg_attr(not(test), allow(dead_code))]
impl MemoryTier {
    /// Every tier, in declaration order.
    pub(crate) const ALL: [MemoryTier; 3] = [
        MemoryTier::Episodic,
        MemoryTier::Semantic,
        MemoryTier::Procedural,
    ];

    /// Stable string form used in the recall payload and the artifact ledger.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            MemoryTier::Episodic => "episodic",
            MemoryTier::Semantic => "semantic",
            MemoryTier::Procedural => "procedural",
        }
    }

    /// Parses the stable string form; unknown values are rejected so a corrupt
    /// ledger is reported instead of silently downgraded.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "episodic" => MemoryTier::Episodic,
            "semantic" => MemoryTier::Semantic,
            "procedural" => MemoryTier::Procedural,
            _ => return None,
        })
    }
}

/// One stored memory with its provenance.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MemoryEntry {
    /// Insertion order; also the vector-index id suffix.
    pub ordinal: u64,
    pub tier: MemoryTier,
    pub origin: String,
    /// Process-local handle of the run that wrote the entry.
    pub run: i64,
    /// The writing run's `run_id`; empty when the spec declared none. The
    /// handle above is meaningless after a restart, so the durable name of the
    /// writer is this value -- the same one the journal keys on.
    pub run_id: String,
    pub goal: String,
    pub timestamp_ms: u64,
    pub text: String,
}

/// Entries plus their vectors, scoped to one agent goal.
#[derive(Debug, Default)]
pub(crate) struct MemoryStore {
    entries: Vec<MemoryEntry>,
    index: Option<VectorIndex>,
    dimension: usize,
}

impl MemoryStore {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Appends one memory and returns whether it was stored.
    ///
    /// The first insert fixes the store's vector dimension; a later embedding
    /// of a different width is a typed error rather than a partial write.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn remember(
        &mut self,
        tier: MemoryTier,
        origin: &str,
        run_handle: i64,
        run_id: &str,
        goal: &str,
        timestamp_ms: u64,
        text: &str,
        vector: &[f64],
    ) -> Result<bool, AgentError> {
        if vector.is_empty() || vector.iter().any(|value| !value.is_finite()) {
            return Err(AgentError::Memory(
                "embedding must be finite and non-empty".to_string(),
            ));
        }
        if self.index.is_none() {
            let mut index = VectorIndex::new(vector.len()).map_err(memory_error)?;
            index.set_custom_metadata("model_version", MEMORY_MODEL_VERSION);
            self.dimension = vector.len();
            self.index = Some(index);
        }
        if vector.len() != self.dimension {
            return Err(AgentError::Memory(format!(
                "embedding dimension {} does not match the store dimension {}",
                vector.len(),
                self.dimension
            )));
        }
        let ordinal = self.entries.len() as u64;
        let index = self.index.as_mut().ok_or_else(|| {
            AgentError::Memory("the memory store has no vector index".to_string())
        })?;
        index
            .insert(entry_id(ordinal), vector)
            .map_err(memory_error)?;
        self.entries.push(MemoryEntry {
            ordinal,
            tier,
            origin: origin.to_string(),
            run: run_handle,
            run_id: run_id.to_string(),
            goal: goal.to_string(),
            timestamp_ms,
            text: text.to_string(),
        });
        Ok(true)
    }

    /// Renders the recall payload for `query_vector`.
    ///
    /// Ranking is cosine score descending with ties broken by insertion order;
    /// the payload accepts entries while their combined tokenizer count stays
    /// within `token_budget`.
    pub(crate) fn recall(
        &mut self,
        scope: &str,
        query: &str,
        query_vector: &[f64],
        top_k: usize,
        token_budget: usize,
    ) -> Result<String, AgentError> {
        if self.entries.is_empty() {
            return Ok(render_recall(scope, query, top_k, token_budget, 0, false, &[]));
        }
        let total = self.entries.len();
        let index = self.index.as_mut().ok_or_else(|| {
            AgentError::Memory("the memory store has no vector index".to_string())
        })?;
        let evidence = index
            .query(query_vector, top_k.min(total))
            .map_err(memory_error)?;
        let mut ranked = Vec::with_capacity(evidence.results.len());
        for result in &evidence.results {
            let ordinal = ordinal_of(&result.id).ok_or_else(|| {
                AgentError::Memory(format!(
                    "vector '{}' is not a memory entry id",
                    result.id
                ))
            })?;
            ranked.push((ordinal, result.score));
        }
        ranked.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        let mut selected: Vec<(&MemoryEntry, f64)> = Vec::with_capacity(ranked.len());
        let mut token_count = 0usize;
        let mut truncated = false;
        for (ordinal, score) in ranked {
            let entry = self.entries.get(ordinal as usize).ok_or_else(|| {
                AgentError::Memory(format!("memory ledger has no entry {ordinal}"))
            })?;
            let tokens = text_token_count(&entry.text);
            if token_count.saturating_add(tokens) > token_budget {
                truncated = true;
                continue;
            }
            token_count += tokens;
            selected.push((entry, score));
        }
        Ok(render_recall(
            scope,
            query,
            top_k,
            token_budget,
            token_count,
            truncated,
            &selected,
        ))
    }

    /// Writes the store in the vector-index artifact format.
    ///
    /// The provenance ledger is encoded into the index metadata only here: a
    /// `remember` never pays for a serialization it does not need.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn persist(&mut self, path: &Path) -> Result<(), AgentError> {
        let encoded = encode_entries(&self.entries);
        let index = self.index.as_mut().ok_or_else(|| {
            AgentError::Memory("cannot persist an empty memory store".to_string())
        })?;
        index.set_custom_metadata(ENTRIES_METADATA_KEY, &encoded);
        vector_index::write_artifact(path, index).map_err(|detail| {
            AgentError::Memory(format!("cannot persist the memory store: {detail}"))
        })
    }

    /// Loads a store written by [`MemoryStore::persist`] (or by
    /// `spectra.std.ml.vector_index_persist` over a memory store's index).
    ///
    /// The returned store shares nothing with the writer beyond the artifact
    /// bytes: this is the restart path.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn load(path: &Path) -> Result<Self, AgentError> {
        let index = vector_index::read_artifact(path).map_err(|detail| {
            AgentError::Memory(format!("cannot load the memory store: {detail}"))
        })?;
        let encoded = index.custom_metadata(ENTRIES_METADATA_KEY).ok_or_else(|| {
            AgentError::Memory("the artifact carries no memory ledger".to_string())
        })?;
        let entries = decode_entries(encoded)?;
        if entries.len() != index.len() {
            return Err(AgentError::Memory(format!(
                "the memory ledger has {} entries but the index has {}",
                entries.len(),
                index.len()
            )));
        }
        let dimension = index.dimension();
        Ok(Self {
            entries,
            index: Some(index),
            dimension,
        })
    }
}

fn memory_error(error: vector_index::VectorIndexError) -> AgentError {
    AgentError::Memory(format!("vector index rejected the memory: {error}"))
}

/// Vector-index id of one entry. Fixed width keeps the index's lexical
/// tie-break aligned with insertion order.
fn entry_id(ordinal: u64) -> String {
    format!("m{ordinal:08}")
}

/// Inverse of [`entry_id`]; `None` when the id was not written by this module.
fn ordinal_of(id: &str) -> Option<u64> {
    id.strip_prefix('m').and_then(|digits| {
        (digits.len() == 8)
            .then(|| digits.parse::<u64>().ok())
            .flatten()
    })
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn encode_entries(entries: &[MemoryEntry]) -> String {
    let rendered = entries
        .iter()
        .map(|entry| {
            format!(
                concat!(
                    "{{\"ordinal\":{},\"tier\":{},\"origin\":{},\"run\":{},\"run_id\":{},",
                    "\"goal\":{},\"timestamp_ms\":{},\"text\":{}}}"
                ),
                entry.ordinal,
                json_string(entry.tier.as_str()),
                json_string(&entry.origin),
                entry.run,
                json_string(&entry.run_id),
                json_string(&entry.goal),
                entry.timestamp_ms,
                json_string(&entry.text),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{rendered}]")
}

#[cfg_attr(not(test), allow(dead_code))]
fn decode_entries(encoded: &str) -> Result<Vec<MemoryEntry>, AgentError> {
    let value: serde_json::Value = serde_json::from_str(encoded).map_err(|error| {
        AgentError::Memory(format!("the memory ledger is not valid JSON: {error}"))
    })?;
    let array = value.as_array().ok_or_else(|| {
        AgentError::Memory("the memory ledger must be a JSON array".to_string())
    })?;
    let mut entries = Vec::with_capacity(array.len());
    for item in array {
        let invalid = || AgentError::Memory("the memory ledger entry is malformed".to_string());
        let ordinal = item.get("ordinal").and_then(|value| value.as_u64()).ok_or_else(invalid)?;
        let tier = item
            .get("tier")
            .and_then(|value| value.as_str())
            .and_then(MemoryTier::parse)
            .ok_or_else(invalid)?;
        let origin = item
            .get("origin")
            .and_then(|value| value.as_str())
            .ok_or_else(invalid)?
            .to_string();
        let run = item.get("run").and_then(|value| value.as_i64()).ok_or_else(invalid)?;
        // Absent in artifacts written before the run id was recorded; an empty
        // value is the honest reading of "the writer did not name itself".
        let run_id = item
            .get("run_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let goal = item
            .get("goal")
            .and_then(|value| value.as_str())
            .ok_or_else(invalid)?
            .to_string();
        let timestamp_ms = item
            .get("timestamp_ms")
            .and_then(|value| value.as_u64())
            .ok_or_else(invalid)?;
        let text = item
            .get("text")
            .and_then(|value| value.as_str())
            .ok_or_else(invalid)?
            .to_string();
        if ordinal != entries.len() as u64 {
            return Err(AgentError::Memory(format!(
                "the memory ledger is out of order at entry {ordinal}"
            )));
        }
        entries.push(MemoryEntry {
            ordinal,
            tier,
            origin,
            run,
            run_id,
            goal,
            timestamp_ms,
            text,
        });
    }
    Ok(entries)
}

#[allow(clippy::too_many_arguments)]
fn render_recall(
    scope: &str,
    query: &str,
    top_k: usize,
    token_budget: usize,
    token_count: usize,
    truncated: bool,
    entries: &[(&MemoryEntry, f64)],
) -> String {
    let rendered = entries
        .iter()
        .map(|(entry, score)| {
            format!(
                concat!(
                    "{{\"ordinal\":{},\"tier\":{},\"origin\":{},\"run\":{},\"run_id\":{},",
                    "\"goal\":{},\"timestamp_ms\":{},\"score\":{:.6},\"text\":{}}}"
                ),
                entry.ordinal,
                json_string(entry.tier.as_str()),
                json_string(&entry.origin),
                entry.run,
                json_string(&entry.run_id),
                json_string(&entry.goal),
                entry.timestamp_ms,
                score,
                json_string(&entry.text),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{\"schema\":\"spectra.agent.memory_recall.v1\",\"scope\":{},\"query\":{},",
            "\"top_k\":{},\"token_budget\":{},\"token_count\":{},\"truncated\":{},",
            "\"entries\":[{}]}}"
        ),
        json_string(scope),
        json_string(query),
        top_k,
        token_budget,
        token_count,
        truncated,
        rendered,
    )
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn lock<T>(mutex: &'static Mutex<T>) -> MutexGuard<'static, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Goal-scoped stores. Memory outlives the run that wrote it — that is the
/// point of a memory — so the registry is never cleared by `agent_end`.
fn stores() -> &'static Mutex<HashMap<String, MemoryStore>> {
    static STORES: LazyLock<Mutex<HashMap<String, MemoryStore>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    &STORES
}

/// Embeds `text` with the run's provider.
fn embed_with_run(run_handle: i64, text: &str) -> Result<Vec<f64>, AgentError> {
    let (spec, _) = hosts::snapshot(run_handle)?;
    let provider = hosts::resolve_provider(&spec)?;
    provider.embed(text).map_err(AgentError::from)
}

// ── host functions ───────────────────────────────────────────────────────

/// `spectra.std.agent.remember(run, text) -> Result<bool, Error>`.
///
/// The write is attributed to the run that made it: the entry records the run
/// handle, the run's durable id, the run goal, the provider-embedded text, a
/// timestamp and the `episodic` tier ("what happened during the run"). Stores
/// are scoped by goal, so a later run with the same goal sees the entry and can
/// still name the run that wrote it -- after a restart the handle is gone but
/// the id still resolves against the journal.
extern "C" fn remember_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((run_handle, strings)) = hosts::read_run_and_prompt(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let text = strings[0].clone();
    let outcome = (|| -> Result<i64, AgentError> {
        let (spec, _) = hosts::snapshot(run_handle)?;
        // The embedding happens outside the registry lock: provider work must
        // not serialize unrelated runs.
        let vector = embed_with_run(run_handle, &text)?;
        let timestamp_ms = now_ms();
        let mut registry = lock(stores());
        let store = registry.entry(spec.goal.clone()).or_insert_with(MemoryStore::new);
        let inserted = store.remember(
            MemoryTier::Episodic,
            "agent",
            run_handle,
            &spec.run_id,
            &spec.goal,
            timestamp_ms,
            &text,
            &vector,
        )?;
        Ok(i64::from(inserted))
    })();
    hosts::write_outcome(ctx, outcome)
}

/// `spectra.std.agent.recall(run, query, top_k) -> Result<string, Error>`.
extern "C" fn recall_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some(args) = abi::args(ctx) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if args.len() != 3 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let run_handle = args[0];
    let Some(query) = abi::read_string_arg(args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let top_k = args[2];
    let outcome = (|| -> Result<i64, AgentError> {
        if top_k < 1 {
            return Err(AgentError::Memory(format!(
                "top_k must be >= 1, found {top_k}"
            )));
        }
        let (spec, _) = hosts::snapshot(run_handle)?;
        let scope = spec.goal.clone();
        // An empty store answers without a provider: there is nothing to rank.
        let empty = lock(stores())
            .get(&scope)
            .map(MemoryStore::len)
            .unwrap_or(0)
            == 0;
        let payload = if empty {
            MemoryStore::new().recall(
                &scope,
                &query,
                &[],
                top_k as usize,
                RECALL_TOKEN_BUDGET,
            )?
        } else {
            let vector = embed_with_run(run_handle, &query)?;
            let mut registry = lock(stores());
            match registry.get_mut(&scope) {
                Some(store) => store.recall(
                    &scope,
                    &query,
                    &vector,
                    top_k as usize,
                    RECALL_TOKEN_BUDGET,
                )?,
                // The registry never removes a store; render the empty
                // payload instead of panicking if that ever changes.
                None => MemoryStore::new().recall(
                    &scope,
                    &query,
                    &[],
                    top_k as usize,
                    RECALL_TOKEN_BUDGET,
                )?,
            }
        };
        let pointer = unsafe { abi::alloc_string(&payload) };
        if pointer == 0 {
            return Err(AgentError::Internal(
                "could not allocate the recall payload".to_string(),
            ));
        }
        Ok(pointer)
    })();
    hosts::write_outcome(ctx, outcome)
}

/// Registers the memory host functions; returns the number of newly inserted
/// entries.
pub(crate) fn register() -> usize {
    let mut inserted = 0;
    for (name, function) in [
        (crate::REMEMBER_HOST_CALL, remember_host as _),
        (crate::RECALL_HOST_CALL, recall_host as _),
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
    use spectra_runtime::ffi::{clear_host_functions, HOST_STATUS_SUCCESS};

    fn vector(seed: f64) -> Vec<f64> {
        vec![seed, 1.0, 0.0, 0.0]
    }

    fn store_with_three() -> MemoryStore {
        let mut store = MemoryStore::new();
        store
            .remember(
                MemoryTier::Episodic,
                "agent",
                7,
                "run-7",
                "goal",
                100,
                "alpha",
                &vector(1.0),
            )
            .expect("first memory");
        store
            .remember(
                MemoryTier::Semantic,
                "ingest",
                8,
                "run-8",
                "goal",
                200,
                "beta",
                &vector(0.5),
            )
            .expect("second memory");
        store
            .remember(
                MemoryTier::Procedural,
                "skill",
                9,
                "run-9",
                "goal",
                300,
                "gamma",
                &vector(0.1),
            )
            .expect("third memory");
        store
    }

    #[test]
    fn tiers_round_trip_and_order_by_score_then_insertion() {
        assert_eq!(
            MemoryTier::ALL.map(MemoryTier::as_str),
            ["episodic", "semantic", "procedural"]
        );
        for tier in MemoryTier::ALL {
            assert_eq!(MemoryTier::parse(tier.as_str()), Some(tier));
        }
        assert_eq!(MemoryTier::parse("unknown"), None);

        let mut store = store_with_three();
        let payload = store
            .recall("goal", "alpha", &vector(1.0), 3, RECALL_TOKEN_BUDGET)
            .expect("recall");
        // Descending score, then insertion order.
        let alpha = payload.find("\"text\":\"alpha\"").expect("alpha present");
        let beta = payload.find("\"text\":\"beta\"").expect("beta present");
        let gamma = payload.find("\"text\":\"gamma\"").expect("gamma present");
        assert!(alpha < beta && beta < gamma, "{payload}");
        // Provenance of every entry is reported.
        assert!(payload.contains("\"origin\":\"agent\""), "{payload}");
        assert!(payload.contains("\"origin\":\"ingest\""), "{payload}");
        assert!(payload.contains("\"origin\":\"skill\""), "{payload}");
        assert!(payload.contains("\"tier\":\"semantic\""), "{payload}");
        assert!(payload.contains("\"run\":8"), "{payload}");
        // The durable name of the writer travels with the entry, so a recall
        // after a restart still resolves against the journal.
        assert!(payload.contains("\"run_id\":\"run-8\""), "{payload}");
        assert!(payload.contains("\"goal\":\"goal\""), "{payload}");
        assert!(payload.contains("\"timestamp_ms\":200"), "{payload}");
        assert!(payload.contains("\"truncated\":false"), "{payload}");
        assert!(payload.contains("\"token_count\":3"), "{payload}");
    }

    #[test]
    fn equal_scores_break_ties_by_insertion_order() {
        let mut store = MemoryStore::new();
        for text in ["first", "second"] {
            store
                .remember(
                    MemoryTier::Episodic,
                    "agent",
                    1,
                    "run-1",
                    "ties",
                    1,
                    text,
                    &vector(1.0),
                )
                .expect("memory");
        }
        let payload = store
            .recall("ties", "first", &vector(1.0), 2, RECALL_TOKEN_BUDGET)
            .expect("recall");
        let first = payload.find("\"text\":\"first\"").expect("first");
        let second = payload.find("\"text\":\"second\"").expect("second");
        assert!(first < second, "{payload}");
        // Identical inputs produce identical payloads.
        let again = store
            .recall("ties", "first", &vector(1.0), 2, RECALL_TOKEN_BUDGET)
            .expect("recall");
        assert_eq!(payload, again);
    }

    #[test]
    fn payload_is_capped_by_tokens_not_only_top_k() {
        let mut store = MemoryStore::new();
        let long = (0..40)
            .map(|index| format!("token{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        store
            .remember(
            MemoryTier::Episodic,
            "agent",
            1,
            "run-1",
            "budget",
            1,
            &long,
            &vector(1.0),
        )
            .expect("long memory");
        store
            .remember(
            MemoryTier::Episodic,
            "agent",
            1,
            "run-1",
            "budget",
            2,
            "short",
            &vector(1.0),
        )
            .expect("short memory");
        // top_k admits both entries; the token budget admits only the short one.
        let payload = store
            .recall("budget", "short", &vector(1.0), 2, 8)
            .expect("recall");
        assert!(payload.contains("\"truncated\":true"), "{payload}");
        assert!(payload.contains("\"text\":\"short\""), "{payload}");
        assert!(!payload.contains("token39"), "{payload}");
        assert!(payload.contains("\"token_count\":1"), "{payload}");
    }

    #[test]
    fn recall_reports_an_empty_store_without_entries() {
        let mut store = MemoryStore::new();
        let payload = store
            .recall("empty", "query", &vector(1.0), 3, RECALL_TOKEN_BUDGET)
            .expect("recall");
        assert!(payload.contains("\"entries\":[]"), "{payload}");
        assert!(payload.contains("\"truncated\":false"), "{payload}");
    }

    #[test]
    fn persist_and_load_survive_without_shared_state() {
        let mut store = store_with_three();
        let before = store
            .recall("goal", "alpha", &vector(1.0), 3, RECALL_TOKEN_BUDGET)
            .expect("recall");
        let path = std::env::temp_dir().join(format!(
            "spectra-agent-memory-{}-{}.idx",
            std::process::id(),
            now_ms()
        ));
        store.persist(&path).expect("persist");
        // Drop every in-process structure: the load below shares nothing with
        // the writer, which is exactly the restart path.
        drop(store);

        let mut restored = MemoryStore::load(&path).expect("load");
        let after = restored
            .recall("goal", "alpha", &vector(1.0), 3, RECALL_TOKEN_BUDGET)
            .expect("recall");
        assert_eq!(before, after, "a loaded store recalls identically");
        // The loaded provenance still names the writer's durable id, which is
        // what makes the entry identifiable after the writer's process is gone.
        assert!(after.contains("\"run_id\":\"run-7\""), "{after}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn dimension_mismatch_is_a_typed_error() {
        let mut store = MemoryStore::new();
        store
            .remember(
            MemoryTier::Episodic,
            "agent",
            1,
            "run-1",
            "goal",
            1,
            "a",
            &vector(1.0),
        )
            .expect("first memory");
        let error = store
            .remember(
                MemoryTier::Episodic,
                "agent",
                1,
                "run-1",
                "goal",
                2,
                "b",
                &[1.0, 2.0],
            )
            .expect_err("mismatched width");
        assert!(error.detail().contains("dimension"), "{error:?}");
    }

    /// Runs one registered memory host through its pointer.
    fn call(name: &str, args: &[i64]) -> (i32, i64) {
        let function = spectra_runtime::ffi::lookup_host_function(name).expect("host registered");
        let mut results = [0 as i64; 1];
        let mut ctx = SpectraHostCallContext {
            args: args.as_ptr(),
            arg_len: args.len(),
            results: results.as_mut_ptr(),
            result_len: results.len(),
            invoke_fn: None,
        };
        (function(&mut ctx), results[0])
    }

    fn tagged_payload(result: i64) -> i64 {
        unsafe { *(result as *const i64).add(1) }
    }

    #[test]
    fn memory_hosts_are_registered_and_answer_through_the_abi() {
        let _guard = crate::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear_host_functions();
        spectra_runtime::register();
        // token_count + twenty-six gateway/dispatch/governance/taint/
        // compensation/MCP/protocol hosts + remember + recall.
        assert_eq!(crate::register(), 29);
        // The two regression goals below must be distinct from every other
        // fixture goal so the process-wide store stays isolated.
        let spec = format!(
            r#"{{"goal":"memory-host-test","model":"mock/echo","endpoint":"mock:","journal":""}}"#
        );
        let (status, started) = call(
            "spectra.std.agent.agent_start",
            &[unsafe { abi::alloc_string(&spec) }],
        );
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_eq!(unsafe { *(started as *const i64) }, 0, "start must succeed");
        let run = tagged_payload(started);

        for text in ["alpha", "beta"] {
            let (status, stored) = call(
                crate::REMEMBER_HOST_CALL,
                &[run, unsafe { abi::alloc_string(text) }],
            );
            assert_eq!(status, HOST_STATUS_SUCCESS);
            assert_eq!(unsafe { *(stored as *const i64) }, 0, "remember must succeed");
            assert_eq!(tagged_payload(stored), 1);
        }

        let query = unsafe { abi::alloc_string("alpha") };
        let (status, recalled) = call(crate::RECALL_HOST_CALL, &[run, query, 2]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_eq!(unsafe { *(recalled as *const i64) }, 0, "recall must succeed");
        let payload = abi::read_string_arg(tagged_payload(recalled)).expect("payload string");
        assert!(payload.contains("\"schema\":\"spectra.agent.memory_recall.v1\""), "{payload}");
        assert!(payload.contains("\"scope\":\"memory-host-test\""), "{payload}");
        assert!(payload.contains("\"origin\":\"agent\""), "{payload}");
        assert!(payload.contains("\"run\":"), "{payload}");
        assert!(payload.contains("alpha") && payload.contains("beta"), "{payload}");

        // A missing run is a typed error, never a trap.
        let (status, missing) = call(crate::RECALL_HOST_CALL, &[9999, query, 2]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_eq!(unsafe { *(missing as *const i64) }, 1);
        // top_k below one is rejected before any embedding work.
        let (status, bad_top_k) = call(crate::RECALL_HOST_CALL, &[run, query, 0]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_eq!(unsafe { *(bad_top_k as *const i64) }, 1);
        call("spectra.std.agent.agent_end", &[run]);
    }
}
