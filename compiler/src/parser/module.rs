use crate::{
    ast::{Import, Module, NamedImport},
    span::span_union,
    token::Keyword,
};

use super::Parser;

impl Parser {
    pub(super) fn parse_module(&mut self) -> Module {
        // Expect: module <name>
        let start_span = match self.consume_keyword(Keyword::Module, "Expected 'module' keyword") {
            Ok(span) => span,
            Err(_) => {
                self.synchronize();
                return Module::new("error", self.current().span);
            }
        };

        let mut name = match self.consume_identifier("Expected module name") {
            Ok((name, _)) => name,
            Err(_) => {
                self.synchronize();
                return Module::new("error", start_span);
            }
        };

        while self.check_symbol('.') {
            self.advance();
            let Ok((segment, _)) =
                self.consume_identifier("Expected identifier after '.' in module name")
            else {
                self.synchronize();
                return Module::new(name, start_span);
            };
            name.push('.');
            name.push_str(&segment);
        }

        let end_span =
            match self.consume_statement_terminator("Expected a line break after module name") {
                Ok(span) => span,
                Err(_) => {
                    self.synchronize();
                    return Module::new(name, start_span);
                }
            };

        let mut module = Module::new(name, span_union(start_span, end_span));

        // Parse module items
        while !self.is_at_end() {
            let start_position = self.position;
            match self.parse_item() {
                Ok(item) => module.items.push(item),
                Err(_) => self.synchronize_with_progress(start_position),
            }
        }

        module
    }

    pub(super) fn parse_import(&mut self, is_reexport: bool) -> Result<Import, ()> {
        // Supports namespace imports, public re-exports, and TS-style brace
        // imports (`import {a, b as c} from path`). Canonical ordered named
        // imports use the `from path import name` surface parsed by
        // `parse_from_import`.
        let start_span = self.consume_keyword(Keyword::Import, "Expected 'import' keyword")?;

        // Brace import: import {name, other as alias} from module.path
        if self.check_symbol('{') {
            self.advance(); // consume '{'
            let names = self.parse_named_import_list(true)?;
            self.consume_symbol('}', "Expected '}' to close the imported name list")?;
            self.consume_keyword(Keyword::From, "Expected 'from' after imported names")?;
            let (path, path_span) = self.parse_module_path()?;
            let end_span =
                self.consume_statement_terminator("Expected a line break after import")?;

            return Ok(Import {
                path,
                alias: None,
                names: Some(names),
                is_reexport,
                span: span_union(start_span, span_union(path_span, end_span)),
            });
        }

        // Standard path form
        let (path, path_span) = self.parse_module_path()?;

        // Optional namespace alias: import path as alias
        let alias = match &self.current().kind {
            crate::token::TokenKind::Keyword(Keyword::As) => {
                self.advance(); // consume 'as'
                let (alias_name, _) = self.consume_identifier("Expected alias name after 'as'")?;
                Some(alias_name)
            }
            _ => None,
        };

        let end_span = self.consume_statement_terminator("Expected a line break after import")?;

        Ok(Import {
            path,
            alias,
            names: None,
            is_reexport,
            span: span_union(start_span, span_union(path_span, end_span)),
        })
    }

    pub(super) fn parse_from_import(&mut self, is_reexport: bool) -> Result<Import, ()> {
        let start_span = self.consume_keyword(Keyword::From, "Expected 'from' keyword")?;
        let (path, path_span) = self.parse_module_path()?;
        self.consume_keyword(Keyword::Import, "Expected 'import' after module path")?;

        let names = self.parse_named_import_list(false)?;

        let end_span =
            self.consume_statement_terminator("Expected a line break after import declaration")?;
        Ok(Import {
            path,
            alias: None,
            names: Some(names),
            is_reexport,
            span: span_union(start_span, span_union(path_span, end_span)),
        })
    }

    /// Parses a comma-separated imported-name list. When `inside_braces` is
    /// set (TS-style brace imports), the list may span multiple lines and the
    /// closing `}` terminates it; otherwise a line break ends the statement.
    fn parse_named_import_list(&mut self, inside_braces: bool) -> Result<Vec<NamedImport>, ()> {
        let mut names = Vec::new();
        loop {
            let (name, name_span) = self.consume_identifier("Expected imported name")?;
            let (alias, end_span) = self.parse_import_alias(name_span)?;
            names.push(NamedImport {
                name,
                alias,
                span: span_union(name_span, end_span),
            });

            if !self.check_symbol(',') {
                break;
            }
            self.advance();
            if self.check_symbol('}') && inside_braces {
                break;
            }
            if !inside_braces && self.statement_ends_before_current() {
                break;
            }
        }
        Ok(names)
    }

    fn parse_module_path(&mut self) -> Result<(Vec<String>, crate::span::Span), ()> {
        let (first, first_span) = self.consume_identifier("Expected module path")?;
        let mut path = vec![first];
        let mut end_span = first_span;
        while self.check_symbol('.') {
            self.advance();
            let (segment, segment_span) =
                self.consume_identifier("Expected identifier after '.' in module path")?;
            path.push(segment);
            end_span = segment_span;
        }
        Ok((path, end_span))
    }

    fn parse_import_alias(
        &mut self,
        name_span: crate::span::Span,
    ) -> Result<(Option<String>, crate::span::Span), ()> {
        if self.check_keyword(Keyword::As) {
            self.advance();
            let (alias, alias_span) = self.consume_identifier("Expected alias after 'as'")?;
            Ok((Some(alias), alias_span))
        } else {
            Ok((None, name_span))
        }
    }
}
