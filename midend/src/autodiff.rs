//! Compiler-visible reverse-mode autodiff graph.
//!
//! The runtime still owns tensor storage and kernel execution, but this module
//! owns the differentiation contract: which forward operations participate,
//! which values must be retained, and which reverse rule is selected.

use crate::ir::{Function, Instruction, InstructionKind, Module, SourceSpan, Value};
use crate::tensor_graph::{
    TensorGraph, TensorGraphFunction, TensorGraphOp, TensorGraphSource, TensorMetadata,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutodiffGraph {
    pub schema: &'static str,
    pub functions: Vec<AutodiffFunction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutodiffFunction {
    pub name: String,
    pub forward: Vec<AutodiffNode>,
    pub backward: Vec<AutodiffNode>,
    pub loss_nodes: Vec<usize>,
    /// Compatibility accessor for consumers that only support one loss.
    pub loss_node: Option<usize>,
    pub diagnostics: Vec<AutodiffDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutodiffNode {
    pub id: usize,
    pub kind: AutodiffNodeKind,
    pub inputs: Vec<usize>,
    pub output: TensorMetadata,
    pub source: TensorGraphSource,
    pub rule: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutodiffNodeKind {
    Forward { op: String },
    SaveForBackward { forward_node: usize },
    BackwardSeed { loss_node: usize },
    Gradient { target_node: usize, op: String },
    AccumulateGradient { target_node: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutodiffDiagnostic {
    pub code: &'static str,
    pub node: usize,
    pub operation: String,
    pub message: String,
}

/// Replace compiler-generated autodiff adapters with explicit reverse steps.
/// This pass intentionally handles the straight-line SSA tensor graphs used
/// by the first production slice; unsupported control-flow graphs fail before
/// reaching the backend instead of silently delegating to runtime traversal.
pub fn materialize_autodiff_steps(module: &mut Module) -> Result<usize, String> {
    let mut materialized = 0;
    for function in &mut module.functions {
        let definitions = function_host_definitions(function);
        for block_index in 0..function.blocks.len() {
            let original = std::mem::take(&mut function.blocks[block_index].instructions);
            let mut replacement = Vec::new();
            for instruction in original {
                let InstructionKind::HostCall { host, args, .. } = &instruction.kind else {
                    replacement.push(instruction);
                    continue;
                };
                if host != "spectra.compiler.autodiff_region" {
                    replacement.push(instruction);
                    continue;
                }
                let Some(loss) = args.first().copied() else {
                    return Err("E3004: autodiff adapter has no loss operand".to_string());
                };
                let mut steps = Vec::new();
                let mut visiting = HashSet::new();
                materialize_node(
                    function,
                    loss,
                    None,
                    &definitions,
                    instruction.source_span.clone(),
                    &mut visiting,
                    &mut steps,
                )?;
                materialized += steps.len();
                replacement.extend(steps);
            }
            for (id, instruction) in replacement.iter_mut().enumerate() {
                instruction.id = id;
            }
            function.blocks[block_index].instructions = replacement;
        }
    }
    Ok(materialized)
}

type HostDefinition = (String, Vec<Value>, Option<SourceSpan>);

fn function_host_definitions(function: &Function) -> HashMap<usize, HostDefinition> {
    let mut definitions = HashMap::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let InstructionKind::HostCall {
                result: Some(result),
                host,
                args,
                ..
            } = &instruction.kind
            {
                definitions.insert(
                    result.id,
                    (host.clone(), args.clone(), instruction.source_span.clone()),
                );
            }
        }
    }
    definitions
}

fn materialize_node(
    function: &mut Function,
    output: Value,
    upstream: Option<Value>,
    definitions: &HashMap<usize, HostDefinition>,
    source: Option<SourceSpan>,
    visiting: &mut HashSet<usize>,
    steps: &mut Vec<Instruction>,
) -> Result<(), String> {
    if !visiting.insert(output.id) {
        return Err(format!(
            "E3004: cyclic autodiff dependency at value %{}",
            output.id
        ));
    }
    let Some((host, args, node_source)) = definitions.get(&output.id) else {
        visiting.remove(&output.id);
        return Ok(());
    };
    let Some(operation) = autodiff_operation(host) else {
        if host.starts_with("spectra.std.tensor.") || host.starts_with("spectra.std.ml.") {
            if host.ends_with(".to_device") {
                return Err(format!(
                    "E3010: device transfer is not legal inside compiler-native diff ({host})"
                ));
            }
            if host.ends_with(".full") || host.ends_with(".full_i") {
                return Err(format!(
                    "E3006: integer tensor is not differentiable ({host})"
                ));
            }
            if !is_autodiff_leaf_or_auxiliary(Some(host)) {
                return Err(format!(
                    "E3004: operation has no registered reverse kernel ({host})"
                ));
            }
        }
        visiting.remove(&output.id);
        return Ok(());
    };
    let tensor_args = tensor_arguments(host, args);
    if tensor_args.is_empty() {
        visiting.remove(&output.id);
        return Ok(());
    }
    let step_source = node_source.clone().or_else(|| source.clone());
    let effective_upstream = if upstream.is_some() { upstream } else { None };
    steps.push(Instruction {
        id: 0,
        kind: InstructionKind::AutodiffStep {
            result: None,
            operation: format!("grad_apply_{operation}"),
            output,
            upstream: effective_upstream,
            inputs: tensor_args.clone(),
            targets: tensor_args.clone(),
        },
        source_span: step_source.clone(),
    });

    for input in tensor_args {
        if is_autodiff_leaf_or_auxiliary(definitions.get(&input.id).map(|d| d.0.as_str())) {
            continue;
        }
        let grad_handle = function.next_value();
        steps.push(Instruction {
            id: 0,
            kind: InstructionKind::AutodiffStep {
                result: Some(grad_handle),
                operation: "grad_handle".to_string(),
                output: input,
                upstream: None,
                inputs: vec![input],
                targets: vec![],
            },
            source_span: step_source.clone(),
        });
        materialize_node(
            function,
            input,
            Some(grad_handle),
            definitions,
            step_source.clone(),
            visiting,
            steps,
        )?;
    }
    visiting.remove(&output.id);
    Ok(())
}

fn autodiff_operation(host: &str) -> Option<&str> {
    let name = host
        .strip_prefix("spectra.std.tensor.")
        .or_else(|| host.strip_prefix("spectra.std.ml."))?;
    match name {
        "add" | "sub" | "mul" | "div" | "neg" | "relu" | "sum_t" | "mean_t" | "dot_t"
        | "matmul" | "transpose" | "reshape" | "linear" | "mse_loss" => Some(name),
        "exp_f" => Some("exp"),
        "log_f" => Some("log"),
        "sigmoid_f" => Some("sigmoid"),
        _ => None,
    }
}

fn tensor_arguments(host: &str, args: &[Value]) -> Vec<Value> {
    let name = host
        .strip_prefix("spectra.std.tensor.")
        .or_else(|| host.strip_prefix("spectra.std.ml."))
        .unwrap_or("");
    let positions: &[usize] = match name {
        "reshape" | "transpose" | "sum_t" | "neg" | "exp_f" | "log_f" | "relu" | "sigmoid_f" => {
            &[0]
        }
        "add" | "sub" | "mul" | "div" | "matmul" | "dot_t" | "mse_loss" => &[0, 1],
        "linear" => &[0, 1, 2],
        _ => &[],
    };
    positions
        .iter()
        .filter_map(|index| args.get(*index).copied())
        .collect()
}

fn is_autodiff_leaf_or_auxiliary(host: Option<&str>) -> bool {
    match host {
        None => true,
        Some(value) => {
            value.ends_with(".requires_grad")
                || value.ends_with(".full_f")
                || value.ends_with(".full2_f")
                || value.ends_with(".literal_f")
                || value.ends_with(".literal2_f")
        }
    }
}

impl AutodiffGraph {
    pub const SCHEMA: &'static str = "spectralang.r3004_autodiff_ir.v1";

    pub fn from_tensor_graph(graph: &TensorGraph) -> Self {
        Self {
            schema: Self::SCHEMA,
            functions: graph
                .functions
                .iter()
                .map(AutodiffFunction::from_tensor_function)
                .collect(),
        }
    }

    pub fn has_gradient_nodes(&self) -> bool {
        self.functions
            .iter()
            .any(|function| !function.backward.is_empty())
    }

    pub fn stable_dump(&self) -> String {
        let mut out = format!("autodiff_ir schema={}\n", self.schema);
        for function in &self.functions {
            out.push_str(&format!(
                "fn {} losses={:?}\n",
                function.name, function.loss_nodes
            ));
            for node in &function.forward {
                out.push_str(&format!(
                    "  forward %{} {} rule={} inputs={:?}\n",
                    node.id,
                    node_name(&node.kind),
                    node.rule,
                    node.inputs
                ));
            }
            for node in &function.backward {
                out.push_str(&format!(
                    "  backward %{} {} rule={} inputs={:?}\n",
                    node.id,
                    node_name(&node.kind),
                    node.rule,
                    node.inputs
                ));
            }
            for diagnostic in &function.diagnostics {
                out.push_str(&format!(
                    "  diagnostic {} node={} op={} {}\n",
                    diagnostic.code, diagnostic.node, diagnostic.operation, diagnostic.message
                ));
            }
        }
        out
    }
}

impl AutodiffFunction {
    fn from_tensor_function(function: &TensorGraphFunction) -> Self {
        let forward = function
            .nodes
            .iter()
            .map(|node| AutodiffNode {
                id: node.id,
                kind: AutodiffNodeKind::Forward {
                    op: node.op.stable_name(),
                },
                inputs: node.inputs.clone(),
                output: node.output.clone(),
                source: node.source.clone(),
                rule: gradient_rule(&node.op).unwrap_or_else(|| "none".to_string()),
            })
            .collect::<Vec<_>>();

        let loss_nodes = function
            .nodes
            .iter()
            .filter(|node| is_loss_candidate(&node.op))
            .map(|node| node.id)
            .collect::<Vec<_>>();
        let loss_node = loss_nodes.first().copied();

        let mut backward = Vec::new();
        let mut diagnostics = Vec::new();
        if !loss_nodes.is_empty() {
            let mut reachable = std::collections::BTreeSet::new();
            for loss in &loss_nodes {
                collect_ancestors(*loss, function, &mut reachable);
            }
            for loss in &loss_nodes {
                backward.push(AutodiffNode {
                    id: 0,
                    kind: AutodiffNodeKind::BackwardSeed { loss_node: *loss },
                    inputs: vec![*loss],
                    output: function.nodes[*loss].output.clone(),
                    source: function.nodes[*loss].source.clone(),
                    rule: "seed=1".to_string(),
                });
            }

            for node in function.nodes.iter().rev() {
                if !reachable.contains(&node.id)
                    || matches!(
                        node.op,
                        TensorGraphOp::Parameter | TensorGraphOp::Create { .. }
                    )
                {
                    continue;
                }
                if is_autodiff_auxiliary(&node.op) {
                    continue;
                }
                match gradient_rule(&node.op) {
                    Some(rule) => {
                        let save_id = backward.len();
                        backward.push(AutodiffNode {
                            id: save_id,
                            kind: AutodiffNodeKind::SaveForBackward {
                                forward_node: node.id,
                            },
                            inputs: vec![node.id],
                            output: node.output.clone(),
                            source: node.source.clone(),
                            rule: "save_forward_value".to_string(),
                        });
                        backward.push(AutodiffNode {
                            id: backward.len(),
                            kind: AutodiffNodeKind::Gradient {
                                target_node: node.id,
                                op: node.op.stable_name(),
                            },
                            inputs: node.inputs.clone(),
                            output: node.output.clone(),
                            source: node.source.clone(),
                            rule,
                        });
                        if node.inputs.len() > 1 {
                            backward.push(AutodiffNode {
                                id: backward.len(),
                                kind: AutodiffNodeKind::AccumulateGradient {
                                    target_node: node.inputs[0],
                                },
                                inputs: node.inputs.clone(),
                                output: node.output.clone(),
                                source: node.source.clone(),
                                rule: "stable_sum".to_string(),
                            });
                        }
                    }
                    None => diagnostics.push(AutodiffDiagnostic {
                        code: "E3004",
                        node: node.id,
                        operation: node.op.stable_name(),
                        message: "operation has no registered compiler-native gradient rule"
                            .to_string(),
                    }),
                }
            }
        }

        Self {
            name: function.name.clone(),
            forward,
            backward,
            loss_nodes,
            loss_node,
            diagnostics,
        }
    }
}

fn is_loss_candidate(op: &TensorGraphOp) -> bool {
    matches!(
        op,
        TensorGraphOp::Reduction { .. } | TensorGraphOp::Loss { .. }
    ) || matches!(op, TensorGraphOp::Elementwise { name } if name == "dot_t")
}

fn collect_ancestors(
    node_id: usize,
    function: &TensorGraphFunction,
    reachable: &mut std::collections::BTreeSet<usize>,
) {
    if !reachable.insert(node_id) {
        return;
    }
    if let Some(node) = function.nodes.iter().find(|node| node.id == node_id) {
        for input in &node.inputs {
            collect_ancestors(*input, function, reachable);
        }
    }
}

fn gradient_rule(op: &TensorGraphOp) -> Option<String> {
    match op {
        TensorGraphOp::Elementwise { name } => match name.as_str() {
            "add" => Some("d(a+b)=(g,g)".to_string()),
            "sub" => Some("d(a-b)=(g,-g)".to_string()),
            "mul" => Some("d(a*b)=(g*b,g*a)".to_string()),
            "div" => Some("d(a/b)=(g/b,-g*a/(b*b))".to_string()),
            "neg" => Some("d(-a)=-g".to_string()),
            "exp" | "exp_f" => Some("d(exp(a))=g*exp(a)".to_string()),
            "log" | "log_f" => Some("d(log(a))=g/a".to_string()),
            "relu" => Some("d(relu(a))=g*(a>0)".to_string()),
            "sigmoid" | "sigmoid_f" => Some("d(sigmoid(a))=g*y*(1-y)".to_string()),
            _ => None,
        },
        TensorGraphOp::Reduction { name } => match name.as_str() {
            "sum_t" => Some("broadcast(g,input_shape)".to_string()),
            "mean_t" => Some("broadcast(g/input_size,input_shape)".to_string()),
            "dot_t" => Some("(g*b,g*a)".to_string()),
            _ => None,
        },
        TensorGraphOp::Matmul => Some("(g@transpose(b),transpose(a)@g)".to_string()),
        TensorGraphOp::Transpose => Some("transpose(g)".to_string()),
        TensorGraphOp::Reshape => Some("reshape(g,input_shape)".to_string()),
        TensorGraphOp::DeviceTransfer { .. } => {
            Some("identity_with_device_transfer(g)".to_string())
        }
        TensorGraphOp::Linear => Some("linear_backward(input,weight,bias,g)".to_string()),
        TensorGraphOp::Loss { name } if name == "mse_loss" => {
            Some("2*(prediction-target)/count".to_string())
        }
        _ => None,
    }
}

fn is_autodiff_auxiliary(op: &TensorGraphOp) -> bool {
    matches!(op, TensorGraphOp::UnknownHost { host }
        if host.ends_with(".requires_grad")
            || host.ends_with(".grad")
            || host.ends_with(".free_all")
            || host.ends_with(".set_grad_enabled"))
}

fn node_name(kind: &AutodiffNodeKind) -> String {
    match kind {
        AutodiffNodeKind::Forward { op } => format!("forward.{op}"),
        AutodiffNodeKind::SaveForBackward { forward_node } => format!("save(%{forward_node})"),
        AutodiffNodeKind::BackwardSeed { loss_node } => format!("seed(%{loss_node})"),
        AutodiffNodeKind::Gradient { target_node, op } => format!("grad(%{target_node},{op})"),
        AutodiffNodeKind::AccumulateGradient { target_node } => {
            format!("accumulate(%{target_node})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tensor_graph::{
        TensorDType, TensorDevice, TensorGraphNode, TensorGraphOp, TensorGraphSource, TensorShape,
    };

    #[test]
    fn builds_reverse_rule_and_accumulation_for_mul_sum() {
        let source = TensorGraphSource {
            block: 0,
            instruction: 0,
            host: None,
        };
        let graph = TensorGraph {
            module: "test".into(),
            functions: vec![TensorGraphFunction {
                name: "loss".into(),
                nodes: vec![
                    TensorGraphNode {
                        id: 0,
                        value: Some(0),
                        op: TensorGraphOp::Parameter,
                        inputs: vec![],
                        output: TensorMetadata::unknown(),
                        source: source.clone(),
                    },
                    TensorGraphNode {
                        id: 1,
                        value: Some(1),
                        op: TensorGraphOp::Elementwise { name: "mul".into() },
                        inputs: vec![0, 0],
                        output: TensorMetadata::new(
                            TensorDType::Float,
                            TensorShape::Ranked(vec![Some(3)]),
                            TensorDevice::Cpu,
                        ),
                        source: source.clone(),
                    },
                    TensorGraphNode {
                        id: 2,
                        value: Some(2),
                        op: TensorGraphOp::Reduction {
                            name: "sum_t".into(),
                        },
                        inputs: vec![1],
                        output: TensorMetadata::new(
                            TensorDType::Float,
                            TensorShape::Ranked(vec![]),
                            TensorDevice::Cpu,
                        ),
                        source,
                    },
                ],
            }],
        };
        let autodiff = AutodiffGraph::from_tensor_graph(&graph);
        let function = &autodiff.functions[0];
        assert_eq!(function.loss_node, Some(2));
        assert!(function
            .backward
            .iter()
            .any(|node| matches!(node.kind, AutodiffNodeKind::Gradient { target_node: 1, .. })));
        assert!(function
            .backward
            .iter()
            .any(|node| matches!(node.kind, AutodiffNodeKind::AccumulateGradient { .. })));
        assert!(function.diagnostics.is_empty());
    }

    #[test]
    fn reports_unsupported_gradient_rule() {
        let graph = TensorGraph {
            module: "test".into(),
            functions: vec![TensorGraphFunction {
                name: "loss".into(),
                nodes: vec![
                    TensorGraphNode {
                        id: 0,
                        value: Some(0),
                        op: TensorGraphOp::Elementwise {
                            name: "tanh_f".into(),
                        },
                        inputs: vec![],
                        output: TensorMetadata::unknown(),
                        source: TensorGraphSource {
                            block: 0,
                            instruction: 0,
                            host: None,
                        },
                    },
                    TensorGraphNode {
                        id: 1,
                        value: Some(1),
                        op: TensorGraphOp::Loss {
                            name: "mse_loss".into(),
                        },
                        inputs: vec![0],
                        output: TensorMetadata::unknown(),
                        source: TensorGraphSource {
                            block: 0,
                            instruction: 1,
                            host: None,
                        },
                    },
                ],
            }],
        };
        let autodiff = AutodiffGraph::from_tensor_graph(&graph);
        assert!(autodiff.functions[0]
            .backward
            .iter()
            .any(|node| matches!(node.kind, AutodiffNodeKind::BackwardSeed { .. })));
        assert_eq!(autodiff.functions[0].diagnostics[0].code, "E3004");
    }
}
