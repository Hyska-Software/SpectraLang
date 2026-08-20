// ── std.ml runtime ──────────────────────────────────────────────────────────

#[derive(Default)]
struct MlModule {
    parameters: Vec<usize>,
    training: bool,
}

#[derive(Clone, Copy)]
struct MlDataset {
    features: usize,
    labels: usize,
    len: usize,
}

#[derive(Clone, Copy)]
struct MlDataLoader {
    dataset: usize,
    batch_size: usize,
    shuffle_seed: u64,
}

#[derive(Clone)]
struct MlDataFrame {
    rows: usize,
    cols: usize,
    data: Vec<f64>,
}

#[derive(Clone)]
struct MlMetricRecord {
    name: String,
    value: f64,
    step: i64,
}

#[derive(Clone)]
struct MlArtifactRecord {
    path: String,
    size: u64,
    fnv64: String,
}

#[derive(Clone)]
struct MlExperiment {
    name: String,
    out_dir: String,
    seed: i64,
    configs: Vec<(String, String)>,
    metrics: Vec<MlMetricRecord>,
    artifacts: Vec<MlArtifactRecord>,
    lockfile: Option<MlArtifactRecord>,
    model_output: Option<MlArtifactRecord>,
    manifest_path: String,
    reproduction_command: String,
    finished: bool,
}

#[derive(Clone)]
struct MlDistributedWorker {
    worker_id: usize,
    step_count: i64,
    sample_count: i64,
    accumulator: f64,
    active: bool,
}

#[derive(Clone)]
struct MlDistributedSession {
    name: String,
    out_dir: String,
    worker_count: usize,
    seed: i64,
    global_step: i64,
    interrupted_worker: Option<usize>,
    workers: Vec<MlDistributedWorker>,
    last_checkpoint_path: Option<String>,
}

#[derive(Clone)]
struct MlKvCache {
    max_tokens: usize,
    dim: usize,
    keys: Vec<f64>,
    values: Vec<f64>,
}

#[derive(Clone)]
struct MlWordpieceTokenizer {
    token_to_id: HashMap<String, i64>,
    id_to_token: HashMap<i64, String>,
    unk_id: i64,
    max_token_chars: usize,
    special_tokens: HashMap<String, i64>,
    lowercase: bool,
    continuation_prefix: String,
    strict_ids: bool,
}

#[derive(Clone)]
struct MlArtifact {
    name: String,
    model_version: String,
    kind: String,
    metadata: BTreeMap<String, String>,
    tensors: HashMap<String, usize>,
}

/// ML resources use the same generational handle encoding as core collections
/// and time values. This adapter keeps the existing `Option`-based call sites
/// small while removing the old process-global `next_id` namespace.
struct MlHandleTable<T> {
    table: HandleTable<T>,
}

impl<T> MlHandleTable<T> {
    fn new(kind: HandleKind) -> Self {
        Self {
            table: HandleTable::new(kind),
        }
    }

    fn insert(&mut self, value: T) -> usize {
        self.table.insert(value).raw() as usize
    }

    fn id(&self, raw: &usize) -> Option<HandleId> {
        HandleId::from_raw(*raw as i64).ok()
    }

    fn get(&self, raw: &usize) -> Option<&T> {
        self.id(raw).and_then(|id| self.table.get(id).ok())
    }

    fn get_mut(&mut self, raw: &usize) -> Option<&mut T> {
        let id = self.id(raw)?;
        self.table.get_mut(id).ok()
    }

    fn remove(&mut self, raw: &usize) -> Option<T> {
        let id = self.id(raw)?;
        self.table.remove(id).ok()
    }

    fn contains_key(&self, raw: &usize) -> bool {
        self.get(raw).is_some()
    }
}

struct MlRegistry {
    modules: MlHandleTable<MlModule>,
    datasets: MlHandleTable<MlDataset>,
    loaders: MlHandleTable<MlDataLoader>,
    dataframes: MlHandleTable<MlDataFrame>,
    experiments: MlHandleTable<MlExperiment>,
    distributed_sessions: MlHandleTable<MlDistributedSession>,
    kv_caches: MlHandleTable<MlKvCache>,
    tokenizers: MlHandleTable<MlWordpieceTokenizer>,
    vector_indexes: MlHandleTable<crate::vector_index::VectorIndex>,
    artifacts: MlHandleTable<MlArtifact>,
}

impl MlRegistry {
    fn new() -> Self {
        Self {
            modules: MlHandleTable::new(HandleKind::MlModule),
            datasets: MlHandleTable::new(HandleKind::MlDataset),
            loaders: MlHandleTable::new(HandleKind::MlDataLoader),
            dataframes: MlHandleTable::new(HandleKind::MlDataFrame),
            experiments: MlHandleTable::new(HandleKind::MlExperiment),
            distributed_sessions: MlHandleTable::new(HandleKind::MlDistributedSession),
            kv_caches: MlHandleTable::new(HandleKind::MlKvCache),
            tokenizers: MlHandleTable::new(HandleKind::MlTokenizer),
            vector_indexes: MlHandleTable::new(HandleKind::MlVectorIndex),
            artifacts: MlHandleTable::new(HandleKind::MlArtifact),
        }
    }
}

fn ml_registry() -> &'static Mutex<MlRegistry> {
    static REGISTRY: OnceLock<Mutex<MlRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(MlRegistry::new()))
}

fn with_ml_registry<F, R>(action: F) -> R
where
    F: FnOnce(&mut MlRegistry) -> R,
{
    let mut guard = lock_unpoisoned(ml_registry());
    action(&mut guard)
}

unsafe fn ml_args<'a>(
    ctx: *mut SpectraHostCallContext,
    expected: usize,
) -> Result<(&'a mut SpectraHostCallContext, &'a [SpectraHostValue]), i32> {
    tensor_args(ctx, expected)
}

fn ml_tensor_float_data(handle: usize) -> Option<(Vec<usize>, Vec<f64>, bool)> {
    with_tensor_registry(|registry| {
        let tensor = registry.get(handle)?;
        if tensor.dtype != TensorDType::Float {
            return None;
        }
        Some((
            tensor.shape.clone(),
            tensor_values_as_f64(tensor),
            tensor.requires_grad,
        ))
    })
}

fn ml_tensor_int_data(handle: usize) -> Option<Vec<i64>> {
    with_tensor_registry(|registry| {
        let tensor = registry.get(handle)?;
        Some(tensor.materialize())
    })
}

fn ml_store_float_tensor(handle: usize, values: Vec<f64>) -> bool {
    with_tensor_registry(|registry| {
        let Some(tensor) = registry.get_mut(handle) else {
            return false;
        };
        if tensor.dtype != TensorDType::Float || tensor.len() != values.len() {
            return false;
        }
        tensor.storage = Arc::new(f64_values_to_host(&values));
        tensor.offset = 0;
        tensor.layout = TensorLayout::Contiguous;
        tensor.strides = tensor_strides(&tensor.shape);
        true
    })
}

fn ml_alloc_float_tensor(shape: Vec<usize>, values: Vec<f64>) -> Result<usize, i32> {
    if shape.iter().product::<usize>() != values.len() {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    tensor_alloc(TensorDType::Float, shape, f64_values_to_host(&values))
}

fn ml_sigmoid(value: f64) -> f64 {
    if value >= 0.0 {
        let z = (-value).exp();
        1.0 / (1.0 + z)
    } else {
        let z = value.exp();
        z / (1.0 + z)
    }
}

fn ml_softmax_row(values: &[f64]) -> Option<Vec<f64>> {
    if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let exp = values
        .iter()
        .map(|value| (value - max).exp())
        .collect::<Vec<_>>();
    let sum = exp.iter().sum::<f64>();
    if !sum.is_finite() || sum <= 0.0 {
        return None;
    }
    Some(exp.into_iter().map(|value| value / sum).collect())
}

fn ml_parse_wordpiece_vocab(spec: &str) -> Option<MlWordpieceTokenizer> {
    let mut token_to_id = HashMap::new();
    let mut id_to_token = HashMap::new();
    let mut next_id = 0i64;
    for raw_line in spec.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let (token, id) = if let Some((token, id)) = line.split_once(':') {
            (token.trim().to_string(), id.trim().parse::<i64>().ok()?)
        } else {
            let id = next_id;
            (line.to_string(), id)
        };
        if token.is_empty() || token_to_id.contains_key(&token) || id_to_token.contains_key(&id) {
            return None;
        }
        next_id = next_id.max(id + 1);
        token_to_id.insert(token.clone(), id);
        id_to_token.insert(id, token);
    }
    if token_to_id.is_empty() {
        return None;
    }
    let unk_id = *token_to_id
        .get("[UNK]")
        .or_else(|| token_to_id.get("<unk>"))
        .unwrap_or(&0);
    let max_token_chars = token_to_id
        .keys()
        .map(|token| token.len())
        .max()
        .unwrap_or(1);
    Some(MlWordpieceTokenizer {
        token_to_id,
        id_to_token,
        unk_id,
        max_token_chars,
        special_tokens: HashMap::new(),
        lowercase: true,
        continuation_prefix: "##".to_owned(),
        strict_ids: false,
    })
}

fn ml_vocab_json_depth(value: &serde_json::Value, depth: usize) -> bool {
    if depth > 32 {
        return false;
    }
    match value {
        serde_json::Value::Array(values) => values
            .iter()
            .all(|item| ml_vocab_json_depth(item, depth + 1)),
        serde_json::Value::Object(values) => values
            .values()
            .all(|item| ml_vocab_json_depth(item, depth + 1)),
        _ => true,
    }
}

fn ml_parse_artifact_tokenizer(
    data: &crate::artifact::ArtifactData,
) -> Option<MlWordpieceTokenizer> {
    if data.kind != "multi_array"
        || data.metadata.get("tokenizer_type")? != "wordpiece"
        || data.metadata.get("tokenizer_version")? != "v1"
    {
        return None;
    }
    let vocab_source = data.metadata.get("vocab_json")?;
    if vocab_source.len() > 8 * 1024 * 1024 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(vocab_source).ok()?;
    if !ml_vocab_json_depth(&value, 0) {
        return None;
    }
    let object = value.as_object()?;
    let tokens = object.get("tokens")?.as_array()?;
    if tokens.is_empty() || tokens.len() > 1_000_000 {
        return None;
    }
    let mut token_to_id = HashMap::new();
    let mut id_to_token = HashMap::new();
    for item in tokens {
        let item = item.as_object()?;
        if item.keys().any(|key| key != "id" && key != "token") {
            return None;
        }
        let id = i64::try_from(item.get("id")?.as_u64()?).ok()?;
        let token = item.get("token")?.as_str()?.to_owned();
        if token.is_empty()
            || token_to_id.insert(token.clone(), id).is_some()
            || id_to_token.insert(id, token).is_some()
        {
            return None;
        }
    }
    let mut ids = id_to_token.keys().copied().collect::<Vec<_>>();
    ids.sort_unstable();
    if ids != (0..ids.len() as i64).collect::<Vec<_>>() {
        return None;
    }
    let special_object = object.get("special_tokens")?.as_object()?;
    let mut special_tokens = HashMap::new();
    for (name, value) in special_object {
        let id = i64::try_from(value.as_u64()?).ok()?;
        if !id_to_token.contains_key(&id) || special_tokens.insert(name.clone(), id).is_some() {
            return None;
        }
        let token = id_to_token.get(&id)?.clone();
        special_tokens.insert(token, id);
    }
    let unk_id = *special_tokens.get("unk")?;
    let lowercase = object.get("lowercase")?.as_bool()?;
    let continuation_prefix = object.get("continuation_prefix")?.as_str()?.to_owned();
    if continuation_prefix.is_empty() {
        return None;
    }
    let token_ids = data
        .tensors
        .iter()
        .find(|tensor| tensor.name == "token_ids")?;
    if token_ids.dtype != "int"
        || token_ids.precision != "f64"
        || token_ids.layout != "contiguous"
        || token_ids.shape != vec![tokens.len()]
        || token_ids.bytes.len() != tokens.len() * 8
    {
        return None;
    }
    for (index, bytes) in token_ids.bytes.chunks_exact(8).enumerate() {
        let id = i64::from_le_bytes(bytes.try_into().ok()?);
        if id != index as i64 {
            return None;
        }
    }
    let max_token_chars = token_to_id
        .keys()
        .map(|token| token.chars().count())
        .max()
        .unwrap_or(1);
    Some(MlWordpieceTokenizer {
        token_to_id,
        id_to_token,
        unk_id,
        max_token_chars,
        special_tokens,
        lowercase,
        continuation_prefix,
        strict_ids: true,
    })
}

fn ml_wordpiece_encode(tokenizer: &MlWordpieceTokenizer, text: &str) -> Vec<i64> {
    let mut ids = Vec::new();
    for word in text.split_whitespace() {
        if let Some(id) = tokenizer.special_tokens.get(word) {
            ids.push(*id);
            continue;
        }
        let normalized = word.trim_matches(|ch: char| ch.is_ascii_punctuation());
        let normalized = if tokenizer.lowercase {
            normalized.to_ascii_lowercase()
        } else {
            normalized.to_owned()
        };
        if normalized.is_empty() {
            continue;
        }
        let chars = normalized.chars().collect::<Vec<_>>();
        let mut start = 0usize;
        let mut word_ids = Vec::new();
        let mut failed = false;
        while start < chars.len() {
            let mut end = chars.len().min(start + tokenizer.max_token_chars);
            let mut found = None;
            while end > start {
                let piece = chars[start..end].iter().collect::<String>();
                let candidate = if start == 0 {
                    piece
                } else {
                    format!("{}{}", tokenizer.continuation_prefix, piece)
                };
                if let Some(id) = tokenizer.token_to_id.get(&candidate) {
                    found = Some((*id, end));
                    break;
                }
                end -= 1;
            }
            if let Some((id, next)) = found {
                word_ids.push(id);
                start = next;
            } else {
                failed = true;
                break;
            }
        }
        if failed || word_ids.is_empty() {
            ids.push(tokenizer.unk_id);
        } else {
            ids.extend(word_ids);
        }
    }
    ids
}

fn ml_wordpiece_decode(tokenizer: &MlWordpieceTokenizer, ids: &[i64]) -> Option<String> {
    let mut words = Vec::<String>::new();
    for id in ids {
        let token = tokenizer.id_to_token.get(id).cloned().or_else(|| {
            if tokenizer.strict_ids {
                None
            } else {
                Some("[UNK]".to_string())
            }
        })?;
        if let Some(piece) = token.strip_prefix(&tokenizer.continuation_prefix) {
            if let Some(last) = words.last_mut() {
                last.push_str(piece);
            } else {
                words.push(piece.to_string());
            }
        } else if token == "[PAD]" {
            continue;
        } else {
            words.push(token);
        }
    }
    Some(words.join(" "))
}

fn ml_hash_text_to_embedding(text: &str, dim: usize) -> Option<Vec<f64>> {
    if dim == 0 {
        return None;
    }
    let mut values = vec![0.0f64; dim];
    for token in text.split_whitespace() {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in token.to_ascii_lowercase().as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        let idx = (hash as usize) % dim;
        let sign = if (hash >> 63) == 0 { 1.0 } else { -1.0 };
        values[idx] += sign;
    }
    let norm = values.iter().map(|value| value * value).sum::<f64>().sqrt();
    if norm > 0.0 {
        for value in &mut values {
            *value /= norm;
        }
    }
    Some(values)
}

fn ml_token_set(text: &str) -> HashSet<String> {
    text.split_whitespace()
        .map(|token| {
            token
                .trim_matches(|ch: char| ch.is_ascii_punctuation())
                .to_ascii_lowercase()
        })
        .filter(|token| !token.is_empty())
        .collect()
}

fn ml_f1_overlap(answer: &str, expected: &str) -> f64 {
    let answer_tokens = ml_token_set(answer);
    let expected_tokens = ml_token_set(expected);
    if answer_tokens.is_empty() || expected_tokens.is_empty() {
        return 0.0;
    }
    let overlap = answer_tokens.intersection(&expected_tokens).count() as f64;
    if overlap == 0.0 {
        return 0.0;
    }
    let precision = overlap / answer_tokens.len() as f64;
    let recall = overlap / expected_tokens.len() as f64;
    2.0 * precision * recall / (precision + recall)
}

fn ml_metrics_json(kind: &str, fields: &[(&str, String)]) -> String {
    let fields = fields
        .iter()
        .map(|(key, value)| format!("\"{}\":{}", key, value))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":\"spectra.ml.metric.v1\",\"kind\":\"{}\",{}}}",
        kind, fields
    )
}

fn ml_float_json(value: f64) -> String {
    if value.is_finite() {
        format!("{:.6}", value)
    } else {
        "null".to_string()
    }
}

fn ml_json_payload_arg(value: SpectraHostValue) -> Option<String> {
    let payload = ml_read_path_arg(value)?;
    let trimmed = payload.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        Some(trimmed.to_string())
    } else {
        Some(ml_json_string(trimmed))
    }
}

fn ml_loss_tensor(
    ctx_ref: &mut SpectraHostCallContext,
    value: f64,
    requires_grad: bool,
    creator: Option<AutogradNode>,
) -> i32 {
    match tensor_alloc_autograd(
        TensorDType::Float,
        vec![1],
        vec![value.to_bits() as SpectraHostValue],
        requires_grad,
        creator,
    ) {
        Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
        Err(code) => code,
    }
}

