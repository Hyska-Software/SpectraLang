use super::*;

impl ASTLowering {
    pub(crate) fn lower_statement(&mut self, stmt: &Statement, ir_func: &mut IRFunction) {
        self.builder
            .set_source_span(Some(self.source_span(stmt.span)));
        match &stmt.kind {
            StatementKind::Let(let_stmt) => {
                let binding_name = match &let_stmt.pattern {
                    spectra_compiler::ast::Pattern::Identifier(name, _) => Some(name.clone()),
                    _ => None,
                };

                let declared_type = let_stmt
                    .ty
                    .as_ref()
                    .map(|type_ann| self.lower_type_annotation(type_ann));

                // Discover variable type either from initializer or annotation.
                // An explicit annotation is the binding contract and must win over
                // partially inferred constructor types like Result::Ok(int, unknown).
                let inferred_type = if let Some(ref value_expr) = let_stmt.value {
                    Some(self.infer_expr_ir_type(value_expr))
                } else {
                    declared_type.clone()
                };
                let binding_type = declared_type.clone().or_else(|| inferred_type.clone());

                if let (Some(name), Some(ty)) = (binding_name.as_ref(), binding_type.as_ref()) {
                    self.variable_types.insert(name.clone(), (*ty).clone());
                }

                if let Some(ref value_expr) = let_stmt.value {
                    // Track the lambda function name BEFORE lowering so closure_var_map
                    // is populated even if the lambda itself modifies lambda_counter.
                    let is_lambda_binding =
                        matches!(&value_expr.kind, ExpressionKind::Lambda { .. });

                    let annotated_tensor_type = let_stmt.ty.as_ref().and_then(|type_ann| {
                        let ty = self.lower_type_annotation(type_ann);
                        matches!(ty, IRType::Tensor { .. }).then_some(ty)
                    });
                    let saved_expected_annotation = self.current_expected_annotation.clone();
                    self.current_expected_annotation = let_stmt.ty.clone();
                    let mut lowered_range_info = None;
                    let value = if let Some(IRType::Tensor { dtype, rank, .. }) =
                        annotated_tensor_type.as_ref()
                    {
                        if let Some(value) =
                            self.lower_tensor_literal(value_expr, dtype, *rank, ir_func)
                        {
                            value
                        } else {
                            self.lower_expression(value_expr, ir_func)
                        }
                    } else if let ExpressionKind::Range {
                        start,
                        end,
                        inclusive,
                    } = &value_expr.kind
                    {
                        let (value, info) =
                            self.lower_range_expression(start, end, *inclusive, ir_func);
                        lowered_range_info = Some(info);
                        value
                    } else {
                        self.lower_expression(value_expr, ir_func)
                    };
                    self.current_expected_annotation = saved_expected_annotation;

                    // Register in closure_var_map when the value bound is a lambda
                    if let Some(name) = binding_name.as_ref().filter(|_| is_lambda_binding) {
                        if let Some(IRType::Function {
                            params,
                            return_type,
                        }) = binding_type.clone()
                        {
                            self.closure_var_map.insert(
                                name.clone(),
                                ClosureInfo {
                                    signature_params: params,
                                    signature_return: *return_type,
                                },
                            );
                        }
                    }

                    if binding_name.is_none() {
                        let scrutinee_type = binding_type.as_ref().or(inferred_type.as_ref());
                        let scrutinee_enum = match scrutinee_type {
                            Some(IRType::Enum { name, .. }) => Some(name.as_str()),
                            _ => None,
                        };
                        self.lower_pattern_bindings(
                            &let_stmt.pattern,
                            value,
                            scrutinee_enum,
                            scrutinee_type,
                            ir_func,
                        );
                        return;
                    }

                    let name = binding_name.expect("identifier pattern should be present");

                    // Keep user bindings in the debug model even when the
                    // value is represented directly in SSA.  The backend can
                    // later map the value to a register or stack location;
                    // the declaration and type must not be reconstructed
                    // from the sidecar.
                    let debug_ty = binding_type
                        .clone()
                        .or_else(|| inferred_type.clone())
                        .filter(|ty| !matches!(ty, IRType::Unknown));
                    if let Some(ty) = debug_ty {
                        ir_func.locals.push(LocalDebugInfo {
                            name: name.clone(),
                            ty,
                            value_id: Some(value.id),
                            declaration: Some(self.source_span(stmt.span)),
                            scope_start: Some(self.source_span(stmt.span)),
                            scope_end: None,
                        });
                    }

                    match &value_expr.kind {
                        ExpressionKind::ArrayLiteral { elements } => {
                            if let Some(IRType::Array { element_type, size }) =
                                binding_type.clone().or_else(|| inferred_type.clone())
                            {
                                // `array<T>` is an unsized source annotation.  The
                                // lowered value is nevertheless a fixed-size stack
                                // array, so retain the concrete literal length in the
                                // sidecar used by identifier inference and iteration.
                                // Keeping the annotation's placeholder size (0) here
                                // makes a valid `for` loop silently skip its body.
                                let concrete_size = if size == 0 { elements.len() } else { size };
                                self.array_map.insert(
                                    name.clone(),
                                    ArrayInfo {
                                        ptr: value,
                                        element_type: *element_type,
                                        size: concrete_size,
                                    },
                                );
                            }
                            self.value_map.insert(name.clone(), value);
                        }
                        ExpressionKind::Range { .. } => {
                            if let Some(info) = lowered_range_info {
                                self.range_map.insert(name.clone(), info);
                            }
                            self.value_map.insert(name.clone(), value);
                        }
                        ExpressionKind::StructLiteral {
                            name: struct_name,
                            type_args,
                            ..
                        } => {
                            let (actual_name, _) =
                                self.ensure_struct_definition(struct_name, type_args.as_slice());
                            self.struct_var_map
                                .insert(name.clone(), (value, actual_name.clone()));
                            self.value_map.insert(name.clone(), value);
                        }
                        _ => {
                            if let Some(ref type_ann) = let_stmt.ty {
                                let var_type = self.lower_type_annotation(type_ann);
                                if let IRType::Struct {
                                    name: struct_type_name,
                                    ..
                                } = var_type
                                {
                                    // Structs are pointers: bind the value pointer
                                    // directly so field access, method calls, and
                                    // scope-exit drop glue see the real struct.
                                    self.struct_var_map
                                        .insert(name.clone(), (value, struct_type_name));
                                    self.value_map.insert(name.clone(), value);
                                } else if let Some(&alloca_ptr) = self.alloca_map.get(&name) {
                                    self.builder.build_store(ir_func, alloca_ptr, value);
                                } else {
                                    self.value_map.insert(name.clone(), value);
                                }
                            } else if let Some(IRType::Struct {
                                name: struct_type_name,
                                ..
                            }) = inferred_type.as_ref()
                            {
                                // Value produced by a call/expression returning a
                                // struct: track it so scope-exit drop glue runs.
                                self.struct_var_map
                                    .insert(name.clone(), (value, struct_type_name.clone()));
                                self.value_map.insert(name.clone(), value);
                            } else if let Some(&alloca_ptr) = self.alloca_map.get(&name) {
                                self.builder.build_store(ir_func, alloca_ptr, value);
                            } else {
                                self.value_map.insert(name.clone(), value);
                            }
                        }
                    }
                }
            }
            StatementKind::Assignment(assign) => {
                let value = self.lower_expression(&assign.value, ir_func);
                let value_type = self.infer_expr_ir_type(&assign.value);

                match &assign.target {
                    spectra_compiler::ast::LValue::Identifier(name) => {
                        if let Some((global_key, static_type)) =
                            self.static_globals.get(name).cloned()
                        {
                            let ptr = self.builder.build_global_addr(
                                ir_func,
                                global_key,
                                static_type.clone(),
                            );
                            let value = self.coerce_value_to_type(
                                value,
                                &value_type,
                                &static_type,
                                ir_func,
                            );
                            self.builder.build_store(ir_func, ptr, value);
                            return;
                        }

                        // Assignment to simple variable (uses memory)
                        if let Some(&alloca_ptr) = self.alloca_map.get(name) {
                            let value = self
                                .variable_types
                                .get(name)
                                .map(|target| {
                                    self.coerce_value_to_type(value, &value_type, &target, ir_func)
                                })
                                .unwrap_or(value);
                            self.builder.build_store(ir_func, alloca_ptr, value);

                            if let Some((_, struct_name)) = self.struct_var_map.get(name) {
                                self.struct_var_map
                                    .insert(name.clone(), (alloca_ptr, struct_name));
                            }

                            if let Some(IRType::Array { element_type, size }) =
                                self.variable_types.get(name)
                            {
                                self.array_map.insert(
                                    name.clone(),
                                    ArrayInfo {
                                        ptr: alloca_ptr,
                                        element_type: *element_type,
                                        size,
                                    },
                                );
                            }
                        } else {
                            // Fallback: update value_map (shouldn't happen if analysis is correct)
                            self.value_map.insert(name.clone(), value);

                            if let Some(var_ty) = self.variable_types.get(name) {
                                match var_ty {
                                    IRType::Struct {
                                        name: struct_name, ..
                                    } => {
                                        self.struct_var_map
                                            .insert(name.clone(), (value, struct_name.clone()));
                                    }
                                    IRType::Array { element_type, size } => {
                                        self.array_map.insert(
                                            name.clone(),
                                            ArrayInfo {
                                                ptr: value,
                                                element_type: *element_type,
                                                size,
                                            },
                                        );
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    spectra_compiler::ast::LValue::IndexAccess { array, index } => {
                        // Assignment to array element
                        let array_ptr = self.lower_expression(array, ir_func);
                        let index_value = self.lower_expression(index, ir_func);

                        // Calcular endereço do elemento
                        let elem_type = match self.infer_expr_ir_type(array) {
                            IRType::Array { element_type, .. } => *element_type,
                            // Strings are packed byte buffers: element access
                            // is byte-granular (1-byte stride, 1-byte store).
                            IRType::String => IRType::ExactInt {
                                signed: true,
                                width: crate::ir::IntWidth::I8,
                            },
                            other => {
                                self.error(format!(
                                    "index assignment expected array or string, found {:?}",
                                    other
                                ));
                                return;
                            }
                        };
                        let elem_ptr = self.builder.build_getelementptr(
                            ir_func,
                            array_ptr,
                            index_value,
                            elem_type.clone(),
                        );

                        // Store valor no elemento
                        let value =
                            self.coerce_value_to_type(value, &value_type, &elem_type, ir_func);
                        self.builder.build_store(ir_func, elem_ptr, value);
                    }
                    spectra_compiler::ast::LValue::FieldAccess { object, field } => {
                        // Assignment to struct field (e.g. self.x = ...)
                        // Step 1: collect the field info before any mutable borrow
                        let field_info: Option<(usize, IRType)> =
                            if let spectra_compiler::ast::ExpressionKind::Identifier(var_name) =
                                &object.kind
                            {
                                let lookup = self.struct_var_map.get(var_name.as_str());
                                if let Some((_, sname)) = lookup {
                                    let sname = sname.clone();
                                    self.struct_definitions.get(&sname).and_then(|defs| {
                                        defs.iter()
                                            .enumerate()
                                            .find(|(_, (fname, _))| {
                                                fname.as_str() == field.as_str()
                                            })
                                            .map(|(idx, (_, ty))| (idx, ty.clone()))
                                    })
                                } else {
                                    None
                                }
                            } else {
                                match self.infer_expr_ir_type(object) {
                                    IRType::Struct { fields, .. } => fields
                                        .into_iter()
                                        .enumerate()
                                        .find(|(_, (fname, _))| fname.as_str() == field.as_str())
                                        .map(|(idx, (_, ty))| (idx, ty)),
                                    _ => None,
                                }
                            };

                        // Step 2: get (or compute) the struct pointer
                        let struct_ptr =
                            if let spectra_compiler::ast::ExpressionKind::Identifier(var_name) =
                                &object.kind
                            {
                                if let Some((ptr, _)) = self.struct_var_map.get(var_name.as_str()) {
                                    ptr
                                } else {
                                    self.lower_expression(object, ir_func)
                                }
                            } else {
                                self.lower_expression(object, ir_func)
                            };

                        // Step 3: field pointer (padded layout) + store
                        if let Some((field_idx, field_type)) = field_info {
                            let (offsets, struct_label) = match self.infer_expr_ir_type(object) {
                                IRType::Struct { fields, name, .. } => (
                                    layout::layout_of(fields.iter().map(|(_, ty)| ty)).offsets,
                                    name,
                                ),
                                _ => {
                                    if let spectra_compiler::ast::ExpressionKind::Identifier(
                                        var_name,
                                    ) = &object.kind
                                    {
                                        self.struct_var_map
                                            .get(var_name.as_str())
                                            .and_then(|(_, sname)| {
                                                self.struct_definitions.get(sname.as_str()).map(
                                                    |defs| {
                                                        (
                                                            layout::layout_of(
                                                                defs.iter().map(|(_, ty)| ty),
                                                            )
                                                            .offsets,
                                                            sname.clone(),
                                                        )
                                                    },
                                                )
                                            })
                                            .unwrap_or_default()
                                    } else {
                                        (Vec::new(), "<unknown>".to_string())
                                    }
                                }
                            };
                            let Some(byte_offset) = offsets.get(field_idx).copied() else {
                                self.error(format!(
                                    "field layout for '{struct_label}.{field}' has no offset for field type {field_type:?}"
                                ));
                                return;
                            };
                            let byte_offset = byte_offset as i64;
                            let field_ptr =
                                self.builder
                                    .build_field_ptr(ir_func, struct_ptr, byte_offset);
                            let value =
                                self.coerce_value_to_type(value, &value_type, &field_type, ir_func);
                            self.builder.build_store(ir_func, field_ptr, value);
                        }
                    }
                }
            }
            StatementKind::Return(ret) => {
                let mut skipped_names = HashSet::new();
                let return_type = ret
                    .value
                    .as_ref()
                    .map(|expr| self.infer_expr_ir_type(expr))
                    .unwrap_or(IRType::Void);
                if let Some(expr) = ret.value.as_ref() {
                    Self::collect_moved_identifiers(expr, &mut skipped_names);
                }
                let value = ret
                    .value
                    .as_ref()
                    .map(|expr| self.lower_expression(expr, ir_func));
                if let Some(value) = value {
                    self.emit_escape_for_value(value, &return_type, ir_func);
                }
                let value = if let Some(output_type) = self.current_async_output_type.clone() {
                    Some(self.wrap_async_return_value(ir_func, value, output_type))
                } else {
                    value
                };
                self.emit_scope_drops(ir_func, &skipped_names);
                self.builder.build_return(ir_func, value);
            }
            StatementKind::Expression(expr) => {
                self.lower_expression(expr, ir_func);
            }
            StatementKind::While(while_stmt) => {
                let header_block = ir_func.add_block("while.header");
                let body_block = ir_func.add_block("while.body");
                let exit_block = ir_func.add_block("while.exit");

                // Branch to header
                self.builder.build_branch(ir_func, header_block);
                self.builder.set_current_block(header_block);

                // Evaluate condition
                let condition = self.lower_expression(&while_stmt.condition, ir_func);
                self.builder
                    .build_cond_branch(ir_func, condition, body_block, exit_block);

                // Body (push loop context for break/continue)
                self.loop_stack.push(LoopContext {
                    header_block,
                    exit_block,
                });
                self.builder.set_current_block(body_block);
                self.lower_block(&while_stmt.body.statements, ir_func);
                if !self.current_block_is_terminated(ir_func) {
                    self.builder.build_branch(ir_func, header_block);
                }
                self.loop_stack.pop();

                // Exit
                self.builder.set_current_block(exit_block);
            }
            StatementKind::DoWhile(do_while) => {
                let body_block = ir_func.add_block("do_while.body");
                let header_block = ir_func.add_block("do_while.header");
                let exit_block = ir_func.add_block("do_while.exit");

                // Branch to body first
                self.builder.build_branch(ir_func, body_block);

                // Body (push loop context for break/continue)
                self.loop_stack.push(LoopContext {
                    header_block,
                    exit_block,
                });
                self.builder.set_current_block(body_block);
                self.lower_block(&do_while.body.statements, ir_func);
                if !self.current_block_is_terminated(ir_func) {
                    self.builder.build_branch(ir_func, header_block);
                }
                self.loop_stack.pop();

                // Header/condition
                self.builder.set_current_block(header_block);
                let condition = self.lower_expression(&do_while.condition, ir_func);
                self.builder
                    .build_cond_branch(ir_func, condition, body_block, exit_block);

                // Exit
                self.builder.set_current_block(exit_block);
            }
            StatementKind::For(for_stmt) => {
                self.lower_for_loop_via_iterator(for_stmt, ir_func);
            }
            StatementKind::Loop(loop_stmt) => {
                let body_block = ir_func.add_block("loop.body");
                let exit_block = ir_func.add_block("loop.exit");

                // Branch to body
                self.builder.build_branch(ir_func, body_block);

                // Body (infinite loop - needs break to exit)
                // Use body_block as header since it's the loop entry point
                self.loop_stack.push(LoopContext {
                    header_block: body_block,
                    exit_block,
                });
                self.builder.set_current_block(body_block);
                self.lower_block(&loop_stmt.body.statements, ir_func);
                if !self.current_block_is_terminated(ir_func) {
                    self.builder.build_branch(ir_func, body_block);
                }
                self.loop_stack.pop();

                // Exit (unreachable unless break is used)
                self.builder.set_current_block(exit_block);
            }
            StatementKind::Switch(switch) => {
                let scrutinee = self.lower_expression(&switch.value, ir_func);

                // Create blocks for each case and default/exit
                let mut exit_block = if switch.default.is_none() {
                    Some(ir_func.add_block("switch.exit"))
                } else {
                    None
                };
                let mut cases = Vec::new();
                let mut case_blocks = Vec::new();

                for (idx, case) in switch.cases.iter().enumerate() {
                    let case_block = ir_func.add_block(format!("switch.case.{}", idx));
                    case_blocks.push((case_block, case));

                    // Extract constant value from pattern
                    let pattern_int = self
                        .evaluate_int_constant(&case.pattern)
                        .unwrap_or_else(|| {
                            self.error(format!(
                                "Switch case pattern must be a constant integer expression, found {:?}",
                                case.pattern.kind
                            ));
                            0
                        });
                    cases.push((pattern_int, case_block));
                }

                // Build switch terminator
                let default = if switch.default.is_some() {
                    ir_func.add_block("switch.default")
                } else {
                    exit_block.expect("switch without default must have an exit block")
                };

                if let Some(current_block) = self.builder.get_current_block() {
                    if let Some(block) = ir_func.get_block_mut(current_block) {
                        block.set_terminator(Terminator::Switch {
                            value: scrutinee,
                            cases,
                            default,
                        });
                    }
                }

                // Lower each case body
                for (case_block, case) in case_blocks {
                    self.builder.set_current_block(case_block);
                    self.lower_block(&case.body.statements, ir_func);
                    if !self.current_block_is_terminated(ir_func) {
                        let exit =
                            *exit_block.get_or_insert_with(|| ir_func.add_block("switch.exit"));
                        self.builder.build_branch(ir_func, exit);
                    }
                }

                // Lower default if present
                if let Some(ref default_block) = switch.default {
                    self.builder.set_current_block(default);
                    self.lower_block(&default_block.statements, ir_func);
                    if !self.current_block_is_terminated(ir_func) {
                        let exit =
                            *exit_block.get_or_insert_with(|| ir_func.add_block("switch.exit"));
                        self.builder.build_branch(ir_func, exit);
                    }
                }

                // Exit
                if let Some(exit) = exit_block {
                    self.builder.set_current_block(exit);
                }
            }
            StatementKind::Break => {
                // Branch to the exit block of the innermost loop
                if let Some(loop_ctx) = self.loop_stack.last() {
                    self.builder.build_branch(ir_func, loop_ctx.exit_block);
                } else {
                    // Break outside of loop - error, but generate unreachable
                    if let Some(current_block) = self.builder.get_current_block() {
                        if let Some(block) = ir_func.get_block_mut(current_block) {
                            block.set_terminator(Terminator::Unreachable);
                        }
                    }
                }
            }
            StatementKind::Continue => {
                // Branch to the header block of the innermost loop
                if let Some(loop_ctx) = self.loop_stack.last() {
                    self.builder.build_branch(ir_func, loop_ctx.header_block);
                } else {
                    // Continue outside of loop - error, but generate unreachable
                    if let Some(current_block) = self.builder.get_current_block() {
                        if let Some(block) = ir_func.get_block_mut(current_block) {
                            block.set_terminator(Terminator::Unreachable);
                        }
                    }
                }
            }
            StatementKind::IfLet(IfLetStatement {
                pattern,
                value,
                then_block,
                else_block,
                ..
            }) => {
                // Evaluate the scrutinee expression once
                let scrutinee_value = self.lower_expression(value, ir_func);
                let scrutinee_type = self.infer_expr_ir_type(value);
                let scrutinee_enum_name = match &scrutinee_type {
                    IRType::Enum { name, .. } => Some(name.clone()),
                    IRType::Generic { .. } => {
                        self.ir_nominal_name(&scrutinee_type).map(str::to_string)
                    }
                    _ => None,
                };

                // Create basic blocks
                let then_blk = ir_func.add_block("if_let.then");
                let exit_blk = ir_func.add_block("if_let.exit");
                let else_blk_opt = else_block
                    .as_ref()
                    .map(|_| ir_func.add_block("if_let.else"));
                let false_target = else_blk_opt.unwrap_or(exit_blk);

                // Pattern check in the current block
                let matches = self.lower_pattern_check(
                    pattern,
                    scrutinee_value,
                    scrutinee_enum_name.as_deref(),
                    Some(&scrutinee_type),
                    ir_func,
                );
                self.builder
                    .build_cond_branch(ir_func, matches, then_blk, false_target);

                // --- Then block ---
                self.builder.set_current_block(then_blk);
                // Push an inner scope for the pattern bindings
                self.value_map.push_scope();
                self.variable_types.push_scope();
                self.array_map.push_scope();
                self.range_map.push_scope();
                self.struct_var_map.push_scope();

                self.lower_pattern_bindings(
                    pattern,
                    scrutinee_value,
                    scrutinee_enum_name.as_deref(),
                    Some(&scrutinee_type),
                    ir_func,
                );
                // lower_block creates its own inner scope; bindings remain visible via scope search
                self.lower_block(&then_block.statements, ir_func);

                self.struct_var_map.pop_scope();
                self.array_map.pop_scope();
                self.range_map.pop_scope();
                self.variable_types.pop_scope();
                self.value_map.pop_scope();

                let cur = self.builder.get_current_block().unwrap_or(then_blk);
                let terminated = ir_func
                    .get_block(cur)
                    .map(|b| b.terminator.is_some())
                    .unwrap_or(false);
                if !terminated {
                    self.builder.build_branch(ir_func, exit_blk);
                }

                // --- Else block (optional) ---
                if let (Some(else_b), Some(else_blk)) = (else_block, else_blk_opt) {
                    self.builder.set_current_block(else_blk);
                    self.lower_block(&else_b.statements, ir_func);
                    let cur = self.builder.get_current_block().unwrap_or(else_blk);
                    let terminated = ir_func
                        .get_block(cur)
                        .map(|b| b.terminator.is_some())
                        .unwrap_or(false);
                    if !terminated {
                        self.builder.build_branch(ir_func, exit_blk);
                    }
                }

                // --- Exit block ---
                self.builder.set_current_block(exit_blk);
            }
            StatementKind::WhileLet(WhileLetStatement {
                pattern,
                value,
                body,
                ..
            }) => {
                let header_block = ir_func.add_block("while_let.header");
                let body_block = ir_func.add_block("while_let.body");
                let exit_block = ir_func.add_block("while_let.exit");

                // Jump from current block into loop header
                self.builder.build_branch(ir_func, header_block);
                self.builder.set_current_block(header_block);

                // Re-evaluate scrutinee on every iteration and check the pattern
                let scrutinee_value = self.lower_expression(value, ir_func);
                let scrutinee_type = self.infer_expr_ir_type(value);
                let scrutinee_enum_name = match &scrutinee_type {
                    IRType::Enum { name, .. } => Some(name.clone()),
                    IRType::Generic { .. } => {
                        self.ir_nominal_name(&scrutinee_type).map(str::to_string)
                    }
                    _ => None,
                };

                let matches = self.lower_pattern_check(
                    pattern,
                    scrutinee_value,
                    scrutinee_enum_name.as_deref(),
                    Some(&scrutinee_type),
                    ir_func,
                );
                self.builder
                    .build_cond_branch(ir_func, matches, body_block, exit_block);

                // --- Body block ---
                // Register loop so that break/continue work correctly
                self.loop_stack.push(LoopContext {
                    header_block,
                    exit_block,
                });
                self.builder.set_current_block(body_block);

                // Push scope for pattern bindings (visible to all statements in the body)
                self.value_map.push_scope();
                self.variable_types.push_scope();
                self.array_map.push_scope();
                self.range_map.push_scope();
                self.struct_var_map.push_scope();

                // Bind pattern variables (e.g. `n` in `while let Option::Some(n) = ...`)
                // header_block dominates body_block, so scrutinee_value is live here.
                self.lower_pattern_bindings(
                    pattern,
                    scrutinee_value,
                    scrutinee_enum_name.as_deref(),
                    Some(&scrutinee_type),
                    ir_func,
                );
                self.lower_block(&body.statements, ir_func);

                self.struct_var_map.pop_scope();
                self.array_map.pop_scope();
                self.range_map.pop_scope();
                self.variable_types.pop_scope();
                self.value_map.pop_scope();

                self.loop_stack.pop();

                // Back-edge to header unless body already has a terminator
                let cur = self.builder.get_current_block().unwrap_or(body_block);
                let terminated = ir_func
                    .get_block(cur)
                    .map(|b| b.terminator.is_some())
                    .unwrap_or(false);
                if !terminated {
                    self.builder.build_branch(ir_func, header_block);
                }

                // --- Exit block ---
                self.builder.set_current_block(exit_block);
            }
        }
    }
}
