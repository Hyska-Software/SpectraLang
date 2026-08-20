impl SemanticAnalyzer {
    fn analyze_expression_literals(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::Identifier(name) => {
                // Check if identifier is declared
                if let Some(info) = self.lookup_symbol(name) {
                    let info = info.clone();
                    self.symbol_resolutions.insert(expr.span, info);
                } else if self.functions.contains_key(name) {
                    let info = SymbolInfo {
                        is_local: false,
                        def_span: None,
                        ty: Type::Unknown,
                    };
                    self.symbol_resolutions.insert(expr.span, info);
                } else {
                    if self.module_namespaces.contains(name.as_str()) {
                        // Valid module namespace identifier (e.g. "std" in std.string.len)
                        self.symbol_resolutions.insert(
                            expr.span,
                            SymbolInfo {
                                is_local: false,
                                def_span: None,
                                ty: Type::Unknown,
                            },
                        );
                    } else {
                        let hint = self.suggest_name(name);
                        if let Some(hint) = hint {
                            self.error_coded_with_hint(
                                "E001",
                                format!("Undefined variable or function '{}'", name),
                                expr.span,
                                hint,
                            );
                        } else {
                            self.error_coded(
                                "E001",
                                format!("Undefined variable or function '{}'", name),
                                expr.span,
                            );
                        }
                    }
                }
            }
            ExpressionKind::NumberLiteral(_)
            | ExpressionKind::StringLiteral(_)
            | ExpressionKind::BoolLiteral(_) => {
                // Literals are always valid
                let ty = self.infer_expression_type(expr);
                self.symbol_resolutions.insert(
                    expr.span,
                    SymbolInfo {
                        is_local: false,
                        def_span: None,
                        ty,
                    },
                );
            }
            _ => unreachable!("expression category mismatch"),
        }
    }
}
