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
            let mut constants: HashMap<usize, i64> = HashMap::new();
            let mut replacements: Vec<(usize, usize, i64)> = Vec::new(); // (block_idx, instr_idx, value)
                                                                         // Collect constants from ConstInt instructions
            for block in &func.blocks {
                for instr in &block.instructions {
                    if let InstructionKind::ConstInt { result, value } = &instr.kind {
                        constants.insert(result.id, *value);
                    }
                }
            }

            // Find arithmetic operations with constant operands
            for (block_idx, block) in func.blocks.iter().enumerate() {
                for (instr_idx, instr) in block.instructions.iter().enumerate() {
                    let folded_value = match &instr.kind {
                        InstructionKind::Add { lhs, rhs, .. } => {
                            if let (Some(&lhs_val), Some(&rhs_val)) =
                                (constants.get(&lhs.id), constants.get(&rhs.id))
                            {
                                Some(lhs_val + rhs_val)
                            } else {
                                None
                            }
                        }
                        InstructionKind::Sub { lhs, rhs, .. } => {
                            if let (Some(&lhs_val), Some(&rhs_val)) =
                                (constants.get(&lhs.id), constants.get(&rhs.id))
                            {
                                Some(lhs_val - rhs_val)
                            } else {
                                None
                            }
                        }
                        InstructionKind::Mul { lhs, rhs, .. } => {
                            if let (Some(&lhs_val), Some(&rhs_val)) =
                                (constants.get(&lhs.id), constants.get(&rhs.id))
                            {
                                Some(lhs_val * rhs_val)
                            } else {
                                None
                            }
                        }
                        InstructionKind::Div { lhs, rhs, .. } => {
                            if let (Some(&lhs_val), Some(&rhs_val)) =
                                (constants.get(&lhs.id), constants.get(&rhs.id))
                            {
                                if rhs_val != 0 {
                                    Some(lhs_val / rhs_val)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        InstructionKind::Rem { lhs, rhs, .. } => {
                            if let (Some(&lhs_val), Some(&rhs_val)) =
                                (constants.get(&lhs.id), constants.get(&rhs.id))
                            {
                                if rhs_val != 0 {
                                    Some(lhs_val % rhs_val)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };

                    if let Some(value) = folded_value {
                        replacements.push((block_idx, instr_idx, value));
                        modified = true;
                    }
                }
            }

            // Apply replacements
            for (block_idx, instr_idx, value) in replacements {
                if let Some(block) = func.blocks.get_mut(block_idx) {
                    if let Some(instr) = block.instructions.get_mut(instr_idx) {
                        if let Some(result) = Self::get_result(&instr.kind) {
                            instr.kind = InstructionKind::ConstInt { result, value };
                            constants.insert(result.id, value);
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
