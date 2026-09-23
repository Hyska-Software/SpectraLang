use super::*;

impl Parser {
    pub(crate) fn parse_async_expression(&mut self) -> Result<Expression, ()> {
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

    pub(crate) fn parse_lambda_params_after_open_pipe(&mut self) -> Result<Vec<LambdaParam>, ()> {
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

    pub(crate) fn finish_lambda_expression(
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

    pub(crate) fn parse_if_expression(&mut self) -> Result<Expression, ()> {
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
    pub(crate) fn parse_unless_expression(&mut self) -> Result<Expression, ()> {
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
    ///
    /// The interpolation text is lexed with the byte offset and line/column
    /// of its real position in the file (`span` covers the whole `f"..."`
    /// token, whose first two bytes are `f"`). Absolute spans keep every
    /// inner expression unique: relative spans restart at `0..N` for each
    /// interpolation, so span-keyed tables (semantic type facts consumed by
    /// lowering) would silently mix unrelated expressions together.
    ///
    /// `{{` / `}}` escape to literal braces, the interpolation scanner skips
    /// quoted string/char literals so a brace inside a nested string does not
    /// mis-slice the expression, and diagnostics produced by the sub-lexer or
    /// sub-parser are forwarded with their (already absolute) spans instead of
    /// being replaced by an uncoded error over the whole f-string.
    pub(crate) fn parse_fstring_parts(
        &mut self,
        raw: &str,
        span: crate::span::Span,
    ) -> Vec<FStringPart> {
        use crate::lexer::Lexer;

        let mut parts = Vec::new();
        let chars: Vec<char> = raw.chars().collect();
        let mut i = 0;

        // Position of `chars[walked]` in the enclosing file. The raw template
        // starts right after the `f"` header of the token.
        const FSTRING_HEADER_BYTES: usize = 2;
        let mut walked = 0usize;
        let mut walked_bytes = 0usize;
        let mut walk_line = span.start_location.line;
        let mut walk_column = span.start_location.column + FSTRING_HEADER_BYTES;
        macro_rules! advance_to {
            ($target:expr) => {
                while walked < ($target).min(chars.len()) {
                    let ch = chars[walked];
                    walked_bytes += ch.len_utf8();
                    if ch == '\n' {
                        walk_line += 1;
                        walk_column = 1;
                    } else {
                        walk_column += 1;
                    }
                    walked += 1;
                }
            };
        }

        while i < chars.len() {
            if chars[i] == '{' {
                // `{{` escapes to a literal brace.
                if i + 1 < chars.len() && chars[i + 1] == '{' {
                    let mut lit = String::new();
                    while i < chars.len() {
                        if chars[i] == '{' && i + 1 < chars.len() && chars[i + 1] == '{' {
                            lit.push('{');
                            i += 2;
                        } else if chars[i] == '{' {
                            break;
                        } else {
                            if chars[i] == '}' && i + 1 < chars.len() && chars[i + 1] == '}' {
                                lit.push('}');
                                i += 2;
                            } else {
                                lit.push(chars[i]);
                                i += 1;
                            }
                        }
                    }
                    parts.push(FStringPart::Literal(lit));
                    continue;
                }

                // Find the matching closing '}' tracking nested braces and
                // SKIPPING quoted string/char literals so a brace inside a
                // nested string literal (`f"{call(\"{\")}"`) does not change
                // the interpolation depth.
                let mut depth = 1;
                let mut j = i + 1;
                let mut in_quote: Option<char> = None;
                while j < chars.len() {
                    let ch = chars[j];
                    if let Some(quote) = in_quote {
                        if ch == '\\' && j + 1 < chars.len() {
                            j += 2;
                            continue;
                        }
                        if ch == quote {
                            in_quote = None;
                        }
                        j += 1;
                        continue;
                    }
                    match ch {
                        '"' | '\'' => {
                            in_quote = Some(ch);
                            j += 1;
                        }
                        '{' => {
                            depth += 1;
                            j += 1;
                        }
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            j += 1;
                        }
                        _ => j += 1,
                    }
                }

                if j >= chars.len() {
                    // Unterminated interpolation: report a coded error at the
                    // opening brace instead of silently dropping the rest.
                    let mut lit = String::new();
                    while i < chars.len() {
                        lit.push(chars[i]);
                        i += 1;
                    }
                    parts.push(FStringPart::Literal(lit));
                    self.push_error_coded(
                        "P002",
                        "Unterminated interpolation in f-string: missing `}`",
                        span,
                        Some("Close every `{` interpolation with a matching `}`.".to_string()),
                        None,
                    );
                    break;
                }

                // chars[i+1..j] is the expression source
                let expr_src: String = chars[i + 1..j].iter().collect();
                advance_to!(i + 1);
                let origin_offset = span.start + FSTRING_HEADER_BYTES + walked_bytes;
                let origin_location = crate::span::Location::new(walk_line, walk_column);
                // Absolute position just past the expression (used to rebase
                // sub-parser diagnostics recorded at the EOF sentinel, which
                // carries a dummy (0,0) span).
                let expr_end_offset = origin_offset + expr_src.len();
                let expr_end_line = origin_location.line + expr_src.matches('\n').count();
                let expr_end_column = match expr_src.rfind('\n') {
                    Some(position) => expr_src[position + 1..].chars().count() + 1,
                    None => origin_location.column + expr_src.chars().count(),
                };
                let expr_end_location = crate::span::Location::new(expr_end_line, expr_end_column);
                advance_to!(j + 1);
                let sub_tokens_result =
                    Lexer::with_origin(&expr_src, origin_offset, origin_location).tokenize();
                match sub_tokens_result {
                    Ok(sub_tokens) => {
                        // Continue the parent parser's depth accounting and
                        // stack budget so nested interpolation recursion is
                        // guarded by `P013` instead of starting a fresh,
                        // unguarded budget.
                        let mut sub_parser = Parser::new(sub_tokens);
                        sub_parser.depth = self.depth;
                        sub_parser.depth_limit_reported = self.depth_limit_reported;
                        sub_parser.stack_probe = self.stack_probe;
                        let sub_result = sub_parser.parse_expression();
                        // Forward the sub-parser's diagnostics: their spans are
                        // already absolute (rebased through `Lexer::with_origin`)
                        // except errors recorded at the parser's EOF sentinel,
                        // which are rebased onto the end of the interpolation.
                        let forwarded = !sub_parser.errors.is_empty();
                        for mut forwarded_error in sub_parser.errors.drain(..) {
                            if forwarded_error.span.start == 0 && forwarded_error.span.end == 0 {
                                forwarded_error.span = crate::span::Span::new(
                                    expr_end_offset,
                                    expr_end_offset,
                                    expr_end_location,
                                    expr_end_location,
                                );
                            }
                            self.errors.push(forwarded_error);
                        }
                        match sub_result {
                            Ok(inner_expr) => {
                                parts.push(FStringPart::Interpolated(Box::new(inner_expr)));
                            }
                            Err(_) if forwarded => {
                                // The sub-parser already recorded the specific
                                // diagnostics; do not add a generic overlay.
                            }
                            Err(_) => {
                                // Guard-only failure without any recorded
                                // diagnostic: keep a coded fallback so the
                                // f-string never swallows the error silently.
                                self.push_error_coded(
                                    "P019",
                                    "Invalid expression inside f-string interpolation",
                                    span,
                                    Some(
                                        "Interpolations must contain a single valid expression."
                                            .to_string(),
                                    ),
                                    None,
                                );
                            }
                        }
                    }
                    Err(lex_errors) => {
                        // Forward the sub-lexer diagnostics with their rebased
                        // spans instead of one uncoded error over the whole
                        // f-string.
                        let mut any_forwarded = false;
                        for lex_error in lex_errors {
                            any_forwarded = true;
                            let mut parse_error =
                                crate::error::ParseError::new(lex_error.message, lex_error.span);
                            if let Some(code) = lex_error.code {
                                parse_error = parse_error.with_code(code);
                            }
                            if let Some(context) = lex_error.context {
                                parse_error = parse_error.with_context(context);
                            }
                            if let Some(hint) = lex_error.hint {
                                parse_error = parse_error.with_hint(hint);
                            }
                            self.errors.push(parse_error);
                        }
                        if !any_forwarded {
                            self.push_error_coded(
                                "P019",
                                "Lexer error inside f-string interpolation",
                                span,
                                Some("Fix the interpolation text; it must lex as a valid expression.".to_string()),
                                None,
                            );
                        }
                    }
                }
                i = j + 1; // skip past '}'
            } else {
                // Collect literal chars until next '{' or end. `}}` escapes to
                // a single literal brace.
                let mut lit = String::new();
                while i < chars.len() && chars[i] != '{' {
                    if chars[i] == '}' && i + 1 < chars.len() && chars[i + 1] == '}' {
                        lit.push('}');
                        i += 2;
                    } else {
                        lit.push(chars[i]);
                        i += 1;
                    }
                }
                if !lit.is_empty() {
                    parts.push(FStringPart::Literal(lit));
                }
            }
        }

        parts
    }

    pub(crate) fn parse_match_expression(&mut self) -> Result<Expression, ()> {
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
            let otherwise_span = self.current().span;
            if is_otherwise || self.check_keyword(Keyword::When) {
                self.advance();
            } else {
                self.push_error_coded(
                    "P001",
                    "Expected 'when' or 'otherwise' in match arm",
                    self.current().span,
                    Some(
                        "Match arms use `when <pattern> then <body>` or `otherwise <body>`."
                            .to_string(),
                    ),
                    None,
                );
                return Err(());
            }

            // Parse pattern.  `otherwise` is the existing wildcard pattern.
            let pattern = if is_otherwise {
                crate::ast::Pattern::Wildcard(otherwise_span)
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
                self.push_error_coded(
                    "P001",
                    "Expected 'then' after pattern in match arm",
                    self.current().span,
                    Some("Write `when <pattern> then <body>`.".to_string()),
                    None,
                );
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
