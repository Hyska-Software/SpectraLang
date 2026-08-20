impl Parser {
    pub(super) fn parse_function(
        &mut self,
        visibility: Visibility,
        attributes: Vec<Attribute>,
    ) -> Result<Function, ()> {
        // Expect: [async] func <name><T: Trait>(<params>) [returns type] { <body> }
        let (start_span, is_async) = if self.check_keyword(Keyword::Async) {
            let async_span = self.current().span;
            self.advance();
            if !self.check_function_keyword() {
                self.push_error_coded(
                    "P005",
                    "Expected `func` after `async` in item declaration",
                    self.current().span,
                    Some("Write `async func name(...) { ... }` or move `async` before a block expression.".to_string()),
                    Some("`async` at item scope only prefixes function declarations".to_string()),
                );
                return Err(());
            }
            self.consume_function_keyword("Expected 'func' keyword after 'async'")?;
            (async_span, true)
        } else {
            (self.consume_function_keyword("Expected 'func' keyword")?, false)
        };

        let (name, _name_span) = self.consume_identifier("Expected function name")?;

        // Parse optional type parameters: <T, U: Trait>
        let type_params = if self.check_symbol('<') {
            self.parse_type_parameters()?
        } else {
            Vec::new()
        };

        self.consume_symbol('(', "Expected '(' after function name")?;

        let params = self.parse_function_params()?;

        self.consume_symbol(')', "Expected ')' after function parameters")?;

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
        let end_span = body.span;

        Ok(Function {
            name,
            span: span_union(start_span, end_span),
            visibility,
            attributes,
            is_async,
            type_params,
            params,
            return_type,
            body,
        })
    }

    fn parse_function_params(&mut self) -> Result<Vec<FunctionParam>, ()> {
        let mut params = Vec::new();

        // Check for empty parameter list
        if self.check_symbol(')') {
            return Ok(params);
        }

        loop {
            let (name, name_span) = self.consume_identifier("Expected parameter name")?;

            // Optional type annotation
            let ty = if self.check_symbol(':') {
                self.advance(); // consume ':'
                Some(self.parse_type_annotation()?)
            } else {
                None
            };

            params.push(FunctionParam {
                name,
                span: name_span,
                ty,
            });

            if !self.check_symbol(',') {
                break;
            }
            self.advance(); // consume ','
        }

        Ok(params)
    }

    pub(super) fn parse_block(&mut self) -> Result<Block, ()> {
        let start_span = self.consume_symbol('{', "Expected '{' to start block")?;

        let mut statements = Vec::new();

        while !self.check_symbol('}') && !self.is_at_end() {
            // Try to parse as statement
            let start_position = self.position;
            match self.parse_statement() {
                Ok(stmt) => statements.push(stmt),
                Err(_) => {
                    // If parsing failed and next is '}', try parsing as final expression without ';'
                    if self.check_symbol('}') {
                        break; // Let the error propagate, block might be empty
                    }
                    self.synchronize_with_progress(start_position);
                }
            }
        }

        let end_span = self.consume_symbol('}', "Expected '}' to end block")?;

        Ok(Block {
            span: span_union(start_span, end_span),
            statements,
        })
    }

    pub(super) fn parse_struct(
        &mut self,
        visibility: Visibility,
        attributes: Vec<Attribute>,
    ) -> Result<crate::ast::Struct, ()> {
        use crate::ast::{Struct, StructField};

        // Expect: record <name> { <fields> }
        let start_span = self.consume_record_keyword("Expected 'record' keyword")?;

        let (name, _name_span) = self.consume_identifier("Expected struct name")?;

        // Parse optional type parameters: <T, U>
        let type_params = if self.check_symbol('<') {
            self.parse_type_parameters()?
        } else {
            Vec::new()
        };

        self.consume_symbol('{', "Expected '{' after struct name")?;

        let mut fields = Vec::new();

        // Parse fields
        while !self.check_symbol('}') && !self.is_at_end() {
            let field_attributes = self.parse_outer_attributes()?;
            // Parse optional visibility modifier before the field name
            let field_visibility = if self.check_keyword(Keyword::Public) {
                self.advance();
                Visibility::Public
            } else if self.check_keyword(Keyword::Internal) {
                self.advance();
                Visibility::Internal
            } else {
                Visibility::Private
            };

            // Parse field: <name>: <type>
            let (field_name, field_span) = self.consume_identifier("Expected field name")?;

            self.consume_symbol(':', "Expected ':' after field name")?;

            let field_type = self.parse_type_annotation()?;

            fields.push(StructField {
                name: field_name,
                span: field_span,
                attributes: field_attributes,
                ty: field_type,
                visibility: field_visibility,
            });

            // Optional comma
            if self.check_symbol(',') {
                self.advance();
            }
        }

        let end_span = self.consume_symbol('}', "Expected '}' to end struct")?;

        Ok(Struct {
            name,
            span: span_union(start_span, end_span),
            visibility,
            attributes,
            fields,
            type_params,
        })
    }

    pub(super) fn parse_enum(
        &mut self,
        visibility: Visibility,
        attributes: Vec<Attribute>,
    ) -> Result<crate::ast::Enum, ()> {
        use crate::ast::{Enum, EnumVariant};

        // Expect: enum <name> { <variants> }
        let start_span = self.consume_keyword(Keyword::Enum, "Expected 'enum' keyword")?;

        let (name, _name_span) = self.consume_identifier("Expected enum name")?;

        // Parse optional type parameters: <T>
        let type_params = if self.check_symbol('<') {
            self.parse_type_parameters()?
        } else {
            Vec::new()
        };

        self.consume_symbol('{', "Expected '{' after enum name")?;

        let mut variants = Vec::new();

        // Parse variants
        while !self.check_symbol('}') && !self.is_at_end() {
            let variant_attributes = self.parse_outer_attributes()?;
            // Parse variant: <name> or <name>(<types>)
            let (variant_name, variant_span) = self.consume_identifier("Expected variant name")?;

            let data = if self.check_symbol('(') {
                self.advance(); // consume '('

                let mut types = Vec::new();

                // Parse tuple variant data types
                if !self.check_symbol(')') {
                    loop {
                        types.push(self.parse_type_annotation()?);
                        if !self.check_symbol(',') {
                            break;
                        }
                        self.advance(); // consume ','
                    }
                }

                self.consume_symbol(')', "Expected ')' after variant data")?;
                Some(types)
            } else {
                None // Unit variant
            };

            // Parse struct-style variant: Variant { field: Type, ... }
            let struct_data = if self.check_symbol('{') {
                self.advance(); // consume '{'
                let mut fields = Vec::new();
                while !self.check_symbol('}') && !self.is_at_end() {
                    let (field_name, _) =
                        self.consume_identifier("Expected field name in struct variant")?;
                    self.consume_symbol(':', "Expected ':' after field name")?;
                    let field_type = self.parse_type_annotation()?;
                    fields.push((field_name, field_type));
                    if self.check_symbol(',') {
                        self.advance();
                    }
                }
                self.consume_symbol('}', "Expected '}' to end struct variant")?;
                Some(fields)
            } else {
                None
            };

            variants.push(EnumVariant {
                name: variant_name,
                span: variant_span,
                attributes: variant_attributes,
                data,
                struct_data,
            });

            // Optional comma
            if self.check_symbol(',') {
                self.advance();
            }
        }

        let end_span = self.consume_symbol('}', "Expected '}' to end enum")?;

        Ok(Enum {
            name,
            span: span_union(start_span, end_span),
            visibility,
            attributes,
            variants,
            type_params,
        })
    }

    pub(super) fn parse_impl_block(&mut self) -> Result<Item, ()> {
        // Expect: impl TypeName { methods... } ou impl TraitName for TypeName { methods... }
        let start_span = self.consume_keyword(Keyword::Impl, "Expected 'impl' keyword")?;

        // Optional generic parameter clause: impl<T: Bound> Trait for Type (R-213).
        let mut impl_type_params = Vec::new();
        if self.check_symbol('<') {
            impl_type_params = self.parse_type_parameters()?;
        }

        let (first_name, _) =
            self.consume_identifier("Expected trait or type name after 'impl'")?;

        // Optional qualified path: impl module::Type
        let mut module_path = None;
        let mut target_name = first_name.clone();
        if self.check_symbol(':')
            && self.position + 1 < self.tokens.len()
            && matches!(self.tokens[self.position + 1].kind, TokenKind::Symbol(':'))
        {
            self.advance();
            self.advance();
            let mut segments = vec![first_name.clone()];
            loop {
                let (seg, _) = self.consume_identifier("Expected name after '::' in impl target")?;
                segments.push(seg);
                if self.check_symbol(':')
                    && self.position + 1 < self.tokens.len()
                    && matches!(self.tokens[self.position + 1].kind, TokenKind::Symbol(':'))
                {
                    self.advance();
                    self.advance();
                } else {
                    break;
                }
            }
            if segments.len() > 1 {
                module_path = Some(segments[..segments.len() - 1].join("::"));
                target_name = segments[segments.len() - 1].clone();
            }
        }

        // Optional type arguments: impl Type<T>
        let mut type_args = Vec::new();
        if self.check_symbol('<') {
            self.advance();
            while !self.check_symbol('>') && !self.is_at_end() {
                type_args.push(self.parse_type_annotation()?);
                if self.check_symbol(',') {
                    self.advance();
                } else {
                    break;
                }
            }
            self.consume_symbol('>', "Expected '>' after impl type arguments")?;
        }

        // Checar se é "impl Trait for Type" ou "impl Type"
        if self.check_keyword(Keyword::For) {
            // É um trait impl: impl TraitName for TypeName
            self.advance(); // consume 'for'
            let (type_name, _) = self.consume_identifier("Expected type name after 'for'")?;
            let mut for_type_args = Vec::new();
            if self.check_symbol('<') {
                self.advance();
                while !self.check_symbol('>') && !self.is_at_end() {
                    for_type_args.push(self.parse_type_annotation()?);
                    if self.check_symbol(',') {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.consume_symbol('>', "Expected '>' after impl type arguments")?;
            }
            let trait_impl = self.parse_trait_impl_block(
                start_span,
                first_name,
                type_name,
                type_args,
                impl_type_params,
            )?;
            return Ok(Item::TraitImpl(trait_impl));
        }

        // É um impl regular: impl TypeName
        let type_name = target_name;
        let _ = module_path; // module qualification is validated by the semantic phase

        self.consume_symbol('{', "Expected '{' to start impl block")?;

        let mut methods = Vec::new();

        while !self.check_symbol('}') && !self.is_at_end() {
            // Parse method: fn method_name(params) -> type { body }
            // Optional visibility modifier before 'fn'
            let method_visibility = if self.check_keyword(Keyword::Public) {
                self.advance();
                Visibility::Public
            } else if self.check_keyword(Keyword::Internal) {
                self.advance();
                Visibility::Internal
            } else {
                Visibility::Private
            };
            let is_async = if self.check_keyword(Keyword::Async) {
                self.advance();
                true
            } else {
                false
            };
            self.consume_function_keyword("Expected 'func' keyword for method")?;

            let (method_name, method_name_span) =
                self.consume_identifier("Expected method name")?;

            self.consume_symbol('(', "Expected '(' after method name")?;

            // Parse parameters (pode incluir self)
            let mut params = Vec::new();

            while !self.check_symbol(')') && !self.is_at_end() {
                let param_start = self.current().span;

                // Verificar se é self, &self, ou &mut self
                let (is_self, is_reference, is_mutable) = if self.check_keyword(Keyword::Mut) {
                    self.advance(); // consume 'mut'
                    if self.check_identifier() {
                        if let TokenKind::Identifier(name) = &self.current().kind {
                            if name == "self" {
                                self.advance();
                                (true, false, true) // mut self
                            } else {
                                (false, false, true) // parâmetro mut normal
                            }
                        } else {
                            (false, false, true)
                        }
                    } else {
                        (false, false, true)
                    }
                } else if self.check_symbol('&') {
                    self.advance(); // consume '&'
                    if self.check_keyword(Keyword::Mut) {
                        self.advance(); // consume 'mut'
                        self.consume_identifier("Expected 'self' after '&mut'")?;
                        (true, true, true) // &mut self
                    } else {
                        self.consume_identifier("Expected 'self' after '&'")?;
                        (true, true, false) // &self
                    }
                } else if self.check_identifier() {
                    if let TokenKind::Identifier(name) = &self.current().kind {
                        if name == "self" {
                            self.advance();
                            (true, false, false) // self
                        } else {
                            (false, false, false) // parâmetro normal
                        }
                    } else {
                        (false, false, false)
                    }
                } else {
                    (false, false, false) // parâmetro normal
                };

                let (param_name, type_annotation) = if is_self {
                    ("self".to_string(), None)
                } else {
                    let (name, _) = self.consume_identifier("Expected parameter name")?;
                    self.consume_symbol(':', "Expected ':' after parameter name")?;
                    let ty = self.parse_type_annotation()?;
                    (name, Some(ty))
                };

                let param_end = self.current().span;
                params.push(Parameter {
                    name: param_name,
                    type_annotation,
                    is_self,
                    is_reference,
                    is_mutable,
                    span: span_union(param_start, param_end),
                });

                // Optional comma
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

            methods.push(Method {
                name: method_name,
                is_async,
                params,
                return_type,
                body,
                span: span_union(method_name_span, body_end_span),
                visibility: method_visibility,
            });
        }

        let end_span = self.consume_symbol('}', "Expected '}' to end impl block")?;

        Ok(Item::Impl(ImplBlock {
            type_name,
            module_path,
            trait_name: None, // impl regular
            methods,
            span: span_union(start_span, end_span),
            type_args,
            type_params: impl_type_params,
        }))
    }

}
