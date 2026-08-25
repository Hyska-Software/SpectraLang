use super::*;

impl SemanticAnalyzer {
    /// Verifica se um match expression é exhaustivo
    pub(crate) fn check_match_exhaustiveness(
        &mut self,
        scrutinee_type: &Type,
        arms: &[crate::ast::MatchArm],
        span: Span,
    ) {
        use crate::ast::{ExpressionKind, Pattern};

        fn pattern_is_catch_all(pattern: &Pattern) -> bool {
            match pattern {
                Pattern::Wildcard | Pattern::Identifier(_) => true,
                Pattern::Or(patterns) => patterns.iter().any(pattern_is_catch_all),
                _ => false,
            }
        }

        fn pattern_contains_bool_literal(pattern: &Pattern, expected: bool) -> bool {
            match pattern {
                Pattern::Literal(expr) => {
                    matches!(expr.kind, ExpressionKind::BoolLiteral(value) if value == expected)
                }
                Pattern::Or(patterns) => patterns
                    .iter()
                    .any(|pattern| pattern_contains_bool_literal(pattern, expected)),
                _ => false,
            }
        }

        fn visit_enum_patterns<'a>(pattern: &'a Pattern, out: &mut Vec<&'a Pattern>) {
            match pattern {
                Pattern::Or(patterns) => {
                    for pattern in patterns {
                        visit_enum_patterns(pattern, out);
                    }
                }
                Pattern::EnumVariant { .. } => out.push(pattern),
                _ => {}
            }
        }

        // Se tem wildcard ou identifier, é automaticamente exhaustivo
        let has_catch_all = arms.iter().any(|arm| pattern_is_catch_all(&arm.pattern));

        if has_catch_all {
            return; // Exhaustivo
        }

        match scrutinee_type {
            Type::Enum { .. } | Type::Applied { .. } => {
                let Some((base_enum_name, enum_info, _)) =
                    self.specialized_enum_context_for_type(scrutinee_type)
                else {
                    return;
                };

                let mut covered_variants: HashSet<String> = HashSet::new();
                let mut payload_coverage: HashMap<String, bool> = enum_info
                    .variants
                    .iter()
                    .filter(|(_, info)| info.data.as_ref().map(|d| !d.is_empty()).unwrap_or(false))
                    .map(|(variant, _)| (variant.clone(), false))
                    .collect();

                for arm in arms {
                    let mut enum_patterns = Vec::new();
                    visit_enum_patterns(&arm.pattern, &mut enum_patterns);

                    for pattern in enum_patterns {
                        if let Pattern::EnumVariant {
                            enum_name,
                            variant_name,
                            data,
                            ..
                        } = pattern
                        {
                            if enum_name != &base_enum_name {
                                continue;
                            }

                            covered_variants.insert(variant_name.clone());

                            if let Some(flag) = payload_coverage.get_mut(variant_name) {
                                let expected_len = enum_info
                                    .variants
                                    .get(variant_name)
                                    .and_then(|info| info.data.as_ref())
                                    .map(|data| data.len())
                                    .unwrap_or(0);

                                if let Some(payload_patterns) = data {
                                    if payload_patterns.len() == expected_len
                                        && payload_patterns.iter().all(|p| {
                                            matches!(p, Pattern::Wildcard | Pattern::Identifier(_))
                                        })
                                    {
                                        *flag = true;
                                    }
                                }
                            }
                        }
                    }
                }

                let missing_variants: Vec<String> = enum_info
                    .variants
                    .keys()
                    .filter(|variant| !covered_variants.contains(*variant))
                    .cloned()
                    .collect();

                if !missing_variants.is_empty() {
                    let missing_str = missing_variants
                        .into_iter()
                        .map(|variant| format!("{}::{}", base_enum_name, variant))
                        .collect::<Vec<_>>()
                        .join(", ");

                    self.error_coded(
                        "E031",
                        format!(
                            "Match expression is not exhaustive. Missing patterns: {}",
                            missing_str
                        ),
                        span,
                    );
                    return;
                }

                let missing_payload_guard: Vec<String> = payload_coverage
                    .into_iter()
                    .filter_map(|(variant, covered)| {
                        if covered {
                            None
                        } else {
                            Some(format!("{}::{}", base_enum_name, variant))
                        }
                    })
                    .collect();

                if !missing_payload_guard.is_empty() {
                    let list = missing_payload_guard.join(", ");
                    self.error_coded(
                        "E031",
                        format!(
                            "Match on enum '{}' must include wildcard bindings for payload of variant(s): {}",
                            base_enum_name, list
                        ),
                        span,
                    );
                }
            }
            Type::Bool => {
                let has_true = arms
                    .iter()
                    .any(|arm| pattern_contains_bool_literal(&arm.pattern, true));
                let has_false = arms
                    .iter()
                    .any(|arm| pattern_contains_bool_literal(&arm.pattern, false));

                if !(has_true && has_false) {
                    self.error_coded(
                        "E031",
                        "Match on 'bool' is not exhaustive. Consider adding 'true', 'false', or a wildcard pattern (_).",
                        span,
                    );
                }
            }
            Type::Tuple { elements } => {
                if elements.is_empty() {
                    return;
                }

                let mut bool_combinations: HashSet<Vec<bool>> = HashSet::new();
                let mut unsupported_pattern = false;

                for arm in arms {
                    if let Pattern::Literal(expr) = &arm.pattern {
                        if let ExpressionKind::TupleLiteral {
                            elements: tuple_elems,
                        } = &expr.kind
                        {
                            if tuple_elems.len() != elements.len() {
                                unsupported_pattern = true;
                                break;
                            }

                            let mut combo = Vec::with_capacity(elements.len());
                            let mut tuple_supported = true;

                            for (tuple_ty, tuple_expr) in elements.iter().zip(tuple_elems.iter()) {
                                match (tuple_ty, &tuple_expr.kind) {
                                    (Type::Bool, ExpressionKind::BoolLiteral(value)) => {
                                        combo.push(*value);
                                    }
                                    _ => {
                                        tuple_supported = false;
                                        break;
                                    }
                                }
                            }

                            if tuple_supported {
                                bool_combinations.insert(combo);
                            } else {
                                unsupported_pattern = true;
                                break;
                            }
                        } else {
                            unsupported_pattern = true;
                            break;
                        }
                    } else {
                        unsupported_pattern = true;
                        break;
                    }
                }

                if unsupported_pattern {
                    self.error_coded(
                        "E031",
                        "Match on tuple requires a wildcard (_) pattern to cover remaining combinations.",
                        span,
                    );
                }

                let expected = 1 << elements.len();
                if bool_combinations.len() != expected {
                    self.error_coded(
                        "E031",
                        format!(
                            "Match on tuple of bools is not exhaustive. Expected {} combination(s).",
                            expected
                        ),
                        span,
                    );
                }
            }
            _ => {
                let only_literals = arms
                    .iter()
                    .all(|arm| matches!(arm.pattern, Pattern::Literal(_)));

                if only_literals {
                    self.error_coded(
                        "E031",
                        "Match expression with only literal patterns is not exhaustive. Consider adding a wildcard pattern (_).",
                        span,
                    );
                }
            }
        }
    }

}
