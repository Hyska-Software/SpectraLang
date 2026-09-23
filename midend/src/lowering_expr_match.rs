use super::*;

impl ASTLowering {
    pub(crate) fn lower_expression_match(
        &mut self,
        expr: &Expression,
        ir_func: &mut IRFunction,
    ) -> Value {
        match &expr.kind {
            ExpressionKind::Match { scrutinee, arms } => {
                // Lower the value being matched.
                let scrutinee_value = self.lower_expression(scrutinee, ir_func);

                let scrutinee_type = self.infer_expr_ir_type(scrutinee);
                let scrutinee_enum_name = match &scrutinee_type {
                    IRType::Enum { name, .. } => Some(name.clone()),
                    IRType::Generic { .. } => {
                        self.ir_nominal_name(&scrutinee_type).map(str::to_string)
                    }
                    _ => None,
                };

                // Create one block per arm plus an exit block.
                let exit_block = ir_func.add_block("match_exit");

                // A "guaranteed" arm is irrefutably matched and has no
                // guard: it always matches. Without one, every check can fail
                // at runtime and no result value is ever produced.
                let has_guaranteed_arm = arms
                    .iter()
                    .any(|arm| arm.guard.is_none() && pattern_is_irrefutable(&arm.pattern));
                let mut arm_check_blocks = Vec::new();
                let mut arm_body_blocks = Vec::new();

                // Create two blocks per arm: one to check the pattern, one to
                // run the body.
                for (idx, _) in arms.iter().enumerate() {
                    arm_check_blocks.push(ir_func.add_block(format!("match_check_{}", idx)));
                    arm_body_blocks.push(ir_func.add_block(format!("match_body_{}", idx)));
                }

                // Without a guaranteed arm, matching arms branch to their own
                // continuation block; the exit block is sealed with
                // Unreachable (no pattern matched => result never stored).
                let match_end = if has_guaranteed_arm {
                    None
                } else {
                    Some(ir_func.add_block("match_end"))
                };

                // Infer the result type by combining the types of every arm.
                let mut result_type = if let Some(first_arm) = arms.first() {
                    self.infer_match_arm_type(
                        &first_arm.pattern,
                        &first_arm.body,
                        scrutinee_enum_name.as_deref(),
                        &scrutinee_type,
                    )
                } else {
                    IRType::Int
                };
                for arm in arms.iter().skip(1) {
                    let arm_type = self.infer_match_arm_type(
                        &arm.pattern,
                        &arm.body,
                        scrutinee_enum_name.as_deref(),
                        &scrutinee_type,
                    );
                    if let Some(merged) = self.merge_types(&result_type, &arm_type) {
                        result_type = merged;
                    }
                }
                let result_alloca = if result_type != IRType::Void {
                    Some(self.builder.build_alloca(ir_func, result_type.clone()))
                } else {
                    None
                };

                // Branch from the current block to the first check.
                self.builder.build_branch(ir_func, arm_check_blocks[0]);

                // Process each arm.
                for (idx, arm) in arms.iter().enumerate() {
                    // Pattern check block.
                    self.builder.set_current_block(arm_check_blocks[idx]);

                    let pattern_matches = self.lower_pattern_check(
                        &arm.pattern,
                        scrutinee_value,
                        scrutinee_enum_name.as_deref(),
                        Some(&scrutinee_type),
                        ir_func,
                    );

                    // Next block: the next arm's check, or the exit block
                    // when no arms remain.
                    let next_check = if idx + 1 < arms.len() {
                        arm_check_blocks[idx + 1]
                    } else {
                        exit_block
                    };

                    // On pattern match, go to the body; otherwise, the next
                    // check.
                    self.builder.build_cond_branch(
                        ir_func,
                        pattern_matches,
                        arm_body_blocks[idx],
                        next_check,
                    );

                    // Arm body block.
                    self.builder.set_current_block(arm_body_blocks[idx]);

                    // Create the pattern bindings before running the body.
                    self.value_map.push_scope();
                    self.variable_types.push_scope();
                    self.array_map.push_scope();
                    self.range_map.push_scope();
                    self.struct_var_map.push_scope();

                    self.lower_pattern_bindings(
                        &arm.pattern,
                        scrutinee_value,
                        scrutinee_enum_name.as_deref(),
                        Some(&scrutinee_type),
                        ir_func,
                    );

                    // If the arm has a guard (e.g. `Pattern if cond =>`),
                    // evaluate the condition and fall through to the next
                    // check when it is false.
                    if let Some(guard_expr) = &arm.guard {
                        let guard_val = self.lower_expression(guard_expr, ir_func);
                        let guard_body_block =
                            ir_func.add_block(format!("match_guard_body_{}", idx));
                        self.builder.build_cond_branch(
                            ir_func,
                            guard_val,
                            guard_body_block,
                            next_check,
                        );
                        self.builder.set_current_block(guard_body_block);
                    }

                    let body_value = self.lower_expression(&arm.body, ir_func);
                    // Emit store+branch only when the arm did not end with an
                    // explicit return.
                    let arm_final_block = self
                        .builder
                        .get_current_block()
                        .unwrap_or(arm_body_blocks[idx]);
                    let arm_terminated = ir_func
                        .get_block(arm_final_block)
                        .map(|b| b.terminator.is_some())
                        .unwrap_or(false);
                    if !arm_terminated {
                        if let Some(result_alloca) = result_alloca {
                            self.builder.build_store(ir_func, result_alloca, body_value);
                        }
                        self.builder
                            .build_branch(ir_func, match_end.unwrap_or(exit_block));
                    }

                    self.struct_var_map.pop_scope();
                    self.array_map.pop_scope();
                    self.range_map.pop_scope();
                    self.variable_types.pop_scope();
                    self.value_map.pop_scope();
                }

                // Exit block.
                self.builder.set_current_block(exit_block);

                // No arm guarantees a match: if every check fails at runtime,
                // control reaches here with `result_alloca` never stored —
                // reading it would read uninitialized memory. Seal this block
                // with Unreachable; normal control flow continues through
                // `match_end`, reached only by the arms that matched.
                if !has_guaranteed_arm {
                    self.builder.build_unreachable(ir_func);
                    // `match_end` is created exactly when no arm is
                    // irrefutably matched. If that invariant ever breaks,
                    // record a lowering error and hand back a poison value
                    // instead of panicking: `lower_module` rejects the module
                    // before verification or the backend can see it.
                    let Some(match_end) = match_end else {
                        return self.invalid_value(
                            "match lowering requires a match_end block when no arm is irrefutably matched",
                        );
                    };
                    self.builder.set_current_block(match_end);
                }

                if let Some(result_alloca) = result_alloca {
                    self.builder
                        .build_load_typed(ir_func, result_alloca, result_type.clone())
                } else {
                    self.builder.build_const_int(ir_func, 0)
                }
            }
            // `lower_expression` dispatches match expressions to this helper;
            // any other category is a lowering bug. Report it as a normal
            // midend error (with a poison value) instead of panicking.
            other => self.invalid_value(format!(
                "lower_expression_match reached with a non-match expression category: {:?}",
                other
            )),
        }
    }
}

/// Pattern that matches any value of the type (irrefutable), ignoring guards.
pub(crate) fn pattern_is_irrefutable(pattern: &spectra_compiler::ast::Pattern) -> bool {
    use spectra_compiler::ast::Pattern;

    match pattern {
        Pattern::Wildcard(_) | Pattern::Identifier(_, _) => true,
        Pattern::Tuple(elements) => elements.iter().all(pattern_is_irrefutable),
        Pattern::Struct { fields, .. } => fields.iter().all(|(_, p)| pattern_is_irrefutable(p)),
        Pattern::Or(patterns) => patterns.iter().any(pattern_is_irrefutable),
        // Literal and enum-variant patterns are refutable; enum
        // exhaustiveness is the semantic analyzer's responsibility, not the
        // lowering's.
        Pattern::Literal(_) | Pattern::EnumVariant { .. } => false,
    }
}
