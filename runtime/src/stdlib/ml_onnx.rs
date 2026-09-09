use super::*;
#[derive(Clone)]
pub(crate) struct MlOnnxValue {
    pub(crate) name: &'static str,
    pub(crate) dtype: &'static str,
    pub(crate) shape: &'static [i64],
}

#[derive(Clone)]
pub(crate) struct MlOnnxNode {
    pub(crate) name: &'static str,
    pub(crate) op_type: &'static str,
    pub(crate) inputs: &'static [&'static str],
    pub(crate) outputs: &'static [&'static str],
}

#[derive(Clone)]
pub(crate) struct MlOnnxModel {
    pub(crate) kind: &'static str,
    pub(crate) nodes: Vec<MlOnnxNode>,
    pub(crate) inputs: Vec<MlOnnxValue>,
    pub(crate) outputs: Vec<MlOnnxValue>,
    /// Real weights baked into the exported ModelProto so every exported
    /// template is directly executable by an onnxruntime session.
    pub(crate) initializers: Vec<MlOnnxInitializer>,
}

#[derive(Clone)]
pub(crate) struct MlOnnxInitializer {
    pub(crate) name: &'static str,
    pub(crate) shape: &'static [i64],
}

/// FNV-1a seed so every initializer name maps to a stable value stream.
pub(crate) fn ml_onnx_seed(name: &str) -> u64 {
    name.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ byte as u64).wrapping_mul(0x100_0000_01B3)
    })
}

/// Deterministic xorshift64 stream quantized to multiples of 0.25 in
/// [-2.0, 2.0]. Exact reproducibility matters: the inference tests recompute
/// expected outputs from this same stream.
pub(crate) fn ml_onnx_deterministic_values(seed: u64, len: usize) -> Vec<f32> {
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
    pub(crate) fn values(&self) -> Vec<f32> {
        let len = self.shape.iter().product::<i64>() as usize;
        ml_onnx_deterministic_values(ml_onnx_seed(self.name), len)
    }
}
pub(crate) fn pb_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

pub(crate) fn pb_key(field: u32, wire: u8, out: &mut Vec<u8>) {
    pb_varint(((field << 3) | wire as u32) as u64, out);
}

pub(crate) fn pb_i64(field: u32, value: i64, out: &mut Vec<u8>) {
    pb_key(field, 0, out);
    pb_varint(value as u64, out);
}

pub(crate) fn pb_i32(field: u32, value: i32, out: &mut Vec<u8>) {
    pb_key(field, 0, out);
    pb_varint(value as u64, out);
}

pub(crate) fn pb_string(field: u32, value: &str, out: &mut Vec<u8>) {
    pb_key(field, 2, out);
    pb_varint(value.len() as u64, out);
    out.extend_from_slice(value.as_bytes());
}

pub(crate) fn pb_message(field: u32, payload: Vec<u8>, out: &mut Vec<u8>) {
    pb_key(field, 2, out);
    pb_varint(payload.len() as u64, out);
    out.extend_from_slice(&payload);
}

pub(crate) fn ml_onnx_dimension(value: i64) -> Vec<u8> {
    let mut out = Vec::new();
    pb_i64(1, value, &mut out);
    out
}

pub(crate) fn ml_onnx_shape(shape: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    for dim in shape {
        pb_message(1, ml_onnx_dimension(*dim), &mut out);
    }
    out
}

pub(crate) fn ml_onnx_type(value: &MlOnnxValue) -> Vec<u8> {
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

pub(crate) fn ml_onnx_value_info(value: &MlOnnxValue) -> Vec<u8> {
    let mut out = Vec::new();
    pb_string(1, value.name, &mut out);
    pb_message(2, ml_onnx_type(value), &mut out);
    out
}

pub(crate) fn ml_onnx_node(node: &MlOnnxNode) -> Vec<u8> {
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

pub(crate) fn ml_onnx_model_spec(kind: &str) -> Option<MlOnnxModel> {
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
        "dual_linear" => Some(MlOnnxModel {
            kind: "dual_linear",
            nodes: vec![MlOnnxNode {
                name: "dual_add",
                op_type: "Add",
                inputs: &["lhs", "rhs"],
                outputs: &["output"],
            }],
            inputs: vec![
                MlOnnxValue {
                    name: "lhs",
                    dtype: "float32",
                    shape: &[1, 3],
                },
                MlOnnxValue {
                    name: "rhs",
                    dtype: "float32",
                    shape: &[1, 3],
                },
            ],
            outputs: vec![MlOnnxValue {
                name: "output",
                dtype: "float32",
                shape: &[1, 3],
            }],
            initializers: vec![],
        }),
        _ => None,
    }
}

pub(crate) fn ml_onnx_initializer_proto_with_values(init: &MlOnnxInitializer, values: &[f32]) -> Vec<u8> {
    let mut out = Vec::new();
    for dim in init.shape {
        pb_i64(1, *dim, &mut out);
    }
    // TensorProto data_type FLOAT = 1.
    pb_i32(2, 1, &mut out);
    pb_string(8, init.name, &mut out);
    let mut raw = Vec::with_capacity(values.len() * 4);
    for value in values {
        raw.extend_from_slice(&value.to_le_bytes());
    }
    pb_message(9, raw, &mut out);
    out
}


pub(crate) fn ml_onnx_model_proto_with_values(model: &MlOnnxModel, values: &[Vec<f32>]) -> Vec<u8> {
    let mut graph = Vec::new();
    for node in &model.nodes {
        pb_message(1, ml_onnx_node(node), &mut graph);
    }
    pb_string(2, &format!("spectra_{}_graph", model.kind), &mut graph);
    for (initializer, live) in model.initializers.iter().zip(values.iter()) {
        pb_message(5, ml_onnx_initializer_proto_with_values(initializer, live), &mut graph);
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

pub(crate) fn ml_onnx_model_proto(model: &MlOnnxModel) -> Vec<u8> {
    let seeded: Vec<Vec<f32>> = model.initializers.iter().map(|init| init.values()).collect();
    ml_onnx_model_proto_with_values(model, &seeded)
}

pub(crate) fn pb_read_varint(bytes: &[u8], index: &mut usize) -> Option<u64> {
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

pub(crate) fn pb_read_len<'a>(bytes: &'a [u8], index: &mut usize) -> Option<&'a [u8]> {
    let len = pb_read_varint(bytes, index)? as usize;
    let end = index.checked_add(len)?;
    if end > bytes.len() {
        return None;
    }
    let slice = &bytes[*index..end];
    *index = end;
    Some(slice)
}

pub(crate) fn pb_skip(bytes: &[u8], index: &mut usize, wire: u64) -> Option<()> {
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

pub(crate) fn ml_onnx_node_op_types(node: &[u8], ops: &mut Vec<String>) -> Option<()> {
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

pub(crate) fn ml_onnx_graph_ops(graph: &[u8], ops: &mut Vec<String>) -> Option<(usize, usize)> {
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

pub(crate) fn ml_onnx_import_summary_from_bytes(bytes: &[u8]) -> Option<String> {
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

pub(crate) fn ml_onnx_validate_summary(summary: &str) -> bool {
    summary.contains("\"schema\":\"spectra.onnx.subset.v1\"")
        && summary.contains("\"nodes\":")
        && summary.contains("\"inputs\":")
        && summary.contains("\"outputs\":")
        && summary.contains("\"float32\"")
        && summary.contains("\"ranked\"")
}

#[cfg(feature = "onnx")]
pub(crate) fn ml_onnx_proto_op_types(bytes: &[u8]) -> Option<Vec<String>> {
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
pub(crate) type MlOnnxSessionTable = std::collections::HashMap<u64, ort::session::Session>;

#[cfg(feature = "onnx")]
pub(crate) static ML_ONNX_SESSION_NEXT_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

/// Process-local table of live onnxruntime sessions keyed by handle id.
#[cfg(feature = "onnx")]
pub(crate) static ML_ONNX_SESSIONS: std::sync::LazyLock<Mutex<MlOnnxSessionTable>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

#[cfg(feature = "onnx")]
pub(crate) fn ml_onnx_sessions() -> &'static Mutex<MlOnnxSessionTable> {
    &ML_ONNX_SESSIONS
}

#[cfg(feature = "onnx")]
pub(crate) fn ml_onnx_sessions_lock() -> std::sync::MutexGuard<'static, MlOnnxSessionTable> {
    lock_unpoisoned(ml_onnx_sessions())
}

/// Commits an onnxruntime session from in-memory model bytes and registers
/// it under a fresh process-local handle. Sessions may carry any number of
/// graph inputs: `spectra.std.ml.onnx_run` keeps serving single-input models
/// while multi-graph-input models go through `spectra.std.ml.onnx_run_multi`,
/// which validates names, order and f32 shapes against the real ORT session.
#[cfg(feature = "onnx")]
pub(crate) fn ml_onnx_commit_session(
    bytes: &[u8],
) -> Result<u64, i32> {
    use ort::session::{builder::GraphOptimizationLevel, Session};
    let builder = Session::builder().map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    let session = builder
        .with_optimization_level(GraphOptimizationLevel::Level1)
        .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?
        .commit_from_memory(bytes)
        .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    let id = ML_ONNX_SESSION_NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ml_onnx_sessions_lock().insert(id, session);
    Ok(id)
}

/// Real metadata inventory from a committed onnxruntime session: input and
/// output names, element dtypes and shapes come from ORT itself, not from
/// template knowledge. Falls back to the protobuf walker in the caller when
/// the model cannot be loaded (still a real parse of the actual bytes).
#[cfg(feature = "onnx")]
pub(crate) fn ml_onnx_real_summary_from_bytes(bytes: &[u8]) -> Option<String> {
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
pub(crate) fn ml_onnx_run_inner(
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

/// Typed failure for `spectra.std.ml.onnx_run_multi`: `status` preserves the
/// host status contract while `message` feeds the tagged `Error` record the
/// binding allocates, so missing names and shape mismatches surface with a
/// code plus a human-readable reason instead of a bare status alone.
#[cfg(feature = "onnx")]
pub(crate) struct MlOnnxMultiRunError {
    pub(crate) status: i32,
    pub(crate) code: SpectraHostValue,
    pub(crate) message: String,
}

#[cfg(feature = "onnx")]
pub(crate) fn ml_onnx_multi_error(status: i32, message: String) -> MlOnnxMultiRunError {
    MlOnnxMultiRunError {
        status,
        code: status as SpectraHostValue,
        message,
    }
}

/// ONNX static-shape compatibility for a supplied feed: ranks must match
/// and every FIXED model dimension (a parseable positive integer) must
/// equal the corresponding provided dim. Symbolic dimensions ('batch',
/// 'seq', '?', ...) carry no static constraint — onnxruntime resolves them
/// against the actual feed — so they accept any concrete value; likewise a
/// model input with no declared shape accepts every feed.
/// Also reused by `onnx_export_weights`, where the "model metadata" is the
/// spec shape for each initializer.
pub(crate) fn ml_onnx_shape_compatible(model_shape: &str, provided_dims: &[i64]) -> bool {
    if model_shape.is_empty() {
        return true;
    }
    let model_dims: Vec<&str> = model_shape.split(',').collect();
    if model_dims.len() != provided_dims.len() {
        return false;
    }
    model_dims
        .iter()
        .zip(provided_dims.iter())
        .all(|(model_dim, provided)| match model_dim.trim().parse::<i64>() {
            // Negative dims also mean "dynamic" in several exporters.
            Ok(fixed) if fixed > 0 => fixed == *provided,
            _ => true,
        })
}

/// Multi-graph-input inference against a committed onnxruntime session:
/// `names` and `tensor_handles` arrive as parallel lists (List<string> /
/// List<int> handles in the binding), are validated one-to-one against the
/// REAL session metadata — input names, full coverage, float32 dtypes and
/// static shapes read straight from ORT — executed together, and the first
/// graph output comes back as a fresh stdlib tensor.
#[cfg(feature = "onnx")]
pub(crate) fn ml_onnx_run_multi_inner(
    session_id: u64,
    names: &[String],
    tensor_handles: &[usize],
) -> Result<usize, MlOnnxMultiRunError> {
    use ort::value::{Tensor, TensorElementType};

    if names.len() != tensor_handles.len() {
        return Err(ml_onnx_multi_error(
            HOST_STATUS_INVALID_ARGUMENT,
            format!(
                "onnx_run_multi received {} input name(s) but {} tensor(s)",
                names.len(),
                tensor_handles.len()
            ),
        ));
    }

    // Narrow every stdlib tensor first (f64 storage -> f32) so the session
    // borrow below only covers metadata reads and the actual run.
    let mut prepared: Vec<(String, Vec<i64>, Vec<f32>)> = Vec::with_capacity(names.len());
    for (name, handle) in names.iter().zip(tensor_handles.iter()) {
        let Some((shape, values_f64, _requires_grad)) = ml_tensor_float_data(*handle) else {
            return Err(ml_onnx_multi_error(
                HOST_STATUS_NOT_FOUND,
                format!("tensor handle {handle} for input '{name}' does not exist"),
            ));
        };
        if values_f64.iter().any(|value| !value.is_finite()) {
            return Err(ml_onnx_multi_error(
                HOST_STATUS_INVALID_ARGUMENT,
                format!("input '{name}' carries non-finite values"),
            ));
        }
        prepared.push((
            name.clone(),
            shape.iter().map(|dim| *dim as i64).collect(),
            values_f64.iter().map(|value| *value as f32).collect(),
        ));
    }

    let mut sessions = ml_onnx_sessions_lock();
    let session =
        sessions
            .get_mut(&session_id)
            .ok_or_else(|| {
                ml_onnx_multi_error(
                    HOST_STATUS_NOT_FOUND,
                    format!("session handle {session_id} does not exist"),
                )
            })?;
    if prepared.len() != session.inputs().len() {
        let expected = session
            .inputs()
            .iter()
            .map(|inlet| inlet.name())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ml_onnx_multi_error(
            HOST_STATUS_INVALID_ARGUMENT,
            format!(
                "model has {} graph input(s) [{}] but {} were supplied",
                session.inputs().len(),
                expected,
                prepared.len()
            ),
        ));
    }

    // Real metadata inventory from ORT itself: names in graph order, static
    // shapes ("?" stays symbolic and never matches concrete dims) and the
    // float32 element type.
    let mut model_inputs: Vec<(String, String, bool)> = Vec::with_capacity(session.inputs().len());
    for inlet in session.inputs() {
        let is_f32 = inlet.dtype().tensor_type() == Some(TensorElementType::Float32);
        let shape = inlet
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
        model_inputs.push((inlet.name().to_owned(), shape, is_f32));
    }

    // Resolve every requested name against the real metadata; each model
    // input must be fed exactly once (duplicates leave another input unfed
    // and fail below with the missing-name error).
    let mut feed: Vec<Option<usize>> = vec![None; model_inputs.len()];
    for (index, (name, dims, _)) in prepared.iter().enumerate() {
        let position = model_inputs
            .iter()
            .position(|(model_name, _, _)| model_name == name)
            .ok_or_else(|| {
                let expected = model_inputs
                    .iter()
                    .map(|(model_name, _, _)| model_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                ml_onnx_multi_error(
                    HOST_STATUS_NOT_FOUND,
                    format!("unknown graph input '{name}'; model inputs are [{expected}]"),
                )
            })?;
        let (_, model_shape, is_f32) = &model_inputs[position];
        if !*is_f32 {
            return Err(ml_onnx_multi_error(
                HOST_STATUS_INVALID_ARGUMENT,
                format!("graph input '{name}' expects float32 but the model declares another dtype"),
            ));
        }
        let provided = dims
            .iter()
            .map(|dim| dim.to_string())
            .collect::<Vec<_>>()
            .join(",");
        if !ml_onnx_shape_compatible(model_shape, dims) {
            return Err(ml_onnx_multi_error(
                HOST_STATUS_INVALID_ARGUMENT,
                format!(
                    "shape mismatch for input '{name}': got [{provided}], model expects [{model_shape}]"
                ),
            ));
        }
        feed[position] = Some(index);
    }
    for (position, slot) in feed.iter().enumerate() {
        let Some(_) = *slot else {
            return Err(ml_onnx_multi_error(
                HOST_STATUS_NOT_FOUND,
                format!(
                    "missing graph input '{}' in the supplied name list",
                    model_inputs[position].0
                ),
            ));
        };
    }

    let mut inputs: Vec<(String, Tensor<f32>)> = Vec::with_capacity(feed.len());
    for (position, slot) in feed.iter().enumerate() {
        let index = slot.expect("coverage checked above");
        let (_, dims, data) = &prepared[index];
        let tensor = Tensor::from_array((dims.clone(), data.clone()))
            .map_err(|_| {
                ml_onnx_multi_error(
                    HOST_STATUS_INVALID_ARGUMENT,
                    format!("cannot build ORT tensor for input '{}'", model_inputs[position].0),
                )
            })?;
        inputs.push((model_inputs[position].0.clone(), tensor));
    }

    // Copy the output name out before `run`: the borrowed string must not
    // overlap with the `&mut session` that inference takes.
    let output_name = session.outputs()[0].name().to_owned();
    let outputs = session.run(inputs).map_err(|_| {
        ml_onnx_multi_error(
            HOST_STATUS_INTERNAL_ERROR,
            "onnxruntime rejected the multi-input feed".to_string(),
        )
    })?;
    let output = outputs.get(output_name.as_str()).ok_or_else(|| {
        ml_onnx_multi_error(
            HOST_STATUS_NOT_FOUND,
            format!("output '{output_name}' missing from inference results"),
        )
    })?;
    let (out_shape, flat) = output.try_extract_tensor::<f32>().map_err(|_| {
        ml_onnx_multi_error(
            HOST_STATUS_INTERNAL_ERROR,
            format!("output '{output_name}' is not an f32 tensor"),
        )
    })?;
    let out_dims: Vec<usize> = out_shape
        .iter()
        .map(|dim| usize::try_from(*dim).unwrap_or(0))
        .collect();
    let values: Vec<f64> = flat.iter().map(|value| *value as f64).collect();
    ml_alloc_float_tensor(out_dims, values).map_err(|status| {
        ml_onnx_multi_error(
            status,
            "cannot register the multi-input output tensor".to_string(),
        )
    })
}

// ── Symbolic-shape fixture ──────────────────────────────────────────────────

/// Real `.onnx` ModelProto for a single-graph-input Identity model whose
/// input `x` and output `y` are float32 [batch, seq] with SYMBOLIC dim
/// names. Used to prove that `ml_onnx_run_multi` accepts feeds whose
/// concrete dims differ from the symbolic names while still rejecting rank
/// mismatches.
#[cfg(all(test, feature = "onnx"))]
pub(crate) fn ml_onnx_symbolic_identity_proto() -> Vec<u8> {
    let mut identity = Vec::new();
    pb_string(1, "x", &mut identity);
    pb_string(2, "y", &mut identity);
    pb_string(3, "sym_identity", &mut identity);
    pb_string(4, "Identity", &mut identity);

    // Reuses the StatsEmbed fixture helpers (same cfg): symbolic dim params.
    let batch_dim = ml_embed_dim_param("batch");
    let seq_dim = ml_embed_dim_param("seq");

    let mut graph = Vec::new();
    pb_message(1, identity, &mut graph);
    pb_string(2, "spectra_symbolic_identity_graph", &mut graph);
    pb_message(
        11,
        ml_embed_value_info("x", 1, &[batch_dim.clone(), seq_dim.clone()]),
        &mut graph,
    );
    pb_message(12, ml_embed_value_info("y", 1, &[batch_dim, seq_dim]), &mut graph);

    let mut opset = Vec::new();
    pb_string(1, "", &mut opset);
    pb_i64(2, 20, &mut opset);

    let mut out = Vec::new();
    pb_i64(1, 9, &mut out);
    pb_string(2, "SpectraLang", &mut out);
    pb_string(5, "Symbolic-shape identity fixture", &mut out);
    pb_message(7, graph, &mut out);
    pb_message(8, opset, &mut out);
    out
}

#[cfg(all(test, feature = "onnx"))]
mod shape_compat_tests {
    use super::ml_onnx_shape_compatible;

    #[test]
    fn symbolic_dims_accept_any_concrete_feed() {
        assert!(ml_onnx_shape_compatible("batch,seq", &[1, 4]));
        assert!(ml_onnx_shape_compatible("batch,seq", &[8, 128]));
        assert!(ml_onnx_shape_compatible("?,?", &[2, 3]));
    }

    #[test]
    fn fixed_dims_must_match_and_rank_conflicts_fail() {
        assert!(ml_onnx_shape_compatible("1,3", &[1, 3]));
        assert!(!ml_onnx_shape_compatible("1,3", &[2, 3]));
        assert!(!ml_onnx_shape_compatible("batch,seq", &[3]));
        assert!(!ml_onnx_shape_compatible("batch", &[2, 3]));
        // No declared shape accepts everything; negative dims stay dynamic.
        assert!(ml_onnx_shape_compatible("", &[5, 6]));
        assert!(ml_onnx_shape_compatible("-1,3", &[7, 3]));
        assert!(!ml_onnx_shape_compatible("-1,3", &[7, 4]));
    }
}