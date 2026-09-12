//! Compiler-visible reverse-mode autodiff graph.
//!
//! The runtime still owns tensor storage and kernel execution, but this module
//! owns the differentiation contract: which forward operations participate,
//! which values must be retained, and which reverse rule is selected.

use crate::ir::{Function, Instruction, InstructionKind, Module, SourceSpan, Terminator, Value};
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
/// Straight-line SSA tensor graphs lower to steps in place. `if`/`else`
/// joins over tensor values accumulate per-arm adjoints: the walk defers a
/// join until its upstream is known, then replays one guarded backward chain
/// per arm behind a re-branch on the original condition, so only the taken
/// arm contributes. Loop-carried values (memory loads), `switch` joins with
/// more than two arms, nested joins, and non-differentiable passthrough arms
/// fail before reaching the backend instead of silently producing partial
/// gradients; use runtime `tensor.backward` outside `diff` for those shapes.
pub fn materialize_autodiff_steps(module: &mut Module) -> Result<usize, String> {
    let mut materialized = 0;
    for function in &mut module.functions {
        let (definitions, phis, loads, copies) = function_value_definitions(function);
        let mut back_counter = 0usize;
        let mut block_index = 0;
        // New adjoint blocks are appended while walking, so re-check length.
        while block_index < function.blocks.len() {
            let original = std::mem::take(&mut function.blocks[block_index].instructions);
            // Uncommitted content for `insertion`: starts as this block's
            // replacement, then tracks the newest continuation after each
            // branch splice below.
            let mut replacement = Vec::new();
            let mut insertion = block_index;
            for instruction in original {
                let InstructionKind::HostCall { host, args, .. } = &instruction.kind else {
                    replacement.push(instruction);
                    continue;
                };
                if host != "spectra.compiler.autodiff_region" {
                    replacement.push(instruction);
                    continue;
                };
                let Some(loss) = args.first().copied() else {
                    return Err("E3004: autodiff adapter has no loss operand".to_string());
                };
                let mut steps = Vec::new();
                let mut visiting = HashSet::new();
                let mut deferred = Vec::new();
                materialize_node(
                    function,
                    loss,
                    None,
                    MaterializeTables {
                        definitions: &definitions,
                        phis: &phis,
                        loads: &loads,
                        copies: &copies,
                    },
                    instruction.source_span.clone(),
                    &mut MaterializeState {
                        visiting: &mut visiting,
                        steps: &mut steps,
                        deferred: &mut deferred,
                    },
                    false,
                )?;
                materialized += steps.len();
                replacement.extend(steps);
                // Commit first: the join gradient appended below must land
                // after the use's own step.
                function.blocks[insertion]
                    .instructions
                    .append(&mut replacement);
                for pending in deferred {
                    let (placed, next) = splice_branch_adjoint(
                        function,
                        insertion,
                        pending,
                        &definitions,
                        &phis,
                        &loads,
                        &copies,
                        &mut back_counter,
                    )?;
                    materialized += placed;
                    insertion = next;
                }
                replacement = Vec::new();
            }
            function.blocks[insertion]
                .instructions
                .append(&mut replacement);
            block_index += 1;
        }
        for block in &mut function.blocks {
            for (id, instruction) in block.instructions.iter_mut().enumerate() {
                instruction.id = id;
            }
        }
    }
    Ok(materialized)
}

type HostDefinition = (String, Vec<Value>, Option<SourceSpan>);
type PhiIncoming = Vec<(Value, usize)>;

type FunctionValueDefinitions = (
    HashMap<usize, HostDefinition>,
    HashMap<usize, PhiIncoming>,
    HashSet<usize>,
    HashMap<usize, Value>,
);

/// Join descriptor deferred until its upstream gradient is known. The join's
/// own upstream is resolved at splice time from the use site, so only the
/// phi, its incoming arms, and the source span are stored.
struct DeferredPhi {
    phi: Value,
    incoming: PhiIncoming,
    source: Option<SourceSpan>,
}

// Shared autodiff lookup tables threaded through materialize_node.
#[derive(Clone, Copy)]
struct MaterializeTables<'a> {
    definitions: &'a HashMap<usize, HostDefinition>,
    phis: &'a HashMap<usize, PhiIncoming>,
    loads: &'a HashSet<usize>,
    copies: &'a HashMap<usize, Value>,
}

// Mutable gradient-construction state threaded through materialize_node.
struct MaterializeState<'a> {
    visiting: &'a mut HashSet<usize>,
    steps: &'a mut Vec<Instruction>,
    deferred: &'a mut Vec<DeferredPhi>,
}

fn function_value_definitions(function: &Function) -> FunctionValueDefinitions {
    let mut definitions = HashMap::new();
    let mut phis = HashMap::new();
    let mut loads = HashSet::new();
    let mut copies = HashMap::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            match &instruction.kind {
                InstructionKind::HostCall {
                    result: Some(result),
                    host,
                    args,
                    ..
                } => {
                    definitions.insert(
                        result.id,
                        (host.clone(), args.clone(), instruction.source_span.clone()),
                    );
                }
                InstructionKind::Phi { result, incoming } => {
                    phis.insert(result.id, incoming.clone());
                }
                InstructionKind::Load { result, .. }
                | InstructionKind::FrameLoad { result, .. } => {
                    loads.insert(result.id);
                }
                InstructionKind::Copy { result, source } => {
                    copies.insert(result.id, *source);
                }
                _ => {}
            }
        }
    }
    (definitions, phis, loads, copies)
}

fn materialize_node(
    function: &mut Function,
    output: Value,
    upstream: Option<Value>,
    tables: MaterializeTables<'_>,
    source: Option<SourceSpan>,
    state: &mut MaterializeState<'_>,
    in_arm: bool,
) -> Result<(), String> {
    // Transparent aliases resolve to their source before anything else.
    let mut resolved = output;
    while let Some(source_value) = tables.copies.get(&resolved.id) {
        if !state.visiting.insert(resolved.id) {
            return Err(format!(
                "E3004: cyclic autodiff dependency at value %{}",
                output.id
            ));
        }
        resolved = *source_value;
    }
    if !state.visiting.insert(resolved.id) {
        return Err(format!(
            "E3004: cyclic autodiff dependency at value %{}",
            output.id
        ));
    }
    let output = resolved;
    let Some((host, args, node_source)) = tables.definitions.get(&output.id) else {
        if let Some(incoming) = tables.phis.get(&output.id) {
            if in_arm {
                state.visiting.remove(&output.id);
                return Err(format!(
                    "E3004: nested branch joins are not supported inside compiler-native diff (value %{})",
                    output.id
                ));
            }
            state.deferred.push(DeferredPhi {
                phi: output,
                incoming: incoming.clone(),
                source: source.clone(),
            });
            state.visiting.remove(&output.id);
            return Ok(());
        }
        if tables.loads.contains(&output.id) {
            state.visiting.remove(&output.id);
            return Err(format!(
                "E3004: loop-carried tensor value has no static adjoint inside compiler-native diff (value %{}); move the loop outside `diff` and use runtime tensor.backward, or restructure with straight-line bindings",
                output.id
            ));
        }
        state.visiting.remove(&output.id);
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
        state.visiting.remove(&output.id);
        return Ok(());
    };
    let tensor_args = tensor_arguments(host, args);
    if tensor_args.is_empty() {
        // Every ruled operation takes at least one tensor input by
        // construction (`tensor_arguments` covers positions 0-2 of each
        // rule); reaching here means the call was malformed upstream.
        // Failing loudly beats silently dropping the gradient.
        state.visiting.remove(&output.id);
        return Err(format!(
            "E3004: operation '{host}' reached compiler-native diff without tensor inputs"
        ));
    }
    let step_source = node_source.clone().or_else(|| source.clone());
    let effective_upstream = if upstream.is_some() { upstream } else { None };
    state.steps.push(Instruction {
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
        let mut resolved = input;
        while let Some(source_value) = tables.copies.get(&resolved.id) {
            resolved = *source_value;
        }
        if tables.loads.contains(&resolved.id) {
            return Err(format!(
                "E3004: loop-carried tensor value has no static adjoint inside compiler-native diff (value %{}); move the loop outside `diff` and use runtime tensor.backward, or restructure with straight-line bindings",
                resolved.id
            ));
        }
        if tables.phis.contains_key(&input.id) {
            let grad_handle = function.next_value();
            state.steps.push(Instruction {
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
                tables,
                step_source.clone(),
                &mut *state,
                in_arm,
            )?;
            continue;
        }
        if is_autodiff_leaf_or_auxiliary(tables.definitions.get(&input.id).map(|d| d.0.as_str())) {
            continue;
        }
        let grad_handle = function.next_value();
        state.steps.push(Instruction {
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
            tables,
            step_source.clone(),
            &mut *state,
            in_arm,
        )?;
    }
    state.visiting.remove(&output.id);
    Ok(())
}

/// Splice one guarded backward chain per arm of a deferred `if`/`else` join
/// into the flow right after `insertion`, returning the placed count and the
/// continuation block that subsequent content must target. Only the taken arm
/// executes at runtime, so its contribution is exact; multiple uses of the
/// same join route one chain per use and the runtime sums the contributions.
/// The insertion block keeps its instructions; its terminator moves to the
/// new join block, and successor phis that named the insertion block are
/// rewritten to the join.
#[allow(clippy::too_many_arguments)]
fn splice_branch_adjoint(
    function: &mut Function,
    insertion: usize,
    pending: DeferredPhi,
    definitions: &HashMap<usize, HostDefinition>,
    phis: &HashMap<usize, PhiIncoming>,
    loads: &HashSet<usize>,
    copies: &HashMap<usize, Value>,
    back_counter: &mut usize,
) -> Result<(usize, usize), String> {
    if pending.incoming.len() != 2 {
        return Err(format!(
            "E3004: branch joins with {} arms are not supported inside compiler-native diff (value %{})",
            pending.incoming.len(),
            pending.phi.id
        ));
    }
    let mut arms = Vec::with_capacity(2);
    for (value, block) in &pending.incoming {
        let mut resolved = *value;
        while let Some(source_value) = copies.get(&resolved.id) {
            resolved = *source_value;
        }
        arms.push((resolved, *block));
    }
    let mut arm_ops = Vec::with_capacity(2);
    for (value, _) in &arms {
        let Some((host, args, node_source)) = definitions.get(&value.id) else {
            if phis.contains_key(&value.id) {
                return Err(format!(
                    "E3004: nested branch joins are not supported inside compiler-native diff (value %{})",
                    value.id
                ));
            }
            if loads.contains(&value.id) {
                return Err(format!(
                    "E3004: loop-carried tensor value has no static adjoint inside compiler-native diff (value %{}); move the loop outside `diff` and use runtime tensor.backward, or restructure with straight-line bindings",
                    value.id
                ));
            }
            return Err(format!(
                "E3004: branch arm is not a differentiable operation inside compiler-native diff (value %{})",
                value.id
            ));
        };
        let Some(operation) = autodiff_operation(host) else {
            return Err(format!(
                "E3004: operation has no registered reverse kernel ({host})"
            ));
        };
        let tensor_args = tensor_arguments(host, args);
        if tensor_args.is_empty() {
            return Err(format!(
                "E3004: branch arm has no tensor operands inside compiler-native diff ({host})"
            ));
        }
        arm_ops.push((operation.to_string(), tensor_args, node_source.clone()));
    }
    let mut pred_blocks: Vec<usize> = arms.iter().map(|(_, block)| *block).collect();
    pred_blocks.sort_unstable();
    let mut branch = None;
    for block in &function.blocks {
        if let Some(Terminator::CondBranch {
            condition: cond,
            true_block,
            false_block,
        }) = &block.terminator
        {
            let mut targets = vec![*true_block, *false_block];
            targets.sort_unstable();
            if targets == pred_blocks {
                branch = Some((*cond, *true_block, *false_block));
                break;
            }
        }
    }
    let Some((condition, true_block, _)) = branch else {
        return Err(format!(
            "E3004: branch join has no two-way condition for adjoint accumulation (value %{})",
            pending.phi.id
        ));
    };
    let grad_handle = function.next_value();
    let mut placed = 0;
    {
        let insertion_block = function
            .get_block_mut(insertion)
            .ok_or_else(|| "E3004: insertion block vanished during branch lowering".to_string())?;
        insertion_block.instructions.push(Instruction {
            id: 0,
            kind: InstructionKind::AutodiffStep {
                result: Some(grad_handle),
                operation: "grad_handle".to_string(),
                output: pending.phi,
                upstream: None,
                inputs: vec![pending.phi],
                targets: vec![],
            },
            source_span: pending.source.clone(),
        });
        placed += 1;
    }
    let tag = *back_counter;
    *back_counter += 1;
    let then_back = function.add_block(format!("adiff.then.{tag}"));
    let else_back = function.add_block(format!("adiff.else.{tag}"));
    let join_back = function.add_block(format!("adiff.join.{tag}"));
    for (index, ((value, _), (operation, tensor_args, node_source))) in
        arms.iter().zip(arm_ops.iter()).enumerate()
    {
        let back_block = if arms[index].1 == true_block {
            then_back
        } else {
            else_back
        };
        let (forward_host, forward_args, forward_type) = {
            let mut found = None;
            for block in &function.blocks {
                for instruction in &block.instructions {
                    if let InstructionKind::HostCall {
                        result: Some(result),
                        host,
                        args,
                        result_type,
                    } = &instruction.kind
                    {
                        if result.id == value.id {
                            found = Some((host.clone(), args.clone(), result_type.clone()));
                            break;
                        }
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            found.ok_or_else(|| {
                format!(
                    "E3004: branch arm value has no forward operation (value %{})",
                    value.id
                )
            })?
        };
        for argument in forward_args.iter().chain(tensor_args.iter()) {
            let mut resolved = *argument;
            while let Some(source_value) = copies.get(&resolved.id) {
                resolved = *source_value;
            }
            if phis.contains_key(&resolved.id) {
                return Err(format!(
                    "E3004: nested branch joins are not supported inside compiler-native diff (value %{})",
                    resolved.id
                ));
            }
            if loads.contains(&resolved.id) {
                return Err(format!(
                    "E3004: loop-carried tensor value has no static adjoint inside compiler-native diff (value %{}); move the loop outside `diff` and use runtime tensor.backward, or restructure with straight-line bindings",
                    resolved.id
                ));
            }
            if definitions.contains_key(&resolved.id) {
                let mut home = None;
                for block in &function.blocks {
                    if block.instructions.iter().any(|instruction| {
                        matches!(&instruction.kind, InstructionKind::HostCall { result: Some(result), .. } if result.id == resolved.id)
                    }) {
                        home = Some(block.id);
                        break;
                    }
                }
                if home == Some(arms[0].1) || home == Some(arms[1].1) {
                    return Err(format!(
                        "E3004: multi-operation branch arms are not supported inside compiler-native diff (value %{}); keep one differentiable operation per arm",
                        value.id
                    ));
                }
            }
        }
        let step_source = node_source.clone().or_else(|| pending.source.clone());
        let mut arm_steps = Vec::new();
        let replayed = function.next_value();
        arm_steps.push(Instruction {
            id: 0,
            kind: InstructionKind::HostCall {
                result: Some(replayed),
                host: forward_host,
                args: forward_args,
                result_type: forward_type,
            },
            source_span: step_source.clone(),
        });
        placed += 1;
        arm_steps.push(Instruction {
            id: 0,
            kind: InstructionKind::AutodiffStep {
                result: None,
                operation: format!("grad_apply_{operation}"),
                output: replayed,
                upstream: Some(grad_handle),
                inputs: tensor_args.clone(),
                targets: tensor_args.clone(),
            },
            source_span: step_source.clone(),
        });
        placed += 1;
        let mut visiting = HashSet::new();
        for input in tensor_args {
            if phis.contains_key(&input.id) {
                return Err(format!(
                    "E3004: nested branch joins are not supported inside compiler-native diff (value %{})",
                    input.id
                ));
            }
            if loads.contains(&input.id) {
                return Err(format!(
                    "E3004: loop-carried tensor value has no static adjoint inside compiler-native diff (value %{}); move the loop outside `diff` and use runtime tensor.backward, or restructure with straight-line bindings",
                    input.id
                ));
            }
            if is_autodiff_leaf_or_auxiliary(definitions.get(&input.id).map(|d| d.0.as_str())) {
                continue;
            }
            let input_handle = function.next_value();
            arm_steps.push(Instruction {
                id: 0,
                kind: InstructionKind::AutodiffStep {
                    result: Some(input_handle),
                    operation: "grad_handle".to_string(),
                    output: *input,
                    upstream: None,
                    inputs: vec![*input],
                    targets: vec![],
                },
                source_span: step_source.clone(),
            });
            placed += 1;
            let before = arm_steps.len();
            materialize_node(
                function,
                *input,
                Some(input_handle),
                MaterializeTables {
                    definitions,
                    phis,
                    loads,
                    copies,
                },
                step_source.clone(),
                &mut MaterializeState {
                    visiting: &mut visiting,
                    steps: &mut arm_steps,
                    deferred: &mut Vec::new(),
                },
                true,
            )?;
            placed += arm_steps.len() - before;
        }
        let back = function
            .get_block_mut(back_block)
            .ok_or_else(|| "E3004: adjoint block vanished during lowering".to_string())?;
        back.instructions.extend(arm_steps);
        back.set_terminator(Terminator::Branch { target: join_back });
    }
    let original = function
        .get_block_mut(insertion)
        .ok_or_else(|| "E3004: insertion block vanished during lowering".to_string())?
        .terminator
        .replace(Terminator::Unreachable)
        .ok_or_else(|| {
            "E3004: insertion block has no terminator for branch lowering".to_string()
        })?;
    function
        .get_block_mut(join_back)
        .ok_or_else(|| "E3004: adjoint join vanished during lowering".to_string())?
        .set_terminator(original);
    function
        .get_block_mut(insertion)
        .ok_or_else(|| "E3004: insertion block vanished during lowering".to_string())?
        .set_terminator(Terminator::CondBranch {
            condition,
            true_block: then_back,
            false_block: else_back,
        });
    for block in function.blocks.iter_mut() {
        for instruction in block.instructions.iter_mut() {
            if let InstructionKind::Phi { incoming, .. } = &mut instruction.kind {
                for (_, pred) in incoming.iter_mut() {
                    if *pred == insertion {
                        *pred = join_back;
                    }
                }
            }
        }
    }
    Ok((placed, join_back))
}

fn autodiff_operation(host: &str) -> Option<&str> {
    let name = host
        .strip_prefix("spectra.std.tensor.")
        .or_else(|| host.strip_prefix("spectra.std.ml."))?;
    match name {
        "add" | "sub" | "mul" | "div" | "neg" | "relu" | "sum_t" | "mean_t" | "dot_t"
        | "matmul" | "matmul_batched" | "transpose" | "reshape" | "linear" | "mse_loss"
        | "bce_loss" | "conv2d" | "max_pool2d" | "dropout" | "concat" | "stack" | "slice"
        | "permute" => Some(name),
        "exp_f" => Some("exp"),
        "log_f" => Some("log"),
        "sqrt_f" => Some("sqrt"),
        "sigmoid_f" => Some("sigmoid"),
        "tanh_f" => Some("tanh"),
        _ => None,
    }
}

fn tensor_arguments(host: &str, args: &[Value]) -> Vec<Value> {
    let name = host
        .strip_prefix("spectra.std.tensor.")
        .or_else(|| host.strip_prefix("spectra.std.ml."))
        .unwrap_or("");
    let positions: &[usize] = match name {
        "reshape" | "transpose" | "sum_t" | "neg" | "exp_f" | "log_f" | "relu" | "sigmoid_f"
        | "sqrt_f" | "tanh_f" => &[0],
        "add" | "sub" | "mul" | "div" | "matmul" | "matmul_batched" | "dot_t" | "mse_loss"
        | "bce_loss" => &[0, 1],
        "linear" | "conv2d" => &[0, 1, 2],
        "max_pool2d" | "dropout" => &[0],
        "concat" | "stack" => &[0, 1],
        "slice" | "permute" => &[0],
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
            "sqrt" | "sqrt_f" => Some("d(sqrt(a))=g*0.5/y".to_string()),
            "relu" => Some("d(relu(a))=g*(a>0)".to_string()),
            "sigmoid" | "sigmoid_f" => Some("d(sigmoid(a))=g*y*(1-y)".to_string()),
            "tanh" | "tanh_f" => Some("d(tanh(a))=g*(1-y*y)".to_string()),
            _ => None,
        },
        TensorGraphOp::Reduction { name } => match name.as_str() {
            "sum_t" => Some("broadcast(g,input_shape)".to_string()),
            "mean_t" => Some("broadcast(g/input_size,input_shape)".to_string()),
            "dot_t" => Some("(g*b,g*a)".to_string()),
            _ => None,
        },
        TensorGraphOp::Matmul => Some("(g@transpose(b),transpose(a)@g)".to_string()),
        TensorGraphOp::BatchedMatmul => {
            Some("(g@transpose(b),transpose(a)@g) per batch".to_string())
        }
        TensorGraphOp::UnknownHost { host } if host.ends_with(".concat") => {
            Some("split(g,left_len)".to_string())
        }
        TensorGraphOp::UnknownHost { host } if host.ends_with(".stack") => {
            Some("split(g,half)".to_string())
        }
        TensorGraphOp::UnknownHost { host } if host.ends_with(".slice") => {
            Some("scatter(g,start)".to_string())
        }
        TensorGraphOp::UnknownHost { host } if host.ends_with(".permute") => {
            Some("permute(g,axes)".to_string())
        }
        TensorGraphOp::Transpose => Some("transpose(g)".to_string()),
        TensorGraphOp::Reshape => Some("reshape(g,input_shape)".to_string()),
        TensorGraphOp::DeviceTransfer { .. } => {
            Some("identity_with_device_transfer(g)".to_string())
        }
        TensorGraphOp::Linear => Some("linear_backward(input,weight,bias,g)".to_string()),
        TensorGraphOp::Conv2d => Some("conv2d_backward(input,kernel,bias,g)".to_string()),
        TensorGraphOp::MaxPool2d => Some("maxpool2d_backward(input,g)".to_string()),
        TensorGraphOp::Dropout => Some("dropout_backward(mask,g)".to_string()),
        TensorGraphOp::Loss { name } if name == "mse_loss" => {
            Some("2*(prediction-target)/count".to_string())
        }
        TensorGraphOp::Loss { name } if name == "bce_loss" => {
            Some("(clamped-p-target)/(clamped-p*(1-clamped-p)*count)".to_string())
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
                            name: "pow_f".into(),
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

    #[test]
    fn tanh_and_sqrt_chain_registers_gradient_rules() {
        let source = TensorGraphSource {
            block: 0,
            instruction: 0,
            host: None,
        };
        let float_vector = || {
            TensorMetadata::new(
                TensorDType::Float,
                TensorShape::Ranked(vec![Some(3)]),
                TensorDevice::Cpu,
            )
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
                        op: TensorGraphOp::Elementwise {
                            name: "tanh_f".into(),
                        },
                        inputs: vec![0],
                        output: float_vector(),
                        source: source.clone(),
                    },
                    TensorGraphNode {
                        id: 2,
                        value: Some(2),
                        op: TensorGraphOp::Elementwise {
                            name: "sqrt_f".into(),
                        },
                        inputs: vec![1],
                        output: float_vector(),
                        source: source.clone(),
                    },
                    TensorGraphNode {
                        id: 3,
                        value: Some(3),
                        op: TensorGraphOp::Reduction {
                            name: "sum_t".into(),
                        },
                        inputs: vec![2],
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
        assert_eq!(function.loss_node, Some(3));
        assert!(
            function.diagnostics.is_empty(),
            "{:?}",
            function.diagnostics
        );
        let tanh_rule = function
            .backward
            .iter()
            .find(|node| {
                matches!(
                    &node.kind,
                    AutodiffNodeKind::Gradient { target_node: 1, .. }
                )
            })
            .expect("tanh gradient node");
        assert_eq!(tanh_rule.rule, "d(tanh(a))=g*(1-y*y)");
        let sqrt_rule = function
            .backward
            .iter()
            .find(|node| {
                matches!(
                    &node.kind,
                    AutodiffNodeKind::Gradient { target_node: 2, .. }
                )
            })
            .expect("sqrt gradient node");
        assert_eq!(sqrt_rule.rule, "d(sqrt(a))=g*0.5/y");
    }

    #[test]
    fn bce_loss_registers_gradient_rule() {
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
                        op: TensorGraphOp::Parameter,
                        inputs: vec![],
                        output: TensorMetadata::unknown(),
                        source: source.clone(),
                    },
                    TensorGraphNode {
                        id: 2,
                        value: Some(2),
                        op: TensorGraphOp::Loss {
                            name: "bce_loss".into(),
                        },
                        inputs: vec![0, 1],
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
        assert!(
            function.diagnostics.is_empty(),
            "{:?}",
            function.diagnostics
        );
        let rule = function
            .backward
            .iter()
            .find(|node| {
                matches!(
                    &node.kind,
                    AutodiffNodeKind::Gradient { target_node: 2, .. }
                )
            })
            .expect("bce gradient node");
        assert!(rule.rule.contains("clamped-p"), "{}", rule.rule);
    }

    #[test]
    fn conv_pool_dropout_register_gradient_rules() {
        use TensorGraphOp::*;
        let source = TensorGraphSource {
            block: 0,
            instruction: 0,
            host: None,
        };
        let scalar = || {
            TensorMetadata::new(
                TensorDType::Float,
                TensorShape::Ranked(vec![]),
                TensorDevice::Cpu,
            )
        };
        let node = |id: usize, op: TensorGraphOp, inputs: Vec<usize>| TensorGraphNode {
            id,
            value: Some(id),
            op,
            inputs,
            output: TensorMetadata::unknown(),
            source: source.clone(),
        };
        // One loss per op keeps each rule assertion independent.
        let cases: Vec<(&str, TensorGraphOp, Vec<usize>, &str)> = vec![
            ("conv", Conv2d, vec![0, 1, 2], "conv2d_backward"),
            ("pool", MaxPool2d, vec![0], "maxpool2d_backward"),
            ("drop", Dropout, vec![0], "dropout_backward"),
        ];
        for (name, op, inputs, rule_part) in cases {
            let graph = TensorGraph {
                module: "test".into(),
                functions: vec![TensorGraphFunction {
                    name: name.into(),
                    nodes: vec![
                        node(0, Parameter, vec![]),
                        node(1, Parameter, vec![]),
                        node(2, Parameter, vec![]),
                        TensorGraphNode {
                            id: 3,
                            value: Some(3),
                            op,
                            inputs,
                            output: scalar(),
                            source: source.clone(),
                        },
                        TensorGraphNode {
                            id: 4,
                            value: Some(4),
                            op: Reduction {
                                name: "sum_t".into(),
                            },

                            inputs: vec![3],
                            output: scalar(),
                            source: source.clone(),
                        },
                    ],
                }],
            };
            let autodiff = AutodiffGraph::from_tensor_graph(&graph);
            let function = &autodiff.functions[0];
            assert_eq!(function.loss_node, Some(4), "{name}");
            assert!(
                function.diagnostics.is_empty(),
                "{name}: {:?}",
                function.diagnostics
            );
            let rule = function
                .backward
                .iter()
                .find(|node| {
                    matches!(
                        &node.kind,
                        AutodiffNodeKind::Gradient { target_node: 3, .. }
                    )
                })
                .unwrap_or_else(|| panic!("{name} gradient node"));
            assert!(rule.rule.contains(rule_part), "{name}: {}", rule.rule);
        }
    }
    #[test]
    fn batched_and_shape_ops_register_gradient_rules() {
        use TensorGraphOp::*;
        let source = TensorGraphSource {
            block: 0,
            instruction: 0,
            host: None,
        };
        let scalar = || {
            TensorMetadata::new(
                TensorDType::Float,
                TensorShape::Ranked(vec![]),
                TensorDevice::Cpu,
            )
        };
        let node = |id: usize, op: TensorGraphOp, inputs: Vec<usize>| TensorGraphNode {
            id,
            value: Some(id),
            op,
            inputs,
            output: TensorMetadata::unknown(),
            source: source.clone(),
        };
        let host = |name: &str| UnknownHost {
            host: format!("spectra.std.tensor.{name}"),
        };
        let cases: Vec<(&str, TensorGraphOp, Vec<usize>, &str)> = vec![
            ("bmm", BatchedMatmul, vec![0, 1], "per batch"),
            ("cat", host("concat"), vec![0, 1], "split(g,left_len)"),
            ("stk", host("stack"), vec![0, 1], "split(g,half)"),
            ("slc", host("slice"), vec![0], "scatter(g,start)"),
            ("prm", host("permute"), vec![0], "permute(g,axes)"),
        ];
        for (name, op, inputs, rule_part) in cases {
            let graph = TensorGraph {
                module: "test".into(),
                functions: vec![TensorGraphFunction {
                    name: name.into(),
                    nodes: vec![
                        node(0, Parameter, vec![]),
                        node(1, Parameter, vec![]),
                        TensorGraphNode {
                            id: 2,
                            value: Some(2),
                            op,
                            inputs,
                            output: scalar(),
                            source: source.clone(),
                        },
                        TensorGraphNode {
                            id: 3,
                            value: Some(3),
                            op: Reduction {
                                name: "sum_t".into(),
                            },
                            inputs: vec![2],
                            output: scalar(),
                            source: source.clone(),
                        },
                    ],
                }],
            };
            let autodiff = AutodiffGraph::from_tensor_graph(&graph);
            let function = &autodiff.functions[0];
            assert_eq!(function.loss_node, Some(3), "{name}");
            assert!(
                function.diagnostics.is_empty(),
                "{name}: {:?}",
                function.diagnostics
            );
            let rule = function
                .backward
                .iter()
                .find(|node| {
                    matches!(
                        &node.kind,
                        AutodiffNodeKind::Gradient { target_node: 2, .. }
                    )
                })
                .unwrap_or_else(|| panic!("{name} gradient node"));
            assert!(rule.rule.contains(rule_part), "{name}: {}", rule.rule);
        }
    }
    #[test]
    fn loop_carried_loads_fail_loudly_instead_of_partial_grads() {
        use crate::ir::{BasicBlock, Terminator, Type};

        let loss_value = Value { id: 7 };
        let mut module = Module::new("loop_probe");
        let mut function = Function::new("main", Vec::new(), Type::Void);
        function.blocks.push(BasicBlock {
            id: 0,
            label: "entry".to_string(),
            instructions: vec![
                Instruction {
                    id: 0,
                    kind: InstructionKind::Load {
                        result: loss_value,
                        ptr: Value { id: 1 },
                        ty: Type::Unknown,
                    },
                    source_span: None,
                },
                Instruction {
                    id: 1,
                    kind: InstructionKind::HostCall {
                        result: None,
                        host: "spectra.compiler.autodiff_region".to_string(),
                        args: vec![loss_value],
                        result_type: None,
                    },
                    source_span: None,
                },
            ],
            terminator: Some(Terminator::Return { value: None }),
        });
        module.functions.push(function);
        let error = materialize_autodiff_steps(&mut module)
            .expect_err("loop-carried loads must not lower silently");
        assert!(error.contains("E3004"), "{error}");
        assert!(error.contains("loop"), "{error}");
    }
}
