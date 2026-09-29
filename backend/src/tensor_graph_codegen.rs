const FUSED_UNARY_HOST: &str = "spectra.runtime.tensor.fused_unary";

/// Apply the CPU unary plans selected by tensor-graph legalization to the IR
/// that is consumed by both the JIT and AOT code generators.
fn apply_tensor_graph_fusions(
    module: &mut IRModule,
    original_graph: &TensorGraph,
    lowered_graph: &TensorGraph,
) -> BackendResult<()> {
    for lowered_function in &lowered_graph.functions {
        let Some(original_function) = original_graph
            .functions
            .iter()
            .find(|function| function.name == lowered_function.name)
        else {
            continue;
        };
        for node in &lowered_function.nodes {
            let TensorGraphOp::FusedElementwise { ops } = &node.op else {
                continue;
            };
            let function = module
                .functions
                .iter_mut()
                .find(|function| function.name == lowered_function.name)
                .ok_or_else(|| {
                    BackendCodegenError::tensor_ir(format!(
                        "Tensor IR fusion references missing function '{}'",
                        lowered_function.name
                    ))
                })?;
            apply_fused_unary_plan(function, original_function, node, ops)?;
        }
    }
    Ok(())
}

fn apply_fused_unary_plan(
    function: &mut IRFunction,
    graph_function: &TensorGraphFunction,
    fused_node: &TensorGraphNode,
    ops: &[String],
) -> BackendResult<()> {
    if ops.len() < 2 || ops.len() > 8 {
        return Err(BackendCodegenError::tensor_ir(format!(
            "Invalid fused unary chain length {} in '{}'",
            ops.len(),
            function.name
        )));
    }

    let mut reverse_chain = Vec::with_capacity(ops.len());
    let mut current = fused_node
        .value
        .and_then(|value| {
            graph_function
                .nodes
                .iter()
                .find(|node| node.value == Some(value))
        })
        .ok_or_else(|| {
            BackendCodegenError::tensor_ir(format!(
                "Fused unary output is missing from the original graph in '{}'",
                function.name
            ))
        })?;
    for expected in ops.iter().rev() {
        match &current.op {
            TensorGraphOp::Elementwise { name } if name == expected => {}
            _ => {
                return Err(BackendCodegenError::tensor_ir(format!(
                    "Fused unary plan does not match its source graph in '{}'",
                    function.name
                )))
            }
        }
        reverse_chain.push(current);
        if reverse_chain.len() < ops.len() {
            let input = current.inputs.first().copied().ok_or_else(|| {
                BackendCodegenError::tensor_ir(format!(
                    "Fused unary node has no input in '{}'",
                    function.name
                ))
            })?;
            current = graph_function
                .nodes
                .iter()
                .find(|node| node.id == input)
                .ok_or_else(|| {
                    BackendCodegenError::tensor_ir(format!(
                        "Fused unary input is missing from the original graph in '{}'",
                        function.name
                    ))
                })?;
        }
    }
    reverse_chain.reverse();

    let base_node_id = *reverse_chain
        .first()
        .and_then(|node| node.inputs.first())
        .ok_or_else(|| {
            BackendCodegenError::tensor_ir(format!(
                "Fused unary chain has no source value in '{}'",
                function.name
            ))
        })?;
    let base_value = graph_function
        .nodes
        .iter()
        .find(|node| node.id == base_node_id)
        .and_then(|node| node.value)
        .map(|id| IRValue { id })
        .ok_or_else(|| {
            BackendCodegenError::tensor_ir(format!(
                "Fused unary source has no SSA value in '{}'",
                function.name
            ))
        })?;

    let block_id = reverse_chain[0].source.block;
    if reverse_chain
        .iter()
        .any(|node| node.source.block != block_id)
    {
        return Err(BackendCodegenError::tensor_ir(format!(
            "Fused unary chain crosses basic blocks in '{}'",
            function.name
        )));
    }

    let mut source_values = Vec::with_capacity(reverse_chain.len());
    let mut instruction_ids = Vec::with_capacity(reverse_chain.len());
    let mut previous_value = base_value;
    for (index, node) in reverse_chain.iter().enumerate() {
        let expected_host = fused_unary_host(&ops[index]).ok_or_else(|| {
            BackendCodegenError::tensor_ir(format!(
                "Unsupported fused unary operation '{}' in '{}'",
                ops[index], function.name
            ))
        })?;
        let instruction = function
            .blocks
            .iter()
            .find(|block| block.id == block_id)
            .and_then(|block| {
                block
                    .instructions
                    .iter()
                    .find(|instruction| instruction.id == node.source.instruction)
            })
            .ok_or_else(|| {
                BackendCodegenError::tensor_ir(format!(
                    "Fused unary source instruction {} is missing in '{}'",
                    node.source.instruction, function.name
                ))
            })?;
        let InstructionKind::HostCall {
            result: Some(result),
            host,
            args,
            ..
        } = &instruction.kind
        else {
            return Err(BackendCodegenError::tensor_ir(format!(
                "Fused unary source is not a value-producing host call in '{}'",
                function.name
            )));
        };
        if host != expected_host || args.as_slice() != [previous_value] {
            return Err(BackendCodegenError::tensor_ir(format!(
                "Fused unary source operands do not match the plan in '{}'",
                function.name
            )));
        }
        if node.value != Some(result.id) {
            return Err(BackendCodegenError::tensor_ir(format!(
                "Fused unary SSA result does not match the graph in '{}'",
                function.name
            )));
        }
        source_values.push(*result);
        instruction_ids.push(instruction.id);
        previous_value = *result;
    }

    // Prove every removed intermediate has exactly its next fused call as a
    // use. This guards against graph extraction missing a scalar or IR use.
    for (index, value) in source_values
        .iter()
        .take(source_values.len() - 1)
        .enumerate()
    {
        let mut uses = Vec::new();
        for block in &function.blocks {
            for instruction in &block.instructions {
                for operand in
                    spectra_midend::passes::verification::instruction_operands(instruction)
                {
                    if operand == *value {
                        uses.push((block.id, instruction.id));
                    }
                }
            }
            if let Some(terminator) = &block.terminator {
                let operands = match terminator {
                    Terminator::Return { value } => value.iter().copied().collect::<Vec<_>>(),
                    Terminator::CondBranch { condition, .. } => vec![*condition],
                    Terminator::Switch { value, .. } => vec![*value],
                    Terminator::Branch { .. } | Terminator::Unreachable => Vec::new(),
                };
                if operands.contains(value) {
                    uses.push((block.id, usize::MAX));
                }
            }
        }
        if uses.as_slice() != [(block_id, instruction_ids[index + 1])] {
            return Err(BackendCodegenError::tensor_ir(format!(
                "Fused unary intermediate %{} has external uses in '{}'",
                value.id, function.name
            )));
        }
    }

    let descriptor = encode_fused_unary_descriptor(ops).ok_or_else(|| {
        BackendCodegenError::tensor_ir(format!(
            "Cannot encode fused unary chain in '{}'",
            function.name
        ))
    })?;
    let final_instruction_id = *instruction_ids.last().expect("chain is non-empty");
    let descriptor_instruction_id = function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter().map(|instruction| instruction.id))
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| {
            BackendCodegenError::tensor_ir(format!(
                "Instruction id overflow while lowering fusion in '{}'",
                function.name
            ))
        })?;
    let final_block = function
        .blocks
        .iter_mut()
        .find(|block| block.id == block_id)
        .ok_or_else(|| {
            BackendCodegenError::tensor_ir(format!(
                "Fused unary block {block_id} is missing in '{}'",
                function.name
            ))
        })?;
    let final_index = final_block
        .instructions
        .iter()
        .position(|instruction| instruction.id == final_instruction_id)
        .ok_or_else(|| {
            BackendCodegenError::tensor_ir(format!(
                "Fused unary final instruction is missing in '{}'",
                function.name
            ))
        })?;
    let final_instruction = final_block.instructions[final_index].clone();
    let (result, result_type) = match final_instruction.kind {
        InstructionKind::HostCall {
            result: Some(result),
            result_type,
            ..
        } => (result, result_type),
        _ => unreachable!("fused source hostcall was validated above"),
    };
    let descriptor_value = IRValue {
        id: function.next_value_id,
    };
    function.next_value_id = function.next_value_id.checked_add(1).ok_or_else(|| {
        BackendCodegenError::tensor_ir(format!(
            "SSA value id overflow while lowering fusion in '{}'",
            function.name
        ))
    })?;
    let source_span = final_instruction.source_span.clone();
    let fused_call = Instruction {
        id: final_instruction_id,
        kind: InstructionKind::HostCall {
            result: Some(result),
            host: FUSED_UNARY_HOST.to_string(),
            args: vec![base_value, descriptor_value],
            result_type,
        },
        source_span: source_span.clone(),
    };
    let descriptor_const = Instruction {
        id: descriptor_instruction_id,
        kind: InstructionKind::ConstInt {
            result: descriptor_value,
            value: descriptor as i64,
        },
        source_span,
    };
    let removed = instruction_ids
        .iter()
        .take(instruction_ids.len() - 1)
        .copied()
        .collect::<HashSet<_>>();
    let mut rewritten = Vec::with_capacity(final_block.instructions.len() + 1);
    for instruction in final_block.instructions.drain(..) {
        if removed.contains(&instruction.id) {
            continue;
        }
        if instruction.id == final_instruction_id {
            rewritten.push(descriptor_const.clone());
            rewritten.push(fused_call.clone());
        } else {
            rewritten.push(instruction);
        }
    }
    final_block.instructions = rewritten;
    Ok(())
}

fn fused_unary_host(operation: &str) -> Option<&'static str> {
    match operation {
        "neg" => Some("spectra.std.tensor.neg"),
        "relu" => Some("spectra.std.tensor.relu"),
        "sigmoid_f" => Some("spectra.std.tensor.sigmoid_f"),
        "tanh_f" => Some("spectra.std.tensor.tanh_f"),
        "sqrt_f" => Some("spectra.std.tensor.sqrt_f"),
        "log_f" => Some("spectra.std.tensor.log_f"),
        _ => None,
    }
}

/// Store the operation count in bits 56..63 and one four-bit operation code
/// per operation, starting at bit zero. Codes are shared with the runtime.
fn encode_fused_unary_descriptor(ops: &[String]) -> Option<u64> {
    if ops.len() < 2 || ops.len() > 8 {
        return None;
    }
    let mut descriptor = (ops.len() as u64) << 56;
    for (index, operation) in ops.iter().enumerate() {
        let code = match operation.as_str() {
            "neg" => 1,
            "relu" => 2,
            "sigmoid_f" => 3,
            "tanh_f" => 4,
            "sqrt_f" => 5,
            "log_f" => 6,
            _ => return None,
        };
        descriptor |= code << (index * 4);
    }
    Some(descriptor)
}
