    use super::{
        count_brace_transitions, count_leading_closing_braces, ends_with_binary_operator,
        finalize_output, is_wrapped_continuation_line, net_round_bracket_delta, normalize_spacing,
        FormattedLine, FormatterConfig,
    };
    use spectra_compiler::ast::{
        Block, Enum, Expression, ExpressionKind, Function, ImplBlock, Import, Item, Method, Module,
        Statement, StatementKind, Struct, TraitDeclaration, TraitImpl,
    };
    use spectra_compiler::token::{Keyword, Token, TokenKind};
    use spectra_compiler::{span::Span, Lexer, Parser};
    
    use std::mem;
    use std::ops::Range;

    pub(super) fn format_with_cst(
        input: &str,
        config: &FormatterConfig,
    ) -> Result<String, super::FormatError> {
        let tokens = Lexer::new(input).tokenize().map_err(|errors| {
            let first = errors.into_iter().next();
            match first {
                Some(error) => super::FormatError::parse(error.message, error.span),
                None => super::FormatError::internal("lexer failed without diagnostics"),
            }
        })?;
        let parser_tokens = tokens.clone();
        let module = Parser::new(parser_tokens)
            .parse()
            .map_err(|errors| {
                let first = errors.into_iter().next();
                match first {
                    Some(error) => super::FormatError::parse(error.message, error.span),
                    None => super::FormatError::internal("parser failed without diagnostics"),
                }
            })?;

        let lines = build_lines(input, &tokens).map_err(|()| {
            super::FormatError::internal("formatter line splitter rejected valid tokens")
        })?;

        let mut formatted = Vec::new();
        let mut indent_level = 0usize;
        let mut anchor_indent = 0usize;
        let mut pending_open: i32 = 0;
        // True when the previously emitted content line ended with a binary
        // operator, per Spectra's operator-at-line-end continuation rule.
        let mut prev_ends_open = false;

        for line in lines {
            match line {
                CstLine::Blank => formatted.push(FormattedLine::blank()),
                CstLine::DocComment(text) => {
                    formatted.push(FormattedLine::new(indent_level, text));
                }
                CstLine::Content(elements) => {
                    let raw_line = build_line_text(&elements);
                    if raw_line.trim().is_empty() {
                        formatted.push(FormattedLine::blank());
                        continue;
                    }

                    let normalized = normalize_spacing(&raw_line);
                    let trimmed = normalized.trim_start();
                    let mut dedent = count_leading_closing_braces(trimmed);
                    if dedent > indent_level {
                        dedent = indent_level;
                    }
                    let delta = net_round_bracket_delta(trimmed);
                    // Operator continuations only apply at bracket depth
                    // zero; inside a multi-line delimited group the paren
                    // machinery already indents.
                    let ends_open = ends_with_binary_operator(&normalized);
                    let starts_open = pending_open <= 0 && is_wrapped_continuation_line(&normalized);
                    // A line whose predecessor ended with an operator always
                    // indents one level deeper; the wrap-opening line itself
                    // keeps the statement indent.
                    let continuation = prev_ends_open;
                    let _ = starts_open;
                    let indent_for_line = if continuation {
                        anchor_indent + 1
                    } else {
                        indent_level.saturating_sub(dedent)
                    };

                    let (opens, closes) = count_brace_transitions(trimmed, dedent);
                    formatted.push(FormattedLine::new(indent_for_line, normalized));
                    pending_open = (pending_open + delta).max(0);
                    let level_base = if continuation { anchor_indent } else { indent_for_line };
                    indent_level = level_base + opens;
                    indent_level = indent_level.saturating_sub(closes);
                    if !continuation {
                        anchor_indent = indent_for_line;
                    }
                    prev_ends_open = ends_open;
                }
            }
        }

        apply_ast_policies(&mut formatted, &module, &tokens, input);

        Ok(finalize_output(formatted, config))
    }

    enum CstLine {
        Blank,
        DocComment(String),
        Content(Vec<LineElement>),
    }

    #[derive(Clone)]
    struct LineToken {
        kind: TokenKind,
        lexeme: String,
    }

    enum LineElement {
        Token(LineToken),
        Comment(String),
    }

    #[derive(Default)]
    struct LineAccumulator {
        elements: Vec<LineElement>,
    }

    impl LineAccumulator {
        fn push_token(&mut self, token: LineToken) {
            self.elements.push(LineElement::Token(token));
        }

        fn push_comment(&mut self, comment: String) {
            self.elements.push(LineElement::Comment(comment));
        }

        fn finish_into(&mut self, lines: &mut Vec<CstLine>) {
            if self.elements.is_empty() {
                return;
            }
            lines.push(CstLine::Content(mem::take(&mut self.elements)));
        }

        fn consume_into_blank(&mut self, lines: &mut Vec<CstLine>) {
            if self.elements.is_empty() {
                lines.push(CstLine::Blank);
            } else {
                lines.push(CstLine::Content(mem::take(&mut self.elements)));
            }
        }
    }

