use super::*;

impl ASTLowering {
    pub(crate) fn lower_expression_binary(
        &mut self,
        expr: &Expression,
        ir_func: &mut IRFunction,
    ) -> Value {
        match &expr.kind {
            ExpressionKind::Binary {
                left,
                operator,
                right,
            } => {
                let left_ir_type = self.infer_expr_ir_type(left);
                let right_ir_type = self.infer_expr_ir_type(right);

                // Operator overloading: if the left operand is a struct, dispatch to the
                // correspondingly named method (e.g. `Point_add(lhs, rhs)`).
                if let IRType::Struct { name: ref sn, .. } = left_ir_type {
                    if let Some(method_name) = operator_overload_method(operator) {
                        let lhs = self.lower_expression(left, ir_func);
                        let rhs = self.lower_expression(right, ir_func);
                        let fn_name = format!("{}_{}", sn, method_name);
                        return self.require_value(
                            self.builder
                                .build_call(ir_func, fn_name, vec![lhs, rhs], true),
                            "operator overload did not produce its declared result",
                        );
                    }
                }

                if matches!(operator, BinaryOperator::Add)
                    && (matches!(left_ir_type, IRType::String)
                        || matches!(right_ir_type, IRType::String))
                {
                    let lhs = self.lower_expression(left, ir_func);
                    let rhs = self.lower_expression(right, ir_func);
                    let lhs = self.lower_value_to_string(lhs, left_ir_type, ir_func);
                    let rhs = self.lower_value_to_string(rhs, right_ir_type, ir_func);
                    return self.require_value(
                        self.builder.build_typed_host_call(
                            ir_func,
                            "spectra.std.string.concat".to_string(),
                            vec![lhs, rhs],
                            IRType::String,
                            true,
                        ),
                        "string concatenation host call did not produce its declared result",
                    );
                }

                // Logical operators short-circuit: the right-hand side is only
                // evaluated on the path that needs it (`and` iff lhs is true,
                // `or` iff lhs is false). Both operands are statically boolean
                // (enforced by semantic analysis). Previously both sides were
                // lowered unconditionally and combined with one eager And/Or
                // instruction, so guards like `i >= 0 and a[i] == 0` evaluated
                // the out-of-bounds access.
                match operator {
                    BinaryOperator::And => {
                        return self.lower_short_circuit_boolean(true, left, right, ir_func);
                    }
                    BinaryOperator::Or => {
                        return self.lower_short_circuit_boolean(false, left, right, ir_func);
                    }
                    _ => {}
                }

                let mut lhs = self.lower_expression(left, ir_func);
                let mut rhs = self.lower_expression(right, ir_func);
                let (new_lhs, new_rhs, new_left_ty, new_right_ty) = self
                    .widen_mixed_int_float_operands(lhs, rhs, left_ir_type, right_ir_type, ir_func);
                lhs = new_lhs;
                rhs = new_rhs;
                let left_ir_type = new_left_ty;
                let right_ir_type = new_right_ty;

                if let IRType::ExactInt { signed, width } = &left_ir_type {
                    if left_ir_type == right_ir_type {
                        let op_name = match operator {
                            BinaryOperator::Add => Some("add"),
                            BinaryOperator::Subtract => Some("sub"),
                            BinaryOperator::Multiply => Some("mul"),
                            _ => None,
                        };
                        if let Some(op_name) = op_name {
                            let bits = match width {
                                IRIntWidth::I8 => 8,
                                IRIntWidth::I16 => 16,
                                IRIntWidth::I32 => 32,
                                IRIntWidth::I64 | IRIntWidth::Isize | IRIntWidth::Usize => 64,
                            };
                            let lhs_slot = self.builder.build_cast(
                                ir_func,
                                lhs,
                                left_ir_type.clone(),
                                IRType::Int,
                            );
                            let rhs_slot = self.builder.build_cast(
                                ir_func,
                                rhs,
                                right_ir_type.clone(),
                                IRType::Int,
                            );
                            let host = format!(
                                "spectra.std.numeric.checked_{op_name}_{}{}",
                                if *signed { "i" } else { "u" },
                                bits
                            );
                            if let Some(value) = self.builder.build_typed_host_call(
                                ir_func,
                                host,
                                vec![lhs_slot, rhs_slot],
                                left_ir_type.clone(),
                                true,
                            ) {
                                return value;
                            }
                        }
                    }
                }

                match operator {
                    BinaryOperator::Add => self.builder.build_add(ir_func, lhs, rhs),
                    BinaryOperator::Subtract => self.builder.build_sub(ir_func, lhs, rhs),
                    BinaryOperator::Multiply => self.builder.build_mul(ir_func, lhs, rhs),
                    BinaryOperator::Divide => self.builder.build_div(ir_func, lhs, rhs),
                    BinaryOperator::Modulo => self.builder.build_rem(ir_func, lhs, rhs),
                    BinaryOperator::Equal => self.lower_value_equality(
                        lhs,
                        rhs,
                        &left_ir_type,
                        &right_ir_type,
                        false,
                        ir_func,
                    ),
                    BinaryOperator::NotEqual => self.lower_value_equality(
                        lhs,
                        rhs,
                        &left_ir_type,
                        &right_ir_type,
                        true,
                        ir_func,
                    ),
                    BinaryOperator::Less => self.builder.build_lt(ir_func, lhs, rhs),
                    BinaryOperator::LessEqual => self.builder.build_le(ir_func, lhs, rhs),
                    BinaryOperator::Greater => self.builder.build_gt(ir_func, lhs, rhs),
                    BinaryOperator::GreaterEqual => self.builder.build_ge(ir_func, lhs, rhs),
                    BinaryOperator::And => self.builder.build_and(ir_func, lhs, rhs),
                    BinaryOperator::Or => self.builder.build_or(ir_func, lhs, rhs),
                }
            }
            ExpressionKind::Unary { operator, operand } => {                use spectra_compiler::ast::UnaryOperator;

                // Operator overloading: if operand is a struct, dispatch `StructName_neg`.
                if matches!(operator, UnaryOperator::Negate) {
                    let op_ir_type = self.infer_expr_ir_type(operand);
                    if let IRType::Struct { name: ref sn, .. } = op_ir_type {
                        let val = self.lower_expression(operand, ir_func);
                        let fn_name = format!("{}_neg", sn);
                        return self.require_value(
                            self.builder.build_call(ir_func, fn_name, vec![val], true),
                            "unary operator overload did not produce its declared result",
                        );
                    }
                }

                let operand_value = self.lower_expression(operand, ir_func);

                match operator {
                    UnaryOperator::Negate => {
                        // Negate: 0 - operand, preserving numeric kind.
                        let zero = match self.infer_expr_ir_type(operand) {
                            IRType::Float => self.builder.build_const_float(ir_func, 0.0),
                            _ => self.builder.build_const_int(ir_func, 0),
                        };
                        self.builder.build_sub(ir_func, zero, operand_value)
                    }
                    UnaryOperator::Not => self.builder.build_not(ir_func, operand_value),
                }
            }
            _ => unreachable!("lowering expression category mismatch"),
        }
    }

    /// Implicit int→float widening for mixed arithmetic and comparisons
    /// (semantic `can_auto_promote`: the float side wins, matching
    /// `numeric_result_type`). Emits a typed `Cast` on the int side so
    /// signedness survives (a backend-only fixup cannot tell `i8` from
    /// `u8`). Without this the backend receives mismatched operand types
    /// (e.g. `fmul.i64`) and fails verification, or panics on promoted
    /// scalar locals. Returns the (possibly new) operands and their types.
    pub(crate) fn widen_mixed_int_float_operands(
        &mut self,
        lhs: Value,
        rhs: Value,
        left_ty: IRType,
        right_ty: IRType,
        ir_func: &mut IRFunction,
    ) -> (Value, Value, IRType, IRType) {
        fn is_int_family(ty: &IRType) -> bool {
            matches!(ty, IRType::Int | IRType::ExactInt { .. })
        }
        fn is_float_family(ty: &IRType) -> bool {
            matches!(ty, IRType::Float | IRType::ExactFloat { .. })
        }
        if is_int_family(&left_ty) && is_float_family(&right_ty) {
            let lhs = self
                .builder
                .build_cast(ir_func, lhs, left_ty, right_ty.clone());
            (lhs, rhs, right_ty.clone(), right_ty)
        } else if is_float_family(&left_ty) && is_int_family(&right_ty) {
            let rhs = self
                .builder
                .build_cast(ir_func, rhs, right_ty, left_ty.clone());
            (lhs, rhs, left_ty.clone(), left_ty)
        } else {
            (lhs, rhs, left_ty, right_ty)
        }
    }

    /// Lower `and`/`&&` (`is_and`) or `or`/`||` with short-circuit control
    /// flow, mirroring the If lowering: the right-hand side lives in its own
    /// block reachable only from the path that needs its value, and a merge
    /// phi joins it with the short-circuit constant. Both sides are boolean,
    /// so the phi is bool/bool and needs no coercion.
    pub(crate) fn lower_short_circuit_boolean(
        &mut self,
        is_and: bool,
        left: &Expression,
        right: &Expression,
        ir_func: &mut IRFunction,
    ) -> Value {
        let lhs = self.lower_expression(left, ir_func);
        let (rhs_name, short_name, merge_name) = if is_and {
            ("and.rhs", "and.short", "and.merge")
        } else {
            ("or.rhs", "or.short", "or.merge")
        };
        let rhs_bb = ir_func.add_block(rhs_name);
        let short_bb = ir_func.add_block(short_name);
        let merge_bb = ir_func.add_block(merge_name);
        if is_and {
            self.builder
                .build_cond_branch(ir_func, lhs, rhs_bb, short_bb);
        } else {
            self.builder
                .build_cond_branch(ir_func, lhs, short_bb, rhs_bb);
        }
        // Short path: constant result, right-hand side never evaluated.
        self.builder.set_current_block(short_bb);
        let short_val = self.builder.build_const_bool(ir_func, !is_and);
        self.builder.build_branch(ir_func, merge_bb);
        // Evaluation path.
        self.builder.set_current_block(rhs_bb);
        let rhs_val = self.lower_expression(right, ir_func);
        let rhs_final = self.builder.get_current_block().unwrap_or(rhs_bb);
        let rhs_terminated = ir_func
            .get_block(rhs_final)
            .map(|block| block.terminator.is_some())
            .unwrap_or(false);
        if !rhs_terminated {
            self.builder.build_branch(ir_func, merge_bb);
        }
        // Merge.
        self.builder.set_current_block(merge_bb);
        if rhs_terminated {
            // The rhs diverges: merge is only reachable via the short arm.
            short_val
        } else {
            self.builder
                .build_phi(ir_func, vec![(rhs_val, rhs_final), (short_val, short_bb)])
        }
    }
}
