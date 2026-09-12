use super::*;

impl ASTLowering {
    pub(crate) fn lower_function(&mut self, ast_func: &ASTFunction) -> IRFunction {
        // Convert parameters
        let params: Vec<Parameter> = ast_func
            .params
            .iter()
            .enumerate()
            .map(|(idx, param)| Parameter {
                id: idx,
                name: param.name.clone(),
                ty: param
                    .ty
                    .as_ref()
                    .map(|t| self.lower_type_annotation(t))
                    .unwrap_or(IRType::Void),
            })
            .collect();

        // Create function
        let body_return_type = ast_func
            .return_type
            .as_ref()
            .map(|t| self.lower_type_annotation(t))
            .unwrap_or(IRType::Void);
        let return_type = if ast_func.is_async {
            IRType::Task {
                output: Box::new(body_return_type.clone()),
            }
        } else {
            body_return_type.clone()
        };

        self.function_return_types
            .insert(ast_func.name.clone(), return_type.clone());

        let mut ir_func = IRFunction::new(&ast_func.name, params.clone(), return_type.clone());
        ir_func.source_span = Some(self.source_span(ast_func.span));
        ir_func.locals = ast_func
            .params
            .iter()
            .zip(params.iter())
            .map(|(ast_param, param)| LocalDebugInfo {
                name: param.name.clone(),
                ty: param.ty.clone(),
                value_id: Some(param.id),
                declaration: Some(self.source_span(ast_param.span)),
                scope_start: Some(self.source_span(ast_func.body.span)),
                scope_end: Some(self.source_span(ast_func.body.span)),
            })
            .collect();

        // Create entry block
        let entry_block = ir_func.add_block("entry");
        self.builder.set_current_block(entry_block);

        // Map parameters to values
        self.value_map.clear();
        self.variable_types.clear();
        self.alloca_map.clear();
        self.array_map.clear();
        self.range_map.clear();
        self.struct_var_map.clear();
        self.drop_excluded_names.clear();
        for (idx, param) in params.iter().enumerate() {
            let value = Value { id: idx };
            self.value_map.insert(param.name.clone(), value);

            if param.ty != IRType::Void {
                self.variable_types
                    .insert(param.name.clone(), param.ty.clone());

                if let IRType::Struct { name, .. } = &param.ty {
                    self.struct_var_map
                        .insert(param.name.clone(), (value, name.clone()));
                }

                if let IRType::Array { element_type, size } = &param.ty {
                    self.array_map.insert(
                        param.name.clone(),
                        ArrayInfo {
                            ptr: value,
                            element_type: element_type.as_ref().clone(),
                            size: *size,
                        },
                    );
                }
            }
        }

        // Analyze which variables are assigned to (need memory allocation).
        // Slots are typed from a syntactic hint so promoted scalars in the
        // backend match their stored value types (a blanket `Int` slot makes
        // `bool`/`float` locals panic in the Cranelift frontend when mutated
        // across blocks).
        let mut slot_hints: std::collections::HashMap<String, IRType> = params
            .iter()
            .filter(|param| param.ty != IRType::Void)
            .map(|param| (param.name.clone(), param.ty.clone()))
            .collect();
        let assigned_vars = {
            let assigned =
                self.find_assigned_variables_with_types(&ast_func.body.statements, &mut slot_hints);
            let mut names: Vec<String> = assigned.into_iter().collect();
            names.sort();
            names
        };
        for var_name in &assigned_vars {
            let slot_type = slot_hints.get(var_name).cloned().unwrap_or(IRType::Int);
            let alloca_value = self.builder.build_alloca(&mut ir_func, slot_type);
            self.alloca_map.insert(var_name.clone(), alloca_value);
        }

        // Lower async bodies as poll work. The ramp is generated below and
        // therefore cannot execute any body instruction.
        self.current_function = Some(ir_func.clone());
        self.current_function_return_annotation = ast_func.return_type.clone();
        let saved_async_output = self.current_async_output_type.clone();
        let saved_async_poll = self.lowering_async_poll;
        self.lowering_async_poll = ast_func.is_async;
        if ast_func.is_async {
            self.current_async_output_type = None;
        }

        // Check if last statement is an expression (implicit return)
        let mut implicit_return_value = None;
        if let Some(last_stmt) = ast_func.body.statements.last() {
            if let StatementKind::Expression(expr) = &last_stmt.kind {
                // Lower all statements except the last
                if ast_func.body.statements.len() > 1 {
                    for stmt in &ast_func.body.statements[..ast_func.body.statements.len() - 1] {
                        self.lower_statement(stmt, &mut ir_func);
                    }
                }
                let last_type = self.infer_expr_ir_type(expr);
                let last_value = self.lower_expression(expr, &mut ir_func);
                if body_return_type != IRType::Void {
                    self.emit_escape_for_value(last_value, &last_type, &mut ir_func);
                    implicit_return_value = Some(last_value);
                }
                let mut skipped_names = HashSet::new();
                Self::collect_moved_identifiers(expr, &mut skipped_names);
                self.emit_scope_drops(&mut ir_func, &skipped_names);
            } else {
                // No implicit return, lower all statements
                self.lower_block(&ast_func.body.statements, &mut ir_func);
            }
        } else {
            // Empty body
            self.lower_block(&ast_func.body.statements, &mut ir_func);
        }

        // Ensure function has a return in the current block
        // (After lowering all statements, we should be in the final block)
        if let Some(current_block_id) = self.builder.get_current_block() {
            let needs_terminator = ir_func
                .get_block(current_block_id)
                .map(|block| block.terminator.is_none())
                .unwrap_or(false);
            if needs_terminator {
                let value = implicit_return_value;
                if let Some(block) = ir_func.get_block_mut(current_block_id) {
                    block.set_terminator(Terminator::Return { value });
                }
            }
        }

        self.current_async_output_type = saved_async_output;
        self.lowering_async_poll = saved_async_poll;
        if ast_func.is_async {
            return self.finish_async_coroutine(ir_func, params, body_return_type);
        }

        ir_func
    }

    /// Lower an impl block method into a top-level IR function.
    ///
    /// The method `foo` on type `TypeName` becomes a function named `TypeName_foo`
    /// where `self` (in all forms: `self`, `&self`, `&mut self`) is passed as the
    /// first regular parameter of type `TypeName`.
    pub(crate) fn lower_method(&mut self, method: &ASTMethod, type_name: &str) -> IRFunction {
        let mangled_name = format!("{}_{}", type_name, method.name);

        // Convert method parameters to IR parameters.
        // `self` parameters become a parameter typed as the owning struct/enum.
        let params: Vec<Parameter> = method
            .params
            .iter()
            .enumerate()
            .map(|(idx, param)| {
                let ty = if param.is_self {
                    // Resolve the concrete type of `self`
                    if let Some(fields) = self.struct_definitions.get(type_name) {
                        IRType::Struct {
                            name: type_name.to_string(),
                            fields: fields.clone(),
                        }
                    } else if let Some(variants) = self.enum_definitions.get(type_name) {
                        let simplified = variants
                            .iter()
                            .map(|(vn, _, data)| (vn.clone(), data.clone()))
                            .collect();
                        IRType::Enum {
                            name: type_name.to_string(),
                            variants: simplified,
                        }
                    } else {
                        self.error(format!(
                            "type '{}' was not registered before lowering method self",
                            type_name
                        ));
                        IRType::Unknown
                    }
                } else {
                    param
                        .type_annotation
                        .as_ref()
                        .map(|t| self.lower_type_annotation_with_self(t, type_name))
                        .unwrap_or(IRType::Void)
                };
                Parameter {
                    id: idx,
                    name: param.name.clone(),
                    ty,
                }
            })
            .collect();

        let body_return_type = method
            .return_type
            .as_ref()
            .map(|t| self.lower_type_annotation_with_self(t, type_name))
            .unwrap_or(IRType::Void);
        let return_type = if method.is_async {
            IRType::Task {
                output: Box::new(body_return_type.clone()),
            }
        } else {
            body_return_type.clone()
        };

        self.function_return_types
            .insert(mangled_name.clone(), return_type.clone());

        let mut ir_func = IRFunction::new(&mangled_name, params.clone(), return_type.clone());
        let entry_block = ir_func.add_block("entry");
        self.builder.set_current_block(entry_block);

        // Reset per-function lowering state (mirrors lower_function).
        self.value_map.clear();
        self.variable_types.clear();
        self.alloca_map.clear();
        self.array_map.clear();
        self.range_map.clear();
        self.struct_var_map.clear();
        self.drop_excluded_names.clear();

        for (idx, param) in params.iter().enumerate() {
            if method
                .params
                .get(idx)
                .map(|param| param.is_self)
                .unwrap_or(false)
            {
                self.drop_excluded_names.insert(param.name.clone());
            }
            let value = Value { id: idx };
            self.value_map.insert(param.name.clone(), value);

            if param.ty != IRType::Void {
                self.variable_types
                    .insert(param.name.clone(), param.ty.clone());

                if let IRType::Struct { name, .. } = &param.ty {
                    self.struct_var_map
                        .insert(param.name.clone(), (value, name.clone()));
                }

                if let IRType::Array { element_type, size } = &param.ty {
                    self.array_map.insert(
                        param.name.clone(),
                        ArrayInfo {
                            ptr: value,
                            element_type: element_type.as_ref().clone(),
                            size: *size,
                        },
                    );
                }
            }
        }

        // Allocate slots for variables that are assigned inside the body,
        // typed from syntactic hints (see the function-level pre-pass).
        let mut slot_hints: std::collections::HashMap<String, IRType> = params
            .iter()
            .filter(|param| param.ty != IRType::Void)
            .map(|param| (param.name.clone(), param.ty.clone()))
            .collect();
        let assigned_vars = {
            let assigned =
                self.find_assigned_variables_with_types(&method.body.statements, &mut slot_hints);
            let mut names: Vec<String> = assigned.into_iter().collect();
            names.sort();
            names
        };
        for var_name in &assigned_vars {
            let slot_type = slot_hints.get(var_name).cloned().unwrap_or(IRType::Int);
            let alloca_value = self.builder.build_alloca(&mut ir_func, slot_type);
            self.alloca_map.insert(var_name.clone(), alloca_value);
        }

        self.current_function = Some(ir_func.clone());
        self.current_function_return_annotation = method.return_type.clone();
        let saved_async_output = self.current_async_output_type.clone();
        let saved_async_poll = self.lowering_async_poll;
        self.lowering_async_poll = method.is_async;
        if method.is_async {
            self.current_async_output_type = None;
        }

        // Lower the body; support implicit returns (last expression = return value).
        let mut implicit_return_value = None;
        if let Some(last_stmt) = method.body.statements.last() {
            if let StatementKind::Expression(expr) = &last_stmt.kind {
                if method.body.statements.len() > 1 {
                    for stmt in &method.body.statements[..method.body.statements.len() - 1] {
                        self.lower_statement(stmt, &mut ir_func);
                    }
                }
                let last_type = self.infer_expr_ir_type(expr);
                let last_value = self.lower_expression(expr, &mut ir_func);
                if body_return_type != IRType::Void {
                    self.emit_escape_for_value(last_value, &last_type, &mut ir_func);
                    implicit_return_value = Some(last_value);
                }
                let mut skipped_names = HashSet::new();
                Self::collect_moved_identifiers(expr, &mut skipped_names);
                self.emit_scope_drops(&mut ir_func, &skipped_names);
            } else {
                self.lower_block(&method.body.statements, &mut ir_func);
            }
        } else {
            self.lower_block(&method.body.statements, &mut ir_func);
        }

        // Seal the current block with a return terminator if one is missing.
        if let Some(current_block_id) = self.builder.get_current_block() {
            let needs_terminator = ir_func
                .get_block(current_block_id)
                .map(|block| block.terminator.is_none())
                .unwrap_or(false);
            if needs_terminator {
                if let Some(block) = ir_func.get_block_mut(current_block_id) {
                    block.set_terminator(Terminator::Return {
                        value: implicit_return_value,
                    });
                }
            }
        }

        self.current_async_output_type = saved_async_output;
        self.lowering_async_poll = saved_async_poll;
        if method.is_async {
            return self.finish_async_coroutine(ir_func, params, body_return_type);
        }

        ir_func
    }

    pub(crate) fn lower_type_annotation_with_self(
        &self,
        annotation: &TypeAnnotation,
        self_type_name: &str,
    ) -> IRType {
        match &annotation.kind {
            TypeAnnotationKind::Simple { segments }
                if segments.len() == 1 && segments[0] == "Self" =>
            {
                if let Some(fields) = self.struct_definitions.get(self_type_name) {
                    IRType::Struct {
                        name: self_type_name.to_string(),
                        fields: fields.clone(),
                    }
                } else if let Some(variants) = self.enum_definitions.get(self_type_name) {
                    let simplified = variants
                        .iter()
                        .map(|(variant_name, _, data)| (variant_name.clone(), data.clone()))
                        .collect();
                    IRType::Enum {
                        name: self_type_name.to_string(),
                        variants: simplified,
                    }
                } else {
                    IRType::Struct {
                        name: self_type_name.to_string(),
                        fields: vec![],
                    }
                }
            }
            _ => self.lower_type_annotation(annotation),
        }
    }

    /// Lower a lambda expression into a self-contained top-level IR function.
    ///
    /// Saves and restores all per-function state so nested lambdas work correctly.
    /// Returns the generated IR function; the caller must queue it in `pending_lambdas`.
    pub(crate) fn lower_lambda(
        &mut self,
        name: String,
        captures: &[ClosureCapture],
        params: &[spectra_compiler::ast::LambdaParam],
        body: &Expression,
        is_async: bool,
    ) -> IRFunction {
        use crate::ir::Parameter;

        // Slot 0 is the hidden closure environment handle. User-visible
        // parameters start at slot 1 and keep the public `fn(...) -> ...` type.
        let mut ir_params: Vec<Parameter> = vec![Parameter {
            id: 0,
            name: "__closure_env".to_string(),
            ty: IRType::Int,
        }];
        ir_params.extend(params.iter().enumerate().map(|(idx, p)| {
            Parameter {
                id: idx + 1,
                name: p.name.clone(),
                ty: p
                    .ty
                    .as_ref()
                    .map(|t| self.lower_type_annotation(t))
                    .unwrap_or(IRType::Unknown),
            }
        }));

        // --- Save outer function state ---
        let saved_value_map = self.value_map.clone();
        let saved_variable_types = self.variable_types.clone();
        let saved_alloca_map = std::mem::take(&mut self.alloca_map);
        let saved_array_map = self.array_map.clone();
        let saved_async_output = self.current_async_output_type.clone();
        let saved_async_poll = self.lowering_async_poll;
        self.lowering_async_poll = is_async;
        self.current_async_output_type = None;
        let saved_current_function = self.current_function.take();
        let saved_struct_var_map = self.struct_var_map.clone();
        let saved_builder_block = self.builder.get_current_block();

        // --- Reset state for lambda body ---
        self.value_map.clear();
        self.variable_types.clear();
        self.alloca_map = HashMap::new();
        self.array_map.clear();
        self.range_map.clear();
        self.struct_var_map.clear();
        self.drop_excluded_names.clear();

        // Seed the type environment before inferring the body.  Lambda
        // parameters and captures are not part of the enclosing function's
        // maps; inferring first therefore turned ordinary expressions such as
        // `value * 2` into `Unknown` and poisoned the generated lambda.
        for capture in captures {
            self.variable_types
                .insert(capture.name.clone(), capture.ty.clone());
        }
        for param in ir_params.iter().skip(1) {
            if param.ty != IRType::Void {
                self.variable_types
                    .insert(param.name.clone(), param.ty.clone());
            }
        }

        // Infer the return type from the body expression after the local
        // lambda environment has been installed.
        let return_type = self.infer_expr_ir_type(body);

        // Register the function so recursive/forward references resolve correctly
        self.function_return_types
            .insert(name.clone(), return_type.clone());

        let mut lambda_func = IRFunction::new(&name, ir_params.clone(), return_type.clone());
        let entry_block = lambda_func.add_block("entry");
        self.builder.set_current_block(entry_block);

        let env_value = Value { id: 0 };
        self.value_map
            .insert("__closure_env".to_string(), env_value);
        self.variable_types
            .insert("__closure_env".to_string(), IRType::Int);

        for (slot, capture) in captures.iter().enumerate() {
            let index = self
                .builder
                .build_const_int(&mut lambda_func, (slot + 1) as i64);
            let ptr =
                self.builder
                    .build_getelementptr(&mut lambda_func, env_value, index, IRType::Int);
            let value = self
                .builder
                .build_load_typed(&mut lambda_func, ptr, capture.ty.clone());
            self.value_map.insert(capture.name.clone(), value);
        }

        // Map explicit parameters into value/type maps
        for param in ir_params.iter().skip(1) {
            let value = Value { id: param.id };
            self.value_map.insert(param.name.clone(), value);
            if param.ty != IRType::Void {
                self.variable_types
                    .insert(param.name.clone(), param.ty.clone());
            }
        }

        // Pre-allocate mutable slots for any variables assigned inside the
        // body, typed from syntactic hints (see the function-level pre-pass).
        let mut lambda_slot_hints: std::collections::HashMap<String, IRType> = ir_params
            .iter()
            .skip(1)
            .filter(|param| param.ty != IRType::Void)
            .map(|param| (param.name.clone(), param.ty.clone()))
            .collect();
        let assigned_vars = if let ExpressionKind::Block(block) = &body.kind {
            let assigned =
                self.find_assigned_variables_with_types(&block.statements, &mut lambda_slot_hints);
            let mut names: Vec<String> = assigned.into_iter().collect();
            names.sort();
            names
        } else {
            Vec::new()
        };
        for var_name in &assigned_vars {
            let slot_type = lambda_slot_hints
                .get(var_name)
                .cloned()
                .unwrap_or(IRType::Int);
            let alloca_value = self.builder.build_alloca(&mut lambda_func, slot_type);
            self.alloca_map.insert(var_name.clone(), alloca_value);
        }

        // Lower the body
        let result_value = self.lower_expression(body, &mut lambda_func);

        // Emit return if the current block has no terminator yet
        if let Some(cur_block_id) = self.builder.get_current_block() {
            if let Some(block) = lambda_func.get_block_mut(cur_block_id) {
                if block.terminator.is_none() {
                    if return_type != IRType::Void {
                        block.set_terminator(Terminator::Return {
                            value: Some(result_value),
                        });
                    } else {
                        block.set_terminator(Terminator::Return { value: None });
                    }
                }
            }
        }

        // --- Restore outer function state ---
        self.value_map = saved_value_map;
        self.variable_types = saved_variable_types;
        self.alloca_map = saved_alloca_map;
        self.array_map = saved_array_map;
        self.struct_var_map = saved_struct_var_map;
        self.current_function = saved_current_function;
        self.current_async_output_type = saved_async_output;
        self.lowering_async_poll = saved_async_poll;
        if let Some(block_id) = saved_builder_block {
            self.builder.set_current_block(block_id);
        }
        if is_async {
            return self.finish_async_coroutine(lambda_func, ir_params, return_type);
        }
        lambda_func
    }

    pub(crate) fn build_closure_object(
        &mut self,
        ir_func: &mut IRFunction,
        lambda_name: String,
        captures: &[ClosureCapture],
    ) -> Value {
        let slots = captures.len() + 1;
        let closure_ty = IRType::Array {
            element_type: Box::new(IRType::Int),
            size: slots,
        };
        let closure_handle = self.builder.build_alloca(ir_func, closure_ty);

        let code_ptr = self.builder.build_func_addr(ir_func, lambda_name);
        let zero = self.builder.build_const_int(ir_func, 0);
        let code_slot =
            self.builder
                .build_getelementptr(ir_func, closure_handle, zero, IRType::Int);
        self.builder.build_store(ir_func, code_slot, code_ptr);

        for (idx, capture) in captures.iter().enumerate() {
            let capture_value = self.lower_identifier_value(&capture.name, ir_func);
            let slot_index = self.builder.build_const_int(ir_func, (idx + 1) as i64);
            let slot =
                self.builder
                    .build_getelementptr(ir_func, closure_handle, slot_index, IRType::Int);
            self.builder.build_store(ir_func, slot, capture_value);
        }

        closure_handle
    }
}
