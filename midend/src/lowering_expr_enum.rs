use super::*;

impl ASTLowering {
    pub(crate) fn lower_expression_enum(&mut self, expr: &Expression, ir_func: &mut IRFunction) -> Value {
        match &expr.kind {
            ExpressionKind::EnumVariant {
                module_path: _,
                enum_name,
                type_args,
                variant_name,
                data,
                struct_data,
            } => {
                // R-212: UFCS `Trait::method(obj, args)` — parsed as EnumVariant.
                // The first argument is the receiver; dispatch statically to the
                // impl method `Type_method` or dynamically through the vtable.
                if self.trait_method_order.contains_key(enum_name.as_str()) {
                    let mut call_args: Vec<Value> = Vec::new();
                    if let Some(data_exprs) = data {
                        for arg in data_exprs.iter() {
                            call_args.push(self.lower_expression(arg, ir_func));
                        }
                    } else if let Some(named_fields) = struct_data {
                        for (_, val_expr) in named_fields.iter() {
                            call_args.push(self.lower_expression(val_expr, ir_func));
                        }
                    }
                    if call_args.is_empty() {
                        return self.invalid_value("UFCS call requires a receiver argument");
                    }
                    let receiver = call_args.remove(0);
                    let receiver_ty = if let Some(first) = data.as_ref().and_then(|d| d.first()) {
                        self.infer_expr_ir_type(first)
                    } else {
                        IRType::Int
                    };
                    if let IRType::DynTrait { trait_name, .. } = &receiver_ty {
                        // The receiver is the first UFCS argument; the
                        // remaining arguments must be forwarded to the
                        // dynamic signature as well (R-218/R-262).
                        let dyn_arguments: Vec<Expression> = data
                            .as_ref()
                            .map(|expressions| expressions.iter().skip(1).cloned().collect())
                            .unwrap_or_default();
                        return self.lower_dyn_method_call(
                            receiver,
                            trait_name.clone(),
                            variant_name,
                            &dyn_arguments,
                            ir_func,
                        );
                    }
                    let struct_name = match &receiver_ty {
                        IRType::Struct { name, .. } => name.clone(),
                        _ => {
                            self.error(format!(
                                "UFCS call '{}::{}' receiver must be a concrete type or dyn",
                                enum_name, variant_name
                            ));
                            return self.invalid_value(format!(
                                "UFCS call '{}::{}' receiver has no concrete type",
                                enum_name, variant_name
                            ));
                        }
                    };
                    let function_name = format!("{}_{}", struct_name, variant_name);
                    let mut args = vec![receiver];
                    args.extend(call_args);
                    return self.require_value(
                        self.builder.build_call(ir_func, function_name, args, true),
                        "UFCS call did not produce its declared result",
                    );
                }

                // Handle `StructType::static_method(args)` — parsed as EnumVariant by the
                // parser but is actually a static/associated function call.
                if self.struct_definitions.contains_key(enum_name.as_str())
                    || self.generic_structs.contains_key(enum_name.as_str())
                {
                    if variant_name == "json_error_field"
                        && self.json_struct_schemas.contains_key(enum_name.as_str())
                    {
                        let mut call_args: Vec<Value> = Vec::new();
                        if let Some(data_exprs) = data {
                            for arg in data_exprs.iter() {
                                call_args.push(self.lower_expression(arg, ir_func));
                            }
                        }
                        if call_args.len() != 1 {
                            return self.invalid_value(format!(
                                "{enum_name}::json_error_field expects a single JSON string argument"
                            ));
                        }
                        return self.lower_derive_error_field(enum_name, call_args[0], ir_func);
                    }
                    if variant_name == "from_json"
                        && self.json_struct_schemas.contains_key(enum_name.as_str())
                    {
                        let mut call_args: Vec<Value> = Vec::new();
                        if let Some(data_exprs) = data {
                            for arg in data_exprs.iter() {
                                call_args.push(self.lower_expression(arg, ir_func));
                            }
                        }
                        if call_args.len() != 1 {
                            return self.invalid_value(format!(
                                "{enum_name}::from_json expects a single JSON string argument"
                            ));
                        }
                        return self.lower_derive_from_json(enum_name, call_args[0], ir_func);
                    }
                    let function_name = format!("{}_{}", enum_name, variant_name);
                    let mut call_args: Vec<Value> = Vec::new();
                    if let Some(data_exprs) = data {
                        for arg in data_exprs.iter() {
                            call_args.push(self.lower_expression(arg, ir_func));
                        }
                    } else if let Some(named_fields) = struct_data {
                        for (_, val_expr) in named_fields.iter() {
                            call_args.push(self.lower_expression(val_expr, ir_func));
                        }
                    }
                    return self.require_value(
                        self.builder.build_call(ir_func, function_name, call_args, true),
                        "associated function call did not produce its declared result",
                    );
                }

                // Handle qualified-path function calls: module::function(args)
                // The parser can't distinguish these from EnumVariant, so we detect
                // them here when enum_name is not a known local type.
                let is_known_type = self.enum_definitions.contains_key(enum_name.as_str())
                    || self.generic_enums.contains_key(enum_name.as_str());
                let looks_like_call = data.is_some() || struct_data.is_some();
                if !is_known_type && looks_like_call {
                    let callee = variant_name.clone();
                    if self.function_return_types.contains_key(&callee)
                        || self.generic_functions.contains_key(&callee)
                    {
                        let mut call_args: Vec<Value> = Vec::new();
                        if let Some(data_exprs) = data {
                            for arg in data_exprs.iter() {
                                call_args.push(self.lower_expression(arg, ir_func));
                            }
                        } else if let Some(named_fields) = struct_data {
                            for (_, val_expr) in named_fields.iter() {
                                call_args.push(self.lower_expression(val_expr, ir_func));
                            }
                        }
                        let final_name = if self.generic_functions.contains_key(&callee) {
                            let concrete_types =
                                self.infer_argument_types(data.as_deref().unwrap_or(&[]));
                            let request = MonomorphizationRequest {
                                generic_name: callee.clone(),
                                concrete_types,
                            };
                            let mangled = request.mangled_name();
                            if !self.generated_specializations.contains_key(&mangled) {
                                self.pending_specializations.push(request);
                            }
                            mangled
                        } else {
                            callee
                        };
                        return self.require_value(
                            self.builder.build_call(ir_func, final_name, call_args, true),
                            "qualified function call did not produce its declared result",
                        );
                    }
                }

                let needs_refinement = type_args.is_empty()
                    || type_args
                        .iter()
                        .any(|ann| self.type_annotation_needs_refinement(ann));

                let inferred_args = if needs_refinement {
                    if let Some(data_exprs) = data {
                        self.infer_enum_type_args_from_data(enum_name, variant_name, data_exprs)
                            .or_else(|| self.default_type_args_for_enum(enum_name))
                    } else if let Some(named_fields) = struct_data {
                        self.infer_enum_type_args_from_named_fields(
                            enum_name,
                            variant_name,
                            named_fields,
                        )
                        .or_else(|| self.default_type_args_for_enum(enum_name))
                    } else {
                        self.default_type_args_for_enum(enum_name)
                    }
                } else {
                    None
                };

                let mut final_args: Vec<TypeAnnotation> = if let Some(mut args) = inferred_args {
                    // If some args are still "unknown", try to fill them from the function's
                    // declared return type annotation (e.g., fn -> Result<int, string> means
                    // both Result::Ok and Result::Err should use the same specialization).
                    if args
                        .iter()
                        .any(|a| self.type_annotation_needs_refinement(a))
                    {
                        let contextual_annotations = [
                            self.current_expected_annotation.clone(),
                            self.current_function_return_annotation.clone(),
                        ];
                        for context_ann in contextual_annotations.into_iter().flatten() {
                            if let TypeAnnotationKind::Generic {
                                name: context_name,
                                type_args: context_args,
                            } = &context_ann.kind
                            {
                                if context_name == enum_name && context_args.len() == args.len() {
                                    for (arg, context_arg) in
                                        args.iter_mut().zip(context_args.iter())
                                    {
                                        if self.type_annotation_needs_refinement(arg) {
                                            *arg = context_arg.clone();
                                        }
                                    }
                                }
                            }
                            if !args
                                .iter()
                                .any(|a| self.type_annotation_needs_refinement(a))
                            {
                                break;
                            }
                        }
                    }
                    args
                } else {
                    // Check typed expression contexts as the primary source of type args.
                    let contextual_annotations = [
                        self.current_expected_annotation.clone(),
                        self.current_function_return_annotation.clone(),
                    ];
                    let mut contextual_args = None;
                    for context_ann in contextual_annotations.into_iter().flatten() {
                        if let TypeAnnotationKind::Generic {
                            name: context_name,
                            type_args: context_args,
                        } = &context_ann.kind
                        {
                            if context_name == enum_name {
                                contextual_args = Some(context_args.clone());
                                break;
                            }
                        }
                    }
                    contextual_args.unwrap_or_else(|| type_args.clone())
                };

                Self::fill_builtin_enum_defaults(enum_name, &mut final_args);

                let (resolved_enum_name, variants) =
                    self.ensure_enum_definition(enum_name, final_args.as_slice());

                let data_values: Vec<Value> = if let Some(data_exprs) = data {
                    data_exprs
                        .iter()
                        .map(|expr| self.lower_expression(expr, ir_func))
                        .collect()
                } else if let Some(named_fields) = struct_data {
                    self.reorder_named_variant_exprs(
                        &resolved_enum_name,
                        variant_name,
                        named_fields,
                    )
                    .unwrap_or_default()
                    .into_iter()
                    .map(|expr| self.lower_expression(expr, ir_func))
                    .collect()
                } else {
                    Vec::new()
                };

                if !variants.is_empty() {
                    // Encontrar o variant
                    if let Some((_, tag, variant_data_types)) =
                        variants.iter().find(|(name, _, _)| name == variant_name)
                    {
                        // Se é unit variant, alocar um slot de 8 bytes só com o tag
                        // (mesma representação por ponteiro dos variants com dados)
                        if variant_data_types.is_none() {
                            let tag_val = self.builder.build_const_int(ir_func, *tag as i64);
                            if enum_name == "ErrorCode" {
                                // ErrorCode crosses the host ABI as a closed scalar
                                // tag.  Do not pass a pointer to a temporary alloca
                                // into std.error.new.
                                return tag_val;
                            }
                            let tag_alloc = self.builder.build_alloca(
                                ir_func,
                                IRType::Tuple {
                                    elements: vec![IRType::Int],
                                },
                            );
                            let zero = self.builder.build_const_int(ir_func, 0);
                            let tag_slot = self.builder.build_getelementptr(
                                ir_func,
                                tag_alloc,
                                zero,
                                IRType::Int,
                            );
                            self.builder.build_store(ir_func, tag_slot, tag_val);
                            return tag_alloc;
                        }

                        // Se é tuple variant, criar tupla (tag, data...)
                        if let Some(data_types) = variant_data_types {
                            let data_types: Vec<IRType> = data_types
                                .iter()
                                .enumerate()
                                .map(|(idx, ty)| {
                                    if matches!(ty, IRType::Void) {
                                        data.as_ref()
                                            .and_then(|exprs| exprs.get(idx))
                                            .map(|expr| self.infer_expr_ir_type(expr))
                                            .unwrap_or(IRType::Unknown)
                                    } else {
                                        ty.clone()
                                    }
                                })
                                .collect();
                            let mut elements = Vec::new();

                            // Primeiro elemento: tag
                            elements.push(self.builder.build_const_int(ir_func, *tag as i64));

                            // Demais elementos: dados do variant
                            for value in &data_values {
                                elements.push(*value);
                            }

                            // Criar tipos da tupla
                            let mut element_types = vec![IRType::Int];
                            element_types.extend(data_types);

                            let tuple_type = IRType::Tuple {
                                elements: element_types.clone(),
                            };

                            // Alocar tupla no stack
                            let tuple_ptr = self.builder.build_alloca(ir_func, tuple_type.clone());

                            // Store cada elemento com offsets padded (tag + dados)
                            let variant_layout = layout::layout_of(&element_types);
                            for (idx, elem_value) in elements.iter().enumerate() {
                                let elem_ptr = self.builder.build_field_ptr(
                                    ir_func,
                                    tuple_ptr,
                                    variant_layout.offsets[idx] as i64,
                                );
                                self.builder.build_store(ir_func, elem_ptr, *elem_value);
                            }

                            return tuple_ptr;
                        }

                        // Variant com dados mas sem argumentos fornecidos - erro
                        return self.invalid_value(format!(
                            "enum variant '{}' requires payload arguments",
                            variant_name
                        ));
                    }
                }

                // Enum ou variant não encontrado
                self.invalid_value(format!(
                    "unresolved enum variant '{}::{}' during lowering",
                    enum_name, variant_name
                ))
            }
            _ => unreachable!("lowering expression category mismatch"),
        }
    }
}
