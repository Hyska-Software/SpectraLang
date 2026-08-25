impl Parser {
    pub(super) fn parse_pattern(&mut self) -> Result<crate::ast::Pattern, ()> {
        self.enter_parse_depth()?;
        let result = self.parse_pattern_inner();
        self.exit_parse_depth();
        result
    }

    fn parse_pattern_inner(&mut self) -> Result<crate::ast::Pattern, ()> {
        let mut patterns = vec![self.parse_pattern_atom()?];

        while self.check_symbol('|') {
            self.advance();
            patterns.push(self.parse_pattern_atom()?);
        }

        if patterns.len() == 1 {
            Ok(patterns.remove(0))
        } else {
            Ok(crate::ast::Pattern::Or(patterns))
        }
    }

    fn parse_pattern_atom(&mut self) -> Result<crate::ast::Pattern, ()> {
        use crate::ast::Pattern;

        // Wildcard pattern: _
        if self.check_symbol('_') {
            self.advance();
            return Ok(Pattern::Wildcard);
        }

        // Tuple pattern: (a, b, _)
        if self.check_symbol('(') {
            self.advance();
            let mut elements = Vec::new();
            if !self.check_symbol(')') {
                loop {
                    elements.push(self.parse_pattern()?);
                    if !self.check_symbol(',') {
                        break;
                    }
                    self.advance();
                }
            }
            self.consume_symbol(')', "Expected ')' after tuple pattern")?;
            return Ok(Pattern::Tuple(elements));
        }

        // Check for enum variant pattern: EnumName::VariantName or EnumName<Type>::VariantName
        if let TokenKind::Identifier(name) = &self.current().kind {
            let first_name = name.clone();
            self.advance();

            // Parse optional type arguments: EnumName<Type>
            let type_args = if self.is_likely_type_args_lookahead() {
                self.parse_type_arguments()?
            } else {
                Vec::new()
            };

            // Check for :: (enum variant or qualified path)
            if self.check_symbol(':')
                && self.position + 1 < self.tokens.len()
                && matches!(self.tokens[self.position + 1].kind, TokenKind::Symbol(':'))
            {
                self.advance(); // consume first ':'
                self.advance(); // consume second ':'

                let mut segments = vec![first_name.clone()];
                loop {
                    let (seg, _) = self.consume_identifier("Expected name after '::")?;
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

                let (module_path, enum_name, variant_name) = match segments.len() {
                    2 => (None, segments[0].clone(), segments[1].clone()),
                    _ => {
                        let mp = segments[..segments.len() - 2].join("::");
                        let en = segments[segments.len() - 2].clone();
                        let vn = segments[segments.len() - 1].clone();
                        (Some(mp), en, vn)
                    }
                };

                // Check for data patterns: (pattern, pattern, ...)
                let data = if self.check_symbol('(') {
                    self.advance(); // consume '('

                    let mut patterns = Vec::new();
                    if !self.check_symbol(')') {
                        loop {
                            patterns.push(self.parse_pattern()?);
                            if !self.check_symbol(',') {
                                break;
                            }
                            self.advance(); // consume ','
                        }
                    }

                    self.consume_symbol(')', "Expected ')' after variant data patterns")?;
                    Some(patterns)
                } else {
                    None
                };

                let struct_data = if self.check_symbol('{') {
                    self.advance(); // consume '{'

                    let mut fields = Vec::new();
                    while !self.check_symbol('}') && !self.is_at_end() {
                        let (field_name, _) = self
                            .consume_identifier("Expected field name in struct variant pattern")?;
                        let field_pattern = if self.check_symbol(':') {
                            self.advance();
                            self.parse_pattern()?
                        } else {
                            Pattern::Identifier(field_name.clone())
                        };
                        fields.push((field_name, field_pattern));

                        if self.check_symbol(',') {
                            self.advance();
                        }
                    }

                    self.consume_symbol('}', "Expected '}' after struct variant pattern")?;
                    Some(fields)
                } else {
                    None
                };

                return Ok(Pattern::EnumVariant {
                    module_path,
                    enum_name,
                    type_args,
                    variant_name,
                    data,
                    struct_data,
                });
            }

            if self.check_symbol('{') {
                self.advance();
                let mut fields = Vec::new();
                while !self.check_symbol('}') && !self.is_at_end() {
                    let (field_name, _) =
                        self.consume_identifier("Expected field name in struct pattern")?;
                    let field_pattern = if self.check_symbol(':') {
                        self.advance();
                        self.parse_pattern()?
                    } else {
                        Pattern::Identifier(field_name.clone())
                    };
                    fields.push((field_name, field_pattern));

                    if self.check_symbol(',') {
                        self.advance();
                    }
                }
                self.consume_symbol('}', "Expected '}' after struct pattern")?;
                return Ok(Pattern::Struct {
                    name: first_name,
                    fields,
                });
            }

            // Just an identifier pattern (binding)
            return Ok(Pattern::Identifier(first_name));
        }

        // Literal patterns (números, booleanos, etc.)
        let expr = self.parse_primary_expression()?;
        Ok(Pattern::Literal(expr))
    }

    fn is_likely_type_args_lookahead(&self) -> bool {
        if !self.check_symbol('<') {
            return false;
        }
        let mut i = self.position + 1;
        let mut depth = 1;
        while i < self.tokens.len() && depth > 0 {
            let kind = &self.tokens[i].kind;
            match kind {
                TokenKind::Symbol('<') => depth += 1,
                TokenKind::Symbol('>') => depth -= 1,
                TokenKind::Symbol(';')
                | TokenKind::Symbol('{')
                | TokenKind::Keyword(_)
                | TokenKind::Symbol('=') => return false,
                _ => {}
            }
            i += 1;
        }
        if depth > 0 {
            return false;
        }
        // The token after `>` must be either `:` (for `::`) or `{` (for struct literal)
        if i < self.tokens.len() {
            let next_kind = &self.tokens[i].kind;
            return matches!(next_kind, TokenKind::Symbol(':') | TokenKind::Symbol('{'));
        }
        false
    }

    /// Parse type arguments: <Type1, Type2>
    /// Used in generic type instantiation: Point<int>, Option<string>
    fn parse_type_arguments(&mut self) -> Result<Vec<crate::ast::TypeAnnotation>, ()> {
        self.consume_symbol('<', "Expected '<' for type arguments")?;

        let mut type_args = Vec::new();

        // Parse first type argument
        if !self.check_symbol('>') {
            loop {
                let type_arg = self.parse_type_annotation()?;
                type_args.push(type_arg);

                if !self.check_symbol(',') {
                    break;
                }
                self.advance(); // consume ','
            }
        }

        self.consume_symbol('>', "Expected '>' after type arguments")?;

        Ok(type_args)
    }
}
