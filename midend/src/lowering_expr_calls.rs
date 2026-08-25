impl ASTLowering {
    fn lower_expression_call(&mut self, expr: &Expression, ir_func: &mut IRFunction) -> Value {
        match &expr.kind {
            ExpressionKind::Call { callee, arguments } => {
                let arg_values: Vec<Value> = arguments
                    .iter()
                    .map(|arg| self.lower_expression(arg, ir_func))
                    .collect();

                if let ExpressionKind::Identifier(name) = &callee.kind {
                    if name == "block_on" {
                        let Some(task) = arg_values.first().copied() else {
                            self.error("block_on expects one Task<T> argument".to_string());
                            return self.invalid_value("block_on requires one Task<T> argument");
                        };
                        let output_type = match arguments.first() {
                            Some(arg) => match self.infer_expr_ir_type(arg) {
                                IRType::Task { output }
                                    if !Self::ir_type_contains_unknown(&output) => *output,
                                IRType::Task { .. } => {
                                    return self.invalid_value(
                                        "block_on cannot lower a Task with an unresolved output type",
                                    );
                                }
                                other => {
                                    return self.invalid_value(format!(
                                        "block_on operand must lower to Task<T>, found {:?}",
                                        other
                                    ));
                                }
                            },
                            None => {
                                return self.invalid_value(
                                    "block_on requires one Task<T> argument",
                                );
                            }
                        };
                        let block_on_result = self.builder.build_typed_host_call(
                            ir_func,
                            "spectra.async.task.block_on".to_string(),
                            vec![task],
                            output_type.clone(),
                            output_type != IRType::Void,
                        );
                        if output_type != IRType::Void {
                            let _ = self.require_value(
                                block_on_result,
                                "async block_on host call did not produce its declared result",
                            );
                        }
                        return if output_type == IRType::Void {
                            self.builder.build_const_int(ir_func, 0)
                        } else {
                            self.require_value(
                                self.builder.build_typed_host_call(
                                    ir_func,
                                    "spectra.async.task.result".to_string(),
                                    vec![task],
                                    output_type.clone(),
                                    true,
                                ),
                                "async task.result host call did not produce its declared result",
                            )
                        };
                    }
                }

                if let Some(descriptor) = self.host_function_descriptor_for_call(callee, arguments) {
                    if descriptor.runtime_name == "spectra.std.concurrent.task_spawn_fn" {
                        if let Some(closure) = arg_values.first().copied() {
                            // A worker thread retains and invokes this closure
                            // after the current function returns, so move its
                            // manual allocation to the base frame before the
                            // host call stores the handle.
                            self.builder
                                .build_escape_manual_alloc(ir_func, closure);
                        }
                    } else if matches!(
                        descriptor.runtime_name,
                        "spectra.api.handler.register_sync_callback"
                            | "spectra.api.handler.register_async_callback"
                    ) {
                        if let Some(callback) = arg_values.get(1).copied() {
                            // The server retains this closure after the current
                            // function returns, so move its manual allocation
                            // to the base frame before the host call stores it.
                            self.builder
                                .build_escape_manual_alloc(ir_func, callback);
                        }
                    }

                    // Special case: io.print / io.println / io.eprint / io.eprintln
                    // use (type_tag, value) pairs so the runtime can dispatch the
                    // correct formatter per argument.
                    if descriptor.runtime_name == "spectra.std.io.print"
                        || descriptor.runtime_name == "spectra.std.io.println"
                        || descriptor.runtime_name == "spectra.std.io.eprint"
                        || descriptor.runtime_name == "spectra.std.io.eprintln"
                    {
                        let mut paired: Vec<Value> = Vec::with_capacity(arg_values.len() * 2);
                        for (arg_val, arg_expr) in arg_values.iter().zip(arguments.iter()) {
                            let tag: i64 = match self.infer_expr_ir_type(arg_expr) {
                                IRType::String => 1, // PRINT_TAG_STR
                                IRType::Bool => 2,   // PRINT_TAG_BOOL
                                IRType::Float => 3,  // PRINT_TAG_FLOAT
                                _ => 0,              // PRINT_TAG_INT
                            };
                            let tag_val = self.builder.build_const_int(ir_func, tag);
                            paired.push(tag_val);
                            paired.push(*arg_val);
                        }
                        let result_value = self.builder.build_typed_host_call(
                            ir_func,
                            descriptor.runtime_name.to_string(),
                            paired,
                            descriptor.return_type.clone(),
                            descriptor.returns_value,
                        );
                        return if descriptor.returns_value {
                            self.require_value(
                                result_value,
                                "I/O host call did not produce its declared result",
                            )
                        } else {
                            result_value.unwrap_or_else(|| self.builder.build_const_int(ir_func, 0))
                        };
                    }

                    let result_value = self.builder.build_typed_host_call(
                        ir_func,
                        descriptor.runtime_name.to_string(),
                        arg_values.clone(),
                        descriptor.return_type.clone(),
                        descriptor.returns_value,
                    );
                    return if descriptor.returns_value {
                        self.require_value(
                            result_value,
                            "standard-library host call did not produce its declared result",
                        )
                    } else {
                        result_value.unwrap_or_else(|| self.builder.build_const_int(ir_func, 0))
                    };
                }

                // Extract function name from callee
                let function_name = if let ExpressionKind::Identifier(name) = &callee.kind {
                    name.clone()
                } else {
                    "unknown".to_string()
                };

                // --- Closure variable call: direct name lookup ---
                // Function values are closure handles: slot 0 stores the code pointer
                // and the handle itself is passed as hidden environment argument.
                if let ExpressionKind::Identifier(name) = &callee.kind {
                    if let Some(info) = self.closure_var_map.get(name).cloned() {
                        if let Some(handle) = self.value_map.get(name) {
                            return self.lower_closure_handle_call(
                                handle,
                                arg_values,
                                info.signature_params,
                                info.signature_return,
                                ir_func,
                            );
                        }
                    }

                    // --- Function pointer parameter (fn(T) -> R) ---
                    // If the identifier is a variable of Function type, call through the pointer.
                    if let Some(var_type) = self.variable_types.get(name) {
                        if let IRType::Function {
                            params: sig_params,
                            return_type: sig_return,
                        } = var_type.clone()
                        {
                            if let Some(fn_ptr) = self.value_map.get(name) {
                                return self.lower_closure_handle_call(
                                    fn_ptr,
                                    arg_values,
                                    sig_params,
                                    *sig_return,
                                    ir_func,
                                );
                            }
                        }
                    }
                }

                // Semantic analysis should have rejected this call already.
                // Do not silently lower it to integer zero: that would make a
                // broken program appear to compile and execute successfully.
                if !self.function_return_types.contains_key(&function_name)
                    && !self.generic_functions.contains_key(&function_name)
                {
                    return self.invalid_value(format!(
                        "unresolved function '{}' during lowering",
                        function_name
                    ));
                }

                // Check if this is a call to a generic function
                let final_function_name = if self.generic_functions.contains_key(&function_name) {
                    // This is a generic function call - we need to infer concrete types
                    // For now, we'll infer types from the argument expressions
                    let concrete_types = self.infer_argument_types(arguments);

                    let request = MonomorphizationRequest {
                        generic_name: function_name.clone(),
                        concrete_types: concrete_types.clone(),
                    };

                    let mangled = request.mangled_name();

                    // Check if we already generated this specialization
                    if !self.generated_specializations.contains_key(&mangled) {
                        // Mark it as pending
                        // requesting specialization
                        self.pending_specializations.push(request);
                    }

                    mangled
                } else {
                    function_name
                };

                self.require_value(
                    self.builder
                        .build_call(ir_func, final_function_name, arg_values, true),
                    "function call did not produce its declared result",
                )
            }
            _ => unreachable!("lowering expression category mismatch"),
        }
    }
}
