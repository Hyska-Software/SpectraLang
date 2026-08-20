impl SemanticAnalyzer {
    fn analyze_function(&mut self, func: &Function) {
        let pushed_generics = self.push_generic_params(&func.type_params);

        self.current_function = Some(func.name.clone());
        let expected_return = self.type_annotation_to_type_checked(&func.return_type);
        let previous_return = self.current_return_type.replace(expected_return.clone());
        if func.is_async {
            self.async_context_depth += 1;
        }

        self.push_scope();

        // Declare parameters in function scope
        for param in &func.params {
            let param_type = self.type_annotation_to_type_checked(&param.ty);
            if !self.declare_symbol(param.name.clone(), param.span, param_type) {
                self.error(
                    format!("Parameter '{}' is already declared", param.name),
                    param.span,
                );
            }
        }

        // Analyze function body
        self.analyze_block(&func.body);
        self.validate_function_block_return(&func.body, &expected_return, func.span);
        if func.is_async {
            self.validate_async_send_sync_function(func);
        }

        self.pop_scope();
        if func.is_async {
            self.async_context_depth = self.async_context_depth.saturating_sub(1);
        }
        if pushed_generics {
            self.pop_generic_params();
        }
        self.current_function = None;
        self.current_return_type = previous_return;
    }

    fn analyze_block(&mut self, block: &Block) {
        self.push_scope();

        for statement in &block.statements {
            self.analyze_statement(statement);
        }

        self.pop_scope();
    }

    fn validate_async_send_sync_function(&mut self, func: &Function) {
        let mut events = AsyncSendSyncEvents::new();
        for param in &func.params {
            let ty = self.type_annotation_to_type(&param.ty);
            events.env.insert(param.name.clone(), ty.clone());
            let order = events.bump();
            events.locals.push(AsyncLocalEvent {
                name: param.name.clone(),
                ty,
                span: param.span,
                order,
            });
        }

        self.collect_async_send_sync_events_block(&func.body, &mut events);

        for await_event in &events.awaits {
            for local in &events.locals {
                if local.order >= await_event.order || self.type_is_send(&local.ty) {
                    continue;
                }
                let live_after_await = events.uses.iter().any(|use_event| {
                    use_event.name == local.name && use_event.order > await_event.order
                });
                if !live_after_await {
                    continue;
                }

                if self.type_is_ref_cell_family(&local.ty) {
                    self.error_coded_with_details(
                        "E2102",
                        format!(
                            "`{}` has type {} and cannot be held across `await`",
                            local.name,
                            type_name(&local.ty)
                        ),
                        local.span,
                        format!("the value is live across an await at line {}", await_event.span.start_location.line),
                        "Move the value into a shorter block, extract a Send value before await, or use an async-safe synchronization primitive.",
                    );
                } else {
                    self.error_coded_with_details(
                        "E2101",
                        format!(
                            "`{}` has non-Send type {} and is live across `await`",
                            local.name,
                            type_name(&local.ty)
                        ),
                        local.span,
                        format!("the value crosses an await at line {}", await_event.span.start_location.line),
                        "Drop the value before await, clone/send a safe representation, or keep the task on a local executor lane.",
                    );
                }
            }
        }

        for boundary in &events.task_boundaries {
            for name in &boundary.names {
                let Some(ty) = events.env.get(name) else {
                    continue;
                };
                if self.type_is_send(ty) {
                    continue;
                }
                self.error_coded_with_details(
                    "E2103",
                    format!(
                        "`{}` has non-Send type {} and cannot cross a task boundary",
                        name,
                        type_name(ty)
                    ),
                    boundary.span,
                    "spawn-style async APIs require captured values to be Send",
                    "Use a local task API, drop the value before spawning, or replace it with a Send type.",
                );
            }
        }
    }

    fn collect_async_send_sync_events_block(
        &mut self,
        block: &Block,
        events: &mut AsyncSendSyncEvents,
    ) {
        for statement in &block.statements {
            self.collect_async_send_sync_events_statement(statement, events);
        }
    }

    fn collect_async_send_sync_events_statement(
        &mut self,
        statement: &Statement,
        events: &mut AsyncSendSyncEvents,
    ) {
        match &statement.kind {
            StatementKind::Let(let_stmt) => {
                if let Some(value) = &let_stmt.value {
                    self.collect_async_send_sync_events_expression(value, events);
                }
                let declared = let_stmt
                    .ty
                    .as_ref()
                    .map(|ty| self.type_annotation_to_type(&Some(ty.clone())));
                let inferred = let_stmt
                    .value
                    .as_ref()
                    .map(|value| self.infer_async_event_expression_type(value, &events.env));
                let binding_type = declared.or(inferred).unwrap_or(Type::Unknown);
                for name in self.collect_pattern_binding_names(&let_stmt.pattern) {
                    events.env.insert(name.clone(), binding_type.clone());
                    let order = events.bump();
                    events.locals.push(AsyncLocalEvent {
                        name,
                        ty: binding_type.clone(),
                        span: let_stmt.span,
                        order,
                    });
                }
            }
            StatementKind::Assignment(assign_stmt) => {
                self.collect_async_send_sync_events_lvalue(&assign_stmt.target, events);
                self.collect_async_send_sync_events_expression(&assign_stmt.value, events);
            }
            StatementKind::Return(ret) => {
                if let Some(value) = &ret.value {
                    self.collect_async_send_sync_events_expression(value, events);
                }
            }
            StatementKind::Expression(expr) => {
                self.collect_async_send_sync_events_expression(expr, events);
            }
            StatementKind::While(while_loop) => {
                self.collect_async_send_sync_events_expression(&while_loop.condition, events);
                self.collect_async_send_sync_events_block(&while_loop.body, events);
            }
            StatementKind::DoWhile(do_while) => {
                self.collect_async_send_sync_events_block(&do_while.body, events);
                self.collect_async_send_sync_events_expression(&do_while.condition, events);
            }
            StatementKind::For(for_loop) => {
                self.collect_async_send_sync_events_expression(&for_loop.iterable, events);
                events.env.insert(for_loop.iterator.clone(), Type::Unknown);
                let order = events.bump();
                events.locals.push(AsyncLocalEvent {
                    name: for_loop.iterator.clone(),
                    ty: Type::Unknown,
                    span: for_loop.span,
                    order,
                });
                self.collect_async_send_sync_events_block(&for_loop.body, events);
            }
            StatementKind::Loop(loop_stmt) => {
                self.collect_async_send_sync_events_block(&loop_stmt.body, events);
            }
            StatementKind::Switch(switch_stmt) => {
                self.collect_async_send_sync_events_expression(&switch_stmt.value, events);
                for case in &switch_stmt.cases {
                    self.collect_async_send_sync_events_expression(&case.pattern, events);
                    self.collect_async_send_sync_events_block(&case.body, events);
                }
                if let Some(default_body) = &switch_stmt.default {
                    self.collect_async_send_sync_events_block(default_body, events);
                }
            }
            StatementKind::IfLet(stmt) => {
                self.collect_async_send_sync_events_expression(&stmt.value, events);
                self.collect_async_send_sync_events_block(&stmt.then_block, events);
                if let Some(block) = &stmt.else_block {
                    self.collect_async_send_sync_events_block(block, events);
                }
            }
            StatementKind::WhileLet(stmt) => {
                self.collect_async_send_sync_events_expression(&stmt.value, events);
                self.collect_async_send_sync_events_block(&stmt.body, events);
            }
            StatementKind::Break | StatementKind::Continue => {}
        }
    }

    fn collect_async_send_sync_events_lvalue(
        &mut self,
        target: &crate::ast::LValue,
        events: &mut AsyncSendSyncEvents,
    ) {
        match target {
            crate::ast::LValue::Identifier(name) => {
                let order = events.bump();
                events.uses.push(AsyncUseEvent {
                    name: name.clone(),
                    order,
                });
            }
            crate::ast::LValue::IndexAccess { array, index } => {
                self.collect_async_send_sync_events_expression(array, events);
                self.collect_async_send_sync_events_expression(index, events);
            }
            crate::ast::LValue::FieldAccess { object, .. } => {
                self.collect_async_send_sync_events_expression(object, events);
            }
        }
    }

    fn collect_async_send_sync_events_expression(
        &mut self,
        expr: &Expression,
        events: &mut AsyncSendSyncEvents,
    ) {
        match &expr.kind {
            ExpressionKind::Identifier(name) => {
                let order = events.bump();
                events.uses.push(AsyncUseEvent {
                    name: name.clone(),
                    order,
                });
            }
            ExpressionKind::NumberLiteral(_)
            | ExpressionKind::StringLiteral(_)
            | ExpressionKind::BoolLiteral(_)
            | ExpressionKind::CharLiteral(_) => {}
            ExpressionKind::FString(parts) => {
                for part in parts {
                    if let FStringPart::Interpolated(inner) = part {
                        self.collect_async_send_sync_events_expression(inner, events);
                    }
                }
            }
            ExpressionKind::Block(block)
            | ExpressionKind::DifferentiableBlock(block)
            | ExpressionKind::AsyncBlock(block) => {
                self.collect_async_send_sync_events_block(block, events);
            }
            ExpressionKind::Binary { left, right, .. } => {
                self.collect_async_send_sync_events_expression(left, events);
                self.collect_async_send_sync_events_expression(right, events);
            }
            ExpressionKind::Unary { operand, .. }
            | ExpressionKind::Try(operand)
            | ExpressionKind::Grouping(operand) => {
                self.collect_async_send_sync_events_expression(operand, events);
            }
            ExpressionKind::Await(inner) => {
                self.collect_async_send_sync_events_expression(inner, events);
                let order = events.bump();
                events.awaits.push(AsyncAwaitEvent {
                    span: expr.span,
                    order,
                });
            }
            ExpressionKind::Range { start, end, .. } => {
                self.collect_async_send_sync_events_expression(start, events);
                self.collect_async_send_sync_events_expression(end, events);
            }
            ExpressionKind::Call { callee, arguments } => {
                if self.is_async_task_boundary_call(callee) {
                    let names = arguments
                        .iter()
                        .flat_map(|arg| self.collect_identifier_names_in_expression(arg))
                        .collect();
                    events.task_boundaries.push(AsyncTaskBoundaryEvent {
                        span: expr.span,
                        names,
                    });
                }
                self.collect_async_send_sync_events_expression(callee, events);
                for arg in arguments {
                    self.collect_async_send_sync_events_expression(arg, events);
                }
            }
            ExpressionKind::If {
                condition,
                then_block,
                elif_blocks,
                else_block,
            } => {
                self.collect_async_send_sync_events_expression(condition, events);
                self.collect_async_send_sync_events_block(then_block, events);
                for (condition, block) in elif_blocks {
                    self.collect_async_send_sync_events_expression(condition, events);
                    self.collect_async_send_sync_events_block(block, events);
                }
                if let Some(block) = else_block {
                    self.collect_async_send_sync_events_block(block, events);
                }
            }
            ExpressionKind::Unless {
                condition,
                then_block,
                else_block,
            } => {
                self.collect_async_send_sync_events_expression(condition, events);
                self.collect_async_send_sync_events_block(then_block, events);
                if let Some(block) = else_block {
                    self.collect_async_send_sync_events_block(block, events);
                }
            }
            ExpressionKind::ArrayLiteral { elements }
            | ExpressionKind::TupleLiteral { elements } => {
                for element in elements {
                    self.collect_async_send_sync_events_expression(element, events);
                }
            }
            ExpressionKind::IndexAccess { array, index } => {
                self.collect_async_send_sync_events_expression(array, events);
                self.collect_async_send_sync_events_expression(index, events);
            }
            ExpressionKind::TupleAccess { tuple, .. } => {
                self.collect_async_send_sync_events_expression(tuple, events);
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                for (_, value) in fields {
                    self.collect_async_send_sync_events_expression(value, events);
                }
            }
            ExpressionKind::FieldAccess { object, .. } => {
                self.collect_async_send_sync_events_expression(object, events);
            }
            ExpressionKind::EnumVariant {
                data, struct_data, ..
            } => {
                if let Some(data) = data {
                    for value in data {
                        self.collect_async_send_sync_events_expression(value, events);
                    }
                }
                if let Some(struct_data) = struct_data {
                    for (_, value) in struct_data {
                        self.collect_async_send_sync_events_expression(value, events);
                    }
                }
            }
            ExpressionKind::Match { scrutinee, arms } => {
                self.collect_async_send_sync_events_expression(scrutinee, events);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.collect_async_send_sync_events_expression(guard, events);
                    }
                    self.collect_async_send_sync_events_expression(&arm.body, events);
                }
            }
            ExpressionKind::MethodCall {
                object, arguments, ..
            } => {
                self.collect_async_send_sync_events_expression(object, events);
                for arg in arguments {
                    self.collect_async_send_sync_events_expression(arg, events);
                }
            }
            ExpressionKind::Lambda { body, .. } => {
                self.collect_async_send_sync_events_expression(body, events);
            }
            ExpressionKind::Cast { expr: inner, .. } => {
                self.collect_async_send_sync_events_expression(inner, events);
            }
        }
    }

    fn infer_async_event_expression_type(
        &mut self,
        expr: &Expression,
        env: &HashMap<String, Type>,
    ) -> Type {
        match &expr.kind {
            ExpressionKind::Identifier(name) => env.get(name).cloned().unwrap_or_else(|| {
                self.lookup_symbol(name)
                    .map(|info| info.ty.clone())
                    .unwrap_or(Type::Unknown)
            }),
            ExpressionKind::StructLiteral { name, type_args, .. } => {
                if type_args.is_empty() {
                    Type::Struct { name: name.clone() }
                } else {
                    Type::Applied {
                        name: name.clone(),
                        args: type_args
                            .iter()
                            .map(|arg| self.type_annotation_to_type(&Some(arg.clone())))
                            .collect(),
                    }
                }
            }
            ExpressionKind::FieldAccess { object, field } => {
                // The async Send/Sync pass runs after the ordinary function
                // scope has been popped.  Re-resolve field access through the
                // event environment, just like method calls below; otherwise
                // `local.field` is downgraded to Unknown even when `local`
                // has a concrete struct type.
                let object_type = self.infer_async_event_expression_type(object, env);
                if let Some((_, struct_info, substitutions)) =
                    self.specialized_struct_context_for_type(&object_type)
                {
                    return struct_info
                        .fields
                        .get(field)
                        .map(|field_info| {
                            if substitutions.is_empty() {
                                self.type_annotation_to_type(&Some(field_info.ty.clone()))
                            } else {
                                self.type_annotation_to_type_with_substitutions(
                                    &field_info.ty,
                                    &substitutions,
                                )
                            }
                        })
                        .unwrap_or(Type::Unknown);
                }
                Type::Unknown
            }
            ExpressionKind::Call { callee, .. } => {
                if let ExpressionKind::Identifier(name) = &callee.kind {
                    if let Some(signature) = self.functions.get(name) {
                        return signature.return_type.clone();
                    }
                }
                self.infer_expression_type(expr)
            }
            ExpressionKind::Await(inner) => {
                match self.infer_async_event_expression_type(inner, env) {
                    Type::Task { output } => *output,
                    _ => Type::Unknown,
                }
            }
            ExpressionKind::MethodCall {
                object,
                method_name,
                ..
            } => {
                // This pass runs after the ordinary function scope has been
                // popped.  Resolve the receiver through the event environment
                // first; otherwise a method call on a local declared earlier
                // in the async function degrades to Unknown and produces a
                // false non-Send-across-await diagnostic.
                let object_type = self.infer_async_event_expression_type(object, env);
                match object_type {
                    Type::Struct { name } | Type::Enum { name } => self
                        .methods
                        .get(&name)
                        .and_then(|methods| methods.get(method_name))
                        .map(|signature| signature.return_type.clone())
                        .or_else(|| {
                            self.instantiated_method_signature(&name, method_name)
                                .map(|signature| signature.return_type)
                        })
                        .unwrap_or(Type::Unknown),
                    Type::Applied { .. } => {
                        let Some(name) = self.nominal_lookup_name(&object_type) else {
                            return Type::Unknown;
                        };
                        self.methods
                            .get(&name)
                            .and_then(|methods| methods.get(method_name))
                            .map(|signature| signature.return_type.clone())
                            .or_else(|| {
                                self.instantiated_method_signature(&name, method_name)
                                    .map(|signature| signature.return_type)
                            })
                            .unwrap_or(Type::Unknown)
                    }
                    Type::TypeParameter { name } => self
                        .trait_method_signature_for_type_param(&name, method_name)
                        .map(|(signature, _)| signature.return_type)
                        .unwrap_or(Type::Unknown),
                    Type::DynTrait { trait_name, .. } => self
                        .traits
                        .get(&trait_name)
                        .and_then(|methods| methods.get(method_name))
                        .map(|method| method.signature.return_type.clone())
                        .unwrap_or(Type::Unknown),
                    _ => self.infer_expression_type(expr),
                }
            }
            ExpressionKind::AsyncBlock(block) => Type::Task {
                output: Box::new(self.infer_block_type(block)),
            },
            _ => self.infer_expression_type(expr),
        }
    }

    fn collect_identifier_names_in_expression(&self, expr: &Expression) -> Vec<String> {
        let mut names = Vec::new();
        Self::collect_identifier_names_in_expression_inner(expr, &mut names);
        names.sort();
        names.dedup();
        names
    }

    fn collect_identifier_names_in_expression_inner(expr: &Expression, names: &mut Vec<String>) {
        match &expr.kind {
            ExpressionKind::Identifier(name) => names.push(name.clone()),
            ExpressionKind::NumberLiteral(_)
            | ExpressionKind::StringLiteral(_)
            | ExpressionKind::BoolLiteral(_)
            | ExpressionKind::CharLiteral(_) => {}
            ExpressionKind::FString(parts) => {
                for part in parts {
                    if let FStringPart::Interpolated(expr) = part {
                        Self::collect_identifier_names_in_expression_inner(expr, names);
                    }
                }
            }
            ExpressionKind::Block(block)
            | ExpressionKind::DifferentiableBlock(block)
            | ExpressionKind::AsyncBlock(block) => {
                for stmt in &block.statements {
                    Self::collect_identifier_names_in_statement_inner(stmt, names);
                }
            }
            ExpressionKind::Binary { left, right, .. } => {
                Self::collect_identifier_names_in_expression_inner(left, names);
                Self::collect_identifier_names_in_expression_inner(right, names);
            }
            ExpressionKind::Unary { operand, .. }
            | ExpressionKind::Try(operand)
            | ExpressionKind::Await(operand)
            | ExpressionKind::Grouping(operand)
            | ExpressionKind::TupleAccess { tuple: operand, .. }
            | ExpressionKind::Cast { expr: operand, .. } => {
                Self::collect_identifier_names_in_expression_inner(operand, names);
            }
            ExpressionKind::Range { start, end, .. } => {
                Self::collect_identifier_names_in_expression_inner(start, names);
                Self::collect_identifier_names_in_expression_inner(end, names);
            }
            ExpressionKind::Call { callee, arguments } => {
                Self::collect_identifier_names_in_expression_inner(callee, names);
                for arg in arguments {
                    Self::collect_identifier_names_in_expression_inner(arg, names);
                }
            }
            ExpressionKind::If {
                condition,
                then_block,
                elif_blocks,
                else_block,
            } => {
                Self::collect_identifier_names_in_expression_inner(condition, names);
                for stmt in &then_block.statements {
                    Self::collect_identifier_names_in_statement_inner(stmt, names);
                }
                for (condition, block) in elif_blocks {
                    Self::collect_identifier_names_in_expression_inner(condition, names);
                    for stmt in &block.statements {
                        Self::collect_identifier_names_in_statement_inner(stmt, names);
                    }
                }
                if let Some(block) = else_block {
                    for stmt in &block.statements {
                        Self::collect_identifier_names_in_statement_inner(stmt, names);
                    }
                }
            }
            ExpressionKind::Unless {
                condition,
                then_block,
                else_block,
            } => {
                Self::collect_identifier_names_in_expression_inner(condition, names);
                for stmt in &then_block.statements {
                    Self::collect_identifier_names_in_statement_inner(stmt, names);
                }
                if let Some(block) = else_block {
                    for stmt in &block.statements {
                        Self::collect_identifier_names_in_statement_inner(stmt, names);
                    }
                }
            }
            ExpressionKind::ArrayLiteral { elements }
            | ExpressionKind::TupleLiteral { elements } => {
                for element in elements {
                    Self::collect_identifier_names_in_expression_inner(element, names);
                }
            }
            ExpressionKind::IndexAccess { array, index } => {
                Self::collect_identifier_names_in_expression_inner(array, names);
                Self::collect_identifier_names_in_expression_inner(index, names);
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                for (_, value) in fields {
                    Self::collect_identifier_names_in_expression_inner(value, names);
                }
            }
            ExpressionKind::FieldAccess { object, .. } => {
                Self::collect_identifier_names_in_expression_inner(object, names);
            }
            ExpressionKind::EnumVariant {
                data, struct_data, ..
            } => {
                if let Some(data) = data {
                    for value in data {
                        Self::collect_identifier_names_in_expression_inner(value, names);
                    }
                }
                if let Some(struct_data) = struct_data {
                    for (_, value) in struct_data {
                        Self::collect_identifier_names_in_expression_inner(value, names);
                    }
                }
            }
            ExpressionKind::Match { scrutinee, arms } => {
                Self::collect_identifier_names_in_expression_inner(scrutinee, names);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        Self::collect_identifier_names_in_expression_inner(guard, names);
                    }
                    Self::collect_identifier_names_in_expression_inner(&arm.body, names);
                }
            }
            ExpressionKind::MethodCall {
                object, arguments, ..
            } => {
                Self::collect_identifier_names_in_expression_inner(object, names);
                for arg in arguments {
                    Self::collect_identifier_names_in_expression_inner(arg, names);
                }
            }
            ExpressionKind::Lambda { body, .. } => {
                Self::collect_identifier_names_in_expression_inner(body, names);
            }
        }
    }

    fn collect_identifier_names_in_statement_inner(stmt: &Statement, names: &mut Vec<String>) {
        match &stmt.kind {
            StatementKind::Let(let_stmt) => {
                if let Some(value) = &let_stmt.value {
                    Self::collect_identifier_names_in_expression_inner(value, names);
                }
            }
            StatementKind::Assignment(assign_stmt) => {
                Self::collect_identifier_names_in_lvalue_inner(&assign_stmt.target, names);
                Self::collect_identifier_names_in_expression_inner(&assign_stmt.value, names);
            }
            StatementKind::Return(ret) => {
                if let Some(value) = &ret.value {
                    Self::collect_identifier_names_in_expression_inner(value, names);
                }
            }
            StatementKind::Expression(expr) => {
                Self::collect_identifier_names_in_expression_inner(expr, names);
            }
            StatementKind::While(while_loop) => {
                Self::collect_identifier_names_in_expression_inner(&while_loop.condition, names);
                for stmt in &while_loop.body.statements {
                    Self::collect_identifier_names_in_statement_inner(stmt, names);
                }
            }
            StatementKind::DoWhile(do_while) => {
                for stmt in &do_while.body.statements {
                    Self::collect_identifier_names_in_statement_inner(stmt, names);
                }
                Self::collect_identifier_names_in_expression_inner(&do_while.condition, names);
            }
            StatementKind::For(for_loop) => {
                Self::collect_identifier_names_in_expression_inner(&for_loop.iterable, names);
                for stmt in &for_loop.body.statements {
                    Self::collect_identifier_names_in_statement_inner(stmt, names);
                }
            }
            StatementKind::Loop(loop_stmt) => {
                for stmt in &loop_stmt.body.statements {
                    Self::collect_identifier_names_in_statement_inner(stmt, names);
                }
            }
            StatementKind::Switch(switch_stmt) => {
                Self::collect_identifier_names_in_expression_inner(&switch_stmt.value, names);
                for case in &switch_stmt.cases {
                    Self::collect_identifier_names_in_expression_inner(&case.pattern, names);
                    for stmt in &case.body.statements {
                        Self::collect_identifier_names_in_statement_inner(stmt, names);
                    }
                }
                if let Some(default_body) = &switch_stmt.default {
                    for stmt in &default_body.statements {
                        Self::collect_identifier_names_in_statement_inner(stmt, names);
                    }
                }
            }
            StatementKind::IfLet(stmt) => {
                Self::collect_identifier_names_in_expression_inner(&stmt.value, names);
                for stmt in &stmt.then_block.statements {
                    Self::collect_identifier_names_in_statement_inner(stmt, names);
                }
                if let Some(block) = &stmt.else_block {
                    for stmt in &block.statements {
                        Self::collect_identifier_names_in_statement_inner(stmt, names);
                    }
                }
            }
            StatementKind::WhileLet(stmt) => {
                Self::collect_identifier_names_in_expression_inner(&stmt.value, names);
                for stmt in &stmt.body.statements {
                    Self::collect_identifier_names_in_statement_inner(stmt, names);
                }
            }
            StatementKind::Break | StatementKind::Continue => {}
        }
    }

    fn collect_identifier_names_in_lvalue_inner(
        target: &crate::ast::LValue,
        names: &mut Vec<String>,
    ) {
        match target {
            crate::ast::LValue::Identifier(name) => names.push(name.clone()),
            crate::ast::LValue::IndexAccess { array, index } => {
                Self::collect_identifier_names_in_expression_inner(array, names);
                Self::collect_identifier_names_in_expression_inner(index, names);
            }
            crate::ast::LValue::FieldAccess { object, .. } => {
                Self::collect_identifier_names_in_expression_inner(object, names);
            }
        }
    }

    fn is_async_task_boundary_call(&self, callee: &Expression) -> bool {
        match &callee.kind {
            ExpressionKind::Identifier(name) => matches!(
                name.as_str(),
                "spawn" | "spawn_task" | "task_spawn" | "spawn_detached"
            ),
            ExpressionKind::FieldAccess { field, .. } => matches!(
                field.as_str(),
                "spawn" | "spawn_task" | "task_spawn" | "spawn_detached"
            ),
            _ => false,
        }
    }

    fn type_is_send(&self, ty: &Type) -> bool {
        match ty {
            Type::Unknown => false,
            Type::Struct { name } => {
                if self.type_name_is_non_send(name) {
                    return false;
                }
                self.struct_infos
                    .get(name)
                    .map(|info| {
                        info.fields.values().all(|field| {
                            let field_ty = self.type_annotation_to_type(&Some(field.ty.clone()));
                            self.type_is_send(&field_ty)
                        })
                    })
                    .unwrap_or(true)
            }
            Type::Array { element_type, .. } => self.type_is_send(element_type),
            Type::Tuple { elements } => elements.iter().all(|element| self.type_is_send(element)),
            Type::Enum { name } => !self.type_name_is_non_send(name),
            Type::Applied { name, args } => {
                !self.type_name_is_non_send(name) && args.iter().all(|arg| self.type_is_send(arg))
            }
            Type::Task { output } => self.type_is_send(output),
            Type::DynTrait {
                trait_name,
                auto_traits,
            } => {
                (auto_traits.is_empty() || auto_traits.iter().any(|bound| bound == "Send"))
                    && !self.type_name_is_non_send(trait_name)
            }
            Type::Fn { .. } => false,
            Type::Tensor { .. }
            | Type::Range
            | Type::Int
            | Type::Float
            | Type::ExactInt { .. }
            | Type::ExactFloat { .. }
            | Type::Bool
            | Type::String
            | Type::Char
            | Type::Unit
            | Type::SelfType => true,
            Type::TypeParameter { name } => self
                .get_generic_bounds(name)
                .map(|bounds| bounds.iter().any(|bound| bound == "Send"))
                .unwrap_or(false),
        }
    }

    fn type_is_sync(&self, ty: &Type) -> bool {
        match ty {
            Type::Unknown => false,
            Type::Struct { name } => {
                if self.type_name_is_non_sync(name) {
                    return false;
                }
                self.struct_infos
                    .get(name)
                    .map(|info| {
                        info.fields.values().all(|field| {
                            let field_ty = self.type_annotation_to_type(&Some(field.ty.clone()));
                            self.type_is_sync(&field_ty)
                        })
                    })
                    .unwrap_or(true)
            }
            Type::Array { element_type, .. } => self.type_is_sync(element_type),
            Type::Tuple { elements } => elements.iter().all(|element| self.type_is_sync(element)),
            Type::Enum { name } => !self.type_name_is_non_sync(name),
            Type::Applied { name, args } => {
                !self.type_name_is_non_sync(name) && args.iter().all(|arg| self.type_is_sync(arg))
            }
            Type::Task { output } => self.type_is_send(output),
            Type::DynTrait {
                trait_name,
                auto_traits,
            } => {
                (auto_traits.is_empty() || auto_traits.iter().any(|bound| bound == "Sync"))
                    && !self.type_name_is_non_sync(trait_name)
            }
            Type::Fn { .. } => false,
            Type::Tensor { .. }
            | Type::Range
            | Type::Int
            | Type::Float
            | Type::ExactInt { .. }
            | Type::ExactFloat { .. }
            | Type::Bool
            | Type::String
            | Type::Char
            | Type::Unit
            | Type::SelfType => true,
            Type::TypeParameter { name } => self
                .get_generic_bounds(name)
                .map(|bounds| bounds.iter().any(|bound| bound == "Sync"))
                .unwrap_or(false),
        }
    }

    fn type_is_ref_cell_family(&self, ty: &Type) -> bool {
        match ty {
            Type::Struct { name } | Type::Enum { name } => {
                matches!(name.as_str(), "RefCell" | "Cell" | "UnsafeCell")
            }
            Type::Applied { name, args } => {
                matches!(name.as_str(), "RefCell" | "Cell" | "UnsafeCell")
                    || args.iter().any(|arg| self.type_is_ref_cell_family(arg))
            }
            Type::Array { element_type, .. }
            | Type::Task {
                output: element_type,
            } => self.type_is_ref_cell_family(element_type),
            Type::Range => false,
            Type::Tuple { elements } => elements
                .iter()
                .any(|element| self.type_is_ref_cell_family(element)),
            _ => false,
        }
    }

    fn type_name_is_non_send(&self, name: &str) -> bool {
        matches!(
            name,
            "RefCell"
                | "Cell"
                | "UnsafeCell"
                | "Rc"
                | "RawPtr"
                | "LocalOnly"
                | "NonSend"
                | "NotSend"
                | "LocalHandle"
        )
    }

    fn type_name_is_non_sync(&self, name: &str) -> bool {
        matches!(
            name,
            "RefCell" | "Cell" | "UnsafeCell" | "Rc" | "RawPtr" | "LocalOnly" | "LocalHandle"
        )
    }

}
