use super::*;

impl ASTLowering {
    pub(crate) fn lower_closure_handle_call(
        &mut self,
        closure_handle: Value,
        mut arg_values: Vec<Value>,
        public_params: Vec<IRType>,
        public_return: IRType,
        ir_func: &mut IRFunction,
    ) -> Value {
        let zero = self.builder.build_const_int(ir_func, 0);
        let code_slot =
            self.builder
                .build_getelementptr(ir_func, closure_handle, zero, IRType::Int);
        let code_ptr = self.builder.build_load(ir_func, code_slot);

        let mut call_args = Vec::with_capacity(arg_values.len() + 1);
        call_args.push(closure_handle);
        call_args.append(&mut arg_values);

        let mut signature_params = Vec::with_capacity(public_params.len() + 1);
        signature_params.push(IRType::Int);
        signature_params.extend(public_params);

        let result = self.builder.build_call_indirect(
            ir_func,
            code_ptr,
            call_args,
            signature_params,
            public_return.clone(),
        );
        if public_return == IRType::Void {
            result.unwrap_or_else(|| self.builder.build_const_int(ir_func, 0))
        } else {
            self.require_value(result, "closure call did not produce its declared return value")
        }
    }

    pub(crate) fn lower_identifier_value(&mut self, name: &str, ir_func: &mut IRFunction) -> Value {
        if let Some(value) = self.const_values.get(name).cloned() {
            self.emit_const_value(&value, ir_func)
        } else if let Some(value) = self.lower_global_value(name, ir_func) {
            value
        } else if let Some(info) = self.array_map.get(name) {
            info.ptr
        } else if let Some((struct_ptr, _)) = self.struct_var_map.get(name) {
            struct_ptr
        } else if let Some(&alloca_ptr) = self.alloca_map.get(name) {
            self.builder.build_load(ir_func, alloca_ptr)
        } else if let Some(value) = self.value_map.get(name) {
            value
        } else if self.function_parameter_types.contains_key(name)
            && self.function_return_types.contains_key(name)
        {
            self.lower_named_function_value(name, ir_func)
        } else {
            self.invalid_value(format!("unresolved identifier '{}' during lowering", name))
        }
    }

    /// Materializes a named function as a zero-capture closure. Regular
    /// functions use the public ABI, while closure callbacks use
    /// `fn(env, args...)`; this adapter keeps both contracts explicit and
    /// lets named functions cross host-call boundaries such as API handlers.
    pub(crate) fn lower_named_function_value(&mut self, name: &str, ir_func: &mut IRFunction) -> Value {
        let Some(public_params) = self.function_parameter_types.get(name).cloned() else {
            return self.invalid_value(format!("unknown function value '{}'", name));
        };
        let Some(return_type) = self.function_return_types.get(name).cloned() else {
            return self.invalid_value(format!("unknown function return type '{}'", name));
        };

        let label: String = name
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    ch
                } else {
                    '_'
                }
            })
            .collect();
        let wrapper_name = format!("__function_value_{}_{}", label, self.lambda_counter);
        self.lambda_counter = self.lambda_counter.saturating_add(1);

        let mut params = Vec::with_capacity(public_params.len() + 1);
        params.push(crate::ir::Parameter {
            id: 0,
            name: "__closure_env".to_string(),
            ty: IRType::Int,
        });
        params.extend(public_params.iter().enumerate().map(|(index, ty)| {
            crate::ir::Parameter {
                id: index + 1,
                name: format!("arg{index}"),
                ty: ty.clone(),
            }
        }));

        let mut wrapper = IRFunction::new(&wrapper_name, params, return_type.clone());
        let entry = wrapper.add_block("entry");
        let saved_block = self.builder.get_current_block();
        self.builder.set_current_block(entry);
        let args = (1..=public_params.len())
            .map(|id| Value { id })
            .collect::<Vec<_>>();
        let result = self.builder.build_call(
            &mut wrapper,
            name.to_string(),
            args,
            return_type != IRType::Void,
        );
        if let Some(block) = wrapper.get_block_mut(entry) {
            block.set_terminator(crate::ir::Terminator::Return { value: result });
        }
        if let Some(block) = saved_block {
            self.builder.set_current_block(block);
        }
        self.pending_lambdas.push(wrapper);

        self.build_closure_object(ir_func, wrapper_name, &[])
    }

    pub(crate) fn lower_global_value(&mut self, name: &str, ir_func: &mut IRFunction) -> Option<Value> {
        let (global_key, ty) = self.static_globals.get(name)?.clone();
        let ptr = self
            .builder
            .build_global_addr(ir_func, global_key, ty.clone());
        Some(self.builder.build_load_typed(ir_func, ptr, ty))
    }

    pub(crate) fn collect_lambda_captures(
        &self,
        params: &[spectra_compiler::ast::LambdaParam],
        body: &Expression,
    ) -> Vec<ClosureCapture> {
        let mut locals: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
        let mut captures = Vec::new();
        let mut seen = HashSet::new();
        self.collect_lambda_captures_expr(body, &mut locals, &mut captures, &mut seen);
        captures
    }

    pub(crate) fn collect_lambda_captures_expr(
        &self,
        expr: &Expression,
        locals: &mut HashSet<String>,
        captures: &mut Vec<ClosureCapture>,
        seen: &mut HashSet<String>,
    ) {
        match &expr.kind {
            ExpressionKind::Identifier(name) => {
                if !locals.contains(name) && seen.insert(name.clone()) {
                    if let Some(ty) = self.variable_types.get(name) {
                        captures.push(ClosureCapture {
                            name: name.clone(),
                            ty,
                        });
                    }
                }
            }
            ExpressionKind::Binary { left, right, .. } => {
                self.collect_lambda_captures_expr(left, locals, captures, seen);
                self.collect_lambda_captures_expr(right, locals, captures, seen);
            }
            ExpressionKind::Unary { operand, .. }
            | ExpressionKind::Try(operand)
            | ExpressionKind::Await(operand) => {
                self.collect_lambda_captures_expr(operand, locals, captures, seen);
            }
            ExpressionKind::Call { callee, arguments } => {
                self.collect_lambda_captures_expr(callee, locals, captures, seen);
                for arg in arguments {
                    self.collect_lambda_captures_expr(arg, locals, captures, seen);
                }
            }
            ExpressionKind::MethodCall {
                object, arguments, ..
            } => {
                self.collect_lambda_captures_expr(object, locals, captures, seen);
                for arg in arguments {
                    self.collect_lambda_captures_expr(arg, locals, captures, seen);
                }
            }
            ExpressionKind::Lambda {
                params: nested_params,
                body,
                ..
            } => {
                let mut nested_locals = locals.clone();
                for param in nested_params {
                    nested_locals.insert(param.name.clone());
                }
                self.collect_lambda_captures_expr(body, &mut nested_locals, captures, seen);
            }
            ExpressionKind::Block(block) => {
                let mut block_locals = locals.clone();
                for stmt in &block.statements {
                    self.collect_lambda_captures_stmt(stmt, &mut block_locals, captures, seen);
                }
            }
            ExpressionKind::DifferentiableBlock(block) | ExpressionKind::AsyncBlock(block) => {
                let mut block_locals = locals.clone();
                for stmt in &block.statements {
                    self.collect_lambda_captures_stmt(stmt, &mut block_locals, captures, seen);
                }
            }
            ExpressionKind::If {
                condition,
                then_block,
                elif_blocks,
                else_block,
            } => {
                self.collect_lambda_captures_expr(condition, locals, captures, seen);
                self.collect_lambda_captures_block(then_block, locals, captures, seen);
                for (elif_condition, elif_block) in elif_blocks {
                    self.collect_lambda_captures_expr(elif_condition, locals, captures, seen);
                    self.collect_lambda_captures_block(elif_block, locals, captures, seen);
                }
                if let Some(block) = else_block {
                    self.collect_lambda_captures_block(block, locals, captures, seen);
                }
            }
            ExpressionKind::Unless {
                condition,
                then_block,
                else_block,
            } => {
                self.collect_lambda_captures_expr(condition, locals, captures, seen);
                self.collect_lambda_captures_block(then_block, locals, captures, seen);
                if let Some(block) = else_block {
                    self.collect_lambda_captures_block(block, locals, captures, seen);
                }
            }
            ExpressionKind::Grouping(inner) => {
                self.collect_lambda_captures_expr(inner, locals, captures, seen);
            }
            ExpressionKind::FieldAccess { object, .. } => {
                self.collect_lambda_captures_expr(object, locals, captures, seen);
            }
            ExpressionKind::TupleAccess { tuple, .. } => {
                self.collect_lambda_captures_expr(tuple, locals, captures, seen);
            }
            ExpressionKind::IndexAccess { array, index } => {
                self.collect_lambda_captures_expr(array, locals, captures, seen);
                self.collect_lambda_captures_expr(index, locals, captures, seen);
            }
            ExpressionKind::ArrayLiteral { elements }
            | ExpressionKind::TupleLiteral { elements } => {
                for element in elements {
                    self.collect_lambda_captures_expr(element, locals, captures, seen);
                }
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                for (_, value) in fields {
                    self.collect_lambda_captures_expr(value, locals, captures, seen);
                }
            }
            ExpressionKind::EnumVariant {
                data, struct_data, ..
            } => {
                if let Some(values) = data {
                    for value in values {
                        self.collect_lambda_captures_expr(value, locals, captures, seen);
                    }
                }
                if let Some(fields) = struct_data {
                    for (_, value) in fields {
                        self.collect_lambda_captures_expr(value, locals, captures, seen);
                    }
                }
            }
            ExpressionKind::Match { scrutinee, arms } => {
                self.collect_lambda_captures_expr(scrutinee, locals, captures, seen);
                for arm in arms {
                    let mut arm_locals = locals.clone();
                    Self::collect_pattern_names(&arm.pattern, &mut arm_locals);
                    if let Some(guard) = &arm.guard {
                        self.collect_lambda_captures_expr(guard, &mut arm_locals, captures, seen);
                    }
                    self.collect_lambda_captures_expr(&arm.body, &mut arm_locals, captures, seen);
                }
            }
            ExpressionKind::Cast { expr, .. } => {
                self.collect_lambda_captures_expr(expr, locals, captures, seen);
            }
            ExpressionKind::FString(parts) => {
                for part in parts {
                    if let FStringPart::Interpolated(expr) = part {
                        self.collect_lambda_captures_expr(expr, locals, captures, seen);
                    }
                }
            }
            ExpressionKind::Range { start, end, .. } => {
                self.collect_lambda_captures_expr(start, locals, captures, seen);
                self.collect_lambda_captures_expr(end, locals, captures, seen);
            }
            ExpressionKind::NumberLiteral(_)
            | ExpressionKind::StringLiteral(_)
            | ExpressionKind::BoolLiteral(_)
            | ExpressionKind::CharLiteral(_) => {}
        }
    }

    pub(crate) fn collect_lambda_captures_block(
        &self,
        block: &Block,
        locals: &HashSet<String>,
        captures: &mut Vec<ClosureCapture>,
        seen: &mut HashSet<String>,
    ) {
        let mut block_locals = locals.clone();
        for stmt in &block.statements {
            self.collect_lambda_captures_stmt(stmt, &mut block_locals, captures, seen);
        }
    }

    pub(crate) fn collect_lambda_captures_stmt(
        &self,
        stmt: &Statement,
        locals: &mut HashSet<String>,
        captures: &mut Vec<ClosureCapture>,
        seen: &mut HashSet<String>,
    ) {
        match &stmt.kind {
            StatementKind::Let(let_stmt) => {
                if let Some(value) = &let_stmt.value {
                    self.collect_lambda_captures_expr(value, locals, captures, seen);
                }
                Self::collect_pattern_names(&let_stmt.pattern, locals);
            }
            StatementKind::Assignment(assign) => {
                self.collect_lvalue_captures(&assign.target, locals, captures, seen);
                self.collect_lambda_captures_expr(&assign.value, locals, captures, seen);
            }
            StatementKind::Return(ret) => {
                if let Some(value) = &ret.value {
                    self.collect_lambda_captures_expr(value, locals, captures, seen);
                }
            }
            StatementKind::Expression(expr) => {
                self.collect_lambda_captures_expr(expr, locals, captures, seen);
            }
            StatementKind::While(loop_stmt) => {
                self.collect_lambda_captures_expr(&loop_stmt.condition, locals, captures, seen);
                self.collect_lambda_captures_block(&loop_stmt.body, locals, captures, seen);
            }
            StatementKind::DoWhile(loop_stmt) => {
                self.collect_lambda_captures_block(&loop_stmt.body, locals, captures, seen);
                self.collect_lambda_captures_expr(&loop_stmt.condition, locals, captures, seen);
            }
            StatementKind::For(for_loop) => {
                self.collect_lambda_captures_expr(&for_loop.iterable, locals, captures, seen);
                let mut loop_locals = locals.clone();
                loop_locals.insert(for_loop.iterator.clone());
                self.collect_lambda_captures_block(&for_loop.body, &loop_locals, captures, seen);
            }
            StatementKind::IfLet(stmt) => {
                self.collect_lambda_captures_expr(&stmt.value, locals, captures, seen);
                let mut then_locals = locals.clone();
                Self::collect_pattern_names(&stmt.pattern, &mut then_locals);
                self.collect_lambda_captures_block(&stmt.then_block, &then_locals, captures, seen);
                if let Some(block) = &stmt.else_block {
                    self.collect_lambda_captures_block(block, locals, captures, seen);
                }
            }
            StatementKind::WhileLet(stmt) => {
                self.collect_lambda_captures_expr(&stmt.value, locals, captures, seen);
                let mut body_locals = locals.clone();
                Self::collect_pattern_names(&stmt.pattern, &mut body_locals);
                self.collect_lambda_captures_block(&stmt.body, &body_locals, captures, seen);
            }
            StatementKind::Loop(loop_stmt) => {
                self.collect_lambda_captures_block(&loop_stmt.body, locals, captures, seen);
            }
            StatementKind::Switch(switch_stmt) => {
                self.collect_lambda_captures_expr(&switch_stmt.value, locals, captures, seen);
                for case in &switch_stmt.cases {
                    self.collect_lambda_captures_expr(&case.pattern, locals, captures, seen);
                    self.collect_lambda_captures_block(&case.body, locals, captures, seen);
                }
                if let Some(block) = &switch_stmt.default {
                    self.collect_lambda_captures_block(block, locals, captures, seen);
                }
            }
            StatementKind::Break | StatementKind::Continue => {}
        }
    }

    pub(crate) fn collect_lvalue_captures(
        &self,
        target: &spectra_compiler::ast::LValue,
        locals: &mut HashSet<String>,
        captures: &mut Vec<ClosureCapture>,
        seen: &mut HashSet<String>,
    ) {
        match target {
            spectra_compiler::ast::LValue::Identifier(name) => {
                if !locals.contains(name) && seen.insert(name.clone()) {
                    if let Some(ty) = self.variable_types.get(name) {
                        captures.push(ClosureCapture {
                            name: name.clone(),
                            ty,
                        });
                    }
                }
            }
            spectra_compiler::ast::LValue::IndexAccess { array, index } => {
                self.collect_lambda_captures_expr(array, locals, captures, seen);
                self.collect_lambda_captures_expr(index, locals, captures, seen);
            }
            spectra_compiler::ast::LValue::FieldAccess { object, .. } => {
                self.collect_lambda_captures_expr(object, locals, captures, seen);
            }
        }
    }

    pub(crate) fn collect_pattern_names(
        pattern: &spectra_compiler::ast::Pattern,
        names: &mut HashSet<String>,
    ) {
        match pattern {
            spectra_compiler::ast::Pattern::Identifier(name, _) => {
                names.insert(name.clone());
            }
            spectra_compiler::ast::Pattern::Tuple(items) => {
                for item in items {
                    Self::collect_pattern_names(item, names);
                }
            }
            spectra_compiler::ast::Pattern::Struct { fields, .. } => {
                for (_, pattern) in fields {
                    Self::collect_pattern_names(pattern, names);
                }
            }
            spectra_compiler::ast::Pattern::EnumVariant {
                data, struct_data, ..
            } => {
                if let Some(items) = data {
                    for item in items {
                        Self::collect_pattern_names(item, names);
                    }
                }
                if let Some(fields) = struct_data {
                    for (_, item) in fields {
                        Self::collect_pattern_names(item, names);
                    }
                }
            }
            spectra_compiler::ast::Pattern::Or(patterns) => {
                for pattern in patterns {
                    Self::collect_pattern_names(pattern, names);
                }
            }
            spectra_compiler::ast::Pattern::Wildcard(_)
            | spectra_compiler::ast::Pattern::Literal(_) => {}
        }
    }

    pub(crate) fn collect_trait_method_order_recursive(
        &self,
        trait_name: &str,
        seen: &mut HashSet<String>,
        out: &mut Vec<String>,
    ) {
        let Some(trait_decl) = self.trait_declarations.get(trait_name) else {
            return;
        };

        for parent_trait in &trait_decl.parent_traits {
            self.collect_trait_method_order_recursive(parent_trait, seen, out);
        }

        for method in &trait_decl.methods {
            if seen.insert(method.name.clone()) {
                out.push(method.name.clone());
            }
        }
    }

    pub(crate) fn collect_trait_methods_recursive(
        &self,
        trait_name: &str,
        out: &mut HashMap<String, spectra_compiler::ast::TraitMethod>,
    ) {
        let Some(trait_decl) = self.trait_declarations.get(trait_name) else {
            return;
        };

        for parent_trait in &trait_decl.parent_traits {
            self.collect_trait_methods_recursive(parent_trait, out);
        }

        // Insertion at the child level deliberately replaces an inherited
        // signature when a child trait overrides a method.
        for method in &trait_decl.methods {
            out.insert(method.name.clone(), method.clone());
        }
    }

    pub(crate) fn collect_default_trait_methods(
        &self,
        trait_name: &str,
        explicit_methods: &[ASTMethod],
    ) -> Vec<ASTMethod> {
        let mut seen: std::collections::HashSet<String> =
            explicit_methods.iter().map(|m| m.name.clone()).collect();
        let mut out = Vec::new();
        self.collect_default_trait_methods_recursive(trait_name, &mut seen, &mut out);
        out
    }

    pub(crate) fn collect_default_trait_methods_recursive(
        &self,
        trait_name: &str,
        seen: &mut std::collections::HashSet<String>,
        out: &mut Vec<ASTMethod>,
    ) {
        let Some(trait_decl) = self.trait_declarations.get(trait_name) else {
            return;
        };

        for parent_trait in &trait_decl.parent_traits {
            self.collect_default_trait_methods_recursive(parent_trait, seen, out);
        }

        for method in &trait_decl.methods {
            if method.body.is_none() || seen.contains(&method.name) {
                continue;
            }

            seen.insert(method.name.clone());
            out.push(ASTMethod {
                name: method.name.clone(),
                is_async: method.is_async,
                params: method.params.clone(),
                return_type: method.return_type.clone(),
                body: method.body.clone().unwrap_or(Block {
                    span: method.span,
                    statements: Vec::new(),
                }),
                span: method.span,
                visibility: Visibility::Public,
            });
        }
    }

}
