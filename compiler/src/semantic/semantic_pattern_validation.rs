use super::*;

impl SemanticAnalyzer {
    /// Ensure match arm patterns are compatible with the scrutinee type before binding names.
    pub(crate) fn validate_pattern_against_type(
        &mut self,
        pattern: &Pattern,
        scrutinee_type: &Type,
        match_span: Span,
    ) {
        use crate::ast::Pattern;

        match pattern {
            Pattern::Wildcard(_) | Pattern::Identifier(_, _) => {}
            Pattern::Literal(expr) => {
                if matches!(scrutinee_type, Type::Unknown) {
                    return;
                }

                let literal_type = self.infer_expression_type(expr);
                if matches!(literal_type, Type::Unknown) {
                    return;
                }

                if !self.generic_argument_types_match(&literal_type, scrutinee_type) {
                    self.error(
                        format!(
                            "Pattern literal of type {:?} cannot match value of type {:?}",
                            literal_type, scrutinee_type
                        ),
                        expr.span,
                    );
                }
            }
            Pattern::Tuple(elements) => match scrutinee_type {
                Type::Tuple {
                    elements: tuple_types,
                } => {
                    if elements.len() != tuple_types.len() {
                        self.error(
                            format!(
                                "Tuple pattern has {} element(s), but the scrutinee tuple has {}",
                                elements.len(),
                                tuple_types.len()
                            ),
                            match_span,
                        );
                        return;
                    }

                    for (pattern, expected) in elements.iter().zip(tuple_types.iter()) {
                        self.validate_pattern_against_type(pattern, expected, match_span);
                    }
                }
                Type::Unknown => {}
                other => {
                    self.error(
                        format!("Tuple pattern cannot match value of type {:?}", other),
                        match_span,
                    );
                }
            },
            Pattern::Struct { name, fields } => {
                match scrutinee_type {
                    Type::Struct { name: struct_name } if struct_name == name => {}
                    Type::Applied {
                        name: struct_name, ..
                    } if struct_name == name => {}
                    Type::Unknown => {}
                    Type::Struct { name: struct_name } => {
                        self.error(
                            format!(
                                "Struct pattern '{}' cannot match struct value of type '{}'",
                                name, struct_name
                            ),
                            match_span,
                        );
                        return;
                    }
                    Type::Applied {
                        name: struct_name, ..
                    } => {
                        self.error(
                            format!(
                                "Struct pattern '{}' cannot match struct value of type '{}'",
                                name, struct_name
                            ),
                            match_span,
                        );
                        return;
                    }
                    other => {
                        self.error(
                            format!(
                                "Struct pattern '{}' cannot match value of type {:?}",
                                name, other
                            ),
                            match_span,
                        );
                        return;
                    }
                }

                let Some((_, struct_info, substitutions)) = self
                    .specialized_struct_context_for_type(scrutinee_type)
                    .or_else(|| {
                        self.struct_infos
                            .get(name)
                            .cloned()
                            .map(|info| (name.clone(), info, HashMap::new()))
                    })
                else {
                    self.error(format!("Struct '{}' is not defined", name), match_span);
                    return;
                };

                let mut seen = HashSet::new();
                for (field_name, sub_pattern) in fields {
                    if !seen.insert(field_name.clone()) {
                        self.error(
                            format!(
                                "Field '{}' appears more than once in struct pattern '{}'",
                                field_name, name
                            ),
                            match_span,
                        );
                        continue;
                    }

                    let Some(field) = struct_info.fields.get(field_name) else {
                        self.error(
                            format!("Struct '{}' has no field named '{}'", name, field_name),
                            match_span,
                        );
                        continue;
                    };

                    let field_ty =
                        self.type_annotation_to_type_with_substitutions(&field.ty, &substitutions);
                    self.validate_pattern_against_type(sub_pattern, &field_ty, match_span);
                }
            }
            Pattern::EnumVariant {
                module_path,
                enum_name,
                type_args,
                variant_name,
                data,
                struct_data,
            } => {
                // ------------------------------------------------------------------
                // Qualified path in pattern: module::Enum::Variant
                // ------------------------------------------------------------------
                let (resolved_enum_name, _resolved_variant_name) = if let Some(ref mod_path) =
                    module_path
                {
                    let exports_cloned: Option<ModuleExports> = self
                        .registry
                        .read()
                        .unwrap_or_else(|p| p.into_inner())
                        .get_module(mod_path)
                        .cloned();
                    if let Some(exports) = exports_cloned {
                        if let Some(type_export) = exports.types.get(enum_name) {
                            if !type_export.is_enum {
                                self.error(
                                    format!(
                                        "'{}' from module '{}' is a struct, not an enum",
                                        enum_name, mod_path
                                    ),
                                    match_span,
                                );
                                return;
                            }
                            // Inject or update enum info with full payload data from registry.
                            if let Some(ref variants_map) = type_export.enum_variants {
                                let mut variants: HashMap<String, EnumVariantInfo> = HashMap::new();
                                for (vname, payload) in variants_map.iter() {
                                    let struct_data = type_export
                                        .enum_struct_variants
                                        .as_ref()
                                        .and_then(|sv| sv.get(vname))
                                        .cloned();
                                    let data = if struct_data.is_none() {
                                        payload.clone()
                                    } else {
                                        None
                                    };
                                    variants.insert(
                                        vname.clone(),
                                        EnumVariantInfo {
                                            data,
                                            struct_data,
                                            span: Span::dummy(),
                                        },
                                    );
                                }
                                // Also include struct-data variants not in enum_variants.
                                if let Some(ref sv) = type_export.enum_struct_variants {
                                    for (vname, fields) in sv.iter() {
                                        variants.entry(vname.clone()).or_insert(EnumVariantInfo {
                                            data: None,
                                            struct_data: Some(fields.clone()),
                                            span: Span::dummy(),
                                        });
                                    }
                                }
                                let member_names: Vec<String> = variants.keys().cloned().collect();
                                self.enum_infos.insert(
                                    enum_name.clone(),
                                    EnumInfo {
                                        visibility: Visibility::Public,
                                        type_params: vec![],
                                        variants,
                                    },
                                );
                                self.enum_definitions
                                    .insert(enum_name.clone(), member_names);
                            }
                            (enum_name.clone(), variant_name.clone())
                        } else {
                            self.error(
                                format!(
                                    "Module '{}' does not export type '{}'",
                                    mod_path, enum_name
                                ),
                                match_span,
                            );
                            return;
                        }
                    } else {
                        self.error(format!("Module '{}' is not defined", mod_path), match_span);
                        return;
                    }
                } else {
                    (enum_name.clone(), variant_name.clone())
                };

                let scrutinee_enum_context = match scrutinee_type {
                    Type::Unknown => self.specialized_enum_context(&resolved_enum_name),
                    _ => self.specialized_enum_context_for_type(scrutinee_type),
                };

                let (actual_enum_name, enum_info, substitutions) = match scrutinee_enum_context {
                    Some(context) => context,
                    None => match self.enum_infos.get(&resolved_enum_name).cloned() {
                        Some(info) => (resolved_enum_name.clone(), info, HashMap::new()),
                        None => {
                            self.error(
                                format!("Enum '{}' is not defined", resolved_enum_name),
                                match_span,
                            );
                            return;
                        }
                    },
                };

                if actual_enum_name != resolved_enum_name {
                    self.error(
                        format!(
                            "Pattern '{}::{}' cannot match enum value of type '{}'",
                            enum_name,
                            variant_name,
                            match scrutinee_type {
                                Type::Enum { name } | Type::Applied { name, .. } => name.as_str(),
                                _ => "<unknown>",
                            }
                        ),
                        match_span,
                    );
                    return;
                }

                if !matches!(
                    scrutinee_type,
                    Type::Enum { .. } | Type::Applied { .. } | Type::Unknown
                ) {
                    self.error(
                        format!(
                            "Pattern '{}::{}' cannot match value of type {:?}",
                            enum_name, variant_name, scrutinee_type
                        ),
                        match_span,
                    );
                    return;
                }

                if matches!(scrutinee_type, Type::Enum { .. } | Type::Applied { .. })
                    && self
                        .specialized_enum_context_for_type(scrutinee_type)
                        .is_none()
                {
                    let name = match scrutinee_type {
                        Type::Enum { name } | Type::Applied { name, .. } => name,
                        _ => &resolved_enum_name,
                    };
                    self.error(
                        format!(
                            "Pattern '{}::{}' cannot match enum value of type '{}'",
                            enum_name, variant_name, name
                        ),
                        match_span,
                    );
                    return;
                }

                if !type_args.is_empty() {
                    let expected_args = enum_info.type_params.len();
                    if expected_args == 0 {
                        self.error(
                            format!(
                                "Enum '{}' does not accept type arguments, but {} were provided in pattern",
                                enum_name,
                                type_args.len()
                            ),
                            match_span,
                        );
                    } else if type_args.len() != expected_args {
                        self.error(
                            format!(
                                "Enum '{}' pattern expects {} type argument(s), but {} were provided",
                                enum_name,
                                expected_args,
                                type_args.len()
                            ),
                            match_span,
                        );
                    }
                }

                let variant_info = match enum_info.variants.get(variant_name).cloned() {
                    Some(info) => info,
                    None => {
                        self.error(
                            format!(
                                "Enum '{}' has no variant named '{}'",
                                enum_name, variant_name
                            ),
                            match_span,
                        );
                        return;
                    }
                };

                match (
                    &variant_info.data,
                    &variant_info.struct_data,
                    data,
                    struct_data,
                ) {
                    (Some(_), _, _, Some(_)) => {
                        self.error(
                            format!(
                                "Variant '{}::{}' uses tuple payloads and cannot be matched with named fields",
                                enum_name, variant_name
                            ),
                            match_span,
                        );
                    }
                    (_, Some(_), Some(_), _) => {
                        self.error(
                            format!(
                                "Variant '{}::{}' uses named fields and cannot be matched with tuple-style patterns",
                                enum_name, variant_name
                            ),
                            match_span,
                        );
                    }
                    (Some(expected_types), _, Some(sub_patterns), None) => {
                        if expected_types.len() != sub_patterns.len() {
                            self.error(
                                format!(
                                    "Pattern for variant '{}::{}' has {} field(s), but {} were expected",
                                    enum_name,
                                    variant_name,
                                    sub_patterns.len(),
                                    expected_types.len()
                                ),
                                match_span,
                            );
                            return;
                        }

                        for (sub_pattern, expected_ann) in
                            sub_patterns.iter().zip(expected_types.iter())
                        {
                            let expected_ty = self.type_annotation_to_type_with_substitutions(
                                expected_ann,
                                &substitutions,
                            );
                            self.validate_pattern_against_type(
                                sub_pattern,
                                &expected_ty,
                                match_span,
                            );
                        }
                    }
                    (None, Some(expected_fields), None, Some(actual_fields)) => {
                        let expected_map: HashMap<String, crate::ast::TypeAnnotation> =
                            expected_fields.iter().cloned().collect();
                        let mut seen = HashSet::new();

                        for (field_name, sub_pattern) in actual_fields {
                            if !seen.insert(field_name.clone()) {
                                self.error(
                                    format!(
                                        "Field '{}' appears more than once in pattern '{}::{}'",
                                        field_name, enum_name, variant_name
                                    ),
                                    match_span,
                                );
                                continue;
                            }

                            let Some(expected_ann) = expected_map.get(field_name) else {
                                self.error(
                                    format!(
                                        "Variant '{}::{}' has no field named '{}'",
                                        enum_name, variant_name, field_name
                                    ),
                                    match_span,
                                );
                                continue;
                            };

                            let expected_ty = self.type_annotation_to_type_with_substitutions(
                                expected_ann,
                                &substitutions,
                            );
                            self.validate_pattern_against_type(
                                sub_pattern,
                                &expected_ty,
                                match_span,
                            );
                        }

                        for (field_name, _) in expected_fields {
                            if !seen.contains(field_name) {
                                self.error(
                                    format!(
                                        "Pattern for variant '{}::{}' is missing field '{}'",
                                        enum_name, variant_name, field_name
                                    ),
                                    match_span,
                                );
                            }
                        }
                    }
                    (Some(expected_types), _, None, None) => {
                        self.error(
                            format!(
                                "Pattern for variant '{}::{}' is missing {} field(s)",
                                enum_name,
                                variant_name,
                                expected_types.len()
                            ),
                            match_span,
                        );
                    }
                    (None, Some(expected_fields), None, None) => {
                        self.error(
                            format!(
                                "Pattern for variant '{}::{}' is missing {} named field(s)",
                                enum_name,
                                variant_name,
                                expected_fields.len()
                            ),
                            match_span,
                        );
                    }
                    (None, None, Some(sub_patterns), None) if !sub_patterns.is_empty() => {
                        self.error(
                            format!(
                                "Variant '{}::{}' does not contain any data",
                                enum_name, variant_name
                            ),
                            match_span,
                        );
                    }
                    (None, None, None, Some(actual_fields)) if !actual_fields.is_empty() => {
                        self.error(
                            format!(
                                "Variant '{}::{}' does not contain named fields",
                                enum_name, variant_name
                            ),
                            match_span,
                        );
                    }
                    _ => {}
                }
            }
            Pattern::Or(patterns) => {
                if patterns.is_empty() {
                    return;
                }

                let expected_names = self.collect_pattern_binding_names(&patterns[0]);
                for branch in patterns {
                    self.validate_pattern_against_type(branch, scrutinee_type, match_span);
                    if self.collect_pattern_binding_names(branch) != expected_names {
                        self.error_with_hint(
                            "All branches of an OR-pattern must bind the same names",
                            match_span,
                            "Rewrite the pattern so every branch binds the same identifiers in the same order.",
                        );
                        break;
                    }
                }
            }
        }
    }
}
