use super::*;
// ── RagGenerate ──────────────────────────────────────────────────────────────
//
// Real greedy autoregressive generation on top of a committed onnxruntime
// session for a causal-LM style graph (GPT-2 exported to ORT format works).
//
// `spectra.std.ml.generate(session, input_ids, max_new_tokens, eos_id) ->
// int_tensor`
//
// Semantics (all documented contract, verified by tests):
// * The model is called with the FULL growing id sequence every step
//   (`input_ids` reshaped to `[batch=1, seq]`). There is NO KV-cache: every
//   step recomputes the whole prefix through the graph. This is the honest,
//   stateless trade-off for a host-side loop over arbitrary ORT graphs.
// * The graph must expose logits with shape `[batch, seq, vocab]`
//   (float32). Only `batch == 1` is supported; the argmax is taken over the
//   LAST sequence position's vocabulary row.
// * Greedy decoding: `argmax` with ties broken by the LOWEST token id
//   (strict `>` scan), which makes results bit-deterministic for a fixed
//   model and prompt.
// * Stop conditions: EOS (`argmax == eos_id`) or `max_new_tokens` steps,
//   whichever comes first. The EOS token itself is NOT appended: the result
//   contains exactly the ids fed/produced before termination.
// * The session handle comes from `spectra.std.ml.onnx_session_from_bytes`
//   (single-graph-input models); the result is a fresh int64 tensor of the
//   extended id sequence.
// * `spectra.std.ml.generate_ex(session, input_ids, max_new_tokens, eos_id,
//   temperature_bits, top_k, seed)` adds optional sampling on top of the
//   same contract: `temperature_bits` carries an f64 temperature as raw
//   bits, `top_k` bounds the candidate pool and `seed` drives a
//   deterministic splitmix64 stream (no external RNG). `temperature <= 0`
//   or `top_k <= 1` selects plain greedy decoding, bit-compatible with
//   `ml.generate`; otherwise the top-k logits are temperature-scaled into
//   a softmax and sampled, so a fixed seed reproduces the exact sequence
//   for a fixed model and prompt.
//
// The deterministic test fixture below is a real GPT-like toy exported as an
// ONNX ModelProto with trivial fixed weights: `hidden = Gather(embedding,
// input_ids)`, `logits = MatMul(hidden, lm_head)` with `embedding` one-hot
// rows scaled by 8.0 and `lm_head` a permutation matrix, so the greedy next
// token of the real ORT forward pass provably follows a fixed pattern.

/// Deterministic fixture geometry shared by the test writer and the manual
/// reference computation.
#[cfg(all(test, feature = "onnx"))]
pub(crate) const ML_GENERATION_FIXTURE_VOCAB: usize = 6;

/// Sampling controls for `spectra.std.ml.generate_ex`.
#[cfg(feature = "onnx")]
#[derive(Clone, Copy)]
pub(crate) struct MlGenerateSampling {
    /// Softmax temperature; `<= 0.0` selects greedy decoding.
    pub temperature: f64,
    /// Candidate pool size (clamped to the vocabulary); `<= 1` is greedy.
    pub top_k: usize,
    /// Seed of the deterministic splitmix64 sampling stream.
    pub seed: u64,
}

#[cfg(feature = "onnx")]
impl MlGenerateSampling {
    fn is_greedy(&self) -> bool {
        self.temperature <= 0.0 || self.top_k <= 1
    }

    /// One splitmix64 step. The golden-ratio increment makes every seed —
    /// including 0 — a usable stream state, keeping "seed 0" reproducible
    /// instead of degenerate.
    fn next_random(state: &mut u64) -> u64 {
        *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = *state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Samples one token from the LAST-position logits row: keeps the `top_k`
/// highest logits, temperature-scales them into a softmax and walks the
/// cumulative distribution with a uniform draw from the deterministic
/// stream. The stable sort keeps equal logits in LOWEST-token-id order —
/// the same tie-break contract as greedy decoding. Non-finite logits were
/// already rejected by the caller.
#[cfg(feature = "onnx")]
fn ml_generate_sample_top_k(
    last: &[f32],
    vocab: usize,
    options: &MlGenerateSampling,
    state: &mut u64,
) -> usize {
    let mut candidates: Vec<(usize, f32)> = last[..vocab].iter().copied().enumerate().collect();
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    candidates.truncate(options.top_k.min(vocab).max(1));

    let temperature = if options.temperature > 0.0 {
        options.temperature
    } else {
        1.0
    };
    let scaled: Vec<f64> = candidates.iter().map(|(_, logit)| *logit as f64 / temperature).collect();
    let max_scaled = scaled.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let weights: Vec<f64> = scaled.iter().map(|value| (value - max_scaled).exp()).collect();
    let total: f64 = weights.iter().sum();
    let draw =
        MlGenerateSampling::next_random(state) as f64 / (1u64 << 53) as f64 * total;

    let mut cumulative = 0.0;
    for (index, weight) in weights.iter().enumerate() {
        cumulative += weight;
        if draw < cumulative {
            return candidates[index].0;
        }
    }
    candidates[candidates.len() - 1].0
}


/// Real autoregressive greedy generation loop. Holds the process-local
/// session lock for the whole loop: no other host call may run inference on
/// any session while a generate step sequence is in flight.
#[cfg(feature = "onnx")]
pub(crate) fn ml_generate_inner(
    session_id: u64,
    input_ids: &[i64],
    max_new_tokens: usize,
    eos_id: i64,
    sampling: Option<MlGenerateSampling>,
) -> Result<Vec<i64>, i32> {
    use ort::value::Tensor;
    if input_ids.is_empty() || max_new_tokens == 0 {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let mut sessions = ml_onnx_sessions_lock();
    let session = sessions.get_mut(&session_id).ok_or(HOST_STATUS_NOT_FOUND)?;
    if session.inputs().len() != 1 || session.outputs().is_empty() {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    // Copy inlet/outlet names out before `run`: borrowed &str must not
    // overlap with the `&mut session` that inference takes.
    let input_name = session.inputs()[0].name().to_owned();
    let output_name = session.outputs()[0].name().to_owned();

    let mut rng_state = sampling.map(|options| options.seed).unwrap_or(0);

    let mut ids = input_ids.to_vec();
    for _ in 0..max_new_tokens {
        let seq_len = i64::try_from(ids.len()).map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
        // Full prefix every step (no KV-cache): [batch=1, seq].
        let input_value = Tensor::from_array((vec![1i64, seq_len], ids.clone()))
            .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
        let outputs = session
            .run(ort::inputs![input_name.as_str() => input_value])
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        let output = outputs
            .get(output_name.as_str())
            .ok_or(HOST_STATUS_NOT_FOUND)?;
        let (out_shape, flat) = output
            .try_extract_tensor::<f32>()
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;

        // Contract: logits are [batch, seq, vocab] float32.
        if out_shape.len() != 3 || out_shape[0] != 1 {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }
        let seq = usize::try_from(out_shape[1]).map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        let vocab = usize::try_from(out_shape[2]).map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        if seq != ids.len() || vocab == 0 || flat.len() != seq * vocab {
            return Err(HOST_STATUS_INTERNAL_ERROR);
        }
        if flat.iter().any(|value| !value.is_finite()) {
            return Err(HOST_STATUS_INTERNAL_ERROR);
        }

        let last = &flat[(seq - 1) * vocab..];
        let next = match sampling {
            Some(options) if !options.is_greedy() => {
                ml_generate_sample_top_k(last, vocab, &options, &mut rng_state) as i64
            }
            // Greedy argmax over the last position's vocab row; strict `>`
            // keeps the LOWEST index on ties (deterministic tie-break).
            _ => {
                let mut best = 0usize;
                for candidate in 1..vocab {
                    if last[candidate] > last[best] {
                        best = candidate;
                    }
                }
                best as i64
            }
        };
        if next == eos_id {
            // EOS terminates generation and is NOT appended.
            break;
        }
        ids.push(next);
    }
    Ok(ids)
}

/// `spectra.std.ml.generate(session, input_ids, max_new_tokens, eos_id) ->
/// int_tensor`
///
/// Greedy causal-LM generation over a committed onnxruntime session. See the
/// module header for the exact contract. Without the `onnx` feature this
/// degrades to a typed `Error` record like every other real-inference host.
pub(crate) extern "C" fn std_ml_generate(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 4) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[0] <= 0 || args[1] <= 0 || args[2] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        #[cfg(feature = "onnx")]
        {
            let Some(input_ids) = ml_tensor_int_data(args[1] as usize) else {
                return HOST_STATUS_NOT_FOUND;
            };
            if input_ids.is_empty() {
                return HOST_STATUS_INVALID_ARGUMENT;
            }
            match ml_generate_inner(
                args[0] as u64,
                &input_ids,
                args[2] as usize,
                args[3],
                None,
            ) {
                Ok(ids) => match tensor_alloc(TensorDType::Int, vec![ids.len()], ids) {
                    Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
                    Err(_) => HOST_STATUS_INTERNAL_ERROR,
                },
                Err(status) => status,
            }
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = args[1];
            ml_onnx_unavailable_result(ctx_ref, ML_GENERATE)
        }
    }
}

/// `spectra.std.ml.generate_ex(session, input_ids, max_new_tokens, eos_id,
/// temperature_bits, top_k, seed) -> int_tensor`
///
/// Sampling variant of `spectra.std.ml.generate`: `temperature_bits` carries
/// an f64 temperature as raw bits, `top_k` bounds the candidate pool and
/// `seed` drives the deterministic splitmix64 stream. A negative or
/// non-finite temperature is rejected; `temperature <= 0` or `top_k <= 1`
/// degrades to exactly the greedy behavior of `ml.generate`. Without the
/// `onnx` feature this degrades to a typed `Error` record like every other
/// real-inference host.
pub(crate) extern "C" fn std_ml_generate_ex(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 7) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[0] <= 0 || args[1] <= 0 || args[2] <= 0 || args[5] < 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        #[cfg(feature = "onnx")]
        {
            let temperature = f64::from_bits(args[4] as u64);
            if !temperature.is_finite() || temperature < 0.0 {
                return HOST_STATUS_INVALID_ARGUMENT;
            }
            let Ok(top_k) = usize::try_from(args[5]) else {
                return HOST_STATUS_INVALID_ARGUMENT;
            };
            let Some(input_ids) = ml_tensor_int_data(args[1] as usize) else {
                return HOST_STATUS_NOT_FOUND;
            };
            if input_ids.is_empty() {
                return HOST_STATUS_INVALID_ARGUMENT;
            }
            let sampling = MlGenerateSampling {
                temperature,
                top_k,
                seed: args[6] as u64,
            };
            match ml_generate_inner(
                args[0] as u64,
                &input_ids,
                args[2] as usize,
                args[3],
                Some(sampling),
            ) {
                Ok(ids) => match tensor_alloc(TensorDType::Int, vec![ids.len()], ids) {
                    Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
                    Err(_) => HOST_STATUS_INTERNAL_ERROR,
                },
                Err(status) => status,
            }
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = (args[1], args[4], args[5], args[6]);
            ml_onnx_unavailable_result(ctx_ref, ML_GENERATE_EX)
        }
    }
}


// ── RagGenerate: deterministic test fixture ──
//
// Toy causal-LM written with the crate's own protobuf helpers. Weights are
// trivial and fixed: `embedding` is a [vocab, vocab] table whose row `t` is
// the one-hot vector `8.0 * e_t`, and `lm_head` is the permutation matrix
// encoding `next(t)`. The forward pass is therefore exactly
// `logits[b, s, j] = 8.0 * lm_head[t_s, j]`, so the greedy argmax at the last
// position equals `next(last_token)` — a pattern the manual reference below
// reproduces without any floating-point ambiguity (8.0, 1.0 and 0.0 are all
// exact in f32).

#[cfg(all(test, feature = "onnx"))]
pub(crate) fn ml_generation_fixture_next(id: usize) -> i64 {
    const NEXT: [i64; ML_GENERATION_FIXTURE_VOCAB] = [1, 2, 3, 0, 5, 0];
    debug_assert!(id < ML_GENERATION_FIXTURE_VOCAB);
    NEXT[id]
}

/// Manual reference for the whole greedy loop over the fixture model.
#[cfg(all(test, feature = "onnx"))]
pub(crate) fn ml_generation_expected(start: &[i64], max_new_tokens: usize, eos_id: i64) -> Vec<i64> {
    let mut ids = start.to_vec();
    for _ in 0..max_new_tokens {
        let next = ml_generation_fixture_next(*ids.last().expect("non-empty prompt") as usize);
        if next == eos_id {
            break;
        }
        ids.push(next);
    }
    ids
}

/// Builds the deterministic `.onnx` ModelProto bytes for the toy causal-LM.
#[cfg(all(test, feature = "onnx"))]
pub(crate) fn ml_generation_fixture_proto() -> Vec<u8> {
    let vocab = ML_GENERATION_FIXTURE_VOCAB as i64;

    // Initializer `embedding` [vocab, hidden=vocab]: one-hot rows scaled 8.0.
    let mut embedding_raw = Vec::with_capacity(ML_GENERATION_FIXTURE_VOCAB * ML_GENERATION_FIXTURE_VOCAB * 4);
    for row in 0..vocab {
        for col in 0..vocab {
            let value = if row == col { 8.0f32 } else { 0.0 };
            embedding_raw.extend_from_slice(&value.to_le_bytes());
        }
    }
    let mut embedding = Vec::new();
    pb_i64(1, vocab, &mut embedding);
    pb_i64(1, vocab, &mut embedding);
    // TensorProto data_type FLOAT = 1.
    pb_i32(2, 1, &mut embedding);
    pb_string(8, "embedding", &mut embedding);
    pb_message(9, embedding_raw, &mut embedding);

    // Initializer `lm_head` [vocab, vocab]: permutation P[t][next(t)] = 1.
    let mut head_raw =
        Vec::with_capacity(ML_GENERATION_FIXTURE_VOCAB * ML_GENERATION_FIXTURE_VOCAB * 4);
    for row in 0..vocab {
        for col in 0..vocab {
            let value = if col == ml_generation_fixture_next(row as usize) as i64 {
                1.0f32
            } else {
                0.0
            };
            head_raw.extend_from_slice(&value.to_le_bytes());
        }
    }
    let mut head = Vec::new();
    pb_i64(1, vocab, &mut head);
    pb_i64(1, vocab, &mut head);
    pb_i32(2, 1, &mut head);
    pb_string(8, "lm_head", &mut head);
    pb_message(9, head_raw, &mut head);

    // Node: hidden = Gather(embedding, input_ids) → [batch, seq, hidden].
    let mut gather = Vec::new();
    pb_string(1, "embedding", &mut gather);
    pb_string(1, "input_ids", &mut gather);
    pb_string(2, "hidden", &mut gather);
    pb_string(3, "generation_gather", &mut gather);
    pb_string(4, "Gather", &mut gather);

    // Node: logits = MatMul(hidden, lm_head^T-free form) → [batch, seq, vocab].
    let mut matmul = Vec::new();
    pb_string(1, "hidden", &mut matmul);
    pb_string(1, "lm_head", &mut matmul);
    pb_string(2, "logits", &mut matmul);
    pb_string(3, "generation_lm_head", &mut matmul);
    pb_string(4, "MatMul", &mut matmul);

    // Reuses the StatsEmbed fixture helpers (same cfg): symbolic dim params
    // and concrete dim values for TypeProto construction.
    let batch_dim = ml_embed_dim_param("batch");
    let seq_dim = ml_embed_dim_param("seq");
    let vocab_dim = ml_embed_dim_value(vocab);

    let mut graph = Vec::new();
    pb_message(1, gather, &mut graph);
    pb_message(1, matmul, &mut graph);
    pb_string(2, "spectra_generation_graph", &mut graph);
    pb_message(5, embedding, &mut graph);
    pb_message(5, head, &mut graph);
    pb_message(
        11,
        ml_embed_value_info("input_ids", 7, &[batch_dim.clone(), seq_dim.clone()]),
        &mut graph,
    );
    pb_message(
        12,
        ml_embed_value_info("logits", 1, &[batch_dim, seq_dim, vocab_dim]),
        &mut graph,
    );

    let mut opset = Vec::new();
    pb_string(1, "", &mut opset);
    pb_i64(2, 20, &mut opset);

    let mut out = Vec::new();
    pb_i64(1, 9, &mut out);
    pb_string(2, "SpectraLang", &mut out);
    pb_string(5, "RagGenerate causal-LM fixture", &mut out);
    pb_message(7, graph, &mut out);
    pb_message(8, opset, &mut out);
    out
}
