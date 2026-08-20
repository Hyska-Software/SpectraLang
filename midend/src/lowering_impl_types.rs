impl ASTLowering {
    fn ensure_struct_definition(
        &mut self,
        base_name: &str,
        type_args: &[TypeAnnotation],
    ) -> (String, Vec<(String, IRType)>) {
        if type_args.is_empty() {
            if let Some(fields) = self.struct_definitions.get(base_name).cloned() {
                return (base_name.to_string(), fields);
            }

            if let Some(generic_struct) = self.generic_structs.get(base_name).cloned() {
                self.error(format!(
                    "generic struct '{}' requires explicit type arguments ({})",
                    base_name,
                    generic_struct
                        .type_params
                        .iter()
                        .map(|param| param.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                return (base_name.to_string(), Vec::new());
            }

            self.error(format!(
                "Struct '{}' was not registered before lowering; check the semantic phase",
                base_name
            ));
            return (base_name.to_string(), Vec::new());
        }

        let type_names: Vec<String> = type_args
            .iter()
            .map(|ty| self.type_annotation_to_string(ty))
            .collect();
        let mangled = format!("{}_{}", base_name, type_names.join("_"));

        if !self.struct_definitions.contains_key(&mangled) {
            if let Some(generic_struct) = self.generic_structs.get(base_name).cloned() {
                self.specialize_struct(&generic_struct, type_args, &mangled);
            } else {
                self.error(format!(
                    "Generic struct '{}' not found for specialization with arguments {:?}",
                    base_name, type_names
                ));
            }
        }

        // Track the instantiation for generic impl method specialization (R-211).
        let concrete_ir_types: Vec<IRType> = type_args
            .iter()
            .map(|ann| self.lower_type_annotation(ann))
            .collect();
        self.instantiated_structs.insert(
            mangled.clone(),
            (base_name.to_string(), concrete_ir_types),
        );

        let fields = self
            .struct_definitions
            .get(&mangled)
            .cloned()
            .unwrap_or_else(|| {
                self.error(format!(
                    "Struct '{}' not registered after specialization",
                    mangled
                ));
                Vec::new()
            });

        (mangled, fields)
    }

    fn lower_default_struct_value(
        &mut self,
        base_name: &str,
        type_args: &[TypeAnnotation],
        ir_func: &mut IRFunction,
    ) -> Value {
        let (actual_name, field_defs) = self.ensure_struct_definition(base_name, type_args);
        let struct_type = IRType::Struct {
            name: actual_name,
            fields: field_defs.clone(),
        };
        let struct_ptr = self.builder.build_alloca(ir_func, struct_type);
        let layout = layout::layout_of(field_defs.iter().map(|(_, ty)| ty));
        for (field_idx, (_, field_type)) in field_defs.iter().enumerate() {
            let field_ptr = self.builder.build_field_ptr(
                ir_func,
                struct_ptr,
                layout.offsets[field_idx] as i64,
            );
            let value = self.lower_default_value_for_type(field_type, ir_func);
            self.builder.build_store(ir_func, field_ptr, value);
        }
        struct_ptr
    }

    fn lower_default_value_for_type(&mut self, ty: &IRType, ir_func: &mut IRFunction) -> Value {
        match ty {
            IRType::Float => self.builder.build_const_float(ir_func, 0.0),
            IRType::Bool => self.builder.build_const_bool(ir_func, false),
            IRType::ExactFloat { .. } => {
                self.builder
                    .build_const_float_typed(ir_func, 0.0, ty.clone())
            }
            IRType::ExactInt { .. } => {
                self.builder.build_const_int_typed(ir_func, 0, ty.clone())
            }
            IRType::Char => self
                .builder
                .build_const_int_typed(ir_func, 0, IRType::Char),
            IRType::String => self.lower_string_literal("", ir_func),
            IRType::Struct { name, .. } => self.lower_default_struct_value(name, &[], ir_func),
            IRType::Unknown => self.invalid_value(
                "cannot synthesize a default value for an unresolved IR type",
            ),
            IRType::Int => self.builder.build_const_int(ir_func, 0),
            unsupported => self.invalid_value(format!(
                "cannot synthesize a default value for IR type {:?}",
                unsupported
            )),
        }
    }

    fn ensure_enum_definition(
        &mut self,
        base_name: &str,
        type_args: &[TypeAnnotation],
    ) -> (String, Vec<EnumVariantDefinition>) {
        if type_args.is_empty() {
            if let Some(variants) = self.enum_definitions.get(base_name).cloned() {
                return (base_name.to_string(), variants);
            }

            if let Some(generic_enum) = self.generic_enums.get(base_name).cloned() {
                self.error(format!(
                    "generic enum '{}' requires explicit type arguments ({})",
                    base_name,
                    generic_enum
                        .type_params
                        .iter()
                        .map(|param| param.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                return (base_name.to_string(), Vec::new());
            }

            self.error(format!(
                "Enum '{}' was not registered before lowering; check the semantic phase",
                base_name
            ));
            return (base_name.to_string(), Vec::new());
        }

        let type_names: Vec<String> = type_args
            .iter()
            .map(|ty| self.type_annotation_to_string(ty))
            .collect();
        let mangled = format!("{}_{}", base_name, type_names.join("_"));

        if !self.enum_definitions.contains_key(&mangled) {
            if let Some(generic_enum) = self.generic_enums.get(base_name).cloned() {
                self.specialize_enum(&generic_enum, type_args, &mangled);
            } else {
                self.error(format!(
                    "Generic enum '{}' not found for specialization with arguments {:?}",
                    base_name, type_names
                ));
            }
        }

        let variants = self
            .enum_definitions
            .get(&mangled)
            .cloned()
            .unwrap_or_else(|| {
                self.error(format!(
                    "Enum '{}' not registered after specialization",
                    mangled
                ));
                Vec::new()
            });

        (mangled, variants)
    }

    fn resolve_struct_type(&self, base_name: &str, type_args: &[TypeAnnotation]) -> Option<IRType> {
        if type_args.is_empty() {
            return self
                .struct_definitions
                .get(base_name)
                .cloned()
                .map(|fields| IRType::Struct {
                    name: base_name.to_string(),
                    fields,
                });
        }

        let type_names: Vec<String> = type_args
            .iter()
            .map(|ty| self.type_annotation_to_string(ty))
            .collect();
        let mangled = format!("{}_{}", base_name, type_names.join("_"));

        if let Some(fields) = self.struct_definitions.get(&mangled) {
            return Some(IRType::Generic {
                name: base_name.to_string(),
                args: type_args
                    .iter()
                    .map(|arg| self.lower_type_annotation(arg))
                    .collect(),
                representation: Box::new(IRType::Struct {
                    name: mangled,
                    fields: fields.clone(),
                }),
            });
        }

        if let Some(generic_struct) = self.generic_structs.get(base_name) {
            if generic_struct.type_params.len() != type_args.len() {
                return None;
            }

            let mut type_map: HashMap<String, TypeAnnotation> = HashMap::new();
            for (param, arg) in generic_struct.type_params.iter().zip(type_args.iter()) {
                type_map.insert(param.name.clone(), arg.clone());
            }

            let fields: Vec<(String, IRType)> = generic_struct
                .fields
                .iter()
                .map(|field| {
                    let substituted = self.substitute_type(&field.ty, &type_map);
                    let ir_type = self.lower_type_annotation(&substituted);
                    (field.name.clone(), ir_type)
                })
                .collect();

            return Some(IRType::Generic {
                name: base_name.to_string(),
                args: type_args
                    .iter()
                    .map(|arg| self.lower_type_annotation(arg))
                    .collect(),
                representation: Box::new(IRType::Struct {
                    name: mangled,
                    fields,
                }),
            });
        }

        None
    }

    fn resolve_enum_type(&self, base_name: &str, type_args: &[TypeAnnotation]) -> Option<IRType> {
        let mut enum_name = base_name.to_string();
        let variants_data = if type_args.is_empty() {
            self.enum_definitions.get(base_name).cloned()
        } else {
            let type_names: Vec<String> = type_args
                .iter()
                .map(|ty| self.type_annotation_to_string(ty))
                .collect();
            let mangled = format!("{}_{}", base_name, type_names.join("_"));
            enum_name = mangled.clone();

            let mut entry = self.enum_definitions.get(&mangled).cloned();
            if entry.is_none() {
                if let Some(generic_enum) = self.generic_enums.get(base_name) {
                    if generic_enum.type_params.len() != type_args.len() {
                        return None;
                    }

                    let mut type_map: HashMap<String, TypeAnnotation> = HashMap::new();
                    for (param, arg) in generic_enum.type_params.iter().zip(type_args.iter()) {
                        type_map.insert(param.name.clone(), arg.clone());
                    }

                    let computed: Vec<(String, usize, Option<Vec<IRType>>)> = generic_enum
                        .variants
                        .iter()
                        .enumerate()
                        .map(|(tag, variant)| {
                            let data_types = if let Some(types) = variant.data.as_ref() {
                                Some(
                                    types
                                        .iter()
                                        .map(|ty| {
                                            let substituted = self.substitute_type(ty, &type_map);
                                            self.lower_type_annotation(&substituted)
                                        })
                                        .collect::<Vec<_>>(),
                                )
                            } else { variant.struct_data.as_ref().map(|fields| fields
                                        .iter()
                                        .map(|(_, ty)| {
                                            let substituted = self.substitute_type(ty, &type_map);
                                            self.lower_type_annotation(&substituted)
                                        })
                                        .collect::<Vec<_>>()) };
                            (variant.name.clone(), tag, data_types)
                        })
                        .collect();

                    entry = Some(computed);
                }
            }

            entry
        };

        variants_data.map(|variants| {
            let simplified: Vec<(String, Option<Vec<IRType>>)> = variants
                .into_iter()
                .map(|(name, _, data)| (name, data))
                .collect();

            let representation = IRType::Enum {
                name: enum_name,
                variants: simplified,
            };
            if type_args.is_empty() {
                representation
            } else {
                IRType::Generic {
                    name: base_name.to_string(),
                    args: type_args
                        .iter()
                        .map(|arg| self.lower_type_annotation(arg))
                        .collect(),
                    representation: Box::new(representation),
                }
            }
        })
    }

    fn infer_block_result_type(&mut self, block: &Block) -> Option<IRType> {
        // Block-local bindings participate in inference just like they do in
        // lowering. In particular, a closure body may declare another closure
        // and call it in the final expression; skipping `let` statements here
        // turns that valid result into `unknown` and rejects the outer closure.
        self.variable_types.push_scope();
        let mut result: Option<IRType> = None;

        for statement in &block.statements {
            match &statement.kind {
                StatementKind::Let(let_stmt) => {
                    let name = match &let_stmt.pattern {
                        spectra_compiler::ast::Pattern::Identifier(name) => Some(name.clone()),
                        _ => None,
                    };
                    let declared = let_stmt
                        .ty
                        .as_ref()
                        .map(|annotation| self.lower_type_annotation(annotation));
                    let inferred = let_stmt
                        .value
                        .as_ref()
                        .map(|expr| self.infer_expr_ir_type(expr));
                    if let (Some(name), Some(ty)) =
                        (name, declared.or_else(|| inferred.clone()))
                    {
                        self.variable_types.insert(name, ty);
                    }
                }
                StatementKind::Return(ret) => {
                    let ty = ret
                        .value
                        .as_ref()
                        .map(|expr| self.infer_expr_ir_type(expr))
                        .unwrap_or(IRType::Void);
                    result = Some(ty);
                    break;
                }
                StatementKind::Expression(expr) => {
                    result = Some(self.infer_expr_ir_type(expr));
                }
                _ => {}
            }
        }

        self.variable_types.pop_scope();
        result
    }

    fn expected_async_output_type(&self) -> Option<IRType> {
        let annotation = self.current_expected_annotation.as_ref()?;
        match self.lower_type_annotation(annotation) {
            IRType::Task { output } if !Self::ir_type_contains_unknown(&output) => Some(*output),
            _ => None,
        }
    }

    fn unknown_type_annotation() -> TypeAnnotation {
        TypeAnnotation {
            kind: TypeAnnotationKind::Simple {
                segments: vec!["unknown".to_string()],
            },
            span: Span::dummy(),
        }
    }

    fn simple_type_annotation(name: &str) -> TypeAnnotation {
        TypeAnnotation {
            kind: TypeAnnotationKind::Simple {
                segments: vec![name.to_string()],
            },
            span: Span::dummy(),
        }
    }

    fn is_unknown_annotation(type_ann: &TypeAnnotation) -> bool {
        matches!(
            &type_ann.kind,
            TypeAnnotationKind::Simple { segments }
                if segments.len() == 1 && segments[0] == "unknown"
        )
    }

    /// Built-in `Option`/`Result` constructors may leave a type parameter
    /// unobserved (`Option::None`, `Result::Ok(value)`, or `Result::Err(err)`).
    /// Preserve the historical deterministic `int` default for that narrow
    /// compatibility case; every other unresolved generic remains poison and
    /// is rejected before backend code generation.
    fn fill_builtin_enum_defaults(enum_name: &str, args: &mut [TypeAnnotation]) {
        if !matches!(enum_name, "Option" | "Result") {
            return;
        }

        for arg in args {
            if Self::is_unknown_annotation(arg) {
                *arg = Self::simple_type_annotation("int");
            }
        }
    }

    fn ir_type_to_annotation(&self, ir_type: &IRType) -> TypeAnnotation {
        match ir_type {
            IRType::Int => Self::simple_type_annotation("int"),
            IRType::Float => Self::simple_type_annotation("float"),
            IRType::Bool => Self::simple_type_annotation("bool"),
            IRType::String => Self::simple_type_annotation("string"),
            IRType::Char => Self::simple_type_annotation("char"),
            IRType::Struct { name, .. } => self
                .specialized_generic_annotation(name)
                .unwrap_or_else(|| Self::simple_type_annotation(name)),
            IRType::Enum { name, variants } => self
                .specialized_generic_annotation_from_enum(name, variants)
                .or_else(|| self.specialized_generic_annotation(name))
                .unwrap_or_else(|| Self::simple_type_annotation(name)),
            IRType::Array { element_type, .. } => {
                // Represent arrays by their element type (best effort)
                self.ir_type_to_annotation(element_type.as_ref())
            }
            IRType::Tuple { elements } => TypeAnnotation {
                kind: TypeAnnotationKind::Tuple {
                    elements: elements
                        .iter()
                        .map(|elem| self.ir_type_to_annotation(elem))
                        .collect(),
                },
                span: Span::dummy(),
            },
            IRType::Pointer(inner) => self.ir_type_to_annotation(inner.as_ref()),
            IRType::Void => Self::simple_type_annotation("void"),
            _ => Self::unknown_type_annotation(),
        }
    }

}
