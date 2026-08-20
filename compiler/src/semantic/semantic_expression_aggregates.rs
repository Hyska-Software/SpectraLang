impl SemanticAnalyzer {
    fn analyze_expression_aggregates(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::If {
                condition,
                then_block,
                elif_blocks,
                else_block,
            } => {
                self.analyze_expression(condition);
                self.analyze_block(then_block);

                for (elif_cond, elif_body) in elif_blocks {
                    self.analyze_expression(elif_cond);
                    self.analyze_block(elif_body);
                }

                if let Some(ref else_body) = else_block {
                    self.analyze_block(else_body);
                }

                let mut branch_types = Vec::new();
                branch_types.push(self.infer_block_type(then_block));
                for (_, elif_body) in elif_blocks {
                    branch_types.push(self.infer_block_type(elif_body));
                }
                branch_types.push(match else_block {
                    Some(block) => self.infer_block_type(block),
                    None => Type::Unit,
                });

                if let Some((expected, found)) = self.branch_type_mismatch(&branch_types) {
                    self.error(
                        format!(
                            "Incompatible branch types in if expression: expected {}, found {}",
                            type_name(&expected),
                            type_name(&found)
                        ),
                        expr.span,
                    );
                }
            }
            ExpressionKind::Unless {
                condition,
                then_block,
                else_block,
            } => {
                self.analyze_expression(condition);
                self.analyze_block(then_block);

                if let Some(ref else_body) = else_block {
                    self.analyze_block(else_body);
                }

                let mut branch_types = Vec::new();
                branch_types.push(self.infer_block_type(then_block));
                branch_types.push(match else_block {
                    Some(block) => self.infer_block_type(block),
                    None => Type::Unit,
                });

                if let Some((expected, found)) = self.branch_type_mismatch(&branch_types) {
                    self.error(
                        format!(
                            "Incompatible branch types in unless expression: expected {}, found {}",
                            type_name(&expected),
                            type_name(&found)
                        ),
                        expr.span,
                    );
                }
            }
            ExpressionKind::Grouping(inner) => {
                self.analyze_expression(inner);
            }
            ExpressionKind::ArrayLiteral { elements } => {
                let expected_element_type = match self.current_expected_type.as_ref() {
                    Some(Type::Array { element_type, .. })
                        if !matches!(element_type.as_ref(), Type::Unknown) =>
                    {
                        Some(element_type.as_ref())
                    }
                    _ => None,
                };
                if elements.is_empty() && expected_element_type.is_none() {
                    self.error_coded_with_hint(
                        "E004",
                        "Cannot infer the element type of an empty array",
                        expr.span,
                        "Add an explicit array type annotation or provide at least one element.",
                    );
                }
                // Analyze all elements
                for element in elements {
                    self.analyze_expression(element);
                }

                // Check that all elements have the same type
                if !elements.is_empty() {
                    let first_type = self.infer_expression_type(&elements[0]);
                    for (i, element) in elements.iter().enumerate().skip(1) {
                        let elem_type = self.infer_expression_type(element);
                        if first_type != Type::Unknown
                            && elem_type != Type::Unknown
                            && first_type != elem_type
                        {
                            self.error(
                                format!(
                                    "Array element {} has type {}, expected {}",
                                    i,
                                    type_name(&elem_type),
                                    type_name(&first_type)
                                ),
                                element.span,
                            );
                        }
                    }
                }
            }
            ExpressionKind::IndexAccess { array, index } => {
                self.analyze_expression(array);
                self.analyze_expression(index);

                self.validate_static_array_index(array, index);

                // Check that index is an integer
                let index_type = self.infer_expression_type(index);
                if matches!(index_type, Type::Unknown) {
                    if !self.has_error_at_span(index.span) {
                        self.error_with_hint(
                            "Cannot determine the type of the array index",
                            index.span,
                            "Use an integer expression; unresolved index types are rejected before lowering.",
                        );
                    }
                } else if !matches!(index_type, Type::Int) {
                    self.error(
                        format!(
                            "Array index must be an integer, found {}",
                            type_name(&index_type)
                        ),
                        index.span,
                    );
                }

                // Check that array is actually an array
                let array_type = self.infer_expression_type(array);
                if matches!(array_type, Type::Unknown) {
                    if !self.has_error_at_span(array.span) {
                        self.error_with_hint(
                            "Cannot determine the type of the indexed value",
                            array.span,
                            "Use a typed array or string; unresolved values cannot be indexed.",
                        );
                    }
                } else if !matches!(array_type, Type::Array { .. } | Type::String) {
                    self.error(
                        format!(
                            "Cannot index into non-array type {}",
                            type_name(&array_type)
                        ),
                        array.span,
                    );
                }
            }
            ExpressionKind::TupleLiteral { elements } => {
                // Analyze all elements
                for element in elements {
                    self.analyze_expression(element);
                }
            }
            ExpressionKind::TupleAccess { tuple, index } => {
                self.analyze_expression(tuple);

                // Check that tuple is actually a tuple
                let tuple_type = self.infer_expression_type(tuple);
                match tuple_type {
                    Type::Tuple { elements } => {
                        if *index >= elements.len() {
                            self.error(
                                format!(
                                    "Tuple index {} out of bounds (tuple has {} elements)",
                                    index,
                                    elements.len()
                                ),
                                tuple.span,
                            );
                        }
                    }
                    Type::Unknown => {
                        if !self.has_error_at_span(tuple.span) {
                            self.error_with_hint(
                                "Cannot determine the type of the tuple expression",
                                tuple.span,
                                "Use a typed tuple; unresolved values cannot be accessed by position.",
                            );
                        }
                    }
                    _ => {
                        self.error(
                            format!(
                                "Cannot access tuple element on non-tuple type {:?}",
                                tuple_type
                            ),
                            tuple.span,
                        );
                    }
                }
            }
            _ => unreachable!("expression category mismatch"),
        }
    }
}
