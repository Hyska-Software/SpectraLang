impl Parser {
    #[allow(dead_code)]
    fn trait_types_compatible(
        expected: &TypePattern,
        actual: &TypePattern,
        impl_type_name: &str,
    ) -> bool {
        match (expected, actual) {
            (TypePattern::Simple(lhs), TypePattern::Simple(rhs)) => {
                if lhs == rhs {
                    return true;
                }

                lhs.len() == 1 && lhs[0] == "Self" && rhs.len() == 1 && rhs[0] == impl_type_name
            }
            (TypePattern::Tuple(lhs), TypePattern::Tuple(rhs)) => {
                lhs.len() == rhs.len()
                    && lhs
                        .iter()
                        .zip(rhs.iter())
                        .all(|(l, r)| Self::trait_types_compatible(l, r, impl_type_name))
            }
            _ => false,
        }
    }

    // ── Type Alias ────────────────────────────────────────────────────────

    fn parse_type_alias(&mut self, visibility: Visibility) -> Result<Item, ()> {
        let start_span = self.consume_keyword(Keyword::Type, "Expected 'type' keyword")?;
        let (name, _) = self.consume_identifier("Expected alias name after 'type'")?;
        self.consume_symbol('=', "Expected '=' after alias name")?;
        let ty = self.parse_type_annotation()?;
        let end_span = self
            .consume_statement_terminator("Expected a line break after type alias")?;
        Ok(Item::TypeAlias(TypeAlias {
            name,
            span: span_union(start_span, end_span),
            visibility,
            ty,
        }))
    }

    // ── Const / Static ────────────────────────────────────────────────────

    fn parse_const_decl(&mut self, visibility: Visibility) -> Result<Item, ()> {
        let start_span = self.consume_keyword(Keyword::Const, "Expected 'const' keyword")?;
        let (name, _) = self.consume_identifier("Expected constant name")?;
        let ty = if self.check_symbol(':') {
            self.advance();
            Some(self.parse_type_annotation()?)
        } else {
            None
        };
        self.consume_symbol('=', "Expected '=' after constant name")?;
        let value = self.parse_expression()?;
        let end_span = self
            .consume_statement_terminator("Expected a line break after const declaration")?;
        Ok(Item::Const(ConstDecl {
            name,
            span: span_union(start_span, end_span),
            visibility,
            ty,
            value,
        }))
    }

    fn parse_static_decl(&mut self, visibility: Visibility) -> Result<Item, ()> {
        let start_span = self.consume_keyword(Keyword::Static, "Expected 'static' keyword")?;
        let (name, _) = self.consume_identifier("Expected static variable name")?;
        let ty = if self.check_symbol(':') {
            self.advance();
            Some(self.parse_type_annotation()?)
        } else {
            None
        };
        self.consume_symbol('=', "Expected '=' after static variable name")?;
        let value = self.parse_expression()?;
        let end_span = self
            .consume_statement_terminator("Expected a line break after static declaration")?;
        Ok(Item::Static(StaticDecl {
            name,
            span: span_union(start_span, end_span),
            visibility,
            ty,
            value,
        }))
    }

    /// Consume the remainder of a reserved `class` item so parser recovery
    /// reports the stable P007 once instead of cascading on its body tokens.
    fn consume_reserved_class_item(&mut self) {
        if self.check_keyword(Keyword::Class) {
            self.advance();
        }

        let mut brace_depth = 0usize;
        while !self.is_at_end() {
            if self.check_symbol('{') {
                brace_depth += 1;
                self.advance();
                continue;
            }
            if self.check_symbol('}') {
                self.advance();
                if brace_depth == 0 {
                    return;
                }
                brace_depth -= 1;
                if brace_depth == 0 {
                    return;
                }
                continue;
            }

            if brace_depth == 0 && self.is_at_boundary() {
                return;
            }
            self.advance();
        }
    }
}
