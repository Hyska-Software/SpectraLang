//! Deterministic HNSW vector index used by `std.ml.vector_index_*`.

use crate::artifact::{ArtifactData, TensorPayload};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

const INDEX_VERSION: &str = "v1";
const M: usize = 16;
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

fn score(left: &[f64], right: &[f64]) -> f64 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
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
        if let Some(existing) = self.entries.iter_mut().find(|entry| entry.id == id) {
            existing.vector = vector;
        } else {
            let level = deterministic_level(self.entries.len());
            self.entries.push(VectorEntry { id, vector, level });
        }
        self.rebuild_graph();
        self.metrics.insert_count = self.metrics.insert_count.saturating_add(1);
        self.metrics.total_insert_ns = self
            .metrics
            .total_insert_ns
            .saturating_add(started.elapsed().as_nanos());
        Ok(self.entries.len())
    }

    fn rebuild_graph(&mut self) {
        self.max_level = self
            .entries
            .iter()
            .map(|entry| entry.level)
            .max()
            .unwrap_or(0);
        self.entry_point = self
            .entries
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| {
                left.level
                    .cmp(&right.level)
                    .then_with(|| right.id.cmp(&left.id))
            })
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.links = self
            .entries
            .iter()
            .map(|entry| vec![Vec::new(); entry.level + 1])
            .collect();
        for index in 0..self.entries.len() {
            let level = self.entries[index].level;
            for layer in 0..=level {
                let mut candidates = (0..index)
                    .filter(|candidate| self.entries[*candidate].level >= layer)
                    .map(|candidate| {
                        (
                            candidate,
                            score(&self.entries[index].vector, &self.entries[candidate].vector),
                        )
                    })
                    .collect::<Vec<_>>();
                candidates.sort_by(|left, right| {
                    right
                        .1
                        .total_cmp(&left.1)
                        .then_with(|| self.entries[left.0].id.cmp(&self.entries[right.0].id))
                });
                for (candidate, _) in candidates.into_iter().take(M) {
                    self.links[index][layer].push(candidate);
                    self.links[candidate][layer].push(index);
                    self.links[candidate][layer].sort_unstable_by(|left, right| {
                        score(
                            &self.entries[candidate].vector,
                            &self.entries[*right].vector,
                        )
                        .total_cmp(&score(
                            &self.entries[candidate].vector,
                            &self.entries[*left].vector,
                        ))
                        .then_with(|| self.entries[*left].id.cmp(&self.entries[*right].id))
                    });
                    self.links[candidate][layer].truncate(M);
                }
                self.links[index][layer].sort_unstable();
                self.links[index][layer].dedup();
                self.links[index][layer].truncate(M);
            }
        }
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
        let mut current = self.entry_point;
        for layer in (1..=self.max_level).rev() {
            let mut changed = true;
            while changed {
                changed = false;
                let current_score = score(&query, &self.entries[current].vector);
                for neighbor in self
                    .links
                    .get(current)
                    .and_then(|levels| levels.get(layer))
                    .into_iter()
                    .flatten()
                {
                    let neighbor_score = score(&query, &self.entries[*neighbor].vector);
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
                for slot in 0..M {
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
                    shape: vec![self.entries.len(), layers, M],
                    layout: "contiguous".to_owned(),
                    bytes: links,
                },
            ],
        })
    }

    pub(crate) fn from_artifact(data: &ArtifactData) -> Result<Self, VectorIndexError> {
        let metadata = &data.metadata;
        for (key, expected) in [
            ("artifact_role", "vector_index"),
            ("index_type", "hnsw"),
            ("index_version", INDEX_VERSION),
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
            || links.shape != vec![entry_count, max_level + 1, M]
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
                for _slot in 0..M {
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
                if neighbors.len() > M {
                    return Err(invalid("HNSW degree exceeds M"));
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
        Ok(Self {
            dimension,
            metadata: metadata.clone(),
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
}
