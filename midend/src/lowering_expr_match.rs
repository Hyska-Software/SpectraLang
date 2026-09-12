use super::*;

impl ASTLowering {
    pub(crate) fn lower_expression_match(
        &mut self,
        expr: &Expression,
        ir_func: &mut IRFunction,
    ) -> Value {
        match &expr.kind {
            ExpressionKind::Match { scrutinee, arms } => {
                // Lower do valor sendo matcheado
                let scrutinee_value = self.lower_expression(scrutinee, ir_func);

                let scrutinee_type = self.infer_expr_ir_type(scrutinee);
                let scrutinee_enum_name = match &scrutinee_type {
                    IRType::Enum { name, .. } => Some(name.clone()),
                    IRType::Generic { .. } => {
                        self.ir_nominal_name(&scrutinee_type).map(str::to_string)
                    }
                    _ => None,
                };

                // Criar blocos para cada arm e um bloco de saída
                let exit_block = ir_func.add_block("match_exit");

                // Um braço "garantido" é irrefutável e sem guard: casa sempre.
                // Sem ele, todos os checks podem falhar em runtime e nenhum
                // valor de resultado é produzido.
                let has_guaranteed_arm = arms
                    .iter()
                    .any(|arm| arm.guard.is_none() && pattern_is_irrefutable(&arm.pattern));
                let mut arm_check_blocks = Vec::new();
                let mut arm_body_blocks = Vec::new();

                // Criar blocos para cada arm: um para checar pattern, outro para executar body
                for (idx, _) in arms.iter().enumerate() {
                    arm_check_blocks.push(ir_func.add_block(format!("match_check_{}", idx)));
                    arm_body_blocks.push(ir_func.add_block(format!("match_body_{}", idx)));
                }

                // Sem braço garantido, os braços que casam desviam para um bloco
                // de continuação próprio; o bloco de saída fica selado com
                // Unreachable (nenhum padrão casou => resultado nunca armazenado).
                let match_end = if has_guaranteed_arm {
                    None
                } else {
                    Some(ir_func.add_block("match_end"))
                };

                // Inferir tipo do resultado combinando os tipos de cada arm
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

                // Do bloco atual, fazer branch para o primeiro check
                self.builder.build_branch(ir_func, arm_check_blocks[0]);

                // Processar cada arm
                for (idx, arm) in arms.iter().enumerate() {
                    // Bloco de checagem do pattern
                    self.builder.set_current_block(arm_check_blocks[idx]);

                    let pattern_matches = self.lower_pattern_check(
                        &arm.pattern,
                        scrutinee_value,
                        scrutinee_enum_name.as_deref(),
                        Some(&scrutinee_type),
                        ir_func,
                    );

                    // Próximo bloco: ou próximo arm, ou exit se não houver mais arms
                    let next_check = if idx + 1 < arms.len() {
                        arm_check_blocks[idx + 1]
                    } else {
                        exit_block
                    };

                    // Se pattern match, ir para body; senão, próximo check
                    self.builder.build_cond_branch(
                        ir_func,
                        pattern_matches,
                        arm_body_blocks[idx],
                        next_check,
                    );

                    // Bloco de execução do body
                    self.builder.set_current_block(arm_body_blocks[idx]);

                    // Fazer bindings do pattern antes de executar body
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

                    // Se o arm tem guard (p. ex. `Pattern if cond =>`), avaliar a condição
                    // e saltar para o próximo check se ela for falsa.
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
                    // Só emitir store+branch se o arm não terminou com return explícito
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

                // Bloco de saída
                self.builder.set_current_block(exit_block);

                // Nenhum braço garante casamento: se todos os checks falharem
                // em runtime, o fluxo chega aqui com a result_alloca nunca
                // armazenada — lê-la seria ler memória não inicializada. Sela
                // este bloco com Unreachable; o fluxo normal continua em
                // match_end, alcançado apenas pelos braços que casaram.
                if !has_guaranteed_arm {
                    self.builder.build_unreachable(ir_func);
                    self.builder.set_current_block(
                        match_end.expect("match_end must exist without guaranteed arm"),
                    );
                }

                if let Some(result_alloca) = result_alloca {
                    self.builder
                        .build_load_typed(ir_func, result_alloca, result_type.clone())
                } else {
                    self.builder.build_const_int(ir_func, 0)
                }
            }
            _ => unreachable!("lowering expression category mismatch"),
        }
    }
}

/// Padrão que casa com qualquer valor do tipo (irrefutável), ignorando guards.
pub(crate) fn pattern_is_irrefutable(pattern: &spectra_compiler::ast::Pattern) -> bool {
    use spectra_compiler::ast::Pattern;

    match pattern {
        Pattern::Wildcard(_) | Pattern::Identifier(_, _) => true,
        Pattern::Tuple(elements) => elements.iter().all(pattern_is_irrefutable),
        Pattern::Struct { fields, .. } => fields.iter().all(|(_, p)| pattern_is_irrefutable(p)),
        Pattern::Or(patterns) => patterns.iter().any(pattern_is_irrefutable),
        // Literais e variantes de enum são refutáveis; exaustividade de enum é
        // responsabilidade da análise semântica, não do lowering.
        Pattern::Literal(_) | Pattern::EnumVariant { .. } => false,
    }
}
