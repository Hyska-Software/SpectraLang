// Strength reduction optimization pass
//
// Performs algebraic simplifications that replace expensive operations with
// cheaper equivalents:
//
//   x * 0  =>  0        (eliminate multiply-by-zero)
//   x * 1  =>  x        (eliminate identity multiply)
//   x + 0  =>  x        (eliminate identity add)
//   x - 0  =>  x        (eliminate identity subtract)
//   x / 1  =>  x        (eliminate identity divide)
//   x % 1  =>  0        (modulo by 1 is always 0)
//   x == x =>  true     (trivial equality — same SSA value)
//   x != x =>  false    (trivial inequality)

use crate::ir::{InstructionKind, Module, Value};
use crate::passes::Pass;
use std::collections::HashMap;

pub struct StrengthReduction;

impl StrengthReduction {
    pub fn new() -> Self {
        Self
    }
}

enum Replacement {
    /// Replace the instruction with `ConstInt { result, value }`.
    Constant(Value, i64),
    /// Replace the instruction with `ConstBool { result, value }`.
    ConstantBool(Value, bool),
    /// Replace the instruction with `Copy { result, source }`.
    Copy(Value, Value),
}

impl Pass for StrengthReduction {
    fn name(&self) -> &str {
        "StrengthReduction"
    }

    fn run(&mut self, module: &mut Module) -> bool {
        let mut modified = false;

        for func in &mut module.functions {
            // Build a map: value_id → constant integer value
            let mut int_consts: HashMap<usize, i64> = HashMap::new();

            for block in &func.blocks {
                for instr in &block.instructions {
                    if let InstructionKind::ConstInt { result, value } = &instr.kind {
                        int_consts.insert(result.id, *value);
                    }
                }
            }

            // Collect replacements
            let mut replacements: Vec<(usize, usize, Replacement)> = Vec::new();

            for (bi, block) in func.blocks.iter().enumerate() {
                for (ii, instr) in block.instructions.iter().enumerate() {
                    let replacement = match &instr.kind {
                        // ── Multiply ──────────────────────────────────────
                        InstructionKind::Mul { result, lhs, rhs } => {
                            let lv = int_consts.get(&lhs.id).copied();
                            let rv = int_consts.get(&rhs.id).copied();
                            if lv == Some(0) || rv == Some(0) {
                                Some(Replacement::Constant(*result, 0))
                            } else if rv == Some(1) {
                                Some(Replacement::Copy(*result, *lhs))
                            } else if lv == Some(1) {
                                Some(Replacement::Copy(*result, *rhs))
                            } else {
                                None
                            }
                        }
                        // ── Add ───────────────────────────────────────────
                        InstructionKind::Add { result, lhs, rhs } => {
                            let lv = int_consts.get(&lhs.id).copied();
                            let rv = int_consts.get(&rhs.id).copied();
                            if rv == Some(0) {
                                Some(Replacement::Copy(*result, *lhs))
                            } else if lv == Some(0) {
                                Some(Replacement::Copy(*result, *rhs))
                            } else {
                                None
                            }
                        }
                        // ── Subtract ──────────────────────────────────────
                        InstructionKind::Sub { result, lhs, rhs } => {
                            let rv = int_consts.get(&rhs.id).copied();
                            if rv == Some(0) {
                                Some(Replacement::Copy(*result, *lhs))
                            } else {
                                None
                            }
                        }
                        // ── Divide ────────────────────────────────────────
                        InstructionKind::Div { result, lhs, rhs } => {
                            let rv = int_consts.get(&rhs.id).copied();
                            if rv == Some(1) {
                                Some(Replacement::Copy(*result, *lhs))
                            } else {
                                None
                            }
                        }
                        // ── Remainder ─────────────────────────────────────
                        InstructionKind::Rem { result, rhs, .. } => {
                            let rv = int_consts.get(&rhs.id).copied();
                            if rv == Some(1) {
                                Some(Replacement::Constant(*result, 0))
                            } else {
                                None
                            }
                        }
                        // ── Trivial equality (same SSA id) ────────────────
                        InstructionKind::Eq { result, lhs, rhs } => {
                            if lhs.id == rhs.id {
                                Some(Replacement::ConstantBool(*result, true))
                            } else {
                                None
                            }
                        }
                        InstructionKind::Ne { result, lhs, rhs } => {
                            if lhs.id == rhs.id {
                                Some(Replacement::ConstantBool(*result, false))
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };

                    if let Some(rep) = replacement {
                        replacements.push((bi, ii, rep));
                    }
                }
            }

            // Apply replacements
            for (bi, ii, rep) in replacements {
                if let Some(block) = func.blocks.get_mut(bi) {
                    if let Some(instr) = block.instructions.get_mut(ii) {
                        instr.kind = match rep {
                            Replacement::Constant(result, value) => {
                                int_consts.insert(result.id, value);
                                InstructionKind::ConstInt { result, value }
                            }
                            Replacement::ConstantBool(result, value) => {
                                InstructionKind::ConstBool { result, value }
                            }
                            Replacement::Copy(result, source) => {
                                // Propagate the constant if the source was one
                                if let Some(&v) = int_consts.get(&source.id) {
                                    int_consts.insert(result.id, v);
                                }
                                InstructionKind::Copy { result, source }
                            }
                        };
                        modified = true;
                    }
                }
            }
        }

        modified
    }
}

impl Default for StrengthReduction {
    fn default() -> Self {
        Self::new()
    }
}

/// Free helper — cria e executa o passo em uma única chamada.
pub fn run(module: &mut crate::ir::Module) -> bool {
    StrengthReduction::new().run(module)
}
