use super::*;

impl ASTLowering {
    /// Lower a `expr as TargetType` cast expression.
    pub(crate) fn lower_cast_expression(
        &mut self,
        inner: &Expression,
        target_type: &TypeAnnotation,
        ir_func: &mut IRFunction,
    ) -> Value {
        let from_ty = self.infer_expr_ir_type(inner);
        let to_ty = self.lower_type_annotation(target_type);

        // Special case: coerce struct to dyn Trait
        if let IRType::DynTrait { trait_name, .. } = &to_ty.clone() {
            let operand = self.lower_expression(inner, ir_func);
            return self.lower_coerce_to_dyn(operand, &from_ty, trait_name, ir_func);
        }

        if let Some(value) = self.eval_const_expression(inner) {
            if let Some(casted) = self.cast_const_value(value, &to_ty) {
                return self.emit_const_value(&casted, ir_func);
            }
        }

        let operand = self.lower_expression(inner, ir_func);

        // If same type, just copy
        if from_ty == to_ty {
            return operand;
        }

        // Checked narrowing goes through a runtime validator so dynamic values
        // cannot silently saturate or truncate.  The host ABI receives the
        // canonical i64/f64 representation; the result is materialized back
        // into the exact destination type by the backend.
        if let IRType::ExactInt { signed, width } = &to_ty {
            let bits = match width {
                IRIntWidth::I8 => 8,
                IRIntWidth::I16 => 16,
                IRIntWidth::I32 => 32,
                IRIntWidth::I64 | IRIntWidth::Isize | IRIntWidth::Usize => 64,
            };
            let source_is_float = matches!(from_ty, IRType::Float | IRType::ExactFloat { .. });
            let compatible_source = matches!(from_ty, IRType::Int | IRType::ExactInt { .. } | IRType::Float | IRType::ExactFloat { .. });
            if compatible_source {
                let host_operand = if source_is_float {
                    if matches!(from_ty, IRType::ExactFloat { .. }) {
                        self.builder.build_cast(ir_func, operand, from_ty.clone(), IRType::Float)
                    } else { operand }
                } else if matches!(from_ty, IRType::ExactInt { .. }) {
                    self.builder.build_cast(ir_func, operand, from_ty.clone(), IRType::Int)
                } else { operand };
                let host = if source_is_float {
                    format!("spectra.std.numeric.checked_float_{}{}", if *signed { "i" } else { "u" }, bits)
                } else {
                    format!("spectra.std.numeric.checked_{}{}", if *signed { "i" } else { "u" }, bits)
                };
                if let Some(value) = self.builder.build_typed_host_call(ir_func, host, vec![host_operand], to_ty.clone(), true) {
                    return value;
                }
            }
        }

        if matches!(to_ty, IRType::ExactFloat { width: IRFloatWidth::F32 })
            && matches!(from_ty, IRType::Float | IRType::ExactFloat { width: IRFloatWidth::F64 })
        {
            let host_operand = if matches!(from_ty, IRType::ExactFloat { .. }) {
                self.builder.build_cast(ir_func, operand, from_ty.clone(), IRType::Float)
            } else { operand };
            if let Some(value) = self.builder.build_typed_host_call(
                ir_func,
                "spectra.std.numeric.checked_f32".to_string(),
                vec![host_operand],
                to_ty.clone(),
                true,
            ) {
                return value;
            }
        }

        self.builder.build_cast(ir_func, operand, from_ty, to_ty)
    }

    /// Dispatch a method call via vtable for `dyn Trait` objects.
    /// The fat_ptr contains (data_ptr, vtable_ptr); we look up the method slot
    /// and emit a `CallIndirect`.
    pub(crate) fn lower_dyn_method_call(
        &mut self,
        fat_ptr: Value,
        trait_name: String,
        method_name: &str,
        arguments: &[Expression],
        ir_func: &mut IRFunction,
    ) -> Value {
        // Extract data_ptr and vtable_ptr from fat pointer
        let data_ptr = self.builder.build_load_dyn_data_ptr(ir_func, fat_ptr);
        let vtable_ptr = self.builder.build_load_dyn_vtable_ptr(ir_func, fat_ptr);

        // Resolve the signature first: a missing or stale method entry must
        // fail loudly instead of dispatching a guessed slot.
        let Some((sig_params, sig_return)) = self
            .trait_method_signatures
            .get(&trait_name)
            .and_then(|methods| methods.get(method_name))
            .map(|(params, return_type)| {
                let mut signature_params = Vec::with_capacity(params.len() + 1);
                signature_params.push(IRType::Int); // lowered receiver pointer
                signature_params.extend(params.iter().cloned());
                (signature_params, return_type.clone())
            })
        else {
            return self.invalid_value(format!(
                "unresolved trait method '{trait_name}::{method_name}' during lowering"
            ));
        };

        // Determine slot index by looking up the trait's method order. The
        // order entry is resolved only after the signature above: a method
        // present in one map but missing here previously dispatched slot 0.
        let Some(slot_index) = self
            .trait_method_order
            .get(&trait_name)
            .and_then(|methods| methods.iter().position(|m| m == method_name))
        else {
            return self.invalid_value(format!(
                "unresolved trait method order for '{trait_name}::{method_name}' during lowering"
            ));
        };

        // Load function pointer from vtable
        let fn_ptr = self
            .builder
            .build_load_vtable_slot(ir_func, vtable_ptr, slot_index);

        // Build argument list: data_ptr first, then the other args
        let mut call_args = vec![data_ptr];
        for arg in arguments {
            call_args.push(self.lower_expression(arg, ir_func));
        }

        self.require_value(
            self.builder
                .build_call_indirect(ir_func, fn_ptr, call_args, sig_params, sig_return),
            "dynamic trait method call did not produce its declared result",
        )
    }

    /// Build a fat pointer (data_ptr, vtable_ptr) for coercing a concrete struct to `dyn Trait`.
    pub(crate) fn lower_coerce_to_dyn(
        &mut self,
        data_ptr: Value,
        from_ty: &IRType,
        trait_name: &str,
        ir_func: &mut IRFunction,
    ) -> Value {
        let vtable_ptr = if let IRType::Struct { name, .. } = from_ty {
            let methods = self
                .trait_method_order
                .get(trait_name)
                .cloned()
                .unwrap_or_default();

            // R-210: vtables live on the runtime manual heap and are escaped to
            // the base frame so `dyn` values outlive the creating scope.
            let slot_count = methods.len().max(1) as i64;
            let vtable_storage = self
                .builder
                .build_manual_alloc(ir_func, slot_count * 8);

            for (slot, method_name) in methods.iter().enumerate() {
                let fn_name = format!("{}_{}", name, method_name);
                let fn_addr = self.builder.build_func_addr(ir_func, fn_name);
                let slot_index = self.builder.build_const_int(ir_func, slot as i64);
                let slot_ptr = self.builder.build_getelementptr(
                    ir_func,
                    vtable_storage,
                    slot_index,
                    IRType::Int,
                );
                self.builder.build_store(ir_func, slot_ptr, fn_addr);
            }

            // The concrete receiver can escape the current function through
            // the dyn object.  The backend therefore lowers this alloca to
            // the manual heap; explicitly promote it to the base frame along
            // with the vtable so returned/stored dyn values remain valid.
            self.builder.build_escape_manual_alloc(ir_func, data_ptr);
            self.builder.build_escape_manual_alloc(ir_func, vtable_storage);
            vtable_storage
        } else {
            self.builder.build_const_int(ir_func, 0)
        };
        self.builder
            .build_make_dyn_fat_ptr(ir_func, data_ptr, vtable_ptr)
    }

    pub(crate) fn lower_string_literal(&mut self, literal: &str, ir_func: &mut IRFunction) -> Value {
        // R-3126: emit a single ConstString instruction. The backend resolves
        // this to a stable pointer (global .rodata section in AOT, heap
        // buffer in JIT) and tracks the compile-time length for fast
        // `str.len` / `str.char_at` interception.
        self.builder
            .build_const_string(ir_func, literal.to_string())
    }

    pub(crate) fn lower_pattern_check(
        &mut self,
        pattern: &spectra_compiler::ast::Pattern,
        scrutinee: Value,
        scrutinee_enum: Option<&str>,
        scrutinee_type: Option<&IRType>,
        ir_func: &mut IRFunction,
    ) -> Value {
        use spectra_compiler::ast::Pattern;

        match pattern {
            Pattern::Wildcard(_) => {
                // Wildcard sempre match
                self.builder.build_const_int(ir_func, 1)
            }
            Pattern::Identifier(_name, _) => {
                // Binding sempre match
                self.builder.build_const_int(ir_func, 1)
            }
            Pattern::Literal(expr) => {
                // Comparar scrutinee com o valor literal
                let literal_type = self.infer_expr_ir_type(expr);
                let literal_value = self.lower_expression(expr, ir_func);
                self.lower_value_equality(
                    scrutinee,
                    literal_value,
                    scrutinee_type.unwrap_or(&literal_type),
                    &literal_type,
                    false,
                    ir_func,
                )
            }
            Pattern::Tuple(elements) => {
                if let Some(IRType::Tuple {
                    elements: tuple_types,
                }) = scrutinee_type
                {
                    let mut result = self.builder.build_const_int(ir_func, 1);
                    let tuple_layout = layout::layout_of(tuple_types.iter());
                    for (idx, pattern) in elements.iter().enumerate() {
                        if let Some(field_ty) = tuple_types.get(idx) {
                            let field_ptr = self.builder.build_field_ptr(
                                ir_func,
                                scrutinee,
                                tuple_layout.offsets[idx] as i64,
                            );
                            let field_value =
                                self.builder
                                    .build_load_typed(ir_func, field_ptr, field_ty.clone());
                            let sub_match = self.lower_pattern_check(
                                pattern,
                                field_value,
                                None,
                                Some(field_ty),
                                ir_func,
                            );
                            result = self.builder.build_and(ir_func, result, sub_match);
                        }
                    }
                    result
                } else {
                    self.builder.build_const_int(ir_func, 0)
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
                    let mut result = self.builder.build_const_int(ir_func, 1);
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
                            let sub_match = self.lower_pattern_check(
                                pattern,
                                field_value,
                                None,
                                Some(field_ty),
                                ir_func,
                            );
                            result = self.builder.build_and(ir_func, result, sub_match);
                        }
                    }
                    result
                } else {
                    self.builder.build_const_int(ir_func, 0)
                }
            }
            Pattern::EnumVariant {
                enum_name,
                type_args,
                variant_name,
                data: _,
                struct_data: _,
                ..
            } => {
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
                    if let Some((_, expected_tag, variant_types)) =
                        variants.iter().find(|(name, _, _)| name == variant_name)
                    {
                        // Para qualquer variant (unit ou com dados), extrair tag do ponteiro
                        let zero_index = self.builder.build_const_int(ir_func, 0);
                        let tag_ptr = self.builder.build_getelementptr(
                            ir_func,
                            scrutinee,
                            zero_index,
                            IRType::Int,
                        );
                        let tag_value = self.builder.build_load(ir_func, tag_ptr);
                        let expected_tag_value =
                            self.builder.build_const_int(ir_func, *expected_tag as i64);
                        let _ = variant_types; // mantido para futuros guards
                        return self
                            .builder
                            .build_eq(ir_func, tag_value, expected_tag_value);
                    }
                }
                // Fallback: sempre false
                self.builder.build_const_int(ir_func, 0)
            }
            Pattern::Or(patterns) => {
                let mut branches = patterns.iter();
                if let Some(first) = branches.next() {
                    let mut result = self.lower_pattern_check(
                        first,
                        scrutinee,
                        scrutinee_enum,
                        scrutinee_type,
                        ir_func,
                    );
                    for branch in branches {
                        let next = self.lower_pattern_check(
                            branch,
                            scrutinee,
                            scrutinee_enum,
                            scrutinee_type,
                            ir_func,
                        );
                        result = self.builder.build_or(ir_func, result, next);
                    }
                    result
                } else {
                    self.builder.build_const_int(ir_func, 0)
                }
            }
        }
    }

}
