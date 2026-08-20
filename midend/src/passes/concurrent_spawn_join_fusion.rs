//! Conservative fusion for the synchronous concurrent fast path.
//!
//! `task_spawn` currently materializes a value and `task_join` consumes its
//! handle immediately.  When the handle has exactly one use and both calls
//! are in the same block, the pair can be represented by one host call.  The
//! pass deliberately accepts only pure instructions between the calls and
//! never crosses a control-flow boundary.

use std::collections::HashMap;

use crate::ir::{InstructionKind, Module, Terminator, Value};
use crate::passes::Pass;

const SPAWN: &str = "spectra.std.concurrent.task_spawn";
const JOIN: &str = "spectra.std.concurrent.task_join";
const FUSED: &str = "spectra.std.concurrent.task_spawn_join";

pub struct ConcurrentSpawnJoinFusion;

impl ConcurrentSpawnJoinFusion {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ConcurrentSpawnJoinFusion {
    fn default() -> Self {
        Self::new()
    }
}

impl Pass for ConcurrentSpawnJoinFusion {
    fn name(&self) -> &str {
        "ConcurrentSpawnJoinFusion"
    }

    fn run(&mut self, module: &mut Module) -> bool {
        let mut modified = false;
        for function in &mut module.functions {
            let mut uses = HashMap::<usize, usize>::new();
            for block in &function.blocks {
                for instruction in &block.instructions {
                    for value in instruction_inputs(&instruction.kind) {
                        *uses.entry(value.id).or_default() += 1;
                    }
                }
                if let Some(terminator) = &block.terminator {
                    for value in terminator_inputs(terminator) {
                        *uses.entry(value.id).or_default() += 1;
                    }
                }
            }

            for block in &mut function.blocks {
                let mut index = 0;
                while index < block.instructions.len() {
                    let (spawn_value, spawn_arg) = match &block.instructions[index].kind {
                        InstructionKind::HostCall {
                            result: Some(result),
                            host,
                            args,
                            ..
                        } if host == SPAWN && args.len() == 1 => (*result, args[0]),
                        _ => {
                            index += 1;
                            continue;
                        }
                    };

                    if uses.get(&spawn_value.id).copied() != Some(1) {
                        index += 1;
                        continue;
                    }

                    let Some(join_index) = (index + 1..block.instructions.len()).find(|&candidate| {
                        matches!(
                            &block.instructions[candidate].kind,
                            InstructionKind::HostCall { args, host, .. }
                                if host == JOIN && args.len() == 1 && args[0].id == spawn_value.id
                        )
                    }) else {
                        index += 1;
                        continue;
                    };

                    // A handle must not be observed by any instruction other
                    // than the matching join, and no effectful operation may
                    // be moved across the pair.
                    let safe_gap =
                        block.instructions[index + 1..join_index]
                            .iter()
                            .all(|instruction| {
                                instruction_inputs(&instruction.kind)
                                    .into_iter()
                                    .all(|value| value.id != spawn_value.id)
                                    && is_pure(&instruction.kind)
                            });
                    if !safe_gap {
                        index += 1;
                        continue;
                    }

                    let (join_result, join_result_type) = match &block.instructions[join_index].kind
                    {
                        InstructionKind::HostCall {
                            result,
                            result_type,
                            ..
                        } => (*result, result_type.clone()),
                        _ => unreachable!(),
                    };

                    // The result of join is the observable result of the
                    // fused operation.  Keep the spawn instruction's stable
                    // position and remove only the join; pure gap instructions
                    // remain in their original evaluation order.
                    block.instructions[index].kind = InstructionKind::HostCall {
                        result: join_result,
                        host: FUSED.to_string(),
                        args: vec![spawn_arg],
                        result_type: join_result_type,
                    };
                    block.instructions.remove(join_index);
                    modified = true;
                    index += 1;
                }
            }
        }
        modified
    }
}

fn is_pure(kind: &InstructionKind) -> bool {
    matches!(
        kind,
        InstructionKind::Add { .. }
            | InstructionKind::Sub { .. }
            | InstructionKind::Mul { .. }
            | InstructionKind::Div { .. }
            | InstructionKind::Rem { .. }
            | InstructionKind::Eq { .. }
            | InstructionKind::Ne { .. }
            | InstructionKind::Lt { .. }
            | InstructionKind::Le { .. }
            | InstructionKind::Gt { .. }
            | InstructionKind::Ge { .. }
            | InstructionKind::And { .. }
            | InstructionKind::Or { .. }
            | InstructionKind::Not { .. }
            | InstructionKind::Load { .. }
            | InstructionKind::GlobalAddr { .. }
            | InstructionKind::Copy { .. }
            | InstructionKind::ConstInt { .. }
            | InstructionKind::ConstIntTyped { .. }
            | InstructionKind::ConstFloat { .. }
            | InstructionKind::ConstFloatTyped { .. }
            | InstructionKind::ConstBool { .. }
            | InstructionKind::ConstString { .. }
            | InstructionKind::Cast { .. }
            | InstructionKind::AutodiffStep { .. }
    )
}

fn instruction_inputs(kind: &InstructionKind) -> Vec<Value> {
    match kind {
        InstructionKind::Add { lhs, rhs, .. }
        | InstructionKind::Sub { lhs, rhs, .. }
        | InstructionKind::Mul { lhs, rhs, .. }
        | InstructionKind::Div { lhs, rhs, .. }
        | InstructionKind::Rem { lhs, rhs, .. }
        | InstructionKind::Eq { lhs, rhs, .. }
        | InstructionKind::Ne { lhs, rhs, .. }
        | InstructionKind::Lt { lhs, rhs, .. }
        | InstructionKind::Le { lhs, rhs, .. }
        | InstructionKind::Gt { lhs, rhs, .. }
        | InstructionKind::Ge { lhs, rhs, .. }
        | InstructionKind::And { lhs, rhs, .. }
        | InstructionKind::Or { lhs, rhs, .. } => vec![*lhs, *rhs],
        InstructionKind::Not { operand, .. } | InstructionKind::Cast { operand, .. } => {
            vec![*operand]
        }
        InstructionKind::Load { ptr, .. } => vec![*ptr],
        InstructionKind::Store { ptr, value } => vec![*ptr, *value],
        InstructionKind::GetElementPtr { ptr, index, .. } => vec![*ptr, *index],
        InstructionKind::FieldPtr { ptr, .. } => vec![*ptr],
        InstructionKind::EscapeManualAlloc { ptr } => vec![*ptr],
        InstructionKind::Call { args, .. } | InstructionKind::HostCall { args, .. } => args.clone(),
        InstructionKind::CallIndirect { fn_ptr, args, .. } => {
            let mut values = vec![*fn_ptr];
            values.extend(args.iter().copied());
            values
        }
        InstructionKind::AsyncSuspend { task, .. }
        | InstructionKind::AsyncResume { task, .. }
        | InstructionKind::LoadDynDataPtr { fat_ptr: task, .. }
        | InstructionKind::LoadDynVtablePtr { fat_ptr: task, .. } => vec![*task],
        InstructionKind::AsyncReady { value, .. } => value.iter().copied().collect(),
        InstructionKind::Phi { incoming, .. } => incoming.iter().map(|(value, _)| *value).collect(),
        InstructionKind::Copy { source, .. } => vec![*source],
        InstructionKind::MakeDynFatPtr {
            data_ptr,
            vtable_ptr,
            ..
        } => vec![*data_ptr, *vtable_ptr],
        InstructionKind::LoadVtableSlot { vtable_ptr, .. } => vec![*vtable_ptr],
        InstructionKind::Alloca { .. }
        | InstructionKind::GlobalAddr { .. }
        | InstructionKind::ManualAlloc { .. }
        | InstructionKind::FuncAddr { .. }
        | InstructionKind::ConstInt { .. }
        | InstructionKind::ConstIntTyped { .. }
        | InstructionKind::ConstFloat { .. }
        | InstructionKind::ConstFloatTyped { .. }
        | InstructionKind::ConstBool { .. }
        | InstructionKind::ConstString { .. } => Vec::new(),
        InstructionKind::AutodiffStep {
            upstream,
            inputs,
            targets,
            ..
        } => {
            let mut values = Vec::new();
            if let Some(value) = upstream {
                values.push(*value);
            }
            values.extend(inputs.iter().copied());
            values.extend(targets.iter().copied());
            values
        }
    }
}

fn terminator_inputs(terminator: &Terminator) -> Vec<Value> {
    match terminator {
        Terminator::Return { value: Some(value) }
        | Terminator::CondBranch {
            condition: value, ..
        }
        | Terminator::Switch { value, .. } => vec![*value],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BasicBlock, Function, Instruction, Module, Type};

    fn host(id: usize, result: usize, name: &str, arg: usize) -> Instruction {
        Instruction {
            id,
            kind: InstructionKind::HostCall {
                result: Some(Value { id: result }),
                host: name.to_string(),
                args: vec![Value { id: arg }],
                result_type: Some(Type::Int),
            },
            source_span: None,
        }
    }

    fn module_with(instructions: Vec<Instruction>) -> Module {
        let mut function = Function::new("main", vec![], Type::Int);
        function.blocks.push(BasicBlock {
            id: 0,
            label: "entry".into(),
            instructions,
            terminator: Some(Terminator::Return { value: None }),
        });
        let mut module = Module::new("test");
        module.add_function(function);
        module
    }

    #[test]
    fn fuses_single_use_pair_with_pure_gap() {
        let mut module = module_with(vec![
            host(0, 2, SPAWN, 1),
            Instruction {
                id: 1,
                kind: InstructionKind::Load {
                    result: Value { id: 3 },
                    ptr: Value { id: 4 },
                    ty: Type::Int,
                },
                source_span: None,
            },
            host(2, 5, JOIN, 2),
        ]);
        assert!(ConcurrentSpawnJoinFusion::new().run(&mut module));
        assert_eq!(module.functions[0].blocks[0].instructions.len(), 2);
        assert!(
            matches!(module.functions[0].blocks[0].instructions[0].kind, InstructionKind::HostCall { ref host, .. } if host == FUSED)
        );
    }

    #[test]
    fn does_not_fuse_escaped_handle() {
        let mut module = module_with(vec![
            host(0, 2, SPAWN, 1),
            host(1, 5, JOIN, 2),
            Instruction {
                id: 2,
                kind: InstructionKind::Copy {
                    result: Value { id: 6 },
                    source: Value { id: 2 },
                },
                source_span: None,
            },
        ]);
        assert!(!ConcurrentSpawnJoinFusion::new().run(&mut module));
    }
}
