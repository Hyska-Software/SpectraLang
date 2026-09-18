use super::*;

impl ASTLowering {
    pub(crate) fn lower_block(&mut self, statements: &[Statement], ir_func: &mut IRFunction) {
        self.lower_block_with_scope(statements, ir_func, true);
    }

    pub(crate) fn type_has_drop(&self, ty: &IRType) -> bool {
        match ty {
            IRType::Struct { name, fields } => {
                self.trait_implementations
                    .contains_key(&(name.clone(), "Drop".to_string()))
                    || fields
                        .iter()
                        .any(|(_, field_ty)| self.type_has_drop(field_ty))
            }
            IRType::Tuple { elements } => elements.iter().any(|ty| self.type_has_drop(ty)),
            IRType::Array { element_type, .. } => self.type_has_drop(element_type),
            _ => false,
        }
    }

    /// Emit recursive destructor calls for a value represented by an
    /// aggregate pointer.  A user-defined Drop implementation runs for the
    /// owning record, followed by its droppable fields in declaration order.
    /// Fields are themselves stored as pointers, so loading a nested struct
    /// value produces the pointer expected by its drop glue.
    pub(crate) fn emit_drop_for_value(
        &mut self,
        value: Value,
        ty: &IRType,
        ir_func: &mut IRFunction,
    ) {
        match ty {
            IRType::Struct { name, fields } => {
                if self
                    .trait_implementations
                    .contains_key(&(name.clone(), "Drop".to_string()))
                {
                    self.builder
                        .build_call(ir_func, format!("{}_drop", name), vec![value], false);
                }

                let owned_fields = if fields.is_empty() {
                    self.struct_definitions
                        .get(name)
                        .cloned()
                        .unwrap_or_default()
                } else {
                    fields.clone()
                };
                let layout = layout::layout_of(owned_fields.iter().map(|(_, field_ty)| field_ty));
                for (index, (_, field_ty)) in owned_fields.iter().enumerate() {
                    if !self.type_has_drop(field_ty) {
                        continue;
                    }
                    let Some(offset) = layout.offsets.get(index).copied() else {
                        continue;
                    };
                    let field_ptr = self.builder.build_field_ptr(ir_func, value, offset as i64);
                    let field_value =
                        self.builder
                            .build_load_typed(ir_func, field_ptr, field_ty.clone());
                    self.emit_drop_for_value(field_value, field_ty, ir_func);
                }
            }
            IRType::Tuple { elements } => {
                let layout = layout::layout_of(elements.iter());
                for (index, element_ty) in elements.iter().enumerate() {
                    if !self.type_has_drop(element_ty) {
                        continue;
                    }
                    let Some(offset) = layout.offsets.get(index).copied() else {
                        continue;
                    };
                    let element_ptr = self.builder.build_field_ptr(ir_func, value, offset as i64);
                    let element_value =
                        self.builder
                            .build_load_typed(ir_func, element_ptr, element_ty.clone());
                    self.emit_drop_for_value(element_value, element_ty, ir_func);
                }
            }
            IRType::Array { element_type, size } => {
                if !self.type_has_drop(element_type) {
                    return;
                }
                for index in (0..*size).rev() {
                    let index_value = self.builder.build_const_int(ir_func, index as i64);
                    let element_ptr = self.builder.build_getelementptr(
                        ir_func,
                        value,
                        index_value,
                        element_type.as_ref().clone(),
                    );
                    let element_value = self.builder.build_load_typed(
                        ir_func,
                        element_ptr,
                        element_type.as_ref().clone(),
                    );
                    self.emit_drop_for_value(element_value, element_type, ir_func);
                }
            }
            _ => {}
        }
    }

    /// Preserve every manual allocation reachable from an aggregate returned
    /// by the current function.  The backend already escapes the root return
    /// pointer, but nested aggregate fields are independent manual
    /// allocations and would otherwise be reclaimed by the callee's frame
    /// exit.  Returning such a value must transfer the complete ownership
    /// graph, not just its outer pointer.
    pub(crate) fn emit_escape_for_value(
        &mut self,
        value: Value,
        ty: &IRType,
        ir_func: &mut IRFunction,
    ) {
        let mut seen = Vec::new();
        self.emit_escape_for_value_inner(value, ty, ir_func, 0, &mut seen);
    }

    /// Whether a value of `ty` can carry an owned manual allocation.
    ///
    /// Scalars travel by value; every other aggregate and a string are
    /// pointers to tracked allocations.
    fn carries_owned_allocation(ty: &IRType) -> bool {
        matches!(
            ty,
            IRType::String
                | IRType::Struct { .. }
                | IRType::Tuple { .. }
                | IRType::Array { .. }
                | IRType::Enum { .. }
                | IRType::DynTrait { .. }
                // `Option<T>`, `Result<T, E>`, `List<T>` and friends: the walk
                // continues through the concrete representation.
                | IRType::Generic { .. }
                // A buffer (`List` storage, a heap aggregate) is a tracked
                // allocation reachable through a pointer field. Escaping a
                // borrowed pointer is a no-op -- the table has no entry, or the
                // entry belongs to another frame.
                | IRType::Pointer(_)
        )
    }

    fn emit_escape_for_value_inner(
        &mut self,
        value: Value,
        ty: &IRType,
        ir_func: &mut IRFunction,
        depth: usize,
        seen: &mut Vec<String>,
    ) {
        // Recursive types (`Node { next: Option<Node> }`) would otherwise lower
        // forever: the walk follows the type graph, and payload slots are
        // inspected per variant. Named types are visited once per path; deep
        // anonymous nesting stops at a bound.
        const MAX_DEPTH: usize = 16;
        if depth > MAX_DEPTH {
            return;
        }
        let named = match ty {
            IRType::Struct { name, .. } | IRType::Enum { name, .. } => Some(name.clone()),
            _ => None,
        };
        if let Some(name) = &named {
            if seen.contains(name) {
                return;
            }
            seen.push(name.clone());
        }
        match ty {
            IRType::Struct { name, fields } => {
                self.builder.build_escape_manual_alloc(ir_func, value);
                let owned_fields = if fields.is_empty() {
                    self.struct_definitions
                        .get(name)
                        .cloned()
                        .unwrap_or_default()
                } else {
                    fields.clone()
                };
                let layout = layout::layout_of(owned_fields.iter().map(|(_, field_ty)| field_ty));
                for (index, (_, field_ty)) in owned_fields.iter().enumerate() {
                    if !Self::carries_owned_allocation(field_ty) {
                        continue;
                    }
                    let Some(offset) = layout.offsets.get(index).copied() else {
                        continue;
                    };
                    let field_ptr = self.builder.build_field_ptr(ir_func, value, offset as i64);
                    let field_value =
                        self.builder
                            .build_load_typed(ir_func, field_ptr, field_ty.clone());
                    self.emit_escape_for_value_inner(
                        field_value,
                        field_ty,
                        ir_func,
                        depth + 1,
                        seen,
                    );
                }
            }
            IRType::Tuple { elements } => {
                self.builder.build_escape_manual_alloc(ir_func, value);
                let layout = layout::layout_of(elements.iter());
                for (index, element_ty) in elements.iter().enumerate() {
                    if !Self::carries_owned_allocation(element_ty) {
                        continue;
                    }
                    let Some(offset) = layout.offsets.get(index).copied() else {
                        continue;
                    };
                    let element_ptr = self.builder.build_field_ptr(ir_func, value, offset as i64);
                    let element_value =
                        self.builder
                            .build_load_typed(ir_func, element_ptr, element_ty.clone());
                    self.emit_escape_for_value_inner(
                        element_value,
                        element_ty,
                        ir_func,
                        depth + 1,
                        seen,
                    );
                }
            }
            IRType::Array { element_type, size } => {
                self.builder.build_escape_manual_alloc(ir_func, value);
                if !Self::carries_owned_allocation(element_type) {
                    return;
                }
                for index in 0..*size {
                    let index_value = self.builder.build_const_int(ir_func, index as i64);
                    let element_ptr = self.builder.build_getelementptr(
                        ir_func,
                        value,
                        index_value,
                        element_type.as_ref().clone(),
                    );
                    let element_value = self.builder.build_load_typed(
                        ir_func,
                        element_ptr,
                        element_type.as_ref().clone(),
                    );
                    self.emit_escape_for_value_inner(
                        element_value,
                        element_type,
                        ir_func,
                        depth + 1,
                        seen,
                    );
                }
            }
            // A variant with data is laid out as `(tag, data...)`; only the
            // variant the tag names holds a live payload, and every other slot
            // is zeroed or a scalar, which `spectra_rt_manual_escape` ignores.
            IRType::Enum { variants, .. } => {
                self.builder.build_escape_manual_alloc(ir_func, value);
                for (_, data_types) in variants {
                    let Some(data_types) = data_types else {
                        continue;
                    };
                    let mut element_types = vec![IRType::Int];
                    element_types.extend(data_types.iter().cloned());
                    let layout = layout::layout_of(element_types.iter());
                    for (index, data_ty) in data_types.iter().enumerate() {
                        let Some(offset) = layout.offsets.get(index + 1).copied() else {
                            continue;
                        };
                        let payload_ptr =
                            self.builder.build_field_ptr(ir_func, value, offset as i64);
                        let payload =
                            self.builder
                                .build_load_typed(ir_func, payload_ptr, data_ty.clone());
                        // Escape the slot itself before asking its declared type
                        // for more: a call site can instantiate the enum with
                        // type arguments that default to `int` (see the
                        // declared-annotation override in
                        // lowering_impl_type_inference), and the value in the
                        // slot is still a tracked allocation. Escaping a scalar
                        // is a no-op -- no allocation table entry exists for
                        // it -- so this cannot re-parent anything else.
                        self.builder.build_escape_manual_alloc(ir_func, payload);
                        if Self::carries_owned_allocation(data_ty) {
                            self.emit_escape_for_value_inner(
                                payload,
                                data_ty,
                                ir_func,
                                depth + 1,
                                seen,
                            );
                        }
                    }
                }
            }
            // A generic application is an alias for its representation
            // (`Option<string>` -> an enum whose data is a string): the
            // concrete payload types live there, not in the type arguments.
            IRType::Generic { representation, .. } => {
                self.emit_escape_for_value_inner(value, representation, ir_func, depth, seen);
            }
            IRType::DynTrait { .. } | IRType::Pointer(_) => {
                self.builder.build_escape_manual_alloc(ir_func, value);
            }
            // A string is a pointer to a tracked manual allocation: a string
            // built inside the callee is released by the callee's frame exit
            // unless it travels with the returned value. Escaping is a no-op
            // for literals and for allocations that belong to another frame
            // (`spectra_rt_manual_escape` only re-parents its own).
            IRType::String => {
                self.builder.build_escape_manual_alloc(ir_func, value);
            }
            _ => {}
        }
        if named.is_some() {
            seen.pop();
        }
    }

    pub(crate) fn collect_moved_identifiers(expr: &Expression, out: &mut HashSet<String>) {
        match &expr.kind {
            ExpressionKind::Identifier(name) => {
                out.insert(name.clone());
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                for (_, value) in fields {
                    Self::collect_moved_identifiers(value, out);
                }
            }
            ExpressionKind::TupleLiteral { elements }
            | ExpressionKind::ArrayLiteral { elements } => {
                for element in elements {
                    Self::collect_moved_identifiers(element, out);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn emit_scope_drops(
        &mut self,
        ir_func: &mut IRFunction,
        skipped_names: &HashSet<String>,
    ) {
        let scopes: Vec<Vec<(String, Value, String)>> = self
            .struct_var_map
            .scopes
            .iter()
            .rev()
            .map(|scope| {
                scope
                    .iter()
                    .filter(|(name, _)| {
                        !skipped_names.contains(*name) && !self.drop_excluded_names.contains(*name)
                    })
                    .map(|(name, (value, struct_name))| (name.clone(), *value, struct_name.clone()))
                    .collect()
            })
            .collect();
        for mut values in scopes {
            // StructScopeStack intentionally remains a compact hash map; sort
            // names to keep destructor output deterministic until declaration
            // ordering is represented explicitly in the scope structure.
            values.sort_by(|left, right| right.0.cmp(&left.0));
            for (name, value, struct_name) in values {
                let ty = IRType::Struct {
                    name: struct_name.clone(),
                    fields: self
                        .struct_definitions
                        .get(&struct_name)
                        .cloned()
                        .unwrap_or_default(),
                };
                if self.type_has_drop(&ty) {
                    // A reassigned record local's slot holds the pointer that is
                    // currently live; drop that rather than a stale map entry.
                    let target = if self.alloca_map.contains_key(&name) {
                        self.load_slot(&name, ir_func).unwrap_or(value)
                    } else {
                        value
                    };
                    self.emit_drop_for_value(target, &ty, ir_func);
                }
            }
        }
    }

    pub(crate) fn wrap_async_return_value(
        &mut self,
        ir_func: &mut IRFunction,
        value: Option<Value>,
        output_type: IRType,
    ) -> Value {
        if self.current_async_output_type.is_none() {
            let Some(value) = value else {
                return self.invalid_value(
                    "async return wrapper reached without a value outside async output",
                );
            };
            return value;
        }

        let Some(payload) = value else {
            return self.invalid_value("async function is missing its return value payload");
        };
        self.builder
            .build_async_ready(ir_func, Some(payload), output_type.clone());

        self.require_value(
            self.builder.build_typed_host_call(
                ir_func,
                "spectra.async.task.ready".to_string(),
                vec![payload],
                IRType::Task {
                    output: Box::new(output_type),
                },
                true,
            ),
            "async task-ready host call did not produce its task handle",
        )
    }

    pub(crate) fn current_block_is_terminated(&self, ir_func: &IRFunction) -> bool {
        self.builder
            .get_current_block()
            .and_then(|block_id| ir_func.get_block(block_id))
            .map(|block| block.terminator.is_some())
            .unwrap_or(false)
    }

    pub(crate) fn lower_block_with_scope(
        &mut self,
        statements: &[Statement],
        ir_func: &mut IRFunction,
        create_scope: bool,
    ) {
        if create_scope {
            self.value_map.push_scope();
            self.variable_types.push_scope();
            self.array_map.push_scope();
            self.range_map.push_scope();
            self.struct_var_map.push_scope();
        }

        for stmt in statements {
            if self.current_block_is_terminated(ir_func) {
                break;
            }
            self.lower_statement(stmt, ir_func);
        }

        if create_scope {
            if self.current_block_is_terminated(ir_func) {
                self.struct_var_map.pop_scope();
                self.array_map.pop_scope();
                self.range_map.pop_scope();
                self.variable_types.pop_scope();
                self.value_map.pop_scope();
                return;
            }

            self.emit_scope_drops(ir_func, &HashSet::new());

            self.struct_var_map.pop_scope();
            self.array_map.pop_scope();
            self.range_map.pop_scope();
            self.variable_types.pop_scope();
            self.value_map.pop_scope();
        }
    }
    pub(crate) fn find_assigned_variables(
        &self,
        statements: &[Statement],
    ) -> std::collections::HashSet<String> {
        use std::collections::HashSet;
        let mut assigned = HashSet::new();

        for stmt in statements {
            match &stmt.kind {
                StatementKind::Let(let_stmt) => {
                    if let Some(value) = &let_stmt.value {
                        self.collect_assigned_variables_in_expr(value, &mut assigned);
                    }
                }
                StatementKind::Assignment(assign) => {
                    // Extract variable name from LValue
                    // For now, only track simple identifiers (not array elements)
                    if let spectra_compiler::ast::LValue::Identifier(name) = &assign.target {
                        assigned.insert(name.clone());
                    }
                    self.collect_assigned_variables_in_expr(&assign.value, &mut assigned);
                }
                StatementKind::While(while_stmt) => {
                    self.collect_assigned_variables_in_expr(&while_stmt.condition, &mut assigned);
                    assigned.extend(self.find_assigned_variables(&while_stmt.body.statements));
                }
                StatementKind::DoWhile(do_while) => {
                    self.collect_assigned_variables_in_expr(&do_while.condition, &mut assigned);
                    assigned.extend(self.find_assigned_variables(&do_while.body.statements));
                }
                StatementKind::For(for_stmt) => {
                    self.collect_assigned_variables_in_expr(&for_stmt.iterable, &mut assigned);
                    assigned.extend(self.find_assigned_variables(&for_stmt.body.statements));
                }
                StatementKind::Loop(loop_stmt) => {
                    assigned.extend(self.find_assigned_variables(&loop_stmt.body.statements));
                }
                StatementKind::WhileLet(while_let) => {
                    self.collect_assigned_variables_in_expr(&while_let.value, &mut assigned);
                    assigned.extend(self.find_assigned_variables(&while_let.body.statements));
                }
                StatementKind::IfLet(if_let) => {
                    self.collect_assigned_variables_in_expr(&if_let.value, &mut assigned);
                    assigned.extend(self.find_assigned_variables(&if_let.then_block.statements));
                    if let Some(else_b) = &if_let.else_block {
                        assigned.extend(self.find_assigned_variables(&else_b.statements));
                    }
                }
                StatementKind::Switch(switch) => {
                    self.collect_assigned_variables_in_expr(&switch.value, &mut assigned);
                    for case in &switch.cases {
                        assigned.extend(self.find_assigned_variables(&case.body.statements));
                    }
                    if let Some(default) = &switch.default {
                        assigned.extend(self.find_assigned_variables(&default.statements));
                    }
                }
                StatementKind::Expression(expr) => {
                    self.collect_assigned_variables_in_expr(expr, &mut assigned);
                }
                StatementKind::Return(ret) => {
                    if let Some(value) = &ret.value {
                        self.collect_assigned_variables_in_expr(value, &mut assigned);
                    }
                }
                _ => {}
            }
        }

        assigned
    }

    /// Best-effort IR type for a stack slot derived purely from syntax.
    /// Anything not recognized keeps the historical `Int` slot.
    pub(crate) fn syntactic_ir_type_hint(expr: &Expression) -> Option<IRType> {
        match &expr.kind {
            ExpressionKind::BoolLiteral(_) => Some(IRType::Bool),
            ExpressionKind::StringLiteral(_) => Some(IRType::String),
            ExpressionKind::CharLiteral(_) => Some(IRType::Char),
            ExpressionKind::NumberLiteral(text) => {
                if text.contains('.') {
                    Some(IRType::Float)
                } else {
                    Some(IRType::Int)
                }
            }
            _ => None,
        }
    }

    /// The IR type a promoted local's slot must be allocated with.
    ///
    /// The declared annotation wins (it is the binding contract), then a
    /// literal's own type, then the expression's resolved type — a call, a
    /// field access or an operator has no syntactic hint, and a slot that
    /// falls back to `Int` is read back as an integer: inside a coroutine that
    /// slot *is* the frame slot the value crosses a suspension through, so a
    /// float-returning call bound to a local produced `fcmp.f64 ge` against an
    /// `i64` operand and failed Cranelift verification.
    ///
    /// Only scalars are hinted here. Struct, array and tensor locals have their
    /// own storage paths, and widening this to them would move those paths.
    fn slot_hint_for(
        &mut self,
        annotation: Option<&TypeAnnotation>,
        value: &Expression,
    ) -> Option<IRType> {
        let scalar = |ty: IRType| match ty {
            IRType::Int | IRType::Float | IRType::Bool | IRType::String | IRType::Char => Some(ty),
            _ => None,
        };
        if let Some(annotation) = annotation {
            if let Some(ty) = scalar(self.lower_type_annotation(annotation)) {
                return Some(ty);
            }
        }
        if let Some(ty) = Self::syntactic_ir_type_hint(value) {
            return Some(ty);
        }
        scalar(self.infer_expr_ir_type(value))
    }

    /// Like [`Self::find_assigned_variables`], but also records a best-effort
    /// IR type per mutated binding so its stack slot is allocated with the
    /// right type instead of a blanket `Int`. A wrong slot type makes the
    /// backend promote the alloca to a Cranelift variable of the declared
    /// type while stores carry the real value type, which panics in the
    /// Cranelift frontend (e.g. a `bool` mutated inside a loop).
    pub(crate) fn find_assigned_variables_with_types(
        &mut self,
        statements: &[Statement],
        hints: &mut std::collections::HashMap<String, IRType>,
    ) -> std::collections::HashSet<String> {
        use std::collections::HashSet;
        let mut assigned = HashSet::new();

        // Scratch scope so sequential `let`s infer through earlier ones
        // (`let y = x + 1.0` needs x's type). Real lowering runs after the
        // pop, so no pre-pass state can leak into it. First wins, mirroring
        // the hints discipline (shadowing inside branches resolves against
        // whatever semantic analysis accepted).
        self.variable_types.push_scope();

        for stmt in statements {
            match &stmt.kind {
                StatementKind::Let(let_stmt) => {
                    if let (spectra_compiler::ast::Pattern::Identifier(name, _), Some(value)) =
                        (&let_stmt.pattern, &let_stmt.value)
                    {
                        if !hints.contains_key(name) {
                            if let Some(ty) = self.slot_hint_for(let_stmt.ty.as_ref(), value) {
                                hints.insert(name.clone(), ty);
                            }
                        }
                        if self.variable_types.get(name).is_none() {
                            let inferred = self.infer_expr_ir_type(value);
                            if !matches!(inferred, IRType::Unknown) {
                                self.variable_types.insert(name.clone(), inferred);
                            }
                        }
                        self.collect_assigned_variables_in_expr(value, &mut assigned);
                    }
                }
                StatementKind::Assignment(assign) => {
                    if let spectra_compiler::ast::LValue::Identifier(name) = &assign.target {
                        assigned.insert(name.clone());
                        if !hints.contains_key(name) {
                            if let Some(ty) = self.slot_hint_for(None, &assign.value) {
                                hints.insert(name.clone(), ty);
                            }
                        }
                    }
                    self.collect_assigned_variables_in_expr(&assign.value, &mut assigned);
                }
                StatementKind::While(while_stmt) => {
                    self.collect_assigned_variables_in_expr(&while_stmt.condition, &mut assigned);
                    assigned.extend(
                        self.find_assigned_variables_with_types(&while_stmt.body.statements, hints),
                    );
                }
                StatementKind::DoWhile(do_while) => {
                    self.collect_assigned_variables_in_expr(&do_while.condition, &mut assigned);
                    assigned.extend(
                        self.find_assigned_variables_with_types(&do_while.body.statements, hints),
                    );
                }
                StatementKind::For(for_stmt) => {
                    self.collect_assigned_variables_in_expr(&for_stmt.iterable, &mut assigned);
                    assigned.extend(
                        self.find_assigned_variables_with_types(&for_stmt.body.statements, hints),
                    );
                }
                StatementKind::Loop(loop_stmt) => {
                    assigned.extend(
                        self.find_assigned_variables_with_types(&loop_stmt.body.statements, hints),
                    );
                }
                StatementKind::WhileLet(while_let) => {
                    self.collect_assigned_variables_in_expr(&while_let.value, &mut assigned);
                    assigned.extend(
                        self.find_assigned_variables_with_types(&while_let.body.statements, hints),
                    );
                }
                StatementKind::IfLet(if_let) => {
                    self.collect_assigned_variables_in_expr(&if_let.value, &mut assigned);
                    assigned.extend(
                        self.find_assigned_variables_with_types(
                            &if_let.then_block.statements,
                            hints,
                        ),
                    );
                    if let Some(else_b) = &if_let.else_block {
                        assigned.extend(
                            self.find_assigned_variables_with_types(&else_b.statements, hints),
                        );
                    }
                }
                StatementKind::Switch(switch) => {
                    self.collect_assigned_variables_in_expr(&switch.value, &mut assigned);
                    for case in &switch.cases {
                        assigned.extend(
                            self.find_assigned_variables_with_types(&case.body.statements, hints),
                        );
                    }
                    if let Some(default) = &switch.default {
                        assigned.extend(
                            self.find_assigned_variables_with_types(&default.statements, hints),
                        );
                    }
                }
                StatementKind::Expression(expr) => {
                    self.collect_assigned_variables_in_expr(expr, &mut assigned);
                }
                StatementKind::Return(ret) => {
                    if let Some(value) = &ret.value {
                        self.collect_assigned_variables_in_expr(value, &mut assigned);
                    }
                }
                _ => {}
            }
        }

        self.variable_types.pop_scope();
        assigned
    }

    pub(crate) fn collect_assigned_variables_in_expr(
        &self,
        expr: &Expression,
        assigned: &mut std::collections::HashSet<String>,
    ) {
        match &expr.kind {
            ExpressionKind::Block(block)
            | ExpressionKind::DifferentiableBlock(block)
            | ExpressionKind::AsyncBlock(block) => {
                assigned.extend(self.find_assigned_variables(&block.statements));
            }
            ExpressionKind::Binary { left, right, .. } => {
                self.collect_assigned_variables_in_expr(left, assigned);
                self.collect_assigned_variables_in_expr(right, assigned);
            }
            ExpressionKind::Unary { operand, .. }
            | ExpressionKind::Try(operand)
            | ExpressionKind::Await(operand)
            | ExpressionKind::Grouping(operand) => {
                self.collect_assigned_variables_in_expr(operand, assigned);
            }
            ExpressionKind::Range { start, end, .. } => {
                self.collect_assigned_variables_in_expr(start, assigned);
                self.collect_assigned_variables_in_expr(end, assigned);
            }
            ExpressionKind::Call { callee, arguments } => {
                self.collect_assigned_variables_in_expr(callee, assigned);
                for argument in arguments {
                    self.collect_assigned_variables_in_expr(argument, assigned);
                }
            }
            ExpressionKind::If {
                condition,
                then_block,
                elif_blocks,
                else_block,
            } => {
                self.collect_assigned_variables_in_expr(condition, assigned);
                assigned.extend(self.find_assigned_variables(&then_block.statements));
                for (condition, block) in elif_blocks {
                    self.collect_assigned_variables_in_expr(condition, assigned);
                    assigned.extend(self.find_assigned_variables(&block.statements));
                }
                if let Some(else_block) = else_block {
                    assigned.extend(self.find_assigned_variables(&else_block.statements));
                }
            }
            ExpressionKind::Unless {
                condition,
                then_block,
                else_block,
            } => {
                self.collect_assigned_variables_in_expr(condition, assigned);
                assigned.extend(self.find_assigned_variables(&then_block.statements));
                if let Some(else_block) = else_block {
                    assigned.extend(self.find_assigned_variables(&else_block.statements));
                }
            }
            ExpressionKind::ArrayLiteral { elements }
            | ExpressionKind::TupleLiteral { elements } => {
                for element in elements {
                    self.collect_assigned_variables_in_expr(element, assigned);
                }
            }
            ExpressionKind::IndexAccess { array, index } => {
                self.collect_assigned_variables_in_expr(array, assigned);
                self.collect_assigned_variables_in_expr(index, assigned);
            }
            ExpressionKind::TupleAccess { tuple, .. } => {
                self.collect_assigned_variables_in_expr(tuple, assigned);
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                for (_, value) in fields {
                    self.collect_assigned_variables_in_expr(value, assigned);
                }
            }
            ExpressionKind::FieldAccess { object, .. } => {
                self.collect_assigned_variables_in_expr(object, assigned);
            }
            ExpressionKind::EnumVariant {
                data, struct_data, ..
            } => {
                if let Some(values) = data {
                    for value in values {
                        self.collect_assigned_variables_in_expr(value, assigned);
                    }
                }
                if let Some(fields) = struct_data {
                    for (_, value) in fields {
                        self.collect_assigned_variables_in_expr(value, assigned);
                    }
                }
            }
            ExpressionKind::Match { scrutinee, arms } => {
                self.collect_assigned_variables_in_expr(scrutinee, assigned);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.collect_assigned_variables_in_expr(guard, assigned);
                    }
                    self.collect_assigned_variables_in_expr(&arm.body, assigned);
                }
            }
            ExpressionKind::MethodCall {
                object, arguments, ..
            } => {
                self.collect_assigned_variables_in_expr(object, assigned);
                for argument in arguments {
                    self.collect_assigned_variables_in_expr(argument, assigned);
                }
            }
            ExpressionKind::Lambda { .. } => {}
            ExpressionKind::Cast { expr, .. } => {
                self.collect_assigned_variables_in_expr(expr, assigned);
            }
            ExpressionKind::FString(parts) => {
                for part in parts {
                    if let spectra_compiler::ast::FStringPart::Interpolated(expr) = part {
                        self.collect_assigned_variables_in_expr(expr, assigned);
                    }
                }
            }
            ExpressionKind::Identifier(_)
            | ExpressionKind::NumberLiteral(_)
            | ExpressionKind::StringLiteral(_)
            | ExpressionKind::BoolLiteral(_)
            | ExpressionKind::CharLiteral(_) => {}
        }
    }

    pub(crate) fn lower_branch_block_result(
        &mut self,
        block: &Block,
        ir_func: &mut IRFunction,
        entry_block: usize,
    ) -> (Option<Value>, usize, bool) {
        self.value_map.push_scope();
        self.variable_types.push_scope();
        self.array_map.push_scope();
        self.range_map.push_scope();
        self.struct_var_map.push_scope();

        // Lower all statements and extract the value of the last expression.
        // Previously this called lower_block (which lowers ALL stmts) and then
        // re-evaluated the last stmt with lower_expression, causing the last
        // statement to execute TWICE. Fix: lower all-but-last with lower_block,
        // then handle the last stmt specially to capture its value without duplication.
        let stmts = &block.statements;
        let produced_value = if stmts.is_empty() {
            None
        } else {
            for stmt in &stmts[..stmts.len() - 1] {
                self.lower_statement(stmt, ir_func);
            }
            let last = &stmts[stmts.len() - 1];
            match &last.kind {
                StatementKind::Expression(expr) => Some(self.lower_expression(expr, ir_func)),
                _ => {
                    self.lower_statement(last, ir_func);
                    None
                }
            }
        };

        let current_block_id = self.builder.get_current_block().unwrap_or(entry_block);

        let has_terminator = ir_func
            .get_block(current_block_id)
            .map(|block| block.terminator.is_some())
            .unwrap_or(false);

        self.struct_var_map.pop_scope();
        self.array_map.pop_scope();
        self.range_map.pop_scope();
        self.variable_types.pop_scope();
        self.value_map.pop_scope();

        (produced_value, current_block_id, has_terminator)
    }

    pub(crate) fn evaluate_int_constant(&self, expr: &Expression) -> Option<i64> {
        match &expr.kind {
            ExpressionKind::NumberLiteral(value) => {
                match spectra_compiler::numeric::parse_number_literal(value) {
                    Some(spectra_compiler::numeric::ParsedNumber::Int(int_value)) => {
                        Some(int_value)
                    }
                    _ => None,
                }
            }
            ExpressionKind::BoolLiteral(value) => Some(if *value { 1 } else { 0 }),
            ExpressionKind::Grouping(inner) => self.evaluate_int_constant(inner),
            ExpressionKind::Unary { operator, operand } => {
                let inner = self.evaluate_int_constant(operand)?;
                match operator {
                    UnaryOperator::Negate => inner.checked_neg(),
                    UnaryOperator::Not => Some(if inner == 0 { 1 } else { 0 }),
                }
            }
            ExpressionKind::Binary {
                left,
                operator,
                right,
            } => {
                let lhs = self.evaluate_int_constant(left)?;
                let rhs = self.evaluate_int_constant(right)?;
                match operator {
                    BinaryOperator::Add => lhs.checked_add(rhs),
                    BinaryOperator::Subtract => lhs.checked_sub(rhs),
                    BinaryOperator::Multiply => lhs.checked_mul(rhs),
                    BinaryOperator::Divide => {
                        if rhs == 0 {
                            None
                        } else {
                            Some(lhs / rhs)
                        }
                    }
                    BinaryOperator::Modulo => {
                        if rhs == 0 {
                            None
                        } else {
                            Some(lhs % rhs)
                        }
                    }
                    BinaryOperator::Equal => Some(if lhs == rhs { 1 } else { 0 }),
                    BinaryOperator::NotEqual => Some(if lhs != rhs { 1 } else { 0 }),
                    BinaryOperator::Less => Some(if lhs < rhs { 1 } else { 0 }),
                    BinaryOperator::Greater => Some(if lhs > rhs { 1 } else { 0 }),
                    BinaryOperator::LessEqual => Some(if lhs <= rhs { 1 } else { 0 }),
                    BinaryOperator::GreaterEqual => Some(if lhs >= rhs { 1 } else { 0 }),
                    BinaryOperator::And => Some(if lhs != 0 && rhs != 0 { 1 } else { 0 }),
                    BinaryOperator::Or => Some(if lhs != 0 || rhs != 0 { 1 } else { 0 }),
                }
            }
            _ => None,
        }
    }

    pub(crate) fn lower_tensor_literal(
        &mut self,
        expr: &Expression,
        dtype: &IRType,
        rank: Option<usize>,
        ir_func: &mut IRFunction,
    ) -> Option<Value> {
        let ExpressionKind::ArrayLiteral { elements } = &expr.kind else {
            return None;
        };

        match rank {
            Some(1) => {
                let mut args = Vec::with_capacity(elements.len() + 1);
                args.push(self.builder.build_const_int(ir_func, elements.len() as i64));
                for element in elements {
                    args.push(self.lower_expression(element, ir_func));
                }
                let host = match dtype {
                    IRType::Float => "spectra.std.tensor.literal_f",
                    IRType::Int => "spectra.std.tensor.literal",
                    _ => return None,
                };
                self.builder
                    .build_host_call(ir_func, host.to_string(), args, true)
            }
            Some(2) => {
                let mut rows = Vec::with_capacity(elements.len());
                let mut cols: Option<usize> = None;
                for row in elements {
                    let ExpressionKind::ArrayLiteral {
                        elements: row_elements,
                    } = &row.kind
                    else {
                        return None;
                    };
                    if let Some(expected_cols) = cols {
                        if row_elements.len() != expected_cols {
                            return None;
                        }
                    } else {
                        cols = Some(row_elements.len());
                    }
                    rows.push(row_elements);
                }

                let cols = cols.unwrap_or(0);
                let flat_len = elements.len().saturating_mul(cols);
                let mut args = Vec::with_capacity(flat_len + 2);
                args.push(self.builder.build_const_int(ir_func, elements.len() as i64));
                args.push(self.builder.build_const_int(ir_func, cols as i64));
                for row in rows {
                    for element in row {
                        args.push(self.lower_expression(element, ir_func));
                    }
                }
                let host = match dtype {
                    IRType::Float => "spectra.std.tensor.literal2_f",
                    IRType::Int => "spectra.std.tensor.literal2",
                    _ => return None,
                };
                self.builder
                    .build_host_call(ir_func, host.to_string(), args, true)
            }
            _ => None,
        }
    }
}
