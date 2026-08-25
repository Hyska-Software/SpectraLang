use super::*;

impl Parser {
    /// Parse trait declaration: trait Name: Parent1, Parent2 { fn method(&self) -> Type; }
    pub(crate) fn parse_trait_declaration(&mut self) -> Result<TraitDeclaration, ()> {
        // Expect: trait <name> [: Parent1 + Parent2] { <method signatures> }
        let start_span = self.consume_keyword(Keyword::Trait, "Expected 'trait' keyword")?;

        let (name, _name_span) = self.consume_identifier("Expected trait name")?;

        // Optional generic type parameters: trait Container<T> (R-213)
        let mut type_params = Vec::new();
        if self.check_symbol('<') {
            type_params = self.parse_type_parameters()?;
        }

        // Parse optional parent traits: trait A: B, C
        let mut parent_traits = Vec::new();
        if self.check_symbol(':') {
            self.advance(); // consume ':'

            loop {
                let (parent_name, _) = self.consume_identifier("Expected parent trait name")?;
                parent_traits.push(parent_name);

                // Check for '+' separator
                if self.check_symbol('+') {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        self.consume_symbol('{', "Expected '{' after trait name")?;

        let mut methods = Vec::new();

        // Parse method signatures (sem corpo, apenas assinaturas)
        while !self.check_symbol('}') && !self.is_at_end() {
            let (method_start, is_async) = if self.check_keyword(Keyword::Async) {
                let async_span = self.current().span;
                self.advance();
                self.consume_function_keyword(
                    "Expected 'func' after 'async' for method signature",
                )?;
                (async_span, true)
            } else {
                (
                    self.consume_function_keyword("Expected 'func' for method signature")?,
                    false,
                )
            };

            let (method_name, _method_name_span) =
                self.consume_identifier("Expected method name")?;

            self.consume_symbol('(', "Expected '(' after method name")?;

            // Parse parameters (igual a métodos regulares)
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

                // Para self, o span vai até o token atual - 1
                // Como já avançamos, precisamos usar param_start até onde paramos
                let _param_end = param_start; // Simplificado - usamos só o start
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

            // Trait methods can have:
            // 1. No body (just signature): fn method(&self) -> Type;
            // 2. Default implementation: fn method(&self) -> Type { body }
            let (body, method_end) = if self.check_symbol('{') {
                // Has default implementation
                let body = if is_async {
                    self.push_async_context();
                    let parsed = self.parse_block();
                    self.pop_async_context();
                    parsed?
                } else {
                    self.parse_block()?
                };
                let end = body.span;
                (Some(body), end)
            } else {
                // Just signature, no body
                let end = self
                    .consume_statement_terminator("Expected a line break after trait method signature")?;
                (None, end)
            };

            methods.push(TraitMethod {
                name: method_name,
                is_async,
                params,
                return_type,
                body,
                span: span_union(method_start, method_end),
            });
        }

        let end_span = self.consume_symbol('}', "Expected '}' to end trait declaration")?;

        let trait_decl = TraitDeclaration {
            name,
            parent_traits,
            methods,
            span: span_union(start_span, end_span),
            type_params,
        };

        let mut signature_map: HashMap<String, TraitMethodSignature> = HashMap::new();
        for parent_trait in &trait_decl.parent_traits {
            if let Some(parent_sigs) = self.trait_signatures.get(parent_trait).cloned() {
                for (method_name, signature) in parent_sigs {
                    signature_map.insert(method_name, signature);
                }
            }
        }
        for method in &trait_decl.methods {
            signature_map.insert(
                method.name.clone(),
                Self::trait_method_signature_from_decl(method),
            );
        }

        self.trait_signatures
            .insert(trait_decl.name.clone(), signature_map);

        Ok(trait_decl)
    }

}
