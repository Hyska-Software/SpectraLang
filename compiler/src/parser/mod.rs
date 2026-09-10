mod expression;
mod item;
mod module;
mod statement;
mod type_annotation;
pub mod workspace;

use crate::{
    ast::{Module, TypeAnnotation, TypeAnnotationKind},
    error::ParseError,
    span::{Location, Span},
    token::{Keyword, Token, TokenKind},
};
use std::collections::HashMap;
use std::fmt;

/// Hard cap on parser recursion depth across the main descent points
/// (expression / statement / block / pattern). Legitimate programs nest far
/// below this (the test suite peaks around 50 levels); anything deeper fails
/// with `P013` instead of exhausting the stack.
const MAX_PARSE_DEPTH: usize = 1000;

/// Approximate stack bytes the parser may consume before bailing out with
/// `P013`. Depth counting alone cannot know the thread's real stack size, so
/// this byte budget (measured from a probe captured in [`Parser::new`])
/// guarantees the guard fires on small-stack threads (test harness, spawned
/// tasks) as well as on the main thread. Debug-build recursion frames are fat
/// enough that ~512 KiB corresponds to several hundred nesting levels — far
/// above any legitimate program.
const MAX_STACK_USE_BYTES: usize = 512 * 1024;

pub struct Parser {
    tokens: Vec<Token>,
    /// Sentinela EOF devolvido por `current()` quando position é maior que tokens.
    /// Garante que o parser nunca panique por lista de tokens vazia.
    eof_sentinel: Token,
    position: usize,
    errors: Vec<ParseError>,
    trait_signatures: HashMap<String, HashMap<String, TraitMethodSignature>>,
    async_context_depth: usize,
    /// Current recursion depth across the main descent points
    /// (expression / statement / block / pattern), capped by
    /// [`MAX_PARSE_DEPTH`] to avoid stack exhaustion on pathological input.
    depth: usize,
    /// Ensures the `P013` nesting diagnostic is emitted only once even after
    /// the guard trips repeatedly during error recovery.
    depth_limit_reported: bool,
    /// Stack address captured when the parser was created; used to estimate
    /// how much stack the recursive descent has already consumed.
    stack_probe: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        let eof_sentinel = Token::new(
            TokenKind::EndOfFile,
            Span::new(0, 0, Location::new(1, 1), Location::new(1, 1)),
        );
        let mut parser = Self {
            tokens,
            eof_sentinel,
            position: 0,
            errors: Vec::new(),
            // Pre-allocate with a reasonable capacity to reduce rehashing while
            // parsing modules that typically have a handful of known traits.
            trait_signatures: HashMap::with_capacity(8),
            async_context_depth: 0,
            depth: 0,
            depth_limit_reported: false,
            stack_probe: Self::capture_stack_probe(),
        };
        parser.register_builtin_async_traits();
        parser
    }

    fn register_builtin_async_traits(&mut self) {
        let self_param = ParameterSignature {
            is_self: true,
            is_reference: true,
            is_mutable: false,
            ty: None,
        };
        let int_return = Some(TypePattern::Simple(vec!["int".to_string()]));

        self.trait_signatures.insert(
            "Future".to_string(),
            HashMap::from([
                (
                    "poll".to_string(),
                    TraitMethodSignature {
                        params: vec![self_param.clone()],
                        return_type: int_return.clone(),
                        has_default_body: false,
                        is_async: true,
                    },
                ),
                (
                    "cancel".to_string(),
                    TraitMethodSignature {
                        params: vec![self_param.clone()],
                        return_type: int_return.clone(),
                        has_default_body: false,
                        is_async: false,
                    },
                ),
            ]),
        );
        self.trait_signatures.insert(
            "Stream".to_string(),
            HashMap::from([
                (
                    "next".to_string(),
                    TraitMethodSignature {
                        params: vec![self_param.clone()],
                        return_type: int_return.clone(),
                        has_default_body: false,
                        is_async: true,
                    },
                ),
                (
                    "cancel".to_string(),
                    TraitMethodSignature {
                        params: vec![self_param],
                        return_type: int_return,
                        has_default_body: false,
                        is_async: false,
                    },
                ),
            ]),
        );
    }

    pub fn parse(mut self) -> Result<Module, Vec<ParseError>> {
        let module = self.parse_module();

        if self.errors.is_empty() {
            Ok(module)
        } else {
            Err(self.errors)
        }
    }

    // === Token Navigation Methods ===

    fn current(&self) -> &Token {
        self.tokens.get(self.position).unwrap_or(&self.eof_sentinel)
    }

    fn advance(&mut self) {
        if !self.is_at_end() {
            self.position += 1;
        }
    }

    fn is_at_end(&self) -> bool {
        matches!(self.current().kind, TokenKind::EndOfFile)
    }

    // === Token Checking Methods ===

    fn check_keyword(&self, keyword: Keyword) -> bool {
        matches!(&self.current().kind, TokenKind::Keyword(k) if *k == keyword)
    }

    pub(super) fn check_function_keyword(&self) -> bool {
        self.check_keyword(Keyword::Func)
    }

    pub(super) fn consume_function_keyword(&mut self, error_message: &str) -> Result<Span, ()> {
        if self.check_function_keyword() {
            let span = self.current().span;
            self.advance();
            Ok(span)
        } else {
            let span = self.current().span;
            self.push_error_coded("P001", error_message, span, None, None);
            Err(())
        }
    }

    pub(super) fn consume_record_keyword(&mut self, error_message: &str) -> Result<Span, ()> {
        if self.check_keyword(Keyword::Record) {
            let span = self.current().span;
            self.advance();
            Ok(span)
        } else {
            let span = self.current().span;
            self.push_error_coded("P001", error_message, span, None, None);
            Err(())
        }
    }

    fn check_symbol(&self, symbol: char) -> bool {
        matches!(&self.current().kind, TokenKind::Symbol(s) if *s == symbol)
    }

    fn check_identifier(&self) -> bool {
        matches!(self.current().kind, TokenKind::Identifier(_))
    }

    // === Token Consumption Methods ===

    fn consume_keyword(&mut self, keyword: Keyword, error_message: &str) -> Result<Span, ()> {
        if self.check_keyword(keyword.clone()) {
            let span = self.current().span;
            self.advance();
            Ok(span)
        } else {
            let span = self.current().span;
            let context = format!(
                "expected keyword `{}`, found {}",
                keyword,
                Self::describe_token(self.current())
            );
            let hint = self.keyword_hint(keyword.clone());
            self.push_error_coded("P001", error_message, span, hint, Some(context));
            Err(())
        }
    }

    fn consume_symbol(&mut self, symbol: char, error_message: &str) -> Result<Span, ()> {
        if self.check_symbol(symbol) {
            let span = self.current().span;
            self.advance();
            Ok(span)
        } else if let Some(recovery_span) = self.recover_missing_symbol(symbol) {
            let context = format!(
                "missing `{}` before {}",
                symbol,
                Self::describe_token(self.current())
            );
            let hint = self
                .symbol_hint(symbol)
                .or_else(|| Some(format!("Insert `{}` here.", symbol)));
            self.push_error_coded("P002", error_message, recovery_span, hint, Some(context));
            Ok(recovery_span)
        } else {
            let span = self.current().span;
            let context = format!(
                "expected symbol `{}`, found {}",
                symbol,
                Self::describe_token(self.current())
            );
            let hint = self.symbol_hint(symbol);
            self.push_error_coded("P002", error_message, span, hint, Some(context));
            Err(())
        }
    }

    /// Consume the canonical statement terminator.
    ///
    /// Spectra's readable surface terminates statements with a line break (or
    /// the end of a block/file). Semicolons are rejected after the migration.
    pub(super) fn consume_statement_terminator(&mut self, error_message: &str) -> Result<Span, ()> {
        if self.check_symbol(';') {
            let span = self.current().span;
            self.push_error_coded(
                "P012",
                "Semicolons are not valid statement terminators in Spectra",
                span,
                Some(
                    "Remove the semicolon and end the statement with a line break instead."
                        .to_string(),
                ),
                Some("semicolon terminator".to_string()),
            );
            self.advance();
            return Err(());
        }

        if self.is_at_end() || self.check_symbol('}') || self.line_break_before_current() {
            return Ok(self.synthetic_span_before_current());
        }

        let span = self.current().span;
        self.push_error_coded(
            "P011",
            error_message,
            span,
            Some("End the statement with a line break or close the surrounding block.".to_string()),
            Some("a statement continued on the same line without a separator".to_string()),
        );
        Err(())
    }

    /// Whether the current token starts on a later source line than the token
    /// most recently consumed.  Newlines are deliberately not emitted as
    /// tokens: spans already carry the exact source location and this keeps
    /// delimiter parsing and tooling token streams stable during migration.
    fn line_break_before_current(&self) -> bool {
        if self.position == 0 {
            return false;
        }
        let previous = self.tokens[self.position - 1].span;
        self.current().span.start_location.line > previous.end_location.line
    }

    /// Whether an infix binary operator may legally appear even though a
    /// line break precedes it.  This implements the explicit-continuation
    /// exception of the newline rule documented at the top of
    /// `expression_precedence.rs`: the previous token must be a construct
    /// that visibly cannot end the expression (another operator, an
    /// assignment `=`, a comma, or an open delimiter).
    fn infix_operator_continues_line(&self) -> bool {
        if self.position == 0 {
            return false;
        }
        matches!(
            &self.tokens[self.position - 1].kind,
            TokenKind::Symbol(',')
                | TokenKind::Symbol('(')
                | TokenKind::Symbol('[')
                | TokenKind::Symbol('=')
                | TokenKind::Operator(_)
                | TokenKind::Keyword(Keyword::AndWord)
                | TokenKind::Keyword(Keyword::OrWord)
                | TokenKind::Keyword(Keyword::NotWord)
        )
    }

    fn infix_token_text(kind: &TokenKind) -> String {
        match kind {
            TokenKind::Symbol(c) => c.to_string(),
            TokenKind::Operator(op) => op.to_string(),
            TokenKind::Keyword(Keyword::AndWord) => "and".to_string(),
            TokenKind::Keyword(Keyword::OrWord) => "or".to_string(),
            _ => "operator".to_string(),
        }
    }

    /// Gate used by every binary-operator loop in the precedence ladder.
    ///
    /// Returns `Ok(())` when the current infix operator sits on the same line
    /// as its left-hand side, or when it is an explicit continuation under the
    /// rule above. Otherwise emits `P015` and fails so the expression ends at
    /// the previous line instead of silently absorbing the next one.
    pub(super) fn reject_line_broken_infix(&mut self) -> Result<(), ()> {
        if !self.line_break_before_current() || self.infix_operator_continues_line() {
            return Ok(());
        }
        let span = self.current().span;
        let text = Self::infix_token_text(&self.current().kind);
        self.push_error_coded(
            "P015",
            "a line break before an infix operator ends the expression",
            span,
            Some(format!(
                "Move `{text}` to the end of the previous line to continue the expression, or wrap the operands in parentheses."
            )),
            Some("operator at the start of a new line".to_string()),
        );
        Err(())
    }

    pub(super) fn statement_ends_before_current(&self) -> bool {
        self.check_symbol(';')
            || self.is_at_end()
            || self.check_symbol('}')
            || self.line_break_before_current()
    }

    fn consume_identifier(&mut self, error_message: &str) -> Result<(String, Span), ()> {
        if let TokenKind::Identifier(name) = &self.current().kind.clone() {
            let name = name.clone();
            let span = self.current().span;
            self.advance();
            Ok((name, span))
        } else {
            let span = self.current().span;
            let context = format!(
                "expected identifier, found {}",
                Self::describe_token(self.current())
            );
            self.push_error_coded("P003", error_message, span, None, Some(context));
            Err(())
        }
    }

    // === Error Handling ===

    fn error(&mut self, message: &str) {
        let span = self.current().span;
        self.push_error(message, span, None, None);
    }

    fn error_at(&mut self, message: &str, span: Span) {
        self.push_error(message, span, None, None);
    }

    // === Synchronization ===

    fn synchronize(&mut self) {
        // Check if the current token is already a recovery boundary before
        // advancing, so we don't inadvertently skip valid constructs.
        if self.is_at_boundary() {
            // Consume a ';' boundary so the caller starts fresh after it.
            if self.check_symbol(';') {
                self.advance();
            }
            return;
        }

        self.advance();

        while !self.is_at_end() {
            if self.check_symbol(';') {
                self.advance();
                return;
            }

            if self.is_at_boundary() {
                return;
            }

            self.advance();
        }
    }

    fn synchronize_with_progress(&mut self, start_position: usize) {
        self.synchronize();
        if self.position == start_position && !self.is_at_end() {
            self.advance();
        }
    }

    /// Returns `true` when the current token is a natural recovery boundary
    /// (a `}` or a keyword that starts a new top-level / statement construct).
    fn is_at_boundary(&self) -> bool {
        if self.check_symbol('}') {
            return true;
        }
        matches!(
            &self.current().kind,
            TokenKind::Keyword(Keyword::Module)
                | TokenKind::Keyword(Keyword::Import)
                | TokenKind::Keyword(Keyword::From)
                | TokenKind::Keyword(Keyword::Async)
                | TokenKind::Keyword(Keyword::Func)
                | TokenKind::Keyword(Keyword::Record)
                | TokenKind::Keyword(Keyword::Public)
                | TokenKind::Keyword(Keyword::Class)
                | TokenKind::Keyword(Keyword::Trait)
                | TokenKind::Keyword(Keyword::Let)
                | TokenKind::Keyword(Keyword::Return)
                | TokenKind::Keyword(Keyword::Else)
                | TokenKind::Keyword(Keyword::Case)
                | TokenKind::Keyword(Keyword::Switch)
        )
    }

    fn recover_missing_symbol(&self, symbol: char) -> Option<Span> {
        match symbol {
            ';' => {
                if self.is_at_end() || self.is_statement_boundary_token() {
                    return Some(self.synthetic_span_before_current());
                }
            }
            '}' => {
                if self.is_at_end() || self.is_block_terminator_token() {
                    return Some(self.synthetic_span_before_current());
                }
            }
            ')' if (self.is_at_end()
                || self.is_post_paren_boundary_token()
                || self.is_statement_boundary_token()) =>
            {
                return Some(self.synthetic_span_before_current());
            }
            _ => {}
        }

        None
    }

    fn recover_in_delimited_list(
        &mut self,
        terminator_symbols: &[char],
        separator_symbols: &[char],
    ) {
        while !self.is_at_end() {
            match &self.current().kind {
                TokenKind::Symbol(symbol)
                    if terminator_symbols.contains(symbol)
                        || separator_symbols.contains(symbol)
                        || matches!(symbol, '}' | ';') =>
                {
                    return;
                }
                TokenKind::Keyword(_) | TokenKind::EndOfFile => return,
                _ => self.advance(),
            }
        }
    }

    fn synthetic_span_before_current(&self) -> Span {
        if self.position == 0 {
            return Span::dummy();
        }

        let prev_span = self.tokens[self.position - 1].span;
        Span::new(
            prev_span.end,
            prev_span.end,
            prev_span.end_location,
            prev_span.end_location,
        )
    }

    fn is_statement_boundary_token(&self) -> bool {
        if self.is_at_end() {
            return true;
        }

        match &self.current().kind {
            TokenKind::Keyword(kw) => matches!(
                kw,
                Keyword::Let
                    | Keyword::Return
                    | Keyword::If
                    | Keyword::Unless
                    | Keyword::NotWord
                    | Keyword::Match
                    | Keyword::While
                    | Keyword::Do
                    | Keyword::For
                    | Keyword::Loop
                    | Keyword::Switch
                    | Keyword::Break
                    | Keyword::Continue
                    | Keyword::Func
                    | Keyword::Record
                    | Keyword::Enum
                    | Keyword::Trait
                    | Keyword::Class
                    | Keyword::Module
                    | Keyword::Import
                    | Keyword::Async
                    | Keyword::Public
                    | Keyword::Case
                    | Keyword::Else
                    | Keyword::From
            ),
            TokenKind::Identifier(_) | TokenKind::Number(_) | TokenKind::StringLiteral(_) => true,
            TokenKind::Symbol('(') | TokenKind::Symbol('{') => true,
            _ => false,
        }
    }

    fn is_block_terminator_token(&self) -> bool {
        if self.is_at_end() {
            return true;
        }

        matches!(
            &self.current().kind,
            TokenKind::Keyword(Keyword::Else)
                | TokenKind::Keyword(Keyword::From)
                | TokenKind::Keyword(Keyword::Case)
                | TokenKind::Keyword(Keyword::Func)
                | TokenKind::Keyword(Keyword::Record)
                | TokenKind::Keyword(Keyword::Enum)
                | TokenKind::Keyword(Keyword::Impl)
                | TokenKind::Keyword(Keyword::Class)
                | TokenKind::Keyword(Keyword::Module)
                | TokenKind::Keyword(Keyword::Import)
                | TokenKind::Keyword(Keyword::Async)
                | TokenKind::Keyword(Keyword::Return)
        )
    }

    fn is_post_paren_boundary_token(&self) -> bool {
        if self.is_at_end() {
            return true;
        }

        matches!(
            &self.current().kind,
            TokenKind::Symbol('{') | TokenKind::Symbol(')')
        )
    }

    fn push_error(
        &mut self,
        message: impl Into<String>,
        span: Span,
        hint: Option<String>,
        context: Option<String>,
    ) {
        self.push_error_coded("P999", message, span, hint, context);
    }

    fn push_error_coded(
        &mut self,
        code: &str,
        message: impl Into<String>,
        span: Span,
        hint: Option<String>,
        context: Option<String>,
    ) {
        let mut error = ParseError::new(message, span).with_code(code);

        if let Some(context) = context {
            error = error.with_context(context);
        } else {
            error = error.with_context(format!("found {}", Self::describe_token(self.current())));
        }

        if let Some(hint) = hint {
            error = error.with_hint(hint);
        }

        self.errors.push(error);
    }

    fn describe_token(token: &Token) -> String {
        match &token.kind {
            TokenKind::Identifier(name) => format!("identifier `{}`", name),
            TokenKind::Number(value) => format!("number `{}`", value),
            TokenKind::Keyword(keyword) => format!("keyword `{}`", keyword),
            TokenKind::Symbol(symbol) => format!("symbol `{}`", symbol),
            TokenKind::Operator(op) => format!("operator `{}`", op),
            TokenKind::StringLiteral(value) => {
                const MAX_PREVIEW: usize = 24;
                if value.len() > MAX_PREVIEW {
                    // Recue até um limite de caractere UTF-8: fatiar por byte
                    // dentro de um caractere multibyte causaria panic.
                    let mut end = MAX_PREVIEW;
                    while !value.is_char_boundary(end) {
                        end -= 1;
                    }
                    let mut preview = value[..end].to_string();
                    preview.push('…');
                    format!("string literal \"{}\"", preview)
                } else {
                    format!("string literal \"{}\"", value)
                }
            }
            TokenKind::EndOfFile => "end of file".to_string(),
            TokenKind::CharLiteral(c) => format!("char literal '{}'", c),
            TokenKind::FStringLiteral(_) => "f-string literal".to_string(),
        }
    }

    fn keyword_hint(&self, keyword: Keyword) -> Option<String> {
        match keyword {
            Keyword::Module => {
                Some("Start the file with `module <name>` on its own line.".to_string())
            }
            Keyword::Import => Some(
                "Use `from path.to.module import name` or `import path.to.module` on its own line."
                    .to_string(),
            ),
            Keyword::Func => Some("Function declarations start with `func name(...)`.".to_string()),
            Keyword::Async => Some(
                "`async` must be followed by `func`, `{ ... }`, or a closure parameter list."
                    .to_string(),
            ),
            Keyword::Trait => {
                Some("Traits are declared with `trait TraitName { ... }`.".to_string())
            }
            Keyword::Impl => Some("Use `impl Type` to provide trait implementations.".to_string()),
            Keyword::Let => {
                Some("Introduce bindings with `let name = expression` on its own line.".to_string())
            }
            Keyword::Return => {
                Some("Use `return expression` to exit a function early.".to_string())
            }
            Keyword::While | Keyword::For | Keyword::Loop => Some(
                "Loops require a control keyword such as `while`, `for`, or `loop`.".to_string(),
            ),
            _ => None,
        }
    }

    fn symbol_hint(&self, symbol: char) -> Option<String> {
        match symbol {
            ';' => {
                Some("Remove the semicolon and end the statement with a line break.".to_string())
            }
            ')' => Some("Close the parenthesis with `)`.".to_string()),
            '}' => Some("Close the block with `}`.".to_string()),
            ']' => Some("Close the bracket with `]`.".to_string()),
            '{' => Some("Insert `{` to open a block.".to_string()),
            '(' => Some("Insert `(` to start the parameter or argument list.".to_string()),
            _ => None,
        }
    }

    pub(super) fn push_async_context(&mut self) {
        self.async_context_depth += 1;
    }

    pub(super) fn pop_async_context(&mut self) {
        self.async_context_depth = self.async_context_depth.saturating_sub(1);
    }

    pub(super) fn in_async_context(&self) -> bool {
        self.async_context_depth > 0
    }

    /// Enters one level of parser recursion. Returns `Err(())` — after
    /// emitting a single `P013` diagnostic at the crossing point — when the
    /// nesting exceeds [`MAX_PARSE_DEPTH`] or the parser has consumed more
    /// than [`MAX_STACK_USE_BYTES`] of stack, so deeply nested but otherwise
    /// well-formed input fails cleanly instead of exhausting the stack.
    pub(super) fn enter_parse_depth(&mut self) -> Result<(), ()> {
        self.depth += 1;
        let over_depth = self.depth > MAX_PARSE_DEPTH;
        let over_stack = Self::stack_used_bytes(self.stack_probe) > MAX_STACK_USE_BYTES;
        if over_depth || over_stack {
            if !self.depth_limit_reported {
                self.depth_limit_reported = true;
                let span = self.current().span;
                self.push_error_coded(
                    "P013",
                    "nesting too deep",
                    span,
                    Some(format!(
                        "Reduce nesting of expressions, statements, blocks, or patterns to at most {} levels.",
                        MAX_PARSE_DEPTH
                    )),
                    Some(
                        "parser recursion exceeded its nesting/stack guard".to_string(),
                    ),
                );
            }
            return Err(());
        }
        Ok(())
    }

    pub(super) fn exit_parse_depth(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Captures an approximate "top of stack" marker at parser creation so
    /// [`Self::stack_used_bytes`] can estimate consumed stack later. Stacks
    /// grow downward on all supported platforms, so later frames live at
    /// lower addresses.
    fn capture_stack_probe() -> usize {
        let marker = 0u8;
        &marker as *const u8 as usize
    }

    fn stack_used_bytes(base: usize) -> usize {
        base.saturating_sub(Self::capture_stack_probe())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Module;
    use crate::lexer::Lexer;
    fn parse_source(source: &str) -> Result<Module, Vec<ParseError>> {
        let tokens = Lexer::new(source)
            .tokenize()
            .expect("lexer should not fail in parser tests");
        Parser::new(tokens).parse()
    }

    #[test]
    fn loop_parses_without_feature_flag() {
        let source = r#"
            module demo

            func main() {
                loop {
                    break
                }
            }
        "#;

        assert!(parse_source(source).is_ok());
    }

    #[test]
    fn negated_condition_parses_without_feature_flag() {
        let source = r#"
            module demo

            func main() {
                let value = if not false { 1 }
            }
        "#;

        assert!(parse_source(source).is_ok());
    }

    #[test]
    fn parses_import_forms() {
        let source = r#"
            module demo

            import std.io
            import std.math as math
            from std.io import println, print
            public from std.io import println
        "#;

        let module = parse_source(source).expect("imports should parse");
        let imports: Vec<_> = module
            .items
            .iter()
            .filter_map(|item| match item {
                crate::ast::Item::Import(import) => Some(import),
                _ => None,
            })
            .collect();

        assert_eq!(imports.len(), 4);

        assert_eq!(imports[0].path, vec!["std".to_string(), "io".to_string()]);
        assert_eq!(imports[0].alias, None);
        assert_eq!(imports[0].names, None);
        assert!(!imports[0].is_reexport);

        assert_eq!(imports[1].path, vec!["std".to_string(), "math".to_string()]);
        assert_eq!(imports[1].alias.as_deref(), Some("math"));
        assert_eq!(imports[1].names, None);

        assert_eq!(imports[2].path, vec!["std".to_string(), "io".to_string()]);
        assert_eq!(
            imports[2]
                .names
                .as_ref()
                .unwrap()
                .iter()
                .map(|entry| (entry.name.as_str(), entry.alias.as_deref()))
                .collect::<Vec<_>>(),
            vec![("println", None), ("print", None)]
        );
        assert!(!imports[2].is_reexport);

        assert_eq!(imports[3].path, vec!["std".to_string(), "io".to_string()]);
        assert_eq!(
            imports[3]
                .names
                .as_ref()
                .unwrap()
                .iter()
                .map(|entry| (entry.name.as_str(), entry.alias.as_deref()))
                .collect::<Vec<_>>(),
            vec![("println", None)]
        );
        assert!(imports[3].is_reexport);
    }

    #[test]
    fn parses_brace_import_forms() {
        let source = r#"
            module demo

            import { println, print } from std.io
            import { abs as absolute } from std.math
            public import { min } from std.math
            import {
                max,
                sign,
            } from std.math
        "#;

        let module = parse_source(source).expect("brace imports should parse");
        let imports: Vec<_> = module
            .items
            .iter()
            .filter_map(|item| match item {
                crate::ast::Item::Import(import) => Some(import),
                _ => None,
            })
            .collect();

        assert_eq!(imports.len(), 4);

        assert_eq!(imports[0].path, vec!["std".to_string(), "io".to_string()]);
        assert_eq!(imports[0].alias, None);
        assert_eq!(
            imports[0]
                .names
                .as_ref()
                .unwrap()
                .iter()
                .map(|entry| (entry.name.as_str(), entry.alias.as_deref()))
                .collect::<Vec<_>>(),
            vec![("println", None), ("print", None)]
        );
        assert!(!imports[0].is_reexport);

        // Alias inside braces desugars to the same NamedImport shape as the
        // canonical `from X import name as alias` form.
        assert_eq!(
            imports[1]
                .names
                .as_ref()
                .unwrap()
                .iter()
                .map(|entry| (entry.name.as_str(), entry.alias.as_deref()))
                .collect::<Vec<_>>(),
            vec![("abs", Some("absolute"))]
        );

        assert!(imports[2].is_reexport);
        assert_eq!(
            imports[2].names.as_ref().unwrap()[0].name,
            "min".to_string()
        );

        // Multi-line brace list with trailing comma.
        assert_eq!(
            imports[3]
                .names
                .as_ref()
                .unwrap()
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["max", "sign"]
        );
    }

    #[test]
    fn wildcard_and_identifier_patterns_carry_real_spans() {
        let source = r#"
            module demo

            func pick(value: int) returns int {
                match value {
                    otherwise then 0
                }
            }

            func bind(value: int) returns int {
                match value {
                    when n then n
                }
            }
        "#;

        let module = parse_source(source).expect("match expressions should parse");

        let mut spans = Vec::new();
        for item in &module.items {
            if let crate::ast::Item::Function(function) = item {
                for statement in &function.body.statements {
                    if let crate::ast::StatementKind::Expression(expression) = &statement.kind {
                        collect_match_arm_pattern_spans(expression, &mut spans);
                    }
                }
            }
        }

        assert_eq!(spans.len(), 2, "expected two match arm patterns");

        for span in &spans {
            // Span::dummy() has start == end == 0.
            assert!(
                !(span.start == 0 && span.end == 0),
                "pattern span should not be dummy, got {:?}",
                span
            );
        }
        // The `otherwise` wildcard sits on an earlier line than `when n`.
        assert!(spans[0].start_location.line < spans[1].start_location.line);
    }

    fn collect_match_arm_pattern_spans(expression: &crate::ast::Expression, spans: &mut Vec<Span>) {
        if let crate::ast::ExpressionKind::Match { scrutinee, arms } = &expression.kind {
            let _ = scrutinee;
            for arm in arms {
                spans.push(arm.pattern.span());
            }
        }
    }

    #[test]
    fn async_frontend_surface_preserves_ast_markers() {
        let source = r#"
            module demo

            async func fetch() {
                let task = async { 1 }
                let work = async |value: int| { value }
                let empty = async || 1
            }

            record Service {}

            impl Service {
                public async func handle(&self) {
                    let value = async { 2 }
                }
            }

            trait Handler {
                async func call(&self)
            }
        "#;

        let module = parse_source(source).expect("async frontend syntax should parse");
        let crate::ast::Item::Function(function) = &module.items[0] else {
            panic!("expected async function item");
        };
        assert!(function.is_async);

        let crate::ast::StatementKind::Let(task_stmt) = &function.body.statements[0].kind else {
            panic!("expected async block binding");
        };
        assert!(matches!(
            task_stmt.value.as_ref().map(|expr| &expr.kind),
            Some(crate::ast::ExpressionKind::AsyncBlock(_))
        ));

        let crate::ast::StatementKind::Let(work_stmt) = &function.body.statements[1].kind else {
            panic!("expected async lambda binding");
        };
        assert!(matches!(
            work_stmt.value.as_ref().map(|expr| &expr.kind),
            Some(crate::ast::ExpressionKind::Lambda { is_async: true, .. })
        ));

        let crate::ast::Item::Impl(impl_block) = &module.items[2] else {
            panic!("expected impl block");
        };
        assert!(impl_block.methods[0].is_async);

        let crate::ast::Item::Trait(trait_decl) = &module.items[3] else {
            panic!("expected trait declaration");
        };
        assert!(trait_decl.methods[0].is_async);
    }

    #[test]
    fn async_and_await_misuse_produce_actionable_diagnostics() {
        let misplaced_async = r#"
            module demo

            func main() {
                let value = async 1
            }
        "#;
        let errors = parse_source(misplaced_async).expect_err("misplaced async should fail");
        assert!(errors.iter().any(|error| {
            error.code.as_deref() == Some("P005")
                && error
                    .hint
                    .as_deref()
                    .is_some_and(|hint| hint.contains("async {"))
        }));

        let await_outside_async = r#"
            module demo

            func main() {
                let value = await work()
            }
        "#;
        let errors =
            parse_source(await_outside_async).expect_err("await outside async should fail");
        assert!(errors.iter().any(|error| {
            error.code.as_deref() == Some("P006")
                && error
                    .hint
                    .as_deref()
                    .is_some_and(|hint| hint.contains("async func"))
        }));

        let await_inside_async = r#"
            module demo

            async func main() {
                let value = await work()
            }
        "#;
        let module = parse_source(await_inside_async).expect("await inside async parses");
        let crate::ast::Item::Function(function) = &module.items[0] else {
            panic!("expected function");
        };
        let crate::ast::StatementKind::Let(stmt) = &function.body.statements[0].kind else {
            panic!("expected let statement");
        };
        assert!(matches!(
            stmt.value.as_ref().map(|expr| &expr.kind),
            Some(crate::ast::ExpressionKind::Await(_))
        ));
    }

    #[test]
    fn class_declarations_are_reserved_with_stable_diagnostic() {
        let source = r#"
            module demo
            class User { }
        "#;
        let errors = parse_source(source).expect_err("class must be rejected");
        assert!(errors.iter().any(|error| {
            error.code.as_deref() == Some("P007")
                && error.message.contains("reserved")
                && error
                    .hint
                    .as_deref()
                    .is_some_and(|hint| hint.contains("record"))
        }));
    }

    #[test]
    fn for_of_misuse_produces_coded_diagnostic() {
        let source = r#"
            module demo

            func sum(values: [int]) returns int {
                for item of values {
                    return item
                }
                return 0
            }
        "#;
        let errors = parse_source(source).expect_err("`for x of` must fail");
        assert!(errors.iter().any(|error| {
            error.code.as_deref() == Some("P016")
                && error
                    .hint
                    .as_deref()
                    .is_some_and(|hint| hint.contains("for x in"))
        }));
    }

    #[test]
    fn missing_in_after_iterator_produces_coded_diagnostic() {
        let source = r#"
            module demo

            func sum(values: [int]) returns int {
                for item values {
                    return item
                }
                return 0
            }
        "#;
        let errors = parse_source(source).expect_err("missing `in` must fail");
        assert!(errors
            .iter()
            .any(|error| error.code.as_deref() == Some("P017")));
    }

    #[test]
    fn invalid_assignment_target_produces_coded_diagnostic() {
        let source = r#"
            module demo

            func broken() returns int {
                1 + 2 = 3
                return 0
            }
        "#;
        let errors =
            parse_source(source).expect_err("assignment to a literal expression must fail");
        assert!(errors.iter().any(|error| {
            error.code.as_deref() == Some("P018")
                && error
                    .hint
                    .as_deref()
                    .is_some_and(|hint| hint.contains("field accesses"))
        }));
    }

    #[test]
    fn describe_token_truncates_long_multibyte_string_literals_on_char_boundary() {
        let span = Span::new(0, 40, Location::new(1, 1), Location::new(1, 2));

        // Byte 24 cai dentro do caractere multibyte `é` que começa no byte 23.
        let crossing = format!("{}{}", "x".repeat(23), "é".repeat(5));
        assert!(crossing.len() > 24);
        assert!(!crossing.is_char_boundary(24));
        let token = Token::new(TokenKind::StringLiteral(crossing), span);
        let described = Parser::describe_token(&token);
        assert!(described.starts_with("string literal \""));
        assert!(described.ends_with("…\""));

        // String longa uniformemente multibyte também não pode panicar.
        let uniform = "á".repeat(20);
        let token = Token::new(TokenKind::StringLiteral(uniform), span);
        let described = Parser::describe_token(&token);
        assert!(described.contains('…'));
    }

    #[test]
    fn legacy_alias_keywords_are_rejected() {
        for source in [
            "module demo\npublic struct Point { x: int }\nfunc main() returns int { return 0 }\n",
            "module demo\npublic fn main() returns int { return 0 }\n",
            "module demo\npub func main() returns int { return 0 }\n",
        ] {
            let errors = parse_source(source).expect_err("legacy alias must be rejected");
            assert!(!errors.is_empty(), "expected errors for {source:?}");
        }
    }

    #[test]
    fn unless_parses_in_statement_and_expression_position() {
        let statement_position = r#"
            module demo

            func main(value: bool) {
                unless value {
                    let a = 1
                } else {
                    let b = 2
                }
            }
        "#;
        let expression_position = r#"
            module demo

            func main(value: bool) returns int {
                let picked = unless value { 1 } else { 2 }
                return picked
            }
        "#;

        let module = parse_source(statement_position).expect("unless parses as a statement");
        let crate::ast::Item::Function(function) = &module.items[0] else {
            panic!("expected function item");
        };
        let crate::ast::StatementKind::Expression(expr) = &function.body.statements[0].kind else {
            panic!("expected unless expression statement");
        };
        assert!(matches!(
            expr.kind,
            crate::ast::ExpressionKind::Unless { .. }
        ));

        let module =
            parse_source(expression_position).expect("unless parses in expression position");
        let crate::ast::Item::Function(function) = &module.items[0] else {
            panic!("expected function item");
        };
        let crate::ast::StatementKind::Let(stmt) = &function.body.statements[0].kind else {
            panic!("expected let statement");
        };
        let Some(crate::ast::ExpressionKind::Unless {
            condition,
            then_block,
            else_block,
        }) = stmt.value.as_ref().map(|expr| &expr.kind)
        else {
            panic!("expected Unless expression");
        };
        assert!(matches!(
            condition.kind,
            crate::ast::ExpressionKind::Identifier(_)
        ));
        assert_eq!(then_block.statements.len(), 1);
        let Some(else_block) = else_block else {
            panic!("expected else block on unless");
        };
        assert_eq!(else_block.statements.len(), 1);
    }

    #[test]
    fn unless_rejects_elif_chains() {
        let source = r#"
            module demo

            func main(value: bool) {
                unless value {
                    let a = 1
                } else if value {
                    let b = 2
                }
            }
        "#;

        let errors = parse_source(source).expect_err("unless must reject elif");
        assert!(errors
            .iter()
            .any(|error| error.message.contains("`unless` does not support")));
    }

    #[test]
    fn range_binds_loosest_and_continues_logical_operators() {
        let source = r#"
            module demo

            func main(x: int) {
                let chained = 1..2..3
                let compared = 1..2 == x
            }
        "#;

        let module = parse_source(source).expect("range expressions parse");
        let crate::ast::Item::Function(function) = &module.items[0] else {
            panic!("expected function item");
        };

        let crate::ast::StatementKind::Let(chained) = &function.body.statements[0].kind else {
            panic!("expected chained range binding");
        };
        let Some(crate::ast::ExpressionKind::Range { start, .. }) =
            chained.value.as_ref().map(|expr| &expr.kind)
        else {
            panic!("expected outer Range expression for `1..2..3`");
        };
        assert!(matches!(
            start.kind,
            crate::ast::ExpressionKind::Range { .. }
        ));

        let crate::ast::StatementKind::Let(compared) = &function.body.statements[1].kind else {
            panic!("expected comparison binding");
        };
        let Some(crate::ast::ExpressionKind::Binary {
            left,
            operator: crate::ast::BinaryOperator::Equal,
            ..
        }) = compared.value.as_ref().map(|expr| &expr.kind)
        else {
            panic!(
                "`1..2 == x` should parse as `(1..2) == x`, got {:#?}",
                compared.value.as_ref().map(|expr| &expr.kind)
            );
        };
        assert!(matches!(
            left.kind,
            crate::ast::ExpressionKind::Range { .. }
        ));
    }

    #[test]
    fn excessive_nesting_fails_with_p013_instead_of_stack_overflow() {
        // Pure grouping nesting: each paren level enters parse_expression once.
        let depth = MAX_PARSE_DEPTH + 64;
        let mut source = String::from("module demo\n\nfunc main() {\n    let v = ");
        for _ in 0..depth {
            source.push('(');
        }
        source.push('1');
        for _ in 0..depth {
            source.push(')');
        }
        source.push_str("\n}\n");

        // Run on a thread with a large stack so the depth guard — not the OS —
        // decides the outcome regardless of the harness thread stack size.
        std::thread::Builder::new()
            .stack_size(256 * 1024 * 1024)
            .spawn(move || {
                let errors =
                    parse_source(&source).expect_err("nesting beyond the limit must fail cleanly");
                assert!(
                    errors
                        .iter()
                        .any(|error| error.code.as_deref() == Some("P013")),
                    "expected a P013 nesting diagnostic"
                );
            })
            .expect("spawn deep-parse thread")
            .join()
            .expect("deep parse must not panic");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitMethodSignature {
    pub params: Vec<ParameterSignature>,
    pub return_type: Option<TypePattern>,
    pub has_default_body: bool,
    pub is_async: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterSignature {
    pub is_self: bool,
    pub is_reference: bool,
    pub is_mutable: bool,
    pub ty: Option<TypePattern>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypePattern {
    Simple(Vec<String>),
    Tuple(Vec<TypePattern>),
}

impl TypePattern {
    pub fn from_annotation(annotation: &TypeAnnotation) -> Self {
        match &annotation.kind {
            TypeAnnotationKind::Simple { segments } => TypePattern::Simple(segments.clone()),
            TypeAnnotationKind::Tuple { elements } => {
                TypePattern::Tuple(elements.iter().map(TypePattern::from_annotation).collect())
            }
            TypeAnnotationKind::Function { .. } => TypePattern::Simple(vec!["fn".to_string()]),
            TypeAnnotationKind::Generic { name, .. } => TypePattern::Simple(vec![name.clone()]),
            TypeAnnotationKind::DynTrait {
                trait_name,
                auto_traits,
            } => {
                let name = if auto_traits.is_empty() {
                    format!("dyn {}", trait_name)
                } else {
                    format!("dyn {} + {}", trait_name, auto_traits.join(" + "))
                };
                TypePattern::Simple(vec![name])
            }
        }
    }
}

impl fmt::Display for TypePattern {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypePattern::Simple(segments) => formatter.write_str(&segments.join("::")),
            TypePattern::Tuple(elements) => {
                formatter.write_str("(")?;
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(formatter, "{element}")?;
                }
                formatter.write_str(")")
            }
        }
    }
}
