impl SemanticAnalyzer {
    fn analyze_expression_call(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::Call { callee, arguments } => {
                let call_ty = self.infer_expression_type(expr);
                self.symbol_resolutions.insert(
                    expr.span,
                    SymbolInfo {
                        is_local: false,
                        def_span: None,
                        ty: call_ty,
                    },
                );

                // Track callee resolution if it's an identifier
                if let ExpressionKind::Identifier(name) = &callee.kind {
                    if name == "block_on" {
                        if arguments.len() != 1 {
                            self.error(
                                format!(
                                    "Function 'block_on' expects 1 argument, but {} were provided",
                                    arguments.len()
                                ),
                                expr.span,
                            );
                        } else {
                            let arg_type = self.infer_expression_type(&arguments[0]);
                            if !matches!(arg_type, Type::Task { .. } | Type::Unknown) {
                                self.error(
                                    format!(
                                        "Argument 1 of function 'block_on' has type {}, expected Task<T>",
                                        type_name(&arg_type)
                                    ),
                                    arguments[0].span,
                                );
                            }
                        }
                    } else if let Some(signature) = self.functions.get(name).cloned() {
                        let def_span = self.lookup_symbol(name).and_then(|info| info.def_span);
                        self.symbol_resolutions.insert(
                            callee.span,
                            SymbolInfo {
                                is_local: false,
                                def_span,
                                ty: signature.return_type.clone(),
                            },
                        );
                        // Validate number of arguments
                        if arguments.len() != signature.params.len() {
                            self.error(
                                format!(
                                    "Function '{}' expects {} arguments, but {} were provided",
                                    name,
                                    signature.params.len(),
                                    arguments.len()
                                ),
                                expr.span,
                            );
                        } else {
                            // Validate argument types
                            for (i, (arg, expected_type)) in
                                arguments.iter().zip(&signature.params).enumerate()
                            {
                                let saved_expected = self.current_expected_type.clone();
                                self.current_expected_type = Some(expected_type.clone());
                                let arg_type = self.infer_expression_type(arg);
                                self.current_expected_type = saved_expected;
                                if matches!(arg_type, Type::Unknown) {
                                    self.error_with_hint(
                                        format!(
                                            "Argument {} of function '{}' has an unknown or uninferrable type",
                                            i + 1,
                                            name,
                                        ),
                                        arg.span,
                                        "Add an explicit type annotation to resolve the argument type.",
                                    );
                                } else if *expected_type != Type::Unknown
                                    && !self.types_match(&arg_type, expected_type)
                                {
                                    self.error(
                                        format!(
                                            "Argument {} of function '{}' has type {}, expected {}",
                                            i + 1,
                                            name,
                                            type_name(&arg_type),
                                            type_name(expected_type)
                                        ),
                                        arg.span,
                                    );
                                }
                            }

                            let substitutions = self
                                .infer_type_parameter_substitutions(&signature.params, arguments);
                            let type_params = self
                                .function_type_params
                                .get(name)
                                .cloned()
                                .unwrap_or_default();
                            self.validate_type_parameter_bounds(
                                name,
                                &type_params,
                                &substitutions,
                                expr.span,
                            );
                        }
                    } else if let Some(symbol_ty) =
                        self.lookup_symbol(name).map(|info| info.ty.clone())
                    {
                        if let Type::Fn {
                            params,
                            return_type: _,
                        } = symbol_ty
                        {
                            if arguments.len() != params.len() {
                                self.error(
                                    format!(
                                        "Function value '{}' expects {} arguments, but {} were provided",
                                        name,
                                        params.len(),
                                        arguments.len()
                                    ),
                                    expr.span,
                                );
                            } else {
                                for (i, (arg, expected_type)) in
                                    arguments.iter().zip(params.iter()).enumerate()
                                {
                                    let arg_type = self.infer_expression_type(arg);
                                    if matches!(arg_type, Type::Unknown) {
                                        self.error_with_hint(
                                            format!(
                                                "Argument {} of function value '{}' has an unknown or uninferrable type",
                                                i + 1,
                                                name,
                                            ),
                                            arg.span,
                                            "Add an explicit type annotation to resolve the argument type.",
                                        );
                                    } else if *expected_type != Type::Unknown
                                        && !self.types_match(&arg_type, expected_type)
                                    {
                                        self.error(
                                            format!(
                                                "Argument {} of function value '{}' has type {}, expected {}",
                                                i + 1,
                                                name,
                                                type_name(&arg_type),
                                                type_name(expected_type)
                                            ),
                                            arg.span,
                                        );
                                    }
                                }
                            }
                        } else {
                            self.error(format!("'{}' is not callable", name), callee.span);
                        }
                    } else if self.lookup_symbol(name).is_none() {
                        let hint = self.suggest_name(name);
                        if let Some(hint) = hint {
                            self.error_with_hint(
                                format!("Undefined function '{}'", name),
                                callee.span,
                                hint,
                            );
                        } else {
                            self.error(format!("Undefined function '{}'", name), callee.span);
                        }
                    }
                } else {
                    self.analyze_expression(callee);
                }

                // Analyze arguments
                for arg in arguments {
                    self.analyze_expression(arg);
                }

                self.validate_static_tensor_call(callee, arguments, expr.span);
            }
            _ => unreachable!("expression category mismatch"),
        }
    }
}
