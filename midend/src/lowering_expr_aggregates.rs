impl ASTLowering {
    fn lower_expression_aggregates(&mut self, expr: &Expression, ir_func: &mut IRFunction) -> Value {
        match &expr.kind {
            ExpressionKind::If {
                condition,
                then_block,
                elif_blocks,
                else_block,
            } => {
                let then_bb = ir_func.add_block("if.then");
                let merge_bb = ir_func.add_block("if.merge");
                let else_bb = if else_block.is_some() {
                    Some(ir_func.add_block("if.else"))
                } else {
                    None
                };

                let first_false_bb = if !elif_blocks.is_empty() {
                    ir_func.add_block("if.elif.0.cond")
                } else if let Some(else_id) = else_bb {
                    else_id
                } else {
                    merge_bb
                };

                let cond_value = self.lower_expression(condition, ir_func);
                self.builder
                    .build_cond_branch(ir_func, cond_value, then_bb, first_false_bb);

                let mut phi_inputs: Vec<(Value, usize)> = Vec::new();
                let mut merge_has_predecessor = first_false_bb == merge_bb;

                self.builder.set_current_block(then_bb);
                let (then_value, then_final_block, then_has_terminator) =
                    self.lower_branch_block_result(then_block, ir_func, then_bb);

                if let Some(value) = then_value {
                    if !then_has_terminator {
                        phi_inputs.push((value, then_final_block));
                    }
                }

                if !then_has_terminator {
                    self.builder.build_branch(ir_func, merge_bb);
                    merge_has_predecessor = true;
                }

                let mut current_false_block = first_false_bb;

                for (idx, (elif_condition, elif_body)) in elif_blocks.iter().enumerate() {
                    self.builder.set_current_block(current_false_block);
                    let cond_value = self.lower_expression(elif_condition, ir_func);

                    let elif_body_block = ir_func.add_block(format!("if.elif.{}.body", idx));
                    let next_false_block = if idx + 1 < elif_blocks.len() {
                        ir_func.add_block(format!("if.elif.{}.cond", idx + 1))
                    } else if let Some(else_id) = else_bb {
                        else_id
                    } else {
                        merge_bb
                    };

                    self.builder.build_cond_branch(
                        ir_func,
                        cond_value,
                        elif_body_block,
                        next_false_block,
                    );

                    if next_false_block == merge_bb {
                        merge_has_predecessor = true;
                    }

                    self.builder.set_current_block(elif_body_block);
                    let (elif_value, elif_final_block, elif_has_terminator) =
                        self.lower_branch_block_result(elif_body, ir_func, elif_body_block);

                    if let Some(value) = elif_value {
                        if !elif_has_terminator {
                            phi_inputs.push((value, elif_final_block));
                        }
                    }

                    if !elif_has_terminator {
                        self.builder.build_branch(ir_func, merge_bb);
                        merge_has_predecessor = true;
                    }

                    current_false_block = next_false_block;
                }

                if let Some(else_block_ast) = else_block {
                    self.builder.set_current_block(current_false_block);
                    let (else_value, else_final_block, else_has_terminator) = self
                        .lower_branch_block_result(else_block_ast, ir_func, current_false_block);

                    if let Some(value) = else_value {
                        if !else_has_terminator {
                            phi_inputs.push((value, else_final_block));
                        }
                    }

                    if !else_has_terminator {
                        self.builder.build_branch(ir_func, merge_bb);
                        merge_has_predecessor = true;
                    }
                } else if current_false_block != merge_bb {
                    self.builder.set_current_block(current_false_block);
                    self.builder.build_branch(ir_func, merge_bb);
                    merge_has_predecessor = true;
                }

                if merge_has_predecessor {
                    self.builder.set_current_block(merge_bb);
                    if phi_inputs.len() >= 2 {
                        self.builder.build_phi(ir_func, phi_inputs)
                    } else {
                        self.builder.build_const_int(ir_func, 0)
                    }
                } else {
                    // Merge block is unreachable (all branches return/tail-call);
                    // seal it with Unreachable so the IR verifier is happy.
                    self.builder.set_current_block(merge_bb);
                    self.builder.build_unreachable(ir_func);
                    self.builder.build_const_int(ir_func, 0)
                }
            }
            ExpressionKind::Unless {
                condition,
                then_block,
                else_block,
            } => {
                // Unless is equivalent to: if (!condition) { then_block } else { else_block }
                let unless_then_bb = ir_func.add_block("unless.then");
                let unless_else_bb = ir_func.add_block("unless.else");
                let unless_merge_bb = ir_func.add_block("unless.merge");

                // `unless condition { then } else { else }` branches to `else`
                // when the condition is true and to `then` when it is false.
                let cond_value = self.lower_expression(condition, ir_func);

                self.builder
                    .build_cond_branch(ir_func, cond_value, unless_else_bb, unless_then_bb);

                // Unless body (executes when condition is false)
                self.builder.set_current_block(unless_then_bb);
                let mut unless_value = None;
                self.lower_block(&then_block.statements, ir_func);
                if let Some(Statement {
                    kind: StatementKind::Expression(expr),
                    ..
                }) = then_block.statements.last()
                {
                    unless_value = Some(self.lower_expression(expr, ir_func));
                }
                let unless_then_final = self.builder.get_current_block().unwrap_or(unless_then_bb);

                // Only add branch if block doesn't have terminator
                if let Some(block) = ir_func.get_block_mut(unless_then_final) {
                    if block.terminator.is_none() {
                        self.builder.build_branch(ir_func, unless_merge_bb);
                    }
                }

                // Else branch (executes when condition is true)
                self.builder.set_current_block(unless_else_bb);
                let mut unless_else_value = None;
                if let Some(else_body) = else_block {
                    self.lower_block(&else_body.statements, ir_func);
                    if let Some(Statement {
                        kind: StatementKind::Expression(expr),
                        ..
                    }) = else_body.statements.last()
                    {
                        unless_else_value = Some(self.lower_expression(expr, ir_func));
                    }
                }
                let unless_else_final = self.builder.get_current_block().unwrap_or(unless_else_bb);

                // Check if else block has terminator
                let else_has_terminator = if let Some(block) = ir_func.get_block(unless_else_final)
                {
                    block.terminator.is_some()
                } else {
                    false
                };

                // Only add branch if block doesn't have terminator
                if !else_has_terminator {
                    if let Some(block) = ir_func.get_block_mut(unless_else_final) {
                        if block.terminator.is_none() {
                            self.builder.build_branch(ir_func, unless_merge_bb);
                        }
                    }
                }

                // Check if then block has terminator
                let then_has_terminator = if let Some(block) = ir_func.get_block(unless_then_final)
                {
                    block.terminator.is_some()
                } else {
                    false
                };

                // Only use merge block if at least one branch reaches it
                if !then_has_terminator || !else_has_terminator {
                    // Merge block with PHI node
                    self.builder.set_current_block(unless_merge_bb);

                    // If both branches produce values, create PHI node
                    if let (Some(then_val), Some(else_val)) = (unless_value, unless_else_value) {
                        self.builder.build_phi(
                            ir_func,
                            vec![(then_val, unless_then_final), (else_val, unless_else_final)],
                        )
                    } else {
                        // No value produced (void)
                        self.builder.build_const_int(ir_func, 0)
                    }
                } else {
                    // Both branches have terminators (returns), merge block is unreachable.
                    self.builder.set_current_block(unless_merge_bb);
                    self.builder.build_unreachable(ir_func);
                    self.builder.build_const_int(ir_func, 0)
                }
            }
            ExpressionKind::Grouping(inner) => self.lower_expression(inner, ir_func),
            ExpressionKind::ArrayLiteral { elements } => {
                // Alocar memória para o array
                let size = elements.len();
                // Inferir o tipo dos elementos
                let elem_type = self.infer_array_element_type(elements);
                if Self::ir_type_contains_unknown(&elem_type) {
                    return self.invalid_value(
                        "array literal reached lowering without a concrete element type",
                    );
                }

                if size == 0 {
                    let array_type = IRType::Array {
                        element_type: Box::new(elem_type),
                        size: 0,
                    };
                    return self.builder.build_alloca(ir_func, array_type);
                }

                // Alocar espaço para o array no stack (tipo Array com tamanho)
                let array_type = IRType::Array {
                    element_type: Box::new(elem_type.clone()),
                    size,
                };
                let array_ptr = self.builder.build_alloca(ir_func, array_type);

                // Inicializar cada elemento
                for (i, elem_expr) in elements.iter().enumerate() {
                    let elem_value = self.lower_expression_as_type(elem_expr, &elem_type, ir_func);
                    let index_value = self.builder.build_const_int(ir_func, i as i64);
                    let elem_ptr = self.builder.build_getelementptr(
                        ir_func,
                        array_ptr,
                        index_value,
                        elem_type.clone(),
                    );
                    self.builder.build_store(ir_func, elem_ptr, elem_value);
                }

                // Retornar o ponteiro para o array
                array_ptr
            }
            ExpressionKind::IndexAccess { array, index } => {
                // Avaliar a expressão do array
                let array_ptr = self.lower_expression(array, ir_func);

                // Avaliar o índice
                let index_value = self.lower_expression(index, ir_func);

                let elem_type = match self.infer_expr_ir_type(array) {
                    IRType::Array { element_type, .. } => *element_type,
                    other => {
                        return self.invalid_value(format!(
                            "Index access expected array expression, found {:?}",
                            other
                        ));
                    }
                };
                let elem_ptr = self.builder.build_getelementptr(
                    ir_func,
                    array_ptr,
                    index_value,
                    elem_type.clone(),
                );

                self.builder.build_load_typed(ir_func, elem_ptr, elem_type)
            }
            ExpressionKind::TupleLiteral { elements } => {
                // Alocar memória para a tuple
                let size = elements.len();
                if size == 0 {
                    // Tuple vazia — emitir constante 0 como sentinel em vez de
                    // consumir um Value ID sem instrução.
                    return self.builder.build_const_int(ir_func, 0);
                }

                // Determinar os tipos dos elementos usando inferência
                let elem_types: Vec<IRType> = elements
                    .iter()
                    .map(|e| self.infer_expr_ir_type(e))
                    .collect();

                // Alocar espaço para a tuple no stack
                let tuple_type = IRType::Tuple {
                    elements: elem_types.clone(),
                };
                let tuple_ptr = self.builder.build_alloca(ir_func, tuple_type);

                // Inicializar cada elemento (offsets com padding)
                let layout = layout::layout_of(&elem_types);
                for (i, elem_expr) in elements.iter().enumerate() {
                    let elem_value =
                        self.lower_expression_as_type(elem_expr, &elem_types[i], ir_func);
                    let elem_ptr =
                        self.builder
                            .build_field_ptr(ir_func, tuple_ptr, layout.offsets[i] as i64);
                    self.builder.build_store(ir_func, elem_ptr, elem_value);
                }

                // Retornar o ponteiro para a tuple
                tuple_ptr
            }
            ExpressionKind::TupleAccess { tuple, index } => {
                // Avaliar a expressão da tuple
                let tuple_ptr = self.lower_expression(tuple, ir_func);

                // Inferir o tipo e o offset somente de uma tuple concreta.  Um
                // acesso inválido não pode virar `int` e seguir para o backend.
                let tuple_type = self.infer_expr_ir_type(tuple);
                let (elem_type, offset) = match tuple_type {
                    IRType::Tuple { elements } if *index < elements.len() => {
                        let offset = match layout::layout_of(&elements).offsets.get(*index) {
                            Some(offset) => *offset,
                            None => {
                                return self.invalid_value(format!(
                                    "tuple index {} has no computed layout offset",
                                    index
                                ));
                            }
                        };
                        (elements[*index].clone(), offset)
                    }
                    IRType::Tuple { .. } => {
                        return self.invalid_value(format!(
                            "tuple index {} is out of bounds",
                            index
                        ));
                    }
                    other => {
                        return self.invalid_value(format!(
                            "tuple access expected tuple expression, found {:?}",
                            other
                        ));
                    }
                };
                let elem_ptr = self.builder.build_field_ptr(ir_func, tuple_ptr, offset as i64);

                // Carregar o valor do elemento
                self.builder.build_load_typed(ir_func, elem_ptr, elem_type)
            }
            _ => unreachable!("lowering expression category mismatch"),
        }
    }
}
