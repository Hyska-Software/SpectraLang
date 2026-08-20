// Conservative function inlining for small internal helpers.

use crate::ir::{
    BasicBlock, Function, Instruction, InstructionKind, Module, Terminator, Type, Value,
};
use crate::passes::Pass;
use std::collections::{HashMap, HashSet};

const MAX_INLINE_BLOCKS: usize = 24;
const MAX_INLINE_INSTRUCTIONS: usize = 220;

#[derive(Clone)]
struct InlineCandidate {
    function: Function,
}

pub struct FunctionInlining;

impl FunctionInlining {
    pub fn new() -> Self {
        Self
    }
}

impl Pass for FunctionInlining {
    fn name(&self) -> &str {
        "FunctionInlining"
    }

    fn run(&mut self, module: &mut Module) -> bool {
        let mut modified = false;
        for _ in 0..4 {
            let call_graph = collect_direct_calls(module);
            let candidates = collect_candidates(module, &call_graph);
            if candidates.is_empty() {
                break;
            }

            let mut round_modified = false;
            for function in &mut module.functions {
                if inline_calls_in_function(function, &candidates) {
                    round_modified = true;
                }
            }

            if !round_modified {
                break;
            }
            modified = true;
        }
        modified
    }
}

fn collect_direct_calls(module: &Module) -> HashMap<String, HashSet<String>> {
    let mut calls = HashMap::new();
    for function in &module.functions {
        let entry = calls
            .entry(function.name.clone())
            .or_insert_with(HashSet::new);
        for block in &function.blocks {
            for instruction in &block.instructions {
                if let InstructionKind::Call { function, .. } = &instruction.kind {
                    entry.insert(function.clone());
                }
            }
        }
    }
    calls
}

fn collect_candidates(
    module: &Module,
    call_graph: &HashMap<String, HashSet<String>>,
) -> HashMap<String, InlineCandidate> {
    let mut candidates = HashMap::new();
    for function in &module.functions {
        if is_inline_candidate(function, call_graph) {
            candidates.insert(
                function.name.clone(),
                InlineCandidate {
                    function: function.clone(),
                },
            );
        }
    }
    candidates
}

fn is_inline_candidate(function: &Function, call_graph: &HashMap<String, HashSet<String>>) -> bool {
    if function.name == "main" || function.params.len() > 8 {
        return false;
    }
    if !matches!(function.return_type, Type::Int | Type::Bool | Type::Void) {
        return false;
    }
    if function.blocks.is_empty() || function.blocks.len() > MAX_INLINE_BLOCKS {
        return false;
    }
    if has_complex_control_labels(function) {
        return false;
    }
    if has_unreachable_loop_exit(function) {
        return false;
    }
    if call_graph
        .get(&function.name)
        .is_some_and(|calls| calls.contains(&function.name))
    {
        return false;
    }

    let mut instruction_count = 0usize;
    for block in &function.blocks {
        instruction_count += block.instructions.len();
        for instruction in &block.instructions {
            match &instruction.kind {
                InstructionKind::Alloca { ty, .. } if !is_inline_alloca_type(ty) => return false,
                InstructionKind::HostCall { .. }
                | InstructionKind::Call { .. }
                | InstructionKind::CallIndirect { .. }
                | InstructionKind::FuncAddr { .. }
                | InstructionKind::AsyncSuspend { .. }
                | InstructionKind::AsyncResume { .. }
                | InstructionKind::AsyncReady { .. }
                | InstructionKind::MakeDynFatPtr { .. }
                | InstructionKind::LoadDynDataPtr { .. }
                | InstructionKind::LoadDynVtablePtr { .. }
                | InstructionKind::LoadVtableSlot { .. } => return false,
                _ => {}
            }
        }
    }
    instruction_count <= MAX_INLINE_INSTRUCTIONS
}

fn has_complex_control_labels(function: &Function) -> bool {
    function.blocks.iter().any(|block| {
        block.label.contains("match")
            || block.label.contains("unless")
            || block.label.contains("block.result")
    })
}

fn has_unreachable_loop_exit(function: &Function) -> bool {
    let mut targets = HashSet::new();
    for block in &function.blocks {
        if let Some(terminator) = &block.terminator {
            collect_terminator_targets(terminator, &mut targets);
        }
    }

    function
        .blocks
        .iter()
        .any(|block| block.label.contains("loop.exit") && !targets.contains(&block.id))
}

fn collect_terminator_targets(terminator: &Terminator, targets: &mut HashSet<usize>) {
    match terminator {
        Terminator::Branch { target } => {
            targets.insert(*target);
        }
        Terminator::CondBranch {
            true_block,
            false_block,
            ..
        } => {
            targets.insert(*true_block);
            targets.insert(*false_block);
        }
        Terminator::Switch { cases, default, .. } => {
            for (_, target) in cases {
                targets.insert(*target);
            }
            targets.insert(*default);
        }
        Terminator::Return { .. } | Terminator::Unreachable => {}
    }
}

fn is_inline_alloca_type(ty: &Type) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::Char => true,
        Type::Array { element_type, size } => *size <= 4096 && is_inline_alloca_type(element_type),
        _ => false,
    }
}

fn inline_calls_in_function(
    function: &mut Function,
    candidates: &HashMap<String, InlineCandidate>,
) -> bool {
    let mut modified = false;
    while let Some((block_index, instruction_index, candidate_name)) =
        find_inline_call(function, candidates)
    {
        let candidate = candidates
            .get(&candidate_name)
            .expect("candidate disappeared")
            .clone();
        inline_call(function, block_index, instruction_index, &candidate);
        modified = true;
    }
    modified
}

fn find_inline_call(
    function: &Function,
    candidates: &HashMap<String, InlineCandidate>,
) -> Option<(usize, usize, String)> {
    for (block_index, block) in function.blocks.iter().enumerate() {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            if let InstructionKind::Call {
                function,
                args,
                result: _,
            } = &instruction.kind
            {
                if let Some(candidate) = candidates.get(function) {
                    if candidate.function.params.len() == args.len() {
                        return Some((block_index, instruction_index, function.clone()));
                    }
                }
            }
        }
    }
    None
}

fn inline_call(
    caller: &mut Function,
    block_index: usize,
    instruction_index: usize,
    candidate: &InlineCandidate,
) {
    let call_instruction = caller.blocks[block_index].instructions[instruction_index].clone();
    let (call_result, callee_name, call_args) = match &call_instruction.kind {
        InstructionKind::Call {
            result,
            function,
            args,
        } => (*result, function.clone(), args.clone()),
        _ => unreachable!("inline_call called for non-call"),
    };

    let continuation_id = caller.next_block_id;
    caller.next_block_id += 1;

    let mut block_id_map = HashMap::new();
    for block in &candidate.function.blocks {
        let new_id = caller.next_block_id;
        caller.next_block_id += 1;
        block_id_map.insert(block.id, new_id);
    }

    let mut value_map = HashMap::new();
    for (param, arg) in candidate.function.params.iter().zip(call_args.iter()) {
        value_map.insert(param.id, *arg);
    }
    for block in &candidate.function.blocks {
        for instruction in &block.instructions {
            if let Some(value) = instruction_result(&instruction.kind) {
                let new_value = caller.next_value();
                value_map.insert(value.id, new_value);
            }
        }
    }

    let result_slot = call_result.map(|_| caller.next_value());

    let original_terminator = caller.blocks[block_index].terminator.take();
    let tail = caller.blocks[block_index]
        .instructions
        .split_off(instruction_index + 1);
    caller.blocks[block_index].instructions.pop();

    if let Some(slot) = result_slot {
        let id = caller.blocks[block_index].instructions.len();
        caller.blocks[block_index].instructions.push(Instruction {
            id,
            kind: InstructionKind::Alloca {
                result: slot,
                ty: candidate.function.return_type.clone(),
            },
            source_span: None,
        });
    }

    let entry_id = candidate.function.blocks[0].id;
    caller.blocks[block_index].terminator = Some(Terminator::Branch {
        target: block_id_map[&entry_id],
    });

    let mut cloned_blocks = Vec::new();
    for block in &candidate.function.blocks {
        let mut cloned = BasicBlock {
            id: block_id_map[&block.id],
            label: format!("inline.{}.{}", callee_name, block.label),
            instructions: block
                .instructions
                .iter()
                .map(|instruction| clone_instruction(instruction, &value_map))
                .collect(),
            terminator: clone_terminator(
                block.terminator.as_ref(),
                &block_id_map,
                &value_map,
                continuation_id,
            ),
        };
        if let Some(Terminator::Return { value: Some(value) }) = &block.terminator {
            if let Some(slot) = result_slot {
                cloned.instructions.push(Instruction {
                    id: cloned.instructions.len(),
                    kind: InstructionKind::Store {
                        ptr: slot,
                        value: map_value(*value, &value_map),
                    },
                    source_span: None,
                });
            }
        }
        renumber_instructions(&mut cloned);
        cloned_blocks.push(cloned);
    }

    let mut continuation_instructions = Vec::new();
    if let (Some(result), Some(slot)) = (call_result, result_slot) {
        continuation_instructions.push(Instruction {
            id: 0,
            kind: InstructionKind::Load {
                result,
                ptr: slot,
                ty: candidate.function.return_type.clone(),
            },
            source_span: None,
        });
    }
    continuation_instructions.extend(tail);
    let mut continuation = BasicBlock {
        id: continuation_id,
        label: format!("inline.{}.cont", callee_name),
        instructions: continuation_instructions,
        terminator: original_terminator,
    };
    renumber_instructions(&mut continuation);

    let insert_at = block_index + 1;
    for (offset, block) in cloned_blocks.into_iter().enumerate() {
        caller.blocks.insert(insert_at + offset, block);
    }
    caller
        .blocks
        .insert(insert_at + candidate.function.blocks.len(), continuation);
}

fn clone_instruction(instruction: &Instruction, values: &HashMap<usize, Value>) -> Instruction {
    Instruction {
        id: instruction.id,
        kind: remap_instruction(&instruction.kind, values),
        source_span: instruction.source_span.clone(),
    }
}

fn remap_instruction(kind: &InstructionKind, values: &HashMap<usize, Value>) -> InstructionKind {
    match kind {
        InstructionKind::Add { result, lhs, rhs } => InstructionKind::Add {
            result: map_value(*result, values),
            lhs: map_value(*lhs, values),
            rhs: map_value(*rhs, values),
        },
        InstructionKind::Sub { result, lhs, rhs } => InstructionKind::Sub {
            result: map_value(*result, values),
            lhs: map_value(*lhs, values),
            rhs: map_value(*rhs, values),
        },
        InstructionKind::Mul { result, lhs, rhs } => InstructionKind::Mul {
            result: map_value(*result, values),
            lhs: map_value(*lhs, values),
            rhs: map_value(*rhs, values),
        },
        InstructionKind::Div { result, lhs, rhs } => InstructionKind::Div {
            result: map_value(*result, values),
            lhs: map_value(*lhs, values),
            rhs: map_value(*rhs, values),
        },
        InstructionKind::Rem { result, lhs, rhs } => InstructionKind::Rem {
            result: map_value(*result, values),
            lhs: map_value(*lhs, values),
            rhs: map_value(*rhs, values),
        },
        InstructionKind::Eq { result, lhs, rhs } => InstructionKind::Eq {
            result: map_value(*result, values),
            lhs: map_value(*lhs, values),
            rhs: map_value(*rhs, values),
        },
        InstructionKind::Ne { result, lhs, rhs } => InstructionKind::Ne {
            result: map_value(*result, values),
            lhs: map_value(*lhs, values),
            rhs: map_value(*rhs, values),
        },
        InstructionKind::Lt { result, lhs, rhs } => InstructionKind::Lt {
            result: map_value(*result, values),
            lhs: map_value(*lhs, values),
            rhs: map_value(*rhs, values),
        },
        InstructionKind::Le { result, lhs, rhs } => InstructionKind::Le {
            result: map_value(*result, values),
            lhs: map_value(*lhs, values),
            rhs: map_value(*rhs, values),
        },
        InstructionKind::Gt { result, lhs, rhs } => InstructionKind::Gt {
            result: map_value(*result, values),
            lhs: map_value(*lhs, values),
            rhs: map_value(*rhs, values),
        },
        InstructionKind::Ge { result, lhs, rhs } => InstructionKind::Ge {
            result: map_value(*result, values),
            lhs: map_value(*lhs, values),
            rhs: map_value(*rhs, values),
        },
        InstructionKind::And { result, lhs, rhs } => InstructionKind::And {
            result: map_value(*result, values),
            lhs: map_value(*lhs, values),
            rhs: map_value(*rhs, values),
        },
        InstructionKind::Or { result, lhs, rhs } => InstructionKind::Or {
            result: map_value(*result, values),
            lhs: map_value(*lhs, values),
            rhs: map_value(*rhs, values),
        },
        InstructionKind::Not { result, operand } => InstructionKind::Not {
            result: map_value(*result, values),
            operand: map_value(*operand, values),
        },
        InstructionKind::Alloca { result, ty } => InstructionKind::Alloca {
            result: map_value(*result, values),
            ty: ty.clone(),
        },
        InstructionKind::GlobalAddr { result, name, ty } => InstructionKind::GlobalAddr {
            result: map_value(*result, values),
            name: name.clone(),
            ty: ty.clone(),
        },
        InstructionKind::Load { result, ptr, ty } => InstructionKind::Load {
            result: map_value(*result, values),
            ptr: map_value(*ptr, values),
            ty: ty.clone(),
        },
        InstructionKind::Store { ptr, value } => InstructionKind::Store {
            ptr: map_value(*ptr, values),
            value: map_value(*value, values),
        },
        InstructionKind::GetElementPtr {
            result,
            ptr,
            index,
            element_type,
        } => InstructionKind::GetElementPtr {
            result: map_value(*result, values),
            ptr: map_value(*ptr, values),
            index: map_value(*index, values),
            element_type: element_type.clone(),
        },
        InstructionKind::FieldPtr {
            result,
            ptr,
            offset,
        } => InstructionKind::FieldPtr {
            result: map_value(*result, values),
            ptr: map_value(*ptr, values),
            offset: *offset,
        },
        InstructionKind::ManualAlloc { result, size } => InstructionKind::ManualAlloc {
            result: map_value(*result, values),
            size: *size,
        },
        InstructionKind::EscapeManualAlloc { ptr } => InstructionKind::EscapeManualAlloc {
            ptr: map_value(*ptr, values),
        },
        InstructionKind::Copy { result, source } => InstructionKind::Copy {
            result: map_value(*result, values),
            source: map_value(*source, values),
        },
        InstructionKind::Phi { result, incoming } => InstructionKind::Phi {
            result: map_value(*result, values),
            incoming: incoming
                .iter()
                .map(|(value, block)| (map_value(*value, values), *block))
                .collect(),
        },
        InstructionKind::ConstInt { result, value } => InstructionKind::ConstInt {
            result: map_value(*result, values),
            value: *value,
        },
        InstructionKind::ConstFloat { result, value } => InstructionKind::ConstFloat {
            result: map_value(*result, values),
            value: *value,
        },
        InstructionKind::ConstBool { result, value } => InstructionKind::ConstBool {
            result: map_value(*result, values),
            value: *value,
        },
        InstructionKind::Cast {
            result,
            operand,
            from_ty,
            to_ty,
        } => InstructionKind::Cast {
            result: map_value(*result, values),
            operand: map_value(*operand, values),
            from_ty: from_ty.clone(),
            to_ty: to_ty.clone(),
        },
        InstructionKind::HostCall {
            result,
            host,
            args,
            result_type,
        } => InstructionKind::HostCall {
            result: result.map(|value| map_value(value, values)),
            host: host.clone(),
            args: args.iter().map(|arg| map_value(*arg, values)).collect(),
            result_type: result_type.clone(),
        },
        other => other.clone(),
    }
}

fn clone_terminator(
    terminator: Option<&Terminator>,
    blocks: &HashMap<usize, usize>,
    values: &HashMap<usize, Value>,
    continuation_id: usize,
) -> Option<Terminator> {
    match terminator {
        Some(Terminator::Return { .. }) => Some(Terminator::Branch {
            target: continuation_id,
        }),
        Some(Terminator::Branch { target }) => Some(Terminator::Branch {
            target: blocks[target],
        }),
        Some(Terminator::CondBranch {
            condition,
            true_block,
            false_block,
        }) => Some(Terminator::CondBranch {
            condition: map_value(*condition, values),
            true_block: blocks[true_block],
            false_block: blocks[false_block],
        }),
        Some(Terminator::Switch {
            value,
            cases,
            default,
        }) => Some(Terminator::Switch {
            value: map_value(*value, values),
            cases: cases
                .iter()
                .map(|(case, block)| (*case, blocks[block]))
                .collect(),
            default: blocks[default],
        }),
        Some(Terminator::Unreachable) => Some(Terminator::Unreachable),
        None => None,
    }
}

fn map_value(value: Value, values: &HashMap<usize, Value>) -> Value {
    values.get(&value.id).copied().unwrap_or(value)
}

fn instruction_result(kind: &InstructionKind) -> Option<Value> {
    match kind {
        InstructionKind::Add { result, .. }
        | InstructionKind::Sub { result, .. }
        | InstructionKind::Mul { result, .. }
        | InstructionKind::Div { result, .. }
        | InstructionKind::Rem { result, .. }
        | InstructionKind::Eq { result, .. }
        | InstructionKind::Ne { result, .. }
        | InstructionKind::Lt { result, .. }
        | InstructionKind::Le { result, .. }
        | InstructionKind::Gt { result, .. }
        | InstructionKind::Ge { result, .. }
        | InstructionKind::And { result, .. }
        | InstructionKind::Or { result, .. }
        | InstructionKind::Not { result, .. }
        | InstructionKind::Alloca { result, .. }
        | InstructionKind::GlobalAddr { result, .. }
        | InstructionKind::ManualAlloc { result, .. }
        | InstructionKind::Load { result, .. }
        | InstructionKind::GetElementPtr { result, .. }
        | InstructionKind::FieldPtr { result, .. }
        | InstructionKind::Copy { result, .. }
        | InstructionKind::Phi { result, .. }
        | InstructionKind::ConstInt { result, .. }
        | InstructionKind::ConstIntTyped { result, .. }
        | InstructionKind::ConstFloat { result, .. }
        | InstructionKind::ConstFloatTyped { result, .. }
        | InstructionKind::ConstBool { result, .. }
        | InstructionKind::ConstString { result, .. }
        | InstructionKind::Cast { result, .. }
        | InstructionKind::FuncAddr { result, .. }
        | InstructionKind::MakeDynFatPtr { result, .. }
        | InstructionKind::LoadDynDataPtr { result, .. }
        | InstructionKind::LoadDynVtablePtr { result, .. }
        | InstructionKind::LoadVtableSlot { result, .. } => Some(*result),
        InstructionKind::Call { result, .. }
        | InstructionKind::HostCall { result, .. }
        | InstructionKind::CallIndirect { result, .. } => *result,
        _ => None,
    }
}

fn renumber_instructions(block: &mut BasicBlock) {
    for (index, instruction) in block.instructions.iter_mut().enumerate() {
        instruction.id = index;
    }
}

pub fn run(module: &mut Module) -> bool {
    FunctionInlining::new().run(module)
}

impl Default for FunctionInlining {
    fn default() -> Self {
        Self::new()
    }
}
