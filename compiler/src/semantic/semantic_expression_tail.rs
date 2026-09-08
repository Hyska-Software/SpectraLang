use super::*;

impl SemanticAnalyzer {
    pub(crate) fn analyze_expression_tail(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::CharLiteral(_) => {
                // char literal is always valid
            }
            ExpressionKind::FString(parts) => {
                for part in parts {
                    if let FStringPart::Interpolated(expr) = part {
                        self.analyze_expression(expr);
                    }
                }
            }
            ExpressionKind::Lambda {
                is_async,
                params,
                body,
            } => {
                // Closures get a fresh flow-state frame (E034): captured
                // handles are analyzed by value at the capture site, not
                // tracked across the closure boundary.
                let previous_uaf = self.uaf_enter_function();
                let captured = self.collect_lambda_capture_names(params, body);
                let mut mutated = Vec::new();
                Self::collect_assigned_names_in_expression(body, &mut mutated);
                for name in mutated {
                    if captured.contains(&name) {
                        self.error(
                            format!(
                                "Cannot assign to captured variable '{}' inside closure; captures are by value",
                                name
                            ),
                            body.span,
                        );
                    }
                }
                self.push_scope();
                let expected_params = match self.current_expected_type.as_ref() {
                    Some(Type::Fn { params, .. }) => Some(params.clone()),
                    _ => None,
                };
                for (index, p) in params.iter().enumerate() {
                    let annotated = self.type_annotation_to_type(&p.ty);
                    let ty = if matches!(annotated, Type::Unknown) {
                        expected_params
                            .as_ref()
                            .and_then(|types| types.get(index))
                            .cloned()
                            .unwrap_or(annotated)
                    } else {
                        annotated
                    };
                    self.uaf_on_bind(&p.name, &ty);
                    self.declare_symbol(p.name.clone(), p.span, ty);
                }
                let expected_output = match self.current_expected_type.as_ref() {
                    Some(Type::Fn { return_type, .. }) if *is_async => match return_type.as_ref() {
                        Type::Task { output } => Some(output.as_ref().clone()),
                        _ => None,
                    },
                    Some(Type::Fn { return_type, .. }) => Some(return_type.as_ref().clone()),
                    _ => None,
                };
                let previous_return = self
                    .current_return_type
                    .replace(expected_output.unwrap_or(Type::Unknown));
                let previous_async_depth = self.async_context_depth;
                if *is_async {
                    self.async_context_depth += 1;
                }
                self.analyze_expression(body);
                self.async_context_depth = previous_async_depth;
                self.current_return_type = previous_return;
                self.pop_scope();
                self.uaf_restore(previous_uaf);

                if *is_async {
                    self.validate_async_send_sync_lambda(params, body, expr.span);
                }
            }
            ExpressionKind::Try(inner) => {
                self.analyze_expression(inner);
            }
            ExpressionKind::Await(inner) => {
                if self.async_context_depth == 0 {
                    self.error("`await` is only valid inside an async context", expr.span);
                }
                self.analyze_expression(inner);
                let task_type = self.infer_expression_type(inner);
                if !matches!(task_type, Type::Task { .. } | Type::Unknown) {
                    self.error(
                        format!("`await` expects Task<T>, found {}", type_name(&task_type)),
                        expr.span,
                    );
                }
            }
            ExpressionKind::Cast {
                expr: inner,
                target_type,
                mode,
                ..
            } => {
                self.analyze_expression(inner);
                let from_ty = self.infer_expression_type(inner);
                let to_ty = self.type_annotation_to_type(&Some(target_type.clone()));

                if matches!(from_ty, Type::Unknown) {
                    self.error_with_hint(
                        "Cannot cast an expression with unknown type",
                        expr.span,
                        "Add an explicit type annotation before casting.",
                    );
                }

                // Validate cast legality
                let mut valid = is_cast_valid(&from_ty, &to_ty);

                if *mode == CastMode::Wrapping
                    && (matches!(from_ty, Type::Float | Type::ExactFloat { .. })
                        || matches!(to_ty, Type::Float | Type::ExactFloat { .. }))
                {
                    self.error_coded(
                        "E2903",
                        "wrapping casts are only defined for integer-to-integer conversions",
                        expr.span,
                    );
                    valid = false;
                }

                // For dyn Trait casts, verify that the concrete type actually implements the trait.
                if let (
                    Type::Struct { name: struct_name } | Type::Applied { name: struct_name, .. },
                    Type::DynTrait {
                        trait_name,
                        auto_traits,
                    },
                ) = (&from_ty, &to_ty)
                {
                    let lookup_name = self
                        .nominal_lookup_name(&from_ty)
                        .unwrap_or_else(|| struct_name.clone());
                    // R-213: dyn of a generic trait requires fixed type arguments,
                    // which are not supported yet.
                    if self
                        .trait_type_params
                        .get(trait_name)
                        .map(|params| !params.is_empty())
                        .unwrap_or(false)
                    {
                        valid = false;
                        self.error_coded(
                            "E026",
                            format!(
                                "Cannot cast `{}` to `dyn {}`: generic traits require fixed type arguments for object safety (e.g. `dyn Trait<int>`)",
                                struct_name, trait_name
                            ),
                            expr.span,
                        );
                    } else if !self
                        .trait_impls
                        .contains_key(&(trait_name.clone(), lookup_name.clone()))
                    {
                        valid = false;
                        self.error_coded(
                            "E022",
                            format!(
                                "Cannot cast `{}` to `dyn {}`: type `{}` does not implement trait `{}`",
                                struct_name, trait_name, struct_name, trait_name
                            ),
                            expr.span,
                        );
                    }
                    for bound in auto_traits {
                        if !self.type_satisfies_trait_bound(&from_ty, bound) {
                            valid = false;
                            self.error_coded_with_hint(
                                "E2104",
                                format!(
                                    "Cannot cast `{}` to `dyn {} + {}`: type `{}` does not satisfy `{}`",
                                    struct_name, trait_name, bound, struct_name, bound
                                ),
                                expr.span,
                                format!(
                                    "Add a formal `{}` implementation/evidence for `{}` before creating this dyn object.",
                                    bound, struct_name
                                ),
                            );
                        }
                    }
                }

                if !valid {
                    self.error(
                        format!(
                            "Cannot cast from `{}` to `{}`; valid casts: numeric↔numeric, int↔char, and `T as dyn Trait` where T implements Trait",
                            type_name(&from_ty),
                            type_name(&to_ty)
                        ),
                        expr.span,
                    );
                }
            }
            ExpressionKind::Range { start, end, .. } => {
                self.analyze_expression(start);
                self.analyze_expression(end);
            }
            ExpressionKind::Block(block) => {
                self.push_scope();
                for stmt in &block.statements {
                    self.analyze_statement(stmt);
                }
                self.pop_scope();
            }
            ExpressionKind::DifferentiableBlock(block) => {
                self.push_scope();
                for stmt in &block.statements {
                    self.analyze_statement(stmt);
                }
                self.pop_scope();
                self.validate_differentiable_block_operations(block);

                let result_type = self.infer_block_type(block);
                if !matches!(result_type, Type::Tensor { .. } | Type::Unknown) {
                    self.error_with_hint(
                        format!(
                            "Differentiable block must produce a Tensor loss, found {}",
                            type_name(&result_type)
                        ),
                        expr.span,
                        "Return the tensor loss as the final expression of the `diff { ... }` block.",
                    );
                }
            }
            ExpressionKind::AsyncBlock(block) => {
                self.push_scope();
                self.async_context_depth += 1;
                for stmt in &block.statements {
                    self.analyze_statement(stmt);
                }
                self.async_context_depth = self.async_context_depth.saturating_sub(1);
                let output_type = self.infer_block_type(block);
                self.pop_scope();
                self.symbol_resolutions.insert(
                    expr.span,
                    SymbolInfo {
                        is_local: false,
                        def_span: None,
                        ty: Type::Task {
                            output: Box::new(output_type),
                        },
                    },
                );
            }
            _ => unreachable!("expression category mismatch"),
        }
    }

    fn collect_assigned_names_in_expression(expr: &Expression, assigned: &mut Vec<String>) {
        match &expr.kind {
            ExpressionKind::Block(block) | ExpressionKind::AsyncBlock(block) => {
                for stmt in &block.statements {
                    Self::collect_assigned_names_in_statement(stmt, assigned);
                }
            }
            ExpressionKind::If {
                then_block,
                elif_blocks,
                else_block,
                ..
            } => {
                for stmt in &then_block.statements {
                    Self::collect_assigned_names_in_statement(stmt, assigned);
                }
                for (_, block) in elif_blocks {
                    for stmt in &block.statements {
                        Self::collect_assigned_names_in_statement(stmt, assigned);
                    }
                }
                if let Some(block) = else_block {
                    for stmt in &block.statements {
                        Self::collect_assigned_names_in_statement(stmt, assigned);
                    }
                }
            }
            _ => {}
        }
    }

    fn collect_assigned_names_in_statement(stmt: &Statement, assigned: &mut Vec<String>) {
        match &stmt.kind {
            StatementKind::Assignment(assign) => {
                if let crate::ast::LValue::Identifier(name) = &assign.target {
                    assigned.push(name.clone());
                }
                Self::collect_assigned_names_in_expression(&assign.value, assigned);
            }
            StatementKind::Let(let_stmt) => {
                if let Some(value) = &let_stmt.value {
                    Self::collect_assigned_names_in_expression(value, assigned);
                }
            }
            StatementKind::Return(ret) => {
                if let Some(value) = &ret.value {
                    Self::collect_assigned_names_in_expression(value, assigned);
                }
            }
            StatementKind::Expression(expr) => {
                Self::collect_assigned_names_in_expression(expr, assigned);
            }
            StatementKind::While(loop_stmt) => {
                for stmt in &loop_stmt.body.statements {
                    Self::collect_assigned_names_in_statement(stmt, assigned);
                }
            }
            StatementKind::DoWhile(loop_stmt) => {
                for stmt in &loop_stmt.body.statements {
                    Self::collect_assigned_names_in_statement(stmt, assigned);
                }
            }
            StatementKind::For(for_loop) => {
                for stmt in &for_loop.body.statements {
                    Self::collect_assigned_names_in_statement(stmt, assigned);
                }
            }
            StatementKind::IfLet(stmt) => {
                for stmt in &stmt.then_block.statements {
                    Self::collect_assigned_names_in_statement(stmt, assigned);
                }
                if let Some(block) = &stmt.else_block {
                    for stmt in &block.statements {
                        Self::collect_assigned_names_in_statement(stmt, assigned);
                    }
                }
            }
            StatementKind::WhileLet(stmt) => {
                for stmt in &stmt.body.statements {
                    Self::collect_assigned_names_in_statement(stmt, assigned);
                }
            }
            StatementKind::Loop(loop_stmt) => {
                for stmt in &loop_stmt.body.statements {
                    Self::collect_assigned_names_in_statement(stmt, assigned);
                }
            }
            StatementKind::Switch(switch_stmt) => {
                for case in &switch_stmt.cases {
                    for stmt in &case.body.statements {
                        Self::collect_assigned_names_in_statement(stmt, assigned);
                    }
                }
                if let Some(block) = &switch_stmt.default {
                    for stmt in &block.statements {
                        Self::collect_assigned_names_in_statement(stmt, assigned);
                    }
                }
            }
            StatementKind::Break | StatementKind::Continue => {}
        }
    }
}
