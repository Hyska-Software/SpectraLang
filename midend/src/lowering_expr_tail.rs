impl ASTLowering {
    fn lower_expression_tail(&mut self, expr: &Expression, ir_func: &mut IRFunction) -> Value {
        match &expr.kind {
            ExpressionKind::CharLiteral(c) => self
                .builder
                .build_const_int_typed(ir_func, *c as i64, IRType::Char),
            ExpressionKind::FString(parts) => {
                // Lower each part to a string value:
                // - Literal parts: inline string literals (already String type)
                // - Interpolated parts: lower expression then convert to String via
                //   the appropriate runtime function (int_to_string / float_to_string /
                //   bool_to_string). String-typed expressions pass through unchanged.
                // All parts are then concatenated via spectra.std.string.concat.
                let string_parts: Vec<Value> = parts
                    .iter()
                    .map(|part| match part {
                        FStringPart::Literal(s) => self.lower_string_literal(s, ir_func),
                        FStringPart::Interpolated(expr) => {
                            let val = self.lower_expression(expr, ir_func);
                            let ty = self.infer_expr_ir_type(expr);
                            let conv = match ty {
                                IRType::String => None,
                                IRType::Float => Some("spectra.std.convert.float_to_string"),
                                IRType::Bool => Some("spectra.std.convert.bool_to_string"),
                                _ => Some("spectra.std.convert.int_to_string"),
                            };
                            if let Some(runtime_fn) = conv {
                                self.require_value(
                                    self.builder.build_typed_host_call(
                                        ir_func,
                                        runtime_fn.to_string(),
                                        vec![val],
                                        IRType::String,
                                        true,
                                    ),
                                    "f-string conversion host call did not produce its declared result",
                                )
                            } else {
                                val
                            }
                        }
                    })
                    .collect();

                if string_parts.is_empty() {
                    return self.lower_string_literal("", ir_func);
                }
                let mut result = string_parts[0];
                for part in &string_parts[1..] {
                    result = self.require_value(
                        self.builder.build_typed_host_call(
                            ir_func,
                            "spectra.std.string.concat".to_string(),
                            vec![result, *part],
                            IRType::String,
                            true,
                        ),
                        "f-string concatenation host call did not produce its declared result",
                    );
                }
                result
            }
            ExpressionKind::Try(inner) => {
                // Proper Result-propagation semantics:
                //   1. Evaluate the inner expression → Result pointer (tagged heap tuple)
                //   2. Load the tag from slot 0
                //   3. tag == 0  →  Ok: extract the payload from slot 1 and continue
                //      tag != 0  →  Err: early-return the Result pointer directly
                //        (the calling function is assumed to also return Result<_, E>)
                let result_ptr = self.lower_expression(inner, ir_func);

                // Load tag (first field of the tagged tuple)
                let zero_idx = self.builder.build_const_int(ir_func, 0);
                let tag_ptr =
                    self.builder
                        .build_getelementptr(ir_func, result_ptr, zero_idx, IRType::Int);
                let tag_val = self.builder.build_load(ir_func, tag_ptr);

                // is_ok = (tag == 0)
                let zero = self.builder.build_const_int(ir_func, 0);
                let is_ok = self.builder.build_eq(ir_func, tag_val, zero);

                let ok_block = ir_func.add_block("try.ok");
                let err_block = ir_func.add_block("try.err");
                self.builder
                    .build_cond_branch(ir_func, is_ok, ok_block, err_block);

                // Err branch: early return the error result pointer
                self.builder.set_current_block(err_block);
                self.builder.build_return(ir_func, Some(result_ptr));

                // Ok branch: extract the Ok payload from slot 1
                self.builder.set_current_block(ok_block);
                let one_idx = self.builder.build_const_int(ir_func, 1);
                let payload_ptr =
                    self.builder
                        .build_getelementptr(ir_func, result_ptr, one_idx, IRType::Int);
                self.builder.build_load(ir_func, payload_ptr)
            }
            ExpressionKind::Await(inner) => {
                let task = self.lower_expression(inner, ir_func);
                let output_type = match self.infer_expr_ir_type(inner) {
                    IRType::Task { output } => *output,
                    other => {
                        return self.invalid_value(format!(
                            "await operand must lower to Task<T>, found {:?}",
                            other
                        ));
                    }
                };
                let state = self.next_async_state();
                // AsyncSuspend/AsyncResume stay as pure markers: the backend
                // currently emits no code for them (preemptive suspension /
                // stackful coroutines are future work). The actual waiting
                // contract of `await` is carried entirely by the host calls
                // between them.
                self.builder.build_async_suspend(ir_func, task, state);
                // Block until the task reaches a terminal state. The host
                // parks inside the reactor (no CPU spin) and reports the
                // join status: 0 = completed, 1 = cancelled, 2 = failed.
                // The status value itself is intentionally discarded:
                // `spectra.async.task.result` below re-checks the task and
                // fails with HOST_STATUS_INVALID_ARGUMENT for cancelled or
                // failed tasks, which is how cancellation surfaces to the
                // executing backend today.
                let _ = self.builder.build_typed_host_call(
                    ir_func,
                    "spectra.async.task.wait".to_string(),
                    vec![task],
                    IRType::Int,
                    true,
                );
                self.builder.build_async_resume(ir_func, task, state);
                if output_type == IRType::Void {
                    self.builder.build_const_int(ir_func, 0)
                } else {
                    self.require_value(
                        self.builder.build_typed_host_call(
                            ir_func,
                            "spectra.async.task.result".to_string(),
                            vec![task],
                            output_type,
                            true,
                        ),
                        "async task.result host call did not produce its declared result",
                    )
                }
            }
            ExpressionKind::Range {
                start,
                end,
                inclusive,
            } => {
                self.lower_range_expression(start, end, *inclusive, ir_func)
                    .0
            }
            ExpressionKind::Lambda { params, body, .. } => {
                // Lower as a top-level IR function with a generated unique name.
                let lambda_name = format!("__lambda_{}", self.lambda_counter);
                self.lambda_counter += 1;

                let captures = self.collect_lambda_captures(params, body);
                let lambda_func = self.lower_lambda(lambda_name.clone(), &captures, params, body);
                self.pending_lambdas.push(lambda_func);

                self.build_closure_object(ir_func, lambda_name, &captures)
            }
            ExpressionKind::Block(block) => {
                let stmts = &block.statements;
                if stmts.is_empty() {
                    return self.builder.build_const_int(ir_func, 0);
                }

                self.value_map.push_scope();
                self.variable_types.push_scope();
                self.array_map.push_scope();
                self.range_map.push_scope();
                self.struct_var_map.push_scope();

                for stmt in &stmts[..stmts.len() - 1] {
                    self.lower_statement(stmt, ir_func);
                }

                let last = &stmts[stmts.len() - 1];
                let last_type = match &last.kind {
                    spectra_compiler::ast::StatementKind::Expression(expr) => {
                        Some(self.infer_expr_ir_type(expr))
                    }
                    _ => None,
                };
                let last_value = match &last.kind {
                    spectra_compiler::ast::StatementKind::Expression(expr) => {
                        self.lower_expression(expr, ir_func)
                    }
                    _ => {
                        self.lower_statement(last, ir_func);
                        self.builder.build_const_int(ir_func, 0)
                    }
                };

                if !self.current_block_is_terminated(ir_func) {
                    let mut skipped_names = HashSet::new();
                    if let spectra_compiler::ast::StatementKind::Expression(expr) = &last.kind {
                        Self::collect_moved_identifiers(expr, &mut skipped_names);
                    }
                    if let Some(last_type) = last_type.as_ref() {
                        self.emit_escape_for_value(last_value, last_type, ir_func);
                    }
                    self.emit_scope_drops(ir_func, &skipped_names);
                }

                self.struct_var_map.pop_scope();
                self.array_map.pop_scope();
                self.range_map.pop_scope();
                self.variable_types.pop_scope();
                self.value_map.pop_scope();

                last_value
            }
            ExpressionKind::DifferentiableBlock(block) => {
                let stmts = &block.statements;
                let loss = if stmts.is_empty() {
                    self.builder.build_const_int(ir_func, 0)
                } else {
                    for stmt in &stmts[..stmts.len() - 1] {
                        self.lower_statement(stmt, ir_func);
                    }

                    let last = &stmts[stmts.len() - 1];
                    match &last.kind {
                        spectra_compiler::ast::StatementKind::Expression(expr) => {
                            self.lower_expression(expr, ir_func)
                        }
                        _ => {
                            self.lower_statement(last, ir_func);
                            self.builder.build_const_int(ir_func, 0)
                        }
                    }
                };
                self.builder.build_host_call(
                    ir_func,
                    // Compiler-only region marker. The midend immediately
                    // materializes this into AutodiffStep instructions; it is
                    // never registered as a runtime host call.
                    "spectra.compiler.autodiff_region".to_string(),
                    vec![loss],
                    false,
                );
                loss
            }
            ExpressionKind::AsyncBlock(block) => {
                let output_type = self
                    .expected_async_output_type()
                    .or_else(|| self.infer_block_result_type(block))
                    .unwrap_or(IRType::Void);
                let saved_async_output = self.current_async_output_type.clone();
                self.current_async_output_type = Some(output_type.clone());

                let stmts = &block.statements;
                let value = if stmts.is_empty() {
                    None
                } else {
                    for stmt in &stmts[..stmts.len() - 1] {
                        self.lower_statement(stmt, ir_func);
                    }
                    match &stmts[stmts.len() - 1].kind {
                        StatementKind::Expression(expr) => {
                            if output_type == IRType::Void {
                                self.lower_expression(expr, ir_func);
                                None
                            } else {
                                Some(self.lower_expression(expr, ir_func))
                            }
                        }
                        last => {
                            let stmt = Statement {
                                kind: last.clone(),
                                span: stmts[stmts.len() - 1].span,
                            };
                            self.lower_statement(&stmt, ir_func);
                            None
                        }
                    }
                };

                let result = if self.current_block_is_terminated(ir_func) {
                    self.builder.build_const_int(ir_func, 0)
                } else {
                    self.wrap_async_return_value(ir_func, value, output_type)
                };

                self.current_async_output_type = saved_async_output;
                result
            }
            _ => unreachable!("lowering expression category mismatch"),
        }
    }
}
