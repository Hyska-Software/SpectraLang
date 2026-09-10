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

                let lhs = self.lower_expression(left, ir_func);
                let rhs = self.lower_expression(right, ir_func);

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
            ExpressionKind::Unary { operator, operand } => {
                use spectra_compiler::ast::UnaryOperator;

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
}
