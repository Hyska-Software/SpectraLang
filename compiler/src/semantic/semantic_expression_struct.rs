use super::*;

impl SemanticAnalyzer {
    pub(crate) fn analyze_expression_struct(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::StructLiteral {
                name,
                type_args,
                fields,
            } => {
                // Validate struct exists
                let struct_info = match self.struct_infos.get(name).cloned() {
                    Some(info) => info,
                    None => {
                        self.error_coded(
                            "E021",
                            format!("Struct '{}' is not defined", name),
                            expr.span,
                        );
                        // Still analyze field expressions to surface nested errors
                        for (_, field_value) in fields {
                            self.analyze_expression(field_value);
                        }
                        return;
                    }
                };

                // Validate type argument arity when explicitly provided
                let expected_type_arg_count = struct_info.type_params.len();
                if !type_args.is_empty() {
                    if expected_type_arg_count == 0 {
                        self.error(
                            format!(
                                "Struct '{}' does not accept type arguments, but {} were provided",
                                name,
                                type_args.len()
                            ),
                            expr.span,
                        );
                    } else if type_args.len() != expected_type_arg_count {
                        self.error(
                            format!(
                                "Struct '{}' expects {} type argument(s), but {} were provided",
                                name,
                                expected_type_arg_count,
                                type_args.len()
                            ),
                            expr.span,
                        );
                    }
                }

                let mut substitutions = struct_info
                    .type_params
                    .iter()
                    .zip(type_args.iter())
                    .map(|(param, arg)| {
                        (
                            param.clone(),
                            self.type_annotation_to_type(&Some(arg.clone())),
                        )
                    })
                    .collect::<HashMap<_, _>>();

                // A declared expected application specializes a generic
                // record even when one or more fields do not expose their
                // type parameters (for example `Snapshot<int>` containing
                // `Maybe::Empty`). Explicit constructor arguments remain the
                // source of truth when present.
                if type_args.is_empty() {
                    if let Some(Type::Applied {
                        name: expected_name,
                        args,
                    }) = self.current_expected_type.clone()
                    {
                        if expected_name == *name && args.len() == struct_info.type_params.len() {
                            for (param, arg) in struct_info.type_params.iter().zip(args) {
                                substitutions.entry(param.clone()).or_insert(arg);
                            }
                        }
                    }
                }

                // Generic struct constructors may omit their type arguments
                // when the field values provide an unambiguous application,
                // e.g. `Wrapper { value: 12 }` -> `Wrapper<int>`.  The later
                // AST inference pass is needed by the midend, but semantic
                // validation must use the same concrete substitution now;
                // otherwise a valid field is compared with `Unknown` and the
                // binding keeps the unspecialized aggregate type.
                if type_args.is_empty() && !struct_info.type_params.is_empty() {
                    if let Some((type_params, field_defs)) = self.generic_structs.get(name).cloned()
                    {
                        let inferred_args =
                            self.infer_struct_type_args(&type_params, &field_defs, fields);
                        for (param, arg) in struct_info.type_params.iter().zip(inferred_args) {
                            substitutions
                                .entry(param.clone())
                                .or_insert_with(|| self.type_annotation_to_type(&Some(arg)));
                        }
                    }
                }

                let mut provided_fields = HashSet::new();

                for (field_name, field_value) in fields {
                    let expected_field_type = struct_info.fields.get(field_name).map(|field| {
                        self.type_annotation_to_type_with_substitutions(&field.ty, &substitutions)
                    });
                    let saved_expected = self.current_expected_type.clone();
                    self.current_expected_type = expected_field_type.clone();
                    self.analyze_expression(field_value);
                    let value_type = self.infer_expression_type(field_value);
                    self.current_expected_type = saved_expected;

                    if !provided_fields.insert(field_name.clone()) {
                        self.error(
                            format!(
                                "Field '{}' is specified multiple times in struct literal '{}'",
                                field_name, name
                            ),
                            field_value.span,
                        );
                        continue;
                    }

                    if let Some(expected_field) = struct_info.fields.get(field_name) {
                        let expected_type = expected_field_type.unwrap_or_else(|| {
                            self.type_annotation_to_type_with_substitutions(
                                &expected_field.ty,
                                &substitutions,
                            )
                        });

                        if !self.generic_argument_types_match(&value_type, &expected_type) {
                            let mut message = format!(
                                "Field '{}' in struct '{}' has type {:?}, but {:?} was expected",
                                field_name, name, value_type, expected_type
                            );
                            if let Some(hint) = self.conversion_hint(&value_type, &expected_type) {
                                message.push(' ');
                                message.push_str(&hint);
                            }

                            self.error_coded("E020", message, field_value.span);
                        }
                    } else {
                        self.error_coded(
                            "E020",
                            format!("Struct '{}' has no field named '{}'", name, field_name),
                            field_value.span,
                        );
                    }
                }

                for expected_field_name in struct_info.fields.keys() {
                    if !provided_fields.contains(expected_field_name) {
                        self.error_coded(
                            "E019",
                            format!(
                                "Struct literal for '{}' is missing field '{}'",
                                name, expected_field_name
                            ),
                            expr.span,
                        );
                    }
                }
            }
            _ => unreachable!("expression category mismatch"),
        }
    }
}
