use super::*;

use crate::ir::{Type, Value};

use std::collections::{BTreeMap, HashMap, HashSet};
pub(crate) struct FusedTensorNode {
    pub(crate) old_node_ids: Vec<usize>,
    pub(crate) node: TensorGraphNode,
}

impl TensorGraphNode {
    pub(crate) fn with_id(mut self, id: usize) -> Self {
        self.id = id;
        self
    }
}

impl TensorGraphError {
    pub(crate) fn new(
        function: &str,
        node: Option<usize>,
        kind: TensorGraphErrorKind,
        message: String,
    ) -> Self {
        Self {
            function: function.to_string(),
            node,
            kind,
            message,
        }
    }
}

#[derive(Default)]
pub(crate) struct TensorGraphExtractor {
    pub(crate) nodes: Vec<TensorGraphNode>,
    pub(crate) value_to_node: HashMap<usize, usize>,
    pub(crate) constants: HashMap<usize, i64>,
}

impl TensorGraphExtractor {
    pub(crate) fn fold_const_int(
        &mut self,
        result: Value,
        lhs: Value,
        rhs: Value,
        op: impl FnOnce(i64, i64) -> i64,
    ) {
        self.fold_const_int_checked(result, lhs, rhs, |left, right| Some(op(left, right)));
    }

    pub(crate) fn fold_const_int_checked(
        &mut self,
        result: Value,
        lhs: Value,
        rhs: Value,
        op: impl FnOnce(i64, i64) -> Option<i64>,
    ) {
        if let (Some(left), Some(right)) =
            (self.constants.get(&lhs.id), self.constants.get(&rhs.id))
        {
            if let Some(value) = op(*left, *right) {
                self.constants.insert(result.id, value);
            }
        }
    }

    pub(crate) fn lower_host_call(
        &mut self,
        block: usize,
        instruction: usize,
        result: Option<Value>,
        host: &str,
        args: &[Value],
    ) {
        if !is_tensor_host(host) {
            return;
        }
        let Some(value) = result else {
            return;
        };
        let Some(op) = classify_host(host, args, &self.constants) else {
            return;
        };
        let tensor_positions = tensor_arg_positions(host);
        let inputs = tensor_positions
            .iter()
            .filter_map(|position| args.get(*position).copied())
            .map(|arg| self.node_for_input(arg, block, instruction))
            .collect::<Vec<_>>();
        let output = infer_output_metadata(&op, &inputs, &self.nodes, args, &self.constants);
        let id = self.nodes.len();
        self.nodes.push(TensorGraphNode {
            id,
            value: Some(value.id),
            op,
            inputs,
            output,
            source: TensorGraphSource {
                block,
                instruction,
                host: Some(host.to_string()),
            },
        });
        self.value_to_node.insert(value.id, id);
    }

    pub(crate) fn node_for_input(&mut self, value: Value, block: usize, instruction: usize) -> usize {
        if let Some(node_id) = self.value_to_node.get(&value.id) {
            return *node_id;
        }
        let id = self.nodes.len();
        self.nodes.push(TensorGraphNode {
            id,
            value: Some(value.id),
            op: TensorGraphOp::Parameter,
            inputs: Vec::new(),
            output: TensorMetadata::unknown(),
            source: TensorGraphSource {
                block,
                instruction,
                host: None,
            },
        });
        self.value_to_node.insert(value.id, id);
        id
    }
}

impl TensorGraphOp {
    pub fn stable_name(&self) -> String {
        match self {
            TensorGraphOp::Parameter => "param".to_string(),
            TensorGraphOp::Create { name } => format!("create.{name}"),
            TensorGraphOp::Reshape => "reshape".to_string(),
            TensorGraphOp::Transpose => "transpose".to_string(),
            TensorGraphOp::Matmul => "matmul".to_string(),
            TensorGraphOp::BatchedMatmul => "matmul_batched".to_string(),
            TensorGraphOp::Elementwise { name } => format!("elementwise.{name}"),
            TensorGraphOp::FusedElementwise { ops } => {
                format!("fused_elementwise.{}", ops.join("+"))
            }
            TensorGraphOp::Reduction { name } => format!("reduction.{name}"),
            TensorGraphOp::FusedReduction {
                elementwise_ops,
                reduction,
            } => {
                format!(
                    "fused_reduction.{}->{}",
                    elementwise_ops.join("+"),
                    reduction
                )
            }
            TensorGraphOp::DeviceTransfer { target } => {
                format!("to_device.{}", target.stable_name())
            }
            TensorGraphOp::Linear => "linear".to_string(),
            TensorGraphOp::Conv2d => "conv2d".to_string(),
            TensorGraphOp::Dropout => "dropout".to_string(),
            TensorGraphOp::MaxPool2d => "max_pool2d".to_string(),
            TensorGraphOp::Loss { name } => format!("loss.{name}"),
            TensorGraphOp::UnknownHost { host } => format!("unknown.{host}"),
        }
    }
}

impl TensorMetadata {
    pub fn new(dtype: TensorDType, shape: TensorShape, device: TensorDevice) -> Self {
        Self {
            dtype,
            shape,
            layout: TensorLayout::Contiguous,
            device,
        }
    }

    pub fn unknown() -> Self {
        Self {
            dtype: TensorDType::Unknown,
            shape: TensorShape::Unknown,
            layout: TensorLayout::Unknown,
            device: TensorDevice::Unknown,
        }
    }

    pub(crate) fn stable_name(&self) -> String {
        format!(
            "dtype={} shape={} layout={} device={}",
            self.dtype.stable_name(),
            self.shape.stable_name(),
            self.layout.stable_name(),
            self.device.stable_name()
        )
    }
}

impl TensorDType {
    pub(crate) fn stable_name(self) -> &'static str {
        match self {
            TensorDType::Int => "int",
            TensorDType::Float => "float",
            TensorDType::Unknown => "?",
        }
    }
}

impl TensorShape {
    pub fn rank(&self) -> Option<usize> {
        match self {
            TensorShape::Ranked(dims) => Some(dims.len()),
            TensorShape::Unknown => None,
        }
    }

    pub fn dim(&self, index: usize) -> Option<usize> {
        match self {
            TensorShape::Ranked(dims) => dims.get(index).copied().flatten(),
            TensorShape::Unknown => None,
        }
    }

    pub(crate) fn compatible_with(&self, other: &Self) -> bool {
        match (self, other) {
            (TensorShape::Unknown, _) | (_, TensorShape::Unknown) => true,
            (TensorShape::Ranked(left), TensorShape::Ranked(right)) => {
                let rank = left.len().max(right.len());
                (0..rank).all(|offset| {
                    let l = left.iter().rev().nth(offset).copied().flatten();
                    let r = right.iter().rev().nth(offset).copied().flatten();
                    match (l, r) {
                        (Some(l), Some(r)) => l == r || l == 1 || r == 1,
                        _ => true,
                    }
                })
            }
        }
    }

    pub(crate) fn stable_name(&self) -> String {
        match self {
            TensorShape::Unknown => "?".to_string(),
            TensorShape::Ranked(dims) => format!(
                "[{}]",
                dims.iter()
                    .map(|dim| dim
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "?".to_string()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
}

impl TensorLayout {
    pub(crate) fn stable_name(&self) -> &'static str {
        match self {
            TensorLayout::Contiguous => "contiguous",
            TensorLayout::Unknown => "?",
        }
    }
}

impl TensorDevice {
    pub(crate) fn stable_name(&self) -> String {
        match self {
            TensorDevice::Cpu => "cpu".to_string(),
            TensorDevice::Wgpu => "wgpu".to_string(),
            TensorDevice::Reserved(code) => format!("reserved{code}"),
            TensorDevice::Unknown => "?".to_string(),
        }
    }
}

pub(crate) fn is_tensor_host(host: &str) -> bool {
    host.starts_with("spectra.std.tensor.") || host.starts_with("spectra.std.ml.")
}

pub(crate) fn classify_host(
    host: &str,
    args: &[Value],
    constants: &HashMap<usize, i64>,
) -> Option<TensorGraphOp> {
    let name = host
        .strip_prefix("spectra.std.tensor.")
        .or_else(|| host.strip_prefix("spectra.std.ml."))?;
    match name {
        "zeros" | "ones" | "full" | "full_f" | "arange" | "zeros2" | "ones2" | "full2"
        | "full2_f" | "uniform" | "uniform_f" | "normal_f" | "bernoulli" => {
            Some(TensorGraphOp::Create {
                name: name.to_string(),
            })
        }
        "reshape" => Some(TensorGraphOp::Reshape),
        "transpose" => Some(TensorGraphOp::Transpose),
        "matmul" => Some(TensorGraphOp::Matmul),
        "matmul_batched" => Some(TensorGraphOp::BatchedMatmul),
        "add" | "sub" | "mul" | "div" | "neg" | "relu" | "sigmoid_f" | "tanh_f" | "sqrt_f"
        | "log_f" => Some(TensorGraphOp::Elementwise {
            name: name.to_string(),
        }),
        "sum_t" => Some(TensorGraphOp::Reduction {
            name: name.to_string(),
        }),
        "to_device" => {
            let device = args
                .get(1)
                .and_then(|value| constants.get(&value.id))
                .map(|code| device_from_code(*code))
                .unwrap_or(TensorDevice::Unknown);
            Some(TensorGraphOp::DeviceTransfer { target: device })
        }
        "cpu" => Some(TensorGraphOp::DeviceTransfer {
            target: TensorDevice::Cpu,
        }),
        "linear" => Some(TensorGraphOp::Linear),
        "conv2d" => Some(TensorGraphOp::Conv2d),
        "dropout" => Some(TensorGraphOp::Dropout),
        "max_pool2d" => Some(TensorGraphOp::MaxPool2d),
        "mse_loss" | "bce_loss" => Some(TensorGraphOp::Loss {
            name: name.to_string(),
        }),
        _ if returns_tensor_like(host, name) => Some(TensorGraphOp::UnknownHost {
            host: host.to_string(),
        }),
        _ => None,
    }
}

pub(crate) fn returns_tensor_like(host: &str, name: &str) -> bool {
    host.starts_with("spectra.std.ml.")
        || matches!(
            name,
            "requires_grad" | "grad" | "clone" | "slice" | "concat" | "stack" | "permute"
        )
}

pub(crate) fn tensor_arg_positions(host: &str) -> &'static [usize] {
    let name = host
        .strip_prefix("spectra.std.tensor.")
        .or_else(|| host.strip_prefix("spectra.std.ml."))
        .unwrap_or("");
    match name {
        "reshape" | "transpose" | "sum_t" | "neg" | "relu" | "sigmoid_f" | "tanh_f" | "sqrt_f"
        | "log_f" | "to_device" | "cpu" | "dropout" | "max_pool2d" => &[0],
        "add" | "sub" | "mul" | "div" | "matmul" | "matmul_batched" | "mse_loss" | "bce_loss" => {
            &[0, 1]
        }
        "linear" | "conv2d" => &[0, 1, 2],
        "requires_grad" | "grad" | "clone" | "slice" | "permute" => &[0],
        "concat" | "stack" => &[0, 1],
        _ => &[],
    }
}

pub(crate) fn infer_output_metadata(
    op: &TensorGraphOp,
    inputs: &[usize],
    nodes: &[TensorGraphNode],
    args: &[Value],
    constants: &HashMap<usize, i64>,
) -> TensorMetadata {
    let input = |index: usize| inputs.get(index).and_then(|id| nodes.get(*id));
    match op {
        TensorGraphOp::Create { name } => infer_create_metadata(name, args, constants),
        TensorGraphOp::Reshape => {
            let mut meta = input(0)
                .map(|node| node.output.clone())
                .unwrap_or_else(TensorMetadata::unknown);
            let rows = const_usize(args.get(1), constants);
            let cols = const_usize(args.get(2), constants);
            meta.shape = TensorShape::Ranked(vec![rows, cols]);
            meta
        }
        TensorGraphOp::Transpose => {
            let mut meta = input(0)
                .map(|node| node.output.clone())
                .unwrap_or_else(TensorMetadata::unknown);
            if let TensorShape::Ranked(dims) = &meta.shape {
                if dims.len() == 2 {
                    meta.shape = TensorShape::Ranked(vec![dims[1], dims[0]]);
                }
            }
            meta
        }
        TensorGraphOp::Matmul => {
            let left = input(0).map(|node| &node.output);
            let right = input(1).map(|node| &node.output);
            let dtype = left
                .map(|meta| meta.dtype)
                .filter(|dtype| *dtype != TensorDType::Unknown)
                .or_else(|| right.map(|meta| meta.dtype))
                .unwrap_or(TensorDType::Unknown);
            let device = left
                .map(|meta| meta.device.clone())
                .filter(|device| *device != TensorDevice::Unknown)
                .or_else(|| right.map(|meta| meta.device.clone()))
                .unwrap_or(TensorDevice::Unknown);
            let shape = match (left.map(|meta| &meta.shape), right.map(|meta| &meta.shape)) {
                (Some(TensorShape::Ranked(l)), Some(TensorShape::Ranked(r)))
                    if l.len() == 2 && r.len() == 2 =>
                {
                    TensorShape::Ranked(vec![l[0], r[1]])
                }
                _ => TensorShape::Unknown,
            };
            TensorMetadata::new(dtype, shape, device)
        }
        TensorGraphOp::Reduction { .. } | TensorGraphOp::Loss { .. } => input(0)
            .map(|node| {
                TensorMetadata::new(
                    node.output.dtype,
                    TensorShape::Ranked(Vec::new()),
                    node.output.device.clone(),
                )
            })
            .unwrap_or_else(TensorMetadata::unknown),
        TensorGraphOp::DeviceTransfer { target } => {
            let mut meta = input(0)
                .map(|node| node.output.clone())
                .unwrap_or_else(TensorMetadata::unknown);
            meta.device = target.clone();
            meta
        }
        TensorGraphOp::Elementwise { .. }
        | TensorGraphOp::FusedElementwise { .. }
        | TensorGraphOp::Dropout
        | TensorGraphOp::MaxPool2d
        | TensorGraphOp::BatchedMatmul
        | TensorGraphOp::Linear
        | TensorGraphOp::Conv2d => input(0)
            .map(|node| node.output.clone())
            .unwrap_or_else(TensorMetadata::unknown),
        TensorGraphOp::FusedReduction { .. } => input(0)
            .map(|node| {
                TensorMetadata::new(
                    node.output.dtype,
                    TensorShape::Ranked(Vec::new()),
                    node.output.device.clone(),
                )
            })
            .unwrap_or_else(TensorMetadata::unknown),
        TensorGraphOp::Parameter | TensorGraphOp::UnknownHost { .. } => TensorMetadata::unknown(),
    }
}

pub(crate) fn infer_create_metadata(
    name: &str,
    args: &[Value],
    constants: &HashMap<usize, i64>,
) -> TensorMetadata {
    let dtype = if name.ends_with("_f") || matches!(name, "uniform_f" | "normal_f" | "bernoulli") {
        TensorDType::Float
    } else {
        TensorDType::Int
    };
    let shape = match name {
        "zeros" | "ones" | "full" | "full_f" | "uniform" | "uniform_f" | "normal_f"
        | "bernoulli" => TensorShape::Ranked(vec![const_usize(args.first(), constants)]),
        "arange" => TensorShape::Ranked(vec![infer_arange_len(args, constants)]),
        "zeros2" | "ones2" | "full2" | "full2_f" => TensorShape::Ranked(vec![
            const_usize(args.first(), constants),
            const_usize(args.get(1), constants),
        ]),
        _ => TensorShape::Unknown,
    };
    TensorMetadata::new(dtype, shape, TensorDevice::Cpu)
}

pub(crate) fn infer_arange_len(args: &[Value], constants: &HashMap<usize, i64>) -> Option<usize> {
    let start = args
        .first()
        .and_then(|value| constants.get(&value.id))
        .copied()?;
    let end = args
        .get(1)
        .and_then(|value| constants.get(&value.id))
        .copied()?;
    let step = args
        .get(2)
        .and_then(|value| constants.get(&value.id))
        .copied()?;
    if step == 0 {
        return None;
    }
    let distance = if step > 0 {
        if start >= end {
            return Some(0);
        }
        end.saturating_sub(start)
    } else {
        if start <= end {
            return Some(0);
        }
        start.saturating_sub(end)
    };
    let step_abs = step.unsigned_abs();
    Some(distance.unsigned_abs().div_ceil(step_abs) as usize)
}

pub(crate) fn const_usize(value: Option<&Value>, constants: &HashMap<usize, i64>) -> Option<usize> {
    value
        .and_then(|value| constants.get(&value.id))
        .and_then(|value| usize::try_from(*value).ok())
}

pub(crate) fn device_from_code(code: i64) -> TensorDevice {
    match code {
        0 => TensorDevice::Cpu,
        6 => TensorDevice::Wgpu,
        other => TensorDevice::Reserved(other),
    }
}

pub(crate) fn input_pair<'a>(
    node: &TensorGraphNode,
    by_id: &'a BTreeMap<usize, &TensorGraphNode>,
) -> Option<[&'a TensorGraphNode; 2]> {
    Some([
        by_id.get(node.inputs.first()?)?,
        by_id.get(node.inputs.get(1)?)?,
    ])
}

pub(crate) fn validate_same_device(
    function: &str,
    node: &TensorGraphNode,
    left: &TensorGraphNode,
    right: &TensorGraphNode,
    errors: &mut Vec<TensorGraphError>,
) {
    if left.output.device != TensorDevice::Unknown
        && right.output.device != TensorDevice::Unknown
        && left.output.device != right.output.device
    {
        errors.push(TensorGraphError::new(
            function,
            Some(node.id),
            TensorGraphErrorKind::DeviceMismatch,
            format!(
                "{} expects operands on the same device, got {} and {}",
                node.op.stable_name(),
                left.output.device.stable_name(),
                right.output.device.stable_name()
            ),
        ));
    }
}

pub(crate) fn validate_same_dtype(
    function: &str,
    node: &TensorGraphNode,
    left: &TensorGraphNode,
    right: &TensorGraphNode,
    errors: &mut Vec<TensorGraphError>,
) {
    if left.output.dtype != TensorDType::Unknown
        && right.output.dtype != TensorDType::Unknown
        && left.output.dtype != right.output.dtype
    {
        errors.push(TensorGraphError::new(
            function,
            Some(node.id),
            TensorGraphErrorKind::DtypeMismatch,
            format!(
                "{} expects operands with the same dtype, got {} and {}",
                node.op.stable_name(),
                left.output.dtype.stable_name(),
                right.output.dtype.stable_name()
            ),
        ));
    }
}

pub(crate) fn has_cycle(
    node_id: usize,
    by_id: &BTreeMap<usize, &TensorGraphNode>,
    visiting: &mut HashSet<usize>,
    visited: &mut HashSet<usize>,
) -> bool {
    if visited.contains(&node_id) {
        return false;
    }
    if !visiting.insert(node_id) {
        return true;
    }
    if let Some(node) = by_id.get(&node_id) {
        for input in &node.inputs {
            if has_cycle(*input, by_id, visiting, visited) {
                return true;
            }
        }
    }
    visiting.remove(&node_id);
    visited.insert(node_id);
    false
}

impl From<&Type> for TensorMetadata {
    fn from(ty: &Type) -> Self {
        match ty {
            Type::Tensor {
                dtype,
                rank,
                dims,
                layout,
                device,
            } => {
                let dtype = match dtype.as_ref() {
                    Type::Int => TensorDType::Int,
                    Type::Float => TensorDType::Float,
                    _ => TensorDType::Unknown,
                };
                let shape = dims
                    .clone()
                    .or_else(|| rank.map(|rank| vec![None; rank]))
                    .map(TensorShape::Ranked)
                    .unwrap_or(TensorShape::Unknown);
                let layout = match layout.as_deref() {
                    Some("contiguous") => TensorLayout::Contiguous,
                    _ => TensorLayout::Unknown,
                };
                let device = match device.as_deref() {
                    Some("cpu") => TensorDevice::Cpu,
                    Some("wgpu") => TensorDevice::Wgpu,
                    _ => TensorDevice::Unknown,
                };
                Self {
                    dtype,
                    shape,
                    layout,
                    device,
                }
            }
            _ => Self::unknown(),
        }
    }
}
