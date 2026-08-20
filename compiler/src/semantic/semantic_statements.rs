impl SemanticAnalyzer {
    fn analyze_statement(&mut self, statement: &Statement) {
        match &statement.kind {
            StatementKind::Let(let_stmt) => {
                let declared_type = let_stmt
                    .ty
                    .as_ref()
                    .map(|ann| self.type_annotation_to_type_checked(&Some(ann.clone())));

                // Check if value expression is valid (if present) BEFORE inferring
                // its type so that qualified-path resolutions are already recorded.
                if let Some(ref value) = let_stmt.value {
                    let saved_expected = self.current_expected_type.clone();
                    self.current_expected_type = declared_type.clone();
                    self.analyze_expression(value);
                    self.current_expected_type = saved_expected;
                }

                // Infer type from value expression or annotation. An explicit annotation
                // also provides context for generic constructors such as Result::Ok(v),
                // where not every type parameter appears in the selected variant payload.
                let inferred_type = if let Some(ref value) = let_stmt.value {
                    let saved_expected = self.current_expected_type.clone();
                    self.current_expected_type = declared_type.clone();
                    let ty = self.infer_expression_type(value);
                    self.current_expected_type = saved_expected;
                    ty
                } else {
                    declared_type.clone().unwrap_or(Type::Unknown)
                };
                let binding_type = if let Some(declared_type) = declared_type {
                    if let Some(ref value) = let_stmt.value {
                        if !self.types_match(&inferred_type, &declared_type)
                            && !self.tensor_literal_matches(value, &declared_type)
                        {
                            if let Some((code, message, hint)) =
                                Self::tensor_mismatch_diagnostic(&inferred_type, &declared_type)
                            {
                                self.error_coded_with_hint(code, message, value.span, hint);
                            } else {
                                self.error_with_hint(
                                    format!(
                                        "Let binding has type {}, but {} was declared",
                                        type_name(&inferred_type),
                                        type_name(&declared_type)
                                    ),
                                    value.span,
                                    "Change the annotation or convert the initializer explicitly.",
                                );
                            }
                        }
                    }
                    declared_type
                } else {
                    inferred_type.clone()
                };

                if let Some(ann) = &let_stmt.ty {
                    if !matches!(let_stmt.pattern, Pattern::Identifier(_)) {
                        self.error_with_hint(
                            "Typed destructuring is not supported yet on `let` patterns",
                            ann.span,
                            "Bind the value to a named variable first or remove the explicit annotation.",
                        );
                    }
                }

                self.validate_pattern_against_type(&let_stmt.pattern, &binding_type, let_stmt.span);

                for name in self.collect_pattern_binding_names(&let_stmt.pattern) {
                    if self.lookup_symbol_in_current_scope(&name).is_some() {
                        self.error_coded_with_hint(
                            "E002",
                            format!("Variable '{}' is already declared in this scope", name),
                            let_stmt.span,
                            "Rename the new binding or remove the duplicate declaration.",
                        );
                    }
                }

                self.register_typed_pattern_bindings(&let_stmt.pattern, &binding_type);
                if let Pattern::Identifier(_) = &let_stmt.pattern {
                    self.record_symbol_resolution(
                        let_stmt.span,
                        SymbolInfo {
                            is_local: self.symbols.len() > 1,
                            def_span: Some(let_stmt.span),
                            ty: binding_type,
                        },
                    );
                }
            }
            StatementKind::Assignment(assign_stmt) => {
                // Analyze the target (lvalue)
                match &assign_stmt.target {
                    crate::ast::LValue::Identifier(name) => {
                        // Check if variable exists
                        if self.lookup_symbol(name).is_none() {
                            self.error_with_details(
                                format!("Variable '{}' is not defined", name),
                                assign_stmt.target_span,
                                "no binding with this name exists in the current scope",
                                format!(
                                    "Declare `{}` before using it or fix the identifier spelling.",
                                    name
                                ),
                            );
                        }
                    }
                    crate::ast::LValue::IndexAccess { array, index } => {
                        // Analyze array and index expressions
                        self.analyze_expression(array);
                        self.analyze_expression(index);

                        self.validate_static_array_index(array, index);

                        // Check that index is an integer
                        let index_type = self.infer_expression_type(index);
                        if matches!(index_type, Type::Unknown) {
                            if !self.has_error_at_span(index.span) {
                                self.error_with_hint(
                                    "Cannot determine the type of the array index",
                                    index.span,
                                    "Use an integer expression; unresolved index types are rejected before lowering.",
                                );
                            }
                        } else if !matches!(index_type, Type::Int) {
                            self.error_with_hint(
                                format!(
                                    "Array index must be an integer, found {}",
                                    type_name(&index_type)
                                ),
                                assign_stmt.target_span,
                                "Convert the index expression to `int` before indexing.",
                            );
                        }
                    }
                    crate::ast::LValue::FieldAccess { object, .. } => {
                        self.analyze_expression(object);
                    }
                }

                // Analyze the value expression
                self.analyze_expression(&assign_stmt.value);

                let target_type = match &assign_stmt.target {
                    crate::ast::LValue::Identifier(name) => self
                        .lookup_symbol(name)
                        .map(|info| info.ty.clone())
                        .unwrap_or(Type::Unknown),
                    crate::ast::LValue::IndexAccess { array, .. } => {
                        match self.infer_expression_type(array) {
                            Type::Array { element_type, .. } => *element_type,
                            Type::String => Type::Char,
                            _ => Type::Unknown,
                        }
                    }
                    crate::ast::LValue::FieldAccess { object, field } => {
                        let obj_type = self.infer_expression_type(object);
                        self.specialized_struct_context_for_type(&obj_type)
                            .and_then(|(_, info, substitutions)| {
                                info.fields.get(field.as_str()).map(|field_info| {
                                    if substitutions.is_empty() {
                                        self.type_annotation_to_type(&Some(field_info.ty.clone()))
                                    } else {
                                        self.type_annotation_to_type_with_substitutions(
                                            &field_info.ty,
                                            &substitutions,
                                        )
                                    }
                                })
                            })
                            .unwrap_or(Type::Unknown)
                    }
                };

                let value_type = self.infer_expression_type(&assign_stmt.value);

                if matches!(value_type, Type::Unknown) && !matches!(target_type, Type::Unknown) {
                    self.error_with_hint(
                        "Cannot determine the type of the assigned value",
                        assign_stmt.value.span,
                        "Add an explicit type annotation to the expression or variable.",
                    );
                }

                if !self.types_match(&value_type, &target_type) {
                    let hint = self.conversion_hint(&value_type, &target_type);
                    self.push_semantic_error_coded(
                        "E003",
                        format!(
                            "Cannot assign value of type {} to target of type {}",
                            type_name(&value_type),
                            type_name(&target_type)
                        ),
                        assign_stmt.value.span,
                        Some(format!(
                            "assignment target resolves to {} while the expression resolves to {}",
                            type_name(&target_type),
                            type_name(&value_type)
                        )),
                        hint,
                    );
                }
            }
            StatementKind::Return(ret_stmt) => {
                if self.current_function.is_none() {
                    self.error_coded_with_hint(
                        "E009",
                        "Return statement outside of function",
                        ret_stmt.span,
                        "Move this return inside a function body.",
                    );
                }

                if let Some(ref value) = ret_stmt.value {
                    self.analyze_expression(value);
                }

                self.check_return_statement(ret_stmt.value.as_ref(), ret_stmt.span);
            }
            StatementKind::Expression(expr) => {
                self.analyze_expression(expr);
            }
            StatementKind::While(while_loop) => {
                self.analyze_expression(&while_loop.condition);
                self.loop_depth += 1;
                self.analyze_block(&while_loop.body);
                self.loop_depth -= 1;
            }
            StatementKind::DoWhile(do_while_loop) => {
                self.loop_depth += 1;
                self.analyze_block(&do_while_loop.body);
                self.loop_depth -= 1;
                self.analyze_expression(&do_while_loop.condition);
            }
            StatementKind::For(for_loop) => {
                self.push_scope();

                // Analyze iterable expression
                self.analyze_expression(&for_loop.iterable);

                // Infer iterator type from iterable expression
                let iterable_type = self.infer_expression_type(&for_loop.iterable);
                let iterator_type = match iterable_type {
                    Type::Array { element_type, .. } => *element_type,
                    Type::Range => Type::Int,
                    Type::Applied { name, args } if name == "List" => args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| {
                            self.error(
                                "List<T> for-loop iterable is missing its element type".to_string(),
                                for_loop.span,
                            );
                            Type::Unknown
                        }),
                    Type::Applied { name, args } if name == "Set" => args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| {
                            self.error(
                                "Set<T> for-loop iterable is missing its element type".to_string(),
                                for_loop.span,
                            );
                            Type::Unknown
                        }),
                    Type::Applied { name, args } if name == "Iterator" => args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| {
                            self.error(
                                "Iterator<T> for-loop iterable is missing its element type"
                                    .to_string(),
                                for_loop.span,
                            );
                            Type::Unknown
                        }),
                    Type::Applied { name, args } if name == "Map" => args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| {
                            self.error(
                                "Map<K, V> for-loop iterable is missing its key type".to_string(),
                                for_loop.span,
                            );
                            Type::Unknown
                        }),
                    Type::Struct { name } if name == "List" => Type::TypeParameter {
                        name: "T".to_string(),
                    },
                    Type::Struct { name } if name.starts_with("List_") => {
                        self.type_from_mangle_part(&name["List_".len()..])
                    }
                    Type::Struct { name } if name == "Set" => Type::TypeParameter {
                        name: "T".to_string(),
                    },
                    Type::Struct { name } if name.starts_with("Set_") => {
                        self.type_from_mangle_part(&name["Set_".len()..])
                    }
                    Type::Struct { name } if name == "Iterator" => Type::TypeParameter {
                        name: "T".to_string(),
                    },
                    Type::Struct { name } if name.starts_with("Iterator_") => {
                        self.type_from_mangle_part(&name["Iterator_".len()..])
                    }
                    Type::Struct { name } if name == "Map" => Type::TypeParameter {
                        name: "K".to_string(),
                    },
                    Type::Struct { name } if name.starts_with("Map_") => {
                        let key = name["Map_".len()..].split('_').next().unwrap_or("unknown");
                        self.type_from_mangle_part(key)
                    }
                    Type::Unknown => {
                        if !self.has_error_at_span(for_loop.iterable.span) {
                            self.error_with_hint(
                                "Cannot determine the type of the for-loop iterable",
                                for_loop.iterable.span,
                                "Use a typed array, Range, List<T>, Set<T>, Map<K,V>, or Iterator<T>; unresolved expressions cannot reach lowering.",
                            );
                        }
                        Type::Unknown
                    }
                    other => {
                        self.error(
                            format!(
                                "For-loop iterable must be an array, Range, List<T>, Set<T>, Map<K,V>, or Iterator<T>, found {}",
                                type_name(&other)
                            ),
                            for_loop.span,
                        );
                        Type::Unknown
                    }
                };

                if !self.declare_symbol(
                    for_loop.iterator.clone(),
                    for_loop.span,
                    iterator_type.clone(),
                ) {
                    self.error(
                        format!(
                            "Iterator variable '{}' conflicts with existing declaration",
                            for_loop.iterator
                        ),
                        for_loop.span,
                    );
                }

                // Analyze loop body
                self.loop_depth += 1;
                self.analyze_block(&for_loop.body);
                self.loop_depth -= 1;

                self.pop_scope();
            }
            StatementKind::Loop(loop_stmt) => {
                self.loop_depth += 1;
                self.analyze_block(&loop_stmt.body);
                self.loop_depth -= 1;
            }
            StatementKind::Switch(switch_stmt) => {
                // Analyze the value being switched on
                self.analyze_expression(&switch_stmt.value);

                // Analyze each case
                for case in &switch_stmt.cases {
                    self.analyze_expression(&case.pattern);
                    self.analyze_block(&case.body);
                }

                // Analyze default case if present
                if let Some(ref default_block) = switch_stmt.default {
                    self.analyze_block(default_block);
                }
            }
            StatementKind::Break => {
                if self.loop_depth == 0 {
                    self.error_coded("E007", "Break statement outside of loop", statement.span);
                }
            }
            StatementKind::Continue => {
                if self.loop_depth == 0 {
                    self.error_coded("E008", "Continue statement outside of loop", statement.span);
                }
            }
            StatementKind::IfLet(stmt) => {
                self.analyze_expression(&stmt.value);
                let value_type = self.infer_expression_type(&stmt.value);
                self.push_scope();
                self.register_typed_pattern_bindings(&stmt.pattern, &value_type);
                for statement in &stmt.then_block.statements {
                    self.analyze_statement(statement);
                }
                self.pop_scope();
                if let Some(else_b) = &stmt.else_block {
                    self.analyze_block(else_b);
                }
            }
            StatementKind::WhileLet(stmt) => {
                self.analyze_expression(&stmt.value);
                let value_type = self.infer_expression_type(&stmt.value);
                self.loop_depth += 1;
                self.push_scope();
                self.register_typed_pattern_bindings(&stmt.pattern, &value_type);
                for statement in &stmt.body.statements {
                    self.analyze_statement(statement);
                }
                self.pop_scope();
                self.loop_depth -= 1;
            }
        }
    }

    fn generic_enum_payload_type(&self, value_type: &Type, variant: &str) -> Option<Type> {
        let (enum_name, substitutions) = match value_type {
            Type::Applied { name, args } => {
                let (params, _) = self.generic_enums.get(name)?;
                if params.len() != args.len() {
                    return None;
                }
                let substitutions = params
                    .iter()
                    .zip(args.iter())
                    .map(|(param, arg)| (param.name.clone(), arg.clone()))
                    .collect();
                (name.clone(), substitutions)
            }
            Type::Enum { name } => {
                let (base, _, substitutions) = self.specialized_enum_context(name)?;
                (base, substitutions)
            }
            _ => return None,
        };
        let enum_info = self.enum_infos.get(&enum_name)?;
        let payload = enum_info.variants.get(variant)?.data.as_ref()?.first()?;
        Some(self.type_annotation_to_type_with_substitutions(payload, &substitutions))
    }

    fn std_generic_unwrap_return(
        &mut self,
        function_name: &str,
        arguments: &[Expression],
    ) -> Option<Type> {
        let operation = function_name.rsplit('.').next().unwrap_or(function_name);
        let first_type = arguments
            .first()
            .map(|argument| self.infer_expression_type(argument));
        let resolved_payload = |payload: Option<Type>| {
            payload.filter(|ty| {
                !matches!(ty, Type::Unknown | Type::TypeParameter { .. })
            })
        };
        match operation {
            "option_unwrap" => resolved_payload(
                self.generic_enum_payload_type(first_type.as_ref()?, "Some"),
            )
                .or_else(|| Some(Type::TypeParameter { name: "T".to_string() })),
            "option_unwrap_or" => resolved_payload(
                self.generic_enum_payload_type(first_type.as_ref()?, "Some"),
            )
                .or_else(|| arguments.get(1).map(|argument| self.infer_expression_type(argument)))
                .or_else(|| Some(Type::TypeParameter { name: "T".to_string() })),
            "result_unwrap" => resolved_payload(
                self.generic_enum_payload_type(first_type.as_ref()?, "Ok"),
            )
                .or_else(|| Some(Type::TypeParameter { name: "T".to_string() })),
            "result_unwrap_or" => resolved_payload(
                self.generic_enum_payload_type(first_type.as_ref()?, "Ok"),
            )
                .or_else(|| arguments.get(1).map(|argument| self.infer_expression_type(argument)))
                .or_else(|| Some(Type::TypeParameter { name: "T".to_string() })),
            "result_unwrap_err" => resolved_payload(
                self.generic_enum_payload_type(first_type.as_ref()?, "Err"),
            )
                .or_else(|| Some(Type::TypeParameter { name: "E".to_string() })),
            _ => None,
        }
    }

    /// Built-in collection exports use unspecialized collection types as
    /// their generic signature.  User bindings retain the concrete mangled
    /// application (`List_int`, `List_string`, `Map_int_string`, ...), so the
    /// generic pattern must match only the base name and never two different
    /// concrete applications.
    fn generic_collection_pattern_matches(actual: &Type, expected: &Type) -> bool {
        let actual_name = match actual {
            Type::Struct { name } | Type::Applied { name, .. } => name.as_str(),
            _ => return false,
        };
        let expected_name = match expected {
            Type::Struct { name } | Type::Applied { name, .. } => name.as_str(),
            _ => return false,
        };

        // Two concrete applications are checked structurally by `types_match`;
        // this helper only recognizes an unspecialized builtin signature.
        if matches!(actual, Type::Applied { .. }) && matches!(expected, Type::Applied { .. }) {
            return false;
        }

        ["List", "Map", "Set", "Iterator"].iter().any(|base| {
            let actual_is_base = actual_name == *base;
            let actual_is_application = actual_name.starts_with(&format!("{base}_"));
            let expected_is_base = expected_name == *base;
            let expected_is_application = expected_name.starts_with(&format!("{base}_"));
            (actual_is_base && (expected_is_base || expected_is_application))
                || (expected_is_base && actual_is_application)
        })
    }

    fn collection_type_arguments(&self, ty: &Type) -> Option<(&'static str, Vec<Type>)> {
        if let Type::Applied { name, args } = ty {
            return match name.as_str() {
                "List" if args.len() == 1 => Some(("List", args.clone())),
                "Map" if args.len() == 2 => Some(("Map", args.clone())),
                "Set" if args.len() == 1 => Some(("Set", args.clone())),
                "Iterator" if args.len() == 1 => Some(("Iterator", args.clone())),
                _ => None,
            };
        }
        let Type::Struct { name } = ty else {
            return None;
        };
        if name == "List" {
            return Some(("List", vec![Type::Int]));
        }
        if let Some(suffix) = name.strip_prefix("List_") {
            return Some(("List", vec![self.type_from_mangle_part(suffix)]));
        }
        if name == "Map" {
            return Some(("Map", vec![Type::Int, Type::Int]));
        }
        name.strip_prefix("Map_").and_then(|suffix| {
            let (key, value) = suffix.split_once('_')?;
            Some((
                "Map",
                vec![self.type_from_mangle_part(key), self.type_from_mangle_part(value)],
            ))
        })
        .or_else(|| {
            if name == "Set" {
                return Some(("Set", vec![Type::Int]));
            }
            name.strip_prefix("Set_")
                .map(|suffix| ("Set", vec![self.type_from_mangle_part(suffix)]))
        })
        .or_else(|| {
            if name == "Iterator" {
                return Some(("Iterator", vec![Type::Int]));
            }
            name.strip_prefix("Iterator_")
                .map(|suffix| ("Iterator", vec![self.type_from_mangle_part(suffix)]))
        })
    }

    fn is_compat_collections_path(&self, qualified_name: &str) -> bool {
        if qualified_name.contains(".compat.collections.") {
            return true;
        }

        let Some((namespace, _)) = qualified_name.split_once('.') else {
            return false;
        };
        self.stdlib_namespace_aliases
            .get(namespace)
            .is_some_and(|path| path.ends_with(".compat.collections"))
    }

    fn specialize_std_collection_signature(
        &mut self,
        qualified_name: &str,
        signature: &FunctionSignature,
        arguments: &[Expression],
    ) -> FunctionSignature {
        let operation = qualified_name.rsplit('.').next().unwrap_or(qualified_name);
        let mut substitutions = HashMap::new();
        if let Some(first) = arguments.first() {
            let first_type = self.infer_expression_type(first);
            match self.collection_type_arguments(&first_type) {
                Some(("List", values)) if values.len() == 1 => {
                    substitutions.insert("T".to_string(), values[0].clone());
                }
                Some(("Map", values)) if values.len() == 2 => {
                    substitutions.insert("K".to_string(), values[0].clone());
                    substitutions.insert("V".to_string(), values[1].clone());
                }
                Some(("Set", values)) | Some(("Iterator", values))
                    if values.len() == 1 =>
                {
                    substitutions.insert("T".to_string(), values[0].clone());
                }
                _ => {}
            }
        }

        let mut specialized = signature.clone();
        specialized.params = signature
            .params
            .iter()
            .map(|ty| self.substitute_type_parameters(ty, &substitutions))
            .collect();
        specialized.return_type = self.substitute_type_parameters(
            &signature.return_type,
            &substitutions,
        );

        // Constructors have no value from which to infer T/K/V.  Use the
        // expected binding type when present and keep the legacy int ABI as
        // the deterministic default for unannotated source.
        if matches!(operation, "list_new" | "map_new" | "set_new") {
            if let Some(expected) = self.current_expected_type.clone() {
                if self.collection_type_arguments(&expected).is_some() {
                    specialized.return_type = expected;
                }
            } else if operation == "list_new" {
                specialized.return_type = Type::Applied {
                    name: "List".to_string(),
                    args: vec![Type::Int],
                };
            } else if operation == "map_new" {
                specialized.return_type = Type::Applied {
                    name: "Map".to_string(),
                    args: vec![Type::Int, Type::Int],
                };
            } else {
                specialized.return_type = Type::Applied {
                    name: "Set".to_string(),
                    args: vec![Type::Int],
                };
            }
        }

        let option_type = |_semantic: &Self, payload: Type| Type::Applied {
            name: "Option".to_string(),
            args: vec![payload],
        };
        let first_collection = arguments.first().and_then(|argument| {
            let first_type = self.infer_expression_type(argument);
            self.collection_type_arguments(&first_type)
        });
        let closure_return_type = |semantic: &mut Self| {
            arguments.get(1).and_then(|argument| {
                match semantic.infer_expression_type(argument) {
                    Type::Fn { return_type, .. } => Some(*return_type),
                    _ => None,
                }
            })
        };

        match operation {
            "iter" if qualified_name == "std.range.iter" => {
                specialized.return_type = Type::Applied {
                    name: "Iterator".to_string(),
                    args: vec![Type::Int],
                };
            }
            "list_iter" | "set_iter" => {
                if let Some((_, values)) = first_collection.as_ref() {
                    if let Some(element) = values.first() {
                        specialized.return_type = Type::Applied {
                            name: "Iterator".to_string(),
                            args: vec![element.clone()],
                        };
                    }
                }
            }
            "map_iter" => {
                if let Some(("Map", values)) = first_collection.as_ref() {
                    if let Some(key) = values.first() {
                        specialized.return_type = Type::Applied {
                            name: "Iterator".to_string(),
                            args: vec![key.clone()],
                        };
                    }
                }
            }
            "list_get"
            | "list_pop"
            | "list_pop_front"
            | "list_remove_at"
            | "set_get"
            | "iterator_next" => {
                if let Some((_, values)) = first_collection.as_ref() {
                    if let Some(element) = values.first() {
                        let compatibility = self.is_compat_collections_path(qualified_name);
                        specialized.return_type = if compatibility {
                            element.clone()
                        } else {
                            option_type(self, element.clone())
                        };
                    }
                }
            }
            "map_get" | "map_remove" => {
                if let Some(("Map", values)) = first_collection.as_ref() {
                    if let Some(value) = values.get(1) {
                        let compatibility = self.is_compat_collections_path(qualified_name);
                        specialized.return_type = if compatibility {
                            value.clone()
                        } else {
                            option_type(self, value.clone())
                        };
                    }
                }
            }
            "option_map" => {
                let first_type = arguments.first().map(|argument| {
                    self.infer_expression_type(argument)
                });
                if let (Some(input), Some(mapped)) =
                    (first_type.as_ref(), closure_return_type(self))
                {
                    if self
                        .generic_enum_payload_type(input, "Some")
                        .is_some()
                        && !matches!(mapped, Type::Unknown | Type::TypeParameter { .. })
                    {
                        specialized.return_type = Type::Applied {
                            name: "Option".to_string(),
                            args: vec![mapped],
                        };
                    }
                }
            }
            "result_map" => {
                let first_type = arguments.first().map(|argument| {
                    self.infer_expression_type(argument)
                });
                if let (Some(input), Some(mapped)) =
                    (first_type.as_ref(), closure_return_type(self))
                {
                    if let (Some(_value), Some(error)) = (
                        self.generic_enum_payload_type(input, "Ok"),
                        self.generic_enum_payload_type(input, "Err"),
                    ) {
                        specialized.return_type = Type::Applied {
                            name: "Result".to_string(),
                            args: vec![mapped, error],
                        };
                    }
                }
            }
            "result_map_err" => {
                let first_type = arguments.first().map(|argument| {
                    self.infer_expression_type(argument)
                });
                if let (Some(input), Some(mapped)) =
                    (first_type.as_ref(), closure_return_type(self))
                {
                    if let (Some(value), Some(_error)) = (
                        self.generic_enum_payload_type(input, "Ok"),
                        self.generic_enum_payload_type(input, "Err"),
                    ) {
                        specialized.return_type = Type::Applied {
                            name: "Result".to_string(),
                            args: vec![value, mapped],
                        };
                    }
                }
            }
            "list_get_option"
            | "list_pop_option"
            | "list_pop_front_option"
            | "list_remove_at_option"
            | "map_get_option"
            | "map_remove_option" => {
                if let Some((collection, values)) = first_collection.as_ref() {
                    let payload = if *collection == "Map" {
                        values.get(1)
                    } else {
                        values.first()
                    };
                    if let Some(payload) = payload {
                        specialized.return_type = option_type(self, payload.clone());
                    }
                }
            }
            _ => {}
        }
        specialized
    }

    fn constant_integer_expression(expr: &Expression) -> Option<i128> {
        match &expr.kind {
            ExpressionKind::NumberLiteral(value) => value.replace('_', "").parse().ok(),
            ExpressionKind::Unary {
                operator: crate::ast::UnaryOperator::Negate,
                operand,
            } => Self::constant_integer_expression(operand).map(|value| -value),
            ExpressionKind::Grouping(inner) => Self::constant_integer_expression(inner),
            _ => None,
        }
    }

    fn validate_static_array_index(&mut self, array: &Expression, index: &Expression) {
        let array_type = self.infer_expression_type(array);
        let Type::Array {
            size: Some(length),
            ..
        } = array_type
        else {
            return;
        };

        let Some(index_value) = Self::constant_integer_expression(index) else {
            return;
        };
        if index_value < 0 || index_value >= length as i128 {
            self.error_coded_with_hint(
                "E004",
                format!(
                    "Array index {} out of bounds (array has {} elements)",
                    index_value, length
                ),
                index.span,
                format!(
                    "Use an index in the range 0..{} or validate a dynamic index before access.",
                    length
                ),
            );
        }
    }

}
