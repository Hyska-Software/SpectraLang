use super::*;

impl SemanticAnalyzer {
    pub(crate) fn register_pattern_bindings(&mut self, pattern: &Pattern) {
        use crate::ast::Pattern;

        match pattern {
            Pattern::Wildcard => {
                // Não cria bindings
            }
            Pattern::Identifier(name) => {
                let is_local = self.symbols.len() > 1;
                // Registra a variável no escopo atual
                // Tipo será inferido posteriormente
                if let Some(scope) = self.symbols.last_mut() {
                    scope.insert(
                        name.clone(),
                        SymbolInfo {
                            is_local,
                            def_span: None, // Pattern doesn't currently easily provide span here
                            ty: Type::Unknown,
                        },
                    );
                }
            }
            Pattern::Literal(_) => {
                // Não cria bindings
            }
            Pattern::Tuple(elements) => {
                for element in elements {
                    self.register_pattern_bindings(element);
                }
            }
            Pattern::Struct { fields, .. } => {
                for (_, pattern) in fields {
                    self.register_pattern_bindings(pattern);
                }
            }
            Pattern::EnumVariant {
                enum_name: _,
                variant_name: _,
                data,
                struct_data,
                ..
            } => {
                // Se houver sub-patterns, registrar recursivamente
                if let Some(sub_patterns) = data {
                    for sub_pattern in sub_patterns {
                        self.register_pattern_bindings(sub_pattern);
                    }
                }
                if let Some(named_patterns) = struct_data {
                    for (_, sub_pattern) in named_patterns {
                        self.register_pattern_bindings(sub_pattern);
                    }
                }
            }
            Pattern::Or(patterns) => {
                if let Some(first) = patterns.first() {
                    self.register_pattern_bindings(first);
                }
            }
        }
    }

    pub(crate) fn normalize_bare_enum_pattern(&self, pattern: &mut Pattern, scrutinee_type: &Type) {
        let enum_name = match scrutinee_type {
            Type::Enum { name } | Type::Applied { name, .. } => name,
            _ => return,
        };

        if self
            .specialized_enum_context_for_type(scrutinee_type)
            .is_none()
        {
            return;
        }

        match pattern {
            Pattern::Identifier(variant_name) => {
                let is_variant = self
                    .specialized_enum_context(enum_name)
                    .map(|(_, info, _)| info.variants.contains_key(variant_name))
                    .unwrap_or(false);
                if is_variant {
                    let variant_name = variant_name.clone();
                    *pattern = Pattern::EnumVariant {
                        module_path: None,
                        enum_name: enum_name.clone(),
                        type_args: Vec::new(),
                        variant_name,
                        data: None,
                        struct_data: None,
                    };
                }
            }
            Pattern::Or(patterns) => {
                for pattern in patterns {
                    self.normalize_bare_enum_pattern(pattern, scrutinee_type);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn register_typed_pattern_bindings(&mut self, pattern: &Pattern, ty: &Type) {
        use crate::ast::Pattern;

        let mut effective_type = ty.clone();
        if matches!(effective_type, Type::Unknown) {
            if let Some(inferred) = self.infer_pattern_type(pattern) {
                effective_type = inferred;
            }
        }
        // Declaring a pattern binding revives any prior release state (E034).
        if let Pattern::Identifier(name) = pattern {
            self.uaf_on_bind(name, &effective_type);
        }

        match pattern {
            Pattern::Wildcard | Pattern::Literal(_) => {}
            Pattern::Identifier(name) => {
                let is_local = self.symbols.len() > 1;
                if let Some(scope) = self.symbols.last_mut() {
                    scope.insert(
                        name.clone(),
                        SymbolInfo {
                            is_local,
                            def_span: None,
                            ty: effective_type,
                        },
                    );
                }
            }
            Pattern::Tuple(elements) => {
                if let Type::Tuple {
                    elements: tuple_types,
                } = &effective_type
                {
                    for (pattern, field_type) in elements.iter().zip(tuple_types.iter()) {
                        self.register_typed_pattern_bindings(pattern, field_type);
                    }
                }
            }
            Pattern::Struct { name, fields } => {
                if let Type::Struct { name: struct_name } | Type::Applied {
                    name: struct_name, ..
                } = &effective_type
                {
                    if struct_name != name {
                        return;
                    }
                }

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
                            self.register_typed_pattern_bindings(sub_pattern, &field_type);
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
                let inferred_enum_type = if matches!(&effective_type, Type::Unknown) {
                    self.infer_pattern_type(pattern)
                } else {
                    None
                };
                let enum_context = self
                    .specialized_enum_context_for_type(&effective_type)
                    .or_else(|| {
                        inferred_enum_type
                            .as_ref()
                            .and_then(|ty| self.specialized_enum_context_for_type(ty))
                    });
                let Some((base_enum_name, enum_info, substitutions)) = enum_context
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
                        self.register_typed_pattern_bindings(sub_pattern, field_type);
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
                            self.register_typed_pattern_bindings(sub_pattern, field_type);
                        }
                    }
                }
            }
            Pattern::Or(patterns) => {
                if let Some(first) = patterns.first() {
                    self.register_typed_pattern_bindings(first, &effective_type);
                }
            }
        }
    }

}
