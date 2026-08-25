use super::*;

impl ASTLowering {
    pub(crate) fn lower_range_expression(
        &mut self,
        start: &Expression,
        end: &Expression,
        inclusive: bool,
        ir_func: &mut IRFunction,
    ) -> (Value, RangeInfo) {
        let start_val = self.lower_expression(start, ir_func);
        let end_val = self.lower_expression(end, ir_func);
        let inclusive_val = self.builder.build_const_int(ir_func, inclusive as i64);
        let handle = self.require_value(
            self.builder.build_typed_host_call(
                ir_func,
                "spectra.std.range.create".to_string(),
                vec![start_val, end_val, inclusive_val],
                IRType::Range,
                true,
            ),
            "range.create host call did not produce its declared result",
        );
        (
            handle,
            RangeInfo {
                start: start_val,
                end: end_val,
                inclusive,
            },
        )
    }

    pub(crate) fn lower_iterator_for_loop(
        &mut self,
        for_stmt: &spectra_compiler::ast::ForLoop,
        iterator_value: Value,
        element_type: IRType,
        owns_iterator: bool,
        ir_func: &mut IRFunction,
    ) {
        if Self::ir_type_contains_unknown(&element_type) {
            self.error("for-loop iterator has an unresolved element type");
            return;
        }

        let option_type = IRType::Generic {
            name: "Option".to_string(),
            args: vec![element_type.clone()],
            representation: Box::new(IRType::Enum {
                name: format!("Option_{}", self.ir_type_to_ast_name(&element_type)),
                variants: vec![
                    ("Some".to_string(), Some(vec![element_type.clone()])),
                    ("None".to_string(), None),
                ],
            }),
        };
        let iterator_header = ir_func.add_block("iterator.header");
        let iterator_body = ir_func.add_block("iterator.body");
        let iterator_exit = ir_func.add_block("iterator.exit");

        self.builder.build_branch(ir_func, iterator_header);
        self.builder.set_current_block(iterator_header);
        let remaining = self.require_value(
            self.builder.build_typed_host_call(
                ir_func,
                "spectra.std.collections.iterator_remaining".to_string(),
                vec![iterator_value],
                IRType::Int,
                true,
            ),
            "iterator.remaining host call did not produce its declared result",
        );
        let zero = self.builder.build_const_int(ir_func, 0);
        let has_next = self.builder.build_gt(ir_func, remaining, zero);
        self.builder
            .build_cond_branch(ir_func, has_next, iterator_body, iterator_exit);

        self.builder.set_current_block(iterator_body);
        self.loop_stack.push(LoopContext {
            header_block: iterator_header,
            exit_block: iterator_exit,
        });
        self.value_map.push_scope();
        self.variable_types.push_scope();
        self.array_map.push_scope();
        self.range_map.push_scope();
        self.struct_var_map.push_scope();

        let option_value = self.require_value(
            self.builder.build_typed_host_call(
                ir_func,
                "spectra.std.collections.iterator_next".to_string(),
                vec![iterator_value],
                option_type,
                true,
            ),
            "iterator.next host call did not produce its declared result",
        );
        let element_value = self.require_value(
            self.builder.build_typed_host_call(
                ir_func,
                "spectra.std.option.option_unwrap".to_string(),
                vec![option_value],
                element_type.clone(),
                true,
            ),
            "iterator option unwrap did not produce its declared element",
        );
        self.value_map
            .insert(for_stmt.iterator.clone(), element_value);
        self.variable_types
            .insert(for_stmt.iterator.clone(), element_type.clone());
        if let Some(name) = self.ir_nominal_name(&element_type) {
            self.struct_var_map
                .insert(for_stmt.iterator.clone(), (element_value, name.to_string()));
        }

        self.lower_block_with_scope(&for_stmt.body.statements, ir_func, false);
        if let Some(current_block) = self.builder.get_current_block() {
            if let Some(block) = ir_func.get_block_mut(current_block) {
                if block.terminator.is_none() {
                    self.builder.build_branch(ir_func, iterator_header);
                }
            }
        }

        self.struct_var_map.pop_scope();
        self.range_map.pop_scope();
        self.array_map.pop_scope();
        self.variable_types.pop_scope();
        self.value_map.pop_scope();
        self.loop_stack.pop();
        self.builder.set_current_block(iterator_exit);
        if owns_iterator {
            let _ = self.builder.build_host_call(
                ir_func,
                "spectra.std.collections.iterator_free".to_string(),
                vec![iterator_value],
                false,
            );
        }
    }

    /// True when `value` is one of the current function's parameter values.
    ///
    /// Parameter values occupy the ids `[0, params.len())` and instruction
    /// results start at `params.len()`, so an exact id match identifies a
    /// parameter. Used to keep unsized `[T]` array parameters (whose recorded
    /// size is the annotation placeholder 0) out of the statically-empty
    /// zero-trip path: their true length lives with the caller.
    pub(crate) fn binding_is_parameter(&self, value: &Value) -> bool {
        self.current_function
            .as_ref()
            .is_some_and(|function| function.params.len() > value.id)
    }

    /// Lowers `for element in fixed_array` as an index loop over the array
    /// storage: init index, condition `index < len`, and one GEP+Load per trip.
    /// Unlike the iterator path this issues zero host calls per iteration and
    /// never materializes the elements up front. `continue` targets the latch
    /// block so the index still advances before the next condition check.
    #[allow(clippy::too_many_statements)]

    pub(crate) fn lower_array_index_for_loop(
        &mut self,
        for_stmt: &spectra_compiler::ast::ForLoop,
        array_ptr: Value,
        element_type: IRType,
        size: usize,
        ir_func: &mut IRFunction,
    ) {
        let length_value = self.builder.build_const_int(ir_func, size as i64);
        let index_ptr = self.builder.build_alloca(ir_func, IRType::Int);
        let zero = self.builder.build_const_int(ir_func, 0);
        self.builder.build_store(ir_func, index_ptr, zero);

        let cond_block = ir_func.add_block("array.cond");
        let body_block = ir_func.add_block("array.body");
        let latch_block = ir_func.add_block("array.latch");
        let exit_block = ir_func.add_block("array.exit");

        self.builder.build_branch(ir_func, cond_block);
        self.builder.set_current_block(cond_block);
        let index_value = self
            .builder
            .build_load_typed(ir_func, index_ptr, IRType::Int);
        let has_next = self.builder.build_lt(ir_func, index_value, length_value);
        self.builder
            .build_cond_branch(ir_func, has_next, body_block, exit_block);

        self.builder.set_current_block(body_block);
        self.loop_stack.push(LoopContext {
            header_block: latch_block,
            exit_block,
        });
        self.value_map.push_scope();
        self.variable_types.push_scope();
        self.array_map.push_scope();
        self.range_map.push_scope();
        self.struct_var_map.push_scope();

        let element_ptr = self.builder.build_getelementptr(
            ir_func,
            array_ptr,
            index_value,
            element_type.clone(),
        );
        let element_value =
            self.builder
                .build_load_typed(ir_func, element_ptr, element_type.clone());
        self.value_map.insert(for_stmt.iterator.clone(), element_value);
        self.variable_types
            .insert(for_stmt.iterator.clone(), element_type.clone());
        if let Some(name) = self.ir_nominal_name(&element_type) {
            self.struct_var_map
                .insert(for_stmt.iterator.clone(), (element_value, name.to_string()));
        }

        self.lower_block_with_scope(&for_stmt.body.statements, ir_func, false);
        if let Some(current_block) = self.builder.get_current_block() {
            if let Some(block) = ir_func.get_block_mut(current_block) {
                if block.terminator.is_none() {
                    self.builder.build_branch(ir_func, latch_block);
                }
            }
        }

        self.struct_var_map.pop_scope();
        self.range_map.pop_scope();
        self.array_map.pop_scope();
        self.variable_types.pop_scope();
        self.value_map.pop_scope();
        self.loop_stack.pop();

        self.builder.set_current_block(latch_block);
        let current_index = self
            .builder
            .build_load_typed(ir_func, index_ptr, IRType::Int);
        let one = self.builder.build_const_int(ir_func, 1);
        let next_index = self.builder.build_add(ir_func, current_index, one);
        self.builder.build_store(ir_func, index_ptr, next_index);
        self.builder.build_branch(ir_func, cond_block);

        self.builder.set_current_block(exit_block);
    }

    pub(crate) fn lower_for_loop_via_iterator(
        &mut self,
        for_stmt: &spectra_compiler::ast::ForLoop,
        ir_func: &mut IRFunction,
    ) {
        // Statically empty arrays execute the body zero times. Direct `[]`
        // literals exit before expression lowering because an unannotated
        // `[]` has no inferrable element type; let-bound empties (`let xs:
        // array<int> = []`) resolve through the array sidecar, which records
        // the concrete literal length used by identifier inference.
        match &for_stmt.iterable.kind {
            ExpressionKind::ArrayLiteral { elements } if elements.is_empty() => {
                return;
            }
            ExpressionKind::Identifier(name) => {
                if let Some(info) = self.array_map.get(name) {
                    if info.size == 0 && !self.binding_is_parameter(&info.ptr) {
                        return;
                    }
                }
            }
            _ => {}
        }
        let iterable_value = self.lower_expression(&for_stmt.iterable, ir_func);
        let iterable_type = self.infer_expr_ir_type(&for_stmt.iterable);
        let mangle_type = |part: &str| -> IRType {
            match part {
                "int" => IRType::Int,
                "float" => IRType::Float,
                "bool" => IRType::Bool,
                "string" => IRType::String,
                "char" => IRType::Char,
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
                other => IRType::Struct {
                    name: other.to_string(),
                    fields: Vec::new(),
                },
            }
        };

        let iterator_type = |element_type: &IRType| IRType::Generic {
            name: "Iterator".to_string(),
            args: vec![element_type.clone()],
            representation: Box::new(IRType::Struct {
                name: format!("Iterator_{}", self.ir_type_to_ast_name(element_type)),
                fields: Vec::new(),
            }),
        };

        let (iterator_value, element_type, owns_iterator) = match iterable_type {
            IRType::Range => {
                (
                    self.require_value(
                    self.builder.build_typed_host_call(
                        ir_func,
                        "spectra.std.range.iter".to_string(),
                        vec![iterable_value],
                        iterator_type(&IRType::Int),
                        true,
                    ),
                    "range.iter host call did not produce its declared iterator",
                    ),
                    IRType::Int,
                    true,
                )
            }
            IRType::Array { element_type, size } => {
                let element_type = *element_type;
                if size == 0 {
                    // Fixed arrays carry no runtime length word, so an array
                    // whose length is not statically known cannot be iterated.
                    // (Statically empty literals/bindings already exited at the
                    // top of this function.) Use `List<T>` for dynamically
                    // sized sequences.
                    self.error(
                        "cannot iterate an array without a static length: fixed arrays carry no \
                         runtime length, use List<T> for dynamically sized sequences",
                    );
                    return;
                }
                // Direct index loop instead of materializing every element up
                // front through `iterator_from_values`: no host calls are issued
                // per iteration, and the element is loaded once per trip.
                self.lower_array_index_for_loop(for_stmt, iterable_value, element_type, size, ir_func);
                return;
            }
            IRType::Generic { name, args, .. } if name == "List" => {
                let Some(element_type) = args.first().cloned() else {
                    self.error("cannot iterate List<T> without its element type");
                    return;
                };
                (
                    self.require_value(
                        self.builder.build_typed_host_call(
                            ir_func,
                            "spectra.std.collections.list_iter".to_string(),
                            vec![iterable_value],
                            iterator_type(&element_type),
                            true,
                        ),
                        "list.iter host call did not produce its declared iterator",
                    ),
                    element_type,
                    true,
                )
            }
            IRType::Generic { name, args, .. } if name == "Set" => {
                let Some(element_type) = args.first().cloned() else {
                    self.error("cannot iterate Set<T> without its element type");
                    return;
                };
                (
                    self.require_value(
                        self.builder.build_typed_host_call(
                            ir_func,
                            "spectra.std.collections.set_iter".to_string(),
                            vec![iterable_value],
                            iterator_type(&element_type),
                            true,
                        ),
                        "set.iter host call did not produce its declared iterator",
                    ),
                    element_type,
                    true,
                )
            }
            IRType::Generic { name, args, .. } if name == "Map" => {
                let Some(element_type) = args.first().cloned() else {
                    self.error("cannot iterate Map<K, V> without its key type");
                    return;
                };
                (
                    self.require_value(
                        self.builder.build_typed_host_call(
                            ir_func,
                            "spectra.std.collections.map_iter".to_string(),
                            vec![iterable_value],
                            iterator_type(&element_type),
                            true,
                        ),
                        "map.iter host call did not produce its declared iterator",
                    ),
                    element_type,
                    true,
                )
            }
            IRType::Generic { name, args, .. } if name == "Iterator" => {
                let Some(element_type) = args.first().cloned() else {
                    self.error("cannot consume Iterator<T> without its element type");
                    return;
                };
                (iterable_value, element_type, false)
            }
            IRType::Struct { name, .. } if name.starts_with("List_") => {
                let suffix = &name["List_".len()..];
                let element_type = mangle_type(suffix);
                (
                    self.require_value(
                    self.builder.build_typed_host_call(
                        ir_func,
                        "spectra.std.collections.list_iter".to_string(),
                        vec![iterable_value],
                        IRType::Struct {
                            name: format!("Iterator_{suffix}"),
                            fields: Vec::new(),
                        },
                        true,
                    ),
                    "list.iter host call did not produce its declared iterator",
                    ),
                    element_type,
                    true,
                )
            }
            IRType::Struct { name, .. } if name.starts_with("Set_") => {
                let suffix = &name["Set_".len()..];
                let element_type = mangle_type(suffix);
                (
                    self.require_value(
                    self.builder.build_typed_host_call(
                        ir_func,
                        "spectra.std.collections.set_iter".to_string(),
                        vec![iterable_value],
                        IRType::Struct {
                            name: format!("Iterator_{suffix}"),
                            fields: Vec::new(),
                        },
                        true,
                    ),
                    "set.iter host call did not produce its declared iterator",
                    ),
                    element_type,
                    true,
                )
            }
            IRType::Struct { name, .. } if name.starts_with("Map_") => {
                let suffix = &name["Map_".len()..];
                let Some((key, _value)) = suffix.split_once('_') else {
                    self.error(format!("cannot resolve map key type for {name}"));
                    return;
                };
                let element_type = mangle_type(key);
                (
                    self.require_value(
                    self.builder.build_typed_host_call(
                        ir_func,
                        "spectra.std.collections.map_iter".to_string(),
                        vec![iterable_value],
                        IRType::Struct {
                            name: format!("Iterator_{key}"),
                            fields: Vec::new(),
                        },
                        true,
                    ),
                    "map.iter host call did not produce its declared iterator",
                    ),
                    element_type,
                    true,
                )
            }
            IRType::Struct { name, .. } if name.starts_with("Iterator_") => {
                let suffix = &name["Iterator_".len()..];
                (iterable_value, mangle_type(suffix), false)
            }
            other => {
                self.error(format!(
                    "for-loop lowering requires an Iterator<T>-compatible source, found {:?}",
                    other
                ));
                return;
            }
        };

        self.lower_iterator_for_loop(
            for_stmt,
            iterator_value,
            element_type,
            owns_iterator,
            ir_func,
        );
    }

}
