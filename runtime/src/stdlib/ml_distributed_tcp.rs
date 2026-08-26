use super::*;
// ── DistTCP ──────────────────────────────────────────────────────────────────
// Real data-parallel distributed training. Workers compute local gradients
// with the tensor autograd engine over disjoint data shards, ship them to a
// coordinator over TCP loopback (or through an honest multi-thread channel
// path), and apply the ALLREDUCE-averaged gradient via the SGD optimizer
// update path. There are no simulated workers: every reported loss and step
// count comes from an actual forward/backward pass.

// `HashMap`, `Read`, `Write`, `mpsc`, `Arc`, `Mutex`, atomics and `Duration`
use std::net::TcpStream;
use std::sync::Barrier;

pub(crate) const ML_DIST_PROTOCOL_MAGIC: [u8; 4] = [0x53, 0x50, 0x44, 0x57]; // b"SPDW"
pub(crate) const ML_DIST_PROTOCOL_VERSION: u32 = 1;
pub(crate) const ML_DIST_FRAME_HEADER_LEN: usize = 9; // magic(4) + type(1) + payload_len(u32 LE)

pub(crate) const ML_DIST_MSG_HELLO: u8 = 1;
pub(crate) const ML_DIST_MSG_ASSIGN: u8 = 2;
pub(crate) const ML_DIST_MSG_GRADIENTS: u8 = 3;
pub(crate) const ML_DIST_MSG_ACK: u8 = 4;
pub(crate) const ML_DIST_MSG_DONE: u8 = 5;
pub(crate) const ML_DIST_MSG_HEARTBEAT: u8 = 6;

pub(crate) const ML_DISTRIBUTED_TOPOLOGY_TCP: &str = "tcp-workers";
pub(crate) const ML_DISTRIBUTED_TOPOLOGY_MULTITHREAD: &str = "multi-thread";

/// Fully validated specification for one distributed training run.
pub(crate) struct DistTrainSpec {
    pub(crate) worker_count: usize,
    pub(crate) steps: usize,
    pub(crate) lr: f64,
    pub(crate) features: usize,
    pub(crate) total_samples: usize,
    pub(crate) seed: i64,
}

/// Result of a completed run: real per-worker stats plus the global view.
pub(crate) struct DistRunOutcome {
    pub(crate) global_step: i64,
    pub(crate) last_loss: f64,
    pub(crate) workers: Vec<MlDistributedWorker>,
}

// ── wire format ──────────────────────────────────────────────────────────────
//
// frame            := magic[4] type u8 payload_len u32 LE payload[payload_len]
// HELLO payload    := version u32 LE worker_id u32 LE ++ [token_len u32 LE ++ token bytes]
// ASSIGN payload   := x_tensor ++ y_tensor
// GRADIENTS        := local_loss f64 LE ++ w_grad_tensor ++ b_grad_tensor ++ step i64 LE
//                     (the trailing step is a backward-compatible v1 suffix;
//                      payloads without it decode as step 1)
// ACK payload      := avg_w_grad_tensor ++ avg_b_grad_tensor
// DONE payload     := global_step i64 LE ++ mean_loss f64 LE
// HEARTBEAT        := empty payload; worker liveness proof while idle
// tensor           := rank u32 LE dims[u32 LE]^rank values[f64 LE]^n
//
// Optional shared-secret authentication: when the coordinator runs with
// SPECTRA_DIST_TOKEN set (non-empty), every HELLO MUST carry the exact token
// suffix above — a missing or differing token fails the whole run with the
// typed `DistFailure::Auth` before any shard data leaves the coordinator.
// Deployment knobs honored by `dist_run_tcp` (and the multi-node CI entry
// point):
//   SPECTRA_DIST_BIND   interface the coordinator binds (default "127.0.0.1";
//                       "0.0.0.0" exposes it to other containers/hosts)
//   SPECTRA_DIST_TOKEN  shared secret appended to HELLO frames (optional)

pub(crate) fn dist_encode_frame(msg_type: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(ML_DIST_FRAME_HEADER_LEN + payload.len());
    frame.extend_from_slice(&ML_DIST_PROTOCOL_MAGIC);
    frame.push(msg_type);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// Decodes exactly one frame from the front of `buffer`. Returns the message
/// type and the total consumed byte count (header + payload) when a complete,
/// well-formed frame is present.
pub(crate) fn dist_decode_frame(buffer: &[u8]) -> Option<(u8, usize)> {
    if buffer.len() < ML_DIST_FRAME_HEADER_LEN || buffer[..4] != ML_DIST_PROTOCOL_MAGIC {
        return None;
    }
    let msg_type = buffer[4];
    if !matches!(
        msg_type,
        ML_DIST_MSG_HELLO
            | ML_DIST_MSG_ASSIGN
            | ML_DIST_MSG_GRADIENTS
            | ML_DIST_MSG_ACK
            | ML_DIST_MSG_DONE
    ) {
        return None;
    }
    let len = u32::from_le_bytes([buffer[5], buffer[6], buffer[7], buffer[8]]) as usize;
    let total = ML_DIST_FRAME_HEADER_LEN.checked_add(len)?;
    if buffer.len() < total {
        return None;
    }
    Some((msg_type, total))
}

pub(crate) fn dist_push_u32(payload: &mut Vec<u8>, value: u32) {
    payload.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn dist_push_f64(payload: &mut Vec<u8>, value: f64) {
    payload.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn dist_encode_tensor_payload(shape: &[usize], values: &[f64]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(4 + shape.len() * 4 + values.len() * 8);
    dist_push_u32(&mut payload, shape.len() as u32);
    for dim in shape {
        dist_push_u32(&mut payload, *dim as u32);
    }
    for value in values {
        dist_push_f64(&mut payload, *value);
    }
    payload
}

pub(crate) fn dist_read_u32(payload: &[u8], cursor: &mut usize) -> Option<u32> {
    let slice = payload.get(*cursor..*cursor + 4)?;
    *cursor += 4;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

pub(crate) fn dist_read_f64(payload: &[u8], cursor: &mut usize) -> Option<f64> {
    let slice = payload.get(*cursor..*cursor + 8)?;
    *cursor += 8;
    Some(f64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

pub(crate) fn dist_decode_tensor_payload(payload: &[u8], cursor: &mut usize) -> Option<(Vec<usize>, Vec<f64>)> {
    let rank = dist_read_u32(payload, cursor)? as usize;
    if rank == 0 || rank > 8 {
        return None;
    }
    let mut shape = Vec::with_capacity(rank);
    let mut elements = 1usize;
    for _ in 0..rank {
        let dim = dist_read_u32(payload, cursor)? as usize;
        elements = elements.checked_mul(dim)?;
        shape.push(dim);
    }
    if elements > 1 << 24 {
        return None;
    }
    let mut values = Vec::with_capacity(elements);
    for _ in 0..elements {
        values.push(dist_read_f64(payload, cursor)?);
    }
    Some((shape, values))
}

pub(crate) fn dist_encode_hello(worker_id: usize) -> Vec<u8> {
    dist_encode_hello_tokenized(worker_id, None)
}

/// Upper bound on a HELLO token suffix; anything longer is treated as a
/// malformed frame (Protocol) rather than an allocation vector.
pub(crate) const ML_DIST_MAX_TOKEN_LEN: usize = 256;

/// Tokenless HELLOs stay byte-identical to v1 (8-byte payload); passing
/// `Some(token)` appends the `token_len u32 LE ++ token bytes` suffix.
pub(crate) fn dist_encode_hello_tokenized(worker_id: usize, token: Option<&str>) -> Vec<u8> {
    let mut payload = Vec::with_capacity(12 + token.map_or(0, str::len));
    dist_push_u32(&mut payload, ML_DIST_PROTOCOL_VERSION);
    dist_push_u32(&mut payload, worker_id as u32);
    if let Some(token) = token {
        dist_push_u32(&mut payload, token.len() as u32);
        payload.extend_from_slice(token.as_bytes());
    }
    dist_encode_frame(ML_DIST_MSG_HELLO, &payload)
}

pub(crate) fn dist_encode_assign(x_shape: &[usize], x: &[f64], y_shape: &[usize], y: &[f64]) -> Vec<u8> {
    let mut payload = dist_encode_tensor_payload(x_shape, x);
    payload.extend_from_slice(&dist_encode_tensor_payload(y_shape, y));
    dist_encode_frame(ML_DIST_MSG_ASSIGN, &payload)
}
/// Legacy-layout encode (no step suffix); kept so v1 payloads stay decodable.
/// Production sends always carry the step; this path is exercised by the
/// shared protocol-roundtrip test.
#[allow(dead_code)]
pub(crate) fn dist_encode_gradients(loss: f64, w_grad: &[f64], b_grad: &[f64]) -> Vec<u8> {
    dist_encode_gradients_at_step(loss, 1, w_grad, b_grad)
}

/// Full encode: the trailing i64 identifies the 1-based training step so a
/// reconnecting worker can resubmit exactly the round it never got an
/// ACK/DONE for, and the coordinator can replay cached replies idempotently.
pub(crate) fn dist_encode_gradients_at_step(
    loss: f64,
    step: i64,
    w_grad: &[f64],
    b_grad: &[f64],
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(24 + (w_grad.len() + b_grad.len()) * 8);
    dist_push_f64(&mut payload, loss);
    payload.extend_from_slice(&dist_encode_tensor_payload(&[w_grad.len(), 1], w_grad));
    payload.extend_from_slice(&dist_encode_tensor_payload(&[b_grad.len()], b_grad));
    payload.extend_from_slice(&step.to_le_bytes());
    dist_encode_frame(ML_DIST_MSG_GRADIENTS, &payload)
}

pub(crate) fn dist_encode_ack(w_grad: &[f64], b_grad: &[f64]) -> Vec<u8> {
    let mut payload = dist_encode_tensor_payload(&[w_grad.len(), 1], w_grad);
    payload.extend_from_slice(&dist_encode_tensor_payload(&[b_grad.len()], b_grad));
    dist_encode_frame(ML_DIST_MSG_ACK, &payload)
}

pub(crate) fn dist_encode_done(global_step: i64, mean_loss: f64) -> Vec<u8> {
    let mut payload = Vec::with_capacity(16);
    payload.extend_from_slice(&global_step.to_le_bytes());
    dist_push_f64(&mut payload, mean_loss);
    dist_encode_frame(ML_DIST_MSG_DONE, &payload)
}

pub(crate) struct DistHello {
    pub(crate) version: u32,
    pub(crate) worker_id: usize,
    /// Trailing shared-secret suffix; `None` for legacy v1 tokenless HELLOs.
    pub(crate) token: Option<String>,
}

pub(crate) fn dist_decode_hello_full(payload: &[u8]) -> Option<DistHello> {
    let mut cursor = 0usize;
    let version = dist_read_u32(payload, &mut cursor)?;
    let worker_id = dist_read_u32(payload, &mut cursor)?;
    let token = if cursor == payload.len() {
        None
    } else {
        let len = dist_read_u32(payload, &mut cursor)? as usize;
        if len > ML_DIST_MAX_TOKEN_LEN || cursor + len > payload.len() {
            return None;
        }
        Some(String::from_utf8(payload[cursor..cursor + len].to_vec()).ok()?)
    };
    Some(DistHello {
        version,
        worker_id: worker_id as usize,
        token,
    })
}

/// Legacy tuple decode used by the shared protocol-roundtrip tests.
pub(crate) fn dist_decode_hello(payload: &[u8]) -> Option<(u32, usize)> {
    dist_decode_hello_full(payload).map(|hello| (hello.version, hello.worker_id))
}

#[derive(Clone)]
pub(crate) struct DistGradients {
    pub(crate) loss: f64,
    /// 1-based training step this submission belongs to (0 where the concept
    /// does not apply, e.g. gradients computed outside the TCP round-trip).
    pub(crate) step: i64,
    pub(crate) w_grad: Vec<f64>,
    pub(crate) b_grad: Vec<f64>,
}

pub(crate) fn dist_decode_gradients(payload: &[u8]) -> Option<DistGradients> {
    let mut cursor = 0usize;
    let loss = dist_read_f64(payload, &mut cursor)?;
    let (w_shape, w_grad) = dist_decode_tensor_payload(payload, &mut cursor)?;
    let (b_shape, b_grad) = dist_decode_tensor_payload(payload, &mut cursor)?;
    if w_shape != vec![w_grad.len(), 1] || b_shape != vec![b_grad.len()] {
        return None;
    }
    let remaining = payload.len() - cursor;
    let step = match remaining {
        0 => 1, // legacy v1 layout without the step suffix
        8 => i64::from_le_bytes(payload[cursor..cursor + 8].try_into().ok()?),
        _ => return None,
    };
    Some(DistGradients { loss, step, w_grad, b_grad })
}

pub(crate) fn dist_decode_ack(payload: &[u8]) -> Option<(Vec<f64>, Vec<f64>)> {
    let mut cursor = 0usize;
    let (_, w_grad) = dist_decode_tensor_payload(payload, &mut cursor)?;
    let (_, b_grad) = dist_decode_tensor_payload(payload, &mut cursor)?;
    Some((w_grad, b_grad))
}

// ── deterministic dataset ────────────────────────────────────────────────────

/// SplitMix64-style mixing so every (seed, sample, feature) coordinate is a
/// stable pseudo-random value in [-1, 1]. The dataset is generated identically
/// on every participant; shards stay disjoint because each worker owns a
/// contiguous index range.
pub(crate) fn dist_sample_value(seed: i64, sample: usize, feature: usize) -> f64 {
    let mut h = (seed as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (sample as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)
        ^ ((feature as u64).wrapping_add(1).wrapping_mul(0x94D0_49BB_1331_11EB));
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    ((h % 20_001) as f64 / 10_000.0) - 1.0
}

pub(crate) fn dist_true_weight(feature: usize) -> f64 {
    match feature % 4 {
        0 => 0.9,
        1 => 0.6,
        2 => -0.3,
        _ => -0.6,
    }
}

pub(crate) fn dist_target_value(features: usize, row: &[f64]) -> f64 {
    let dot = (0..features)
        .map(|f| dist_true_weight(f) * row[f])
        .sum::<f64>();
    dot + 0.25
}

pub(crate) struct DistShard {
    pub(crate) rows: usize,
    pub(crate) x: Vec<f64>,
    pub(crate) y: Vec<f64>,
}

pub(crate) fn dist_build_shard(spec: &DistTrainSpec, worker_id: usize) -> DistShard {
    let start = spec.total_samples * worker_id / spec.worker_count;
    let end = spec.total_samples * (worker_id + 1) / spec.worker_count;
    let rows = end - start;
    let mut x = Vec::with_capacity(rows * spec.features);
    let mut y = Vec::with_capacity(rows);
    let mut row = vec![0.0f64; spec.features];
    for sample in start..end {
        for feature in 0..spec.features {
            row[feature] = dist_sample_value(spec.seed, sample, feature);
            x.push(row[feature]);
        }
        y.push(dist_target_value(spec.features, &row));
    }
    DistShard { rows, x, y }
}

// ── model parameters and the shared local-gradient kernel ───────────────────

pub(crate) fn dist_init_params(features: usize) -> Result<(usize, usize), i32> {
    let weight = tensor_alloc_autograd(
        TensorDType::Float,
        vec![features, 1],
        f64_values_to_host(&vec![0.0; features]),
        true,
        None,
    )?;
    let bias =
        tensor_alloc_autograd(TensorDType::Float, vec![1], f64_values_to_host(&[0.0]), true, None)?;
    Ok((weight, bias))
}

/// One real training step on a shard: forward through a linear layer, MSE
/// loss, full reverse-mode backward through `tensor_autograd`, then read the
/// accumulated parameter gradients.
pub(crate) fn dist_local_gradient_step(
    shard: &DistShard,
    features: usize,
    weight_handle: usize,
    bias_handle: usize,
) -> Result<DistGradients, i32> {
    if shard.rows == 0 || shard.x.len() != shard.rows * features || shard.y.len() != shard.rows {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let input_h = tensor_alloc_autograd(
        TensorDType::Float,
        vec![shard.rows, features],
        f64_values_to_host(&shard.x),
        false,
        None,
    )?;

    // Forward pass equivalent to std_ml_linear with a single output unit.
    let (pred_shape, pred, requires_grad, creator) = with_tensor_registry(|registry| {
        let input = registry.get(input_h)?.clone();
        let weight = registry.get(weight_handle)?.clone();
        let bias = registry.get(bias_handle)?.clone();
        if input.dtype != TensorDType::Float
            || weight.dtype != TensorDType::Float
            || bias.dtype != TensorDType::Float
            || input.shape.len() != 2
            || weight.shape.len() != 2
            || bias.shape.len() != 1
            || input.shape[1] != weight.shape[0]
            || weight.shape[1] != 1
            || bias.shape[0] != 1
        {
            return None;
        }
        let (batch, in_features) = (input.shape[0], input.shape[1]);
        let x = tensor_values_as_f64(&input);
        let w = tensor_values_as_f64(&weight);
        let b = tensor_values_as_f64(&bias);
        let mut out = matmul_f64(&x, &w, batch, in_features, 1);
        for row in out.iter_mut() {
            *row += b[0];
        }
        let requires_grad =
            tensor_requires_autograd(registry, &[input_h, weight_handle, bias_handle]);
        let creator = requires_grad.then(|| AutogradNode {
            op: AutogradOp::MlLinear,
            parents: vec![input_h, weight_handle, bias_handle],
            input_shape: input.shape.clone(),
            left_shape: input.shape.clone(),
            right_shape: weight.shape.clone(),
            input: b,
            output: out.clone(),
            left: x,
            right: w,
            aux: vec![batch, in_features, 1],
            #[cfg(feature = "gpu")]
            device_aux: None,
        });
        Some((vec![batch, 1], out, requires_grad, creator))
    })
    .ok_or(HOST_STATUS_INVALID_ARGUMENT)?;

    let pred_h = tensor_alloc_autograd(
        TensorDType::Float,
        pred_shape,
        f64_values_to_host(&pred),
        requires_grad,
        creator,
    )?;

    let elements = pred.len();
    let loss_value = pred
        .iter()
        .zip(shard.y.iter())
        .map(|(p, t)| (p - t) * (p - t))
        .sum::<f64>()
        / elements as f64;

    let loss_creator = requires_grad.then(|| AutogradNode {
        op: AutogradOp::MlMse,
        parents: vec![pred_h],
        input_shape: vec![elements],
        left_shape: Vec::new(),
        right_shape: Vec::new(),
        input: Vec::new(),
        output: Vec::new(),
        left: pred.clone(),
        right: shard.y.to_vec(),
        aux: vec![elements],
        #[cfg(feature = "gpu")]
        device_aux: None,
    });
    let loss_h = tensor_alloc_autograd(
        TensorDType::Float,
        vec![1],
        f64_values_to_host(&[loss_value]),
        requires_grad,
        loss_creator,
    )?;
    tensor_backward_impl(loss_h)?;

    let grads = with_tensor_registry(|registry| {
        let weight = registry.get(weight_handle)?;
        let bias = registry.get(bias_handle)?;
        Some((weight.grad.clone()?, bias.grad.clone()?))
    })
    .ok_or(HOST_STATUS_NOT_FOUND)?;

    Ok(DistGradients {
        loss: loss_value,
        step: 0,
        w_grad: grads.0,
        b_grad: grads.1,
    })
}

/// Installs the ALLREDUCE-averaged gradient on a parameter (replacing any
/// leftover local gradient) and applies one SGD update through the shared
/// optimizer path (`ml_optimizer_update`).
pub(crate) fn dist_apply_averaged_update(handle: usize, grad: &[f64], lr: f64) -> bool {
    let installed = with_tensor_registry(|registry| {
        let Some(param) = registry.get_mut(handle) else {
            return false;
        };
        if param.dtype != TensorDType::Float || param.len() != grad.len() {
            return false;
        }
        param.grad = Some(grad.to_vec());
        true
    });
    if !installed {
        return false;
    }
    ml_optimizer_update(handle, |value, g, _| value - lr * g)
}

pub(crate) fn dist_clear_param_grads(weight_handle: usize, bias_handle: usize) {
    with_tensor_registry(|registry| {
        if let Some(weight) = registry.get_mut(weight_handle) {
            weight.grad = None;
        }
        if let Some(bias) = registry.get_mut(bias_handle) {
            bias.grad = None;
        }
    });
}

/// Mean of per-worker local gradients (ALLREDUCE sum/N).
pub(crate) fn dist_average_gradients(parts: &[Option<DistGradients>]) -> Option<(Vec<f64>, Vec<f64>)> {
    let Some(parts): Option<Vec<&DistGradients>> =
        parts.iter().map(|part| part.as_ref()).collect()
    else {
        return None;
    };
    let n = parts.len() as f64;
    let len = parts[0].w_grad.len();
    if parts.iter().any(|part| part.w_grad.len() != len) {
        return None;
    }
    let mut w = vec![0.0; len];
    let mut b = vec![0.0; parts[0].b_grad.len()];
    for part in &parts {
        if part.b_grad.len() != b.len() {
            return None;
        }
        for (slot, value) in w.iter_mut().zip(part.w_grad.iter()) {
            *slot += value / n;
        }
        for (slot, value) in b.iter_mut().zip(part.b_grad.iter()) {
            *slot += value / n;
        }
    }
    Some((w, b))
}

pub(crate) fn dist_parse_spec(args: &[SpectraHostValue]) -> Result<DistTrainSpec, i32> {
    let worker_count = args[2];
    let steps = args[3];
    let lr = f64::from_bits(args[4] as u64);
    let features = args[5];
    let total_samples = args[6];
    let seed = args[7];
    if worker_count <= 0
        || worker_count > 256
        || steps <= 0
        || steps > 100_000
        || !lr.is_finite()
        || lr <= 0.0
        || lr > 10.0
        || features <= 0
        || features > 1024
        || total_samples < worker_count
        || total_samples > 1 << 20
    {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    Ok(DistTrainSpec {
        worker_count: worker_count as usize,
        steps: steps as usize,
        lr,
        features: features as usize,
        total_samples: total_samples as usize,
        seed,
    })
}

pub(crate) fn dist_outcome_to_session(
    name: String,
    out_dir: String,
    spec: &DistTrainSpec,
    topology: &str,
    outcome: DistRunOutcome,
) -> MlDistributedSession {
    MlDistributedSession {
        name,
        out_dir,
        worker_count: spec.worker_count,
        seed: spec.seed,
        global_step: outcome.global_step,
        interrupted_worker: None,
        workers: outcome.workers,
        last_checkpoint_path: None,
        topology: topology.to_string(),
        last_loss: outcome.last_loss,
    }
}

// ── multi-thread mode: OS threads running the identical gradient kernel ──────

pub(crate) fn dist_run_multithread(spec: &DistTrainSpec) -> Result<DistRunOutcome, i32> {
    // Each worker owns a private replica of the parameters (as in real data
    // parallelism); replicas stay bit-identical because every worker applies
    // the same ALLREDUCE-averaged gradient from the same deterministic state.
    let shards: Vec<DistShard> = (0..spec.worker_count)
        .map(|worker_id| dist_build_shard(spec, worker_id))
        .collect();

    let (tx, rx) = mpsc::channel::<Result<DistGradients, i32>>();
    let barrier = Arc::new(Barrier::new(spec.worker_count + 1));
    let averaged: Arc<Mutex<Option<(Vec<f64>, Vec<f64>)>>> = Arc::new(Mutex::new(None));
    let abort = Arc::new(AtomicBool::new(false));

    let (steps, features, lr) = (spec.steps, spec.features, spec.lr);
    let mut handles = Vec::with_capacity(spec.worker_count);
    for shard in shards {
        let tx = tx.clone();
        let barrier = Arc::clone(&barrier);
        let averaged = Arc::clone(&averaged);
        let abort = Arc::clone(&abort);
        handles.push(std::thread::spawn(move || {
            let Ok((weight_handle, bias_handle)) = dist_init_params(features) else {
                abort.store(true, Ordering::SeqCst);
                let _ = tx.send(Err(HOST_STATUS_INTERNAL_ERROR));
                for _ in 0..steps * 2 {
                    barrier.wait();
                }
                return (0.0, 0);
            };
            let mut loss_sum = 0.0f64;
            let samples = shard.rows as i64;
            for _ in 0..steps {
                barrier.wait();
                if abort.load(Ordering::SeqCst) {
                    let _ = tx.send(Err(HOST_STATUS_INTERNAL_ERROR));
                } else {
                    dist_clear_param_grads(weight_handle, bias_handle);
                    let result =
                        dist_local_gradient_step(&shard, features, weight_handle, bias_handle);
                    match &result {
                        Ok(gradients) => loss_sum += gradients.loss,
                        Err(_) => abort.store(true, Ordering::SeqCst),
                    }
                    let _ = tx.send(result);
                }
                barrier.wait();
                let reduced = averaged.lock().expect("averaged lock").clone();
                if let Some((w_grad, b_grad)) = reduced {
                    if !dist_apply_averaged_update(weight_handle, &w_grad, lr)
                        || !dist_apply_averaged_update(bias_handle, &b_grad, lr)
                    {
                        abort.store(true, Ordering::SeqCst);
                    }
                }
            }
            (loss_sum, samples)
        }));
    }
    drop(tx);

    let mut failure: Option<i32> = None;
    let mut mean_losses = Vec::with_capacity(spec.steps);
    for _ in 0..spec.steps {
        barrier.wait();
        let mut parts: Vec<Option<DistGradients>> = Vec::with_capacity(spec.worker_count);
        for _ in 0..spec.worker_count {
            match rx.recv() {
                Ok(Ok(gradients)) => parts.push(Some(gradients)),
                Ok(Err(code)) => {
                    failure.get_or_insert(code);
                    parts.push(None);
                }
                Err(_) => {
                    failure.get_or_insert(HOST_STATUS_INTERNAL_ERROR);
                    parts.push(None);
                }
            }
        }
        if failure.is_some() {
            abort.store(true, Ordering::SeqCst);
            *averaged.lock().expect("averaged lock") = None;
        } else {
            match dist_average_gradients(&parts) {
                Some((w_grad, b_grad)) => {
                    mean_losses.push(
                        parts
                            .iter()
                            .map(|part| part.as_ref().expect("checked above").loss)
                            .sum::<f64>()
                            / parts.len() as f64,
                    );
                    *averaged.lock().expect("averaged lock") = Some((w_grad, b_grad));
                }
                None => {
                    failure.get_or_insert(HOST_STATUS_INVALID_ARGUMENT);
                    abort.store(true, Ordering::SeqCst);
                    *averaged.lock().expect("averaged lock") = None;
                }
            }
        }
        barrier.wait();
    }
    let mut loss_sums = Vec::with_capacity(spec.worker_count);
    let mut sample_counts = Vec::with_capacity(spec.worker_count);
    for handle in handles {
        let (loss_sum, samples) = handle.join().map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        loss_sums.push(loss_sum);
        sample_counts.push(samples);
    }
    if let Some(code) = failure {
        return Err(code);
    }

    let workers = (0..spec.worker_count)
        .map(|worker_id| MlDistributedWorker {
            worker_id,
            step_count: spec.steps as i64,
            sample_count: sample_counts[worker_id],
            accumulator: loss_sums[worker_id],
            active: true,
        })
        .collect();
    Ok(DistRunOutcome {
        global_step: spec.steps as i64,
        last_loss: mean_losses.last().copied().unwrap_or(f64::INFINITY),
        workers,
    })
}

// ── TCP mode: coordinator event loop over mio + blocking worker clients ──────

pub(crate) const ML_DIST_TOKEN_LISTENER: mio::Token = mio::Token(usize::MAX);
pub(crate) const ML_DIST_POLL_TIMEOUT: Duration = Duration::from_millis(250);
pub(crate) const ML_DIST_DEADLINE: Duration = Duration::from_secs(120);

/// Worker reconnect budget per outage: up to 8 attempts with exponential
/// backoff (100ms doubling, hard cap 2s ⇒ ~10.1s worst case).
pub(crate) const ML_DIST_RECONNECT_ATTEMPTS: usize = 8;
pub(crate) const ML_DIST_BACKOFF_BASE_MS: u64 = 100;
pub(crate) const ML_DIST_BACKOFF_CAP_MS: u64 = 2_000;
/// A worker emits HEARTBEAT whenever no inbound frame arrives for this long
/// (implemented via the socket read timeout).
pub(crate) const ML_DIST_HEARTBEAT_PERIOD: Duration = Duration::from_millis(500);

/// Exponential backoff for the worker connect/reconnect loop: 100ms doubling
/// per attempt, capped at 2s.
pub(crate) fn dist_backoff_delay(attempt: usize) -> Duration {
    let factor = 1u64 << attempt.min(6);
    Duration::from_millis((ML_DIST_BACKOFF_BASE_MS * factor).min(ML_DIST_BACKOFF_CAP_MS))
}

/// Bounded-failure knobs for the coordinator event loop; tests shrink these
/// to prove the no-deadlock property quickly.
pub(crate) struct DistTiming {
    /// Max silence (no frame of any kind) tolerated from one worker slot
    /// before the run fails with `DistFailure::WorkerLost` instead of hanging.
    pub(crate) silence_timeout: Duration,
    /// Hard wall-clock cap for the whole run.
    pub(crate) deadline: Duration,
    /// How long the coordinator keeps serving idempotent ACK/DONE replays
    /// after the final DONE was broadcast, for ranks that missed it.
    pub(crate) post_done_grace: Duration,
}

impl Default for DistTiming {
    fn default() -> Self {
        // Must exceed the worst reconnect backoff budget (~10.1s) so an honest
        // worker can always reattach before being declared lost.
        Self {
            silence_timeout: Duration::from_secs(20),
            deadline: ML_DIST_DEADLINE,
            post_done_grace: Duration::from_secs(2),
        }
    }
}

/// Typed coordinator failure. Mapped to host status codes at the host-call
/// boundary, but tests and logs match on the precise cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DistFailure {
    /// A worker slot produced no frame (or stayed disconnected) past the
    /// silence window while the run was still incomplete.
    WorkerLost { worker_id: usize },
    /// Wire-protocol violation by some participant.
    Protocol,
    /// Transport/poll failure or global deadline exceeded.
    Io,
    /// A HELLO presented a missing, unexpected, or non-matching shared
    /// secret against the coordinator's configured token. Fails the run
    /// immediately — unauthenticated peers never receive shard data.
    Auth,
}

pub(crate) struct DistConn {
    pub(crate) stream: mio::net::TcpStream,
    pub(crate) read_buf: Vec<u8>,
    pub(crate) write_queue: Vec<u8>,
    pub(crate) write_offset: usize,
    pub(crate) worker_id: Option<usize>,
    pub(crate) assigned: bool,
}

impl DistConn {
    pub(crate) fn enqueue(&mut self, frame: Vec<u8>) {
        self.write_queue.extend_from_slice(&frame);
    }

    /// Flush queued bytes; returns Ok(false) while bytes remain queued.
    pub(crate) fn flush(&mut self) -> std::io::Result<bool> {
        while self.write_offset < self.write_queue.len() {
            match self.stream.write(&self.write_queue[self.write_offset..]) {
                Ok(0) => return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "closed")),
                Ok(written) => self.write_offset += written,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
                Err(err) => return Err(err),
            }
        }
        self.write_queue.clear();
        self.write_offset = 0;
        Ok(true)
    }

    /// Drain readable bytes into the accumulation buffer until WouldBlock/EOF.
    /// Returns whether EOF was reached.
    pub(crate) fn fill(&mut self) -> std::io::Result<bool> {
        let mut chunk = [0u8; 8192];
        let mut eof = false;
        loop {
            match self.stream.read(&mut chunk) {
                Ok(0) => {
                    eof = true;
                    break;
                }
                Ok(read) => self.read_buf.extend_from_slice(&chunk[..read]),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(err) => return Err(err),
            }
        }
        Ok(eof)
    }

    /// Pop one complete frame if present.
    pub(crate) fn pop_frame(&mut self) -> Option<(u8, Vec<u8>)> {
        let (msg_type, consumed) = dist_decode_frame(&self.read_buf)?;
        let payload = self.read_buf[ML_DIST_FRAME_HEADER_LEN..consumed].to_vec();
        self.read_buf.drain(..consumed);
        Some((msg_type, payload))
    }
}

/// Mutable coordination state shared by the event loop and frame handlers.
pub(crate) struct DistCoordState {
    pub(crate) pending_gradients: HashMap<usize, DistGradients>,
    pub(crate) round_losses: Vec<f64>,
    pub(crate) current_step: usize,
    /// Shared secret every HELLO must present; `None` disables the check.
    pub(crate) auth_token: Option<String>,
    pub(crate) done_sent: bool,
    /// Last completed ALLREDUCE: (step, avg_w, avg_b) — replay source for a
    /// worker that resubmits a step whose ACK it never received.
    pub(crate) last_round: Option<(usize, Vec<f64>, Vec<f64>)>,
    /// Serialized final DONE frame, replayed identically to late ranks.
    pub(crate) done_frame: Option<Vec<u8>>,
    /// Last frame arrival (any type, heartbeats included) per worker slot.
    /// Survives disconnects so a silent or never-returning rank trips the
    /// bounded WorkerLost failure instead of hanging the run.
    pub(crate) slot_seen: Vec<StdInstant>,
    /// Live worker-slot → connection-token bindings.
    pub(crate) bindings: HashMap<usize, mio::Token>,
}

/// Drops a connection; if it carried a worker binding, that slot becomes free
/// for reconnect and its pending gradient is voided (the deterministic
/// gradient kernel lets the rank reproduce it exactly after reattaching).
/// Gradients already contributed by surviving workers stay valid for the
/// interrupted round.
pub(crate) fn dist_retire_conn(
    conns: &mut HashMap<mio::Token, DistConn>,
    state: &mut DistCoordState,
    token: mio::Token,
) {
    if let Some(conn) = conns.remove(&token) {
        if let Some(id) = conn.worker_id {
            if state.bindings.get(&id) == Some(&token) {
                state.bindings.remove(&id);
            }
            state.pending_gradients.remove(&id);
        }
    }
}

/// One decoded frame from connection `token`. Returns a typed failure for
/// protocol violations instead of silently tolerating garbage.
pub(crate) fn dist_coord_handle_frame(
    conns: &mut HashMap<mio::Token, DistConn>,
    state: &mut DistCoordState,
    spec: &DistTrainSpec,
    token: mio::Token,
    msg_type: u8,
    payload: Vec<u8>,
) -> Result<(), DistFailure> {
    match msg_type {
        ML_DIST_MSG_HELLO => {
            let Some(hello) = dist_decode_hello_full(&payload) else {
                return Err(DistFailure::Protocol);
            };
            if hello.version != ML_DIST_PROTOCOL_VERSION || hello.worker_id >= spec.worker_count {
                return Err(DistFailure::Protocol);
            }
            // Shared-secret gate: exact match required in both directions —
            // a tokenless HELLO against a tokened coordinator, or vice versa,
            // is rejected with the typed Auth failure before any ASSIGN.
            if state.auth_token.as_deref() != hello.token.as_deref() {
                return Err(DistFailure::Auth);
            }
            // One identity per connection; one live connection per rank.
            if let Some(conn) = conns.get(&token) {
                if matches!(conn.worker_id, Some(bound) if bound != hello.worker_id) {
                    return Err(DistFailure::Protocol);
                }
            }
            if let Some(&bound) = state.bindings.get(&hello.worker_id) {
                if bound != token && conns.contains_key(&bound) {
                    // Fast retry: the rank reattached before its dead socket
                    // was noticed. Take over the slot, retiring the stale
                    // connection (any voided gradient is reproduced exactly
                    // by the deterministic kernel after this ASSIGN).
                    dist_retire_conn(conns, state, bound);
                }
            }
            let assign = {
                let shard = dist_build_shard(spec, hello.worker_id);
                dist_encode_assign(
                    &[shard.rows, spec.features],
                    &shard.x,
                    &[shard.rows, 1],
                    &shard.y,
                )
            };
            let Some(conn) = conns.get_mut(&token) else {
                return Ok(());
            };
            conn.worker_id = Some(hello.worker_id);
            conn.assigned = true;
            conn.enqueue(assign);
            state.bindings.insert(hello.worker_id, token);
            state.slot_seen[hello.worker_id] = StdInstant::now();
            Ok(())
        }
        ML_DIST_MSG_HEARTBEAT => {
            // Pure liveness: refresh freshness, touch nothing else.
            if let Some(conn) = conns.get_mut(&token) {
                if let Some(id) = conn.worker_id {
                    state.slot_seen[id] = StdInstant::now();
                }
            }
            Ok(())
        }
        ML_DIST_MSG_GRADIENTS => {
            let Some(sender) = conns.get(&token).and_then(|conn| conn.worker_id) else {
                return Err(DistFailure::Protocol); // gradients before HELLO
            };
            let assigned = conns.get(&token).map(|conn| conn.assigned).unwrap_or(false);
            let Some(gradients) = dist_decode_gradients(&payload) else {
                return Err(DistFailure::Protocol);
            };
            let step = gradients.step;
            let valid = assigned
                && step >= 1
                && gradients.w_grad.len() == spec.features
                && gradients.b_grad.len() == 1
                && gradients.loss.is_finite();
            if !valid || state.pending_gradients.contains_key(&sender) {
                return Err(DistFailure::Protocol);
            }
            state.slot_seen[sender] = StdInstant::now();

            // Idempotent replay: the sender already earned its reply for this
            // step but never received it (connection died mid-ALLREDUCE).
            // Re-send the cached answer without touching round state.
            if step <= state.current_step as i64 {
                let replay = if state.done_sent && step == state.current_step as i64 {
                    state.done_frame.clone()
                } else {
                    state.last_round.as_ref().and_then(|(done_step, w, b)| {
                        (*done_step as i64 == step).then(|| dist_encode_ack(w, b))
                    })
                };
                let Some(frame) = replay else {
                    return Err(DistFailure::Protocol);
                };
                if let Some(conn) = conns.get_mut(&token) {
                    conn.enqueue(frame);
                }
                return Ok(());
            }
            if step != state.current_step as i64 + 1 {
                return Err(DistFailure::Protocol);
            }
            state.pending_gradients.insert(sender, gradients);
            if state.pending_gradients.len() < spec.worker_count {
                return Ok(());
            }
            // ALLREDUCE barrier reached: average and advance.
            let ordered: Vec<Option<DistGradients>> = (0..spec.worker_count)
                .map(|id| state.pending_gradients.remove(&id))
                .collect();
            state.round_losses = ordered
                .iter()
                .filter_map(|part| part.as_ref())
                .map(|g| g.loss)
                .collect();
            let Some((w_grad, b_grad)) = dist_average_gradients(&ordered) else {
                return Err(DistFailure::Protocol);
            };
            state.current_step += 1;
            state.last_round = Some((state.current_step, w_grad.clone(), b_grad.clone()));
            if state.current_step >= spec.steps {
                let mean_loss = state.round_losses.iter().sum::<f64>()
                    / state.round_losses.len() as f64;
                let done = dist_encode_done(state.current_step as i64, mean_loss);
                state.done_frame = Some(done.clone());
                state.done_sent = true;
                for conn in conns.values_mut() {
                    if conn.worker_id.is_some() {
                        conn.enqueue(done.clone());
                    }
                }
            } else {
                let ack = dist_encode_ack(&w_grad, &b_grad);
                for conn in conns.values_mut() {
                    if conn.worker_id.is_some() {
                        conn.enqueue(ack.clone());
                    }
                }
            }
            Ok(())
        }
        _ => Err(DistFailure::Protocol),
    }
}

/// Coordinator event loop over a pre-bound listener. Workers arrive purely
/// over TCP — none are spawned here — which is what makes real multi-process
/// deployments possible. Every wait is bounded: a silent/disconnected worker
/// fails with `WorkerLost` after `timing.silence_timeout`, the whole run is
/// capped by `timing.deadline`; neither path can hang.
pub(crate) fn dist_coord_loop(
    listener: mio::net::TcpListener,
    spec: &DistTrainSpec,
    timing: DistTiming,
    auth_token: Option<&str>,
) -> Result<(i64, f64), DistFailure> {
    use mio::Interest;

    let mut poll = mio::Poll::new().map_err(|_| DistFailure::Io)?;
    let mut listener = listener;
    poll.registry()
        .register(&mut listener, ML_DIST_TOKEN_LISTENER, Interest::READABLE)
        .map_err(|_| DistFailure::Io)?;

    let mut events = mio::Events::with_capacity(64);
    let mut conns: HashMap<mio::Token, DistConn> = HashMap::new();
    let mut next_token = 0usize;
    let started = StdInstant::now();
    let mut state = DistCoordState {
        pending_gradients: HashMap::new(),
        round_losses: Vec::new(),
        current_step: 0,
        done_sent: false,
        auth_token: auth_token.map(str::to_owned),
        last_round: None,
        done_frame: None,
        slot_seen: vec![started; spec.worker_count],
        bindings: HashMap::new(),
    };
    let deadline = started + timing.deadline;

    // Post-DONE: keep serving idempotent replays until every rank either
    // confirmed delivery (closed its socket ⇒ binding retired) or the grace
    // window elapsed — whichever comes first. Never unbounded.
    let mut done_at: Option<StdInstant> = None;
    while !state.done_sent
        || (!state.bindings.is_empty()
            && done_at.map_or(true, |at| at.elapsed() < timing.post_done_grace))
    {
        let now = StdInstant::now();
        if now > deadline {
            return Err(DistFailure::Io);
        }
        if !state.done_sent {
            for slot in 0..spec.worker_count {
                if now.duration_since(state.slot_seen[slot]) > timing.silence_timeout {
                    return Err(DistFailure::WorkerLost { worker_id: slot });
                }
            }
        }
        match poll.poll(&mut events, Some(ML_DIST_POLL_TIMEOUT)) {
            Ok(()) => {}
            Err(_) => return Err(DistFailure::Io),
        }

        let mut dead: Vec<mio::Token> = Vec::new();
        for event in events.iter() {
            if event.token() == ML_DIST_TOKEN_LISTENER {
                loop {
                    match listener.accept() {
                        Ok((stream, _addr)) => {
                            let token = mio::Token(next_token);
                            next_token += 1;
                            let mut stream = stream;
                            if poll
                                .registry()
                                .register(
                                    &mut stream,
                                    token,
                                    Interest::READABLE.add(Interest::WRITABLE),
                                )
                                .is_err()
                            {
                                return Err(DistFailure::Io);
                            }
                            conns.insert(
                                token,
                                DistConn {
                                    stream,
                                    read_buf: Vec::new(),
                                    write_queue: Vec::new(),
                                    write_offset: 0,
                                    worker_id: None,
                                    assigned: false,
                                },
                            );
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(_) => return Err(DistFailure::Io),
                    }
                }
                continue;
            }
            let token = event.token();
            let Some(conn) = conns.get_mut(&token) else {
                continue;
            };
            if event.is_readable() {
                match conn.fill() {
                    // EOF with nothing decodable buffered ⇒ peer went away.
                    Ok(true) if dist_decode_frame(&conn.read_buf).is_none() => {
                        dead.push(token);
                    }
                    Ok(_) => {}
                    Err(_) => dead.push(token),
                }
            }
            if event.is_writable() && conn.flush().is_err() {
                dead.push(token);
            }
        }
        // Process buffered frames per connection so HELLO and GRADIENTS are
        // attributed to the exact peer that sent them.
        for token in conns.keys().copied().collect::<Vec<_>>() {
            loop {
                let frame = match conns.get_mut(&token) {
                    Some(conn) => conn.pop_frame(),
                    None => break,
                };
                let Some((msg_type, payload)) = frame else {
                    break;
                };
                if let Err(failure) =
                    dist_coord_handle_frame(&mut conns, &mut state, spec, token, msg_type, payload)
                {
                    return Err(failure);
                }
            }
        }
        for token in dead {
            dist_retire_conn(&mut conns, &mut state, token);
        }
        // Flush queued writes opportunistically.
        let mut flush_dead: Vec<mio::Token> = Vec::new();
        for (token, conn) in conns.iter_mut() {
            if !conn.write_queue.is_empty() && conn.flush().is_err() {
                flush_dead.push(*token);
            }
        }
        for token in flush_dead {
            dist_retire_conn(&mut conns, &mut state, token);
        }
        if state.done_sent {
            done_at.get_or_insert_with(StdInstant::now);
        }
    }

    Ok((
        state.current_step as i64,
        state.round_losses.last().copied().unwrap_or(f64::INFINITY),
    ))
}

pub(crate) fn dist_run_tcp(spec: &DistTrainSpec) -> Result<DistRunOutcome, i32> {
    // Deployment knobs: SPECTRA_DIST_BIND selects the listening interface
    // (default loopback; "0.0.0.0" for container/multi-node deployments) and
    // SPECTRA_DIST_TOKEN turns on shared-secret HELLO authentication.
    let bind = std::env::var("SPECTRA_DIST_BIND")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "127.0.0.1".to_owned());
    let token = std::env::var("SPECTRA_DIST_TOKEN")
        .ok()
        .filter(|token| !token.is_empty());
    let addr: std::net::SocketAddr = format!("{bind}:0")
        .parse()
        .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    let listener = mio::net::TcpListener::bind(addr).map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    let port = listener
        .local_addr()
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?
        .port();
    // In-process workers always reach the coordinator over loopback, even
    // when the listener is bound to a specific external interface.
    let connect_host = if bind == "0.0.0.0" || bind == "::" {
        "127.0.0.1".to_owned()
    } else {
        bind.clone()
    };

    let (stats_tx, stats_rx) = mpsc::channel::<Result<(usize, f64, i64), String>>();
    // Workers create their own private parameter replicas on connect.
    let shards: Vec<DistShard> = (0..spec.worker_count)
        .map(|worker_id| dist_build_shard(spec, worker_id))
        .collect();

    let (steps, features, lr) = (spec.steps, spec.features, spec.lr);
    let mut handles = Vec::with_capacity(spec.worker_count);
    for (worker_id, shard) in shards.into_iter().enumerate() {
        let stats_tx = stats_tx.clone();
        let connect_host = connect_host.clone();
        let token = token.clone();
        handles.push(std::thread::spawn(move || {
            let result = dist_tcp_worker(
                &connect_host,
                port,
                worker_id,
                shard,
                features,
                steps,
                lr,
                token.as_deref(),
            );
            let _ = stats_tx.send(result);
        }));
    }
    drop(stats_tx);

    let coord_result = dist_coord_loop(listener, spec, DistTiming::default(), token.as_deref());

    let mut received: Vec<(f64, i64)> = vec![(0.0, 0); spec.worker_count];
    let mut worker_error: Option<i32> = None;
    for _ in 0..spec.worker_count {
        match stats_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok((worker_id, loss_sum, samples))) => {
                if worker_id < spec.worker_count {
                    received[worker_id] = (loss_sum, samples);
                } else {
                    worker_error.get_or_insert(HOST_STATUS_INTERNAL_ERROR);
                }
            }
            // On failure paths workers observe the closed sockets well inside
            // their heartbeat period, so this drains fast instead of hanging.
            Ok(Err(_)) | Err(_) => {
                worker_error.get_or_insert(HOST_STATUS_INTERNAL_ERROR);
            }
        }
    }
    for handle in handles {
        if handle.join().is_err() {
            worker_error.get_or_insert(HOST_STATUS_INTERNAL_ERROR);
        }
    }
    let failure_to_status = |failure: DistFailure| match failure {
        DistFailure::Protocol | DistFailure::Auth => HOST_STATUS_INVALID_ARGUMENT,
        DistFailure::WorkerLost { .. } | DistFailure::Io => HOST_STATUS_INTERNAL_ERROR,
    };
    let (global_step, last_loss) = coord_result.map_err(failure_to_status)?;
    if let Some(code) = worker_error {
        return Err(code);
    }

    let workers = received
        .into_iter()
        .enumerate()
        .map(|(worker_id, (loss_sum, samples))| MlDistributedWorker {
            worker_id,
            step_count: spec.steps as i64,
            sample_count: samples,
            accumulator: loss_sum,
            active: true,
        })
        .collect();
    Ok(DistRunOutcome {
        global_step,
        last_loss,
        workers,
    })
}

/// Attach (connect → HELLO → validated ASSIGN) with bounded exponential
/// backoff: up to `ML_DIST_RECONNECT_ATTEMPTS` tries spaced by
/// `dist_backoff_delay` — 100ms doubling, capped at 2s (~10.1s worst case).
/// Used for the initial attach and again after every mid-run connection loss.
pub(crate) fn dist_tcp_attach(
    host: &str,
    port: u16,
    worker_id: usize,
    shard: &DistShard,
    features: usize,
    token: Option<&str>,
    read_buf: &mut Vec<u8>,
) -> Result<TcpStream, String> {
    let mut last_err = String::from("no connection attempt was made");
    for attempt in 0..ML_DIST_RECONNECT_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(dist_backoff_delay(attempt - 1));
        }
        let mut stream = match TcpStream::connect((host, port)) {
            Ok(stream) => stream,
            Err(err) => {
                last_err = format!("connect failed: {err}");
                continue;
            }
        };
        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(ML_DIST_HEARTBEAT_PERIOD));
        if stream
            .write_all(&dist_encode_hello_tokenized(worker_id, token))
            .is_err()
        {
            last_err = "hello write failed".to_string();
            continue;
        }
        read_buf.clear();
        match dist_worker_read_frame(&mut stream, read_buf) {
            Some((ML_DIST_MSG_ASSIGN, payload)) => {
                let mut cursor = 0usize;
                let x_tensor = dist_decode_tensor_payload(&payload, &mut cursor);
                let y_tensor = dist_decode_tensor_payload(&payload, &mut cursor);
                match (x_tensor, y_tensor) {
                    (
                        Some((x_shape, x_values)),
                        Some((y_shape, y_values)),
                    ) if x_shape == vec![shard.rows, features]
                        && y_shape == vec![shard.rows, 1]
                        && x_values == shard.x
                        && y_values == shard.y =>
                    {
                        return Ok(stream);
                    }
                    (Some(_), Some(_)) => {
                        last_err = "protocol: ASSIGN shard mismatch".to_string();
                    }
                    _ => {
                        last_err = "protocol: malformed ASSIGN tensors".to_string();
                    }
                }
            }
            Some((other, _)) => {
                last_err = format!("protocol: expected ASSIGN, got message type {other}");
            }
            None => {
                last_err = "protocol: connection closed before ASSIGN".to_string();
            }
        }
    }
    Err(format!(
        "giving up after {ML_DIST_RECONNECT_ATTEMPTS} attempts: {last_err}"
    ))
}

/// Blocking worker client: attaches to the coordinator (retrying with
/// exponential backoff), performs the
/// HELLO → ASSIGN → (GRADIENTS ⇄ ACK)* → DONE conversation byte-for-byte,
/// emits HEARTBEAT frames while idle so the coordinator can distinguish a
/// live worker from a dead one, and survives transient disconnections by
/// reconnecting and resubmitting the exact step it never got a reply for.
pub(crate) fn dist_tcp_worker(
    host: &str,
    port: u16,
    worker_id: usize,
    shard: DistShard,
    features: usize,
    steps: usize,
    lr: f64,
    token: Option<&str>,
) -> Result<(usize, f64, i64), String> {
    // Private parameter replica for this rank, as in real data parallelism.
    let (weight_handle, bias_handle) =
        dist_init_params(features).map_err(|code| format!("parameter init failed: {code}"))?;
    let mut loss_sum = 0.0f64;
    // Highest step confirmed by an ACK/DONE — the resume point after a loss.
    let mut acked_steps = 0usize;
    let mut read_buf: Vec<u8> = Vec::new();

    let mut stream = dist_tcp_attach(host, port, worker_id, &shard, features, token, &mut read_buf)?;
    while acked_steps < steps {
        dist_clear_param_grads(weight_handle, bias_handle);
        let gradients = dist_local_gradient_step(&shard, features, weight_handle, bias_handle)
            .map_err(|code| format!("local gradient step failed: status {code}"))?;
        let pending_loss = gradients.loss;
        let submission = dist_encode_gradients_at_step(
            gradients.loss,
            acked_steps as i64 + 1,
            &gradients.w_grad,
            &gradients.b_grad,
        );
        if stream.write_all(&submission).is_err() {
            stream = dist_tcp_attach(host, port, worker_id, &shard, features, token, &mut read_buf)?;
            continue;
        }
        match dist_worker_read_frame(&mut stream, &mut read_buf) {
            Some((ML_DIST_MSG_ACK, payload)) => {
                let (w_grad, b_grad) = dist_decode_ack(&payload)
                    .ok_or_else(|| "protocol: malformed ACK".to_string())?;
                if !dist_apply_averaged_update(weight_handle, &w_grad, lr)
                    || !dist_apply_averaged_update(bias_handle, &b_grad, lr)
                {
                    return Err("parameter update failed".to_string());
                }
                loss_sum += pending_loss;
                acked_steps += 1;
            }
            Some((ML_DIST_MSG_DONE, payload)) => {
                if payload.len() != 16 {
                    return Err("protocol: malformed DONE".to_string());
                }
                loss_sum += pending_loss;
                acked_steps = steps;
            }
            Some((other, _)) => return Err(format!("protocol: unexpected message type {other}")),
            None => {
                stream = dist_tcp_attach(host, port, worker_id, &shard, features, token, &mut read_buf)?;
            }
        }
    }
    Ok((worker_id, loss_sum, shard.rows as i64))
}

/// Blocking frame reader used by worker clients: accumulates stream bytes
/// until one complete frame is available, then returns (type, payload).
/// While idle it emits a HEARTBEAT every read-timeout period (the socket's
/// configured `ML_DIST_HEARTBEAT_PERIOD`) so the coordinator can distinguish
/// a live-but-idle worker from a dead one. Returns None when the peer closed
/// the connection or IO failed.
pub(crate) fn dist_worker_read_frame(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> Option<(u8, Vec<u8>)> {
    loop {
        if let Some((msg_type, consumed)) = dist_decode_frame(buffer) {
            let payload = buffer[ML_DIST_FRAME_HEADER_LEN..consumed].to_vec();
            buffer.drain(..consumed);
            return Some((msg_type, payload));
        }
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk) {
            Ok(0) => return None,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                // Read timeout elapsed with no inbound frame: prove liveness.
                stream
                    .write_all(&dist_encode_frame(ML_DIST_MSG_HEARTBEAT, &[]))
                    .ok()?;
            }
            Err(_) => return None,
        }
    }
}

// ── DistTCP failure-tolerance & multi-process e2e tests ─────────────────────
// APPEND-ONLY section. Covers: reconnect backoff schedule, coordinator
// tolerance of a mid-ALLREDUCE disconnect with worker retry, typed bounded
// WorkerLost failure (no deadlock), idempotent DONE replay, and the
// multi-process e2e entry point driven by
// .github/workflows/distributed-e2e.yml.
#[cfg(test)]
mod dist_tcp_fault_tests {
    use super::*;
    use std::net::{Ipv4Addr, TcpListener as StdListener};
    use std::process::{Command, Stdio};

    fn fault_spec(worker_count: usize, steps: usize) -> DistTrainSpec {
        DistTrainSpec {
            worker_count,
            steps,
            lr: 0.05,
            features: 4,
            total_samples: 16,
            seed: 7,
        }
    }

    /// Raw test-client socket configured for the tick-based frame reader.
    fn raw_client(port: u16) -> TcpStream {
        let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connect");
        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(ML_DIST_HEARTBEAT_PERIOD));
        stream
    }

    fn raw_hello(stream: &mut TcpStream, worker_id: usize) {
        stream.write_all(&dist_encode_hello(worker_id)).expect("hello write");
    }

    fn raw_expect_frame(stream: &mut TcpStream, buf: &mut Vec<u8>, want: u8) -> Vec<u8> {
        let (msg_type, payload) =
            dist_worker_read_frame(stream, buf).expect("frame from coordinator");
        assert_eq!(msg_type, want);
        payload
    }

    fn raw_gradients(loss: f64, step: i64) -> Vec<u8> {
        let w_grad: Vec<f64> = (0..4).map(|i| loss + i as f64 * 0.25).collect();
        dist_encode_gradients_at_step(loss, step, &w_grad, &[loss])
    }

    #[test]
    fn dist_backoff_schedule_doubles_and_caps_at_two_seconds() {
        assert_eq!(dist_backoff_delay(0), Duration::from_millis(100));
        assert_eq!(dist_backoff_delay(1), Duration::from_millis(200));
        assert_eq!(dist_backoff_delay(2), Duration::from_millis(400));
        assert_eq!(dist_backoff_delay(3), Duration::from_millis(800));
        assert_eq!(dist_backoff_delay(4), Duration::from_millis(1600));
        for attempt in 5..=48 {
            assert_eq!(dist_backoff_delay(attempt), Duration::from_millis(2_000));
        }
        // The full reconnect budget must fit inside the default silence
        // window so an honest worker can always reattach.
        let total_ms: u128 = (0..ML_DIST_RECONNECT_ATTEMPTS)
            .map(|attempt| dist_backoff_delay(attempt).as_millis())
            .sum();
        assert!(
            total_ms < DistTiming::default().silence_timeout.as_millis(),
            "reconnect budget {total_ms}ms must beat the silence window"
        );
    }

    #[test]
    fn dist_tcp_coord_survives_mid_allreduce_disconnect_and_reconnect() {
        let std_listener = StdListener::bind("127.0.0.1:0").expect("bind");
        let port = std_listener.local_addr().unwrap().port();
        std_listener.set_nonblocking(true).unwrap();
        let listener = mio::net::TcpListener::from_std(std_listener);
        let handle = std::thread::spawn(move || {
            dist_coord_loop(listener, &fault_spec(2, 1), DistTiming::default(), None)
        });

        let mut buf0 = Vec::new();
        let mut buf1 = Vec::new();
        // Both ranks attach normally.
        let mut w1 = raw_client(port);
        raw_hello(&mut w1, 1);
        raw_expect_frame(&mut w1, &mut buf1, ML_DIST_MSG_ASSIGN);
        let mut w0 = raw_client(port);
        raw_hello(&mut w0, 0);
        raw_expect_frame(&mut w0, &mut buf0, ML_DIST_MSG_ASSIGN);

        // Worker 1 submits its round-1 gradient and stays connected waiting
        // for the ACK; worker 0 dies mid-round without contributing.
        w1.write_all(&raw_gradients(0.5, 1)).expect("gradients w1");
        drop(w0);

        // The retried rank reconnects (what dist_tcp_attach does) and gets a
        // fresh ASSIGN for the interrupted round.
        let mut w0b = raw_client(port);
        raw_hello(&mut w0b, 0);
        raw_expect_frame(&mut w0b, &mut buf0, ML_DIST_MSG_ASSIGN);
        w0b.write_all(&raw_gradients(0.25, 1)).expect("resubmit");

        // Final step ⇒ identical DONE broadcast to every live participant.
        let done1 = raw_expect_frame(&mut w1, &mut buf1, ML_DIST_MSG_DONE);
        let done0 = raw_expect_frame(&mut w0b, &mut buf0, ML_DIST_MSG_DONE);
        assert_eq!(done0, done1);

        let (global_step, last_loss) = handle.join().unwrap().expect("coordination ok");
        assert_eq!(global_step, 1);
        assert!(last_loss.is_finite());
    }

    #[test]
    fn dist_tcp_coord_reports_typed_worker_lost_within_bound() {
        let std_listener = StdListener::bind("127.0.0.1:0").expect("bind");
        let port = std_listener.local_addr().unwrap().port();
        std_listener.set_nonblocking(true).unwrap();
        let listener = mio::net::TcpListener::from_std(std_listener);
        let timing = DistTiming {
            silence_timeout: Duration::from_millis(600),
            deadline: Duration::from_secs(30),
            post_done_grace: Duration::from_secs(1),
        };
        let started = StdInstant::now();
        let handle = std::thread::spawn(move || {
            dist_coord_loop(listener, &fault_spec(2, 3), timing, None)
        });

        let mut buf0 = Vec::new();
        let mut buf1 = Vec::new();
        let mut w0 = raw_client(port);
        raw_hello(&mut w0, 0);
        raw_expect_frame(&mut w0, &mut buf0, ML_DIST_MSG_ASSIGN);
        std::thread::sleep(Duration::from_millis(120));
        let mut w1 = raw_client(port);
        raw_hello(&mut w1, 1);
        raw_expect_frame(&mut w1, &mut buf1, ML_DIST_MSG_ASSIGN);
        w1.write_all(&raw_gradients(0.5, 1)).expect("gradients w1");

        // Both ranks vanish permanently; nobody ever completes the round.
        drop(w0);
        drop(w1);

        let result = handle.join().unwrap();
        let elapsed = started.elapsed();
        assert_eq!(
            result,
            Err(DistFailure::WorkerLost { worker_id: 0 }),
            "typed failure required, got {result:?}"
        );
        // Bounded: far below the deadline; not an instant spurious error.
        assert!(elapsed >= Duration::from_millis(400), "{elapsed:?}");
        assert!(elapsed < Duration::from_secs(10), "no deadlock: {elapsed:?}");
    }

    #[test]
    fn dist_tcp_coord_replays_done_to_worker_that_missed_it() {
        let std_listener = StdListener::bind("127.0.0.1:0").expect("bind");
        let port = std_listener.local_addr().unwrap().port();
        std_listener.set_nonblocking(true).unwrap();
        let listener = mio::net::TcpListener::from_std(std_listener);
        let handle = std::thread::spawn(move || {
            dist_coord_loop(listener, &fault_spec(2, 1), DistTiming::default(), None)
        });

        let mut buf0 = Vec::new();
        let mut buf1 = Vec::new();
        let mut w0 = raw_client(port);
        raw_hello(&mut w0, 0);
        raw_expect_frame(&mut w0, &mut buf0, ML_DIST_MSG_ASSIGN);
        let mut w1 = raw_client(port);
        raw_hello(&mut w1, 1);
        raw_expect_frame(&mut w1, &mut buf1, ML_DIST_MSG_ASSIGN);

        // Round completes normally; both receive DONE.
        w0.write_all(&raw_gradients(0.25, 1)).expect("grad w0");
        w1.write_all(&raw_gradients(0.5, 1)).expect("grad w1");
        let first_done = raw_expect_frame(&mut w0, &mut buf0, ML_DIST_MSG_DONE);
        let done1 = raw_expect_frame(&mut w1, &mut buf1, ML_DIST_MSG_DONE);
        assert_eq!(first_done, done1);

        // Worker 0 resubmits step 1 as if it had never processed the DONE:
        // the coordinator must replay the cached frame idempotently instead
        // of corrupting round state or hanging.
        w0.write_all(&raw_gradients(0.25, 1)).expect("resubmit");
        let replayed = raw_expect_frame(&mut w0, &mut buf0, ML_DIST_MSG_DONE);
        assert_eq!(replayed, first_done);

        let (global_step, _) = handle.join().unwrap().expect("coordination ok");
        assert_eq!(global_step, 1);
    }

    fn e2e_spec() -> DistTrainSpec {
        DistTrainSpec {
            worker_count: 2,
            steps: 4,
            lr: 0.08,
            features: 4,
            total_samples: 24,
            seed: 11,
        }
    }

    /// Worker role executed by a SEPARATE OS PROCESS re-invoking this same
    /// test binary (see the parent test below / CI workflow).
    fn e2e_worker_child() {
        let port: u16 = std::env::var("SPECTRA_DIST_PORT")
            .expect("SPECTRA_DIST_PORT")
            .parse()
            .expect("parse port");
        let worker_id: usize = std::env::var("SPECTRA_DIST_WORKER_ID")
            .expect("SPECTRA_DIST_WORKER_ID")
            .parse()
            .expect("parse worker id");
        let spec = e2e_spec();
        let shard = dist_build_shard(&spec, worker_id);
        let (_, loss_sum, samples) = dist_tcp_worker(
            "127.0.0.1",
            port,
            worker_id,
            shard,
            spec.features,
            spec.steps,
            spec.lr,
            None,
        )
        .expect("worker conversation over real TCP");
        assert!(loss_sum.is_finite());
        assert!(samples > 0);
        println!("dist e2e worker {worker_id}: samples={samples} loss_sum={loss_sum:.6}");
    }

    /// Multi-process e2e: THIS process runs the coordinator event loop while
    /// two separate OS processes re-invoke the compiled test binary with
    /// SPECTRA_DIST_ROLE=worker and drive the byte-level protocol over real
    /// TCP loopback — no shared memory, no threads-as-workers. Used directly
    /// by `.github/workflows/distributed-e2e.yml`; also safe to run locally
    /// via `cargo test -p spectra-runtime --lib dist_tcp_e2e`.
    #[test]
    fn dist_tcp_e2e_two_os_processes_over_real_tcp() {
        // ── Fork-bomb guards ────────────────────────────────────────────
        // A child re-invokes THIS test binary filtered to THIS test with
        // SPECTRA_DIST_ROLE=worker. The role MUST route to the worker path
        // and exit BEFORE any spawn; nesting is otherwise impossible.
        if std::env::var("SPECTRA_DIST_ROLE").as_deref() == Ok("worker") {
            e2e_worker_child();
            std::process::exit(0);
        }
        assert!(
            std::env::var("SPECTRA_DIST_ROLE").is_err(),
            "unknown SPECTRA_DIST_ROLE inside the coordinator process"
        );
        static E2E_SPAWN_BUDGET: std::sync::atomic::AtomicU32 =
            std::sync::atomic::AtomicU32::new(0);
        let spawns = E2E_SPAWN_BUDGET.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert!(
            spawns < 8,
            "e2e spawn budget exceeded — recursive coordinator detected"
        );
        // ────────────────────────────────────────────────────────────────
        // libtest registers names WITHOUT the crate segment, while
        // module_path!() includes it ("spectra_runtime::stdlib::...").
        const QUALIFIED: &str = concat!(
            module_path!(),
            "::dist_tcp_e2e_two_os_processes_over_real_tcp"
        );
        let test_name = match QUALIFIED.split_once("::") {
            Some((_, rest)) => rest,
            None => QUALIFIED,
        };
        let exe = std::env::current_exe().expect("current test binary");

        let std_listener = StdListener::bind("127.0.0.1:0").expect("bind");
        let port = std_listener.local_addr().unwrap().port();
        std_listener.set_nonblocking(true).unwrap();
        let listener = mio::net::TcpListener::from_std(std_listener);
        println!("dist e2e: spawning children with filter {test_name:?} exe {}", exe.display());

        let log_dir = std::env::temp_dir().join(format!(
            "spectra_dist_e2e_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&log_dir).expect("create log dir");

        let mut children = Vec::new();
        for worker_id in 0..e2e_spec().worker_count {
            let log = std::fs::File::create(log_dir.join(format!("worker{worker_id}.log")))
                .expect("create worker log");
            let err_log = log.try_clone().expect("clone log handle");
            children.push(
                Command::new(&exe)
                    .args([test_name, "--exact", "--nocapture"])
                    .env("SPECTRA_DIST_ROLE", "worker")
                    .env("SPECTRA_DIST_PORT", port.to_string())
                    .env("SPECTRA_DIST_WORKER_ID", worker_id.to_string())
                    .stdout(log)
                    .stderr(err_log)
                    .stdin(Stdio::null())
                    .spawn()
                    .expect("spawn worker process"),
            );
        }

        // Coordinator side: bounded by the global deadline inside the loop.
        let (global_step, last_loss) =
            dist_coord_loop(listener, &e2e_spec(), DistTiming::default(), None)
                .expect("multi-process coordination over TCP");
        assert_eq!(global_step, e2e_spec().steps as i64);
        assert!(last_loss.is_finite());

        // Every child process must exit clean within a hard cap — never hang.
        let cap = StdInstant::now() + Duration::from_secs(60);
        for (worker_id, mut child) in children.into_iter().enumerate() {
            loop {
                match child.try_wait().expect("poll worker process") {
                    Some(status) => {
                        assert!(status.success(), "worker {worker_id} failed: {status}");
                        break;
                    }
                    None if StdInstant::now() > cap => {
                        let _ = child.kill();
                        panic!("worker {worker_id} hung past the hard cap");
                    }
                    None => std::thread::sleep(Duration::from_millis(50)),
                }
            }
        }
        println!("dist e2e worker logs: {}", log_dir.display());
    }

    // ── HELLO shared-secret authentication (APPEND-ONLY) ────────────────
    // Covers the optional SPECTRA_DIST_TOKEN contract: tokenized HELLO
    // round-trip, typed Auth rejection of a wrong/missing token, and a full
    // tokened training run. The multi-node container entrypoint at the bottom
    // is driven by .github/workflows/distributed-multi-node.yml.

    #[test]
    fn dist_hello_token_roundtrip_and_malformed_rejection() {
        // Tokenless encoding stays byte-identical to v1 (8-byte payload).
        assert_eq!(dist_encode_hello(3).len(), 9 + 8);
        let tokened = dist_encode_hello_tokenized(2, Some("s3cret"));
        let hello = dist_decode_hello_full(&tokened[ML_DIST_FRAME_HEADER_LEN..])
            .expect("decode tokenized hello");
        assert_eq!(hello.version, ML_DIST_PROTOCOL_VERSION);
        assert_eq!(hello.worker_id, 2);
        assert_eq!(hello.token.as_deref(), Some("s3cret"));

        // Truncated token suffix ⇒ malformed, not a panic or short read.
        let truncated = &tokened[..tokened.len() - 2];
        assert!(dist_decode_hello_full(&truncated[ML_DIST_FRAME_HEADER_LEN..]).is_none());
        // Oversized declared length ⇒ malformed.
        let mut bloated = Vec::new();
        dist_push_u32(&mut bloated, ML_DIST_PROTOCOL_VERSION);
        dist_push_u32(&mut bloated, 0);
        dist_push_u32(&mut bloated, (ML_DIST_MAX_TOKEN_LEN + 1) as u32);
        assert!(dist_decode_hello_full(&bloated).is_none());
    }

    fn raw_hello_tokenized(stream: &mut TcpStream, worker_id: usize, token: Option<&str>) {
        stream
            .write_all(&dist_encode_hello_tokenized(worker_id, token))
            .expect("hello write");
    }

    #[test]
    fn dist_tcp_coord_rejects_wrong_and_missing_tokens_with_typed_auth() {
        for presented in [Some("wrong-token"), None] {
            let std_listener = StdListener::bind("127.0.0.1:0").expect("bind");
            let port = std_listener.local_addr().unwrap().port();
            std_listener.set_nonblocking(true).unwrap();
            let listener = mio::net::TcpListener::from_std(std_listener);
            let handle = std::thread::spawn(move || {
                dist_coord_loop(
                    listener,
                    &fault_spec(1, 1),
                    DistTiming::default(),
                    Some("right-token"),
                )
            });
            let mut impostor = raw_client(port);
            raw_hello_tokenized(&mut impostor, 0, presented);
            // No ASSIGN may ever reach an unauthenticated peer; the run fails
            // typed instead of hanging on the silent slot.
            let mut buf = Vec::new();
            assert!(
                dist_worker_read_frame(&mut impostor, &mut buf).is_none(),
                "unauthenticated peer must not receive shard data"
            );
            assert_eq!(
                handle.join().unwrap(),
                Err(DistFailure::Auth),
                "presented {presented:?} must yield the typed Auth failure"
            );
        }
    }

    #[test]
    fn dist_tcp_coord_with_matching_token_completes_training() {
        let std_listener = StdListener::bind("127.0.0.1:0").expect("bind");
        let port = std_listener.local_addr().unwrap().port();
        std_listener.set_nonblocking(true).unwrap();
        let listener = mio::net::TcpListener::from_std(std_listener);
        let handle = std::thread::spawn(move || {
            dist_coord_loop(
                listener,
                &fault_spec(2, 1),
                DistTiming::default(),
                Some("shared-secret"),
            )
        });
        let mut buf0 = Vec::new();
        let mut buf1 = Vec::new();
        let mut w0 = raw_client(port);
        raw_hello_tokenized(&mut w0, 0, Some("shared-secret"));
        raw_expect_frame(&mut w0, &mut buf0, ML_DIST_MSG_ASSIGN);
        let mut w1 = raw_client(port);
        raw_hello_tokenized(&mut w1, 1, Some("shared-secret"));
        raw_expect_frame(&mut w1, &mut buf1, ML_DIST_MSG_ASSIGN);
        w0.write_all(&raw_gradients(0.25, 1)).expect("grad w0");
        w1.write_all(&raw_gradients(0.5, 1)).expect("grad w1");
        let done0 = raw_expect_frame(&mut w0, &mut buf0, ML_DIST_MSG_DONE);
        let done1 = raw_expect_frame(&mut w1, &mut buf1, ML_DIST_MSG_DONE);
        assert_eq!(done0, done1);
        let (global_step, last_loss) = handle.join().unwrap().expect("coordination ok");
        assert_eq!(global_step, 1);
        assert!(last_loss.is_finite());
    }

    // ── multi-node deployment entrypoint ─────────────────────────────────
    // One test binary, two roles selected purely by environment:
    //   SPECTRA_DIST_ROLE=coordinator — bind SPECTRA_DIST_BIND:SPECTRA_DIST_PORT
    //     (CI passes SPECTRA_DIST_BIND=0.0.0.0) and run one real training
    //     session against whatever remote workers attach.
    //   SPECTRA_DIST_ROLE=worker — attach to SPECTRA_DIST_HOST:SPECTRA_DIST_PORT
    //     and drive the full conversation from inside another network
    //     namespace/container.
    // Without SPECTRA_DIST_ROLE the test self-verifies both roles over
    // loopback with a token, so plain `cargo test` stays green.
    fn multi_node_spec() -> DistTrainSpec {
        DistTrainSpec {
            worker_count: 1,
            steps: 10,
            lr: 0.05,
            features: 4,
            total_samples: 16,
            seed: 42,
        }
    }

    fn env_token() -> Option<String> {
        std::env::var("SPECTRA_DIST_TOKEN")
            .ok()
            .filter(|token| !token.is_empty())
    }

    fn multi_node_coordinator_role() {
        let bind = std::env::var("SPECTRA_DIST_BIND")
            .ok()
            .filter(|bind| !bind.trim().is_empty())
            .unwrap_or_else(|| "127.0.0.1".to_owned());
        let port: u16 = std::env::var("SPECTRA_DIST_PORT")
            .expect("SPECTRA_DIST_PORT")
            .parse()
            .expect("parse SPECTRA_DIST_PORT");
        let addr: std::net::SocketAddr = format!("{bind}:{port}")
            .parse()
            .expect("coordinator bind address");
        let listener =
            mio::net::TcpListener::bind(addr).expect("coordinator bind succeeded");
        println!("dist multi-node coordinator listening on {addr}");
        let spec = multi_node_spec();
        let (global_step, last_loss) =
            dist_coord_loop(listener, &spec, DistTiming::default(), env_token().as_deref())
                .expect("multi-node coordination over real cross-namespace TCP");
        assert_eq!(global_step, spec.steps as i64);
        assert!(last_loss.is_finite());
        println!("dist multi-node coordinator done: step={global_step} loss={last_loss:.6}");
    }

    fn multi_node_worker_role() {
        let host = std::env::var("SPECTRA_DIST_HOST")
            .ok()
            .filter(|host| !host.trim().is_empty())
            .unwrap_or_else(|| "127.0.0.1".to_owned());
        let port: u16 = std::env::var("SPECTRA_DIST_PORT")
            .expect("SPECTRA_DIST_PORT")
            .parse()
            .expect("parse SPECTRA_DIST_PORT");
        let worker_id: usize = std::env::var("SPECTRA_DIST_WORKER_ID")
            .ok()
            .and_then(|id| id.parse().ok())
            .unwrap_or(0);
        let spec = multi_node_spec();
        let shard = dist_build_shard(&spec, worker_id);
        let (_, loss_sum, samples) = dist_tcp_worker(
            &host,
            port,
            worker_id,
            shard,
            spec.features,
            spec.steps,
            spec.lr,
            env_token().as_deref(),
        )
        .expect("worker conversation over real cross-namespace TCP");
        assert!(loss_sum.is_finite());
        assert!(samples > 0);
        println!("dist multi-node worker {worker_id}@{host}:{port}: samples={samples} loss_sum={loss_sum:.6}");
    }

    #[test]
    fn dist_tcp_multi_node_container_entrypoint() {
        match std::env::var("SPECTRA_DIST_ROLE").as_deref() {
            Ok("coordinator") => {
                multi_node_coordinator_role();
                std::process::exit(0);
            }
            Ok("worker") => {
                multi_node_worker_role();
                std::process::exit(0);
            }
            other => {
                assert!(
                    other.unwrap_or_default().is_empty(),
                    "unknown SPECTRA_DIST_ROLE: {other:?}"
                );
            }
        }

        // Default in-process proof of the exact CI topology: coordinator on a
        // tokened listener, worker client connecting by host string, real
        // training end-to-end.
        const LOCAL_TOKEN: &str = "in-process-topology-proof";
        let std_listener = StdListener::bind("127.0.0.1:0").expect("bind");
        let port = std_listener.local_addr().unwrap().port();
        std_listener.set_nonblocking(true).unwrap();
        let listener = mio::net::TcpListener::from_std(std_listener);
        let coord = std::thread::spawn(move || {
            dist_coord_loop(
                listener,
                &multi_node_spec(),
                DistTiming::default(),
                Some(LOCAL_TOKEN),
            )
        });
        let worker = std::thread::spawn(move || {
            let spec = multi_node_spec();
            let shard = dist_build_shard(&spec, 0);
            dist_tcp_worker(
                "127.0.0.1",
                port,
                0,
                shard,
                spec.features,
                spec.steps,
                spec.lr,
                Some(LOCAL_TOKEN),
            )
        });
        let (_, loss_sum, samples) = worker.join().unwrap().expect("worker ok");
        let (global_step, _) = coord.join().unwrap().expect("coord ok");
        assert_eq!(global_step, multi_node_spec().steps as i64);
        assert!(loss_sum.is_finite() && samples > 0);
    }
}
