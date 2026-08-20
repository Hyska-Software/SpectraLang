impl Parser {
    pub(super) fn parse_item(&mut self) -> Result<Item, ()> {
        let attributes = self.parse_outer_attributes()?;
        match &self.current().kind {
            crate::token::TokenKind::Keyword(Keyword::Import) => {
                if !attributes.is_empty() {
                    self.error_at(
                        "Attributes are currently supported only on functions, structs, and enums",
                        attributes[0].span,
                    );
                    return Err(());
                }
                let import = self.parse_import(false)?;
                Ok(Item::Import(import))
            }
            crate::token::TokenKind::Keyword(Keyword::From) => {
                if !attributes.is_empty() {
                    self.error_at(
                        "Attributes are currently supported only on functions, structs, and enums",
                        attributes[0].span,
                    );
                    return Err(());
                }
                let import = self.parse_from_import(false)?;
                Ok(Item::Import(import))
            }
            crate::token::TokenKind::Keyword(Keyword::Public) => {
                self.advance(); // consume the visibility modifier
                                // `pub import ...` is a re-export: imported symbols are exposed to callers.
                if matches!(
                    &self.current().kind,
                    crate::token::TokenKind::Keyword(Keyword::Import)
                        | crate::token::TokenKind::Keyword(Keyword::From)
                ) {
                    if !attributes.is_empty() {
                        self.error_at(
                            "Attributes are currently supported only on functions, structs, and enums",
                            attributes[0].span,
                        );
                        return Err(());
                    }
                    let import = if self.check_keyword(Keyword::From) {
                        self.parse_from_import(true)?
                    } else {
                        self.parse_import(true)?
                    };
                    return Ok(Item::Import(import));
                }
                self.parse_item_with_visibility(Visibility::Public, attributes)
            }
            crate::token::TokenKind::Keyword(Keyword::Internal) => {
                self.advance(); // consume 'internal'
                                // `internal import ...` re-exports within the same package only.
                if matches!(&self.current().kind, crate::token::TokenKind::Keyword(Keyword::Import) | crate::token::TokenKind::Keyword(Keyword::From)) {
                    if !attributes.is_empty() {
                        self.error_at(
                            "Attributes are currently supported only on functions, structs, and enums",
                            attributes[0].span,
                        );
                        return Err(());
                    }
                    let mut import = if self.check_keyword(Keyword::From) {
                        self.parse_from_import(false)?
                    } else {
                        self.parse_import(false)?
                    };
                    // Mark as internal re-export (visible only within the package).
                    // We reuse the is_reexport flag and rely on the visibility of the
                    // importing module item to be Internal at the call site.
                    import.is_reexport = false; // not a full re-export
                    return Ok(Item::Import(import));
                }
                self.parse_item_with_visibility(Visibility::Internal, attributes)
            }
            crate::token::TokenKind::Keyword(Keyword::Func) => {
                self.parse_item_with_visibility(Visibility::Private, attributes)
            }
            crate::token::TokenKind::Keyword(Keyword::Async) => {
                self.parse_item_with_visibility(Visibility::Private, attributes)
            }
            crate::token::TokenKind::Keyword(Keyword::Record) => {
                self.parse_item_with_visibility(Visibility::Private, attributes)
            }
            crate::token::TokenKind::Keyword(Keyword::Enum) => {
                self.parse_item_with_visibility(Visibility::Private, attributes)
            }
            crate::token::TokenKind::Keyword(Keyword::Impl) => {
                if !attributes.is_empty() {
                    self.error_at(
                        "Attributes are currently supported only on functions, structs, and enums",
                        attributes[0].span,
                    );
                    return Err(());
                }
                self.parse_impl_block()
            }
            crate::token::TokenKind::Keyword(Keyword::Trait) => {
                if !attributes.is_empty() {
                    self.error_at(
                        "Attributes are currently supported only on functions, structs, and enums",
                        attributes[0].span,
                    );
                    return Err(());
                }
                let trait_decl = self.parse_trait_declaration()?;
                Ok(Item::Trait(trait_decl))
            }
            crate::token::TokenKind::Keyword(Keyword::Type) => {
                if !attributes.is_empty() {
                    self.error_at(
                        "Attributes are currently supported only on functions, structs, and enums",
                        attributes[0].span,
                    );
                    return Err(());
                }
                self.parse_type_alias(Visibility::Private)
            }
            crate::token::TokenKind::Keyword(Keyword::Const) => {
                if !attributes.is_empty() {
                    self.error_at(
                        "Attributes are currently supported only on functions, structs, and enums",
                        attributes[0].span,
                    );
                    return Err(());
                }
                self.parse_const_decl(Visibility::Private)
            }
            crate::token::TokenKind::Keyword(Keyword::Static) => {
                if !attributes.is_empty() {
                    self.error_at(
                        "Attributes are currently supported only on functions, structs, and enums",
                        attributes[0].span,
                    );
                    return Err(());
                }
                self.parse_static_decl(Visibility::Private)
            }
            crate::token::TokenKind::Keyword(Keyword::Class) => {
                self.push_error_coded(
                    "P007",
                    "Class declarations are reserved and are not supported in the stable language",
                    self.current().span,
                    Some("Use `struct` with `impl` and `trait`; class layout and inheritance are deferred.".to_string()),
                    Some("`class` is reserved for a future language contract".to_string()),
                );
                self.consume_reserved_class_item();
                Err(())
            }
            _ => {
                self.error("Expected item declaration (import, fn, etc.)");
                Err(())
            }
        }
    }

    fn parse_outer_attributes(&mut self) -> Result<Vec<Attribute>, ()> {
        let mut attributes = Vec::new();

        while self.check_symbol('#') {
            let start_span = self.current().span;
            self.advance();
            self.consume_symbol('[', "Expected '[' after '#' in attribute")?;
            let (name, name_span) = self.consume_identifier("Expected attribute name")?;
            let arguments = if self.check_symbol('(') {
                self.parse_attribute_arguments()?
            } else {
                Vec::new()
            };
            let end_span = self.consume_symbol(']', "Expected ']' after attribute")?;
            attributes.push(Attribute {
                name,
                arguments,
                span: span_union(start_span, span_union(name_span, end_span)),
            });
        }

        Ok(attributes)
    }

    fn parse_attribute_arguments(&mut self) -> Result<Vec<AttributeArgument>, ()> {
        self.consume_symbol('(', "Expected '(' after attribute name")?;
        let mut arguments = Vec::new();
        if self.check_symbol(')') {
            self.advance();
            return Ok(arguments);
        }

        loop {
            let (key, _) = self.consume_identifier("Expected attribute argument")?;
            if self.check_symbol('=') {
                self.advance();
                let value = match &self.current().kind.clone() {
                    TokenKind::StringLiteral(value)
                    | TokenKind::Identifier(value)
                    | TokenKind::Number(value) => {
                        let value = value.clone();
                        self.advance();
                        value
                    }
                    _ => {
                        self.error("Expected string, identifier, or number after '=' in attribute");
                        return Err(());
                    }
                };
                arguments.push(AttributeArgument::KeyValue { key, value });
            } else {
                arguments.push(AttributeArgument::Name(key));
            }

            if self.check_symbol(',') {
                self.advance();
                if self.check_symbol(')') {
                    break;
                }
            } else if self.check_symbol(')') {
                break;
            } else {
                self.error("Expected ',' or ')' in attribute argument list");
                return Err(());
            }
        }

        self.consume_symbol(')', "Expected ')' after attribute arguments")?;
        Ok(arguments)
    }

    fn parse_item_with_visibility(
        &mut self,
        visibility: Visibility,
        attributes: Vec<Attribute>,
    ) -> Result<Item, ()> {
        match &self.current().kind {
            crate::token::TokenKind::Keyword(Keyword::Func) => {
                let function = self.parse_function(visibility, attributes)?;
                Ok(Item::Function(function))
            }
            crate::token::TokenKind::Keyword(Keyword::Async) => {
                let function = self.parse_function(visibility, attributes)?;
                Ok(Item::Function(function))
            }
            crate::token::TokenKind::Keyword(Keyword::Record) => {
                let struct_item = self.parse_struct(visibility, attributes)?;
                Ok(Item::Struct(struct_item))
            }
            crate::token::TokenKind::Keyword(Keyword::Enum) => {
                let enum_item = self.parse_enum(visibility, attributes)?;
                Ok(Item::Enum(enum_item))
            }
            crate::token::TokenKind::Keyword(Keyword::Impl) => {
                if !attributes.is_empty() {
                    self.error_at(
                        "Attributes are currently supported only on functions, structs, and enums",
                        attributes[0].span,
                    );
                    return Err(());
                }
                self.parse_impl_block()
            }
            crate::token::TokenKind::Keyword(Keyword::Trait) => {
                if !attributes.is_empty() {
                    self.error_at(
                        "Attributes are currently supported only on functions, structs, and enums",
                        attributes[0].span,
                    );
                    return Err(());
                }
                let trait_decl = self.parse_trait_declaration()?;
                Ok(Item::Trait(trait_decl))
            }
            crate::token::TokenKind::Keyword(Keyword::Type) => {
                if !attributes.is_empty() {
                    self.error_at(
                        "Attributes are currently supported only on functions, structs, and enums",
                        attributes[0].span,
                    );
                    return Err(());
                }
                self.parse_type_alias(visibility)
            }
            crate::token::TokenKind::Keyword(Keyword::Const) => {
                if !attributes.is_empty() {
                    self.error_at(
                        "Attributes are currently supported only on functions, structs, and enums",
                        attributes[0].span,
                    );
                    return Err(());
                }
                self.parse_const_decl(visibility)
            }
            crate::token::TokenKind::Keyword(Keyword::Static) => {
                if !attributes.is_empty() {
                    self.error_at(
                        "Attributes are currently supported only on functions, structs, and enums",
                        attributes[0].span,
                    );
                    return Err(());
                }
                self.parse_static_decl(visibility)
            }
            crate::token::TokenKind::Keyword(Keyword::Class) => {
                self.push_error_coded(
                    "P007",
                    "Class declarations are reserved and are not supported in the stable language",
                    self.current().span,
                    Some("Use `struct` with `impl` and `trait`; class layout and inheritance are deferred.".to_string()),
                    Some("`class` is reserved for a future language contract".to_string()),
                );
                self.consume_reserved_class_item();
                Err(())
            }
            _ => {
                self.error("Expected function, struct, enum, or impl declaration");
                Err(())
            }
        }
    }

}
