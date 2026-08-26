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
// * Two execution modes, selected AUTOMATICALLY per session:
//   - KV-cache mode: the graph exports a GPT-2-style interface — every
//     `past_<suffix>` input paired 1:1 with a `present_<suffix>` output
//     (same suffix). Generation runs incrementally: step 1 feeds the full
//     prompt plus EMPTY past tensors (cache length 0 on the penultimate
//     axis); every later step feeds ONLY the last produced token plus the
//     previous step's present outputs re-fed as past. O(new tokens) graph
//     work instead of the O(n²) prefix recomputation.
//   - Re-feed mode (fallback, no past/present interface): the FULL growing
//     id sequence goes through the graph every step (`input_ids` reshaped to
//     `[batch=1, seq]`). Correct but quadratic in processed tokens — the
//     honest, stateless trade-off for arbitrary ORT graphs.
//   Both modes share the exact same selection and stop-rule code, so a
//   model whose cached graph computes the same function as its re-feed form
//   yields bit-identical sequences.
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

// ── KV-cache (past_/present_) interface detection ───────────────────────────
//
// Pure, dependency-free detection of a GPT-2-style KV-cache interface from
// graph I/O names: inputs named `past_<suffix>` pair 1:1 with outputs named
// `present_<suffix>` (identical suffix). The cache length conventionally
// lives on the PENULTIMATE axis of the past tensors (GPT-2 ONNX export
// convention), which drives the empty-cache construction below.

/// One detected `past_X` ↔ `present_X` pairing plus the remaining single id
/// inlet and first remaining (logits) outlet. `pairs` keeps INPUT
/// declaration order — stable even when the model declares its present
/// outputs in a different order.
pub(crate) struct MlKvCacheBinding {
    /// `(past_input_name, present_output_name)`, input-declaration order.
    pub pairs: Vec<(String, String)>,
    /// The single non-cache graph input fed with the token ids.
    pub ids_input: String,
    /// First non-cache graph output read for next-token logits.
    pub logits_output: String,
}

const ML_KV_PAST_PREFIX: &str = "past_";
const ML_KV_PRESENT_PREFIX: &str = "present_";

/// Detects a generic past/present KV-cache interface by suffix pairing.
/// Returns `None` (→ re-feed mode) unless EVERY `past_*` input matches
/// exactly one `present_*` output and vice versa, exactly ONE non-cache
/// input remains (the id inlet) and at least one non-cache output remains
/// (the logits outlet). Bare `past_`/`present_` names have empty suffixes
/// and are treated as ordinary names; duplicate suffixes are ambiguous and
/// reject the whole interface.
pub(crate) fn ml_kv_cache_binding(
    input_names: &[String],
    output_names: &[String],
) -> Option<MlKvCacheBinding> {
    let mut pairs = Vec::new();
    for input in input_names {
        let suffix = match input.strip_prefix(ML_KV_PAST_PREFIX) {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };
        let mut matches = output_names.iter().filter(|output| {
            matches!(output.strip_prefix(ML_KV_PRESENT_PREFIX), Some(s) if s == suffix)
        });
        // Unpaired past input → not a usable cache interface.
        let output = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        pairs.push((input.clone(), output.clone()));
    }
    if pairs.is_empty() {
        return None;
    }
    let rest_inputs: Vec<&String> = input_names
        .iter()
        .filter(|name| match name.strip_prefix(ML_KV_PAST_PREFIX) {
            Some(s) if !s.is_empty() => false,
            _ => true,
        })
        .collect();
    if rest_inputs.len() != 1 {
        return None;
    }
    let rest_outputs: Vec<&String> = output_names
        .iter()
        .filter(|name| match name.strip_prefix(ML_KV_PRESENT_PREFIX) {
            Some(s) if !s.is_empty() => false,
            _ => true,
        })
        .collect();
    if rest_outputs.is_empty() {
        return None;
    }
    Some(MlKvCacheBinding {
        pairs,
        ids_input: rest_inputs[0].clone(),
        logits_output: rest_outputs[0].clone(),
    })
}

/// Shape of the EMPTY past tensor fed on step 1 of the KV path, derived from
/// the dims ORT declares for the past input: FIXED positive dims are kept
/// verbatim, symbolic dims default to 1, and the penultimate (cache-length)
/// dim becomes 0. A fixed positive penultimate dim means the model cannot
/// accept an empty cache (`None`); rank < 2 is not a cache tensor (`None`).
pub(crate) fn ml_kv_empty_past_shape(dims: &[String]) -> Option<Vec<i64>> {
    if dims.len() < 2 {
        return None;
    }
    let mut shape = Vec::with_capacity(dims.len());
    for (index, dim) in dims.iter().enumerate() {
        match dim.parse::<i64>() {
            Ok(value) if value > 0 => {
                if index + 2 == dims.len() {
                    return None;
                }
                shape.push(value);
            }
            _ => shape.push(if index + 2 == dims.len() { 0 } else { 1 }),
        }
    }
    Some(shape)
}

#[cfg(test)]
mod kv_binding_tests {
    use super::*;

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn pairs_by_equal_suffix_in_input_declaration_order() {
        let binding = ml_kv_cache_binding(
            &names(&["input_ids", "past_v", "past_k"]),
            &names(&["present_v", "logits", "present_k"]),
        )
        .expect("interface must be detected");
        assert_eq!(binding.ids_input, "input_ids");
        assert_eq!(binding.logits_output, "logits");
        assert_eq!(
            binding.pairs,
            vec![
                ("past_v".to_string(), "present_v".to_string()),
                ("past_k".to_string(), "present_k".to_string()),
            ],
            "pair order follows INPUT declaration, not output order"
        );
    }

    #[test]
    fn gpt2_style_layer_index_suffixes_pair_stably() {
        let binding = ml_kv_cache_binding(
            &names(&["past_11", "input_ids", "past_0"]),
            &names(&["logits", "present_0", "present_11"]),
        )
        .expect("layer-indexed caches must pair");
        assert_eq!(
            binding.pairs,
            vec![
                ("past_11".to_string(), "present_11".to_string()),
                ("past_0".to_string(), "present_0".to_string()),
            ]
        );
    }

    #[test]
    fn unpaired_past_or_present_rejects_interface() {
        assert!(ml_kv_cache_binding(
            &names(&["input_ids", "past_k"]),
            &names(&["logits"]),
        )
        .is_none());
        assert!(ml_kv_cache_binding(
            &names(&["input_ids"]),
            &names(&["logits", "present_k"]),
        )
        .is_none());
    }

    #[test]
    fn duplicate_present_suffix_is_ambiguous_and_rejected() {
        assert!(ml_kv_cache_binding(
            &names(&["input_ids", "past_k"]),
            &names(&["logits", "present_a_k", "present_b_k"]),
        )
        .is_none());
    }

    #[test]
    fn no_past_inputs_or_extra_id_inputs_fall_back_to_refeed() {
        // No past at all: plain re-feed model.
        assert!(ml_kv_cache_binding(
            &names(&["input_ids"]),
            &names(&["logits"]),
        )
        .is_none());
        // Two non-cache inlets: generation cannot know which one takes ids.
        assert!(ml_kv_cache_binding(
            &names(&["input_ids", "position_ids", "past_k"]),
            &names(&["logits", "present_k"]),
        )
        .is_none());
        // Every outlet is a cache outlet: no logits to decode with.
        assert!(ml_kv_cache_binding(
            &names(&["input_ids", "past_k"]),
            &names(&["present_k"]),
        )
        .is_none());
    }

    #[test]
    fn bare_prefix_names_are_ordinary_identifiers() {
        assert!(ml_kv_cache_binding(
            &names(&["past_", "present_"]),
            &names(&["present_", "past_"]),
        )
        .is_none());
    }

    #[test]
    fn empty_past_shapes_zero_the_penultimate_axis() {
        use std::string::ToString as _;
        let dims = |items: &[i64]| -> Vec<String> {
            items.iter().map(ToString::to_string).collect()
        };
        // Symbolic [batch, past_len, vocab] → [1, 0, 1].
        assert_eq!(
            ml_kv_empty_past_shape(&names(&["batch", "past_seq", "vocab"])),
            Some(vec![1, 0, 1])
        );
        // GPT-2 export style: only the cache length is dynamic.
        assert_eq!(
            ml_kv_empty_past_shape(&dims(&[1, -1, 64]).as_slice()),
            Some(vec![1, 0, 64])
        );
        // Fixed positive cache length cannot be emptied.
        assert_eq!(ml_kv_empty_past_shape(&dims(&[1, 8, 64]).as_slice()), None);
        // Rank < 2 is not a cache tensor.
        assert_eq!(ml_kv_empty_past_shape(&dims(&[16]).as_slice()), None);
    }
}

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


/// Next-token decision shared by BOTH execution modes: sampling when the
/// options select it, otherwise the deterministic greedy argmax. Sharing
/// this function is what makes the KV-cache and re-feed paths produce
/// bit-identical sequences for the same logits.
#[cfg(feature = "onnx")]
fn ml_generate_pick_token(
    last: &[f32],
    vocab: usize,
    sampling: Option<MlGenerateSampling>,
    rng_state: &mut u64,
) -> i64 {
    match sampling {
        Some(options) if !options.is_greedy() => {
            ml_generate_sample_top_k(last, vocab, &options, rng_state) as i64
        }
        // Greedy argmax over the last position's vocab row; strict `>` keeps
        // the LOWEST index on ties (deterministic tie-break).
        _ => {
            let mut best = 0usize;
            for candidate in 1..vocab {
                if last[candidate] > last[best] {
                    best = candidate;
                }
            }
            best as i64
        }
    }
}

/// Incremental KV-cache generation over a session whose graph pairs
/// `past_*` inputs with `present_*` outputs. Step 1 feeds the full prompt
/// plus EMPTY past tensors built from ORT's declared dims; every later step
/// feeds ONLY the newest token plus the previous step's present outputs,
/// extracted into owned tensors and re-fed as past (present → handles →
/// next inputs). Logits validation and EOS handling follow exactly the
/// re-feed contract applied to THIS step's feed length; token selection is
/// literally the same code (`ml_generate_pick_token`).
#[cfg(feature = "onnx")]
fn ml_generate_kv(
    session: &mut ort::session::Session,
    binding: &MlKvCacheBinding,
    input_ids: &[i64],
    max_new_tokens: usize,
    eos_id: i64,
    sampling: Option<MlGenerateSampling>,
    rng_state: &mut u64,
) -> Result<Vec<i64>, i32> {
    use ort::value::{Tensor, Value};

    // Declared dims of every past input, straight from ORT metadata, drive
    // the empty-cache tensors of step 1.
    let past_dims: Vec<(String, Vec<String>)> = session
        .inputs()
        .iter()
        .filter(|inlet| inlet.name().starts_with(ML_KV_PAST_PREFIX))
        .map(|inlet| {
            (
                inlet.name().to_owned(),
                inlet
                    .dtype()
                    .tensor_shape()
                    .map(|shape| shape.iter().map(|dim| dim.to_string()).collect())
                    .unwrap_or_default(),
            )
        })
        .collect();

    let mut ids = input_ids.to_vec();
    let mut past: Option<Vec<(String, Tensor<f32>)>> = None;
    for step in 0..max_new_tokens {
        let first_step = step == 0;
        // Step 1 feeds the whole prompt; later steps only the newest id.
        let feed_ids: Vec<i64> = if first_step {
            ids.clone()
        } else {
            vec![*ids.last().ok_or(HOST_STATUS_INTERNAL_ERROR)?]
        };
        let feed_len = feed_ids.len();
        let seq_len = i64::try_from(feed_len).map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;

        let mut feed: Vec<(&str, Value)> = Vec::with_capacity(1 + binding.pairs.len());
        feed.push((
            binding.ids_input.as_str(),
            Tensor::from_array((vec![1i64, seq_len], feed_ids))
                .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?
                .into_dyn(),
        ));
        if first_step {
            for (name, dims) in &past_dims {
                let shape =
                    ml_kv_empty_past_shape(dims).ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
                feed.push((
                    name.as_str(),
                    Tensor::<f32>::from_array((shape, Vec::<f32>::new()))
                        .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?
                        .into_dyn(),
                ));
            }
        } else {
            for (name, tensor) in past.take().ok_or(HOST_STATUS_INTERNAL_ERROR)? {
                feed.push((name.as_str(), tensor.into_dyn()));
            }
        }

        let outputs = session.run(feed).map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;

        // Contract: logits are [batch=1, fed_len, vocab] float32 — identical
        // to the re-feed path, applied to this step's feed length.
        let output = outputs
            .get(binding.logits_output.as_str())
            .ok_or(HOST_STATUS_NOT_FOUND)?;
        let (out_shape, flat) = output
            .try_extract_tensor::<f32>()
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        if out_shape.len() != 3 || out_shape[0] != 1 || out_shape[1] != feed_len as i64 {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }
        let vocab = usize::try_from(out_shape[2]).map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        if vocab == 0 || flat.len() != feed_len * vocab {
            return Err(HOST_STATUS_INTERNAL_ERROR);
        }
        if flat.iter().any(|value| !value.is_finite()) {
            return Err(HOST_STATUS_INTERNAL_ERROR);
        }

        let last = &flat[(feed_len - 1) * vocab..];
        let next = ml_generate_pick_token(last, vocab, sampling, rng_state);
        if next == eos_id {
            // EOS terminates generation and is NOT appended.
            break;
        }

        // Present outputs become next step's past BEFORE the sequence grows:
        // owned copies decouple them from the dropped SessionOutputs borrow.
        let mut carried: Vec<(String, Tensor<f32>)> = Vec::with_capacity(binding.pairs.len());
        for (past_name, present_name) in &binding.pairs {
            let present = outputs
                .get(present_name.as_str())
                .ok_or(HOST_STATUS_NOT_FOUND)?;
            let (shape, data) = present
                .try_extract_tensor::<f32>()
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
            if shape.is_empty() || shape[0] != 1 {
                return Err(HOST_STATUS_INTERNAL_ERROR);
            }
            let tensor = Tensor::from_array((shape.to_vec(), data.to_vec()))
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
            carried.push((past_name.clone(), tensor));
        }

        ids.push(next);
        past = Some(carried);
    }
    Ok(ids)
}

/// Real autoregressive generation entry point. Holds the process-local
/// session lock for the whole loop: no other host call may run inference on
/// any session while a generate step sequence is in flight. Sessions whose
/// I/O names pair `past_*`/`present_*` take the incremental KV-cache path;
/// everything else keeps the single-input re-feed loop.
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
    // Copy inlet/outlet names out before any `run`: borrowed &str must not
    // overlap with the `&mut session` that inference takes.
    let input_names: Vec<String> = session
        .inputs()
        .iter()
        .map(|inlet| inlet.name().to_owned())
        .collect();
    let output_names: Vec<String> = session
        .outputs()
        .iter()
        .map(|outlet| outlet.name().to_owned())
        .collect();

    let mut rng_state = sampling.map(|options| options.seed).unwrap_or(0);

    if let Some(binding) = ml_kv_cache_binding(&input_names, &output_names) {
        return ml_generate_kv(
            session,
            &binding,
            input_ids,
            max_new_tokens,
            eos_id,
            sampling,
            &mut rng_state,
        );
    }

    if session.inputs().len() != 1 || session.outputs().is_empty() {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let input_name = &input_names[0];
    let output_name = &output_names[0];

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
        let next = ml_generate_pick_token(last, vocab, sampling, &mut rng_state);
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
/// module header for the exact contract: sessions exporting a `past_*` /
/// `present_*` KV-cache interface take the incremental cached path
/// automatically, all others re-feed the full prefix. No host variant is
/// needed — detection is per-session. Without the `onnx` feature this
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
/// degrades to exactly the greedy behavior of `ml.generate`. Like
/// `ml.generate`, this host picks the incremental KV-cache path or the
/// re-feed path automatically from the session's I/O names — sampling runs
/// on identical selection code in both modes. Without the `onnx` feature
/// this degrades to a typed `Error` record like every other real-inference
/// host.
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

/// Shared deterministic weights of both fixtures, already encoded as
/// TensorProto messages: `embedding` ([vocab, vocab], one-hot rows scaled
/// by exactly 8.0) and `lm_head` (the permutation matrix encoding next(t)).
#[cfg(all(test, feature = "onnx"))]
fn ml_generation_fixture_weights() -> (Vec<u8>, Vec<u8>) {
    let vocab = ML_GENERATION_FIXTURE_VOCAB as i64;

    let mut embedding_raw =
        Vec::with_capacity(ML_GENERATION_FIXTURE_VOCAB * ML_GENERATION_FIXTURE_VOCAB * 4);
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

    (embedding, head)
}

/// Wraps an assembled GraphProto into the full ModelProto envelope shared by
/// both generation fixtures (IR version 9, opset 20).
#[cfg(all(test, feature = "onnx"))]
fn ml_generation_model_proto(graph: Vec<u8>, doc: &str) -> Vec<u8> {
    let mut opset = Vec::new();
    pb_string(1, "", &mut opset);
    pb_i64(2, 20, &mut opset);

    let mut out = Vec::new();
    pb_i64(1, 9, &mut out);
    pb_string(2, "SpectraLang", &mut out);
    pb_string(5, doc, &mut out);
    pb_message(7, graph, &mut out);
    pb_message(8, opset, &mut out);
    out
}

/// Builds the deterministic `.onnx` ModelProto bytes for the toy causal-LM.
#[cfg(all(test, feature = "onnx"))]
pub(crate) fn ml_generation_fixture_proto() -> Vec<u8> {
    let vocab = ML_GENERATION_FIXTURE_VOCAB as i64;
    let (embedding, head) = ml_generation_fixture_weights();

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

    ml_generation_model_proto(graph, "RagGenerate causal-LM fixture")
}

/// Builds the deterministic `.onnx` ModelProto for the toy causal-LM WITH a
/// GPT-2-style past_/present_ KV cache:
///
/// ```text
/// hidden     = Gather(embedding, input_ids)      [batch, seq,      vocab]
/// present_kv = Concat(past_kv, hidden)  axis=1   [batch, past+seq, vocab]
/// logits     = MatMul(present_kv, lm_head)       [batch, past+seq, vocab]
/// ```
///
/// Because embedding rows are orthogonal one-hot vectors scaled by exactly
/// 8.0 and lm_head is the permutation matrix encoding next(t), concatenating
/// the cached rows reconstructs precisely the full-history hidden state the
/// re-feed model recomputes every step — both graphs share ONE next-token
/// function, so greedy and sampled sequences must be bit-identical across
/// execution modes (asserted by `kv_generation_tests`). The crate's own
/// protobuf writer is expressive enough to build this graph (Gather /
/// Concat / MatMul plus a single axis attribute), so a real e2e KV fixture
/// exists — no documented-writer limitation applies.
#[cfg(all(test, feature = "onnx"))]
pub(crate) fn ml_generation_fixture_kv_proto() -> Vec<u8> {
    let vocab = ML_GENERATION_FIXTURE_VOCAB as i64;
    let (embedding, head) = ml_generation_fixture_weights();

    // Node: hidden = Gather(embedding, input_ids).
    let mut gather = Vec::new();
    pb_string(1, "embedding", &mut gather);
    pb_string(1, "input_ids", &mut gather);
    pb_string(2, "hidden", &mut gather);
    pb_string(3, "generation_kv_gather", &mut gather);
    pb_string(4, "Gather", &mut gather);

    // AttributeProto { name: "axis" (field 1), i: 1 (field 3),
    // type: INT = 2 (field 20) }.
    let mut axis_attr = Vec::new();
    pb_string(1, "axis", &mut axis_attr);
    pb_i64(3, 1, &mut axis_attr);
    pb_i32(20, 2, &mut axis_attr);

    // Node: present_kv = Concat(past_kv, hidden) along the sequence axis.
    let mut concat = Vec::new();
    pb_string(1, "past_kv", &mut concat);
    pb_string(1, "hidden", &mut concat);
    pb_string(2, "present_kv", &mut concat);
    pb_string(3, "generation_kv_concat", &mut concat);
    pb_string(4, "Concat", &mut concat);
    pb_message(5, axis_attr, &mut concat);

    // Node: logits = MatMul(present_kv, lm_head).
    let mut matmul = Vec::new();
    pb_string(1, "present_kv", &mut matmul);
    pb_string(1, "lm_head", &mut matmul);
    pb_string(2, "logits", &mut matmul);
    pb_string(3, "generation_kv_lm_head", &mut matmul);
    pb_string(4, "MatMul", &mut matmul);

    let batch_dim = ml_embed_dim_param("batch");
    let past_dim = ml_embed_dim_param("past_len");
    let seq_dim = ml_embed_dim_param("seq");
    let total_dim = ml_embed_dim_param("total_len");
    let vocab_dim = ml_embed_dim_value(vocab);

    let mut graph = Vec::new();
    pb_message(1, gather, &mut graph);
    pb_message(1, concat, &mut graph);
    pb_message(1, matmul, &mut graph);
    pb_string(2, "spectra_generation_kv_graph", &mut graph);
    pb_message(5, embedding, &mut graph);
    pb_message(5, head, &mut graph);
    pb_message(
        11,
        ml_embed_value_info("input_ids", 7, &[batch_dim.clone(), seq_dim]),
        &mut graph,
    );
    pb_message(
        11,
        ml_embed_value_info("past_kv", 1, &[batch_dim.clone(), past_dim, vocab_dim.clone()]),
        &mut graph,
    );
    pb_message(
        12,
        ml_embed_value_info("logits", 1, &[batch_dim.clone(), total_dim.clone(), vocab_dim.clone()]),
        &mut graph,
    );
    pb_message(
        12,
        ml_embed_value_info("present_kv", 1, &[batch_dim, total_dim, vocab_dim]),
        &mut graph,
    );

    ml_generation_model_proto(graph, "RagGenerate KV-cache causal-LM fixture")
}

// Real ORT end-to-end proof that the KV path runs incrementally and stays
// equivalent to re-feed on identical model semantics.
#[cfg(all(test, feature = "onnx"))]
mod kv_generation_tests {
    use super::*;

    #[test]
    fn kv_cache_greedy_matches_manual_reference_and_eos_rules() {
        // Success alone proves the KV branch ran: the cached graph has TWO
        // inputs and the single-input re-feed loop rejects such models with
        // HOST_STATUS_INVALID_ARGUMENT before any inference.
        let session_id = ml_onnx_commit_session(&ml_generation_fixture_kv_proto())
            .expect("commit KV-cache fixture session");
        assert!(session_id > 0);

        for prompt in [&[3i64][..], &[0i64, 1][..], &[4i64]] {
            let got =
                ml_generate_inner(session_id, prompt, 6, -1, None).expect("kv greedy generate");
            assert_eq!(got, ml_generation_expected(prompt, 6, -1));
        }

        // EOS terminates identically to re-feed and is NOT appended.
        let got = ml_generate_inner(session_id, &[0], 10, 3, None).expect("kv eos mid-run");
        assert_eq!(got, vec![0, 1, 2]);
        assert_eq!(got, ml_generation_expected(&[0], 10, 3));
        // First sampled token already hits EOS: result is just the prompt.
        let got = ml_generate_inner(session_id, &[4], 10, 5, None).expect("kv immediate eos");
        assert_eq!(got, vec![4]);

        ml_onnx_sessions_lock().remove(&session_id);
    }

    #[test]
    fn kv_cache_sampling_matches_refeed_sampling_bitwise() {
        let kv_session = ml_onnx_commit_session(&ml_generation_fixture_kv_proto())
            .expect("commit KV fixture session");
        let refeed_session = ml_onnx_commit_session(&ml_generation_fixture_proto())
            .expect("commit re-feed fixture session");

        let options = MlGenerateSampling {
            temperature: 50.0,
            top_k: ML_GENERATION_FIXTURE_VOCAB,
            seed: 42,
        };
        // Both graphs encode the same next-token function, and both modes
        // share one selection implementation, so identical seeds must walk
        // identical trajectories.
        let from_cache =
            ml_generate_inner(kv_session, &[0], 8, -1, Some(options)).expect("kv sampled generate");
        let from_refeed = ml_generate_inner(refeed_session, &[0], 8, -1, Some(options))
            .expect("refeed sampled generate");
        assert_eq!(from_cache, from_refeed);

        // Same-seed reproducibility of the cached sampling stream.
        let again = ml_generate_inner(kv_session, &[0], 8, -1, Some(options))
            .expect("kv sampled regenerate");
        assert_eq!(from_cache, again);

        ml_onnx_sessions_lock().remove(&kv_session);
        ml_onnx_sessions_lock().remove(&refeed_session);
    }
}
