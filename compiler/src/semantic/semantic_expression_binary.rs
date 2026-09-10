use super::*;

impl SemanticAnalyzer {
    pub(crate) fn analyze_expression_binary(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::Binary {
                left,
                operator,
                right,
            } => {
                self.analyze_expression(left);
                self.analyze_expression(right);

                // Type check binary operations
                let left_type = self.infer_expression_type(left);
                let right_type = self.infer_expression_type(right);

                // Operator overloading: if left side is a struct that implements the
                // corresponding operator trait, skip all type-error checks.
                if matches!(&left_type, Type::Struct { .. } | Type::Applied { .. }) {
                    if let Some((trait_name, _method_name)) = operator_trait_and_method(*operator) {
                        if let Some(sn) = self.nominal_lookup_name(&left_type) {
                            if self.trait_impls.contains_key(&(trait_name.to_string(), sn)) {
                                // Overloaded — no further checks needed.
                                // (The method signature is validated when analyze_impl_block runs.)
                                return; // from analyze_expression
                            }
                        }
                    }
                }

                use crate::ast::BinaryOperator;
                match operator {
                    BinaryOperator::Add => {
                        // Add supports both numeric types and string concatenation
                        let is_string_concat =
                            matches!(left_type, Type::String) || matches!(right_type, Type::String);

                        if is_string_concat {
                            // String concatenation - both operands must be strings
                            if !matches!(left_type, Type::String | Type::Unknown) {
                                self.error(
                                    format!(
                                        "Cannot concatenate non-string type {:?} with string",
                                        left_type
                                    ),
                                    left.span,
                                );
                            }
                            if !matches!(right_type, Type::String | Type::Unknown) {
                                self.error(
                                    format!(
                                        "Cannot concatenate string with non-string type {:?}",
                                        right_type
                                    ),
                                    right.span,
                                );
                            }
                        } else {
                            // Numeric addition
                            if !Self::is_numeric_type(&left_type)
                                && !matches!(left_type, Type::Unknown)
                            {
                                self.error(
                                    format!("Left operand of arithmetic operation must be numeric, found {}", type_name(&left_type)),
                                    left.span,
                                );
                            }
                            if !Self::is_numeric_type(&right_type)
                                && !matches!(right_type, Type::Unknown)
                            {
                                self.error(
                                    format!("Right operand of arithmetic operation must be numeric, found {}", type_name(&right_type)),
                                    right.span,
                                );
                            }
                        }
                    }
                    BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
                    | BinaryOperator::Modulo => {
                        // Arithmetic operations require numeric types
                        if !Self::is_numeric_type(&left_type) && !matches!(left_type, Type::Unknown)
                        {
                            self.error(
                                format!("Left operand of arithmetic operation must be numeric, found {}", type_name(&left_type)),
                                left.span,
                            );
                        }
                        if !Self::is_numeric_type(&right_type)
                            && !matches!(right_type, Type::Unknown)
                        {
                            self.error(
                                format!("Right operand of arithmetic operation must be numeric, found {}", type_name(&right_type)),
                                right.span,
                            );
                        }
                        if !self.numeric_types_can_interact(&left_type, &right_type) {
                            self.error(
                                format!(
                                    "Type mismatch in arithmetic operation: {} and {}",
                                    type_name(&left_type),
                                    type_name(&right_type)
                                ),
                                expr.span,
                            );
                        }
                    }
                    BinaryOperator::Equal | BinaryOperator::NotEqual => {
                        // Equality can compare any types, but they should match
                        if left_type != Type::Unknown
                            && right_type != Type::Unknown
                            && left_type != right_type
                            && !self.numeric_types_can_interact(&left_type, &right_type)
                        {
                            self.error(
                                format!(
                                    "Type mismatch in equality comparison: {} and {}",
                                    type_name(&left_type),
                                    type_name(&right_type)
                                ),
                                expr.span,
                            );
                        }
                    }
                    BinaryOperator::Less
                    | BinaryOperator::Greater
                    | BinaryOperator::LessEqual
                    | BinaryOperator::GreaterEqual => {
                        // Comparison requires numeric types
                        if !Self::is_numeric_type(&left_type) && !matches!(left_type, Type::Unknown)
                        {
                            self.error(
                                format!(
                                    "Left operand of comparison must be numeric, found {}",
                                    type_name(&left_type)
                                ),
                                left.span,
                            );
                        }
                        if !Self::is_numeric_type(&right_type)
                            && !matches!(right_type, Type::Unknown)
                        {
                            self.error(
                                format!(
                                    "Right operand of comparison must be numeric, found {}",
                                    type_name(&right_type)
                                ),
                                right.span,
                            );
                        }
                    }
                    BinaryOperator::And | BinaryOperator::Or => {
                        // Logical operations require boolean types
                        if !matches!(left_type, Type::Bool | Type::Unknown) {
                            self.error(
                                format!(
                                    "Left operand of logical operation must be boolean, found {}",
                                    type_name(&left_type)
                                ),
                                left.span,
                            );
                        }
                        if !matches!(right_type, Type::Bool | Type::Unknown) {
                            self.error(
                                format!(
                                    "Right operand of logical operation must be boolean, found {}",
                                    type_name(&right_type)
                                ),
                                right.span,
                            );
                        }
                    }
                }
            }
            ExpressionKind::Unary { operand, .. } => {
                self.analyze_expression(operand);
            }
            _ => unreachable!("expression category mismatch"),
        }
    }
}
