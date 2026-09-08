use super::*;

impl SemanticAnalyzer {
    pub(crate) fn infer_pattern_type(&mut self, pattern: &Pattern) -> Option<Type> {
        use crate::ast::Pattern;

        match pattern {
            Pattern::Wildcard(_) => None,
            Pattern::Identifier(_, _) => Some(Type::Unknown),
            Pattern::Literal(expr) => Some(self.infer_expression_type(expr)),
            Pattern::Tuple(elements) => Some(Type::Tuple {
                elements: elements
                    .iter()
                    .map(|pattern| self.infer_pattern_type(pattern).unwrap_or(Type::Unknown))
                    .collect(),
            }),
            Pattern::Struct { name, .. } => Some(Type::Struct { name: name.clone() }),
            Pattern::EnumVariant {
                enum_name,
                variant_name,
                ..
            } => {
                if let Some(enum_info) = self.enum_infos.get(enum_name) {
                    if enum_info.variants.contains_key(variant_name) {
                        return Some(Type::Enum {
                            name: enum_name.clone(),
                        });
                    }
                }
                None
            }
            Pattern::Or(patterns) => patterns
                .iter()
                .find_map(|pattern| self.infer_pattern_type(pattern)),
        }
    }

    pub(crate) fn bind_pattern_types(&mut self, pattern: &Pattern, ty: &Type) {
        use crate::ast::Pattern;

        let mut effective_type = ty.clone();
        if matches!(effective_type, Type::Unknown) {
            if let Some(inferred) = self.infer_pattern_type(pattern) {
                effective_type = inferred;
            }
        }

        match pattern {
            Pattern::Wildcard(_) => {}
            Pattern::Identifier(name, _) => {
                if let Some(scope) = self.symbols.last_mut() {
                    if let Some(info) = scope.get_mut(name) {
                        info.ty = effective_type.clone();
                    }
                }
            }
            Pattern::Literal(_) => {}
            Pattern::Tuple(elements) => {
                if let Type::Tuple {
                    elements: tuple_types,
                } = &effective_type
                {
                    for (pattern, field_type) in elements.iter().zip(tuple_types.iter()) {
                        self.bind_pattern_types(pattern, field_type);
                    }
                }
            }
            Pattern::Struct { name, fields } => {
                if let Some((_, struct_info, substitutions)) = self
                    .specialized_struct_context_for_type(&effective_type)
                    .or_else(|| {
                        self.struct_infos
                            .get(name)
                            .cloned()
                            .map(|info| (name.clone(), info, HashMap::new()))
                    })
                {
                    for (field_name, sub_pattern) in fields {
                        if let Some(field) = struct_info.fields.get(field_name) {
                            let field_type = self
                                .type_annotation_to_type_with_substitutions(
                                    &field.ty,
                                    &substitutions,
                                );
                            self.bind_pattern_types(sub_pattern, &field_type);
                        }
                    }
                }
            }
            Pattern::EnumVariant {
                enum_name,
                variant_name,
                data,
                struct_data,
                ..
            } => {
                let enum_type_name = match &effective_type {
                    Type::Enum { name } | Type::Applied { name, .. } => name.clone(),
                    _ => {
                        let Some(inferred) = self.infer_pattern_type(pattern) else {
                            return;
                        };
                        match inferred {
                            Type::Enum { name } | Type::Applied { name, .. } => name,
                            _ => return,
                        }
                    }
                };

                let Some((base_enum_name, enum_info, substitutions)) = self
                    .specialized_enum_context_for_type(&effective_type)
                    .or_else(|| self.specialized_enum_context(&enum_type_name))
                else {
                    return;
                };
                if base_enum_name != *enum_name {
                    return;
                }

                let Some(variant_info) = enum_info.variants.get(variant_name).cloned() else {
                    return;
                };

                if let (Some(sub_patterns), Some(field_types)) = (&data, &variant_info.data) {
                    if sub_patterns.len() != field_types.len() {
                        return;
                    }

                    let inferred_field_types: Vec<Type> = field_types
                        .iter()
                        .map(|ann| {
                            self.type_annotation_to_type_with_substitutions(ann, &substitutions)
                        })
                        .collect();

                    for (sub_pattern, field_type) in
                        sub_patterns.iter().zip(inferred_field_types.iter())
                    {
                        self.bind_pattern_types(sub_pattern, field_type);
                    }
                }

                if let (Some(named_patterns), Some(field_types)) =
                    (&struct_data, &variant_info.struct_data)
                {
                    let inferred_field_types: HashMap<String, Type> = field_types
                        .iter()
                        .map(|(name, ann)| {
                            (
                                name.clone(),
                                self.type_annotation_to_type_with_substitutions(
                                    ann,
                                    &substitutions,
                                ),
                            )
                        })
                        .collect();

                    for (field_name, sub_pattern) in named_patterns {
                        if let Some(field_type) = inferred_field_types.get(field_name) {
                            self.bind_pattern_types(sub_pattern, field_type);
                        }
                    }
                }
            }
            Pattern::Or(patterns) => {
                if let Some(first) = patterns.first() {
                    self.bind_pattern_types(first, &effective_type);
                }
            }
        }
    }

    pub(crate) fn check_return_statement(&mut self, value: Option<&Expression>, span: Span) {
        let expected = match self.current_return_type.as_ref() {
            Some(ty) => ty.clone(),
            None => return,
        };

        match value {
            Some(expr) => {
                if matches!(expected, Type::Unit) {
                    self.error_with_hint(
                        "Return statement in function with no return type",
                        expr.span,
                        "Remove the returned value or declare an explicit return type.",
                    );
                    return;
                }

                let saved_expected = self.current_expected_type.clone();
                self.current_expected_type = Some(expected.clone());
                let actual = self.infer_expression_type(expr);
                self.current_expected_type = saved_expected;
                if matches!(actual, Type::Unknown) {
                    if self.has_error_at_span(expr.span) {
                        return;
                    }
                    self.error_with_hint(
                        "Cannot determine return type: the expression has an unknown or uninferrable type",
                        expr.span,
                        "Add an explicit type annotation to help the compiler resolve the type.",
                    );
                    return;
                }
                if matches!(expected, Type::Unknown) {
                    // The function itself has an unresolvable return type; error already reported elsewhere.
                    return;
                }

                if !self.return_types_match(&actual, &expected) {
                    let hint = self.conversion_hint(&actual, &expected);
                    self.push_semantic_error_coded(
                        "E004",
                        format!(
                            "Return type mismatch: expected {}, found {}",
                            type_name(&expected),
                            type_name(&actual)
                        ),
                        expr.span,
                        Some(format!(
                            "function declared to return {}",
                            type_name(&expected)
                        )),
                        hint,
                    );
                }
            }
            None => {
                if !matches!(expected, Type::Unit | Type::Unknown) {
                    self.error_coded_with_hint(
                        "E005",
                        format!(
                            "Return statement missing value of type {}",
                            type_name(&expected)
                        ),
                        span,
                        "Supply a value or change the function's return type to `unit`.",
                    );
                }
            }
        }
    }

}
