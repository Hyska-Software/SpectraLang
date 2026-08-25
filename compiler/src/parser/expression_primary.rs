impl Parser {
    fn parse_async_expression(&mut self) -> Result<Expression, ()> {
        let start_span = self.current().span;
        self.advance();

        if self.check_symbol('{') {
            self.push_async_context();
            let parsed = self.parse_block();
            self.pop_async_context();
            let block = parsed?;
            let block_span = block.span;
            return Ok(Expression {
                span: crate::span::span_union(start_span, block_span),
                kind: ExpressionKind::AsyncBlock(block),
            });
        }

        if matches!(&self.current().kind, TokenKind::Operator(Operator::Or)) {
            self.advance();
            return self.finish_lambda_expression(start_span, true, Vec::new());
        }

        if self.check_symbol('|') {
            self.advance();
            let params = self.parse_lambda_params_after_open_pipe()?;
            return self.finish_lambda_expression(start_span, true, params);
        }

        if self.check_function_keyword() {
            self.push_error_coded(
                "P005",
                "`async func` is only valid as an item, method, or trait method declaration",
                start_span,
                Some("Move `async func` to declaration position, or use `async { ... }` for an async block expression.".to_string()),
                Some("misplaced `async func` in expression position".to_string()),
            );
        } else {
            self.push_error_coded(
                "P005",
                "Expected `{`, `|`, or `func` after `async`",
                self.current().span,
                Some("Use `async { ... }`, `async |args| expr`, or `async func name(...) { ... }` in declaration position.".to_string()),
                Some("`async` must prefix a block, closure, function, or method declaration".to_string()),
            );
        }
        Err(())
    }

    fn parse_lambda_params_after_open_pipe(&mut self) -> Result<Vec<LambdaParam>, ()> {
        let mut params = Vec::new();
        while !matches!(&self.current().kind, TokenKind::Symbol('|')) && !self.is_at_end() {
            let (param_name, param_span) =
                self.consume_identifier("Expected parameter name in lambda")?;
            let ty = if self.check_symbol(':') {
                self.advance(); // consume ':'
                Some(self.parse_type_annotation()?)
            } else {
                None
            };
            params.push(LambdaParam {
                name: param_name,
                ty,
                span: param_span,
            });
            if self.check_symbol(',') {
                self.advance(); // consume ','
            } else {
                break;
            }
        }
        self.consume_symbol('|', "Expected '|' to close lambda parameters")?;
        Ok(params)
    }

    fn finish_lambda_expression(
        &mut self,
        start_span: crate::span::Span,
        is_async: bool,
        params: Vec<LambdaParam>,
    ) -> Result<Expression, ()> {
        let body = if is_async {
            self.push_async_context();
            let parsed = self.parse_lambda_body_expression();
            self.pop_async_context();
            parsed?
        } else {
            self.parse_lambda_body_expression()?
        };
        let end_span = body.span;
        Ok(Expression {
            span: crate::span::span_union(start_span, end_span),
            kind: ExpressionKind::Lambda {
                is_async,
                params,
                body: Box::new(body),
            },
        })
    }

    fn parse_lambda_body_expression(&mut self) -> Result<Expression, ()> {
        if self.check_symbol('{') {
            let block = self.parse_block()?;
            let block_span = block.span;
            Ok(Expression {
                span: block_span,
                kind: ExpressionKind::Block(block),
            })
        } else {
            self.parse_expression()
        }
    }

    fn parse_if_expression(&mut self) -> Result<Expression, ()> {
        let start_span = self.consume_keyword(Keyword::If, "Expected 'if'")?;

        let condition = Box::new(self.parse_expression()?);
        let then_block = self.parse_block()?;

        let mut elif_blocks = Vec::new();

        // Parse optional else block
        let else_block = if self.check_keyword(Keyword::Else) {
            self.advance(); // consume 'else'
            if self.check_keyword(Keyword::If) {
                self.advance();
                let elif_condition = self.parse_expression()?;
                let elif_body = self.parse_block()?;
                elif_blocks.push((elif_condition, elif_body));

                loop {
                    if !self.check_keyword(Keyword::Else) {
                        break None;
                    }
                    self.advance();
                    if self.check_keyword(Keyword::If) {
                        self.advance();
                        let elif_condition = self.parse_expression()?;
                        let elif_body = self.parse_block()?;
                        elif_blocks.push((elif_condition, elif_body));
                    } else {
                        break Some(self.parse_block()?);
                    }
                }
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };

        let end_span = else_block
            .as_ref()
            .map(|b| b.span)
            .or_else(|| elif_blocks.last().map(|(_, b)| b.span))
            .unwrap_or(then_block.span);

        Ok(Expression {
            span: crate::span::span_union(start_span, end_span),
            kind: ExpressionKind::If {
                condition,
                then_block,
                elif_blocks,
                else_block,
            },
        })
    }

    /// Parses `unless <condition> { ... } else { ... }`.
    ///
    /// Mirrors `parse_if_expression` but emits [`ExpressionKind::Unless`]
    /// (`if` with a negated condition). `elif`/`else if` chaining is not
    /// part of the `unless` surface — rewrite as `if`/`elif` or nest blocks.
    fn parse_unless_expression(&mut self) -> Result<Expression, ()> {
        let start_span = self.consume_keyword(Keyword::Unless, "Expected 'unless'")?;

        let condition = Box::new(self.parse_expression()?);
        let then_block = self.parse_block()?;

        // Parse optional else block
        let else_block = if self.check_keyword(Keyword::Else) {
            self.advance(); // consume 'else'
            if self.check_keyword(Keyword::If) {
                let span = self.current().span;
                self.push_error_coded(
                    "P001",
                    "`unless` does not support `elif`/`else if` chains",
                    span,
                    Some(
                        "Rewrite the chain with `if`/`elif`, or nest another `unless` inside the block."
                            .to_string(),
                    ),
                    Some("`unless` is exactly an `if` with a negated condition".to_string()),
                );
                return Err(());
            }
            Some(self.parse_block()?)
        } else {
            None
        };

        let end_span = else_block
            .as_ref()
            .map(|b| b.span)
            .unwrap_or(then_block.span);

        Ok(Expression {
            span: crate::span::span_union(start_span, end_span),
            kind: ExpressionKind::Unless {
                condition,
                then_block,
                else_block,
            },
        })
    }

    /// Splits f-string raw template into literal and interpolated parts,
    /// then sub-parses inner expressions.
    fn parse_fstring_parts(&mut self, raw: &str, span: crate::span::Span) -> Vec<FStringPart> {
        use crate::lexer::Lexer;
        use std::collections::HashSet;

        let mut parts = Vec::new();
        let chars: Vec<char> = raw.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if chars[i] == '{' {
                // Find matching closing '}' tracking nested braces
                let mut depth = 1;
                let mut j = i + 1;
                while j < chars.len() && depth > 0 {
                    if chars[j] == '{' {
                        depth += 1;
                    } else if chars[j] == '}' {
                        depth -= 1;
                    }
                    if depth > 0 {
                        j += 1;
                    } else {
                        break;
                    }
                }
                // chars[i+1..j] is the expression source
                let expr_src: String = chars[i + 1..j].iter().collect();
                let sub_tokens_result = Lexer::new(&expr_src).tokenize();
                match sub_tokens_result {
                    Ok(sub_tokens) => {
                        let mut sub_parser = Parser::new(sub_tokens, HashSet::new());
                        match sub_parser.parse_expression() {
                            Ok(inner_expr) => {
                                parts.push(FStringPart::Interpolated(Box::new(inner_expr)));
                            }
                            Err(_) => {
                                self.error_at(
                                    "Invalid expression inside f-string interpolation",
                                    span,
                                );
                            }
                        }
                    }
                    Err(_) => {
                        self.error_at("Lexer error inside f-string interpolation", span);
                    }
                }
                i = j + 1; // skip past '}'
            } else {
                // Collect literal chars until next '{' or end
                let mut lit = String::new();
                while i < chars.len() && chars[i] != '{' {
                    lit.push(chars[i]);
                    i += 1;
                }
                if !lit.is_empty() {
                    parts.push(FStringPart::Literal(lit));
                }
            }
        }

        parts
    }

    fn parse_match_expression(&mut self) -> Result<Expression, ()> {
        use crate::ast::MatchArm;

        let start_span = self.consume_keyword(Keyword::Match, "Expected 'match'")?;
        let scrutinee = Box::new(self.parse_expression()?);

        // Expect '{'
        self.consume_symbol('{', "Expected '{' after match scrutinee")?;

        let mut arms = Vec::new();

        // Parse match arms using the canonical `when pattern then` and
        // `otherwise` surface.
        while !self.check_symbol('}') && !self.is_at_end() {
            let is_otherwise = self.check_keyword(Keyword::Otherwise);
            if is_otherwise || self.check_keyword(Keyword::When) {
                self.advance();
            } else {
                self.error("Expected 'when' or 'otherwise' in match arm");
                return Err(());
            }

            // Parse pattern.  `otherwise` is the existing wildcard pattern.
            let pattern = if is_otherwise {
                crate::ast::Pattern::Wildcard
            } else {
                self.parse_pattern()?
            };

            // Parse optional guard: if <expr>
            let guard = if matches!(self.current().kind, TokenKind::Keyword(Keyword::If)) {
                self.advance(); // consume 'if'
                Some(self.parse_expression()?)
            } else {
                None
            };

            if self.check_keyword(Keyword::Then) {
                self.advance();
            } else if !is_otherwise {
                self.error("Expected 'then' after pattern in match arm");
                return Err(());
            }

            // Parse body expression
            let body = self.parse_expression()?;

            arms.push(MatchArm {
                pattern,
                guard,
                body,
            });

            // Optional comma, or break if we see '}'.  A line break is the
            // normal separator and is detected by the next `when`/`otherwise`.
            if self.check_symbol(',') {
                self.advance();
            } else if self.check_symbol('}') {
                break; // End of match arms
            }
        }

        let end_span = self.consume_symbol('}', "Expected '}' to end match")?;

        Ok(Expression {
            span: crate::span::span_union(start_span, end_span),
            kind: ExpressionKind::Match { scrutinee, arms },
        })
    }

}
