impl Parser {
    /// Parse trait implementation: impl TraitName for TypeName { methods... }
    fn parse_trait_impl_block(
        &mut self,
        start_span: Span,
        trait_name: String,
        type_name: String,
        type_args: Vec<TypeAnnotation>,
        impl_type_params: Vec<TypeParameter>,
    ) -> Result<TraitImpl, ()> {
        self.consume_symbol('{', "Expected '{' to start trait impl block")?;

        let mut implemented_methods: HashSet<String> = HashSet::new();
        let mut methods = Vec::new();

        while !self.check_symbol('}') && !self.is_at_end() {
            // Parse method (igual ao impl block regular)
            let is_async = if self.check_keyword(Keyword::Async) {
                self.advance();
                true
            } else {
                false
            };
            let _fn_keyword_span = self.consume_function_keyword("Expected 'func' for method")?;

            let (method_name, method_name_span) =
                self.consume_identifier("Expected method name")?;

            self.consume_symbol('(', "Expected '(' after method name")?;

            // Parse parameters
            let mut params = Vec::new();

            while !self.check_symbol(')') && !self.is_at_end() {
                let param_start = self.current().span;

                // Check for self parameter
                let is_self_param =
                    matches!(&self.current().kind, TokenKind::Identifier(id) if id == "self");

                let (is_self, is_reference, is_mutable) = if is_self_param {
                    self.advance();
                    (true, false, false)
                } else if self.check_symbol('&') {
                    self.advance();
                    let is_mut = if self.check_keyword(Keyword::Mut) {
                        self.advance();
                        true
                    } else {
                        false
                    };
                    let is_self_after_ref =
                        matches!(&self.current().kind, TokenKind::Identifier(id) if id == "self");
                    if !is_self_after_ref {
                        self.error("Expected 'self' after '&'");
                        return Err(());
                    }
                    self.advance();
                    (true, true, is_mut)
                } else {
                    // Regular parameter
                    let (param_name, _) = self.consume_identifier("Expected parameter name")?;
                    self.consume_symbol(':', "Expected ':' after parameter name")?;
                    let param_type = self.parse_type_annotation()?;
                    let param_end = param_type.span;

                    params.push(Parameter {
                        name: param_name,
                        type_annotation: Some(param_type),
                        is_self: false,
                        is_reference: false,
                        is_mutable: false,
                        span: span_union(param_start, param_end),
                    });

                    if self.check_symbol(',') {
                        self.advance();
                    }
                    continue;
                };

                params.push(Parameter {
                    name: "self".to_string(),
                    type_annotation: None,
                    is_self,
                    is_reference,
                    is_mutable,
                    span: param_start,
                });

                if self.check_symbol(',') {
                    self.advance();
                }
            }

            self.consume_symbol(')', "Expected ')' after method parameters")?;

            // Optional return type
            let return_type = if self.check_keyword(Keyword::Returns) {
                self.advance(); // consume 'returns'
                Some(self.parse_type_annotation()?)
            } else {
                None
            };

            let body = if is_async {
                self.push_async_context();
                let parsed = self.parse_block();
                self.pop_async_context();
                parsed?
            } else {
                self.parse_block()?
            };
            let body_end_span = body.span;

            let method_name_for_checks = method_name.clone();

            methods.push(Method {
                name: method_name,
                is_async,
                params,
                return_type,
                body,
                span: span_union(method_name_span, body_end_span),
                // Methods in trait impls always implement the (public) trait contract.
                visibility: Visibility::Public,
            });

            if !implemented_methods.insert(method_name_for_checks.clone()) {
                let message = format!(
                    "Method '{}' is implemented more than once in trait impl for '{}'",
                    method_name_for_checks, trait_name
                );
                self.error_at(&message, method_name_span);
            }
        }

        let end_span = self.consume_symbol('}', "Expected '}' to end trait impl block")?;

        Ok(TraitImpl {
            trait_name,
            type_name,
            methods,
            span: span_union(start_span, end_span),
            type_args,
            type_params: impl_type_params,
        })
    }

    /// Parse generic type parameters: <T, U: Trait, V: Trait1 + Trait2>
    fn parse_type_parameters(&mut self) -> Result<Vec<TypeParameter>, ()> {
        self.consume_symbol('<', "Expected '<'")?;

        let mut type_params = Vec::new();

        // Empty type parameter list
        if self.check_symbol('>') {
            self.advance();
            return Ok(type_params);
        }

        loop {
            let param_start = self.current().span;
            let (name, _) = self.consume_identifier("Expected type parameter name")?;

            // Parse optional trait bounds: T: Trait1 + Trait2
            let mut bounds = Vec::new();
            if self.check_symbol(':') {
                self.advance(); // consume ':'

                // Parse trait bounds separated by '+'
                loop {
                    let (trait_name, _) = self.consume_identifier("Expected trait name")?;
                    bounds.push(trait_name);

                    // Check for '+'
                    if self.check_symbol('+') {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }

            type_params.push(TypeParameter {
                name,
                bounds,
                span: param_start,
            });

            // Check for continuation or end
            if self.check_symbol(',') {
                self.advance();
                // Allow trailing comma
                if self.check_symbol('>') {
                    break;
                }
            } else if self.check_symbol('>') {
                break;
            } else {
                self.error("Expected ',' or '>' in type parameter list");
                return Err(());
            }
        }

        self.consume_symbol('>', "Expected '>' to close type parameters")?;

        Ok(type_params)
    }

}
