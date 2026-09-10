use super::*;
// ── StatsEmbed ──
//
// Real sentence embeddings through onnxruntime instead of lexical hashing.
//
// `spectra.std.ml.text_embed_model(session, tokenizer, text[, attention_mask])`
// WordPiece-encodes `text` with the registered tokenizer, feeds
// `input_ids` / `attention_mask` (int64) into a committed ORT session whose
// graph embeds token ids (e.g. Gather over an embedding table), takes the
// last hidden state [seq, hidden], applies masked mean-pooling over the
// sequence and L2-normalizes the result.
//
// The lexical FNV embedding (`spectra.std.ml.text_embed`) remains available
// as the documented no-model fallback; this host is the real model path.
//
// The session handle comes from `spectra.std.ml.text_embed_model_session`,
// which commits an ONNX ModelProto from disk. Unlike
// `spectra.std.ml.onnx_session_from_bytes` (single graph input), embedding
// models legitimately carry two int64 graph inputs.

/// Deterministic fixture geometry shared by the test writer and the manual
/// mean-pooling reference computation.
#[cfg(all(test, feature = "onnx"))]
pub(crate) const ML_TEXT_EMBED_FIXTURE_VOCAB: usize = 8;
#[cfg(all(test, feature = "onnx"))]
pub(crate) const ML_TEXT_EMBED_FIXTURE_HIDDEN: usize = 4;

/// Commits an onnxruntime session for an embedding model from in-memory
/// ModelProto bytes. Embedding graphs take two int64 graph inputs
/// (`input_ids`, `attention_mask`); sessions land in the shared process-local
/// session table so `onnx_session_free` can release them too.
#[cfg(feature = "onnx")]
pub(crate) fn ml_text_embed_commit_session(bytes: &[u8]) -> Result<u64, i32> {
    use ort::session::{builder::GraphOptimizationLevel, Session};
    let builder = Session::builder().map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    let session = builder
        .with_optimization_level(GraphOptimizationLevel::Level1)
        .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?
        .commit_from_memory(bytes)
        .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    if session.inputs().len() != 2 {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let id = ML_ONNX_SESSION_NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ml_onnx_sessions_lock().insert(id, session);
    Ok(id)
}

/// Runs the embedding session: int64 ids/mask go in, the last hidden state
/// comes back as f32 [seq, hidden]; masked mean-pooling plus L2 normalization
/// produce the final sentence vector.
#[cfg(feature = "onnx")]
pub(crate) fn ml_text_embed_model_inner(
    session_id: u64,
    input_ids: &[i64],
    attention_mask: &[i64],
) -> Result<Vec<f64>, i32> {
    use ort::value::Tensor;
    if input_ids.is_empty() || attention_mask.len() != input_ids.len() {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let seq_len = i64::try_from(input_ids.len()).map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    let mut sessions = ml_onnx_sessions_lock();
    let session = sessions.get_mut(&session_id).ok_or(HOST_STATUS_NOT_FOUND)?;

    // Resolve graph inlets by canonical name; copy the strings out before
    // `run`: borrowed &str must not overlap with `&mut session`.
    let mut ids_name: Option<String> = None;
    let mut mask_name: Option<String> = None;
    for inlet in session.inputs() {
        match inlet.name() {
            "input_ids" => ids_name = Some(inlet.name().to_owned()),
            "attention_mask" => mask_name = Some(inlet.name().to_owned()),
            _ => {}
        }
    }
    let (Some(ids_name), Some(mask_name)) = (ids_name, mask_name) else {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    };
    let output_name = session.outputs()[0].name().to_owned();

    let ids_value = Tensor::from_array((vec![seq_len], input_ids.to_vec()))
        .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    let mask_value = Tensor::from_array((vec![seq_len], attention_mask.to_vec()))
        .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    let outputs = session
        .run(ort::inputs![
            ids_name.as_str() => ids_value,
            mask_name.as_str() => mask_value
        ])
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    let output = outputs
        .get(output_name.as_str())
        .ok_or(HOST_STATUS_NOT_FOUND)?;
    let (out_shape, flat) = output
        .try_extract_tensor::<f32>()
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    if out_shape.len() != 2 {
        return Err(HOST_STATUS_INTERNAL_ERROR);
    }
    let rows = usize::try_from(out_shape[0]).map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    let hidden = usize::try_from(out_shape[1]).map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    if rows != input_ids.len() || hidden == 0 || flat.len() != rows * hidden {
        return Err(HOST_STATUS_INTERNAL_ERROR);
    }

    // Masked mean-pooling over the sequence dimension.
    let mut pooled = vec![0.0f64; hidden];
    let mut count = 0usize;
    for t in 0..rows {
        if attention_mask[t] == 0 {
            continue;
        }
        count += 1;
        for h in 0..hidden {
            pooled[h] += flat[t * hidden + h] as f64;
        }
    }
    if count == 0 {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    for value in &mut pooled {
        *value /= count as f64;
    }
    // L2 normalization.
    let norm = pooled.iter().map(|value| value * value).sum::<f64>().sqrt();
    if !(norm.is_finite() && norm > 0.0) {
        return Err(HOST_STATUS_INTERNAL_ERROR);
    }
    for value in &mut pooled {
        *value /= norm;
    }
    Ok(pooled)
}

/// `spectra.std.ml.text_embed_model_session(path) -> handle`
///
/// Loads ONNX ModelProto bytes from `path` and commits an onnxruntime
/// session for a two-input embedding model.
pub(crate) extern "C" fn std_ml_text_embed_model_session(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let bytes = match std::fs::read(&path) {
            Ok(value) => value,
            Err(_) => return HOST_STATUS_NOT_FOUND,
        };
        #[cfg(feature = "onnx")]
        {
            return match ml_text_embed_commit_session(&bytes) {
                Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
                Err(_) => HOST_STATUS_INVALID_ARGUMENT,
            };
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = bytes;
            ml_onnx_unavailable_result(ctx_ref, ML_TEXT_EMBED_MODEL_SESSION)
        }
    }
}

/// `spectra.std.ml.text_embed_model(session, tokenizer, text[, mask]) -> tensor[hidden]`
///
/// Real model-backed sentence embedding: WordPiece encode → int64
/// `input_ids`/`attention_mask` → ORT run → masked mean-pooling of the last
/// hidden state → L2 normalize. The optional fourth argument overrides the
/// attention mask (int tensor, one entry per token, 0 = ignored position).
pub(crate) extern "C" fn std_ml_text_embed_model(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        if ctx.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 3 && ctx_ref.arg_len != 4 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        if args[0] <= 0 || args[1] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(text) = ml_read_path_arg(args[2]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(input_ids) = with_ml_registry(|registry| {
            let tokenizer = registry.tokenizers.get(&(args[1] as usize))?;
            Some(ml_wordpiece_encode(tokenizer, &text))
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        if input_ids.is_empty() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let attention_mask: Vec<i64> = if ctx_ref.arg_len == 4 {
            let Some(values) = ml_tensor_int_data(args[3] as usize) else {
                return HOST_STATUS_NOT_FOUND;
            };
            if values.len() != input_ids.len() || values.iter().any(|v| *v != 0 && *v != 1) {
                return HOST_STATUS_INVALID_ARGUMENT;
            }
            values
        } else {
            vec![1; input_ids.len()]
        };
        #[cfg(feature = "onnx")]
        {
            return match ml_text_embed_model_inner(args[0] as u64, &input_ids, &attention_mask) {
                Ok(values) => match ml_alloc_float_tensor(vec![values.len()], values) {
                    Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
                    Err(code) => code,
                },
                Err(status) => status,
            };
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = attention_mask;
            ml_onnx_unavailable_result(ctx_ref, ML_TEXT_EMBED_MODEL)
        }
    }
}

// ── StatsEmbed: deterministic test fixture ──
//
// Tiny embedding model written with the crate's own protobuf helpers:
// `hidden = Gather(embedding, input_ids)` with `attention_mask` as a second
// (host-consumed) graph inlet. Initializer values come from the same
// deterministic stream as every other exported template, so the manual
// mean-pooling reference below recomputes exactly what ORT produces.

#[cfg(all(test, feature = "onnx"))]
pub(crate) fn ml_embed_dim_value(value: i64) -> Vec<u8> {
    let mut out = Vec::new();
    pb_i64(1, value, &mut out);
    out
}

#[cfg(all(test, feature = "onnx"))]
pub(crate) fn ml_embed_dim_param(param: &str) -> Vec<u8> {
    let mut out = Vec::new();
    pb_string(2, param, &mut out);
    out
}

#[cfg(all(test, feature = "onnx"))]
pub(crate) fn ml_embed_value_info(name: &str, elem_type: i32, dims: &[Vec<u8>]) -> Vec<u8> {
    let mut tensor_type = Vec::new();
    pb_i32(1, elem_type, &mut tensor_type);
    let mut shape = Vec::new();
    for dim in dims {
        pb_message(1, dim.clone(), &mut shape);
    }
    pb_message(2, shape, &mut tensor_type);
    // TypeProto { tensor_type: 1 } wraps the Tensor message.
    let mut type_proto = Vec::new();
    pb_message(1, tensor_type, &mut type_proto);
    let mut out = Vec::new();
    pb_string(1, name, &mut out);
    pb_message(2, type_proto, &mut out);
    out
}

/// Manual reference: deterministic embedding table row `id`.
#[cfg(all(test, feature = "onnx"))]
pub(crate) fn ml_embed_fixture_row(id: usize) -> Vec<f64> {
    let table = ml_onnx_deterministic_values(
        ml_onnx_seed("embedding"),
        ML_TEXT_EMBED_FIXTURE_VOCAB * ML_TEXT_EMBED_FIXTURE_HIDDEN,
    );
    table[id * ML_TEXT_EMBED_FIXTURE_HIDDEN..(id + 1) * ML_TEXT_EMBED_FIXTURE_HIDDEN]
        .iter()
        .map(|value| *value as f64)
        .collect()
}

/// Builds the tiny deterministic `.onnx` fixture bytes.
#[cfg(all(test, feature = "onnx"))]
pub(crate) fn ml_text_embed_fixture_proto() -> Vec<u8> {
    let vocab = ML_TEXT_EMBED_FIXTURE_VOCAB as i64;
    let hidden = ML_TEXT_EMBED_FIXTURE_HIDDEN as i64;

    // float32 initializer `embedding` [vocab, hidden].
    let values = ml_onnx_deterministic_values(
        ml_onnx_seed("embedding"),
        ML_TEXT_EMBED_FIXTURE_VOCAB * ML_TEXT_EMBED_FIXTURE_HIDDEN,
    );
    let mut raw = Vec::with_capacity(values.len() * 4);
    for value in &values {
        raw.extend_from_slice(&(*value as f32).to_le_bytes());
    }
    let mut initializer = Vec::new();
    pb_i64(1, vocab, &mut initializer);
    pb_i64(1, hidden, &mut initializer);
    // TensorProto data_type FLOAT = 1.
    pb_i32(2, 1, &mut initializer);
    pb_string(8, "embedding", &mut initializer);
    pb_message(9, raw, &mut initializer);

    // Node: hidden = Gather(embedding, input_ids).
    let mut node = Vec::new();
    pb_string(1, "embedding", &mut node);
    pb_string(1, "input_ids", &mut node);
    pb_string(2, "hidden", &mut node);
    pb_string(3, "embed_gather", &mut node);
    pb_string(4, "Gather", &mut node);

    let mut graph = Vec::new();
    pb_message(1, node, &mut graph);
    pb_string(2, "spectra_text_embed_graph", &mut graph);
    pb_message(5, initializer, &mut graph);
    // int64 TensorProto elem type = 7 (see ml_onnx_type).
    pb_message(
        11,
        ml_embed_value_info("input_ids", 7, &[ml_embed_dim_param("seq")]),
        &mut graph,
    );
    pb_message(
        11,
        ml_embed_value_info("attention_mask", 7, &[ml_embed_dim_param("seq")]),
        &mut graph,
    );
    pb_message(
        12,
        ml_embed_value_info(
            "hidden",
            1,
            &[ml_embed_dim_param("seq"), ml_embed_dim_value(hidden)],
        ),
        &mut graph,
    );

    let mut opset = Vec::new();
    pb_string(1, "", &mut opset);
    pb_i64(2, 20, &mut opset);

    let mut out = Vec::new();
    pb_i64(1, 9, &mut out);
    pb_string(2, "SpectraLang", &mut out);
    pb_string(5, "text embedding fixture", &mut out);
    pb_message(7, graph, &mut out);
    pb_message(8, opset, &mut out);
    out
}
