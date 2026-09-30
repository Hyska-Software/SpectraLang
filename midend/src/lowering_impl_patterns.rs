use super::*;

impl ASTLowering {
    fn lower_named_variant_or_pattern_bindings(
        &mut self,
        patterns: &[spectra_compiler::ast::Pattern],
        scrutinee: Value,
        scrutinee_enum: Option<&str>,
        scrutinee_type: Option<&IRType>,
        ir_func: &mut IRFunction,
    ) -> bool {
        use spectra_compiler::ast::Pattern;

        if patterns.len() < 2 {
            return false;
        }

        let enum_name =
            scrutinee_enum.or_else(|| scrutinee_type.and_then(|ty| self.ir_nominal_name(ty)));
        let Some(enum_name) = enum_name else {
            return false;
        };

        let variants = self
            .enum_variants_from_ir_type(scrutinee_type)
            .or_else(|| self.enum_definitions.get(enum_name).cloned());
        let Some(variants) = variants else {
            return false;
        };

        let mut alternatives = Vec::with_capacity(patterns.len());
        let mut seen_tags = HashSet::new();
        for pattern in patterns {
            let Pattern::EnumVariant {
                variant_name,
                data: None,
                struct_data: Some(named_patterns),
                ..
            } = pattern
            else {
                return false;
            };
            let Some((_, tag, Some(_))) = variants.iter().find(|(name, _, _)| name == variant_name)
            else {
                return false;
            };
            if !seen_tags.insert(*tag)
                || self
                    .reorder_named_variant_patterns(enum_name, variant_name, named_patterns)
                    .is_none()
            {
                // A variant tag identifies one alternative only when every
                // alternative has a distinct, known named-payload layout.
                return false;
            }
            alternatives.push((pattern, *tag as i64));
        }

        fn collect_binding_names(pattern: &Pattern, names: &mut Vec<String>) {
            match pattern {
                Pattern::Identifier(name, _) => {
                    if !names.contains(name) {
                        names.push(name.clone());
                    }
                }
                Pattern::Tuple(elements) => {
                    for element in elements {
                        collect_binding_names(element, names);
                    }
                }
                Pattern::Struct { fields, .. } => {
                    for (_, field_pattern) in fields {
                        collect_binding_names(field_pattern, names);
                    }
                }
                Pattern::EnumVariant {
                    data, struct_data, ..
                } => {
                    if let Some(patterns) = data {
                        for pattern in patterns {
                            collect_binding_names(pattern, names);
                        }
                    }
                    if let Some(fields) = struct_data {
                        for (_, pattern) in fields {
                            collect_binding_names(pattern, names);
                        }
                    }
                }
                Pattern::Or(patterns) => {
                    if let Some(first) = patterns.first() {
                        collect_binding_names(first, names);
                    }
                }
                Pattern::Wildcard(_) | Pattern::Literal(_) => {}
            }
        }

        let mut binding_names = Vec::new();
        collect_binding_names(alternatives[0].0, &mut binding_names);
        if binding_names.is_empty() {
            return false;
        }

        let zero = self.builder.build_const_int(ir_func, 0);
        let tag_ptr = self
            .builder
            .build_getelementptr(ir_func, scrutinee, zero, IRType::Int);
        let tag_value = self.builder.build_load(ir_func, tag_ptr);
        let merge_block = ir_func.add_block("or_pattern.bindings.merge");
        let mut incoming: Vec<Vec<(Value, usize, IRType)>> =
            binding_names.iter().map(|_| Vec::new()).collect();
        let mut next_check = None;

        for (index, (pattern, expected_tag)) in alternatives.iter().enumerate() {
            let binding_block = ir_func.add_block(format!("or_pattern.bindings.{index}"));
            if index + 1 < alternatives.len() {
                let next_block =
                    ir_func.add_block(format!("or_pattern.bindings.check.{}", index + 1));
                let expected_tag_value = self.builder.build_const_int(ir_func, *expected_tag);
                let matches_tag = self
                    .builder
                    .build_eq(ir_func, tag_value, expected_tag_value);
                self.builder
                    .build_cond_branch(ir_func, matches_tag, binding_block, next_block);
                next_check = Some(next_block);
            } else {
                self.builder.build_branch(ir_func, binding_block);
            }

            self.builder.set_current_block(binding_block);
            self.value_map.push_scope();
            self.alloca_map.push_scope();
            self.variable_types.push_scope();
            self.array_map.push_scope();
            self.range_map.push_scope();
            self.struct_var_map.push_scope();

            self.lower_pattern_bindings(
                pattern,
                scrutinee,
                Some(enum_name),
                scrutinee_type,
                ir_func,
            );

            let binding_exit = self.builder.get_current_block().unwrap_or(binding_block);
            for (binding_index, name) in binding_names.iter().enumerate() {
                match (
                    self.value_map.get_current(name),
                    self.variable_types.get(name),
                ) {
                    (Some(value), Some(ty)) => {
                        incoming[binding_index].push((value, binding_exit, ty))
                    }
                    _ => self.error(format!(
                        "OR-pattern alternative did not lower binding '{}'",
                        name
                    )),
                }
            }

            self.builder.build_branch(ir_func, merge_block);
            self.struct_var_map.pop_scope();
            self.range_map.pop_scope();
            self.array_map.pop_scope();
            self.variable_types.pop_scope();
            self.alloca_map.pop_scope();
            self.value_map.pop_scope();

            if let Some(next_check) = next_check.take() {
                self.builder.set_current_block(next_check);
            }
        }

        self.builder.set_current_block(merge_block);
        for (name, alternatives) in binding_names.into_iter().zip(incoming) {
            let Some((first_value, _, binding_type)) = alternatives.first().cloned() else {
                continue;
            };
            if alternatives
                .iter()
                .any(|(_, _, alternative_type)| alternative_type != &binding_type)
            {
                self.error(format!(
                    "OR-pattern binding '{}' has incompatible types across variants",
                    name
                ));
                continue;
            }

            let value = if alternatives.len() == 1 {
                first_value
            } else {
                self.builder.build_phi(
                    ir_func,
                    alternatives
                        .iter()
                        .map(|(value, block, _)| (*value, *block))
                        .collect(),
                )
            };
            self.bind_scoped_value(ir_func, &name, &binding_type, value);
            self.value_map.insert(name.clone(), value);
            self.variable_types.insert(name, binding_type);
        }

        true
    }

    fn lower_or_pattern_bindings_by_branch(
        &mut self,
        patterns: &[spectra_compiler::ast::Pattern],
        scrutinee: Value,
        scrutinee_enum: Option<&str>,
        scrutinee_type: Option<&IRType>,
        ir_func: &mut IRFunction,
    ) -> bool {
        use spectra_compiler::ast::Pattern;

        if patterns.len() < 2 {
            return false;
        }

        fn collect_binding_names(pattern: &Pattern, names: &mut Vec<String>) {
            match pattern {
                Pattern::Identifier(name, _) => {
                    if !names.contains(name) {
                        names.push(name.clone());
                    }
                }
                Pattern::Tuple(elements) => {
                    for element in elements {
                        collect_binding_names(element, names);
                    }
                }
                Pattern::Struct { fields, .. } => {
                    for (_, field_pattern) in fields {
                        collect_binding_names(field_pattern, names);
                    }
                }
                Pattern::EnumVariant {
                    data, struct_data, ..
                } => {
                    if let Some(patterns) = data {
                        for pattern in patterns {
                            collect_binding_names(pattern, names);
                        }
                    }
                    if let Some(fields) = struct_data {
                        for (_, pattern) in fields {
                            collect_binding_names(pattern, names);
                        }
                    }
                }
                Pattern::Or(patterns) => {
                    if let Some(first) = patterns.first() {
                        collect_binding_names(first, names);
                    }
                }
                Pattern::Wildcard(_) | Pattern::Literal(_) => {}
            }
        }

        let mut binding_names = Vec::new();
        collect_binding_names(&patterns[0], &mut binding_names);
        if binding_names.is_empty() {
            return false;
        }

        let merge_block = ir_func.add_block("or_pattern.bindings.merge");
        let mut incoming: Vec<Vec<(Value, usize, IRType)>> =
            binding_names.iter().map(|_| Vec::new()).collect();
        let mut next_check = None;

        for (index, pattern) in patterns.iter().enumerate() {
            let binding_block = ir_func.add_block(format!("or_pattern.bindings.branch.{index}"));
            if index + 1 < patterns.len() {
                let next_block =
                    ir_func.add_block(format!("or_pattern.bindings.next.{}", index + 1));
                let matches = self.lower_pattern_check(
                    pattern,
                    scrutinee,
                    scrutinee_enum,
                    scrutinee_type,
                    ir_func,
                );
                self.builder
                    .build_cond_branch(ir_func, matches, binding_block, next_block);
                next_check = Some(next_block);
            } else {
                // The enclosing OR-pattern check has already succeeded, so if
                // every earlier alternative failed, this final alternative
                // is necessarily the one that matched.
                self.builder.build_branch(ir_func, binding_block);
            }

            self.builder.set_current_block(binding_block);
            self.value_map.push_scope();
            self.alloca_map.push_scope();
            self.variable_types.push_scope();
            self.array_map.push_scope();
            self.range_map.push_scope();
            self.struct_var_map.push_scope();

            self.lower_pattern_bindings(
                pattern,
                scrutinee,
                scrutinee_enum,
                scrutinee_type,
                ir_func,
            );

            let binding_exit = self.builder.get_current_block().unwrap_or(binding_block);
            for (binding_index, name) in binding_names.iter().enumerate() {
                match (
                    self.value_map.get_current(name),
                    self.variable_types.get(name),
                ) {
                    (Some(value), Some(ty)) => {
                        incoming[binding_index].push((value, binding_exit, ty))
                    }
                    _ => self.error(format!(
                        "OR-pattern alternative did not lower binding '{}'",
                        name
                    )),
                }
            }

            self.builder.build_branch(ir_func, merge_block);
            self.struct_var_map.pop_scope();
            self.range_map.pop_scope();
            self.array_map.pop_scope();
            self.variable_types.pop_scope();
            self.alloca_map.pop_scope();
            self.value_map.pop_scope();

            if let Some(next_check) = next_check.take() {
                self.builder.set_current_block(next_check);
            }
        }

        self.builder.set_current_block(merge_block);
        for (name, alternatives) in binding_names.into_iter().zip(incoming) {
            let Some((first_value, _, binding_type)) = alternatives.first().cloned() else {
                continue;
            };
            if alternatives
                .iter()
                .any(|(_, _, alternative_type)| alternative_type != &binding_type)
            {
                self.error(format!(
                    "OR-pattern binding '{}' has incompatible types across alternatives",
                    name
                ));
                continue;
            }

            let value = if alternatives.len() == 1 {
                first_value
            } else {
                self.builder.build_phi(
                    ir_func,
                    alternatives
                        .iter()
                        .map(|(value, block, _)| (*value, *block))
                        .collect(),
                )
            };
            self.bind_scoped_value(ir_func, &name, &binding_type, value);
            self.value_map.insert(name.clone(), value);
            self.variable_types.insert(name, binding_type);
        }

        true
    }

    /// Extrai valores do scrutinee e cria bindings locais de acordo com o pattern
    pub(crate) fn lower_pattern_bindings(
        &mut self,
        pattern: &spectra_compiler::ast::Pattern,
        scrutinee: Value,
        scrutinee_enum: Option<&str>,
        scrutinee_type: Option<&IRType>,
        ir_func: &mut IRFunction,
    ) {
        use spectra_compiler::ast::Pattern;

        match pattern {
            Pattern::Wildcard(_) => {
                // Wildcard não cria bindings
            }
            Pattern::Identifier(name, _) => {
                // Criar variável local para o identifier binding
                // Usar value_map (valores diretos, não precisam de alloca/load)
                let binding_type = scrutinee_type.cloned().unwrap_or(IRType::Int);
                self.bind_scoped_value(ir_func, name, &binding_type, scrutinee);
                self.value_map.insert(name.clone(), scrutinee);
                if let Some(ty) = scrutinee_type {
                    self.variable_types.insert(name.clone(), ty.clone());
                }
            }
            Pattern::Literal(_) => {
                // Literal não cria bindings
            }
            Pattern::Tuple(elements) => {
                if let Some(IRType::Tuple {
                    elements: tuple_types,
                }) = scrutinee_type
                {
                    for (idx, pattern) in elements.iter().enumerate() {
                        if let Some(field_ty) = tuple_types.get(idx) {
                            let byte_offset =
                                layout::layout_of(tuple_types.iter()).offsets[idx] as i64;
                            let field_ptr =
                                self.builder
                                    .build_field_ptr(ir_func, scrutinee, byte_offset);
                            let field_value =
                                self.builder
                                    .build_load_typed(ir_func, field_ptr, field_ty.clone());
                            self.lower_pattern_bindings(
                                pattern,
                                field_value,
                                None,
                                Some(field_ty),
                                ir_func,
                            );
                        }
                    }
                }
            }
            Pattern::Struct { fields, .. } => {
                if let Some(struct_fields) =
                    scrutinee_type.and_then(|ty| self.struct_fields_for_type(ty))
                {
                    let struct_fields = &struct_fields;
                    let field_map: HashMap<String, (usize, IRType)> = struct_fields
                        .iter()
                        .cloned()
                        .enumerate()
                        .map(|(idx, (name, ty))| (name, (idx, ty)))
                        .collect();
                    let struct_layout = layout::layout_of(struct_fields.iter().map(|(_, ty)| ty));
                    for (field_name, pattern) in fields {
                        if let Some((idx, field_ty)) = field_map.get(field_name) {
                            let field_ptr = self.builder.build_field_ptr(
                                ir_func,
                                scrutinee,
                                struct_layout.offsets[*idx] as i64,
                            );
                            let field_value =
                                self.builder
                                    .build_load_typed(ir_func, field_ptr, field_ty.clone());
                            self.lower_pattern_bindings(
                                pattern,
                                field_value,
                                None,
                                Some(field_ty),
                                ir_func,
                            );
                        }
                    }
                }
            }
            Pattern::EnumVariant {
                enum_name,
                type_args,
                variant_name,
                data,
                struct_data,
                ..
            } => {
                // Se há patterns de data, extrair valores e fazer binding recursivo
                let ordered_patterns: Vec<&spectra_compiler::ast::Pattern> = if let Some(patterns) =
                    data
                {
                    patterns.iter().collect()
                } else if let Some(named_patterns) = struct_data {
                    self.reorder_named_variant_patterns(
                        scrutinee_enum.unwrap_or(enum_name),
                        variant_name,
                        named_patterns,
                    )
                    .unwrap_or_else(|| named_patterns.iter().map(|(_, pattern)| pattern).collect())
                } else {
                    Vec::new()
                };

                if !ordered_patterns.is_empty() {
                    let mut variants = self
                        .enum_variants_from_ir_type(scrutinee_type)
                        .or_else(|| {
                            if let Some(IRType::Enum { name, .. }) =
                                scrutinee_type.map(Self::ir_type_representation_static)
                            {
                                self.enum_definitions.get(name).cloned()
                            } else {
                                None
                            }
                        })
                        .or_else(|| {
                            scrutinee_enum.and_then(|name| self.enum_definitions.get(name).cloned())
                        })
                        .or_else(|| self.enum_definitions.get(enum_name).cloned());

                    if variants.is_none() && !type_args.is_empty() {
                        let (_, specialized) =
                            self.ensure_enum_definition(enum_name, type_args.as_slice());
                        variants = Some(specialized);
                    }

                    if let Some(variants) = variants {
                        if let Some((_, _tag, Some(types))) =
                            variants.iter().find(|(name, _, _)| name == variant_name)
                        {
                            // Layout da tupla (tag + dados): offsets padded
                            let variant_layout = layout::layout_of(types.iter());
                            // Para cada pattern de data, extrair o valor correspondente
                            for (idx, sub_pattern) in ordered_patterns.iter().enumerate() {
                                if let Some(sub_type) = types.get(idx) {
                                    // Extrair elemento idx+1 da tuple (idx 0 é o tag)
                                    let byte_offset = 8 + variant_layout.offsets[idx] as i64;
                                    let element_ptr = self.builder.build_field_ptr(
                                        ir_func,
                                        scrutinee,
                                        byte_offset,
                                    );
                                    let element_value = self.builder.build_load_typed(
                                        ir_func,
                                        element_ptr,
                                        sub_type.clone(),
                                    );

                                    let next_enum =
                                        match Self::ir_type_representation_static(sub_type) {
                                            IRType::Enum { name, .. } => Some(name.clone()),
                                            _ => None,
                                        };

                                    // Recursivamente fazer binding do sub-pattern
                                    self.lower_pattern_bindings(
                                        sub_pattern,
                                        element_value,
                                        next_enum.as_deref(),
                                        Some(sub_type),
                                        ir_func,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            Pattern::Or(patterns) => {
                if self.lower_named_variant_or_pattern_bindings(
                    patterns,
                    scrutinee,
                    scrutinee_enum,
                    scrutinee_type,
                    ir_func,
                ) {
                    return;
                }
                if self.lower_or_pattern_bindings_by_branch(
                    patterns,
                    scrutinee,
                    scrutinee_enum,
                    scrutinee_type,
                    ir_func,
                ) {
                    return;
                }
                if let Some(first) = patterns.first() {
                    self.lower_pattern_bindings(
                        first,
                        scrutinee,
                        scrutinee_enum,
                        scrutinee_type,
                        ir_func,
                    );
                }
            }
        }
    }

    pub(crate) fn lower_type_annotation_with_map(
        &self,
        type_ann: &TypeAnnotation,
        substitutions: &HashMap<String, IRType>,
    ) -> IRType {
        use spectra_compiler::ast::TypeAnnotationKind;

        match &type_ann.kind {
            TypeAnnotationKind::Simple { segments } => {
                if segments.is_empty() {
                    return IRType::Unknown;
                }

                // Check if this is a type parameter that needs substitution
                let type_name = segments[0].as_str();
                if let Some(concrete_type) = substitutions.get(type_name) {
                    return concrete_type.clone();
                }
                if let Some(alias) = self.type_aliases.get(type_name).cloned() {
                    return self.lower_type_annotation_with_map(&alias, substitutions);
                }

                match type_name {
                    "int" => IRType::Int,
                    "float" => IRType::Float,
                    "i8" => IRType::ExactInt {
                        signed: true,
                        width: IRIntWidth::I8,
                    },
                    "i16" => IRType::ExactInt {
                        signed: true,
                        width: IRIntWidth::I16,
                    },
                    "i32" => IRType::ExactInt {
                        signed: true,
                        width: IRIntWidth::I32,
                    },
                    "i64" => IRType::ExactInt {
                        signed: true,
                        width: IRIntWidth::I64,
                    },
                    "isize" => IRType::ExactInt {
                        signed: true,
                        width: IRIntWidth::Isize,
                    },
                    "u8" => IRType::ExactInt {
                        signed: false,
                        width: IRIntWidth::I8,
                    },
                    "u16" => IRType::ExactInt {
                        signed: false,
                        width: IRIntWidth::I16,
                    },
                    "u32" => IRType::ExactInt {
                        signed: false,
                        width: IRIntWidth::I32,
                    },
                    "u64" => IRType::ExactInt {
                        signed: false,
                        width: IRIntWidth::I64,
                    },
                    "usize" => IRType::ExactInt {
                        signed: false,
                        width: IRIntWidth::Usize,
                    },
                    "f32" => IRType::ExactFloat {
                        width: IRFloatWidth::F32,
                    },
                    "f64" => IRType::ExactFloat {
                        width: IRFloatWidth::F64,
                    },
                    "bool" => IRType::Bool,
                    "string" => IRType::String,
                    "char" => IRType::Char,
                    "Range" => IRType::Range,
                    "BitSet" | "DisjointSet" => IRType::Struct {
                        name: type_name.to_string(),
                        fields: Vec::new(),
                    },
                    _ => {
                        // Check if this is a struct type
                        if let Some(fields) = self.struct_definitions.get(type_name) {
                            IRType::Struct {
                                name: type_name.to_string(),
                                fields: fields.clone(),
                            }
                        } else if let Some(variants) = self.enum_definitions.get(type_name) {
                            let simplified = variants
                                .iter()
                                .map(|(variant_name, _, data)| (variant_name.clone(), data.clone()))
                                .collect();
                            IRType::Enum {
                                name: type_name.to_string(),
                                variants: simplified,
                            }
                        } else if let Some(generic_enum) = self.generic_enums.get(type_name) {
                            let simplified = generic_enum
                                .variants
                                .iter()
                                .map(|variant| {
                                    let data_types = variant.data.as_ref().map(|types| {
                                        types
                                            .iter()
                                            .map(|ann| self.lower_type_annotation(ann))
                                            .collect::<Vec<_>>()
                                    });
                                    (variant.name.clone(), data_types)
                                })
                                .collect();
                            IRType::Enum {
                                name: type_name.to_string(),
                                variants: simplified,
                            }
                        } else if let Some(specialized) =
                            self.specialized_generic_annotation(type_name)
                        {
                            self.lower_type_annotation_with_map(&specialized, substitutions)
                        } else if let Some(nominal) = self.nominal_type_reference(type_name) {
                            // Forward/self reference to an aggregate whose
                            // definition is still being registered.
                            nominal
                        } else if is_std_api_handle_type_segments(segments) {
                            // A project aggregate takes precedence over an
                            // opaque API handle with the same short name.
                            IRType::Int
                        } else {
                            IRType::Unknown
                        }
                    }
                }
            }
            TypeAnnotationKind::Tuple { elements } => {
                let ir_elements: Vec<IRType> = elements
                    .iter()
                    .map(|elem_ann| self.lower_type_annotation_with_map(elem_ann, substitutions))
                    .collect();
                IRType::Tuple {
                    elements: ir_elements,
                }
            }
            TypeAnnotationKind::Function {
                params,
                return_type,
            } => {
                let ir_params = params
                    .iter()
                    .map(|ann| self.lower_type_annotation_with_map(ann, substitutions))
                    .collect();
                let ir_return_type =
                    Box::new(self.lower_type_annotation_with_map(return_type, substitutions));
                IRType::Function {
                    params: ir_params,
                    return_type: ir_return_type,
                }
            }
            TypeAnnotationKind::Generic { name, type_args } => {
                if name == "array" {
                    let element_type = type_args
                        .first()
                        .map(|ann| self.lower_type_annotation_with_map(ann, substitutions))
                        .unwrap_or(IRType::Unknown);
                    return IRType::Array {
                        element_type: Box::new(element_type),
                        size: 0,
                    };
                }

                // `std.collections` is a compiler/runtime intrinsic rather
                // than a user-declared generic struct.  Preserve its concrete
                // type arguments in the IR name so host-call return refinement
                // can recover `T`, `K`, and `V` at the call site.  Falling
                // through to the named-type path here turns `List<string>`
                // into `Void`, which silently degrades `list_get`/`map_get`
                // to untyped integers.
                if matches!(name.as_str(), "List" | "Vector") {
                    let element_name = type_args
                        .first()
                        .map(|ann| self.type_annotation_to_string(ann))
                        .unwrap_or_else(|| "unknown".to_string());
                    return self.lower_generic_application(
                        name,
                        type_args,
                        IRType::Struct {
                            name: format!("{name}_{element_name}"),
                            fields: Vec::new(),
                        },
                        substitutions,
                    );
                }

                if name == "Map" {
                    let key_name = type_args
                        .first()
                        .map(|ann| self.type_annotation_to_string(ann))
                        .unwrap_or_else(|| "unknown".to_string());
                    let value_name = type_args
                        .get(1)
                        .map(|ann| self.type_annotation_to_string(ann))
                        .unwrap_or_else(|| "unknown".to_string());
                    return self.lower_generic_application(
                        name,
                        type_args,
                        IRType::Struct {
                            name: format!("Map_{key_name}_{value_name}"),
                            fields: Vec::new(),
                        },
                        substitutions,
                    );
                }

                if name == "OrderedMap" {
                    let key_name = type_args
                        .first()
                        .map(|ann| self.type_annotation_to_string(ann))
                        .unwrap_or_else(|| "unknown".to_string());
                    let value_name = type_args
                        .get(1)
                        .map(|ann| self.type_annotation_to_string(ann))
                        .unwrap_or_else(|| "unknown".to_string());
                    return self.lower_generic_application(
                        name,
                        type_args,
                        IRType::Struct {
                            name: format!("OrderedMap_{key_name}_{value_name}"),
                            fields: Vec::new(),
                        },
                        substitutions,
                    );
                }

                if matches!(
                    name.as_str(),
                    "Set" | "Iterator" | "Stack" | "Queue" | "HashSet" | "PriorityQueue"
                ) {
                    let element_name = type_args
                        .first()
                        .map(|ann| self.type_annotation_to_string(ann))
                        .unwrap_or_else(|| "unknown".to_string());
                    return self.lower_generic_application(
                        name,
                        type_args,
                        IRType::Struct {
                            name: format!("{name}_{element_name}"),
                            fields: Vec::new(),
                        },
                        substitutions,
                    );
                }

                if name == "Tensor" && !type_args.is_empty() {
                    let dtype = self.lower_type_annotation_with_map(&type_args[0], substitutions);
                    let meta = tensor_metadata(&type_args[1..]);
                    return IRType::Tensor {
                        dtype: Box::new(dtype),
                        rank: meta.rank,
                        dims: meta.dims,
                        layout: meta.layout,
                        device: meta.device,
                    };
                }

                if name == "Task" {
                    let output = type_args
                        .first()
                        .map(|ann| self.lower_type_annotation_with_map(ann, substitutions))
                        .unwrap_or(IRType::Unknown);
                    return IRType::Task {
                        output: Box::new(output),
                    };
                }

                if name == "Box" {
                    if let Some(TypeAnnotation {
                        kind:
                            TypeAnnotationKind::DynTrait {
                                trait_name,
                                auto_traits,
                            },
                        ..
                    }) = type_args.first()
                    {
                        return IRType::DynTrait {
                            trait_name: trait_name.clone(),
                            auto_traits: auto_traits.clone(),
                        };
                    }
                }

                if self.generic_structs.contains_key(name.as_str()) {
                    // Apply the caller's explicit substitution map before
                    // specializing the nominal struct. This path is also
                    // used while inferring a generic call's return type, where
                    // the type parameters live in a local map and are not yet
                    // installed as the lowering-wide substitution state.
                    let concrete_type_args =
                        self.substituted_type_args_with(type_args, substitutions);
                    if let Some(struct_type) = self.resolve_struct_type(name, &concrete_type_args) {
                        if matches!(&struct_type, IRType::Generic { .. }) {
                            return struct_type;
                        }
                        return self.lower_generic_application(
                            name,
                            &concrete_type_args,
                            struct_type,
                            substitutions,
                        );
                    }
                }

                // Resolve to the monomorphized enum type.
                // First, try the already-specialized version (e.g., "Option_int").
                // The active substitutions are applied before mangling so a
                // `Seq<T>` reference inside a `T -> int` specialization keys
                // the same `Seq_int` definition as an explicit `Seq<int>`.
                let substituted_args: Vec<TypeAnnotation> =
                    self.substituted_type_args_with(type_args, substitutions);
                let type_names: Vec<String> = substituted_args
                    .iter()
                    .map(|ty| self.type_annotation_to_string(ty))
                    .collect();
                let mangled = format!("{}_{}", name, type_names.join("_"));
                // A recursive reference to the specialization currently being
                // computed must not re-enter it: emit the nominal application
                // and let the registered definition supply the structure.
                if self.specializing_enums.borrow().contains(&mangled) {
                    let args: Vec<IRType> = substituted_args
                        .iter()
                        .map(|arg| self.lower_type_annotation_with_map(arg, substitutions))
                        .collect();
                    return IRType::Generic {
                        name: name.clone(),
                        args,
                        representation: Box::new(IRType::Enum {
                            name: mangled,
                            variants: Vec::new(),
                        }),
                    };
                }
                if let Some(variants) = self.enum_definitions.get(&mangled) {
                    let simplified = variants
                        .iter()
                        .map(|(vn, _, data)| (vn.clone(), data.clone()))
                        .collect();
                    return self.lower_generic_application(
                        name,
                        type_args,
                        IRType::Enum {
                            name: mangled,
                            variants: simplified,
                        },
                        substitutions,
                    );
                }
                // Specialization not yet registered — use the generic enum with substituted
                // type args so the caller at least gets Enum { name: "Option_int", ... }.
                // The actual specialization will be triggered by ensure_enum_definition
                // during lowering of the call site.
                if let Some(generic_enum) = self.generic_enums.get(name.as_str()) {
                    let mut type_map: HashMap<String, TypeAnnotation> = HashMap::new();
                    for (param, arg) in generic_enum.type_params.iter().zip(substituted_args.iter())
                    {
                        type_map.insert(param.name.clone(), arg.clone());
                    }
                    // Nested references to this same application must not
                    // re-enter the substitution.
                    self.specializing_enums.borrow_mut().insert(mangled.clone());
                    let simplified: Vec<(String, Option<Vec<IRType>>)> = generic_enum
                        .variants
                        .iter()
                        .map(|v| {
                            let data = if let Some(types) = v.data.as_ref() {
                                Some(
                                    types
                                        .iter()
                                        .map(|ty| {
                                            let subst = self.substitute_type(ty, &type_map);
                                            self.lower_type_annotation_with_map(
                                                &subst,
                                                substitutions,
                                            )
                                        })
                                        .collect(),
                                )
                            } else {
                                v.struct_data.as_ref().map(|fields| {
                                    fields
                                        .iter()
                                        .map(|(_, ty)| {
                                            let subst = self.substitute_type(ty, &type_map);
                                            self.lower_type_annotation_with_map(
                                                &subst,
                                                substitutions,
                                            )
                                        })
                                        .collect()
                                })
                            };
                            (v.name.clone(), data)
                        })
                        .collect();
                    self.specializing_enums.borrow_mut().remove(&mangled);
                    return self.lower_generic_application(
                        name,
                        type_args,
                        IRType::Enum {
                            name: mangled,
                            variants: simplified,
                        },
                        substitutions,
                    );
                }
                // An unresolved generic application is poison.  It must not
                // be reinterpreted as a simple named type because that hides
                // a missing specialization until code generation.
                IRType::Unknown
            }
            TypeAnnotationKind::DynTrait {
                trait_name,
                auto_traits,
            } => IRType::DynTrait {
                trait_name: trait_name.clone(),
                auto_traits: auto_traits.clone(),
            },
        }
    }
}
