//! Deterministic HNSW vector index used by `std.ml.vector_index_*`.
//!
//! Inserts are incremental: each vector is attached with greedy descent through
//! the upper layers followed by ef-construction searches, heuristic neighbor
//! selection, and degree-capped bidirectional links. Artifacts saved as `v2`
//! reserve `M0` link slots per node at layer 0; only `v2` payloads load.

use crate::artifact::{ArtifactData, TensorPayload};
use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::time::Instant;

const INDEX_VERSION: &str = "v2";
const M: usize = 16;
const M0: usize = 2 * M;
const EF_CONSTRUCTION: usize = 200;
const EF_SEARCH: usize = 64;
const MAX_LEVEL: usize = 8;

#[derive(Debug, Clone)]
pub(crate) struct VectorEntry {
    pub id: String,
    pub vector: Vec<f64>,
    pub level: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct QueryResult {
    pub id: String,
    pub score: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct QueryEvidence {
    pub results: Vec<QueryResult>,
    pub visited_nodes: usize,
    pub latency_us: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct VectorIndexMetrics {
    pub insert_count: u64,
    pub query_count: u64,
    pub total_insert_ns: u128,
    pub total_query_ns: u128,
}

#[derive(Debug, Clone)]
pub(crate) struct VectorIndex {
    pub dimension: usize,
    pub metadata: BTreeMap<String, String>,
    pub entries: Vec<VectorEntry>,
    pub links: Vec<Vec<Vec<usize>>>,
    pub entry_point: usize,
    pub max_level: usize,
    pub metrics: VectorIndexMetrics,
}

#[derive(Debug)]
pub(crate) enum VectorIndexError {
    Invalid(String),
}

impl std::fmt::Display for VectorIndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => write!(f, "invalid vector index: {message}"),
        }
    }
}

fn invalid(message: impl Into<String>) -> VectorIndexError {
    VectorIndexError::Invalid(message.into())
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = value;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

fn deterministic_level(ordinal: usize) -> usize {
    let mut state = splitmix64(ordinal as u64);
    let mut level = 0;
    while level < MAX_LEVEL && state & 0x3 == 0 {
        level += 1;
        state = splitmix64(state);
    }
    level
}

fn normalize(vector: &[f64], dimension: usize) -> Result<Vec<f64>, VectorIndexError> {
    if vector.len() != dimension
        || vector.is_empty()
        || vector.iter().any(|value| !value.is_finite())
    {
        return Err(invalid("vector must be finite and match index dimension"));
    }
    let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    if !norm.is_finite() || norm <= f64::EPSILON {
        return Err(invalid("zero-norm vectors are not supported"));
    }
    Ok(vector.iter().map(|value| value / norm).collect())
}

/// Heap item for ef-construction searches. Total order over `(score, node)`
/// keeps heap pop order deterministic for a given insertion sequence.
#[derive(Clone)]
struct SearchCandidate {
    score: f64,
    node: usize,
}

impl PartialEq for SearchCandidate {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for SearchCandidate {}

impl Ord for SearchCandidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| self.node.cmp(&other.node))
    }
}

impl PartialOrd for SearchCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn score(left: &[f64], right: &[f64]) -> f64 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}
/// Maximum degree per layer: `M0` at layer 0 (wider fan-out where recall
/// matters most), `M` above — the standard HNSW split.
fn neighbor_cap(layer: usize) -> usize {
    if layer == 0 {
        M0
    } else {
        M
    }
}

impl VectorIndex {
    pub(crate) fn new(dimension: usize) -> Result<Self, VectorIndexError> {
        if dimension == 0 {
            return Err(invalid("dimension must be positive"));
        }
        let mut metadata = BTreeMap::new();
        metadata.insert("artifact_role".to_owned(), "vector_index".to_owned());
        metadata.insert("index_type".to_owned(), "hnsw".to_owned());
        metadata.insert("index_version".to_owned(), INDEX_VERSION.to_owned());
        metadata.insert("m0".to_owned(), M0.to_string());
        metadata.insert("metric".to_owned(), "cosine".to_owned());
        metadata.insert("dtype".to_owned(), "f64".to_owned());
        metadata.insert("m".to_owned(), M.to_string());
        metadata.insert("ef_construction".to_owned(), EF_CONSTRUCTION.to_string());
        metadata.insert("ef_search".to_owned(), EF_SEARCH.to_string());
        metadata.insert("seed".to_owned(), "0".to_owned());
        Ok(Self {
            dimension,
            metadata,
            entries: Vec::new(),
            links: Vec::new(),
            entry_point: 0,
            max_level: 0,
            metrics: VectorIndexMetrics {
                insert_count: 0,
                query_count: 0,
                total_insert_ns: 0,
                total_query_ns: 0,
            },
        })
    }

    pub(crate) fn set_metadata(&mut self, key: &str, value: &str) -> bool {
        if value.is_empty() || !matches!(key, "model_version" | "model_name") {
            return false;
        }
        self.metadata.insert(key.to_owned(), value.to_owned());
        true
    }

    pub(crate) fn insert(&mut self, id: String, vector: &[f64]) -> Result<usize, VectorIndexError> {
        if id.is_empty() {
            return Err(invalid("id must not be empty"));
        }
        let started = Instant::now();
        let vector = normalize(vector, self.dimension)?;
        match self.entries.iter().position(|entry| entry.id == id) {
            Some(position) => self.update_entry(position, vector),
            None => {
                let ordinal = self.entries.len();
                let level = deterministic_level(ordinal);
                // Capture the descent root and the pre-insert ceiling BEFORE
                // promotion: the node attaches only up to the previous
                // max_level (standard HNSW), and never starts from itself.
                let root = if ordinal == 0 { 0 } else { self.entry_point };
                let prev_max_level = self.max_level;
                self.entries.push(VectorEntry { id, vector, level });
                self.links.push(vec![Vec::new(); level + 1]);
                if ordinal == 0 || level > self.max_level {
                    self.max_level = level;
                    self.entry_point = ordinal;
                }
                self.attach(ordinal, root, prev_max_level);
            }
        }
        self.metrics.insert_count = self.metrics.insert_count.saturating_add(1);
        self.metrics.total_insert_ns = self
            .metrics
            .total_insert_ns
            .saturating_add(started.elapsed().as_nanos());
        Ok(self.entries.len())
    }

    /// Incremental HNSW insertion: greedy descent from `root` (an existing
    /// node, never the one being attached) through the layers above the node's
    /// level, then an ef-construction search and heuristic neighbor selection
    /// per layer up to `layer_ceiling` (the max level before this insert) with
    /// bidirectional links pruned back to the layer capacity. Deterministic
    /// for a given insertion sequence: levels derive from the ordinal and
    /// every tie-break compares scores (then node ordinals) with total order.
    fn attach(&mut self, index: usize, root: usize, layer_ceiling: usize) {
        if self.entries.len() == 1 {
            return;
        }
        let query = self.entries[index].vector.clone();
        let top = self.entries[index].level.min(layer_ceiling);
        let mut entry = root.min(self.entries.len() - 1);
        if entry == index {
            entry = if index == 0 { 1 } else { 0 };
        }
        if top < self.max_level {
            entry = self.greedy_descend(&query, entry, self.max_level, top + 1);
        }
        for layer in (0..=top).rev() {
            let candidates = self
                .search_layer(&query, entry, EF_CONSTRUCTION, layer)
                .into_iter()
                .filter(|candidate| *candidate != index && self.entries[*candidate].level >= layer)
                .collect::<Vec<_>>();
            if candidates.is_empty() {
                continue;
            }
            let capacity = neighbor_cap(layer);
            for chosen in self.select_neighbors(&query, &candidates, capacity) {
                self.link(index, chosen, layer, capacity);
            }
            entry = candidates[0];
        }
    }

    /// Re-links a vector updated in place under an existing id: clears the
    /// node's adjacency, attaches it via the incremental path, then re-prunes
    /// its former neighbors. This keeps id updates sub-quadratic without a
    /// full graph rebuild.
    fn update_entry(&mut self, position: usize, vector: Vec<f64>) {
        let stale = self.links[position]
            .iter()
            .map(Vec::clone)
            .collect::<Vec<_>>();
        self.entries[position].vector = vector;
        let level = self.entries[position].level;
        self.links[position] = vec![Vec::new(); level + 1];
        // If the updated node was the sole descent root, its cleared
        // adjacency would strand attach(); move the entry point to a
        // deterministic fallback (first stale neighbor, else lowest ordinal).
        if position == self.entry_point && self.entries.len() > 1 {
            self.entry_point = stale
                .iter()
                .flatten()
                .copied()
                .find(|candidate| *candidate != position)
                .unwrap_or(
                    (0..self.entries.len())
                        .find(|other| *other != position)
                        .unwrap_or(0),
                );
        }
        self.attach(position, self.entry_point, self.max_level);
        for (layer, neighbors) in stale.into_iter().enumerate() {
            let capacity = neighbor_cap(layer);
            for neighbor in neighbors {
                self.prune(neighbor, layer, capacity);
            }
        }
    }

    fn greedy_descend(&self, query: &[f64], mut current: usize, from: usize, to: usize) -> usize {
        if to > from {
            return current;
        }
        for layer in (to..=from).rev() {
            let mut changed = true;
            while changed {
                changed = false;
                let current_score = score(query, &self.entries[current].vector);
                for neighbor in self.links[current].get(layer).into_iter().flatten() {
                    let neighbor_score = score(query, &self.entries[*neighbor].vector);
                    if neighbor_score > current_score
                        || (neighbor_score == current_score
                            && self.entries[*neighbor].id < self.entries[current].id)
                    {
                        current = *neighbor;
                        changed = true;
                    }
                }
            }
        }
        current
    }

    /// Best-first beam search restricted to one layer, returning up to `ef`
    /// nodes sorted by descending similarity to `query`.
    fn search_layer(&self, query: &[f64], entry: usize, ef: usize, layer: usize) -> Vec<usize> {
        let entry_score = score(query, &self.entries[entry].vector);
        let mut visited = HashSet::new();
        visited.insert(entry);
        let mut frontier = BinaryHeap::from([SearchCandidate {
            score: entry_score,
            node: entry,
        }]);
        let mut results = BinaryHeap::from([Reverse(SearchCandidate {
            score: entry_score,
            node: entry,
        })]);
        while let Some(candidate) = frontier.pop() {
            if results.len() >= ef
                && candidate.score
                    < results
                        .peek()
                        .map(|worst| worst.0.score)
                        .unwrap_or(candidate.score)
            {
                break;
            }
            for neighbor in self.links[candidate.node].get(layer).into_iter().flatten() {
                if visited.insert(*neighbor) {
                    let candidate_score = score(query, &self.entries[*neighbor].vector);
                    let worse = results.len() >= ef
                        && candidate_score
                            <= results
                                .peek()
                                .map(|worst| worst.0.score)
                                .unwrap_or(f64::NEG_INFINITY);
                    if !worse {
                        let next = SearchCandidate {
                            score: candidate_score,
                            node: *neighbor,
                        };
                        frontier.push(next.clone());
                        results.push(Reverse(next));
                        if results.len() > ef {
                            results.pop();
                        }
                    }
                }
            }
        }
        let mut found = results
            .into_iter()
            .map(|wrapped| wrapped.0.node)
            .collect::<Vec<_>>();
        found.sort_by(|left, right| {
            score(query, &self.entries[*right].vector)
                .total_cmp(&score(query, &self.entries[*left].vector))
                .then_with(|| right.cmp(left))
        });
        found
    }

    /// Heuristic neighbor selection (HNSW algorithm 4): keep a candidate when
    /// it is closer to the query than to every already-selected neighbor; fill
    /// any remaining slots with the strongest leftovers so nodes retain full
    /// degree on sparse regions.
    fn select_neighbors(&self, query: &[f64], candidates: &[usize], m: usize) -> Vec<usize> {
        let mut ordered = candidates.to_vec();
        ordered.sort_by(|left, right| {
            score(query, &self.entries[*right].vector)
                .total_cmp(&score(query, &self.entries[*left].vector))
                .then_with(|| left.cmp(right))
        });
        let mut selected: Vec<usize> = Vec::with_capacity(m.min(ordered.len()));
        let mut deferred = Vec::new();
        for candidate in ordered {
            if selected.len() < m {
                let candidate_score = score(query, &self.entries[candidate].vector);
                let diverse = selected.iter().all(|chosen| {
                    score(
                        &self.entries[candidate].vector,
                        &self.entries[*chosen].vector,
                    ) < candidate_score
                });
                if diverse {
                    selected.push(candidate);
                    continue;
                }
            }
            deferred.push(candidate);
        }
        for candidate in deferred {
            if selected.len() >= m {
                break;
            }
            selected.push(candidate);
        }
        selected.sort_unstable();
        selected
    }

    fn link(&mut self, left: usize, right: usize, layer: usize, capacity: usize) {
        if !self.links[left][layer].contains(&right) {
            self.links[left][layer].push(right);
        }
        if !self.links[right][layer].contains(&left) {
            self.links[right][layer].push(left);
        }
        self.prune(left, layer, capacity);
        self.prune(right, layer, capacity);
    }

    fn prune(&mut self, node: usize, layer: usize, capacity: usize) {
        if self.links[node][layer].len() <= capacity {
            return;
        }
        let reference = self.entries[node].vector.clone();
        let candidates = std::mem::take(&mut self.links[node][layer]);
        self.links[node][layer] = self.select_neighbors(&reference, &candidates, capacity);
    }

    pub(crate) fn query(
        &mut self,
        vector: &[f64],
        top_k: usize,
    ) -> Result<QueryEvidence, VectorIndexError> {
        if top_k == 0 || self.entries.is_empty() {
            return Err(invalid(
                "query requires a non-empty index and positive top_k",
            ));
        }
        let started = Instant::now();
        let query = normalize(vector, self.dimension)?;
        let current = self.greedy_descend(&query, self.entry_point, self.max_level, 1);
        let mut visited = HashSet::new();
        let mut frontier = vec![current];
        let mut scored = Vec::new();
        while !frontier.is_empty() && visited.len() < EF_SEARCH {
            frontier.sort_unstable_by(|left, right| {
                score(&query, &self.entries[*right].vector)
                    .total_cmp(&score(&query, &self.entries[*left].vector))
            });
            let node = frontier.remove(0);
            if !visited.insert(node) {
                continue;
            }
            scored.push((node, score(&query, &self.entries[node].vector)));
            for neighbor in &self.links[node][0] {
                if !visited.contains(neighbor) {
                    frontier.push(*neighbor);
                }
            }
        }
        scored.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| self.entries[left.0].id.cmp(&self.entries[right.0].id))
        });
        let results = scored
            .into_iter()
            .take(top_k)
            .map(|(index, value)| QueryResult {
                id: self.entries[index].id.clone(),
                score: value,
            })
            .collect();
        self.metrics.query_count = self.metrics.query_count.saturating_add(1);
        self.metrics.total_query_ns = self
            .metrics
            .total_query_ns
            .saturating_add(started.elapsed().as_nanos());
        Ok(QueryEvidence {
            results,
            visited_nodes: visited.len(),
            latency_us: started.elapsed().as_micros() as u64,
        })
    }

    pub(crate) fn artifact_data(&self) -> Result<ArtifactData, VectorIndexError> {
        let model_version = self
            .metadata
            .get("model_version")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| invalid("model_version metadata is required"))?;
        if self.entries.is_empty() {
            return Err(invalid("cannot persist an empty index"));
        }
        let layers = self.max_level + 1;
        let mut ids = Vec::new();
        let mut vectors = Vec::new();
        let mut levels = Vec::new();
        let mut links = Vec::new();
        for entry in &self.entries {
            ids.push(entry.id.clone());
            vectors.extend(entry.vector.iter().flat_map(|value| value.to_le_bytes()));
            levels.extend_from_slice(&(entry.level as i64).to_le_bytes());
            for layer in 0..layers {
                for slot in 0..M0 {
                    let value = self
                        .links
                        .get(ids.len() - 1)
                        .and_then(|levels| levels.get(layer))
                        .and_then(|neighbors| neighbors.get(slot))
                        .copied()
                        .map(|value| value as i64)
                        .unwrap_or(-1);
                    links.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        let mut metadata = self.metadata.clone();
        metadata.insert("dimension".to_owned(), self.dimension.to_string());
        metadata.insert("entry_count".to_owned(), self.entries.len().to_string());
        metadata.insert("max_level".to_owned(), self.max_level.to_string());
        metadata.insert("entry_point".to_owned(), self.entry_point.to_string());
        metadata.insert(
            "ids_json".to_owned(),
            serde_json::to_string(&ids).map_err(|_| invalid("unable to encode ids"))?,
        );
        Ok(ArtifactData {
            name: metadata
                .get("model_name")
                .cloned()
                .unwrap_or_else(|| "spectra-vector-index".to_owned()),
            model_version: model_version.clone(),
            kind: "multi_array".to_owned(),
            metadata,
            tensors: vec![
                TensorPayload {
                    name: "vectors".to_owned(),
                    dtype: "float".to_owned(),
                    precision: "f64".to_owned(),
                    shape: vec![self.entries.len(), self.dimension],
                    layout: "contiguous".to_owned(),
                    bytes: vectors,
                },
                TensorPayload {
                    name: "levels".to_owned(),
                    dtype: "int".to_owned(),
                    precision: "f64".to_owned(),
                    shape: vec![self.entries.len()],
                    layout: "contiguous".to_owned(),
                    bytes: levels,
                },
                TensorPayload {
                    name: "links".to_owned(),
                    dtype: "int".to_owned(),
                    precision: "f64".to_owned(),
                    shape: vec![self.entries.len(), layers, M0],
                    layout: "contiguous".to_owned(),
                    bytes: links,
                },
            ],
        })
    }

    pub(crate) fn from_artifact(data: &ArtifactData) -> Result<Self, VectorIndexError> {
        let metadata = &data.metadata;
        let index_version = metadata
            .get("index_version")
            .map(String::as_str)
            .unwrap_or_default();
        // Only v2 payloads load: v2 reserves M0 link slots at layer 0 for
        // the wider fan-out used by incremental inserts.
        if index_version != INDEX_VERSION {
            return Err(invalid("metadata index_version is incompatible"));
        }
        for (key, expected) in [
            ("artifact_role", "vector_index"),
            ("index_type", "hnsw"),
            ("metric", "cosine"),
            ("dtype", "f64"),
            ("m", "16"),
            ("ef_construction", "200"),
            ("ef_search", "64"),
            ("seed", "0"),
        ] {
            if metadata.get(key).map(String::as_str) != Some(expected) {
                return Err(invalid(format!("metadata {key} is incompatible")));
            }
        }
        if metadata.get("m0") != Some(&M0.to_string()) {
            return Err(invalid("metadata m0 is incompatible"));
        }
        let link_slots = M0;
        let dimension = metadata
            .get("dimension")
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| invalid("invalid dimension"))?;
        let entry_count = metadata
            .get("entry_count")
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| invalid("invalid entry count"))?;
        let max_level = metadata
            .get("max_level")
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value <= MAX_LEVEL)
            .ok_or_else(|| invalid("invalid max level"))?;
        let entry_point = metadata
            .get("entry_point")
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value < entry_count)
            .ok_or_else(|| invalid("invalid entry point"))?;
        if data.kind != "multi_array"
            || data.model_version.is_empty()
            || metadata.get("model_version") != Some(&data.model_version)
        {
            return Err(invalid("incompatible artifact kind or model version"));
        }
        let arrays = data
            .tensors
            .iter()
            .map(|tensor| (tensor.name.as_str(), tensor))
            .collect::<HashMap<_, _>>();
        if arrays.len() != 3
            || !arrays.contains_key("vectors")
            || !arrays.contains_key("levels")
            || !arrays.contains_key("links")
        {
            return Err(invalid("vector index arrays are incomplete"));
        }
        let vectors = arrays["vectors"];
        let levels = arrays["levels"];
        let links = arrays["links"];
        if vectors.dtype != "float"
            || levels.dtype != "int"
            || links.dtype != "int"
            || vectors.shape != vec![entry_count, dimension]
            || levels.shape != vec![entry_count]
            || links.shape != vec![entry_count, max_level + 1, link_slots]
        {
            return Err(invalid(
                "vector index array shapes or dtypes are incompatible",
            ));
        }
        let ids = metadata
            .get("ids_json")
            .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
            .filter(|ids| ids.len() == entry_count && ids.iter().all(|id| !id.is_empty()))
            .ok_or_else(|| invalid("invalid ids metadata"))?;
        let mut unique = HashSet::new();
        if ids.iter().any(|id| !unique.insert(id)) {
            return Err(invalid("duplicate vector id"));
        }
        let vector_values = vectors
            .bytes
            .chunks_exact(8)
            .map(|chunk| f64::from_le_bytes(chunk.try_into().expect("validated f64 width")))
            .collect::<Vec<_>>();
        let level_values = levels
            .bytes
            .chunks_exact(8)
            .map(|chunk| i64::from_le_bytes(chunk.try_into().expect("validated i64 width")))
            .collect::<Vec<_>>();
        if vector_values.len() != entry_count * dimension
            || level_values.len() != entry_count
            || level_values
                .iter()
                .any(|level| *level < 0 || *level as usize > max_level)
        {
            return Err(invalid("invalid vector index payload"));
        }
        if vector_values
            .chunks_exact(dimension)
            .any(|row| normalize(row, dimension).is_err())
        {
            return Err(invalid("invalid vector values"));
        }
        let mut links_values = links
            .bytes
            .chunks_exact(8)
            .map(|chunk| i64::from_le_bytes(chunk.try_into().expect("validated i64 width")));
        let mut graph = vec![vec![Vec::new(); max_level + 1]; entry_count];
        for node in 0..entry_count {
            for (layer, neighbors) in graph[node].iter_mut().enumerate().take(max_level + 1) {
                let capacity = if layer == 0 { M0 } else { M };
                for _slot in 0..link_slots {
                    let value = links_values
                        .next()
                        .ok_or_else(|| invalid("truncated HNSW links"))?;
                    if value >= 0 {
                        let neighbor = value as usize;
                        if neighbor >= entry_count
                            || neighbor == node
                            || level_values[node] < layer as i64
                            || level_values[neighbor] < layer as i64
                        {
                            return Err(invalid("invalid HNSW link"));
                        }
                        neighbors.push(neighbor);
                    }
                }
                neighbors.sort_unstable();
                neighbors.dedup();
                if neighbors.len() > capacity {
                    return Err(invalid("HNSW degree exceeds layer capacity"));
                }
            }
        }
        if links_values.next().is_some() {
            return Err(invalid("excess HNSW links"));
        }
        let entries = ids
            .into_iter()
            .enumerate()
            .map(|(index, id)| VectorEntry {
                id,
                vector: vector_values[index * dimension..(index + 1) * dimension].to_vec(),
                level: level_values[index] as usize,
            })
            .collect::<Vec<_>>();
        let metadata = metadata.clone();
        Ok(Self {
            dimension,
            metadata,
            entries,
            links: graph,
            entry_point,
            max_level,
            metrics: VectorIndexMetrics {
                insert_count: 0,
                query_count: 0,
                total_insert_ns: 0,
                total_query_ns: 0,
            },
        })
    }

    pub(crate) fn metrics(&self) -> &VectorIndexMetrics {
        &self.metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hnsw_insert_query_update_is_deterministic() {
        let mut left = VectorIndex::new(3).unwrap();
        let mut right = VectorIndex::new(3).unwrap();
        for (id, vector) in [
            ("a", vec![1.0, 0.0, 0.0]),
            ("b", vec![0.0, 1.0, 0.0]),
            ("c", vec![0.0, 0.0, 1.0]),
        ] {
            left.insert(id.to_owned(), &vector).unwrap();
            right.insert(id.to_owned(), &vector).unwrap();
        }
        let left_result = left.query(&[1.0, 0.0, 0.0], 2).unwrap();
        let right_result = right.query(&[1.0, 0.0, 0.0], 2).unwrap();
        assert_eq!(
            left_result
                .results
                .iter()
                .map(|result| &result.id)
                .collect::<Vec<_>>(),
            right_result
                .results
                .iter()
                .map(|result| &result.id)
                .collect::<Vec<_>>()
        );
        left.insert("b".to_owned(), &[1.0, 0.0, 0.0]).unwrap();
        assert_eq!(left.query(&[1.0, 0.0, 0.0], 1).unwrap().results[0].id, "a");
    }

    #[test]
    fn rejects_invalid_vectors_and_missing_model_metadata() {
        let mut index = VectorIndex::new(2).unwrap();
        assert!(index.insert("zero".to_owned(), &[0.0, 0.0]).is_err());
        assert!(index.insert("nan".to_owned(), &[f64::NAN, 1.0]).is_err());
        index.insert("a".to_owned(), &[1.0, 0.0]).unwrap();
        assert!(index.artifact_data().is_err());
        assert!(index.set_metadata("model_version", "v1"));
        assert!(index.artifact_data().is_ok());
    }

    fn sample_vectors(dimension: usize, count: usize) -> Vec<Vec<f64>> {
        let mut state = 0x5eed_1234_9e37_79b9_u64;
        (0..count)
            .map(|_| {
                let raw = (0..dimension)
                    .map(|_| {
                        state = splitmix64(state);
                        ((state >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
                    })
                    .collect::<Vec<_>>();
                normalize(&raw, dimension).unwrap()
            })
            .collect()
    }

    #[test]
    fn incremental_insert_recall_at_10_beats_bruteforce_threshold() {
        let dimension = 8;
        let count = 2000;
        let vectors = sample_vectors(dimension, count);
        let mut index = VectorIndex::new(dimension).unwrap();
        for (ordinal, vector) in vectors.iter().enumerate() {
            index.insert(format!("v{ordinal:05}"), vector).unwrap();
        }
        let queries = vectors.iter().step_by(20).take(100);
        let mut total_recall = 0.0;
        for query in queries {
            let evidence = index.query(query, 10).unwrap();
            let mut brute = (0..count)
                .map(|candidate| (score(query, &vectors[candidate]), candidate))
                .collect::<Vec<_>>();
            brute.sort_by(|left, right| {
                right
                    .0
                    .total_cmp(&left.0)
                    .then_with(|| left.1.cmp(&right.1))
            });
            let truth = brute
                .into_iter()
                .take(10)
                .map(|(_, candidate)| candidate)
                .collect::<HashSet<_>>();
            let hits = evidence
                .results
                .iter()
                .filter_map(|result| result.id.trim_start_matches('v').parse::<usize>().ok())
                .filter(|candidate| truth.contains(candidate))
                .count();
            total_recall += hits as f64 / truth.len() as f64;
        }
        let average_recall = total_recall / 100.0;
        println!(
            "recall@10 over {count} vectors, M={M}, M0={M0}, ef_construction={EF_CONSTRUCTION}, ef_search={EF_SEARCH}: {average_recall:.4}"
        );
        assert!(
            average_recall >= 0.9,
            "recall@10 {average_recall} below 0.9"
        );
    }

    #[test]
    fn incremental_index_matches_rebuild_from_persisted_format() {
        let dimension = 12;
        let vectors = sample_vectors(dimension, 400);
        let mut index = VectorIndex::new(dimension).unwrap();
        assert!(index.set_metadata("model_version", "round-trip"));
        for (ordinal, vector) in vectors.iter().enumerate() {
            index.insert(format!("v{ordinal:05}"), vector).unwrap();
        }
        let artifact = index.artifact_data().unwrap();
        assert_eq!(
            artifact.metadata.get("index_version").map(String::as_str),
            Some("v2")
        );
        let mut rebuilt = VectorIndex::from_artifact(&artifact).unwrap();
        assert_eq!(rebuilt.entry_point, index.entry_point);
        assert_eq!(rebuilt.max_level, index.max_level);
        // The live graph keeps level+1 adjacency rows per node; the loader
        // normalizes every node to max_level+1 padded rows, so compare only
        // the meaningful rows.
        for (live, loaded) in index.links.iter().zip(&rebuilt.links) {
            assert_eq!(loaded[..live.len()], live[..]);
        }
        for query in vectors.iter().step_by(37).take(25) {
            let live = index.query(query, 10).unwrap();
            let loaded = rebuilt.query(query, 10).unwrap();
            assert_eq!(live.results.len(), loaded.results.len());
            for (left, right) in live.results.iter().zip(&loaded.results) {
                assert_eq!(left.id, right.id);
                assert_eq!(
                    left.score.total_cmp(&right.score),
                    std::cmp::Ordering::Equal
                );
            }
        }
    }
    #[test]
    fn legacy_v1_artifacts_are_rejected() {
        let dimension = 3;
        let mut index = VectorIndex::new(dimension).unwrap();
        assert!(index.set_metadata("model_version", "legacy"));
        for (id, vector) in [
            ("a", vec![1.0, 0.0, 0.0]),
            ("b", vec![0.0, 1.0, 0.0]),
            ("c", vec![0.0, 0.0, 1.0]),
        ] {
            index.insert(id.to_owned(), &vector).unwrap();
        }
        let v2_artifact = index.artifact_data().unwrap();
        // A v1-marked payload (no m0, M link slots) is rejected instead of
        // being upgraded in memory.
        let mut metadata = v2_artifact.metadata.clone();
        metadata.insert("index_version".to_owned(), "v1".to_owned());
        metadata.remove("m0");
        let legacy = ArtifactData {
            name: v2_artifact.name.clone(),
            model_version: v2_artifact.model_version.clone(),
            kind: v2_artifact.kind.clone(),
            metadata,
            tensors: v2_artifact.tensors.clone(),
        };
        assert!(VectorIndex::from_artifact(&legacy).is_err());
    }
}
