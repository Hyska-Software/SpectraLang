use super::*;
pub(crate) extern "C" fn std_ml_embedding_lookup(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((table_shape, table, _)) = ml_tensor_float_data(args[0] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let Some(ids) = ml_tensor_int_data(args[1] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        if table_shape.len() != 2 || table_shape[0] == 0 || table_shape[1] == 0 || ids.is_empty() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let vocab = table_shape[0];
        let dim = table_shape[1];
        let mut out = Vec::with_capacity(ids.len() * dim);
        for id in &ids {
            if *id < 0 || (*id as usize) >= vocab {
                return HOST_STATUS_INVALID_ARGUMENT;
            }
            let start = *id as usize * dim;
            out.extend_from_slice(&table[start..start + dim]);
        }
        match ml_alloc_float_tensor(vec![ids.len(), dim], out) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_positional_encoding(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[0] <= 0 || args[1] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let seq_len = args[0] as usize;
        let dim = args[1] as usize;
        let mut out = Vec::with_capacity(seq_len * dim);
        for pos in 0..seq_len {
            for i in 0..dim {
                let pair = (i / 2) * 2;
                let angle = pos as f64 / 10000f64.powf(pair as f64 / dim as f64);
                out.push(if i % 2 == 0 { angle.sin() } else { angle.cos() });
            }
        }
        match ml_alloc_float_tensor(vec![seq_len, dim], out) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_layer_norm(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 4) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((input_shape, input, _)) = ml_tensor_float_data(args[0] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let Some((scale_shape, scale, _)) = ml_tensor_float_data(args[1] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let Some((bias_shape, bias, _)) = ml_tensor_float_data(args[2] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let eps = f64::from_bits(args[3] as u64);
        if input_shape.is_empty()
            || !eps.is_finite()
            || eps <= 0.0
            || scale_shape.len() != 1
            || bias_shape.len() != 1
        {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(&dim) = input_shape.last() else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if dim == 0 || scale.len() != dim || bias.len() != dim || input.len() % dim != 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let mut out = Vec::with_capacity(input.len());
        for row in input.chunks(dim) {
            let mean = row.iter().sum::<f64>() / dim as f64;
            let var = row
                .iter()
                .map(|value| {
                    let delta = value - mean;
                    delta * delta
                })
                .sum::<f64>()
                / dim as f64;
            let denom = (var + eps).sqrt();
            for idx in 0..dim {
                out.push(((row[idx] - mean) / denom) * scale[idx] + bias[idx]);
            }
        }
        match ml_alloc_float_tensor(input_shape, out) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_gelu(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((shape, input, _)) = ml_tensor_float_data(args[0] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let out = input
            .iter()
            .map(|x| {
                0.5 * x
                    * (1.0
                        + ((2.0 / std::f64::consts::PI).sqrt() * (x + 0.044715 * x.powi(3))).tanh())
            })
            .collect::<Vec<_>>();
        match ml_alloc_float_tensor(shape, out) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_swiglu(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((shape, input, _)) = ml_tensor_float_data(args[0] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let Some((gate_shape, gate, _)) = ml_tensor_float_data(args[1] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        if shape != gate_shape || input.len() != gate.len() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let out = input
            .iter()
            .zip(gate.iter())
            .map(|(x, g)| x * ml_sigmoid(*g))
            .collect::<Vec<_>>();
        match ml_alloc_float_tensor(shape, out) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_attention(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((q_shape, q, _)) = ml_tensor_float_data(args[0] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let Some((k_shape, k, _)) = ml_tensor_float_data(args[1] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let Some((v_shape, v, _)) = ml_tensor_float_data(args[2] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        if q_shape.len() != 2 || k_shape.len() != 2 || v_shape.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let (q_len, dim) = (q_shape[0], q_shape[1]);
        let (k_len, k_dim) = (k_shape[0], k_shape[1]);
        let (v_len, v_dim) = (v_shape[0], v_shape[1]);
        if dim == 0 || k_dim != dim || v_len != k_len || q_len == 0 || k_len == 0 || v_dim == 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let scale = (dim as f64).sqrt();
        let mut out = vec![0.0; q_len * v_dim];
        for qi in 0..q_len {
            let mut scores = Vec::with_capacity(k_len);
            for ki in 0..k_len {
                let mut dot = 0.0;
                for d in 0..dim {
                    dot += q[qi * dim + d] * k[ki * dim + d];
                }
                scores.push(dot / scale);
            }
            let Some(weights) = ml_softmax_row(&scores) else {
                return HOST_STATUS_INVALID_ARGUMENT;
            };
            for vi in 0..v_dim {
                let mut value = 0.0;
                for ki in 0..k_len {
                    value += weights[ki] * v[ki * v_dim + vi];
                }
                out[qi * v_dim + vi] = value;
            }
        }
        match ml_alloc_float_tensor(vec![q_len, v_dim], out) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_kv_cache_new(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[0] <= 0 || args[1] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let handle = with_ml_registry(|registry| {
            registry.kv_caches.insert(MlKvCache {
                max_tokens: args[0] as usize,
                dim: args[1] as usize,
                keys: Vec::new(),
                values: Vec::new(),
            })
        });
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_kv_cache_append(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((key_shape, key, _)) = ml_tensor_float_data(args[1] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let Some((value_shape, value, _)) = ml_tensor_float_data(args[2] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        if key_shape != value_shape || key_shape.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let tokens = key_shape[0];
        let dim = key_shape[1];
        let Some(len) = with_ml_registry(|registry| {
            let cache = registry.kv_caches.get_mut(&(args[0] as usize))?;
            if cache.dim != dim || tokens == 0 || cache.len() + tokens > cache.max_tokens {
                return None;
            }
            cache.keys.extend_from_slice(&key);
            cache.values.extend_from_slice(&value);
            Some(cache.len() as SpectraHostValue)
        }) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        tensor_result(ctx_ref, len)
    }
}

impl MlKvCache {
    pub(crate) fn len(&self) -> usize {
        self.keys.len() / self.dim
    }
}

pub(crate) extern "C" fn std_ml_kv_cache_keys(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((shape, values)) = with_ml_registry(|registry| {
            let cache = registry.kv_caches.get(&(args[0] as usize))?;
            Some((vec![cache.len(), cache.dim], cache.keys.clone()))
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        match ml_alloc_float_tensor(shape, values) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_kv_cache_values(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((shape, values)) = with_ml_registry(|registry| {
            let cache = registry.kv_caches.get(&(args[0] as usize))?;
            Some((vec![cache.len(), cache.dim], cache.values.clone()))
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        match ml_alloc_float_tensor(shape, values) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_kv_cache_len(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(len) = with_ml_registry(|registry| {
            registry
                .kv_caches
                .get(&(args[0] as usize))
                .map(|cache| cache.len() as SpectraHostValue)
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, len)
    }
}

pub(crate) extern "C" fn std_ml_logits_sample(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((_shape, logits, _)) = ml_tensor_float_data(args[0] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let temperature = f64::from_bits(args[1] as u64);
        if logits.is_empty() || !temperature.is_finite() || temperature <= 0.0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let scaled = logits
            .iter()
            .map(|value| *value / temperature)
            .collect::<Vec<_>>();
        let Some(probs) = ml_softmax_row(&scaled) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let sample = {
            let mut state = lock_unpoisoned(random_state());
            random_unit_f64(&mut state)
        };
        let mut cumulative = 0.0;
        for (index, prob) in probs.iter().enumerate() {
            cumulative += *prob;
            if sample <= cumulative {
                return tensor_result(ctx_ref, index as SpectraHostValue);
            }
        }
        tensor_result(ctx_ref, (probs.len() - 1) as SpectraHostValue)
    }
}

/// `spectra.std.ml.logits_sample_seeded(seed, logits, temperature) -> int`
///
/// Deterministic sibling of `logits_sample`: identical validation and
/// full-vocabulary temperature sampling, but the uniform draw comes from a
/// per-call splitmix64 stream seeded by the caller instead of the global
/// RNG. Sampling runs through the shared `ml_generate_sample_top_k`
/// (top_k = vocabulary), so a fixed seed reproduces the exact token for
/// fixed logits. Logits must be finite and representable as `f32`;
/// temperature must be finite and positive.
pub(crate) extern "C" fn std_ml_logits_sample_seeded(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let seed = args[0] as u64;
        let Some((_shape, logits, _)) = ml_tensor_float_data(args[1] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let temperature = f64::from_bits(args[2] as u64);
        if logits.is_empty() || !temperature.is_finite() || temperature <= 0.0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if logits.iter().any(|value| !value.is_finite()) {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let logits_f32: Vec<f32> = logits.iter().map(|value| *value as f32).collect();
        if logits_f32.iter().any(|value| !value.is_finite()) {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let vocab = logits_f32.len();
        let options = MlGenerateSampling {
            temperature,
            top_k: vocab,
            seed,
        };
        let mut state = options.seed;
        let index = ml_generate_sample_top_k(&logits_f32, vocab, &options, &mut state);
        tensor_result(ctx_ref, index as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_tokenizer_wordpiece(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(spec) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(tokenizer) = ml_parse_wordpiece_vocab(&spec) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let handle = with_ml_registry(|registry| registry.tokenizers.insert(tokenizer));
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_tokenizer_load(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Ok(data) = crate::artifact::read(Path::new(&path)) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(tokenizer) = ml_parse_artifact_tokenizer(&data) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let handle = with_ml_registry(|registry| registry.tokenizers.insert(tokenizer));
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_tokenizer_encode(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(text) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(ids) = with_ml_registry(|registry| {
            let tokenizer = registry.tokenizers.get(&(args[0] as usize))?;
            Some(ml_wordpiece_encode(tokenizer, &text))
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        if ids.is_empty() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        match tensor_alloc(TensorDType::Int, vec![ids.len()], ids) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_tokenizer_decode(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(ids) = ml_tensor_int_data(args[1] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let Some(text) = with_ml_registry(|registry| {
            let tokenizer = registry.tokenizers.get(&(args[0] as usize))?;
            Some(ml_wordpiece_decode(tokenizer, &ids))
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let Some(text) = text else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        tensor_result(ctx_ref, alloc_spectra_string(&text))
    }
}

pub(crate) extern "C" fn std_ml_embedding_load(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(tensor_name) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Ok(data) = crate::artifact::read(Path::new(&path)) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if data.metadata.get("artifact_role").map(String::as_str) != Some("embedding_weights") {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(vocab_size) = data
            .metadata
            .get("vocab_size")
            .and_then(|value| value.parse::<usize>().ok())
        else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(embedding_dim) = data
            .metadata
            .get("embedding_dim")
            .and_then(|value| value.parse::<usize>().ok())
        else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(payload) = data
            .tensors
            .iter()
            .find(|tensor| tensor.name == tensor_name)
        else {
            return HOST_STATUS_NOT_FOUND;
        };
        if payload.dtype != "float"
            || payload.precision != "f64"
            || payload.layout != "contiguous"
            || payload.shape.len() != 2
            || payload.shape[0] != vocab_size
            || payload.shape[1] != embedding_dim
        {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        match artifact_tensor_from_payload(payload) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_vector_index_new(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Ok(index) = crate::vector_index::VectorIndex::new(args[0] as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let handle = with_ml_registry(|registry| registry.vector_indexes.insert(index));
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_vector_index_insert(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(id) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((_shape, vector, _)) = ml_tensor_float_data(args[2] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let result = with_ml_registry(|registry| {
            let index = registry
                .vector_indexes
                .get_mut(&(args[0] as usize))
                .ok_or(HOST_STATUS_NOT_FOUND)?;
            index
                .insert(id, &vector)
                .map(|count| count as SpectraHostValue)
                .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)
        });
        match result {
            Ok(count) => tensor_result(ctx_ref, count),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_vector_index_query(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[2] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some((_shape, query, _)) = ml_tensor_float_data(args[1] as usize) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let result = with_ml_registry(|registry| {
            let index = registry
                .vector_indexes
                .get_mut(&(args[0] as usize))
                .ok_or(HOST_STATUS_NOT_FOUND)?;
            index
                .query(&query, args[2] as usize)
                .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)
        });
        let evidence = match result {
            Ok(evidence) => evidence,
            Err(code) => return code,
        };
        let results = evidence
            .results
            .iter()
            .map(|result| {
                format!(
                    "{{\"id\":{},\"score\":{}}}",
                    ml_json_string(&result.id),
                    result.score
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let payload = format!("{{\"schema\":\"spectra.ml.vector_query.v2\",\"algorithm\":\"hnsw\",\"metric\":\"cosine\",\"results\":[{}],\"visited_nodes\":{},\"latency_us\":{}}}", results, evidence.visited_nodes, evidence.latency_us);
        tensor_result(ctx_ref, alloc_spectra_string(&payload))
    }
}

pub(crate) extern "C" fn std_ml_vector_index_persist(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(data) = with_ml_registry(|registry| {
            registry
                .vector_indexes
                .get(&(args[0] as usize))
                .map(crate::vector_index::VectorIndex::artifact_data)
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let data = match data {
            Ok(data) => data,
            Err(_) => return HOST_STATUS_INVALID_ARGUMENT,
        };
        if crate::artifact::write_atomic(std::path::Path::new(&path), &data).is_err() {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        tensor_result(ctx_ref, alloc_spectra_string(&path))
    }
}

pub(crate) extern "C" fn std_ml_vector_index_load(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let data = match crate::artifact::read(std::path::Path::new(&path)) {
            Ok(data) => data,
            Err(_) => return HOST_STATUS_INVALID_ARGUMENT,
        };
        let index = match crate::vector_index::VectorIndex::from_artifact(&data) {
            Ok(index) => index,
            Err(_) => return HOST_STATUS_INVALID_ARGUMENT,
        };
        let handle = with_ml_registry(|registry| registry.vector_indexes.insert(index));
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_vector_index_set_metadata(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(key) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(value) = ml_read_path_arg(args[2]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let updated = with_ml_registry(|registry| {
            registry
                .vector_indexes
                .get_mut(&(args[0] as usize))
                .map(|index| index.set_metadata(&key, &value))
                .unwrap_or(false)
        });
        tensor_result(ctx_ref, if updated { 1 } else { 0 })
    }
}

pub(crate) extern "C" fn std_ml_vector_index_metrics(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(payload) = with_ml_registry(|registry| {
            registry.vector_indexes.get(&(args[0] as usize)).map(|index| {
            let metrics = index.metrics();
            let average_insert_us = if metrics.insert_count == 0 { 0.0 } else { metrics.total_insert_ns as f64 / metrics.insert_count as f64 / 1000.0 };
            let average_query_us = if metrics.query_count == 0 { 0.0 } else { metrics.total_query_ns as f64 / metrics.query_count as f64 / 1000.0 };
            format!("{{\"schema\":\"spectra.ml.vector_index_metrics.v1\",\"algorithm\":\"hnsw\",\"metric\":\"cosine\",\"insert_count\":{},\"query_count\":{},\"average_insert_us\":{},\"average_query_us\":{}}}", metrics.insert_count, metrics.query_count, average_insert_us, average_query_us)
        })
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, alloc_spectra_string(&payload))
    }
}

pub(crate) extern "C" fn std_ml_rag_chunk_text(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(text) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[1] <= 0 || args[2] < 0 || args[2] >= args[1] {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let max_chars = args[1] as usize;
        let overlap = args[2] as usize;
        let chars = text.chars().collect::<Vec<_>>();
        let mut chunks = Vec::new();
        let mut start = 0usize;
        while start < chars.len() {
            let end = (start + max_chars).min(chars.len());
            let chunk = chars[start..end].iter().collect::<String>();
            chunks.push(format!(
                "{{\"id\":\"chunk{}\",\"text\":{}}}",
                chunks.len(),
                ml_json_string(&chunk)
            ));
            if end == chars.len() {
                break;
            }
            start = end - overlap;
        }
        let payload = format!(
            "{{\"schema\":\"spectra.ml.rag_chunks.v1\",\"chunks\":[{}]}}",
            chunks.join(",")
        );
        tensor_result(ctx_ref, alloc_spectra_string(&payload))
    }
}

pub(crate) extern "C" fn std_ml_rag_build_prompt(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(context) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(question) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let prompt = format!(
            "Use the context to answer.\nContext:\n{}\nQuestion:\n{}\nAnswer:",
            context, question
        );
        tensor_result(ctx_ref, alloc_spectra_string(&prompt))
    }
}

pub(crate) extern "C" fn std_ml_rag_evaluate_answer(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(answer) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(expected) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let score = (ml_f1_overlap(&answer, &expected) * 1000.0).round() as SpectraHostValue;
        tensor_result(ctx_ref, score.clamp(0, 1000))
    }
}
