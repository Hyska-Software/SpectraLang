use super::*;

impl SemanticAnalyzer {
    pub(crate) fn direct_integer_literal(expr: &Expression) -> Option<&str> {
        match &expr.kind {
            ExpressionKind::NumberLiteral(raw)
                if !crate::numeric::number_literal_is_float(raw) =>
            {
                Some(raw)
            }
            ExpressionKind::Grouping(inner) => Self::direct_integer_literal(inner),
            _ => None,
        }
    }

    pub(crate) fn is_contextual_integer_literal_expression(expr: &Expression) -> bool {
        if Self::direct_integer_literal(expr).is_some() {
            return true;
        }
        matches!(
            &expr.kind,
            ExpressionKind::Unary {
                operator: crate::ast::UnaryOperator::Negate,
                operand,
            } if Self::direct_integer_literal(operand).is_some()
        )
    }

    fn integer_type_bounds(ty: &Type) -> Option<(bool, u32)> {
        match ty {
            Type::Int => Some((true, 64)),
            Type::ExactInt { signed, width } => Some((
                *signed,
                match width {
                    crate::ast::IntWidth::I8 => 8,
                    crate::ast::IntWidth::I16 => 16,
                    crate::ast::IntWidth::I32 => 32,
                    crate::ast::IntWidth::I64
                    | crate::ast::IntWidth::Isize
                    | crate::ast::IntWidth::Usize => 64,
                },
            )),
            _ => None,
        }
    }

    fn integer_value_fits(value: i128, signed: bool, bits: u32) -> bool {
        if signed {
            let min = -(1_i128 << (bits - 1));
            let max = (1_i128 << (bits - 1)) - 1;
            (min..=max).contains(&value)
        } else {
            value >= 0 && (value as u128) <= ((1_u128 << bits) - 1)
        }
    }

    pub(crate) fn contextual_integer_literal_type(&self, value: i128) -> Option<Type> {
        let expected = self.current_expected_type.as_ref();
        let (signed, bits) = expected
            .and_then(Self::integer_type_bounds)
            .unwrap_or((true, 64));
        if !Self::integer_value_fits(value, signed, bits) {
            return None;
        }
        match expected {
            Some(ty @ (Type::Int | Type::ExactInt { .. })) => Some(ty.clone()),
            _ => Some(Type::Int),
        }
    }

    pub(crate) fn analyze_expression_literals(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::Identifier(name) => {
                // Flow-sensitive use-after-free check (E034); no-op when the
                // identifier does not resolve to a visible binding.
                self.uaf_check_use(name, expr.span);
                if let Some(info) = self.lookup_symbol(name) {
                    let info = info.clone();
                    self.symbol_resolutions.insert(expr.span, info);
                } else if self.functions.contains_key(name) {
                    let info = SymbolInfo {
                        is_local: false,
                        def_span: None,
                        ty: Type::Unknown,
                    };
                    self.symbol_resolutions.insert(expr.span, info);
                } else {
                    if self.module_namespaces.contains(name.as_str()) {
                        // Valid module namespace identifier (e.g. "std" in std.string.len)
                        self.symbol_resolutions.insert(
                            expr.span,
                            SymbolInfo {
                                is_local: false,
                                def_span: None,
                                ty: Type::Unknown,
                            },
                        );
                    } else {
                        let hint = self.suggest_name(name);
                        if let Some(hint) = hint {
                            self.error_coded_with_hint(
                                "E001",
                                format!("Undefined variable or function '{}'", name),
                                expr.span,
                                hint,
                            );
                        } else {
                            self.error_coded(
                                "E001",
                                format!("Undefined variable or function '{}'", name),
                                expr.span,
                            );
                        }
                    }
                }
            }
            ExpressionKind::NumberLiteral(num) => {
                if !crate::numeric::number_literal_is_float(num) {
                    let parsed = crate::numeric::parse_number_literal_as_i128(num);
                    let value = parsed.and_then(|value| {
                        if self.current_negative_integer_literal {
                            value.checked_neg()
                        } else {
                            Some(value)
                        }
                    });
                    let valid = value
                        .and_then(|value| self.contextual_integer_literal_type(value))
                        .is_some();
                    if !valid {
                        let expected_integer = self
                            .current_expected_type
                            .as_ref()
                            .and_then(Self::integer_type_bounds)
                            .is_some();
                        if expected_integer {
                            let display_value = value
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| num.clone());
                            let target = self
                                .current_expected_type
                                .as_ref()
                                .map(type_name)
                                .unwrap_or_else(|| "integer type".to_string());
                            self.error_coded(
                                "E2903",
                                format!(
                                    "Integer literal `{}` does not fit in `{}`",
                                    display_value, target
                                ),
                                expr.span,
                            );
                        } else {
                            self.error_coded_with_hint(
                                "E048",
                                format!(
                                    "Integer literal `{}` is out of range for `int` (i64)",
                                    num
                                ),
                                expr.span,
                                "Use a value within -9223372036854775808..=9223372036854775807, or write the literal with a fractional part or exponent to make it a float.",
                            );
                        }
                    }
                }
                let ty = self.infer_expression_type(expr);
                self.symbol_resolutions.insert(
                    expr.span,
                    SymbolInfo {
                        is_local: false,
                        def_span: None,
                        ty,
                    },
                );
            }
            ExpressionKind::StringLiteral(_) | ExpressionKind::BoolLiteral(_) => {
                // Literals are always valid
                let ty = self.infer_expression_type(expr);
                self.symbol_resolutions.insert(
                    expr.span,
                    SymbolInfo {
                        is_local: false,
                        def_span: None,
                        ty,
                    },
                );
            }
            _ => unreachable!("expression category mismatch"),
        }
    }
}

#[cfg(test)]
mod integer_overflow_diagnostic_tests {
    use crate::{CompilationOptions, CompilationPipeline, CompilerError, SemanticError};

    fn semantic_errors(source: &str) -> Vec<SemanticError> {
        let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
        let errors = pipeline
            .compile(source, "int_overflow.spectra")
            .expect_err("the source must be rejected");
        errors
            .into_iter()
            .filter_map(|error| match error {
                CompilerError::Semantic(semantic) => Some(semantic),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn integer_literal_beyond_i64_max_reports_e048_at_the_literal() {
        let source = r#"
            module int_overflow

            public func main() returns int {
                let big = 9223372036854775808
                return 0
            }
        "#;
        let errors = semantic_errors(source);
        let error = errors
            .iter()
            .find(|error| error.code.as_deref() == Some("E048"))
            .unwrap_or_else(|| panic!("expected coded E048: {errors:?}"));
        assert!(
            error.message.contains("9223372036854775808"),
            "the diagnostic must quote the literal: {error:?}"
        );
    }

    #[test]
    fn integer_literal_at_i64_max_still_compiles() {
        let source = r#"
            module int_boundary

            public func main() returns int {
                let max = 9223372036854775807
                return 0
            }
        "#;
        let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
        pipeline
            .compile(source, "int_boundary.spectra")
            .expect("i64::MAX must keep compiling");
    }
}
