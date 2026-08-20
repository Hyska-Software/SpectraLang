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
            inputs: vec![
                MlOnnxValue {
                    name: "input",
                    dtype: "float32",
                    shape: &[1, 4],
                },
                MlOnnxValue {
                    name: "weight",
                    dtype: "float32",
                    shape: &[4, 3],
                },
                MlOnnxValue {
                    name: "bias",
                    dtype: "float32",
                    shape: &[3],
                },
            ],
            outputs: vec![MlOnnxValue {
                name: "output",
                dtype: "float32",
                shape: &[1, 3],
            }],
        }),
        "conv" => Some(MlOnnxModel {
            kind: "conv",
            nodes: vec![MlOnnxNode {
                name: "conv2d",
                op_type: "Conv",
                inputs: &["input", "kernel", "bias"],
                outputs: &["output"],
            }],
            inputs: vec![
                MlOnnxValue {
                    name: "input",
                    dtype: "float32",
                    shape: &[1, 1, 4, 4],
                },
                MlOnnxValue {
                    name: "kernel",
                    dtype: "float32",
                    shape: &[1, 1, 3, 3],
                },
                MlOnnxValue {
                    name: "bias",
                    dtype: "float32",
                    shape: &[1],
                },
            ],
            outputs: vec![MlOnnxValue {
                name: "output",
                dtype: "float32",
                shape: &[1, 1, 2, 2],
            }],
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
        }),
        "normalization" => Some(MlOnnxModel {
            kind: "normalization",
            nodes: vec![MlOnnxNode {
                name: "layer_norm",
                op_type: "LayerNormalization",
                inputs: &["input", "scale", "bias"],
                outputs: &["output"],
            }],
            inputs: vec![
                MlOnnxValue {
                    name: "input",
                    dtype: "float32",
                    shape: &[1, 8],
                },
                MlOnnxValue {
                    name: "scale",
                    dtype: "float32",
                    shape: &[8],
                },
                MlOnnxValue {
                    name: "bias",
                    dtype: "float32",
                    shape: &[8],
                },
            ],
            outputs: vec![MlOnnxValue {
                name: "output",
                dtype: "float32",
                shape: &[1, 8],
            }],
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
                MlOnnxValue {
                    name: "scale",
                    dtype: "float32",
                    shape: &[8],
                },
                MlOnnxValue {
                    name: "bias",
                    dtype: "float32",
                    shape: &[8],
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

fn ml_onnx_model_proto(model: &MlOnnxModel) -> Vec<u8> {
    let mut graph = Vec::new();
    for node in &model.nodes {
        pb_message(1, ml_onnx_node(node), &mut graph);
    }
    pb_string(2, &format!("spectra_{}_graph", model.kind), &mut graph);
    for input in &model.inputs {
        pb_message(11, ml_onnx_value_info(input), &mut graph);
    }
    for output in &model.outputs {
        pb_message(12, ml_onnx_value_info(output), &mut graph);
    }

    let mut opset = Vec::new();
    pb_string(1, "", &mut opset);
    pb_i64(2, 18, &mut opset);

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
