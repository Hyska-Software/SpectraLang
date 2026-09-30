use super::*;

fn bool_tuple_dimensions(ty: &Type) -> Option<usize> {
    match ty {
        Type::Bool => Some(1),
        Type::Tuple { elements } => elements.iter().try_fold(0usize, |count, element| {
            count.checked_add(bool_tuple_dimensions(element)?)
        }),
        // Open domains do not add finite boolean dimensions. Their patterns
        // must still be catch-all bindings (handled by
        // `bool_tuple_pattern_cubes`); literals in those positions remain
        // unsupported as proof of exhaustiveness.
        _ => Some(0),
    }
}

fn has_bool_tuple_dimension(ty: &Type) -> bool {
    match ty {
        Type::Bool => true,
        Type::Tuple { elements } => elements.iter().any(has_bool_tuple_dimension),
        _ => false,
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

fn append_pattern_alternatives(
    pattern: &crate::ast::Pattern,
    output: &mut Vec<crate::ast::Pattern>,
) {
    match pattern {
        crate::ast::Pattern::Or(alternatives) => {
            for alternative in alternatives {
                append_pattern_alternatives(alternative, output);
            }
        }
        other => output.push(other.clone()),
    }
}

/// Determine whether a set of patterns covers every value of a payload type.
/// This is deliberately conservative for open scalar domains: literals never
/// prove exhaustiveness there. Structural records, tuples, booleans, and
/// nested enums are expanded according to their actual declared types.
fn patterns_cover_type(
    analyzer: &SemanticAnalyzer,
    patterns: &[crate::ast::Pattern],
    ty: &Type,
    depth: usize,
) -> bool {
    use crate::ast::Pattern;

    if depth > 64 {
        return false;
    }

    let mut alternatives = Vec::new();
    for pattern in patterns {
        append_pattern_alternatives(pattern, &mut alternatives);
    }
    if alternatives
        .iter()
        .any(|pattern| matches!(pattern, Pattern::Wildcard(_) | Pattern::Identifier(_, _)))
    {
        return true;
    }

    match ty {
        Type::Bool => {
            let mut cubes = Vec::new();
            for pattern in &alternatives {
                let Some(mut pattern_cubes) = bool_tuple_pattern_cubes(pattern, ty) else {
                    return false;
                };
                cubes.append(&mut pattern_cubes);
            }
            bool_tuple_cubes_cover_all(&cubes, 0)
        }
        Type::Tuple { elements } => {
            if has_bool_tuple_dimension(ty) {
                let mut cubes = Vec::new();
                for pattern in &alternatives {
                    let Some(mut pattern_cubes) = bool_tuple_pattern_cubes(pattern, ty) else {
                        return false;
                    };
                    cubes.append(&mut pattern_cubes);
                }
                return bool_tuple_cubes_cover_all(&cubes, 0);
            }

            alternatives.iter().any(|pattern| match pattern {
                Pattern::Tuple(parts) if parts.len() == elements.len() => {
                    elements.iter().zip(parts).all(|(element_type, part)| {
                        patterns_cover_type(
                            analyzer,
                            std::slice::from_ref(part),
                            element_type,
                            depth + 1,
                        )
                    })
                }
                _ => false,
            })
        }
        Type::Struct { .. } | Type::Applied { .. } => {
            if let Some((struct_name, info, substitutions)) =
                analyzer.specialized_struct_context_for_type(ty)
            {
                let mut field_names = info.fields.keys().cloned().collect::<Vec<_>>();
                field_names.sort();
                let field_types = field_names
                    .iter()
                    .filter_map(|name| {
                        info.fields.get(name).map(|field| {
                            analyzer.type_annotation_to_type_with_substitutions(
                                &field.ty,
                                &substitutions,
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                if field_types.len() != field_names.len() {
                    return false;
                }

                let mut tuple_patterns = Vec::new();
                for pattern in &alternatives {
                    let Pattern::Struct { name, fields } = pattern else {
                        continue;
                    };
                    if name != &struct_name {
                        continue;
                    }
                    tuple_patterns.push(Pattern::Tuple(
                        field_names
                            .iter()
                            .map(|field_name| {
                                fields
                                    .iter()
                                    .find(|(name, _)| name == field_name)
                                    .map(|(_, pattern)| pattern.clone())
                                    .unwrap_or(Pattern::Wildcard(Span::dummy()))
                            })
                            .collect(),
                    ));
                }
                return patterns_cover_type(
                    analyzer,
                    &tuple_patterns,
                    &Type::Tuple {
                        elements: field_types,
                    },
                    depth + 1,
                );
            }

            if let Some((enum_name, info, substitutions)) =
                analyzer.specialized_enum_context_for_type(ty)
            {
                for (variant_name, variant_info) in &info.variants {
                    let mut payload_patterns = Vec::new();
                    for pattern in &alternatives {
                        let Pattern::EnumVariant {
                            enum_name: pattern_enum,
                            variant_name: pattern_variant,
                            data,
                            struct_data,
                            ..
                        } = pattern
                        else {
                            continue;
                        };
                        if pattern_enum != &enum_name || pattern_variant != variant_name {
                            continue;
                        }

                        if let Some(payload_types) = &variant_info.data {
                            let Some(parts) = data else {
                                continue;
                            };
                            if parts.len() != payload_types.len() {
                                continue;
                            }
                            if payload_types.len() == 1 {
                                payload_patterns.push(parts[0].clone());
                            } else {
                                payload_patterns.push(Pattern::Tuple(parts.clone()));
                            }
                        } else if let Some(payload_fields) = &variant_info.struct_data {
                            let Some(fields) = struct_data else {
                                continue;
                            };
                            payload_patterns.push(Pattern::Tuple(
                                payload_fields
                                    .iter()
                                    .map(|(field_name, _)| {
                                        fields
                                            .iter()
                                            .find(|(name, _)| name == field_name)
                                            .map(|(_, pattern)| pattern.clone())
                                            .unwrap_or(Pattern::Wildcard(Span::dummy()))
                                    })
                                    .collect(),
                            ));
                        } else {
                            // The variant has no associated data; a matching
                            // constructor pattern covers the whole variant.
                            payload_patterns.push(Pattern::Wildcard(Span::dummy()));
                        }
                    }

                    if variant_info.data.is_none() && variant_info.struct_data.is_none() {
                        if payload_patterns.is_empty() {
                            return false;
                        }
                        continue;
                    }
                    if payload_patterns.is_empty() {
                        return false;
                    }

                    let payload_types = if let Some(types) = &variant_info.data {
                        types
                            .iter()
                            .map(|ty| {
                                analyzer
                                    .type_annotation_to_type_with_substitutions(ty, &substitutions)
                            })
                            .collect::<Vec<_>>()
                    } else {
                        variant_info
                            .struct_data
                            .as_ref()
                            .into_iter()
                            .flatten()
                            .map(|(_, ty)| {
                                analyzer
                                    .type_annotation_to_type_with_substitutions(ty, &substitutions)
                            })
                            .collect::<Vec<_>>()
                    };
                    let payload_type =
                        if variant_info.struct_data.is_some() || payload_types.len() != 1 {
                            Type::Tuple {
                                elements: payload_types,
                            }
                        } else {
                            payload_types[0].clone()
                        };
                    if !patterns_cover_type(analyzer, &payload_patterns, &payload_type, depth + 1) {
                        return false;
                    }
                }
                return !info.variants.is_empty();
            }

            false
        }
        _ => false,
    }
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
                Pattern::Tuple(patterns) => patterns.iter().all(pattern_is_catch_all),
                Pattern::Struct { fields, .. } => fields
                    .iter()
                    .all(|(_, pattern)| pattern_is_catch_all(pattern)),
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
                let Some((base_enum_name, enum_info, enum_substitutions)) =
                    self.specialized_enum_context_for_type(scrutinee_type)
                else {
                    self.error_coded(
                        "E031",
                        "Match expression is not exhaustive. Add an irrefutable pattern (such as '_') to cover remaining values.",
                        span,
                    );
                    return;
                };

                let mut covered_variants: HashSet<String> = HashSet::new();
                let mut variant_patterns: HashMap<String, Vec<Pattern>> = HashMap::new();

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
                            data: _,
                            ..
                        } = pattern
                        {
                            if enum_name != &base_enum_name {
                                continue;
                            }

                            covered_variants.insert(variant_name.clone());
                            variant_patterns
                                .entry(variant_name.clone())
                                .or_default()
                                .push(pattern.clone());
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

                let mut missing_payload_guard = Vec::new();
                for (variant, variant_info) in &enum_info.variants {
                    let has_payload = variant_info
                        .data
                        .as_ref()
                        .is_some_and(|payload| !payload.is_empty())
                        || variant_info
                            .struct_data
                            .as_ref()
                            .is_some_and(|payload| !payload.is_empty());
                    if !has_payload {
                        continue;
                    }

                    let Some(patterns) = variant_patterns.get(variant) else {
                        continue;
                    };
                    let mut payload_patterns = Vec::new();
                    let payload_types = if let Some(types) = &variant_info.data {
                        types
                            .iter()
                            .map(|ty| {
                                self.type_annotation_to_type_with_substitutions(
                                    ty,
                                    &enum_substitutions,
                                )
                            })
                            .collect::<Vec<_>>()
                    } else {
                        variant_info
                            .struct_data
                            .as_ref()
                            .into_iter()
                            .flatten()
                            .map(|(_, ty)| {
                                self.type_annotation_to_type_with_substitutions(
                                    ty,
                                    &enum_substitutions,
                                )
                            })
                            .collect::<Vec<_>>()
                    };

                    for pattern in patterns {
                        let Pattern::EnumVariant {
                            data, struct_data, ..
                        } = pattern
                        else {
                            continue;
                        };
                        if let Some(types) = &variant_info.data {
                            let Some(parts) = data else {
                                continue;
                            };
                            if parts.len() != types.len() {
                                continue;
                            }
                            payload_patterns.push(if parts.len() == 1 {
                                parts[0].clone()
                            } else {
                                Pattern::Tuple(parts.clone())
                            });
                        } else if let Some(fields) = &variant_info.struct_data {
                            let Some(pattern_fields) = struct_data else {
                                continue;
                            };
                            payload_patterns.push(Pattern::Tuple(
                                fields
                                    .iter()
                                    .map(|(field_name, _)| {
                                        pattern_fields
                                            .iter()
                                            .find(|(name, _)| name == field_name)
                                            .map(|(_, pattern)| pattern.clone())
                                            .unwrap_or(Pattern::Wildcard(Span::dummy()))
                                    })
                                    .collect(),
                            ));
                        }
                    }

                    let payload_type =
                        if variant_info.struct_data.is_some() || payload_types.len() != 1 {
                            Type::Tuple {
                                elements: payload_types,
                            }
                        } else {
                            payload_types[0].clone()
                        };
                    if !patterns_cover_type(self, &payload_patterns, &payload_type, 0) {
                        missing_payload_guard.push(format!("{}::{}", base_enum_name, variant));
                    }
                }

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
            Type::Tuple { .. } => {
                let Some(dimensions) = bool_tuple_dimensions(scrutinee_type) else {
                    self.error_coded(
                        "E031",
                        "Match on tuple requires an irrefutable tuple pattern or wildcard (_) to cover remaining combinations.",
                        span,
                    );
                    return;
                };
                if dimensions == 0 {
                    self.error_coded(
                        "E031",
                        "Match on tuple requires an irrefutable tuple pattern or wildcard (_) to cover remaining combinations.",
                        span,
                    );
                    return;
                }

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
                } else if !bool_tuple_cubes_cover_all(&bool_cubes, 0) {
                    self.error_coded(
                        "E031",
                        "Match on tuple is not exhaustive; add patterns for the missing boolean combinations or a wildcard arm.",
                        span,
                    );
                }
            }
            _ => {
                self.error_coded(
                    "E031",
                    "Match expression is not exhaustive. Add an irrefutable pattern (such as '_') to cover remaining values.",
                    span,
                );
            }
        }
    }
}
