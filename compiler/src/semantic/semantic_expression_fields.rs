impl SemanticAnalyzer {
    fn analyze_expression_field(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::FieldAccess { object, field } => {
                self.analyze_expression(object);

                let object_type = self.infer_expression_type(object);
                let mut field_ty = Type::Unknown;
                let mut field_def_span = None;

                match &object_type {
                    Type::Struct { name } | Type::Applied { name, .. } => {
                        // Collect all info from immutable borrows first, then emit errors.
                        enum FieldLookup {
                            Found {
                                ty: Type,
                                span: Span,
                                visibility: Visibility,
                            },
                            NoField,
                            NoStruct,
                        }
                        let is_applied = matches!(&object_type, Type::Applied { .. });
                        let nominal_name = self
                            .nominal_lookup_name(&object_type)
                            .unwrap_or_else(|| name.clone());
                        let lookup = if is_applied {
                            if let Some((_, struct_info, substitutions)) =
                                self.specialized_struct_context(&nominal_name)
                            {
                                if let Some(field_info) = struct_info.fields.get(field.as_str()) {
                                    FieldLookup::Found {
                                        ty: self.type_annotation_to_type_with_substitutions(
                                            &field_info.ty,
                                            &substitutions,
                                        ),
                                        span: field_info.span,
                                        visibility: field_info.visibility,
                                    }
                                } else {
                                    FieldLookup::NoField
                                }
                            } else if let Some(struct_info) = self.struct_infos.get(name) {
                                if let Some(field_info) = struct_info.fields.get(field.as_str()) {
                                    FieldLookup::Found {
                                        ty: self
                                            .type_annotation_to_type(&Some(field_info.ty.clone())),
                                        span: field_info.span,
                                        visibility: field_info.visibility,
                                    }
                                } else {
                                    FieldLookup::NoField
                                }
                            } else {
                                FieldLookup::NoStruct
                            }
                        } else if let Some(struct_info) = self.struct_infos.get(name) {
                            if let Some(field_info) = struct_info.fields.get(field.as_str()) {
                                FieldLookup::Found {
                                    ty: self
                                        .type_annotation_to_type(&Some(field_info.ty.clone())),
                                    span: field_info.span,
                                    visibility: field_info.visibility,
                                }
                            } else {
                                FieldLookup::NoField
                            }
                        } else if let Some((_, struct_info, substitutions)) =
                            self.specialized_struct_context(&nominal_name)
                        {
                            if let Some(field_info) = struct_info.fields.get(field.as_str()) {
                                FieldLookup::Found {
                                    ty: self.type_annotation_to_type_with_substitutions(
                                        &field_info.ty,
                                        &substitutions,
                                    ),
                                    span: field_info.span,
                                    visibility: field_info.visibility,
                                }
                            } else {
                                FieldLookup::NoField
                            }
                        } else {
                            FieldLookup::NoStruct
                        };
                        let name = name.clone();
                        match lookup {
                            FieldLookup::Found {
                                ty,
                                span: fspan,
                                visibility: fvis,
                            } => {
                                // Enforce field visibility
                                let accessible = match fvis {
                                    Visibility::Public => true,
                                    Visibility::Internal => true,
                                    Visibility::Private => true, // module-local: accessible within the module
                                };
                                if !accessible {
                                    self.error(
                                        format!(
                                            "Field '{}' of '{}' is private and cannot be accessed outside its impl block",
                                            field, name
                                        ),
                                        expr.span,
                                    );
                                }
                                field_ty = ty;
                                field_def_span = Some(fspan);
                            }
                            FieldLookup::NoField => {
                                self.error(
                                    format!("Struct '{}' has no field named '{}'", name, field),
                                    expr.span,
                                );
                            }
                            FieldLookup::NoStruct => {
                                self.error_coded(
                                    "E021",
                                    format!("Struct '{}' is not defined", name),
                                    expr.span,
                                );
                            }
                        }
                    }
                    Type::Unknown => {
                        // Cannot validate without type information
                    }
                    _ => {
                        self.error(
                            format!(
                                "Cannot access field '{}' on non-struct type {:?}",
                                field, object_type
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
}
