    use super::{
        count_brace_transitions, count_leading_closing_braces, finalize_output, normalize_spacing,
        FormattedLine, FormatterConfig,
    };
    use spectra_compiler::ast::{
        Block, Enum, Expression, ExpressionKind, Function, ImplBlock, Import, Item, Method, Module,
        Statement, StatementKind, Struct, TraitDeclaration, TraitImpl,
    };
    use spectra_compiler::token::{Keyword, Operator, Token, TokenKind};
    use spectra_compiler::{span::Span, Lexer, Parser};
    use std::collections::HashSet;
    use std::mem;
    use std::ops::Range;

    pub(super) fn format_with_cst(input: &str, config: &FormatterConfig) -> Result<String, ()> {
        let tokens = Lexer::new(input).tokenize().map_err(|_| ())?;
        let parser_tokens = tokens.clone();
        let module = Parser::new(parser_tokens, HashSet::new())
            .parse()
            .map_err(|_| ())?;

        let lines = build_lines(input, &tokens)?;

        let mut formatted = Vec::new();
        let mut indent_level = 0usize;

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
                    let indent_for_line = indent_level.saturating_sub(dedent);

                    let (opens, closes) = count_brace_transitions(trimmed, dedent);
                    formatted.push(FormattedLine::new(indent_for_line, normalized));
                    indent_level = indent_for_line + opens;
                    indent_level = indent_level.saturating_sub(closes);
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

