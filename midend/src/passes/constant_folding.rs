// Constant folding optimization pass

use crate::ir::{InstructionKind, Module, Type, Value};
use crate::passes::Pass;
use std::collections::HashMap;

pub struct ConstantFolding;

impl ConstantFolding {
    pub fn new() -> Self {
        Self
    }
}

/// A compile-time constant recognized by the fold rules.
#[derive(Debug, Clone)]
enum KnownConstant {
    /// Integer constant. `ty` is `None` for the historical untyped
    /// `ConstInt` encoding and `Some` for `ConstIntTyped`.
    Int { value: i64, ty: Option<Type> },
    /// Boolean constant (`ConstBool`).
    Bool(bool),
}

/// Integer fold operations.
#[derive(Clone, Copy)]
enum IntOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl Pass for ConstantFolding {
    fn name(&self) -> &str {
        "ConstantFolding"
    }

    fn run(&mut self, module: &mut Module) -> bool {
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
                let mut constants: HashMap<usize, KnownConstant> = HashMap::new();
                for block in &func.blocks {
                    for instr in &block.instructions {
                        match &instr.kind {
                            InstructionKind::ConstInt { result, value } => {
                                constants.insert(
                                    result.id,
                                    KnownConstant::Int {
                                        value: *value,
                                        ty: None,
                                    },
                                );
                            }
                            InstructionKind::ConstIntTyped { result, value, ty } => {
                                constants.insert(
                                    result.id,
                                    KnownConstant::Int {
                                        value: *value,
                                        ty: Some(ty.clone()),
                                    },
                                );
                            }
                            InstructionKind::ConstBool { result, value } => {
                                constants.insert(result.id, KnownConstant::Bool(*value));
                            }
                            _ => {}
                        }
                    }
                }

                let mut replacements: Vec<(usize, usize, KnownConstant)> = Vec::new();
                for (block_idx, block) in func.blocks.iter().enumerate() {
                    for (instr_idx, instr) in block.instructions.iter().enumerate() {
                        let folded_value = match &instr.kind {
                            InstructionKind::Add { lhs, rhs, .. } => {
                                fold_int_binop(&constants, *lhs, *rhs, IntOp::Add, false)
                            }
                            InstructionKind::Sub { lhs, rhs, .. } => {
                                fold_int_binop(&constants, *lhs, *rhs, IntOp::Sub, false)
                            }
                            InstructionKind::Mul { lhs, rhs, .. } => {
                                fold_int_binop(&constants, *lhs, *rhs, IntOp::Mul, false)
                            }
                            InstructionKind::Div {
                                lhs,
                                rhs,
                                unsigned,
                                ..
                            } => fold_int_binop(&constants, *lhs, *rhs, IntOp::Div, *unsigned),
                            InstructionKind::Rem {
                                lhs,
                                rhs,
                                unsigned,
                                ..
                            } => fold_int_binop(&constants, *lhs, *rhs, IntOp::Rem, *unsigned),
                            InstructionKind::And { lhs, rhs, .. } => {
                                fold_bool_binop(&constants, *lhs, *rhs, true)
                            }
                            InstructionKind::Or { lhs, rhs, .. } => {
                                fold_bool_binop(&constants, *lhs, *rhs, false)
                            }
                            InstructionKind::Not { operand, .. } => {
                                match constants.get(&operand.id) {
                                    Some(KnownConstant::Bool(value)) => {
                                        Some(KnownConstant::Bool(!value))
                                    }
                                    _ => None,
                                }
                            }
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
                                instr.kind = match value {
                                    KnownConstant::Int { value, ty: None } => {
                                        InstructionKind::ConstInt { result, value }
                                    }
                                    KnownConstant::Int { value, ty: Some(ty) } => {
                                        InstructionKind::ConstIntTyped { result, value, ty }
                                    }
                                    KnownConstant::Bool(value) => {
                                        InstructionKind::ConstBool { result, value }
                                    }
                                };
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
    fn get_result(kind: &InstructionKind) -> Option<Value> {
        match kind {
            InstructionKind::Add { result, .. }
            | InstructionKind::Sub { result, .. }
            | InstructionKind::Mul { result, .. }
            | InstructionKind::Div { result, .. }
            | InstructionKind::Rem { result, .. }
            | InstructionKind::And { result, .. }
            | InstructionKind::Or { result, .. }
            | InstructionKind::Not { result, .. } => Some(*result),
            _ => None,
        }
    }
}

/// Fold one integer binary operation over two known constants.
///
/// Untyped `ConstInt` operands keep the historical behavior (fold whenever
/// `checked_*` succeeds). Typed operands fold only when both sides carry the
/// *same* IR type and both inputs plus the result fit that type's range, so a
/// typed fold can never introduce a value that would require wrap-around.
fn fold_int_binop(
    constants: &HashMap<usize, KnownConstant>,
    lhs: Value,
    rhs: Value,
    op: IntOp,
    unsigned: bool,
) -> Option<KnownConstant> {
    let (lhs_value, lhs_ty, rhs_value, rhs_ty) = match (
        constants.get(&lhs.id),
        constants.get(&rhs.id),
    ) {
        (
            Some(KnownConstant::Int {
                value: lhs,
                ty: lhs_ty,
            }),
            Some(KnownConstant::Int {
                value: rhs,
                ty: rhs_ty,
            }),
        ) => (*lhs, lhs_ty.clone(), *rhs, rhs_ty.clone()),
        _ => return None,
    };

    // Mixing a typed constant with an untyped one would force the fold to
    // guess the result type; leave those to the typed lowering paths.
    let result_ty = match (&lhs_ty, &rhs_ty) {
        (None, None) => None,
        (Some(lhs_ty), Some(rhs_ty)) if lhs_ty == rhs_ty => Some(lhs_ty.clone()),
        _ => return None,
    };

    // An unsigned `Div`/`Rem` reinterprets the operand bits; a negative
    // operand would take a different path than the signed fold below. For
    // non-negative operands signed and unsigned semantics coincide (typed fit
    // checks already keep unsigned typed values non-negative).
    if unsigned && (lhs_value < 0 || rhs_value < 0) {
        return None;
    }

    let value = match op {
        IntOp::Add => lhs_value.checked_add(rhs_value),
        IntOp::Sub => lhs_value.checked_sub(rhs_value),
        IntOp::Mul => lhs_value.checked_mul(rhs_value),
        // `checked_div`/`checked_rem` reject division by zero and
        // `MIN / -1`, the two cases the backend turns into runtime traps.
        IntOp::Div => lhs_value.checked_div(rhs_value),
        IntOp::Rem => lhs_value.checked_rem(rhs_value),
    }?;

    if let Some(ty) = &result_ty {
        if !value_fits_int_type(lhs_value, ty)
            || !value_fits_int_type(rhs_value, ty)
            || !value_fits_int_type(value, ty)
        {
            return None;
        }
    }

    Some(KnownConstant::Int {
        value,
        ty: result_ty,
    })
}

/// Fold `And`/`Or` only when *both* operands are known booleans: the same
/// opcodes also model integer bitwise operations, which are left alone.
fn fold_bool_binop(
    constants: &HashMap<usize, KnownConstant>,
    lhs: Value,
    rhs: Value,
    and: bool,
) -> Option<KnownConstant> {
    match (constants.get(&lhs.id), constants.get(&rhs.id)) {
        (Some(KnownConstant::Bool(lhs)), Some(KnownConstant::Bool(rhs))) => {
            Some(KnownConstant::Bool(if and {
                *lhs && *rhs
            } else {
                *lhs || *rhs
            }))
        }
        _ => None,
    }
}

/// Inclusive range of `i64` values representable by an IR integer-like type.
/// `None` for types that are not integer-backed.
fn int_type_range(ty: &Type) -> Option<(i64, i64)> {
    use crate::ir::IntWidth;

    match ty {
        Type::Int => Some((i64::MIN, i64::MAX)),
        Type::ExactInt { signed, width } => Some(match width {
            IntWidth::I8 => {
                if *signed {
                    (i8::MIN as i64, i8::MAX as i64)
                } else {
                    (0, u8::MAX as i64)
                }
            }
            IntWidth::I16 => {
                if *signed {
                    (i16::MIN as i64, i16::MAX as i64)
                } else {
                    (0, u16::MAX as i64)
                }
            }
            IntWidth::I32 => {
                if *signed {
                    (i32::MIN as i64, i32::MAX as i64)
                } else {
                    (0, u32::MAX as i64)
                }
            }
            IntWidth::I64 => {
                if *signed {
                    (i64::MIN, i64::MAX)
                } else {
                    // A u64 above `i64::MAX` has no unambiguous `i64` reading
                    // here, so only the non-negative half is folded.
                    (0, i64::MAX)
                }
            }
            IntWidth::Isize => {
                if *signed {
                    (i64::MIN, i64::MAX)
                } else {
                    (0, i64::MAX)
                }
            }
            IntWidth::Usize => (0, i64::MAX),
        }),
        // Characters are unsigned code points.
        Type::Char => Some((0, 0x0010_FFFF)),
        Type::Bool => Some((0, 1)),
        _ => None,
    }
}

/// True when `value` fits inside `ty`'s representable range.
fn value_fits_int_type(value: i64, ty: &Type) -> bool {
    match int_type_range(ty) {
        Some((min, max)) => (min..=max).contains(&value),
        None => false,
    }
}

/// Free helper — create and run the pass in a single call.
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
        InstructionKind::Div {
            result,
            lhs,
            rhs,
            unsigned: false,
        }
    }

    fn rem(result: Value, lhs: Value, rhs: Value) -> InstructionKind {
        InstructionKind::Rem {
            result,
            lhs,
            rhs,
            unsigned: false,
        }
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
