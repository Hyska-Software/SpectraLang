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

                // Operator overloading: if the left side is a struct that implements the
                // corresponding operator trait, skip the numeric/string checks — but
                // keep validating the RIGHT operand against the operator method's
                // second parameter (tolerating `unknown`) so a wrong RHS still fails.
                if matches!(&left_type, Type::Struct { .. } | Type::Applied { .. }) {
                    if let Some((trait_name, method_name)) = operator_trait_and_method(*operator) {
                        if let Some(sn) = self.nominal_lookup_name(&left_type) {
                            if self
                                .trait_impls
                                .contains_key(&(trait_name.to_string(), sn.clone()))
                            {
                                self.validate_overloaded_operand(
                                    right,
                                    &right_type,
                                    &sn,
                                    method_name,
                                    *operator,
                                );
                                // Overloaded — no further builtin checks needed.
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
                                self.error_coded(
                                    "E037",
                                    format!(
                                        "Cannot concatenate non-string type {} with string",
                                        type_name(&left_type)
                                    ),
                                    left.span,
                                );
                            }
                            if !matches!(right_type, Type::String | Type::Unknown) {
                                self.error_coded(
                                    "E037",
                                    format!(
                                        "Cannot concatenate string with non-string type {}",
                                        type_name(&right_type)
                                    ),
                                    right.span,
                                );
                            }
                        } else {
                            // Numeric addition
                            if !Self::is_numeric_type(&left_type)
                                && !matches!(left_type, Type::Unknown)
                            {
                                self.error_coded(
                                    "E036",
                                    format!("Left operand of arithmetic operation must be numeric, found {}", type_name(&left_type)),
                                    left.span,
                                );
                            }
                            if !Self::is_numeric_type(&right_type)
                                && !matches!(right_type, Type::Unknown)
                            {
                                self.error_coded(
                                    "E036",
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
                            self.error_coded(
                                "E036",
                                format!("Left operand of arithmetic operation must be numeric, found {}", type_name(&left_type)),
                                left.span,
                            );
                        }
                        if !Self::is_numeric_type(&right_type)
                            && !matches!(right_type, Type::Unknown)
                        {
                            self.error_coded(
                                "E036",
                                format!("Right operand of arithmetic operation must be numeric, found {}", type_name(&right_type)),
                                right.span,
                            );
                        }
                        if !self.numeric_types_can_interact(&left_type, &right_type) {
                            self.error_coded(
                                "E038",
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
                            self.error_coded(
                                "E038",
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
                            self.error_coded(
                                "E039",
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
                            self.error_coded(
                                "E039",
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
                            self.error_coded(
                                "E040",
                                format!(
                                    "Left operand of logical operation must be boolean, found {}",
                                    type_name(&left_type)
                                ),
                                left.span,
                            );
                        }
                        if !matches!(right_type, Type::Bool | Type::Unknown) {
                            self.error_coded(
                                "E040",
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
            ExpressionKind::Unary { operator, operand } => {
                let saved_negative_literal = self.current_negative_integer_literal;
                self.current_negative_integer_literal = matches!(
                    operator,
                    crate::ast::UnaryOperator::Negate
                ) && Self::direct_integer_literal(operand).is_some();
                self.analyze_expression(operand);
                self.current_negative_integer_literal = saved_negative_literal;
                // Unary operand type checks: `-` requires a numeric operand,
                // `!` a boolean one (`unknown` operands are tolerated because
                // they already carry their own diagnostic).
                let operand_type = self.infer_expression_type(operand);
                match operator {
                    crate::ast::UnaryOperator::Negate => {
                        if !Self::is_numeric_type(&operand_type)
                            && !matches!(operand_type, Type::Unknown)
                        {
                            self.error_coded(
                                "E036",
                                format!(
                                    "Unary `-` requires a numeric operand, found {}",
                                    type_name(&operand_type)
                                ),
                                operand.span,
                            );
                        }
                    }
                    crate::ast::UnaryOperator::Not => {
                        if !matches!(operand_type, Type::Bool | Type::Unknown) {
                            self.error_coded(
                                "E040",
                                format!(
                                    "Unary `!` requires a boolean operand, found {}",
                                    type_name(&operand_type)
                                ),
                                operand.span,
                            );
                        }
                    }
                }
            }
            _ => unreachable!("expression category mismatch"),
        }
    }

    /// Validates the right operand of an overloaded binary operator against
    /// the registered operator method's second parameter (`other`). Missing
    /// signatures (forward references analyzed before the impl block ran) and
    /// `unknown` types are tolerated; a concrete mismatch is reported with the
    /// stable `E038` operand-mismatch code.
    fn validate_overloaded_operand(
        &mut self,
        right: &Expression,
        right_type: &Type,
        receiver_name: &str,
        method_name: &str,
        operator: crate::ast::BinaryOperator,
    ) {
        let signature = self
            .methods
            .get(receiver_name)
            .and_then(|methods| methods.get(method_name))
            .cloned()
            .or_else(|| self.instantiated_method_signature(receiver_name, method_name));
        let Some(expected) = signature.and_then(|signature| signature.params.get(1).cloned())
        else {
            // Signature not registered yet (impl block analyzed later in the
            // item pass); the impl itself is still validated when analyzed.
            return;
        };

        if matches!(right_type, Type::Unknown) || matches!(expected, Type::Unknown) {
            return;
        }
        if right_type == &expected || self.types_match(right_type, &expected) {
            return;
        }

        self.error_coded(
            "E038",
            format!(
                "Right operand of operator `{}` has type {}, but `{}` expects {}",
                Self::binary_operator_symbol(operator),
                type_name(right_type),
                method_name,
                type_name(&expected)
            ),
            right.span,
        );
    }

    fn binary_operator_symbol(operator: crate::ast::BinaryOperator) -> &'static str {
        use crate::ast::BinaryOperator;
        match operator {
            BinaryOperator::Add => "+",
            BinaryOperator::Subtract => "-",
            BinaryOperator::Multiply => "*",
            BinaryOperator::Divide => "/",
            BinaryOperator::Modulo => "%",
            BinaryOperator::Equal => "==",
            BinaryOperator::NotEqual => "!=",
            BinaryOperator::Less => "<",
            BinaryOperator::Greater => ">",
            BinaryOperator::LessEqual => "<=",
            BinaryOperator::GreaterEqual => ">=",
            BinaryOperator::And => "&&",
            BinaryOperator::Or => "||",
        }
    }
}

#[cfg(test)]
mod operand_and_unary_code_tests {
    use crate::{CompilationOptions, CompilationPipeline, CompilerError, SemanticError};

    fn semantic_errors(source: &str) -> Vec<SemanticError> {
        let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
        let errors = pipeline
            .compile(source, "operand_codes.spectra")
            .expect_err("the source must be rejected");
        errors
            .into_iter()
            .filter_map(|error| match error {
                CompilerError::Semantic(semantic) => Some(semantic),
                _ => None,
            })
            .collect()
    }

    fn has_code(errors: &[SemanticError], code: &str) -> bool {
        errors
            .iter()
            .any(|error| error.code.as_deref() == Some(code))
    }

    #[test]
    fn unary_operand_mismatches_carry_stable_codes() {
        let source = r#"
            module unary_codes

            public func main() returns int {
                let a = !"hello"
                let b = !5
                let c = -true
                let d = -"text"
                return 0
            }
        "#;
        let errors = semantic_errors(source);
        assert!(
            has_code(&errors, "E040"),
            "unary `!` on non-bool must report E040: {errors:?}"
        );
        assert!(
            has_code(&errors, "E036"),
            "unary `-` on non-numeric must report E036: {errors:?}"
        );
    }

    #[test]
    fn logical_operand_mismatch_carries_e040() {
        let source = r#"
            module logical_codes

            public func main() returns int {
                let flag = 1 and true
                if flag {
                    return 1
                }
                return 0
            }
        "#;
        let errors = semantic_errors(source);
        assert!(
            has_code(&errors, "E040"),
            "non-bool logical operand must report E040: {errors:?}"
        );
    }

    #[test]
    fn builtin_operator_impl_without_mapped_method_reports_e016() {
        let source = r#"
            module bad_operator_impl

            record Counter {
                public value: int,
            }

            impl Add for Counter {
                func plus(&self, other: Counter) returns Counter {
                    Counter { value: self.value + other.value }
                }
            }

            public func main() returns int {
                return 0
            }
        "#;
        let errors = semantic_errors(source);
        assert!(
            has_code(&errors, "E016"),
            "`impl Add` without an `add` method must report E016: {errors:?}"
        );
    }

    #[test]
    fn builtin_operator_impl_with_wrong_signature_reports_e023() {
        let source = r#"
            module bad_operator_signature

            record Counter {
                public value: int,
            }

            impl Add for Counter {
                func add(&self, other: string) returns int {
                    0
                }
            }

            public func main() returns int {
                return 0
            }
        "#;
        let errors = semantic_errors(source);
        assert!(
            has_code(&errors, "E023"),
            "`add` with wrong operand/return types must report E023: {errors:?}"
        );
    }

    #[test]
    fn overloaded_operator_still_validates_the_right_operand() {
        let source = r#"
            module overloaded_rhs

            record Counter {
                public value: int,
            }

            impl Add for Counter {
                func add(&self, other: Counter) returns Counter {
                    Counter { value: self.value + other.value }
                }
            }

            public func main() returns int {
                let base = Counter { value: 1 }
                let broken = base + 5
                return 0
            }
        "#;
        let errors = semantic_errors(source);
        assert!(
            has_code(&errors, "E038"),
            "right operand of an overloaded operator is validated against `other`: {errors:?}"
        );
    }

    #[test]
    fn valid_builtin_operator_impl_still_compiles() {
        let source = r#"
            module good_operator_impl

            record Counter {
                public value: int,
            }

            impl Add for Counter {
                func add(&self, other: Counter) returns Counter {
                    Counter { value: self.value + other.value }
                }
            }

            impl Eq for Counter {
                func eq(&self, other: Counter) returns bool {
                    self.value == other.value
                }
            }

            public func main() returns int {
                let a = Counter { value: 1 }
                let b = Counter { value: 1 }
                let sum = a + b
                let same = a == b
                if not same {
                    return 1
                }
                return sum.value - 2
            }
        "#;
        let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
        pipeline
            .compile(source, "good_operator_impl.spectra")
            .expect("a correct builtin operator impl must keep compiling");
    }
}
