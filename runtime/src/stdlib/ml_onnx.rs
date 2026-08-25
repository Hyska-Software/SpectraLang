#[derive(Clone)]
struct MlOnnxValue {
    name: &'static str,
    dtype: &'static str,
    shape: &'static [i64],
}

#[derive(Clone)]
struct MlOnnxNode {
    name: &'static str,
    op_type: &'static str,
    inputs: &'static [&'static str],
    outputs: &'static [&'static str],
}

#[derive(Clone)]
struct MlOnnxModel {
    kind: &'static str,
    nodes: Vec<MlOnnxNode>,
    inputs: Vec<MlOnnxValue>,
    outputs: Vec<MlOnnxValue>,
    /// Real weights baked into the exported ModelProto so every exported
    /// template is directly executable by an onnxruntime session.
    initializers: Vec<MlOnnxInitializer>,
}

#[derive(Clone)]
struct MlOnnxInitializer {
    name: &'static str,
    shape: &'static [i64],
}

/// FNV-1a seed so every initializer name maps to a stable value stream.
fn ml_onnx_seed(name: &str) -> u64 {
    name.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ byte as u64).wrapping_mul(0x100_0000_01B3)
    })
}

/// Deterministic xorshift64 stream quantized to multiples of 0.25 in
/// [-2.0, 2.0]. Exact reproducibility matters: the inference tests recompute
/// expected outputs from this same stream.
fn ml_onnx_deterministic_values(seed: u64, len: usize) -> Vec<f32> {
    let mut state = seed | 1;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let quantum = (state % 17) as i64 - 8;
        out.push(quantum as f32 * 0.25);
    }
    out
}

impl MlOnnxInitializer {
    fn values(&self) -> Vec<f32> {
        let len = self.shape.iter().product::<i64>() as usize;
        ml_onnx_deterministic_values(ml_onnx_seed(self.name), len)
    }
}
fn pb_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn pb_key(field: u32, wire: u8, out: &mut Vec<u8>) {
    pb_varint(((field << 3) | wire as u32) as u64, out);
}

fn pb_i64(field: u32, value: i64, out: &mut Vec<u8>) {
    pb_key(field, 0, out);
    pb_varint(value as u64, out);
}

fn pb_i32(field: u32, value: i32, out: &mut Vec<u8>) {
    pb_key(field, 0, out);
    pb_varint(value as u64, out);
}

fn pb_string(field: u32, value: &str, out: &mut Vec<u8>) {
    pb_key(field, 2, out);
    pb_varint(value.len() as u64, out);
    out.extend_from_slice(value.as_bytes());
}

fn pb_message(field: u32, payload: Vec<u8>, out: &mut Vec<u8>) {
    pb_key(field, 2, out);
    pb_varint(payload.len() as u64, out);
    out.extend_from_slice(&payload);
}

fn ml_onnx_dimension(value: i64) -> Vec<u8> {
    let mut out = Vec::new();
    pb_i64(1, value, &mut out);
    out
}

fn ml_onnx_shape(shape: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    for dim in shape {
        pb_message(1, ml_onnx_dimension(*dim), &mut out);
    }
    out
}

fn ml_onnx_type(value: &MlOnnxValue) -> Vec<u8> {
    let elem_type = match value.dtype {
        "float32" => 1,
        "int64" => 7,
        _ => 0,
    };
    let mut tensor_type = Vec::new();
    pb_i32(1, elem_type, &mut tensor_type);
    pb_message(2, ml_onnx_shape(value.shape), &mut tensor_type);
    let mut type_proto = Vec::new();
    pb_message(1, tensor_type, &mut type_proto);
    type_proto
}

fn ml_onnx_value_info(value: &MlOnnxValue) -> Vec<u8> {
    let mut out = Vec::new();
    pb_string(1, value.name, &mut out);
    pb_message(2, ml_onnx_type(value), &mut out);
    out
}

fn ml_onnx_node(node: &MlOnnxNode) -> Vec<u8> {
    let mut out = Vec::new();
    for input in node.inputs {
        pb_string(1, input, &mut out);
    }
    for output in node.outputs {
        pb_string(2, output, &mut out);
    }
    pb_string(3, node.name, &mut out);
    pb_string(4, node.op_type, &mut out);
    out
}

fn ml_onnx_model_spec(kind: &str) -> Option<MlOnnxModel> {
    match kind {
        "linear" => Some(MlOnnxModel {
            kind: "linear",
            nodes: vec![MlOnnxNode {
                name: "linear_gemm",
                op_type: "Gemm",
                inputs: &["input", "weight", "bias"],
                outputs: &["output"],
            }],
            inputs: vec![MlOnnxValue {
                name: "input",
                dtype: "float32",
                shape: &[1, 2],
            }],
            outputs: vec![MlOnnxValue {
                name: "output",
                dtype: "float32",
                shape: &[1, 3],
            }],
            initializers: vec![
                MlOnnxInitializer {
                    name: "weight",
                    shape: &[2, 3],
                },
                MlOnnxInitializer {
                    name: "bias",
                    shape: &[3],
                },
            ],
        }),
        "conv" => Some(MlOnnxModel {
            kind: "conv",
            nodes: vec![MlOnnxNode {
                name: "conv2d",
                op_type: "Conv",
                inputs: &["input", "kernel", "bias"],
                outputs: &["output"],
            }],
            inputs: vec![MlOnnxValue {
                name: "input",
                dtype: "float32",
                shape: &[1, 1, 4, 4],
            }],
            outputs: vec![MlOnnxValue {
                name: "output",
                dtype: "float32",
                shape: &[1, 1, 2, 2],
            }],
            initializers: vec![
                MlOnnxInitializer {
                    name: "kernel",
                    shape: &[1, 1, 3, 3],
                },
                MlOnnxInitializer {
                    name: "bias",
                    shape: &[1],
                },
            ],
        }),
        "activation" => Some(MlOnnxModel {
            kind: "activation",
            nodes: vec![MlOnnxNode {
                name: "relu",
                op_type: "Relu",
                inputs: &["input"],
                outputs: &["output"],
            }],
            inputs: vec![MlOnnxValue {
                name: "input",
                dtype: "float32",
                shape: &[1, 8],
            }],
            outputs: vec![MlOnnxValue {
                name: "output",
                dtype: "float32",
                shape: &[1, 8],
            }],
            initializers: vec![],
        }),
        "normalization" => Some(MlOnnxModel {
            kind: "normalization",
            nodes: vec![MlOnnxNode {
                name: "layer_norm",
                op_type: "LayerNormalization",
                inputs: &["input", "scale", "bias"],
                outputs: &["output"],
            }],
            inputs: vec![MlOnnxValue {
                name: "input",
                dtype: "float32",
                shape: &[1, 8],
            }],
            outputs: vec![MlOnnxValue {
                name: "output",
                dtype: "float32",
                shape: &[1, 8],
            }],
            initializers: vec![
                MlOnnxInitializer {
                    name: "scale",
                    shape: &[8],
                },
                MlOnnxInitializer {
                    name: "bias",
                    shape: &[8],
                },
            ],
        }),
        "transformer" => Some(MlOnnxModel {
            kind: "transformer",
            nodes: vec![
                MlOnnxNode {
                    name: "qk",
                    op_type: "MatMul",
                    inputs: &["query", "key"],
                    outputs: &["scores"],
                },
                MlOnnxNode {
                    name: "attention",
                    op_type: "Softmax",
                    inputs: &["scores"],
                    outputs: &["weights"],
                },
                MlOnnxNode {
                    name: "context",
                    op_type: "MatMul",
                    inputs: &["weights", "value"],
                    outputs: &["context"],
                },
                MlOnnxNode {
                    name: "norm",
                    op_type: "LayerNormalization",
                    inputs: &["context", "scale", "bias"],
                    outputs: &["normed"],
                },
                MlOnnxNode {
                    name: "ffn",
                    op_type: "Gelu",
                    inputs: &["normed"],
                    outputs: &["output"],
                },
            ],
            initializers: vec![
                MlOnnxInitializer {
                    name: "scale",
                    shape: &[8],
                },
                MlOnnxInitializer {
                    name: "bias",
                    shape: &[8],
                },
            ],
            inputs: vec![
                MlOnnxValue {
                    name: "query",
                    dtype: "float32",
                    shape: &[1, 4, 8],
                },
                MlOnnxValue {
                    name: "key",
                    dtype: "float32",
                    shape: &[1, 8, 4],
                },
                MlOnnxValue {
                    name: "value",
                    dtype: "float32",
                    shape: &[1, 4, 8],
                },
            ],
            outputs: vec![MlOnnxValue {
                name: "output",
                dtype: "float32",
                shape: &[1, 4, 8],
            }],
        }),
        _ => None,
    }
}

fn ml_onnx_initializer_proto(init: &MlOnnxInitializer) -> Vec<u8> {
    let mut out = Vec::new();
    for dim in init.shape {
        pb_i64(1, *dim, &mut out);
    }
    // TensorProto data_type FLOAT = 1.
    pb_i32(2, 1, &mut out);
    pb_string(8, init.name, &mut out);
    let mut raw = Vec::with_capacity(init.values().len() * 4);
    for value in init.values() {
        raw.extend_from_slice(&value.to_le_bytes());
    }
    pb_message(9, raw, &mut out);
    out
}

fn ml_onnx_model_proto(model: &MlOnnxModel) -> Vec<u8> {
    let mut graph = Vec::new();
    for node in &model.nodes {
        pb_message(1, ml_onnx_node(node), &mut graph);
    }
    pb_string(2, &format!("spectra_{}_graph", model.kind), &mut graph);
    for initializer in &model.initializers {
        pb_message(5, ml_onnx_initializer_proto(initializer), &mut graph);
    }
    for input in &model.inputs {
        pb_message(11, ml_onnx_value_info(input), &mut graph);
    }
    for output in &model.outputs {
        pb_message(12, ml_onnx_value_info(output), &mut graph);
    }

    // Opset 20 is the first standard-opset version that covers Gelu;
    // Gemm/Conv/Relu/Softmax/MatMul/LayerNormalization are all valid there.
    let mut opset = Vec::new();
    pb_string(1, "", &mut opset);
    pb_i64(2, 20, &mut opset);

    let mut out = Vec::new();
    pb_i64(1, 9, &mut out);
    pb_string(2, "SpectraLang", &mut out);
    pb_string(5, "R-1801 ONNX subset", &mut out);
    pb_message(7, graph, &mut out);
    pb_message(8, opset, &mut out);
    out
}

fn pb_read_varint(bytes: &[u8], index: &mut usize) -> Option<u64> {
    let mut shift = 0u32;
    let mut value = 0u64;
    while *index < bytes.len() && shift < 64 {
        let byte = bytes[*index];
        *index += 1;
        value |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
    }
    None
}

fn pb_read_len<'a>(bytes: &'a [u8], index: &mut usize) -> Option<&'a [u8]> {
    let len = pb_read_varint(bytes, index)? as usize;
    let end = index.checked_add(len)?;
    if end > bytes.len() {
        return None;
    }
    let slice = &bytes[*index..end];
    *index = end;
    Some(slice)
}

fn pb_skip(bytes: &[u8], index: &mut usize, wire: u64) -> Option<()> {
    match wire {
        0 => {
            pb_read_varint(bytes, index)?;
            Some(())
        }
        1 => {
            *index = index.checked_add(8)?;
            (*index <= bytes.len()).then_some(())
        }
        2 => {
            pb_read_len(bytes, index)?;
            Some(())
        }
        5 => {
            *index = index.checked_add(4)?;
            (*index <= bytes.len()).then_some(())
        }
        _ => None,
    }
}

fn ml_onnx_node_op_types(node: &[u8], ops: &mut Vec<String>) -> Option<()> {
    let mut index = 0usize;
    while index < node.len() {
        let key = pb_read_varint(node, &mut index)?;
        let field = key >> 3;
        let wire = key & 0x7;
        if field == 4 && wire == 2 {
            let raw = pb_read_len(node, &mut index)?;
            ops.push(String::from_utf8(raw.to_vec()).ok()?);
        } else {
            pb_skip(node, &mut index, wire)?;
        }
    }
    Some(())
}

fn ml_onnx_graph_ops(graph: &[u8], ops: &mut Vec<String>) -> Option<(usize, usize)> {
    let mut index = 0usize;
    let mut inputs = 0usize;
    let mut outputs = 0usize;
    while index < graph.len() {
        let key = pb_read_varint(graph, &mut index)?;
        let field = key >> 3;
        let wire = key & 0x7;
        if wire == 2 {
            let raw = pb_read_len(graph, &mut index)?;
            match field {
                1 => ml_onnx_node_op_types(raw, ops)?,
                11 => inputs += 1,
                12 => outputs += 1,
                _ => {}
            }
        } else {
            pb_skip(graph, &mut index, wire)?;
        }
    }
    Some((inputs, outputs))
}

fn ml_onnx_import_summary_from_bytes(bytes: &[u8]) -> Option<String> {
    let mut index = 0usize;
    let mut ops = Vec::new();
    let mut graph_count = 0usize;
    let mut input_count = 0usize;
    let mut output_count = 0usize;
    let mut opset_seen = false;
    while index < bytes.len() {
        let key = pb_read_varint(bytes, &mut index)?;
        let field = key >> 3;
        let wire = key & 0x7;
        if field == 7 && wire == 2 {
            graph_count += 1;
            let raw = pb_read_len(bytes, &mut index)?;
            let (inputs, outputs) = ml_onnx_graph_ops(raw, &mut ops)?;
            input_count += inputs;
            output_count += outputs;
        } else if field == 8 && wire == 2 {
            opset_seen = true;
            pb_read_len(bytes, &mut index)?;
        } else {
            pb_skip(bytes, &mut index, wire)?;
        }
    }
    if graph_count != 1 || ops.is_empty() || input_count == 0 || output_count == 0 || !opset_seen {
        return None;
    }
    let ops_json = ops
        .iter()
        .map(|op| ml_json_string(op))
        .collect::<Vec<_>>()
        .join(",");
    Some(format!(
        "{{\"schema\":\"spectra.onnx.subset.v1\",\"graphs\":{},\"nodes\":{},\"inputs\":{},\"outputs\":{},\"ops\":[{}],\"dtypes\":[\"float32\"],\"shapes\":\"ranked\"}}",
        graph_count,
        ops.len(),
        input_count,
        output_count,
        ops_json
    ))
}

fn ml_onnx_validate_summary(summary: &str) -> bool {
    summary.contains("\"schema\":\"spectra.onnx.subset.v1\"")
        && summary.contains("\"nodes\":")
        && summary.contains("\"inputs\":")
        && summary.contains("\"outputs\":")
        && summary.contains("\"float32\"")
        && summary.contains("\"ranked\"")
}

#[cfg(feature = "onnx")]
fn ml_onnx_proto_op_types(bytes: &[u8]) -> Option<Vec<String>> {
    let mut index = 0usize;
    while index < bytes.len() {
        let key = pb_read_varint(bytes, &mut index)?;
        let field = key >> 3;
        let wire = key & 0x7;
        if field == 7 && wire == 2 {
            let raw = pb_read_len(bytes, &mut index)?;
            let mut ops = Vec::new();
            ml_onnx_graph_ops(raw, &mut ops)?;
            return Some(ops);
        } else {
            pb_skip(bytes, &mut index, wire)?;
        }
    }
    None
}

#[cfg(feature = "onnx")]
type MlOnnxSessionTable = std::collections::HashMap<u64, ort::session::Session>;

#[cfg(feature = "onnx")]
static ML_ONNX_SESSION_NEXT_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

/// Process-local table of live onnxruntime sessions keyed by handle id.
#[cfg(feature = "onnx")]
static ML_ONNX_SESSIONS: std::sync::LazyLock<Mutex<MlOnnxSessionTable>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

#[cfg(feature = "onnx")]
fn ml_onnx_sessions() -> &'static Mutex<MlOnnxSessionTable> {
    &ML_ONNX_SESSIONS
}

#[cfg(feature = "onnx")]
fn ml_onnx_sessions_lock() -> std::sync::MutexGuard<'static, MlOnnxSessionTable> {
    lock_unpoisoned(ml_onnx_sessions())
}

/// Commits an onnxruntime session from in-memory model bytes and registers
/// it under a fresh process-local handle. Only single-graph-input models are
/// accepted: `spectra.std.ml.onnx_run` feeds one tensor per call.
#[cfg(feature = "onnx")]
fn ml_onnx_commit_session(
    bytes: &[u8],
) -> Result<u64, i32> {
    use ort::session::{builder::GraphOptimizationLevel, Session};
    let builder = Session::builder().map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    let session = builder
        .with_optimization_level(GraphOptimizationLevel::Level1)
        .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?
        .commit_from_memory(bytes)
        .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    if session.inputs().len() != 1 {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let id = ML_ONNX_SESSION_NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ml_onnx_sessions_lock().insert(id, session);
    Ok(id)
}

/// Real metadata inventory from a committed onnxruntime session: input and
/// output names, element dtypes and shapes come from ORT itself, not from
/// template knowledge. Falls back to the protobuf walker in the caller when
/// the model cannot be loaded (still a real parse of the actual bytes).
#[cfg(feature = "onnx")]
fn ml_onnx_real_summary_from_bytes(bytes: &[u8]) -> Option<String> {
    fn describe_outlets(outlets: &[ort::value::Outlet]) -> String {
        outlets
            .iter()
            .map(|outlet| {
                let dtype = match outlet.dtype().tensor_type() {
                    Some(TensorElementType::Float32) => "float32",
                    Some(TensorElementType::Float64) => "float64",
                    Some(TensorElementType::Float16) => "float16",
                    Some(TensorElementType::Int8) => "int8",
                    Some(TensorElementType::Int16) => "int16",
                    Some(TensorElementType::Int32) => "int32",
                    Some(TensorElementType::Int64) => "int64",
                    Some(TensorElementType::Uint8) => "uint8",
                    Some(TensorElementType::Uint16) => "uint16",
                    Some(TensorElementType::Uint32) => "uint32",
                    Some(TensorElementType::Uint64) => "uint64",
                    Some(TensorElementType::Bool) => "bool",
                    Some(TensorElementType::String) => "string",
                    _ => "unknown",
                };
                let dims = outlet
                    .dtype()
                    .tensor_shape()
                    .map(|shape| {
                        shape
                            .iter()
                            .map(|dim| dim.to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default();
                format!(
                    "{{\"name\":{},\"dtype\":\"{}\",\"shape\":[{}]}}",
                    ml_json_string(outlet.name()),
                    dtype,
                    dims
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }
    use ort::session::Session;
    use ort::value::TensorElementType;
    let session = Session::builder()
        .ok()?
        .commit_from_memory(bytes)
        .ok()?;
    let inputs = describe_outlets(session.inputs());
    let outputs = describe_outlets(session.outputs());
    let ops = ml_onnx_proto_op_types(bytes)?;
    let ops_json = ops
        .iter()
        .map(|op| ml_json_string(op))
        .collect::<Vec<_>>()
        .join(",");
    Some(format!(
        "{{\"schema\":\"spectra.onnx.subset.v1\",\"graphs\":1,\"nodes\":{},\"inputs\":{},\"outputs\":{},\"ops\":[{}],\"dtypes\":[\"float32\"],\"shapes\":\"ranked\",\"input_details\":[{}],\"output_details\":[{}],\"metadata\":\"onnxruntime-session\"}}",
        ops.len(),
        session.inputs().len(),
        session.outputs().len(),
        ops_json,
        inputs,
        outputs
    ))
}

/// Runs the registered session for `session_id`: maps the stdlib tensor
/// registry entry (f64 storage) into an ORT f32 tensor, executes the graph,
/// extracts `output_name` back into shape + flat f32 data and registers a
/// new stdlib tensor holding the widened result.
#[cfg(feature = "onnx")]
fn ml_onnx_run_inner(
    session_id: u64,
    tensor_handle: usize,
    output_name: &str,
) -> Result<usize, i32> {
    use ort::value::Tensor;
    let (shape, values_f64, _requires_grad) =
        ml_tensor_float_data(tensor_handle).ok_or(HOST_STATUS_NOT_FOUND)?;
    if values_f64.iter().any(|value| !value.is_finite()) {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let data: Vec<f32> = values_f64.iter().map(|value| *value as f32).collect();
    let dims: Vec<i64> = shape.iter().map(|dim| *dim as i64).collect();
    let mut sessions = ml_onnx_sessions_lock();
    let session = sessions.get_mut(&session_id).ok_or(HOST_STATUS_NOT_FOUND)?;
    if session.inputs().len() != 1 {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    // Copy the name out before `run`: the borrowed &str must not overlap
    // with the `&mut session` that inference takes.
    let input_name = session.inputs()[0].name().to_owned();

    let input_value =
        Tensor::from_array((dims, data)).map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    let outputs = session
        .run(ort::inputs![input_name.as_str() => input_value])
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    let output = outputs.get(output_name).ok_or(HOST_STATUS_NOT_FOUND)?;
    let (out_shape, flat) = output
        .try_extract_tensor::<f32>()
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    let out_dims: Vec<usize> = out_shape
        .iter()
        .map(|dim| usize::try_from(*dim).unwrap_or(0))
        .collect();
    let values: Vec<f64> = flat.iter().map(|value| *value as f64).collect();
    ml_alloc_float_tensor(out_dims, values)
}