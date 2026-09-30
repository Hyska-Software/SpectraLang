use super::*;

fn bool_tuple_dimensions(ty: &Type) -> Option<usize> {
    match ty {
        Type::Bool => Some(1),
        Type::Tuple { elements } => elements.iter().try_fold(0usize, |count, element| {
            count.checked_add(bool_tuple_dimensions(element)?)
        }),
        _ => None,
    }
}

fn bool_tuple_expression_cubes(
    expression: &crate::ast::Expression,
    ty: &Type,
) -> Option<Vec<Vec<Option<bool>>>> {
    use crate::ast::ExpressionKind;

    match (ty, &expression.kind) {
        (Type::Bool, ExpressionKind::BoolLiteral(value)) => Some(vec![vec![Some(*value)]]),
        (Type::Tuple { elements }, ExpressionKind::TupleLiteral { elements: values })
            if elements.len() == values.len() =>
        {
            let mut cubes = vec![Vec::new()];
            for (element_type, value) in elements.iter().zip(values) {
                let value_cubes = bool_tuple_expression_cubes(value, element_type)?;
                let mut combined = Vec::with_capacity(cubes.len() * value_cubes.len());
                for prefix in &cubes {
                    for suffix in &value_cubes {
                        let mut cube = prefix.clone();
                        cube.extend(suffix.iter().copied());
                        combined.push(cube);
                    }
                }
                cubes = combined;
            }
            Some(cubes)
        }
        (_, ExpressionKind::Grouping(inner)) => bool_tuple_expression_cubes(inner, ty),
        _ => None,
    }
}

fn bool_tuple_pattern_cubes(
    pattern: &crate::ast::Pattern,
    ty: &Type,
) -> Option<Vec<Vec<Option<bool>>>> {
    use crate::ast::Pattern;

    match pattern {
        Pattern::Wildcard(_) | Pattern::Identifier(_, _) => {
            Some(vec![vec![None; bool_tuple_dimensions(ty)?]])
        }
        Pattern::Literal(expression) => bool_tuple_expression_cubes(expression, ty),
        Pattern::Tuple(patterns) => {
            let Type::Tuple { elements } = ty else {
                return None;
            };
            if patterns.len() != elements.len() {
                return None;
            }

            let mut cubes = vec![Vec::new()];
            for (element_pattern, element_type) in patterns.iter().zip(elements) {
                let element_cubes = bool_tuple_pattern_cubes(element_pattern, element_type)?;
                let mut combined = Vec::with_capacity(cubes.len() * element_cubes.len());
                for prefix in &cubes {
                    for suffix in &element_cubes {
                        let mut cube = prefix.clone();
                        cube.extend(suffix.iter().copied());
                        combined.push(cube);
                    }
                }
                cubes = combined;
            }
            Some(cubes)
        }
        Pattern::Or(patterns) => {
            let mut cubes = Vec::new();
            for alternative in patterns {
                cubes.extend(bool_tuple_pattern_cubes(alternative, ty)?);
            }
            Some(cubes)
        }
        _ => None,
    }
}

fn bool_tuple_cubes_cover_all(cubes: &[Vec<Option<bool>>], dimension: usize) -> bool {
    if cubes.is_empty() {
        return false;
    }
    if cubes
        .iter()
        .any(|cube| cube[dimension..].iter().all(Option::is_none))
    {
        return true;
    }
    if dimension == cubes[0].len() {
        return true;
    }
    if cubes.iter().all(|cube| cube[dimension].is_none()) {
        return bool_tuple_cubes_cover_all(cubes, dimension + 1);
    }

    [false, true].into_iter().all(|value| {
        let matching = cubes
            .iter()
            .filter(|cube| cube[dimension].is_none_or(|constraint| constraint == value))
            .cloned()
            .collect::<Vec<_>>();
        bool_tuple_cubes_cover_all(&matching, dimension + 1)
    })
}

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
                Pattern::Wildcard(_) | Pattern::Identifier(_, _) => true,
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

        // A guarded arm is conditional, so it cannot prove that every value
        // matching its pattern is handled.
        let has_catch_all = arms
            .iter()
            .any(|arm| arm.guard.is_none() && pattern_is_catch_all(&arm.pattern));

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
                    if arm.guard.is_some() {
                        continue;
                    }

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
                                            matches!(
                                                p,
                                                Pattern::Wildcard(_) | Pattern::Identifier(_, _)
                                            )
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
                let has_true = arms.iter().any(|arm| {
                    arm.guard.is_none() && pattern_contains_bool_literal(&arm.pattern, true)
                });
                let has_false = arms.iter().any(|arm| {
                    arm.guard.is_none() && pattern_contains_bool_literal(&arm.pattern, false)
                });

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
                let Some(dimensions) = bool_tuple_dimensions(scrutinee_type) else {
                    let has_catch_all = arms
                        .iter()
                        .any(|arm| arm.guard.is_none() && pattern_is_catch_all(&arm.pattern));
                    if !has_catch_all {
                        self.error_coded(
                            "E031",
                            "Match on tuple requires a wildcard (_) pattern to cover remaining combinations.",
                            span,
                        );
                    }
                    return;
                };

                let mut bool_cubes = Vec::new();
                let mut unsupported_pattern = false;

                for arm in arms {
                    if arm.guard.is_some() {
                        continue;
                    }
                    match bool_tuple_pattern_cubes(&arm.pattern, scrutinee_type) {
                        Some(mut cubes) => bool_cubes.append(&mut cubes),
                        None => {
                            unsupported_pattern = true;
                            break;
                        }
                    }
                }

                if unsupported_pattern {
                    self.error_coded(
                        "E031",
                        "Match on tuple requires a wildcard (_) pattern to cover remaining combinations.",
                        span,
                    );
                } else if dimensions > 0 && !bool_tuple_cubes_cover_all(&bool_cubes, 0) {
                    self.error_coded(
                        "E031",
                        "Match on tuple of bools is not exhaustive; add patterns for the missing combinations or a wildcard arm.",
                        span,
                    );
                }
            }
            _ => {
                let only_literals = arms
                    .iter()
                    .filter(|arm| arm.guard.is_none())
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
