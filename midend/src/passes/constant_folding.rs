// Constant folding optimization pass

use crate::ir::Module;
use crate::passes::Pass;

pub struct ConstantFolding;

impl ConstantFolding {
    pub fn new() -> Self {
        Self
    }
}

impl Pass for ConstantFolding {
    fn name(&self) -> &str {
        "ConstantFolding"
    }

    fn run(&mut self, module: &mut Module) -> bool {
        use crate::ir::InstructionKind;
        use std::collections::HashMap;

        let mut modified = false;

        for func in &mut module.functions {
            if func.suspension_barrier {
                continue;
            }
            // Repeat to a fixed point so an SSA chain such as
            // `(5 + 3) * 2` is folded without relying on an accidental block
            // order. Each replacement is monotonic: an arithmetic instruction
            // becomes a constant and can never be selected again.
            loop {
                let mut constants: HashMap<usize, i64> = HashMap::new();
                for block in &func.blocks {
                    for instr in &block.instructions {
                        if let InstructionKind::ConstInt { result, value } = &instr.kind {
                            constants.insert(result.id, *value);
                        }
                    }
                }

                let mut replacements: Vec<(usize, usize, i64)> = Vec::new();
                for (block_idx, block) in func.blocks.iter().enumerate() {
                    for (instr_idx, instr) in block.instructions.iter().enumerate() {
                        let folded_value = match &instr.kind {
                            InstructionKind::Add { lhs, rhs, .. } => constants
                                .get(&lhs.id)
                                .zip(constants.get(&rhs.id))
                                .and_then(|(&lhs, &rhs)| lhs.checked_add(rhs)),
                            InstructionKind::Sub { lhs, rhs, .. } => constants
                                .get(&lhs.id)
                                .zip(constants.get(&rhs.id))
                                .and_then(|(&lhs, &rhs)| lhs.checked_sub(rhs)),
                            InstructionKind::Mul { lhs, rhs, .. } => constants
                                .get(&lhs.id)
                                .zip(constants.get(&rhs.id))
                                .and_then(|(&lhs, &rhs)| lhs.checked_mul(rhs)),
                            InstructionKind::Div { lhs, rhs, .. } => constants
                                .get(&lhs.id)
                                .zip(constants.get(&rhs.id))
                                .and_then(|(&lhs, &rhs)| lhs.checked_div(rhs)),
                            InstructionKind::Rem { lhs, rhs, .. } => constants
                                .get(&lhs.id)
                                .zip(constants.get(&rhs.id))
                                .and_then(|(&lhs, &rhs)| lhs.checked_rem(rhs)),
                            _ => None,
                        };

                        if let Some(value) = folded_value {
                            replacements.push((block_idx, instr_idx, value));
                        }
                    }
                }

                if replacements.is_empty() {
                    break;
                }

                for (block_idx, instr_idx, value) in replacements {
                    if let Some(block) = func.blocks.get_mut(block_idx) {
                        if let Some(instr) = block.instructions.get_mut(instr_idx) {
                            if let Some(result) = Self::get_result(&instr.kind) {
                                instr.kind = InstructionKind::ConstInt { result, value };
                                modified = true;
                            }
                        }
                    }
                }
            }
        }

        modified
    }
}

impl ConstantFolding {
    fn get_result(kind: &crate::ir::InstructionKind) -> Option<crate::ir::Value> {
        use crate::ir::InstructionKind;
        match kind {
            InstructionKind::Add { result, .. }
            | InstructionKind::Sub { result, .. }
            | InstructionKind::Mul { result, .. }
            | InstructionKind::Div { result, .. }
            | InstructionKind::Rem { result, .. } => Some(*result),
            _ => None,
        }
    }
}

/// Free helper — cria e executa o passo em uma única chamada.
pub fn run(module: &mut crate::ir::Module) -> bool {
    ConstantFolding::new().run(module)
}

impl Default for ConstantFolding {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BasicBlock, Function, Instruction, InstructionKind, Module, Terminator, Value};
    use std::panic::{catch_unwind, AssertUnwindSafe};

    fn add(result: Value, lhs: Value, rhs: Value) -> InstructionKind {
        InstructionKind::Add { result, lhs, rhs }
    }

    fn sub(result: Value, lhs: Value, rhs: Value) -> InstructionKind {
        InstructionKind::Sub { result, lhs, rhs }
    }

    fn mul(result: Value, lhs: Value, rhs: Value) -> InstructionKind {
        InstructionKind::Mul { result, lhs, rhs }
    }

    fn div(result: Value, lhs: Value, rhs: Value) -> InstructionKind {
        InstructionKind::Div { result, lhs, rhs }
    }

    fn rem(result: Value, lhs: Value, rhs: Value) -> InstructionKind {
        InstructionKind::Rem { result, lhs, rhs }
    }

    fn overflow_module(lhs: i64, rhs: i64, operation: fn(Value, Value, Value) -> InstructionKind) -> Module {
        let mut function = Function::new("overflow", Vec::new(), crate::ir::Type::Void);
        function.next_value_id = 3;
        function.next_block_id = 1;
        function.blocks.push(BasicBlock {
            id: 0,
            label: "entry".to_string(),
            instructions: vec![
                Instruction {
                    id: 0,
                    kind: InstructionKind::ConstInt {
                        result: Value { id: 0 },
                        value: lhs,
                    },
                    source_span: None,
                },
                Instruction {
                    id: 1,
                    kind: InstructionKind::ConstInt {
                        result: Value { id: 1 },
                        value: rhs,
                    },
                    source_span: None,
                },
                Instruction {
                    id: 2,
                    kind: operation(
                        Value { id: 2 },
                        Value { id: 0 },
                        Value { id: 1 },
                    ),
                    source_span: None,
                },
            ],
            terminator: Some(Terminator::Return { value: None }),
        });
        let mut module = Module::new("overflow");
        module.add_function(function);
        module
    }

    #[test]
    fn does_not_fold_integer_overflow_or_invalid_division() {
        type Operation = fn(Value, Value, Value) -> InstructionKind;
        let cases = [
            (
                i64::MAX,
                1,
                add as Operation,
            ),
            (
                i64::MIN,
                1,
                sub,
            ),
            (
                i64::MAX,
                2,
                mul,
            ),
            (
                i64::MIN,
                -1,
                div,
            ),
            (
                i64::MIN,
                -1,
                rem,
            ),
        ];

        for (lhs, rhs, operation) in cases {
            let mut module = overflow_module(lhs, rhs, operation);
            let result = catch_unwind(AssertUnwindSafe(|| ConstantFolding::new().run(&mut module)));
            assert!(result.is_ok(), "constant folding must not panic for {lhs} op {rhs}");
            assert!(!result.unwrap(), "undefined integer arithmetic must not be folded");
            assert!(matches!(
                module.functions[0].blocks[0].instructions[2].kind,
                InstructionKind::Add { .. }
                    | InstructionKind::Sub { .. }
                    | InstructionKind::Mul { .. }
                    | InstructionKind::Div { .. }
                    | InstructionKind::Rem { .. }
            ));
        }
    }
}
