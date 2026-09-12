use super::*;

// Newline rule for expressions (single deterministic rule):
//
// A source line break ends the current expression. An infix binary operator
// (`+ - * / % < > <= >= == != && || and or .. ..=`) whose token starts on a
// new line therefore never joins the expression. The only exception is an
// EXPLICIT continuation: the previous token must be another operator, an
// assignment `=`, a comma, or an open delimiter `(` / `[`. In other words,
// continuation operators go at the END of the line they continue
// (`let x = a +\n    b`), while an operator that STARTS a line
// (`let x = a\n    + b`) fails with `P015` instead of being silently
// absorbed into the previous statement.
//
// Postfix continuations (`.method()`, `expr?`, `as`) are postfix tokens, not
// infix operators, and remain legal on a new line. Unary prefixes (`-x`,
// `!x`, `not x`) sit in operand position, never pass through the infix
// gates, and are likewise unaffected.
//
// (Regular comments, not `//!`: inner doc comments are illegal mid-module.)
impl Parser {
    pub(crate) fn parse_expression(&mut self) -> Result<Expression, ()> {
        self.enter_parse_depth()?;
        let result = self.parse_expression_inner();
        self.exit_parse_depth();
        result
    }

    fn parse_expression_inner(&mut self) -> Result<Expression, ()> {
        let mut expr = self.parse_logical_or()?;

        // Range operators bind loosest, Rust style. Chained ranges
        // (`a..b..c`) associate deterministically to the left: `(a..b)..c`.
        let mut built_range = false;
        while matches!(
            &self.current().kind,
            TokenKind::Operator(Operator::Range) | TokenKind::Operator(Operator::RangeInclusive)
        ) {
            let inclusive = matches!(
                &self.current().kind,
                TokenKind::Operator(Operator::RangeInclusive)
            );
            self.reject_line_broken_infix()?;
            self.advance();
            // Bind the range end at the comparison rung so logical-level
            // operators (`or`/`and`/equality/comparison) stay outside the
            // range and can wrap it afterwards.
            let end = self.parse_comparison()?;
            let span = crate::span::span_union(expr.span, end.span);
            expr = Expression {
                span,
                kind: ExpressionKind::Range {
                    start: Box::new(expr),
                    end: Box::new(end),
                    inclusive,
                },
            };
            built_range = true;
        }

        // After building a range, keep consuming operators at the logical
        // level so `1..2 == x` parses as `(1..2) == x`. The type checker may
        // still reject such a comparison later — that is a semantic
        // diagnostic, not a syntax error.
        if built_range {
            expr = self.parse_logical_level_rest(expr)?;
        }

        Ok(expr)
    }

    /// Continues consuming logical-level binary operators (`or`, `and`,
    /// equality, comparison) with an already-parsed left-hand side. Each arm
    /// parses its right operand one rung tighter, preserving the same
    /// precedence ladder as the main chain builders above.
    fn parse_logical_level_rest(&mut self, mut left: Expression) -> Result<Expression, ()> {
        // Tracks an unparenthesized run of relational operators so a second
        // one in the same chain fails with `P014` (see `parse_comparison`).
        let mut previous_relational = false;
        loop {
            if matches!(
                &self.current().kind,
                TokenKind::Operator(Operator::Or) | TokenKind::Keyword(Keyword::OrWord)
            ) {
                self.reject_line_broken_infix()?;
                self.advance();
                let right = self.parse_logical_and()?;
                let span = crate::span::span_union(left.span, right.span);
                left = Expression {
                    span,
                    kind: ExpressionKind::Binary {
                        left: Box::new(left),
                        operator: BinaryOperator::Or,
                        right: Box::new(right),
                    },
                };
                continue;
            }

            if matches!(
                &self.current().kind,
                TokenKind::Operator(Operator::And) | TokenKind::Keyword(Keyword::AndWord)
            ) {
                self.reject_line_broken_infix()?;
                self.advance();
                let right = self.parse_equality()?;
                let span = crate::span::span_union(left.span, right.span);
                left = Expression {
                    span,
                    kind: ExpressionKind::Binary {
                        left: Box::new(left),
                        operator: BinaryOperator::And,
                        right: Box::new(right),
                    },
                };
                continue;
            }

            let operator = match &self.current().kind {
                TokenKind::Operator(Operator::EqualEqual) => Some(BinaryOperator::Equal),
                TokenKind::Operator(Operator::NotEqual) => Some(BinaryOperator::NotEqual),
                TokenKind::Symbol('<') => Some(BinaryOperator::Less),
                TokenKind::Symbol('>') => Some(BinaryOperator::Greater),
                TokenKind::Operator(Operator::LessEqual) => Some(BinaryOperator::LessEqual),
                TokenKind::Operator(Operator::GreaterEqual) => Some(BinaryOperator::GreaterEqual),
                _ => None,
            };

            if let Some(operator) = operator {
                let is_relational =
                    !matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual);
                if is_relational && previous_relational {
                    let span = self.current().span;
                    self.push_error_coded(
                        "P014",
                        "chained comparison; combine conditions with `and`",
                        span,
                        Some(
                            "`a < b < c` parses as `(a < b) < c`, comparing a boolean with a value. Write `a < b and b < c` instead."
                                .to_string(),
                        ),
                        Some(
                            "second relational operator in an unparenthesized comparison chain"
                                .to_string(),
                        ),
                    );
                    return Err(());
                }
                self.reject_line_broken_infix()?;
                previous_relational = is_relational;
                self.advance();
                let right = self.parse_addition()?;
                let span = crate::span::span_union(left.span, right.span);
                left = Expression {
                    span,
                    kind: ExpressionKind::Binary {
                        left: Box::new(left),
                        operator,
                        right: Box::new(right),
                    },
                };
                continue;
            }

            break;
        }

        Ok(left)
    }

    // Logical OR (lowest precedence)
    fn parse_logical_or(&mut self) -> Result<Expression, ()> {
        let mut left = self.parse_logical_and()?;

        while matches!(
            &self.current().kind,
            TokenKind::Operator(Operator::Or) | TokenKind::Keyword(Keyword::OrWord)
        ) {
            self.reject_line_broken_infix()?;
            self.advance();
            let right = self.parse_logical_and()?;
            let span = crate::span::span_union(left.span, right.span);
            left = Expression {
                span,
                kind: ExpressionKind::Binary {
                    left: Box::new(left),
                    operator: BinaryOperator::Or,
                    right: Box::new(right),
                },
            };
        }

        Ok(left)
    }

    // Logical AND
    fn parse_logical_and(&mut self) -> Result<Expression, ()> {
        let mut left = self.parse_logical_not()?;

        while matches!(
            &self.current().kind,
            TokenKind::Operator(Operator::And) | TokenKind::Keyword(Keyword::AndWord)
        ) {
            self.reject_line_broken_infix()?;
            self.advance();
            let right = self.parse_logical_not()?;
            let span = crate::span::span_union(left.span, right.span);
            left = Expression {
                span,
                kind: ExpressionKind::Binary {
                    left: Box::new(left),
                    operator: BinaryOperator::And,
                    right: Box::new(right),
                },
            };
        }

        Ok(left)
    }

    // Word-form `not` follows the readable-language precedence used by
    // Python-like surfaces: comparisons bind more tightly than `not`, while
    // `not` binds more tightly than `and`/`or`.  The symbolic `!` remains a
    // traditional high-precedence unary operator for low-level expressions.
    fn parse_logical_not(&mut self) -> Result<Expression, ()> {
        if self.check_keyword(Keyword::NotWord) {
            let start_span = self.current().span;
            self.advance();
            let operand = self.parse_logical_not()?;
            let span = crate::span::span_union(start_span, operand.span);
            Ok(Expression {
                span,
                kind: ExpressionKind::Unary {
                    operator: UnaryOperator::Not,
                    operand: Box::new(operand),
                },
            })
        } else {
            self.parse_equality()
        }
    }

    // Equality (==, !=)
    fn parse_equality(&mut self) -> Result<Expression, ()> {
        let mut left = self.parse_comparison()?;

        loop {
            let operator = match &self.current().kind {
                TokenKind::Operator(Operator::EqualEqual) => BinaryOperator::Equal,
                TokenKind::Operator(Operator::NotEqual) => BinaryOperator::NotEqual,
                _ => break,
            };

            self.reject_line_broken_infix()?;
            self.advance();
            let right = self.parse_comparison()?;
            let span = crate::span::span_union(left.span, right.span);
            left = Expression {
                span,
                kind: ExpressionKind::Binary {
                    left: Box::new(left),
                    operator,
                    right: Box::new(right),
                },
            };
        }

        Ok(left)
    }

    // Comparison (<, >, <=, >=)
    fn parse_comparison(&mut self) -> Result<Expression, ()> {
        let mut left = self.parse_addition()?;

        // A single unparenthesized comparison chain may contain at most one
        // relational operator: `a < b < c` would otherwise silently evaluate
        // as `(a < b) < c`, comparing a boolean against a value.
        let mut chain_active = false;

        loop {
            let operator = match &self.current().kind {
                TokenKind::Symbol('<') => BinaryOperator::Less,
                TokenKind::Symbol('>') => BinaryOperator::Greater,
                TokenKind::Operator(Operator::LessEqual) => BinaryOperator::LessEqual,
                TokenKind::Operator(Operator::GreaterEqual) => BinaryOperator::GreaterEqual,
                _ => break,
            };

            if chain_active {
                let span = self.current().span;
                self.push_error_coded(
                    "P014",
                    "chained comparison; combine conditions with `and`",
                    span,
                    Some(
                        "`a < b < c` parses as `(a < b) < c`, comparing a boolean with a value. Write `a < b and b < c` instead."
                            .to_string(),
                    ),
                    Some(
                        "second relational operator in an unparenthesized comparison chain"
                            .to_string(),
                    ),
                );
                return Err(());
            }

            self.reject_line_broken_infix()?;
            chain_active = true;
            self.advance();
            let right = self.parse_addition()?;
            let span = crate::span::span_union(left.span, right.span);
            left = Expression {
                span,
                kind: ExpressionKind::Binary {
                    left: Box::new(left),
                    operator,
                    right: Box::new(right),
                },
            };
        }

        Ok(left)
    }

    // Addition and Subtraction
    fn parse_addition(&mut self) -> Result<Expression, ()> {
        let mut left = self.parse_multiplication()?;

        loop {
            let operator = match &self.current().kind {
                TokenKind::Symbol('+') => BinaryOperator::Add,
                TokenKind::Symbol('-') => BinaryOperator::Subtract,
                _ => break,
            };

            self.reject_line_broken_infix()?;
            self.advance();
            let right = self.parse_multiplication()?;
            let span = crate::span::span_union(left.span, right.span);
            left = Expression {
                span,
                kind: ExpressionKind::Binary {
                    left: Box::new(left),
                    operator,
                    right: Box::new(right),
                },
            };
        }

        Ok(left)
    }

    // Multiplication, Division, Modulo
    fn parse_multiplication(&mut self) -> Result<Expression, ()> {
        let mut left = self.parse_unary()?;

        loop {
            let operator = match &self.current().kind {
                TokenKind::Symbol('*') => BinaryOperator::Multiply,
                TokenKind::Symbol('/') => BinaryOperator::Divide,
                TokenKind::Symbol('%') => BinaryOperator::Modulo,
                _ => break,
            };

            self.advance();
            let right = self.parse_unary()?;
            let span = crate::span::span_union(left.span, right.span);
            left = Expression {
                span,
                kind: ExpressionKind::Binary {
                    left: Box::new(left),
                    operator,
                    right: Box::new(right),
                },
            };
        }

        Ok(left)
    }

    // Unary expressions (-, !)
    fn parse_unary(&mut self) -> Result<Expression, ()> {
        if matches!(&self.current().kind, TokenKind::Keyword(Keyword::Await)) {
            let start_span = self.current().span;
            self.advance();
            if !self.in_async_context() {
                self.push_error_coded(
                    "P006",
                    "`await` is only valid inside an async context",
                    start_span,
                    Some("Move this expression into an `async func`, `async { ... }`, or async closure.".to_string()),
                    Some("`await` outside async context".to_string()),
                );
                return Err(());
            }
            let operand = self.parse_unary()?;
            let span = crate::span::span_union(start_span, operand.span);
            return Ok(Expression {
                span,
                kind: ExpressionKind::Await(Box::new(operand)),
            });
        }

        let operator = match &self.current().kind {
            TokenKind::Symbol('-') => Some(UnaryOperator::Negate),
            TokenKind::Symbol('!') => Some(UnaryOperator::Not),
            _ => None,
        };

        if let Some(op) = operator {
            let start_span = self.current().span;
            self.advance();
            let operand = self.parse_unary()?;
            let span = crate::span::span_union(start_span, operand.span);
            Ok(Expression {
                span,
                kind: ExpressionKind::Unary {
                    operator: op,
                    operand: Box::new(operand),
                },
            })
        } else {
            self.parse_call_expression()
        }
    }

    fn parse_call_expression(&mut self) -> Result<Expression, ()> {
        let mut expr = self.parse_primary_expression()?;

        // Handle function calls and array indexing
        loop {
            if self.check_symbol('(') {
                self.advance(); // consume '('

                let mut arguments = Vec::new();

                if !self.check_symbol(')') {
                    loop {
                        match self.parse_expression() {
                            Ok(argument) => arguments.push(argument),
                            Err(_) => {
                                self.recover_in_delimited_list(&[')'], &[',']);
                                if self.check_symbol(',') {
                                    self.advance();
                                    if self.check_symbol(')') {
                                        break;
                                    }
                                    continue;
                                }
                                break;
                            }
                        }

                        if self.check_symbol(',') {
                            self.advance();
                            if self.check_symbol(')') {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }

                let end_span = self.consume_symbol(')', "Expected ')' after arguments")?;

                let span = crate::span::span_union(expr.span, end_span);
                expr = Expression {
                    span,
                    kind: ExpressionKind::Call {
                        callee: Box::new(expr),
                        arguments,
                    },
                };
            } else if self.check_symbol('[') {
                self.advance(); // consume '['
                let index = self.parse_expression()?;
                let end_span = self.consume_symbol(']', "Expected ']' after index")?;

                let span = crate::span::span_union(expr.span, end_span);
                expr = Expression {
                    span,
                    kind: ExpressionKind::IndexAccess {
                        array: Box::new(expr),
                        index: Box::new(index),
                    },
                };
            } else if self.check_symbol('.') {
                // Check for range before consuming '.'
                // (Two dots: '..' would have been lexed as Operator::Range, not Symbol('.'))
                self.advance(); // consume '.'

                // Check if it's a number (tuple access) or identifier (field access)
                if let TokenKind::Number(num_str) = &self.current().kind {
                    // Tuple access: .0, .1, .2, etc.
                    if let Ok(index) = num_str.parse::<usize>() {
                        let end_span = self.current().span;
                        self.advance();

                        let span = crate::span::span_union(expr.span, end_span);
                        expr = Expression {
                            span,
                            kind: ExpressionKind::TupleAccess {
                                tuple: Box::new(expr),
                                index,
                            },
                        };
                    } else {
                        self.error("Invalid tuple index");
                        return Err(());
                    }
                } else if let TokenKind::Identifier(_) = &self.current().kind {
                    // Field access ou method call: .field_name ou .method_name(args)
                    let (name, name_span) =
                        self.consume_identifier("Expected field/method name after '.'")?;

                    // Verificar se é method call (seguido de '(')
                    if self.check_symbol('(') {
                        self.advance(); // consume '('

                        let mut arguments = Vec::new();
                        if !self.check_symbol(')') {
                            loop {
                                match self.parse_expression() {
                                    Ok(argument) => arguments.push(argument),
                                    Err(_) => {
                                        self.recover_in_delimited_list(&[')'], &[',']);
                                        if self.check_symbol(',') {
                                            self.advance();
                                            if self.check_symbol(')') {
                                                break;
                                            }
                                            continue;
                                        }
                                        break;
                                    }
                                }

                                if self.check_symbol(',') {
                                    self.advance();
                                    if self.check_symbol(')') {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                        }

                        let end_span =
                            self.consume_symbol(')', "Expected ')' after method arguments")?;

                        let span = crate::span::span_union(expr.span, end_span);
                        expr = Expression {
                            span,
                            kind: ExpressionKind::MethodCall {
                                object: Box::new(expr),
                                method_name: name,
                                arguments,
                                type_name: None, // Será preenchido pelo semantic analyzer
                            },
                        };
                    } else {
                        // É field access
                        let span = crate::span::span_union(expr.span, name_span);
                        expr = Expression {
                            span,
                            kind: ExpressionKind::FieldAccess {
                                object: Box::new(expr),
                                field: name,
                            },
                        };
                    }
                } else {
                    self.error("Expected number or field name after '.'");
                    return Err(());
                }
            } else if matches!(&self.current().kind, TokenKind::Symbol('?')) {
                // Try/propagate operator: expr?
                let end_span = self.current().span;
                self.advance(); // consume '?'
                let span = crate::span::span_union(expr.span, end_span);
                expr = Expression {
                    span,
                    kind: ExpressionKind::Try(Box::new(expr)),
                };
            } else {
                break;
            }
        }

        // Type cast: expr as TargetType  (left-associative, lowest postfix precedence)
        while matches!(
            &self.current().kind,
            crate::token::TokenKind::Keyword(crate::token::Keyword::As)
        ) {
            let _as_span = self.current().span;
            self.advance(); // consume 'as'
            let mode = if matches!(&self.current().kind, crate::token::TokenKind::Identifier(name) if name == "wrapping")
            {
                self.advance();
                crate::ast::CastMode::Wrapping
            } else {
                crate::ast::CastMode::Checked
            };
            let target_type = self.parse_type_annotation()?;
            let span = crate::span::span_union(expr.span, target_type.span);
            expr = crate::ast::Expression {
                span,
                kind: crate::ast::ExpressionKind::Cast {
                    expr: Box::new(expr),
                    target_type,
                    mode,
                },
            };
        }

        Ok(expr)
    }

    pub(crate) fn parse_primary_expression(&mut self) -> Result<Expression, ()> {
        let token = self.current();
        let span = token.span;

        match &token.kind {
            TokenKind::Keyword(Keyword::True) => {
                self.advance();
                Ok(Expression {
                    span,
                    kind: ExpressionKind::BoolLiteral(true),
                })
            }
            TokenKind::Keyword(Keyword::False) => {
                self.advance();
                Ok(Expression {
                    span,
                    kind: ExpressionKind::BoolLiteral(false),
                })
            }
            TokenKind::Keyword(Keyword::If) => self.parse_if_expression(),
            TokenKind::Keyword(Keyword::Unless) => self.parse_unless_expression(),
            TokenKind::Keyword(Keyword::Match) => self.parse_match_expression(),
            TokenKind::Keyword(Keyword::Async) => self.parse_async_expression(),
            TokenKind::Keyword(Keyword::Await) => self.parse_unary(),
            TokenKind::Symbol('{') => {
                let block = self.parse_block()?;
                let block_span = block.span;
                Ok(Expression {
                    span: block_span,
                    kind: ExpressionKind::Block(block),
                })
            }
            TokenKind::Identifier(name) => {
                let name = name.clone();
                let start_span = span;
                self.advance();

                if name == "diff" && self.check_symbol('{') {
                    let block = self.parse_block()?;
                    let block_span = block.span;
                    return Ok(Expression {
                        span: crate::span::span_union(start_span, block_span),
                        kind: ExpressionKind::DifferentiableBlock(block),
                    });
                }

                // Parse optional type arguments: Name<Type1, Type2>
                let type_args = if self.is_likely_type_args_lookahead() {
                    self.parse_type_arguments()?
                } else {
                    Vec::new()
                };

                // Check if it's an enum variant or qualified path:
                //   Name::Variant
                //   module::Item
                //   module::Enum::Variant
                if self.check_symbol(':')
                    && self.position + 1 < self.tokens.len()
                    && matches!(self.tokens[self.position + 1].kind, TokenKind::Symbol(':'))
                {
                    self.advance(); // consume first ':'
                    self.advance(); // consume second ':'

                    let mut segments = vec![name.clone()];
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

                    // segments.len() >= 2
                    let (module_path, enum_name, variant_name) = match segments.len() {
                        2 => (None, segments[0].clone(), segments[1].clone()),
                        _ => {
                            let mp = segments[..segments.len() - 2].join("::");
                            let en = segments[segments.len() - 2].clone();
                            let vn = segments[segments.len() - 1].clone();
                            (Some(mp), en, vn)
                        }
                    };

                    // Check for tuple variant data / function args
                    let data = if self.check_symbol('(') {
                        self.advance(); // consume '('

                        let mut args = Vec::new();
                        if !self.check_symbol(')') {
                            loop {
                                args.push(self.parse_expression()?);
                                if !self.check_symbol(',') {
                                    break;
                                }
                                self.advance(); // consume ','
                            }
                        }

                        let _end_span =
                            self.consume_symbol(')', "Expected ')' after variant data")?;
                        Some(args)
                    } else {
                        None
                    };

                    let struct_data = if self.check_symbol('{') {
                        self.advance(); // consume '{'

                        let mut fields = Vec::new();
                        while !self.check_symbol('}') && !self.is_at_end() {
                            let (field_name, _) =
                                self.consume_identifier("Expected field name in enum variant")?;
                            self.consume_symbol(':', "Expected ':' after field name")?;
                            let field_value = self.parse_expression()?;
                            fields.push((field_name, field_value));

                            if self.check_symbol(',') {
                                self.advance();
                            }
                        }

                        self.consume_symbol('}', "Expected '}' after enum variant fields")?;
                        Some(fields)
                    } else {
                        None
                    };

                    return Ok(Expression {
                        span: crate::span::span_union(start_span, self.current().span),
                        kind: ExpressionKind::EnumVariant {
                            module_path,
                            enum_name,
                            type_args,
                            variant_name,
                            data,
                            struct_data,
                        },
                    });
                }

                // Check if it's a struct literal: Name { fields }
                // Supports explicit fields (`x: expr`) and shorthand (`x`).
                if self.check_symbol('{') {
                    let is_struct_literal = if self.position + 1 < self.tokens.len() {
                        match &self.tokens[self.position + 1].kind {
                            TokenKind::Symbol('}') => true,
                            TokenKind::Identifier(_) => {
                                let after_field =
                                    self.tokens.get(self.position + 2).map(|token| &token.kind);
                                let after_colon =
                                    self.tokens.get(self.position + 3).map(|token| &token.kind);

                                match after_field {
                                    Some(TokenKind::Symbol(':')) => {
                                        !matches!(after_colon, Some(TokenKind::Symbol(':')))
                                    }
                                    Some(TokenKind::Symbol(',')) | Some(TokenKind::Symbol('}')) => {
                                        true
                                    }
                                    _ => false,
                                }
                            }
                            _ => false,
                        }
                    } else {
                        false
                    };

                    if is_struct_literal {
                        self.advance(); // consume '{'

                        let mut fields = Vec::new();

                        while !self.check_symbol('}') && !self.is_at_end() {
                            // Parse either `field_name: value` or shorthand `field_name`.
                            let (field_name, field_span) =
                                self.consume_identifier("Expected field name")?;
                            let field_value = if self.check_symbol(':') {
                                self.advance();
                                self.parse_expression()?
                            } else {
                                Expression {
                                    span: field_span,
                                    kind: ExpressionKind::Identifier(field_name.clone()),
                                }
                            };

                            fields.push((field_name, field_value));

                            // Optional comma
                            if self.check_symbol(',') {
                                self.advance();
                            }
                        }

                        let end_span = self.current().span;
                        self.consume_symbol('}', "Expected '}' to end struct literal")?;

                        Ok(Expression {
                            span: crate::span::span_union(start_span, end_span),
                            kind: ExpressionKind::StructLiteral {
                                name,
                                type_args,
                                fields,
                            },
                        })
                    } else {
                        // Just an identifier, '{' belongs to surrounding context
                        Ok(Expression {
                            span,
                            kind: ExpressionKind::Identifier(name),
                        })
                    }
                } else {
                    // Just an identifier
                    Ok(Expression {
                        span,
                        kind: ExpressionKind::Identifier(name),
                    })
                }
            }
            TokenKind::Number(value) => {
                let value = value.clone();
                self.advance();
                Ok(Expression {
                    span,
                    kind: ExpressionKind::NumberLiteral(value),
                })
            }
            TokenKind::StringLiteral(value) => {
                let value = value.clone();
                self.advance();
                Ok(Expression {
                    span,
                    kind: ExpressionKind::StringLiteral(value),
                })
            }
            TokenKind::CharLiteral(c) => {
                let c = *c;
                self.advance();
                Ok(Expression {
                    span,
                    kind: ExpressionKind::CharLiteral(c),
                })
            }
            TokenKind::FStringLiteral(raw) => {
                let raw = raw.clone();
                self.advance();
                let parts = self.parse_fstring_parts(&raw, span);
                Ok(Expression {
                    span,
                    kind: ExpressionKind::FString(parts),
                })
            }
            // Lambda: |param, param| expr  or  || expr
            TokenKind::Operator(Operator::Or) => {
                // '||' detected as Operator::Or in primary position = empty lambda params
                self.advance(); // consume '||'
                self.finish_lambda_expression(span, false, Vec::new())
            }
            TokenKind::Symbol('|') => {
                // Lambda with params: |x, y: int| expr
                self.advance(); // consume '|'
                let params = self.parse_lambda_params_after_open_pipe()?;
                self.finish_lambda_expression(span, false, params)
            }
            TokenKind::Symbol('(') => {
                self.advance(); // consume '('

                // Check for empty tuple ()
                if self.check_symbol(')') {
                    let end_span = self.current().span;
                    self.advance();
                    let span = crate::span::span_union(span, end_span);
                    return Ok(Expression {
                        span,
                        kind: ExpressionKind::TupleLiteral { elements: vec![] },
                    });
                }

                let first_expr = self.parse_expression()?;

                // If followed by comma, it's a tuple
                if self.check_symbol(',') {
                    let mut elements = vec![first_expr];

                    while self.check_symbol(',') {
                        self.advance(); // consume ','

                        // Allow trailing comma before ')'
                        if self.check_symbol(')') {
                            break;
                        }

                        elements.push(self.parse_expression()?);
                    }

                    let end_span = self.consume_symbol(')', "Expected ')' after tuple elements")?;
                    let span = crate::span::span_union(span, end_span);

                    Ok(Expression {
                        span,
                        kind: ExpressionKind::TupleLiteral { elements },
                    })
                } else {
                    // Just grouping
                    self.consume_symbol(')', "Expected ')' after expression")?;
                    Ok(Expression {
                        span,
                        kind: ExpressionKind::Grouping(Box::new(first_expr)),
                    })
                }
            }
            TokenKind::Symbol('[') => {
                self.advance(); // consume '['
                let mut elements = Vec::new();

                if !self.check_symbol(']') {
                    loop {
                        elements.push(self.parse_expression()?);
                        if !self.check_symbol(',') {
                            break;
                        }
                        self.advance(); // consume ','

                        // Allow trailing comma before ']'
                        if self.check_symbol(']') {
                            break;
                        }
                    }
                }

                let end_span = self.consume_symbol(']', "Expected ']' after array elements")?;
                let span = crate::span::span_union(span, end_span);

                Ok(Expression {
                    span,
                    kind: ExpressionKind::ArrayLiteral { elements },
                })
            }
            _ => {
                self.error("Expected expression");
                Err(())
            }
        }
    }
}
