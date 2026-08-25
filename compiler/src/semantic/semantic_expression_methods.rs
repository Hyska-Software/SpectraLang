use super::*;

impl SemanticAnalyzer {
    pub(crate) fn analyze_expression_method(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::MethodCall {
                object,
                method_name,
                arguments,
                type_name: _,
            } => {
                let call_ty = self.infer_expression_type(expr);
                self.symbol_resolutions.insert(
                    expr.span,
                    SymbolInfo {
                        is_local: false,
                        def_span: None,
                        ty: call_ty,
                    },
                );

                // Analisar objeto
                self.analyze_expression(object);

                // Analisar argumentos
                for arg in arguments {
                    self.analyze_expression(arg);
                }

                // Resource-release classification (E034): tensor.free(x) /
                // tensor.free_all() release through the namespace receiver.
                self.uaf_after_method_call_analysis(method_name, arguments, expr.span);

                self.validate_static_tensor_method_call(object, method_name, arguments, expr.span);
                self.validate_static_ml_method_call(object, method_name, arguments, expr.span);

                if let Some(path) = namespace_path(object) {
                    let qualified_name = format!("{}.{}", path, method_name);
                    let exports_cloned: Option<ModuleExports> = self
                        .registry
                        .read()
                        .unwrap_or_else(|p| p.into_inner())
                        .get_module(&path)
                        .cloned();
                    let qualified_sig =
                        self.functions.get(&qualified_name).cloned().or_else(|| {
                            exports_cloned.as_ref().and_then(|exports| {
                                exports.functions.get(method_name.as_str()).map(|func| {
                                    FunctionSignature {
                                        params: func.params.clone(),
                                        return_type: func.return_type.clone(),
                                        self_kind: None,
                                        is_async: func.is_async,
                                    }
                                })
                            })
                        });
                    let qualified_sig = qualified_sig.map(|signature| {
                        self.specialize_std_collection_signature(
                            &qualified_name,
                            &signature,
                            arguments,
                        )
                    });

                    if let Some(signature) = qualified_sig {
                        if arguments.len() != signature.params.len() {
                            self.error(
                                format!(
                                    "Function '{}' expects {} arguments, but {} were provided",
                                    qualified_name,
                                    signature.params.len(),
                                    arguments.len()
                                ),
                                expr.span,
                            );
                        } else {
                            for (i, (arg, expected_type)) in
                                arguments.iter().zip(signature.params.iter()).enumerate()
                            {
                                let arg_type = self.infer_expression_type(arg);
                                if matches!(arg_type, Type::Unknown) {
                                    self.error_with_hint(
                                        format!(
                                            "Argument {} of function '{}' has an unknown or uninferrable type",
                                            i + 1,
                                            qualified_name,
                                        ),
                                        arg.span,
                                        "Add an explicit type annotation to resolve the argument type.",
                                    );
                                } else if *expected_type != Type::Unknown
                                    && !self.generic_argument_types_match(&arg_type, expected_type)
                                {
                                    self.error(
                                        format!(
                                            "Argument {} of function '{}' has type {}, expected {}",
                                            i + 1,
                                            qualified_name,
                                            type_name(&arg_type),
                                            type_name(expected_type)
                                        ),
                                        arg.span,
                                    );
                                }
                            }
                        }

                        self.record_expression_type(expr);
                        return;
                    }

                    if let Some(exports) = exports_cloned.as_ref() {
                        self.report_unknown_qualified_member(
                            &path,
                            method_name,
                            exports,
                            expr.span,
                        );
                        self.record_expression_type(expr);
                        return;
                    }

                    if self.module_namespaces.contains(path.as_str()) {
                        let export_names = self.namespace_export_names(&path);
                        self.report_unknown_qualified_member_names(
                            &path,
                            method_name,
                            export_names,
                            expr.span,
                        );
                        self.record_expression_type(expr);
                        return;
                    }
                }

                // Verificar se método existe para o tipo do objeto
                let obj_type = self.infer_expression_type(object);

                if let Type::TypeParameter { name } = &obj_type {
                    match self.trait_method_signature_for_type_param(name, method_name) {
                        Some((signature, _trait_name)) => {
                            self.validate_method_call_signature(
                                method_name,
                                &signature,
                                &obj_type,
                                arguments,
                                expr.span,
                            );
                        }
                        None => match self.get_generic_bounds(name) {
                            Some(bounds) if !bounds.is_empty() => {
                                self.error(
                                        format!(
                                            "Method '{}' is not provided by trait bounds ({}) on type parameter '{}'",
                                            method_name,
                                            bounds.join(", "),
                                            name
                                        ),
                                        expr.span,
                                    );
                            }
                            _ => {
                                self.error(
                                        format!(
                                            "Type parameter '{}' must be constrained by a trait that defines method '{}'",
                                            name, method_name
                                        ),
                                        expr.span,
                                    );
                            }
                        },
                    }
                    self.record_expression_type(expr);
                    return;
                }

                // Extrair nome do tipo
                let type_name = match &obj_type {
                    Type::Struct { name }
                    | Type::Enum { name, .. }
                    | Type::Applied { name, .. } => self
                        .nominal_lookup_name(&obj_type)
                        .or_else(|| Some(name.clone())),
                    Type::Unknown => None,
                    // dyn Trait method calls are valid — dispatch is dynamic
                    Type::DynTrait { .. } => None,
                    _ => {
                        self.error_coded(
                            "E017",
                            format!(
                                "Cannot call method '{}' on type '{:?}'",
                                method_name, obj_type
                            ),
                            expr.span,
                        );
                        None
                    }
                };

                // Se conseguimos extrair o tipo, verificar se método existe
                if let Some(type_name) = &type_name {
                    let method_signature = self
                        .methods
                        .get(type_name)
                        .and_then(|methods| methods.get(method_name).cloned())
                        .or_else(|| {
                            // R-211: instantiated generic struct (e.g. "Par_int").
                            // Resolve the template impl's signature and substitute
                            // the type parameters with the concrete type arguments.
                            self.instantiated_method_signature(type_name, method_name)
                        });

                    let signature = if let Some(sig) = method_signature {
                        Some(sig)
                    } else {
                        let mut found_signature = None;
                        for (trait_name, impl_type) in self.trait_impls.keys() {
                            if impl_type == type_name {
                                if let Some(trait_methods) = self.traits.get(trait_name) {
                                    if let Some(trait_method_info) = trait_methods.get(method_name)
                                    {
                                        if trait_method_info.has_default {
                                            let mut params =
                                                trait_method_info.signature.params.clone();
                                            if trait_method_info.signature.self_kind.is_some()
                                                && !params.is_empty()
                                            {
                                                params[0] = Type::Struct {
                                                    name: type_name.clone(),
                                                };
                                            }

                                            found_signature = Some(FunctionSignature {
                                                params,
                                                return_type: trait_method_info
                                                    .signature
                                                    .return_type
                                                    .clone(),
                                                self_kind: trait_method_info.signature.self_kind,
                                                is_async: trait_method_info.signature.is_async,
                                            });
                                            break;
                                        }
                                    }
                                }
                            }
                            if found_signature.is_some() {
                                break;
                            }
                        }
                        found_signature
                    };

                    if let Some(signature) = signature {
                        // Enforce method visibility
                        let method_vis = self
                            .method_visibility
                            .get(type_name)
                            .and_then(|mv| mv.get(method_name))
                            .copied()
                            .unwrap_or(Visibility::Public); // default pub for trait-default methods
                        let accessible = match method_vis {
                            Visibility::Public => true,
                            Visibility::Internal => true,
                            // Private = module-local: accessible from anywhere within this module.
                            // Cross-module visibility is enforced by not exporting Private methods.
                            Visibility::Private => true,
                        };
                        if !accessible {
                            self.error(
                                format!(
                                    "Method '{}' of '{}' is private and cannot be called outside its impl block",
                                    method_name, type_name
                                ),
                                expr.span,
                            );
                        }

                        self.validate_method_call_signature(
                            method_name,
                            &signature,
                            &obj_type,
                            arguments,
                            expr.span,
                        );

                        let def_span = self
                            .method_definitions
                            .get(type_name)
                            .and_then(|methods| methods.get(method_name))
                            .copied();
                        if let Some(existing) = self.symbol_resolutions.get_mut(&expr.span) {
                            existing.def_span = def_span;
                        }
                    } else {
                        self.error_coded(
                            "E017",
                            self.missing_method_diagnostic(type_name, method_name),
                            expr.span,
                        );
                    }
                }
            }
            _ => unreachable!("expression category mismatch"),
        }
    }
}
