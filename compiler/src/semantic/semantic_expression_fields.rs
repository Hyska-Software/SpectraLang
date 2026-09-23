use super::*;

/// Outcome of resolving a field on a struct-typed receiver, without any
/// diagnostic side effects (see [`SemanticAnalyzer::check_field_access`] for
/// the reporting wrapper shared by the read and assignment paths).
pub(crate) enum FieldLookup {
    Found {
        ty: Type,
        def_span: Span,
        visibility: Visibility,
        defining_module: Option<String>,
        defining_package: Option<String>,
    },
    NoField,
    NoStruct,
}

impl SemanticAnalyzer {
    pub(crate) fn analyze_expression_field(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::FieldAccess { object, field } => {
                self.analyze_expression(object);

                // Module-qualified function value (`handlers.health`): record
                // the imported signature instead of a struct-field lookup.
                let local_shadow = matches!(
                    &object.kind,
                    ExpressionKind::Identifier(name) if self.lookup_symbol(name).is_some()
                );
                if !local_shadow {
                    if let Some(path) = namespace_path(object) {
                        let qualified = format!("{}.{}", path, field);
                        let signature = self
                            .functions
                            .get(&qualified)
                            .map(|sig| (sig.params.clone(), sig.return_type.clone()))
                            .or_else(|| {
                                self.registry
                                    .read()
                                    .unwrap_or_else(|p| p.into_inner())
                                    .get_module(&path)
                                    .and_then(|exports| exports.functions.get(field))
                                    .map(|func| {
                                        (func.params.clone(), func.return_type.clone())
                                    })
                            });
                        if let Some((params, return_type)) = signature {
                            self.symbol_resolutions.insert(
                                expr.span,
                                SymbolInfo {
                                    is_local: false,
                                    def_span: None,
                                    ty: Type::Fn {
                                        params,
                                        return_type: Box::new(return_type),
                                    },
                                },
                            );
                            return;
                        }
                    }
                }

                let object_type = self.infer_expression_type(object);
                let mut field_ty = Type::Unknown;
                let mut field_def_span = None;

                match &object_type {
                    Type::Struct { .. } | Type::Applied { .. } => {
                        if let Some((ty, def_span)) =
                            self.check_field_access(&object_type, field, expr.span)
                        {
                            field_ty = ty;
                            field_def_span = Some(def_span);
                        }
                    }
                    Type::Unknown => {
                        // Cannot validate without type information
                    }
                    _ => {
                        self.error_coded(
                            "E005",
                            format!(
                                "Cannot access field '{}' on non-struct type {}",
                                field,
                                type_name(&object_type)
                            ),
                            expr.span,
                        );
                    }
                }

                self.symbol_resolutions.insert(
                    expr.span,
                    SymbolInfo {
                        is_local: false,
                        def_span: field_def_span,
                        ty: field_ty,
                    },
                );
            }
            _ => unreachable!("expression category mismatch"),
        }
    }

    /// Resolves `field` on a struct-typed receiver and reports every failure
    /// with a stable code: unknown struct (`E021`), missing field (`E005`),
    /// and field visibility (`E045`, enforced against the declaring
    /// module/package on both the read and assignment paths).
    ///
    /// Returns the field type and its definition span when the field exists —
    /// even if a visibility error was reported, so downstream typing stays
    /// best-effort while compilation already failed.
    pub(crate) fn check_field_access(
        &mut self,
        object_type: &Type,
        field: &str,
        access_span: Span,
    ) -> Option<(Type, Span)> {
        let name = match object_type {
            Type::Struct { name } | Type::Applied { name, .. } => name.clone(),
            _ => return None,
        };

        match self.resolve_struct_field(object_type, field) {
            FieldLookup::Found {
                ty,
                def_span,
                visibility,
                defining_module,
                defining_package,
            } => {
                self.enforce_field_visibility(
                    &name,
                    field,
                    visibility,
                    defining_module,
                    defining_package,
                    access_span,
                );
                Some((ty, def_span))
            }
            FieldLookup::NoField => {
                self.error_coded(
                    "E005",
                    format!("Struct '{}' has no field named '{}'", name, field),
                    access_span,
                );
                None
            }
            FieldLookup::NoStruct => {
                self.error_coded(
                    "E021",
                    format!("Struct '{}' is not defined", name),
                    access_span,
                );
                None
            }
        }
    }

    /// Resolves `field` on a struct-typed receiver without reporting
    /// diagnostics. Returns `NoStruct` for non-struct receivers as well, so
    /// callers decide whether that is an error in their context.
    pub(crate) fn resolve_struct_field(&mut self, object_type: &Type, field: &str) -> FieldLookup {
        let name = match object_type {
            Type::Struct { name } | Type::Applied { name, .. } => name.clone(),
            _ => return FieldLookup::NoStruct,
        };

        let is_applied = matches!(object_type, Type::Applied { .. });
        let nominal_name = self
            .nominal_lookup_name(object_type)
            .unwrap_or_else(|| name.clone());
        let lookup = if is_applied {
            if let Some((_, struct_info, substitutions)) =
                self.specialized_struct_context(&nominal_name)
            {
                if let Some(field_info) = struct_info.fields.get(field) {
                    FieldLookup::Found {
                        ty: self.type_annotation_to_type_with_substitutions(
                            &field_info.ty,
                            &substitutions,
                        ),
                        def_span: field_info.span,
                        visibility: field_info.visibility,
                        defining_module: struct_info.defining_module.clone(),
                        defining_package: struct_info.defining_package.clone(),
                    }
                } else {
                    FieldLookup::NoField
                }
            } else if let Some(struct_info) = self.struct_infos.get(&name) {
                if let Some(field_info) = struct_info.fields.get(field) {
                    FieldLookup::Found {
                        ty: self.type_annotation_to_type(&Some(field_info.ty.clone())),
                        def_span: field_info.span,
                        visibility: field_info.visibility,
                        defining_module: struct_info.defining_module.clone(),
                        defining_package: struct_info.defining_package.clone(),
                    }
                } else {
                    FieldLookup::NoField
                }
            } else {
                FieldLookup::NoStruct
            }
        } else if let Some(struct_info) = self.struct_infos.get(&name) {
            if let Some(field_info) = struct_info.fields.get(field) {
                FieldLookup::Found {
                    ty: self.type_annotation_to_type(&Some(field_info.ty.clone())),
                    def_span: field_info.span,
                    visibility: field_info.visibility,
                    defining_module: struct_info.defining_module.clone(),
                    defining_package: struct_info.defining_package.clone(),
                }
            } else {
                FieldLookup::NoField
            }
        } else if let Some((_, struct_info, substitutions)) =
            self.specialized_struct_context(&nominal_name)
        {
            if let Some(field_info) = struct_info.fields.get(field) {
                FieldLookup::Found {
                    ty: self.type_annotation_to_type_with_substitutions(
                        &field_info.ty,
                        &substitutions,
                    ),
                    def_span: field_info.span,
                    visibility: field_info.visibility,
                    defining_module: struct_info.defining_module.clone(),
                    defining_package: struct_info.defining_package.clone(),
                }
            } else {
                FieldLookup::NoField
            }
        } else {
            FieldLookup::NoStruct
        };
        lookup
    }

    /// Enforces field visibility against the module currently being analyzed:
    /// - `public`: always accessible.
    /// - `private`: accessible only in the declaring module (builtins with no
    ///   declaring module are always accessible).
    /// - `internal`: accessible in the declaring module, or across modules of
    ///   the same package when a package model exists; without a package
    ///   model it falls back to the same-module rule.
    pub(crate) fn enforce_field_visibility(
        &mut self,
        struct_name: &str,
        field: &str,
        visibility: Visibility,
        defining_module: Option<String>,
        defining_package: Option<String>,
        access_span: Span,
    ) {
        let same_module = match &defining_module {
            None => true, // builtin / unknown origin: no module restriction
            Some(defined_in) => self.current_module_name.as_deref() == Some(defined_in.as_str()),
        };

        let accessible = match visibility {
            Visibility::Public => true,
            Visibility::Private => same_module,
            Visibility::Internal => {
                if same_module {
                    true
                } else {
                    match (defining_package.as_deref(), self.current_package.as_deref()) {
                        (Some(defined_in), Some(current)) => defined_in == current,
                        // No package model in this compilation: `internal`
                        // degrades to the same-module rule.
                        _ => false,
                    }
                }
            }
        };

        if !accessible {
            let scope = match visibility {
                Visibility::Private => format!(
                    "private to module {}",
                    defining_module.as_deref().unwrap_or("<unknown>")
                ),
                Visibility::Internal => match defining_package.as_deref() {
                    Some(package) => format!("internal to package {}", package),
                    None => "internal".to_string(),
                },
                Visibility::Public => "public".to_string(),
            };
            self.error_coded_with_hint(
                "E045",
                format!(
                    "Field '{}' of '{}' is {} and cannot be accessed from this module",
                    field, struct_name, scope
                ),
                access_span,
                format!(
                    "Read `{}` from within its declaring scope, or change its visibility to `public`.",
                    field
                ),
            );
        }
    }
}

#[cfg(test)]
mod field_access_diagnostic_tests {
    use crate::semantic::analyze_modules;
    use crate::{CompilationOptions, CompilationPipeline, CompilerError, SemanticError};
    use crate::{Lexer, Parser};

    fn parse_module(source: &str) -> crate::ast::Module {
        let tokens = Lexer::new(source)
            .tokenize()
            .expect("lexer should succeed in field tests");
        Parser::new(tokens)
            .parse()
            .expect("parser should succeed in field tests")
    }

    fn semantic_errors(source: &str) -> Vec<SemanticError> {
        let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
        let errors = pipeline
            .compile(source, "field_access.spectra")
            .expect_err("the source must be rejected");
        errors
            .into_iter()
            .filter_map(|error| match error {
                CompilerError::Semantic(semantic) => Some(semantic),
                _ => None,
            })
            .collect()
    }

    fn has_code(errors: &[SemanticError], code: &str) -> bool {
        errors
            .iter()
            .any(|error| error.code.as_deref() == Some(code))
    }

    #[test]
    fn assignment_to_missing_field_reports_e005() {
        let source = r#"
            module field_assign

            record Point {
                public x: int,
            }

            public func main() returns int {
                let p = Point { x: 1 }
                p.y = 2
                return p.x
            }
        "#;
        let errors = semantic_errors(source);
        assert!(
            has_code(&errors, "E005"),
            "assignment to a missing field must report E005: {errors:?}"
        );
    }

    #[test]
    fn same_module_private_field_access_still_works() {
        let source = r#"
            module local_private

            public record Config {
                public ok: int,
                secret: int,
            }

            public func peek(config: Config) returns int {
                config.secret
            }
        "#;
        let mut module = parse_module(source);
        let mut modules = vec![&mut module];
        analyze_modules(modules.as_mut_slice())
            .expect("same-module private field reads must stay legal");
    }

    #[test]
    fn cross_module_private_field_read_reports_e045() {
        let exporter = r#"
            module priv_a

            public record Config {
                public ok: int,
                secret: int,
            }

            public func make() returns Config {
                Config { ok: 1, secret: 2 }
            }

            public func peek(config: Config) returns int {
                config.secret
            }
        "#;
        let importer = r#"
            module priv_b

            import priv_a

            public func main() returns int {
                let config = priv_a.make()
                let visible = config.ok
                let leaked = config.secret
                return visible
            }
        "#;

        let mut exporter_module = parse_module(exporter);
        let mut importer_module = parse_module(importer);
        let mut modules = vec![&mut exporter_module, &mut importer_module];
        let errors = analyze_modules(modules.as_mut_slice())
            .expect_err("cross-module private field access must be rejected");
        assert!(
            errors
                .iter()
                .any(|error| error.code.as_deref() == Some("E045")
                    && error.message.contains("secret")),
            "expected coded E045 for the private field `secret`: {errors:?}"
        );
        assert!(
            !errors
                .iter()
                .any(|error| error.code.as_deref() == Some("E045")
                    && error.message.contains("'ok'")),
            "the public field `ok` must stay accessible: {errors:?}"
        );
    }
}
