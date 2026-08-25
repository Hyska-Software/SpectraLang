use crate::{
    ast::{
        DoWhileLoop, ForLoop, IfLetStatement, LetStatement, LoopStatement, ReturnStatement,
        Statement, StatementKind, SwitchCase, SwitchStatement, WhileLetStatement, WhileLoop,
    },
    span::span_union,
    token::{Keyword, TokenKind},
};

use super::Parser;

impl Parser {
    pub(super) fn parse_statement(&mut self) -> Result<Statement, ()> {
        self.enter_parse_depth()?;
        let result = self.parse_statement_inner();
        self.exit_parse_depth();
        result
    }

    fn parse_statement_inner(&mut self) -> Result<Statement, ()> {
        let start_span = self.current().span;

        // Check for `if let` before falling into the general match
        if matches!(&self.current().kind, TokenKind::Keyword(Keyword::If))
            && self.position + 1 < self.tokens.len()
            && matches!(
                self.tokens[self.position + 1].kind,
                TokenKind::Keyword(Keyword::Let)
            )
        {
            self.advance(); // consume 'if'
            self.advance(); // consume 'let'
            let kind = self.parse_if_let_statement(start_span)?;
            let end_span = self
                .tokens
                .get(self.position.saturating_sub(1))
                .map(|t| t.span)
                .unwrap_or(start_span);
            return Ok(Statement {
                span: span_union(start_span, end_span),
                kind,
            });
        }

        // Check for `while let` before the general while branch
        if matches!(&self.current().kind, TokenKind::Keyword(Keyword::While))
            && self.position + 1 < self.tokens.len()
            && matches!(
                self.tokens[self.position + 1].kind,
                TokenKind::Keyword(Keyword::Let)
            )
        {
            self.advance(); // consume 'while'
            self.advance(); // consume 'let'
            let kind = self.parse_while_let_statement(start_span)?;
            let end_span = self
                .tokens
                .get(self.position.saturating_sub(1))
                .map(|t| t.span)
                .unwrap_or(start_span);
            return Ok(Statement {
                span: span_union(start_span, end_span),
                kind,
            });
        }

        let kind = match &self.current().kind {
            crate::token::TokenKind::Keyword(Keyword::Let) => {
                self.advance(); // consume 'let'
                self.parse_let_statement()?
            }
            crate::token::TokenKind::Keyword(Keyword::Return) => {
                self.advance(); // consume 'return'
                self.parse_return_statement()?
            }
            crate::token::TokenKind::Keyword(Keyword::While) => {
                self.advance(); // consume 'while'
                self.parse_while_statement()?
            }
            crate::token::TokenKind::Keyword(Keyword::Do) => {
                self.advance(); // consume 'do'
                self.parse_do_while_statement()?
            }
            crate::token::TokenKind::Keyword(Keyword::For) => {
                self.advance(); // consume 'for'
                self.parse_for_statement()?
            }
            crate::token::TokenKind::Keyword(Keyword::Loop) => {
                self.advance(); // consume 'loop'
                self.parse_loop_statement()?
            }
            crate::token::TokenKind::Keyword(Keyword::Switch) => {
                self.advance(); // consume 'switch'
                self.parse_switch_statement()?
            }
            crate::token::TokenKind::Keyword(Keyword::Break) => {
                self.advance(); // consume 'break'
                self.consume_statement_terminator("Expected a line break after 'break'")?;
                StatementKind::Break
            }
            crate::token::TokenKind::Keyword(Keyword::Continue) => {
                self.advance(); // consume 'continue'
                self.consume_statement_terminator("Expected a line break after 'continue'")?;
                StatementKind::Continue
            }
            _ => {
                // Try to parse as assignment or expression statement
                let expr = self.parse_expression()?;

                // Check if followed by '='
                if self.check_symbol('=') {
                    // This is an assignment
                    self.advance(); // consume '='

                    // Convert expression to LValue
                    let (target, target_span) = match expr.kind {
                        crate::ast::ExpressionKind::Identifier(name) => {
                            (crate::ast::LValue::Identifier(name), expr.span)
                        }
                        crate::ast::ExpressionKind::IndexAccess { array, index } => {
                            (crate::ast::LValue::IndexAccess { array, index }, expr.span)
                        }
                        crate::ast::ExpressionKind::FieldAccess { object, field } => {
                            (crate::ast::LValue::FieldAccess { object, field }, expr.span)
                        }
                        _ => {
                            self.error("Invalid assignment target");
                            return Err(());
                        }
                    };

                    let value = self.parse_expression()?;
                    self.consume_statement_terminator("Expected a line break after assignment")?;

                    StatementKind::Assignment(crate::ast::AssignmentStatement {
                        target,
                        target_span,
                        value,
                    })
                } else {
                    // Expression statement

                    // Block expressions terminate with their closing brace;
                    // all other statements use a source line break.
                    let requires_terminator =
                        !matches!(
                            expr.kind,
                            crate::ast::ExpressionKind::If { .. }
                                | crate::ast::ExpressionKind::Unless { .. }
                        )
                            && !self.check_symbol('}');

                    if requires_terminator {
                        self.consume_statement_terminator(
                            "Expected a line break after expression",
                        )?;
                    }

                    StatementKind::Expression(expr)
                }
            }
        };

        let end_span = self
            .tokens
            .get(self.position.saturating_sub(1))
            .map(|t| t.span)
            .unwrap_or(start_span);

        Ok(Statement {
            span: span_union(start_span, end_span),
            kind,
        })
    }

    fn parse_if_let_statement(
        &mut self,
        start_span: crate::span::Span,
    ) -> Result<StatementKind, ()> {
        // `if let Pattern = expr { ... } [else { ... }]`
        // 'if' and 'let' are already consumed
        let pattern = self.parse_pattern()?;
        self.consume_symbol('=', "Expected '=' after pattern in if-let")?;
        let value = self.parse_expression()?;
        let then_block = self.parse_block()?;

        let else_block = if self.check_keyword(Keyword::Else) {
            self.advance(); // consume 'else'
            Some(self.parse_block()?)
        } else {
            None
        };

        let end_span = else_block
            .as_ref()
            .map(|b| b.span)
            .unwrap_or(then_block.span);

        Ok(StatementKind::IfLet(IfLetStatement {
            pattern,
            value,
            then_block,
            else_block,
            span: span_union(start_span, end_span),
        }))
    }

    fn parse_while_let_statement(
        &mut self,
        start_span: crate::span::Span,
    ) -> Result<StatementKind, ()> {
        // `while let Pattern = expr { ... }`
        // 'while' and 'let' are already consumed
        let pattern = self.parse_pattern()?;
        self.consume_symbol('=', "Expected '=' after pattern in while-let")?;
        let value = self.parse_expression()?;
        let body = self.parse_block()?;
        let end_span = body.span;

        Ok(StatementKind::WhileLet(WhileLetStatement {
            pattern,
            value,
            body,
            span: span_union(start_span, end_span),
        }))
    }

    fn parse_let_statement(&mut self) -> Result<StatementKind, ()> {
        // Expect: let <pattern> [: type] [= expr];
        let pattern_span = self.current().span;
        let pattern = self.parse_pattern()?;

        // Optional type annotation
        let ty = if self.check_symbol(':') {
            self.advance(); // consume ':'
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        // Optional initializer
        let value = if self.check_symbol('=') {
            self.advance(); // consume '='
            Some(self.parse_expression()?)
        } else {
            None
        };

        self.consume_statement_terminator("Expected a line break after let statement")?;

        Ok(StatementKind::Let(LetStatement {
            pattern,
            span: pattern_span,
            ty,
            value,
        }))
    }

    fn parse_return_statement(&mut self) -> Result<StatementKind, ()> {
        // Expect: return [expr];
        let start_span = self
            .tokens
            .get(self.position.saturating_sub(1))
            .map(|t| t.span)
            .unwrap_or(self.current().span);

        let value = if !self.statement_ends_before_current() {
            Some(self.parse_expression()?)
        } else {
            None
        };

        self.consume_statement_terminator("Expected a line break after return statement")?;

        Ok(StatementKind::Return(ReturnStatement {
            span: start_span,
            value,
        }))
    }

    fn parse_while_statement(&mut self) -> Result<StatementKind, ()> {
        // Expect: while <condition> { <body> }
        let start_span = self
            .tokens
            .get(self.position.saturating_sub(1))
            .map(|t| t.span)
            .unwrap_or(self.current().span);

        let condition = self.parse_expression()?;
        let body = self.parse_block()?;
        let end_span = body.span;

        Ok(StatementKind::While(WhileLoop {
            condition,
            body,
            span: span_union(start_span, end_span),
        }))
    }

    fn parse_for_statement(&mut self) -> Result<StatementKind, ()> {
        // Expect: for <iterator> in <iterable> { <body> }
        let start_span = self
            .tokens
            .get(self.position.saturating_sub(1))
            .map(|t| t.span)
            .unwrap_or(self.current().span);

        let (iterator, _) = self.consume_identifier("Expected iterator variable name")?;

        if self.check_keyword(Keyword::In) {
            self.advance();
        } else {
            self.error("Expected 'in' after iterator variable");
            return Err(());
        }

        let iterable = self.parse_expression()?;
        let body = self.parse_block()?;
        let end_span = body.span;

        Ok(StatementKind::For(ForLoop {
            iterator,
            iterable,
            body,
            span: span_union(start_span, end_span),
        }))
    }

    fn parse_loop_statement(&mut self) -> Result<StatementKind, ()> {
        // loop { ... }
        let start_span = self
            .tokens
            .get(self.position.saturating_sub(1))
            .map(|t| t.span)
            .unwrap_or(self.current().span);
        let body = self.parse_block()?;
        let end_span = body.span;

        Ok(StatementKind::Loop(LoopStatement {
            body,
            span: span_union(start_span, end_span),
        }))
    }

    fn parse_do_while_statement(&mut self) -> Result<StatementKind, ()> {
        // do { ... } while condition;
        let start_span = self
            .tokens
            .get(self.position.saturating_sub(1))
            .map(|t| t.span)
            .unwrap_or(self.current().span);
        let body = self.parse_block()?;

        if !self.check_keyword(Keyword::While) {
            self.error("Expected 'while' after do-block");
            return Err(());
        }
        self.advance(); // consume 'while'

        let condition = self.parse_expression()?;
        let end_span = condition.span;
        self.consume_statement_terminator("Expected a line break after do-while condition")?;

        Ok(StatementKind::DoWhile(DoWhileLoop {
            body,
            condition,
            span: span_union(start_span, end_span),
        }))
    }

    fn parse_switch_statement(&mut self) -> Result<StatementKind, ()> {
        // switch value { case pattern => body, ... default => body }
        let start_span = self
            .tokens
            .get(self.position.saturating_sub(1))
            .map(|t| t.span)
            .unwrap_or(self.current().span);
        let value = self.parse_expression()?;

        self.consume_symbol('{', "Expected '{' after switch value")?;

        let mut cases = Vec::new();
        let mut default = None;

        while !self.check_symbol('}') && self.position < self.tokens.len() {
            if self.check_keyword(Keyword::Case) {
                self.advance(); // consume 'case'
                let pattern = self.parse_expression()?;

                // Case bodies use a colon before their block.
                if self.check_symbol(':') {
                    self.advance();
                } else {
                    self.error("Expected ':' after case pattern");
                    return Err(());
                }

                let case_body = self.parse_block()?;
                let case_span = span_union(pattern.span, case_body.span);

                cases.push(SwitchCase {
                    pattern,
                    body: case_body,
                    span: case_span,
                });
            } else if self.check_keyword(Keyword::Else) {
                self.advance(); // consume 'else'

                // The default branch may use a colon before its block.
                if self.check_symbol(':') {
                    self.advance();
                }

                default = Some(self.parse_block()?);
                break; // default deve ser o último
            } else {
                self.error("Expected 'case' or 'default' in switch body");
                return Err(());
            }
        }

        let end_span = self.current().span;
        self.consume_symbol('}', "Expected '}' to close switch statement")?;

        Ok(StatementKind::Switch(SwitchStatement {
            value,
            cases,
            default,
            span: span_union(start_span, end_span),
        }))
    }
}
