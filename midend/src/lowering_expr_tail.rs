use super::*;
use crate::ir::InstructionKind;

impl ASTLowering {
    pub(crate) fn lower_expression_tail(
        &mut self,
        expr: &Expression,
        ir_func: &mut IRFunction,
    ) -> Value {
        match &expr.kind {
            ExpressionKind::CharLiteral(c) => {
                self.builder
                    .build_const_int_typed(ir_func, *c as i64, IRType::Char)
            }
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
                // Await is deliberately a pure source marker here. The
                // coroutine transform splits the CFG and emits child polling,
                // subscription, wake, and frame operations. Lowering it to a
                // blocking host call would execute async bodies eagerly.
                let result = ir_func.next_value();
                ir_func
                    .get_block_mut(self.builder.get_current_block().unwrap())
                    .expect("current block exists while lowering await")
                    .add_instruction(InstructionKind::Await {
                        result,
                        task,
                        output_type,
                    });
                result
            }
            ExpressionKind::Range {
                start,
                end,
                inclusive,
            } => {
                self.lower_range_expression(start, end, *inclusive, ir_func)
                    .0
            }
            ExpressionKind::Lambda {
                is_async,
                params,
                body,
            } => {
                // Lower as a top-level IR function with a generated unique name.
                let lambda_name = format!("__lambda_{}", self.lambda_counter);
                self.lambda_counter += 1;

                let captures = self.collect_lambda_captures(params, body);
                let lambda_func =
                    self.lower_lambda(lambda_name.clone(), &captures, params, body, *is_async);
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
                // An async block is a lazy closure, not an eager body splice.
                // Lowering it as a generated async function preserves captures
                // and ensures no Await marker reaches the backend.
                let lambda_name = format!("__async_block_{}", self.lambda_counter);
                self.lambda_counter += 1;
                let body_expression = Expression {
                    span: expr.span,
                    kind: ExpressionKind::Block(block.clone()),
                };
                let captures = self.collect_lambda_captures(&[], &body_expression);
                let lambda_func =
                    self.lower_lambda(lambda_name.clone(), &captures, &[], &body_expression, true);
                self.pending_lambdas.push(lambda_func);
                let environment =
                    self.build_closure_object(ir_func, lambda_name.clone(), &captures);
                self.require_value(
                    self.builder
                        .build_call(ir_func, lambda_name, vec![environment], true),
                    "async block ramp did not produce a task",
                )
            }
            _ => unreachable!("lowering expression category mismatch"),
        }
    }
}
