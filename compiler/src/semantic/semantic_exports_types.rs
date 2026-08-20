impl SemanticAnalyzer {
    /// Build the `ModuleExports` for the module that was just analysed so it
    /// can be registered in the shared registry for downstream importers.
    ///
    /// `package_name` overrides the constructor value; pass `None` to reuse it.
    pub fn collect_module_exports(
        &self,
        module: &Module,
        package_name: Option<String>,
    ) -> ModuleExports {
        let pkg = package_name.or_else(|| self.current_package.clone());
        let mut exports = ModuleExports {
            package_name: pkg,
            ..Default::default()
        };

        for item in &module.items {
            match item {
                Item::Function(func)
                    if func.visibility == Visibility::Public
                        || func.visibility == Visibility::Internal =>
                {
                    let vis = if func.visibility == Visibility::Public {
                        ExportVisibility::Public
                    } else {
                        ExportVisibility::Internal
                    };
                    let params: Vec<Type> = func
                        .params
                        .iter()
                        .map(|p| self.type_annotation_to_type(&p.ty))
                        .collect();
                    let return_type = Self::async_task_type(
                        func.is_async,
                        self.type_annotation_to_type(&func.return_type),
                    );
                    exports.functions.insert(
                        func.name.clone(),
                        ExportedFunction {
                            params,
                            return_type,
                            visibility: vis,
                            is_async: func.is_async,
                        },
                    );
                    if !func.type_params.is_empty() {
                        exports
                            .generic_functions
                            .insert(func.name.clone(), func.clone());
                    }
                }
                Item::Static(decl)
                    if decl.visibility == Visibility::Public
                        || decl.visibility == Visibility::Internal =>
                {
                    let vis = if decl.visibility == Visibility::Public {
                        ExportVisibility::Public
                    } else {
                        ExportVisibility::Internal
                    };
                    let ty = decl
                        .ty
                        .as_ref()
                        .map(|annotation| self.type_annotation_to_type(&Some(annotation.clone())))
                        .unwrap_or_else(|| {
                            self.eval_const_expression(&decl.value)
                                .map(|value| value.ty())
                                .unwrap_or(Type::Unknown)
                        });
                    exports.statics.insert(
                        decl.name.clone(),
                        ExportedStatic {
                            ty,
                            visibility: vis,
                            qualified_name: format!("{}::{}", module.name, decl.name),
                        },
                    );
                }
                Item::Struct(s)
                    if s.visibility == Visibility::Public
                        || s.visibility == Visibility::Internal =>
                {
                    let vis = if s.visibility == Visibility::Public {
                        ExportVisibility::Public
                    } else {
                        ExportVisibility::Internal
                    };
                    let members = s.fields.iter().map(|f| f.name.clone()).collect();
                    let struct_fields: HashMap<String, crate::ast::TypeAnnotation> = s
                        .fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect();
                    exports.types.insert(
                        s.name.clone(),
                        ExportedType {
                            members,
                            visibility: vis,
                            is_enum: false,
                            struct_fields: Some(struct_fields),
                            enum_variants: None,
                            enum_struct_variants: None,
                        },
                    );
                }
                Item::Enum(e)
                    if e.visibility == Visibility::Public
                        || e.visibility == Visibility::Internal =>
                {
                    let vis = if e.visibility == Visibility::Public {
                        ExportVisibility::Public
                    } else {
                        ExportVisibility::Internal
                    };
                    let members = e.variants.iter().map(|v| v.name.clone()).collect();
                    // enum_variants: stores tuple-payload types only (None for unit and struct-data variants).
                    let enum_variants: HashMap<String, Option<Vec<crate::ast::TypeAnnotation>>> = e
                        .variants
                        .iter()
                        .map(|v| {
                            let payload = if v.struct_data.is_some() {
                                // struct-data variants are tracked separately in enum_struct_variants.
                                None
                            } else {
                                v.data.clone()
                            };
                            (v.name.clone(), payload)
                        })
                        .collect();
                    // enum_struct_variants: stores named-field lists for struct-data variants.
                    let mut enum_struct_variants: HashMap<
                        String,
                        Vec<(String, crate::ast::TypeAnnotation)>,
                    > = HashMap::new();
                    for v in &e.variants {
                        if let Some(ref fields) = v.struct_data {
                            enum_struct_variants.insert(v.name.clone(), fields.clone());
                        }
                    }
                    let enum_struct_variants_opt = if enum_struct_variants.is_empty() {
                        None
                    } else {
                        Some(enum_struct_variants)
                    };
                    exports.types.insert(
                        e.name.clone(),
                        ExportedType {
                            members,
                            visibility: vis,
                            is_enum: true,
                            struct_fields: None,
                            enum_variants: Some(enum_variants),
                            enum_struct_variants: enum_struct_variants_opt,
                        },
                    );
                }
                Item::Trait(trait_decl) =>
                {
                    let methods = trait_decl
                        .methods
                        .iter()
                        .map(|method| {
                            let mut params = Vec::new();
                            let mut self_kind = None;
                            for param in &method.params {
                                if param.is_self {
                                    self_kind = Some(if param.is_reference {
                                        ExportedSelfParamKind::Reference {
                                            mutable: param.is_mutable,
                                        }
                                    } else {
                                        ExportedSelfParamKind::Value
                                    });
                                    params.push(Type::Unknown);
                                } else {
                                    params.push(
                                        param
                                            .type_annotation
                                            .as_ref()
                                            .map(|ann| self.type_annotation_to_type(&Some(ann.clone())))
                                            .unwrap_or(Type::Unknown),
                                    );
                                }
                            }
                            let return_type = Self::async_task_type(
                                method.is_async,
                                method
                                    .return_type
                                    .as_ref()
                                    .map(|ann| self.type_annotation_to_type(&Some(ann.clone())))
                                    .unwrap_or(Type::Unit),
                            );
                            (
                                method.name.clone(),
                                ExportedTraitMethod {
                                    params,
                                    return_type,
                                    self_kind,
                                    is_async: method.is_async,
                                    has_default: method.body.is_some(),
                                },
                            )
                        })
                        .collect();
                    exports.traits.insert(
                        trait_decl.name.clone(),
                        ExportedTrait {
                            methods,
                            visibility: ExportVisibility::Public,
                        },
                    );
                }
                Item::Impl(impl_block) if impl_block.trait_name.is_none() => {
                    for method in &impl_block.methods {
                        if method.visibility != Visibility::Public
                            && method.visibility != Visibility::Internal
                        {
                            continue;
                        }

                        let vis = if method.visibility == Visibility::Public {
                            ExportVisibility::Public
                        } else {
                            ExportVisibility::Internal
                        };
                        let mut params = Vec::new();
                        let mut self_kind = None;
                        for param in &method.params {
                            if param.is_self {
                                let kind = if param.is_reference {
                                    SelfParamKind::Reference {
                                        mutable: param.is_mutable,
                                    }
                                } else {
                                    SelfParamKind::Value
                                };
                                self_kind = Some(ExportedSelfParamKind::from(kind));
                                params.push(Type::Struct {
                                    name: impl_block.type_name.clone(),
                                });
                            } else {
                                params.push(self.type_annotation_to_type(&param.type_annotation));
                            }
                        }

                        let return_type = Self::async_task_type(
                            method.is_async,
                            self.type_annotation_to_type(&method.return_type),
                        );
                        exports
                            .methods
                            .entry(impl_block.type_name.clone())
                            .or_default()
                            .insert(
                                method.name.clone(),
                                ExportedMethod {
                                    params,
                                    return_type,
                                    visibility: vis,
                                    self_kind,
                                    is_async: method.is_async,
                                },
                            );
                    }
                }
                Item::Impl(impl_block) => {
                    if let Some(trait_name) = &impl_block.trait_name {
                        exports.trait_impls.push(ExportedTraitImpl {
                            trait_name: trait_name.clone(),
                            type_name: impl_block.type_name.clone(),
                            type_args: impl_block
                                .type_args
                                .iter()
                                .map(|ann| self.type_annotation_to_type(&Some(ann.clone())))
                                .collect(),
                        });
                    }
                }
                Item::TraitImpl(trait_impl) => {
                    exports.trait_impls.push(ExportedTraitImpl {
                        trait_name: trait_impl.trait_name.clone(),
                        type_name: trait_impl.type_name.clone(),
                        type_args: trait_impl
                            .type_args
                            .iter()
                            .map(|ann| self.type_annotation_to_type(&Some(ann.clone())))
                            .collect(),
                    });
                }
                _ => {}
            }
        }

        // A trait implementation may live in a module different from the
        // module that declares the concrete type.  Export its concrete method
        // entry points as well as the coherence metadata above; otherwise a
        // downstream `value.method()` call can type-check against the trait
        // but has no `Type_method` symbol to lower or link.
        for item in &module.items {
            let (type_name, methods) = match item {
                Item::Impl(impl_block) if impl_block.trait_name.is_some() => {
                    (&impl_block.type_name, &impl_block.methods)
                }
                Item::TraitImpl(trait_impl) => (&trait_impl.type_name, &trait_impl.methods),
                _ => continue,
            };

            for method in methods {
                if method.visibility != Visibility::Public
                    && method.visibility != Visibility::Internal
                {
                    continue;
                }

                let visibility = if method.visibility == Visibility::Public {
                    ExportVisibility::Public
                } else {
                    ExportVisibility::Internal
                };
                let mut params = Vec::new();
                let mut self_kind = None;
                for param in &method.params {
                    if param.is_self {
                        let kind = if param.is_reference {
                            SelfParamKind::Reference {
                                mutable: param.is_mutable,
                            }
                        } else {
                            SelfParamKind::Value
                        };
                        self_kind = Some(ExportedSelfParamKind::from(kind));
                        params.push(Type::Struct {
                            name: type_name.clone(),
                        });
                    } else {
                        params.push(self.type_annotation_to_type(&param.type_annotation));
                    }
                }
                let return_type = Self::async_task_type(
                    method.is_async,
                    self.type_annotation_to_type(&method.return_type),
                );

                exports
                    .methods
                    .entry(type_name.clone())
                    .or_default()
                    .entry(method.name.clone())
                    .or_insert(ExportedMethod {
                        params,
                        return_type,
                        visibility,
                        self_kind,
                        is_async: method.is_async,
                    });
            }
        }

        // A public named import is also a public re-export.  Keep the
        // re-export in the registry so a downstream module can use the
        // canonical `public from support import answer` form exactly as it
        // could use a directly declared public function.
        for item in &module.items {
            let Item::Import(import) = item else {
                continue;
            };
            if !import.is_reexport {
                continue;
            }

            let module_path = import.path.join(".");
            let Some(source) = self
                .registry
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get_module(&module_path)
                .cloned()
            else {
                continue;
            };

            let names: Vec<(String, String)> = if let Some(named) = &import.names {
                named
                    .iter()
                    .map(|entry| {
                        (
                            entry.name.clone(),
                            entry.alias.clone().unwrap_or_else(|| entry.name.clone()),
                        )
                    })
                    .collect()
            } else {
                source
                    .functions
                    .iter()
                    .filter(|(_, function)| function.visibility == ExportVisibility::Public)
                    .map(|(name, _)| (name.clone(), name.clone()))
                    .chain(
                        source
                            .types
                            .iter()
                            .filter(|(_, ty)| ty.visibility == ExportVisibility::Public)
                            .map(|(name, _)| (name.clone(), name.clone())),
                    )
                    .chain(
                        source
                            .statics
                            .iter()
                            .filter(|(_, static_export)| {
                                static_export.visibility == ExportVisibility::Public
                            })
                            .map(|(name, _)| (name.clone(), name.clone())),
                    )
                    .chain(
                        source
                            .traits
                            .iter()
                            .filter(|(_, trait_export)| {
                                trait_export.visibility == ExportVisibility::Public
                            })
                            .map(|(name, _)| (name.clone(), name.clone())),
                    )
                    .collect()
            };

            for (source_name, public_name) in names {
                if let Some(function) = source.functions.get(&source_name) {
                    if function.visibility == ExportVisibility::Public {
                        let mut reexported = function.clone();
                        reexported.visibility = ExportVisibility::Public;
                        exports.functions.entry(public_name.clone()).or_insert(reexported);
                    }
                }

                if let Some(static_export) = source.statics.get(&source_name) {
                    if static_export.visibility == ExportVisibility::Public {
                        let mut reexported = static_export.clone();
                        reexported.visibility = ExportVisibility::Public;
                        exports.statics.entry(public_name.clone()).or_insert(reexported);
                    }
                }

                if let Some(ty) = source.types.get(&source_name) {
                    if ty.visibility == ExportVisibility::Public {
                        let mut reexported = ty.clone();
                        reexported.visibility = ExportVisibility::Public;
                        exports.types.entry(public_name.clone()).or_insert(reexported);

                        if let Some(methods) = source.methods.get(&source_name) {
                            let public_methods = methods
                                .iter()
                                .filter(|(_, method)| {
                                    method.visibility == ExportVisibility::Public
                                })
                                .map(|(name, method)| (name.clone(), method.clone()));
                            exports
                                .methods
                                .entry(public_name.clone())
                                .or_default()
                                .extend(public_methods);
                        }
                    }
                }

                if let Some(trait_export) = source.traits.get(&source_name) {
                    if trait_export.visibility == ExportVisibility::Public {
                        let mut reexported = trait_export.clone();
                        reexported.visibility = ExportVisibility::Public;
                        exports.traits.entry(public_name).or_insert(reexported);
                    }
                }
            }
        }

        for (type_name, methods) in &self.methods {
            // `self.methods` also contains methods imported for use inside the
            // current module. A normal `import ledger` is not a re-export, so
            // leaking those method symbols into this module's export table
            // creates downstream externals such as `Invoice_due` without
            // exporting the matching `Invoice` layout. Explicit re-exports
            // and trait implementations are handled above; only aggregate
            // types owned/exported by this module may contribute the residual
            // semantic method table here.
            if !exports.types.contains_key(type_name) {
                continue;
            }
            for (method_name, signature) in methods {
                let visibility = self
                    .method_visibility
                    .get(type_name)
                    .and_then(|entries| entries.get(method_name))
                    .copied()
                    .unwrap_or(Visibility::Private);
                if visibility != Visibility::Public && visibility != Visibility::Internal {
                    continue;
                }
                let vis = if visibility == Visibility::Public {
                    ExportVisibility::Public
                } else {
                    ExportVisibility::Internal
                };
                exports
                    .methods
                    .entry(type_name.clone())
                    .or_default()
                    .entry(method_name.clone())
                    .or_insert_with(|| ExportedMethod {
                        params: signature.params.clone(),
                        return_type: signature.return_type.clone(),
                        visibility: vis,
                        self_kind: signature.self_kind.map(ExportedSelfParamKind::from),
                        is_async: signature.is_async,
                    });
            }
        }

        exports
    }

    fn push_semantic_error(
        &mut self,
        message: impl Into<String>,
        span: Span,
        context: Option<String>,
        hint: Option<String>,
    ) {
        let mut error = SemanticError::new(message, span);
        if let Some(context) = context {
            error = error.with_context(context);
        }
        if let Some(hint) = hint {
            error = error.with_hint(hint);
        }
        self.errors.push(error);
    }

    fn push_semantic_error_coded(
        &mut self,
        code: &str,
        message: impl Into<String>,
        span: Span,
        context: Option<String>,
        hint: Option<String>,
    ) {
        let mut error = SemanticError::new(message, span).with_code(code);
        if let Some(context) = context {
            error = error.with_context(context);
        }
        if let Some(hint) = hint {
            error = error.with_hint(hint);
        }
        self.errors.push(error);
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.push_semantic_error(message, span, None, None);
    }

    fn error_coded(&mut self, code: &str, message: impl Into<String>, span: Span) {
        self.push_semantic_error_coded(code, message, span, None, None);
    }

    fn error_coded_with_hint(
        &mut self,
        code: &str,
        message: impl Into<String>,
        span: Span,
        hint: impl Into<String>,
    ) {
        self.push_semantic_error_coded(code, message, span, None, Some(hint.into()));
    }

    fn error_coded_with_details(
        &mut self,
        code: &str,
        message: impl Into<String>,
        span: Span,
        context: impl Into<String>,
        hint: impl Into<String>,
    ) {
        self.push_semantic_error_coded(
            code,
            message,
            span,
            Some(context.into()),
            Some(hint.into()),
        );
    }

    fn error_with_hint(&mut self, message: impl Into<String>, span: Span, hint: impl Into<String>) {
        self.push_semantic_error(message, span, None, Some(hint.into()));
    }

    fn error_with_context(
        &mut self,
        message: impl Into<String>,
        span: Span,
        context: impl Into<String>,
    ) {
        self.push_semantic_error(message, span, Some(context.into()), None);
    }

    fn error_with_details(
        &mut self,
        message: impl Into<String>,
        span: Span,
        context: impl Into<String>,
        hint: impl Into<String>,
    ) {
        self.push_semantic_error(message, span, Some(context.into()), Some(hint.into()));
    }

    fn has_error_at_span(&self, span: Span) -> bool {
        self.errors.iter().any(|error| error.span == span)
    }

    fn push_scope(&mut self) {
        self.symbols.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.symbols.pop();
    }

    fn push_generic_params(&mut self, params: &[crate::ast::TypeParameter]) -> bool {
        if params.is_empty() {
            return false;
        }
        let mut set = HashSet::new();
        let mut bounds_map = HashMap::new();

        for param in params {
            set.insert(param.name.clone());
            bounds_map.insert(param.name.clone(), param.bounds.clone());
        }
        self.generic_params.push(set);
        self.generic_param_bounds.push(bounds_map);
        true
    }

    fn pop_generic_params(&mut self) {
        self.generic_params.pop();
        self.generic_param_bounds.pop();
    }

    fn is_generic_param(&self, name: &str) -> bool {
        self.generic_params
            .iter()
            .rev()
            .any(|params| params.contains(name))
    }

    fn get_generic_bounds(&self, name: &str) -> Option<&Vec<String>> {
        for bounds in self.generic_param_bounds.iter().rev() {
            if let Some(list) = bounds.get(name) {
                return Some(list);
            }
        }
        None
    }

    fn trait_method_signature_for_type_param(
        &self,
        param_name: &str,
        method_name: &str,
    ) -> Option<(FunctionSignature, String)> {
        let bounds = self.get_generic_bounds(param_name)?;

        for trait_name in bounds {
            if let Some(trait_methods) = self.traits.get(trait_name) {
                if let Some(trait_method_info) = trait_methods.get(method_name) {
                    let mut params = trait_method_info.signature.params.clone();
                    if trait_method_info.signature.self_kind.is_some() && !params.is_empty() {
                        params[0] = Type::TypeParameter {
                            name: param_name.to_string(),
                        };
                    }

                    let signature = FunctionSignature {
                        params,
                        return_type: trait_method_info.signature.return_type.clone(),
                        self_kind: trait_method_info.signature.self_kind,
                        is_async: trait_method_info.signature.is_async,
                    };

                    return Some((signature, trait_name.clone()));
                }
            }
        }

        None
    }

    fn infer_type_parameter_substitutions(
        &mut self,
        param_types: &[Type],
        arguments: &[Expression],
    ) -> HashMap<String, Type> {
        let mut substitutions = HashMap::new();
        for (param_ty, arg_expr) in param_types.iter().zip(arguments.iter()) {
            let arg_ty = self.infer_expression_type(arg_expr);
            self.collect_type_parameter_substitutions(param_ty, &arg_ty, &mut substitutions);
        }
        substitutions
    }

    fn collect_type_parameter_substitutions(
        &self,
        expected: &Type,
        actual: &Type,
        substitutions: &mut HashMap<String, Type>,
    ) {
        match expected {
            Type::TypeParameter { name } => {
                substitutions
                    .entry(name.clone())
                    .or_insert_with(|| actual.clone());
            }
            Type::Array { element_type, .. } => {
                if let Type::Array {
                    element_type: actual_element,
                    ..
                } = actual
                {
                    self.collect_type_parameter_substitutions(
                        element_type,
                        actual_element,
                        substitutions,
                    );
                }
            }
            Type::Tuple { elements } => {
                if let Type::Tuple {
                    elements: actual_elements,
                } = actual
                {
                    for (lhs, rhs) in elements.iter().zip(actual_elements.iter()) {
                        self.collect_type_parameter_substitutions(lhs, rhs, substitutions);
                    }
                }
            }
            Type::Fn {
                params,
                return_type,
            } => {
                if let Type::Fn {
                    params: actual_params,
                    return_type: actual_return,
                } = actual
                {
                    for (lhs, rhs) in params.iter().zip(actual_params.iter()) {
                        self.collect_type_parameter_substitutions(lhs, rhs, substitutions);
                    }
                    self.collect_type_parameter_substitutions(
                        return_type,
                        actual_return,
                        substitutions,
                    );
                }
            }
            _ => {}
        }
    }

    fn type_satisfies_trait_bound(&self, concrete_type: &Type, trait_name: &str) -> bool {
        if trait_name == "Send" {
            return self.type_is_send(concrete_type);
        }
        if trait_name == "Sync" {
            return self.type_is_sync(concrete_type);
        }
        match concrete_type {
            Type::Struct { name } | Type::Enum { name, .. } => self
                .trait_impls
                .contains_key(&(trait_name.to_string(), name.clone())),
            Type::TypeParameter { name } => self
                .get_generic_bounds(name)
                .map(|bounds| bounds.iter().any(|bound| bound == trait_name))
                .unwrap_or(false),
            _ => false,
        }
    }

    fn validate_type_parameter_bounds(
        &mut self,
        function_name: &str,
        type_params: &[crate::ast::TypeParameter],
        substitutions: &HashMap<String, Type>,
        span: Span,
    ) {
        for param in type_params {
            let Some(concrete_type) = substitutions.get(&param.name) else {
                continue;
            };

            if matches!(concrete_type, Type::Unknown) {
                continue;
            }

            for trait_name in &param.bounds {
                if !self.type_satisfies_trait_bound(concrete_type, trait_name) {
                    if matches!(trait_name.as_str(), "Send" | "Sync") {
                        self.error_coded_with_hint(
                            "E2104",
                            format!(
                                "Type '{}' does not provide formal {} evidence required by bound '{}: {}'",
                                type_name(concrete_type),
                                trait_name,
                                param.name,
                                trait_name
                            ),
                            span,
                            format!(
                                "Add a `{}` bound/evidence for '{}' or use a type that satisfies `{}`.",
                                trait_name,
                                type_name(concrete_type),
                                trait_name
                            ),
                        );
                        continue;
                    }
                    self.error_coded_with_hint(
                        "E010",
                        format!(
                            "Type '{}' does not satisfy trait bound '{}: {}' required by function '{}'",
                            type_name(concrete_type),
                            param.name,
                            trait_name,
                            function_name
                        ),
                        span,
                        format!(
                            "Implement trait '{}' for type '{}' before passing it to '{}'.",
                            trait_name,
                            type_name(concrete_type),
                            function_name
                        ),
                    );
                }
            }
        }
    }

}
