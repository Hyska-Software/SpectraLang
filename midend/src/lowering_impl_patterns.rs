impl ASTLowering {
    /// Extrai valores do scrutinee e cria bindings locais de acordo com o pattern
    fn lower_pattern_bindings(
        &mut self,
        pattern: &spectra_compiler::ast::Pattern,
        scrutinee: Value,
        scrutinee_enum: Option<&str>,
        scrutinee_type: Option<&IRType>,
        ir_func: &mut IRFunction,
    ) {
        use spectra_compiler::ast::Pattern;

        match pattern {
            Pattern::Wildcard => {
                // Wildcard não cria bindings
            }
            Pattern::Identifier(name) => {
                // Criar variável local para o identifier binding
                // Usar value_map (valores diretos, não precisam de alloca/load)
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
                            let field_ptr = self
                                .builder
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
                if let Some(IRType::Struct {
                    fields: struct_fields,
                    ..
                }) = scrutinee_type.map(Self::ir_type_representation_static)
                {
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
                let ordered_patterns: Vec<&spectra_compiler::ast::Pattern> =
                    if let Some(patterns) = data {
                        patterns.iter().collect()
                    } else if let Some(named_patterns) = struct_data {
                        self.reorder_named_variant_patterns(
                            scrutinee_enum.unwrap_or(enum_name),
                            variant_name,
                            named_patterns,
                        )
                        .unwrap_or_else(|| {
                            named_patterns.iter().map(|(_, pattern)| pattern).collect()
                        })
                    } else {
                        Vec::new()
                    };

                if !ordered_patterns.is_empty() {
                    let mut variants = self
                        .enum_variants_from_ir_type(scrutinee_type)
                        .or_else(|| {
                            if let Some(IRType::Enum { name, .. }) = scrutinee_type
                                .map(Self::ir_type_representation_static)
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
                                    let element_ptr = self
                                        .builder
                                        .build_field_ptr(ir_func, scrutinee, byte_offset);
                                    let element_value = self.builder.build_load_typed(
                                        ir_func,
                                        element_ptr,
                                        sub_type.clone(),
                                    );

                                    let next_enum = match Self::ir_type_representation_static(sub_type) {
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

    fn lower_type_annotation_with_map(
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
                if is_std_api_handle_type_segments(segments) {
                    return IRType::Int;
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
                    "i8" => IRType::ExactInt { signed: true, width: IRIntWidth::I8 },
                    "i16" => IRType::ExactInt { signed: true, width: IRIntWidth::I16 },
                    "i32" => IRType::ExactInt { signed: true, width: IRIntWidth::I32 },
                    "i64" => IRType::ExactInt { signed: true, width: IRIntWidth::I64 },
                    "isize" => IRType::ExactInt { signed: true, width: IRIntWidth::Isize },
                    "u8" => IRType::ExactInt { signed: false, width: IRIntWidth::I8 },
                    "u16" => IRType::ExactInt { signed: false, width: IRIntWidth::I16 },
                    "u32" => IRType::ExactInt { signed: false, width: IRIntWidth::I32 },
                    "u64" => IRType::ExactInt { signed: false, width: IRIntWidth::I64 },
                    "usize" => IRType::ExactInt { signed: false, width: IRIntWidth::Usize },
                    "f32" => IRType::ExactFloat { width: IRFloatWidth::F32 },
                    "f64" => IRType::ExactFloat { width: IRFloatWidth::F64 },
                    "bool" => IRType::Bool,
                    "string" => IRType::String,
                    "char" => IRType::Char,
                    "Range" => IRType::Range,
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
                // back to the legacy integer ABI.
                if name == "List" {
                    let element_name = type_args
                        .first()
                        .map(|ann| self.type_annotation_to_string(ann))
                        .unwrap_or_else(|| "unknown".to_string());
                    return self.lower_generic_application(
                        name,
                        type_args,
                        IRType::Struct {
                            name: format!("List_{element_name}"),
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

                if matches!(name.as_str(), "Set" | "Iterator") {
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
                    if let Some(struct_type) = self.resolve_struct_type(name, type_args) {
                        if matches!(&struct_type, IRType::Generic { .. }) {
                            return struct_type;
                        }
                        return self.lower_generic_application(
                            name,
                            type_args,
                            struct_type,
                            substitutions,
                        );
                    }
                }

                // Resolve to the monomorphized enum type.
                // First, try the already-specialized version (e.g., "Option_int").
                let type_names: Vec<String> = type_args
                    .iter()
                    .map(|ty| self.type_annotation_to_string(ty))
                    .collect();
                let mangled = format!("{}_{}", name, type_names.join("_"));
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
                    for (param, arg) in generic_enum.type_params.iter().zip(type_args.iter()) {
                        type_map.insert(param.name.clone(), arg.clone());
                    }
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
                                            self.lower_type_annotation_with_map(&subst, substitutions)
                                        })
                                        .collect(),
                                )
                            } else { v.struct_data.as_ref().map(|fields| fields
                                        .iter()
                                        .map(|(_, ty)| {
                                            let subst = self.substitute_type(ty, &type_map);
                                            self.lower_type_annotation_with_map(&subst, substitutions)
                                        })
                                        .collect()) };
                            (v.name.clone(), data)
                        })
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
