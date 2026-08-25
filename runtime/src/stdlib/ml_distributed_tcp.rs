// ── DistTCP ──────────────────────────────────────────────────────────────────
// Real data-parallel distributed training. Workers compute local gradients
// with the tensor autograd engine over disjoint data shards, ship them to a
// coordinator over TCP loopback (or through an honest multi-thread channel
// path), and apply the ALLREDUCE-averaged gradient via the SGD optimizer
// update path. There are no simulated workers: every reported loss and step
// count comes from an actual forward/backward pass.

// `HashMap`, `Read`, `Write`, `mpsc`, `Arc`, `Mutex`, atomics and `Duration`
// are already imported at the stdlib module level (see stdlib/mod.rs).
use std::net::{Ipv4Addr, TcpStream};
use std::sync::Barrier;

const ML_DIST_PROTOCOL_MAGIC: [u8; 4] = [0x53, 0x50, 0x44, 0x57]; // b"SPDW"
const ML_DIST_PROTOCOL_VERSION: u32 = 1;
const ML_DIST_FRAME_HEADER_LEN: usize = 9; // magic(4) + type(1) + payload_len(u32 LE)

const ML_DIST_MSG_HELLO: u8 = 1;
const ML_DIST_MSG_ASSIGN: u8 = 2;
const ML_DIST_MSG_GRADIENTS: u8 = 3;
const ML_DIST_MSG_ACK: u8 = 4;
const ML_DIST_MSG_DONE: u8 = 5;

pub(crate) const ML_DISTRIBUTED_TOPOLOGY_TCP: &str = "tcp-workers";
pub(crate) const ML_DISTRIBUTED_TOPOLOGY_MULTITHREAD: &str = "multi-thread";

/// Fully validated specification for one distributed training run.
struct DistTrainSpec {
    worker_count: usize,
    steps: usize,
    lr: f64,
    features: usize,
    total_samples: usize,
    seed: i64,
}

/// Result of a completed run: real per-worker stats plus the global view.
struct DistRunOutcome {
    global_step: i64,
    last_loss: f64,
    workers: Vec<MlDistributedWorker>,
}

// ── wire format ──────────────────────────────────────────────────────────────
//
// frame            := magic[4] type u8 payload_len u32 LE payload[payload_len]
// HELLO payload    := version u32 LE worker_id u32 LE
// ASSIGN payload   := x_tensor ++ y_tensor
// GRADIENTS        := local_loss f64 LE ++ w_grad_tensor ++ b_grad_tensor
// ACK payload      := avg_w_grad_tensor ++ avg_b_grad_tensor
// DONE payload     := global_step i64 LE ++ mean_loss f64 LE
// tensor           := rank u32 LE dims[u32 LE]^rank values[f64 LE]^n

fn dist_encode_frame(msg_type: u8, payload: &[u8]) -> Vec<u8> {
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
fn dist_decode_frame(buffer: &[u8]) -> Option<(u8, usize)> {
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

fn dist_push_u32(payload: &mut Vec<u8>, value: u32) {
    payload.extend_from_slice(&value.to_le_bytes());
}

fn dist_push_f64(payload: &mut Vec<u8>, value: f64) {
    payload.extend_from_slice(&value.to_le_bytes());
}

fn dist_encode_tensor_payload(shape: &[usize], values: &[f64]) -> Vec<u8> {
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

fn dist_read_u32(payload: &[u8], cursor: &mut usize) -> Option<u32> {
    let slice = payload.get(*cursor..*cursor + 4)?;
    *cursor += 4;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn dist_read_f64(payload: &[u8], cursor: &mut usize) -> Option<f64> {
    let slice = payload.get(*cursor..*cursor + 8)?;
    *cursor += 8;
    Some(f64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

fn dist_decode_tensor_payload(payload: &[u8], cursor: &mut usize) -> Option<(Vec<usize>, Vec<f64>)> {
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

fn dist_encode_hello(worker_id: usize) -> Vec<u8> {
    let mut payload = Vec::with_capacity(8);
    dist_push_u32(&mut payload, ML_DIST_PROTOCOL_VERSION);
    dist_push_u32(&mut payload, worker_id as u32);
    dist_encode_frame(ML_DIST_MSG_HELLO, &payload)
}

fn dist_encode_assign(x_shape: &[usize], x: &[f64], y_shape: &[usize], y: &[f64]) -> Vec<u8> {
    let mut payload = dist_encode_tensor_payload(x_shape, x);
    payload.extend_from_slice(&dist_encode_tensor_payload(y_shape, y));
    dist_encode_frame(ML_DIST_MSG_ASSIGN, &payload)
}

fn dist_encode_gradients(loss: f64, w_grad: &[f64], b_grad: &[f64]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(16 + (w_grad.len() + b_grad.len()) * 8);
    dist_push_f64(&mut payload, loss);
    payload.extend_from_slice(&dist_encode_tensor_payload(&[w_grad.len(), 1], w_grad));
    payload.extend_from_slice(&dist_encode_tensor_payload(&[b_grad.len()], b_grad));
    dist_encode_frame(ML_DIST_MSG_GRADIENTS, &payload)
}

fn dist_encode_ack(w_grad: &[f64], b_grad: &[f64]) -> Vec<u8> {
    let mut payload = dist_encode_tensor_payload(&[w_grad.len(), 1], w_grad);
    payload.extend_from_slice(&dist_encode_tensor_payload(&[b_grad.len()], b_grad));
    dist_encode_frame(ML_DIST_MSG_ACK, &payload)
}

fn dist_encode_done(global_step: i64, mean_loss: f64) -> Vec<u8> {
    let mut payload = Vec::with_capacity(16);
    payload.extend_from_slice(&global_step.to_le_bytes());
    dist_push_f64(&mut payload, mean_loss);
    dist_encode_frame(ML_DIST_MSG_DONE, &payload)
}

fn dist_decode_hello(payload: &[u8]) -> Option<(u32, usize)> {
    let mut cursor = 0usize;
    let version = dist_read_u32(payload, &mut cursor)?;
    let worker_id = dist_read_u32(payload, &mut cursor)?;
    Some((version, worker_id as usize))
}

#[derive(Clone)]
struct DistGradients {
    loss: f64,
    w_grad: Vec<f64>,
    b_grad: Vec<f64>,
}

fn dist_decode_gradients(payload: &[u8]) -> Option<DistGradients> {
    let mut cursor = 0usize;
    let loss = dist_read_f64(payload, &mut cursor)?;
    let (w_shape, w_grad) = dist_decode_tensor_payload(payload, &mut cursor)?;
    let (b_shape, b_grad) = dist_decode_tensor_payload(payload, &mut cursor)?;
    if w_shape != vec![w_grad.len(), 1] || b_shape != vec![b_grad.len()] {
        return None;
    }
    Some(DistGradients { loss, w_grad, b_grad })
}

fn dist_decode_ack(payload: &[u8]) -> Option<(Vec<f64>, Vec<f64>)> {
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
fn dist_sample_value(seed: i64, sample: usize, feature: usize) -> f64 {
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

fn dist_true_weight(feature: usize) -> f64 {
    match feature % 4 {
        0 => 0.9,
        1 => 0.6,
        2 => -0.3,
        _ => -0.6,
    }
}

fn dist_target_value(features: usize, row: &[f64]) -> f64 {
    let dot = (0..features)
        .map(|f| dist_true_weight(f) * row[f])
        .sum::<f64>();
    dot + 0.25
}

struct DistShard {
    rows: usize,
    x: Vec<f64>,
    y: Vec<f64>,
}

fn dist_build_shard(spec: &DistTrainSpec, worker_id: usize) -> DistShard {
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

fn dist_init_params(features: usize) -> Result<(usize, usize), i32> {
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
fn dist_local_gradient_step(
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
        w_grad: grads.0,
        b_grad: grads.1,
    })
}

/// Installs the ALLREDUCE-averaged gradient on a parameter (replacing any
/// leftover local gradient) and applies one SGD update through the shared
/// optimizer path (`ml_optimizer_update`).
fn dist_apply_averaged_update(handle: usize, grad: &[f64], lr: f64) -> bool {
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

fn dist_clear_param_grads(weight_handle: usize, bias_handle: usize) {
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
fn dist_average_gradients(parts: &[Option<DistGradients>]) -> Option<(Vec<f64>, Vec<f64>)> {
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

fn dist_parse_spec(args: &[SpectraHostValue]) -> Result<DistTrainSpec, i32> {
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

fn dist_outcome_to_session(
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

fn dist_run_multithread(spec: &DistTrainSpec) -> Result<DistRunOutcome, i32> {
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

const ML_DIST_TOKEN_LISTENER: mio::Token = mio::Token(usize::MAX);
const ML_DIST_POLL_TIMEOUT: Duration = Duration::from_millis(250);
const ML_DIST_DEADLINE: Duration = Duration::from_secs(120);

struct DistConn {
    stream: mio::net::TcpStream,
    read_buf: Vec<u8>,
    write_queue: Vec<u8>,
    write_offset: usize,
    worker_id: Option<usize>,
    finished: bool,
}

impl DistConn {
    fn enqueue(&mut self, frame: Vec<u8>) {
        self.write_queue.extend_from_slice(&frame);
    }

    /// Flush queued bytes; returns Ok(false) while bytes remain queued.
    fn flush(&mut self) -> std::io::Result<bool> {
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
    fn fill(&mut self) -> std::io::Result<()> {
        let mut chunk = [0u8; 8192];
        loop {
            match self.stream.read(&mut chunk) {
                Ok(0) => return Ok(()),
                Ok(read) => self.read_buf.extend_from_slice(&chunk[..read]),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(err) => return Err(err),
            }
        }
    }

    /// Pop one complete frame if present.
    fn pop_frame(&mut self) -> Option<(u8, Vec<u8>)> {
        let (msg_type, consumed) = dist_decode_frame(&self.read_buf)?;
        let payload = self.read_buf[ML_DIST_FRAME_HEADER_LEN..consumed].to_vec();
        self.read_buf.drain(..consumed);
        Some((msg_type, payload))
    }
}

/// Mutable coordination state shared by the event loop and frame handlers.
struct DistCoordState {
    assign_sent: bool,
    pending_gradients: HashMap<usize, DistGradients>,
    round_losses: Vec<f64>,
    current_step: usize,
    done_sent: bool,
}

fn dist_run_tcp(spec: &DistTrainSpec) -> Result<DistRunOutcome, i32> {
    use mio::Interest;

    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().expect("loopback socket address");
    let mut listener =
        mio::net::TcpListener::bind(addr).map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    let port = listener
        .local_addr()
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?
        .port();

    let mut poll = mio::Poll::new().map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    poll.registry()
        .register(&mut listener, ML_DIST_TOKEN_LISTENER, Interest::READABLE)
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;

    let (stats_tx, stats_rx) = mpsc::channel::<Result<(usize, f64, i64), String>>();
    // Workers create their own private parameter replicas on connect.
    let shards: Vec<DistShard> = (0..spec.worker_count)
        .map(|worker_id| dist_build_shard(spec, worker_id))
        .collect();

    let (steps, features, lr) = (spec.steps, spec.features, spec.lr);
    let mut handles = Vec::with_capacity(spec.worker_count);
    for (worker_id, shard) in shards.into_iter().enumerate() {
        let stats_tx = stats_tx.clone();
        handles.push(std::thread::spawn(move || {
            let result = dist_tcp_worker(
                port,
                worker_id,
                shard,
                features,
                steps,
                lr,
            );
            let _ = stats_tx.send(result);
        }));
    }
    drop(stats_tx);

    let mut events = mio::Events::with_capacity(64);
    let mut conns: HashMap<mio::Token, DistConn> = HashMap::new();
    let mut next_token = 0usize;
    let mut state = DistCoordState {
        assign_sent: false,
        pending_gradients: HashMap::new(),
        round_losses: Vec::new(),
        current_step: 0,
        done_sent: false,
    };
    let deadline = StdInstant::now() + ML_DIST_DEADLINE;
    let mut run_error: Option<i32> = None;

    while state.current_step != spec.steps || conns.values().any(|conn| !conn.finished) {
        if StdInstant::now() > deadline {
            run_error = Some(HOST_STATUS_INTERNAL_ERROR);
            break;
        }
        match poll.poll(&mut events, Some(ML_DIST_POLL_TIMEOUT)) {
            Ok(()) => {}
            Err(_) => {
                run_error = Some(HOST_STATUS_INTERNAL_ERROR);
                break;
            }
        }
        for event in events.iter() {
            if event.token() == ML_DIST_TOKEN_LISTENER {
                loop {
                    match listener.accept() {
                        Ok((mut stream, _addr)) => {
                            let token = mio::Token(next_token);
                            next_token += 1;
                            if poll
                                .registry()
                                .register(
                                    &mut stream,
                                    token,
                                    Interest::READABLE.add(Interest::WRITABLE),
                                )
                                .is_err()
                            {
                                run_error = Some(HOST_STATUS_INTERNAL_ERROR);
                                break;
                            }
                            conns.insert(
                                token,
                                DistConn {
                                    stream,
                                    read_buf: Vec::new(),
                                    write_queue: Vec::new(),
                                    write_offset: 0,
                                    worker_id: None,
                                    finished: false,
                                },
                            );
                        }
                        Err(ref err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(_) => {
                            run_error = Some(HOST_STATUS_INTERNAL_ERROR);
                            break;
                        }
                    }
                }
                continue;
            }
            let token = event.token();
            let Some(conn) = conns.get_mut(&token) else {
                continue;
            };
            if event.is_readable() && conn.fill().is_err() {
                conn.finished = true;
                continue;
            }
            if event.is_writable() && matches!(conn.flush(), Err(_)) {
                conn.finished = true;
                continue;
            }
        }
        // Process buffered frames per connection so HELLO and GRADIENTS are
        // attributed to the exact peer that sent them.
        'frames: for token in conns.keys().copied().collect::<Vec<_>>() {
            loop {
                let frame = match conns.get_mut(&token) {
                    Some(conn) => conn.pop_frame(),
                    None => break,
                };
                let Some((msg_type, payload)) = frame else {
                    break;
                };
                let worker_id = match msg_type {
                    ML_DIST_MSG_HELLO => {
                        let Some((version, hello_worker)) = dist_decode_hello(&payload) else {
                            run_error = Some(HOST_STATUS_INVALID_ARGUMENT);
                            break 'frames;
                        };
                        if version != ML_DIST_PROTOCOL_VERSION || hello_worker >= spec.worker_count
                        {
                            run_error = Some(HOST_STATUS_INVALID_ARGUMENT);
                            break 'frames;
                        }
                        if let Some(conn) = conns.get_mut(&token) {
                            if conn.worker_id.is_some() {
                                run_error = Some(HOST_STATUS_INVALID_ARGUMENT);
                                break 'frames;
                            }
                            conn.worker_id = Some(hello_worker);
                        }
                        let all_hello = conns.values().all(|conn| conn.worker_id.is_some());
                        if all_hello && conns.len() == spec.worker_count && !state.assign_sent {
                            state.assign_sent = true;
                            for id in 0..spec.worker_count {
                                let shard = dist_build_shard(spec, id);
                                let assign = dist_encode_assign(
                                    &[shard.rows, spec.features],
                                    &shard.x,
                                    &[shard.rows, 1],
                                    &shard.y,
                                );
                                if let Some(target) =
                                    conns.values_mut().find(|conn| conn.worker_id == Some(id))
                                {
                                    target.enqueue(assign);
                                }
                            }
                        }
                        continue;
                    }
                    ML_DIST_MSG_GRADIENTS => {
                        let sender = conns.get(&token).and_then(|conn| conn.worker_id);
                        let Some(sender) = sender else {
                            run_error = Some(HOST_STATUS_INVALID_ARGUMENT);
                            break 'frames;
                        };
                        sender
                    }
                    _ => {
                        run_error = Some(HOST_STATUS_INVALID_ARGUMENT);
                        break 'frames;
                    }
                };
                // Only GRADIENTS reaches this point; `worker_id` is the sender.
                let Some(gradients) = dist_decode_gradients(&payload) else {
                    run_error = Some(HOST_STATUS_INVALID_ARGUMENT);
                    break 'frames;
                };
                if !state.assign_sent
                    || state.pending_gradients.contains_key(&worker_id)
                    || gradients.w_grad.len() != spec.features
                    || gradients.b_grad.len() != 1
                    || !gradients.loss.is_finite()
                {
                    run_error = Some(HOST_STATUS_INVALID_ARGUMENT);
                    break 'frames;
                }
                state.pending_gradients.insert(worker_id, gradients);
                if state.pending_gradients.len() < spec.worker_count {
                    continue;
                }
                let ordered: Vec<Option<DistGradients>> = (0..spec.worker_count)
                    .map(|id| state.pending_gradients.remove(&id))
                    .collect();
                state.round_losses = ordered
                    .iter()
                    .filter_map(|part| part.as_ref())
                    .map(|g| g.loss)
                    .collect();
                match dist_average_gradients(&ordered) {
                    Some((w_grad, b_grad)) => {
                        state.current_step += 1;
                        if state.current_step >= spec.steps {
                            let mean_loss =
                                state.round_losses.iter().sum::<f64>() / state.round_losses.len() as f64;
                            let done = dist_encode_done(state.current_step as i64, mean_loss);
                            for conn in conns.values_mut() {
                                conn.enqueue(done.clone());
                            }
                            state.done_sent = true;
                        } else {
                            let ack = dist_encode_ack(&w_grad, &b_grad);
                            for conn in conns.values_mut() {
                                conn.enqueue(ack.clone());
                            }
                        }
                    }
                    None => {
                        run_error = Some(HOST_STATUS_INVALID_ARGUMENT);
                    }
                }
                if run_error.is_some() || state.done_sent {
                    break 'frames;
                }
            }
        }
        if run_error.is_some() {
            break;
        }
        // Flush queued writes opportunistically.
        for conn in conns.values_mut() {
            if !conn.write_queue.is_empty() && matches!(conn.flush(), Err(_)) {
                conn.finished = true;
            }
        }
        if state.done_sent {
            for (_, conn) in conns.iter_mut() {
                if conn.write_queue.is_empty() {
                    conn.finished = true;
                }
            }
            conns.retain(|_, conn| !conn.finished);
        }
    }

    drop(conns);

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
            Ok(Err(message)) => {
                worker_error.get_or_insert(if message.starts_with("protocol:") {
                    HOST_STATUS_INVALID_ARGUMENT
                } else {
                    HOST_STATUS_INTERNAL_ERROR
                });
            }
            Err(_) => {
                worker_error.get_or_insert(HOST_STATUS_INTERNAL_ERROR);
            }
        }
    }
    for handle in handles {
        if handle.join().is_err() {
            worker_error.get_or_insert(HOST_STATUS_INTERNAL_ERROR);
        }
    }
    if let Some(code) = run_error.or(worker_error) {
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
        global_step: state.current_step as i64,
        last_loss: state.round_losses.last().copied().unwrap_or(f64::INFINITY),
        workers,
    })
}

/// Blocking worker client: connects to the coordinator, performs the
/// HELLO → ASSIGN → (GRADIENTS ⇄ ACK)* → DONE conversation byte-for-byte.
fn dist_tcp_worker(
    port: u16,
    worker_id: usize,
    shard: DistShard,
    features: usize,
    steps: usize,
    lr: f64,
) -> Result<(usize, f64, i64), String> {
    // Private parameter replica for this rank, as in real data parallelism.
    let (weight_handle, bias_handle) =
        dist_init_params(features).map_err(|code| format!("parameter init failed: {code}"))?;
    let connect = || -> std::io::Result<TcpStream> {
        let mut last_err = None;
        for _ in 0..100 {
            match TcpStream::connect((Ipv4Addr::LOCALHOST, port)) {
                Ok(stream) => return Ok(stream),
                Err(err) => {
                    last_err = Some(err);
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "connect timeout")
        }))
    };
    let mut stream = connect().map_err(|err| format!("connect failed: {err}"))?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
    stream
        .write_all(&dist_encode_hello(worker_id))
        .map_err(|err| format!("hello write failed: {err}"))?;

    let mut read_buf: Vec<u8> = Vec::new();
    let (assign_type, assign_payload) =
        dist_worker_read_frame(&mut stream, &mut read_buf).ok_or_else(|| "protocol: missing ASSIGN".to_string())?;
    if assign_type != ML_DIST_MSG_ASSIGN {
        return Err("protocol: expected ASSIGN".to_string());
    }
    let mut cursor = 0usize;
    let (x_shape, x_values) = dist_decode_tensor_payload(&assign_payload, &mut cursor)
        .ok_or_else(|| "protocol: malformed ASSIGN x tensor".to_string())?;
    let (y_shape, y_values) = dist_decode_tensor_payload(&assign_payload, &mut cursor)
        .ok_or_else(|| "protocol: malformed ASSIGN y tensor".to_string())?;
    if x_shape != vec![shard.rows, features]
        || y_shape != vec![shard.rows, 1]
        || x_values != shard.x
        || y_values != shard.y
    {
        return Err("protocol: ASSIGN shard mismatch".to_string());
    }
    let assigned_shard = DistShard {
        rows: shard.rows,
        x: x_values,
        y: y_values,
    };

    let mut loss_sum = 0.0f64;
    for _ in 0..steps {
        dist_clear_param_grads(weight_handle, bias_handle);
        let gradients = dist_local_gradient_step(&assigned_shard, features, weight_handle, bias_handle)
            .map_err(|code| format!("local gradient step failed: status {code}"))?;
        loss_sum += gradients.loss;
        stream
            .write_all(&dist_encode_gradients(
                gradients.loss,
                &gradients.w_grad,
                &gradients.b_grad,
            ))
            .map_err(|err| format!("gradients write failed: {err}"))?;
        let (reply_type, reply_payload) = dist_worker_read_frame(&mut stream, &mut read_buf)
            .ok_or_else(|| "protocol: connection closed before reply".to_string())?;
        match reply_type {
            ML_DIST_MSG_ACK => {
                let (w_grad, b_grad) = dist_decode_ack(&reply_payload)
                    .ok_or_else(|| "protocol: malformed ACK".to_string())?;
                if !dist_apply_averaged_update(weight_handle, &w_grad, lr)
                    || !dist_apply_averaged_update(bias_handle, &b_grad, lr)
                {
                    return Err("parameter update failed".to_string());
                }
            }
            ML_DIST_MSG_DONE => {
                if reply_payload.len() != 16 {
                    return Err("protocol: malformed DONE".to_string());
                }
                break;
            }
            other => return Err(format!("protocol: unexpected message type {other}")),
        }
    }
    Ok((worker_id, loss_sum, assigned_shard.rows as i64))
}

/// Blocking frame reader used by worker clients: accumulates stream bytes
/// until one complete frame is available, then returns (type, payload).
fn dist_worker_read_frame(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> Option<(u8, Vec<u8>)> {
    loop {
        if let Some((msg_type, consumed)) = dist_decode_frame(buffer) {
            let payload = buffer[ML_DIST_FRAME_HEADER_LEN..consumed].to_vec();
            buffer.drain(..consumed);
            return Some((msg_type, payload));
        }
        let mut chunk = [0u8; 4096];
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}
