use super::*;

impl SemanticAnalyzer {
    pub(crate) fn predeclare_trait_impl(&mut self, trait_name: &str, type_name: &str) {
        if self.traits.contains_key(trait_name) {
            self.trait_impls
                .insert((trait_name.to_string(), type_name.to_string()), true);
        }
    }

    pub(crate) fn analyze_trait_impl(&mut self, trait_impl: &crate::ast::TraitImpl) {
        let derived_impl = crate::ast::ImplBlock {
            type_name: trait_impl.type_name.clone(),
            module_path: None,
            trait_name: Some(trait_impl.trait_name.clone()),
            methods: trait_impl.methods.clone(),
            span: trait_impl.span,
            type_args: trait_impl.type_args.clone(),
            type_params: trait_impl.type_params.clone(),
        };

        self.analyze_impl_block(&derived_impl);
    }

    pub(crate) fn analyze_impl_block(&mut self, impl_block: &crate::ast::ImplBlock) {
        let type_param_info = self
            .generic_structs
            .get(&impl_block.type_name)
            .map(|(params, _)| params.clone())
            .or_else(|| {
                self.generic_enums
                    .get(&impl_block.type_name)
                    .map(|(params, _)| params.clone())
            });
        let pushed_generics = if let Some(ref params) = type_param_info {
            self.push_generic_params(params)
        } else {
            false
        };
        let pushed_impl_generics = if !impl_block.type_params.is_empty() {
            self.push_generic_params(&impl_block.type_params)
        } else {
            false
        };

        // Preserve the public contract of module-qualified inherent impls:
        // the target must resolve to an exported type in the named module.
        // The parser used to collect this prefix and then discard it, which
        // made `impl missing::Foreign { ... }` silently compile.
        if impl_block.trait_name.is_none() {
            if let Some(module_path) = impl_block.module_path.as_deref() {
                let registry_path = module_path.replace("::", ".");
                let exports = self
                    .registry
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .get_module(&registry_path)
                    .cloned();

                match exports {
                    None => self.error_coded_with_hint(
                        "E027",
                        format!(
                            "Qualified impl target module '{}' is not defined",
                            module_path
                        ),
                        impl_block.span,
                        format!(
                            "Import or declare a module exporting '{}', then use `impl {}::{} {{ ... }}`.",
                            impl_block.type_name, module_path, impl_block.type_name
                        ),
                    ),
                    Some(exports) if !exports.types.contains_key(&impl_block.type_name) => {
                        self.error_coded_with_hint(
                            "E027",
                            format!(
                                "Module '{}' does not export impl target type '{}'",
                                module_path, impl_block.type_name
                            ),
                            impl_block.span,
                            format!(
                                "Use an exported struct or enum from '{}'; available types: {}",
                                module_path,
                                exports
                                    .types
                                    .keys()
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        );
                    }
                    Some(_) => {}
                }
            }
        }

        // Validate impl type arguments against the target type's type parameters.
        // For trait impls the type arguments belong to the trait and are validated
        // in validate_trait_impl (R-213).
        if !impl_block.type_args.is_empty() && impl_block.trait_name.is_none() {
            match &type_param_info {
                Some(params) => {
                    if impl_block.type_args.len() != params.len() {
                        self.error_coded(
                            "E025",
                            format!(
                                "Impl for '{}' provides {} type argument(s), but the type declares {} type parameter(s)",
                                impl_block.type_name,
                                impl_block.type_args.len(),
                                params.len()
                            ),
                            impl_block.span,
                        );
                    } else {
                        // Template form: each impl type argument must name one of the
                        // type parameters (e.g. `impl Par<T>`). Concrete arguments
                        // (e.g. `impl Par<int>`) are rejected here; they are planned
                        // with per-instantiation impls in a later item.
                        for arg in &impl_block.type_args {
                            let is_type_param = match &arg.kind {
                                crate::ast::TypeAnnotationKind::Simple { segments } => {
                                    segments.len() == 1
                                        && params.iter().any(|p| p.name == segments[0])
                                }
                                _ => false,
                            };
                            if !is_type_param {
                                self.error_coded(
                                    "E025",
                                    format!(
                                        "Impl type argument '{:?}' for '{}' must be one of the type parameters: {}",
                                        arg.kind,
                                        impl_block.type_name,
                                        params
                                            .iter()
                                            .map(|p| p.name.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    ),
                                    arg.span,
                                );
                            }
                        }
                    }
                }
                None => {
                    self.error_coded(
                        "E025",
                        format!(
                            "Type '{}' is not generic and cannot be used with impl type arguments",
                            impl_block.type_name
                        ),
                        impl_block.span,
                    );
                }
            }
        }

        // Se for impl Trait for Type, validar que implementa todos os métodos
        if let Some(ref trait_name) = impl_block.trait_name {
            // Special handling for Drop: validate the `drop(&mut self)` signature and
            // mark the type as droppable so the midend can emit destructor calls.
            if trait_name == "Drop" {
                let has_drop_method = impl_block.methods.iter().any(|m| {
                    m.name == "drop"
                        && m.params
                            .first()
                            .map(|p| p.is_self && p.is_reference && p.is_mutable)
                            .unwrap_or(false)
                });
                if !has_drop_method {
                    self.error(
                        format!(
                            "impl Drop for '{}' must define a method `func drop(&mut self)`",
                            impl_block.type_name
                        ),
                        impl_block.span,
                    );
                } else {
                    self.drop_types.insert(impl_block.type_name.clone());
                }
            }

            self.validate_trait_impl(impl_block, trait_name);

            // Copiar métodos padrão do trait para o tipo
            self.copy_default_trait_methods(trait_name, &impl_block.type_name, impl_block);
        }

        // Fase 1: Coletar todas as assinaturas dos métodos
        for method in &impl_block.methods {
            // Extrair tipos dos parâmetros
            let mut param_types = Vec::new();
            let mut self_kind = None;
            let mut seen_regular_param = false;
            for param in &method.params {
                if param.is_self {
                    // self parameter - tipo � o do impl block
                    if seen_regular_param {
                        self.error_coded(
                            "E024",
                            format!(
                                "Method '{}' declares 'self' after other parameters; 'self' must be the first parameter",
                                method.name
                            ),
                            param.span,
                        );
                    }
                    if self_kind.is_some() {
                        self.error_coded(
                            "E014",
                            format!(
                                "Method '{}' declares more than one self parameter",
                                method.name
                            ),
                            param.span,
                        );
                    }

                    self_kind = Some(if param.is_reference {
                        SelfParamKind::Reference {
                            mutable: param.is_mutable,
                        }
                    } else {
                        SelfParamKind::Value
                    });
                    param_types.push(Type::Struct {
                        name: impl_block.type_name.clone(),
                    });
                } else {
                    seen_regular_param = true;
                    let param_type = self.type_annotation_to_type(&param.type_annotation);
                    param_types.push(param_type);
                }
            }

            let return_type = Self::async_task_type(
                method.is_async,
                self.type_annotation_to_type(&method.return_type),
            );

            // Registrar método
            let signature = FunctionSignature {
                params: param_types,
                return_type,
                self_kind,
                is_async: method.is_async,
            };

            let type_methods = self
                .methods
                .entry(impl_block.type_name.clone())
                .or_default();
            self.method_definitions
                .entry(impl_block.type_name.clone())
                .or_default()
                .insert(method.name.clone(), method.span);
            // Track per-method visibility
            self.method_visibility
                .entry(impl_block.type_name.clone())
                .or_default()
                .insert(method.name.clone(), method.visibility);

            if type_methods
                .insert(method.name.clone(), signature)
                .is_some()
            {
                self.error_coded(
                    "E013",
                    format!(
                        "Method '{}' is already defined for type '{}'",
                        method.name, impl_block.type_name
                    ),
                    method.span,
                );
            }
        }

        // Fase 2: Analisar corpos dos métodos
        for method in &impl_block.methods {
            self.current_function = Some(format!("{}::{}", impl_block.type_name, method.name));
            let expected_return = self.type_annotation_to_type_checked(&method.return_type);
            let previous_return = self.current_return_type.replace(expected_return.clone());
            if method.is_async {
                self.async_context_depth += 1;
            }
            self.push_scope();

            // Declarar parâmetros no escopo
            for param in &method.params {
                let param_type = if param.is_self {
                    Type::Struct {
                        name: impl_block.type_name.clone(),
                    }
                } else {
                    self.type_annotation_to_type_checked(&param.type_annotation)
                };

                if !self.declare_symbol(param.name.clone(), param.span, param_type) {
                    self.error(
                        format!("Parameter '{}' is already declared", param.name),
                        param.span,
                    );
                }
            }

            self.analyze_block(&method.body);
            self.validate_function_block_return(&method.body, &expected_return, method.span);

            self.pop_scope();
            if method.is_async {
                self.async_context_depth = self.async_context_depth.saturating_sub(1);
            }
            self.current_function = None;
            self.current_return_type = previous_return;
        }

        if pushed_generics {
            self.pop_generic_params();
        }
        if pushed_impl_generics {
            self.pop_generic_params();
        }
    }

    /// Analisa declara��o de trait e registra assinaturas dos m�todos
    pub(crate) fn analyze_trait_declaration(&mut self, trait_decl: &crate::ast::TraitDeclaration) {
        let mut trait_methods = HashMap::new();
        let mut signature_map = HashMap::new();

        // R-213: register the trait's generic type parameters (trait Container<T>).
        if !trait_decl.type_params.is_empty() {
            self.trait_type_params
                .insert(trait_decl.name.clone(), trait_decl.type_params.clone());
            self.push_generic_params(&trait_decl.type_params);
        }

        // First, inherit methods from parent traits
        for parent_trait_name in &trait_decl.parent_traits {
            if let Some(parent_methods) = self.traits.get(parent_trait_name).cloned() {
                // Add all parent methods to this trait
                for (method_name, method_signature) in parent_methods {
                    trait_methods.insert(method_name, method_signature);
                }
            } else {
                self.error_coded(
                    "E015",
                    format!(
                        "Parent trait '{}' is not defined. Traits must be declared before being used as parent traits.",
                        parent_trait_name
                    ),
                    trait_decl.span,
                );
            }

            if let Some(parent_signatures) = self.trait_signatures.get(parent_trait_name).cloned() {
                for (method_name, signature) in parent_signatures {
                    signature_map.insert(method_name, signature);
                }
            }
        }

        // Then add this trait's own methods (can override inherited methods)
        for method in &trait_decl.methods {
            // Converter par�metros para Type
            let mut param_types = Vec::new();
            let mut self_kind = None;
            let mut parameter_infos = Vec::new();
            let mut seen_regular_param = false;
            for param in &method.params {
                if param.is_self {
                    if seen_regular_param {
                        self.error_coded(
                            "E024",
                            format!(
                                "Trait method '{}' declares 'self' after other parameters; 'self' must be the first parameter",
                                method.name
                            ),
                            param.span,
                        );
                    }
                    if self_kind.is_some() {
                        self.error_coded(
                            "E014",
                            format!(
                                "Trait method '{}' declares more than one self parameter",
                                method.name
                            ),
                            param.span,
                        );
                    }

                    self_kind = Some(if param.is_reference {
                        SelfParamKind::Reference {
                            mutable: param.is_mutable,
                        }
                    } else {
                        SelfParamKind::Value
                    });
                    // self em trait é genérico - será o tipo que implementa o trait
                    param_types.push(Type::Unknown);
                    parameter_infos.push(ParameterInfo {
                        is_self: true,
                        is_reference: param.is_reference,
                        is_mutable: param.is_mutable,
                        ty: None,
                    });
                } else {
                    seen_regular_param = true;
                    let param_type = self.type_annotation_to_type(&param.type_annotation);
                    param_types.push(param_type);
                    parameter_infos.push(ParameterInfo {
                        is_self: false,
                        is_reference: false,
                        is_mutable: false,
                        ty: Self::option_annotation_to_pattern(&param.type_annotation),
                    });
                }
            }

            let return_type = Self::async_task_type(
                method.is_async,
                self.type_annotation_to_type(&method.return_type),
            );
            let return_pattern = Self::option_annotation_to_pattern(&method.return_type);
            self.validate_async_trait_method_object_safety(&trait_decl.name, method, self_kind);

            let signature = FunctionSignature {
                params: param_types,
                return_type,
                self_kind,
                is_async: method.is_async,
            };

            let method_info = TraitMethodInfo {
                signature,
                has_default: method.body.is_some(), // Has default if body is present
                default_body: method.body.clone(),  // Clone the body if present
            };

            if trait_methods
                .insert(method.name.clone(), method_info)
                .is_some()
            {
                self.error(
                    format!(
                        "Method '{}' is already declared in trait '{}'",
                        method.name, trait_decl.name
                    ),
                    method.span,
                );
            }

            signature_map.insert(
                method.name.clone(),
                TraitMethodSignature {
                    params: parameter_infos,
                    return_type: return_pattern,
                    has_default_body: method.body.is_some(),
                    is_async: method.is_async,
                },
            );
        }

        // Registrar trait com suas assinaturas
        if self
            .traits
            .insert(trait_decl.name.clone(), trait_methods)
            .is_some()
        {
            self.error(
                format!("Trait '{}' is already defined", trait_decl.name),
                trait_decl.span,
            );
        }

        self.trait_signatures
            .insert(trait_decl.name.clone(), signature_map);
    }

    /// Valida que um impl Trait for Type implementa todos os métodos do trait
    fn validate_trait_impl(&mut self, impl_block: &crate::ast::ImplBlock, trait_name: &str) {
        // Verificar se o trait existe e clonar para evitar borrow conflicts
        const BUILTIN_OP_TRAITS: &[&str] =
            &["Add", "Sub", "Mul", "Div", "Rem", "Eq", "Ord", "Drop"];
        let trait_methods = match self.traits.get(trait_name).cloned() {
            Some(methods) => methods,
            None => {
                if BUILTIN_OP_TRAITS.contains(&trait_name) {
                    // Builtin operator/lifecycle trait — not user-declared, register impl
                    self.trait_impls
                        .insert((trait_name.to_string(), impl_block.type_name.clone()), true);
                    return;
                }
                self.error_coded(
                    "E012",
                    format!("Trait '{}' is not defined", trait_name),
                    impl_block.span,
                );
                return;
            }
        };

        let trait_signature_map = self
            .trait_signatures
            .get(trait_name)
            .cloned()
            .unwrap_or_default();

        // R-213: generic traits (trait Container<T>) — resolve the concrete type
        // arguments from the impl (`impl Container<int> for X`) and substitute
        // them into the trait method signatures before validation.
        let trait_params = self
            .trait_type_params
            .get(trait_name)
            .cloned()
            .unwrap_or_default();
        let trait_concrete_args: Vec<Type> = if trait_params.is_empty() {
            Vec::new()
        } else {
            if impl_block.type_args.len() != trait_params.len() {
                self.error_coded(
                    "E025",
                    format!(
                        "Impl of generic trait '{}' provides {} type argument(s), but the trait declares {} type parameter(s)",
                        trait_name,
                        impl_block.type_args.len(),
                        trait_params.len()
                    ),
                    impl_block.span,
                );
            }
            impl_block
                .type_args
                .iter()
                .map(|ann| self.type_annotation_to_type(&Some(ann.clone())))
                .collect()
        };

        // Coletar métodos implementados
        let mut implemented_methods = HashMap::new();
        for method in &impl_block.methods {
            // Converter parâmetros para Type
            let mut param_types = Vec::new();
            let mut self_kind = None;
            let mut seen_regular_param = false;
            for param in &method.params {
                if param.is_self {
                    if seen_regular_param {
                        self.error_coded(
                            "E024",
                            format!(
                                "Method '{}' declares 'self' after other parameters; 'self' must be the first parameter",
                                method.name
                            ),
                            param.span,
                        );
                    }
                    if self_kind.is_some() {
                        self.error_coded(
                            "E014",
                            format!(
                                "Method '{}' declares more than one self parameter",
                                method.name
                            ),
                            param.span,
                        );
                    }

                    self_kind = Some(if param.is_reference {
                        SelfParamKind::Reference {
                            mutable: param.is_mutable,
                        }
                    } else {
                        SelfParamKind::Value
                    });
                    param_types.push(Type::Struct {
                        name: impl_block.type_name.clone(),
                    });
                } else {
                    seen_regular_param = true;
                    let param_type = self.type_annotation_to_type(&param.type_annotation);
                    param_types.push(param_type);
                }
            }

            let return_type = Self::async_task_type(
                method.is_async,
                self.type_annotation_to_type(&method.return_type),
            );

            let signature = FunctionSignature {
                params: param_types,
                return_type,
                self_kind,
                is_async: method.is_async,
            };

            implemented_methods.insert(method.name.clone(), (signature, method.span));
        }

        // Verificar que todos os métodos do trait foram implementados
        for (trait_method_name, trait_method_info) in &trait_methods {
            let expected_signature_repr = trait_signature_map
                .get(trait_method_name)
                .map(|signature| Self::format_trait_signature(trait_method_name, signature));

            match implemented_methods.get(trait_method_name) {
                Some((impl_signature, _span)) => {
                    // Verificar que as assinaturas correspondem
                    // Primeiro parâmetro do trait é Unknown (self genérico), então pulamos
                    // Mas apenas se houver parâmetros (métodos estáticos não têm self)
                    let trait_has_self = trait_method_info.signature.self_kind.is_some();
                    let impl_has_self = impl_signature.self_kind.is_some();

                    if trait_method_info.signature.self_kind != impl_signature.self_kind {
                        self.error(
                            format!(
                                "Method '{}' has incompatible self receiver between trait and implementation",
                                trait_method_name
                            ),
                            impl_block.span,
                        );
                    }

                    if trait_method_info.signature.is_async != impl_signature.is_async {
                        self.error(
                            format!(
                                "Method '{}' has incompatible async marker between trait and implementation",
                                trait_method_name
                            ),
                            impl_block.span,
                        );
                    }

                    let trait_params =
                        if trait_has_self && !trait_method_info.signature.params.is_empty() {
                            &trait_method_info.signature.params[1..]
                        } else {
                            &trait_method_info.signature.params[..]
                        };

                    // R-213: substitute the trait's type parameters with the impl's
                    // concrete type arguments before comparing signatures.
                    let trait_type_param_defs = self
                        .trait_type_params
                        .get(trait_name)
                        .cloned()
                        .unwrap_or_default();
                    let substituted_trait_params: Vec<Type> = trait_params
                        .iter()
                        .map(|t| {
                            self.substitute_generic_types(
                                t,
                                &trait_type_param_defs,
                                &trait_concrete_args,
                            )
                        })
                        .collect();
                    let substituted_trait_return = self.substitute_generic_types(
                        &trait_method_info.signature.return_type,
                        &trait_type_param_defs,
                        &trait_concrete_args,
                    );

                    let impl_params = if impl_has_self && !impl_signature.params.is_empty() {
                        &impl_signature.params[1..]
                    } else {
                        &impl_signature.params[..]
                    };

                    if trait_params.len() != impl_params.len() {
                        let mut message = format!(
                            "Method '{}' has wrong number of parameters. Expected {}, found {}",
                            trait_method_name,
                            trait_params.len(),
                            impl_params.len()
                        );

                        if let Some(signature_repr) = &expected_signature_repr {
                            message.push_str(&format!(". Expected {}", signature_repr));
                        }

                        self.error_coded("E023", message, impl_block.span);
                        continue;
                    }

                    // Verificar tipos dos parâmetros
                    for (i, (trait_param, impl_param)) in substituted_trait_params
                        .iter()
                        .zip(impl_params.iter())
                        .enumerate()
                    {
                        if !self.generic_argument_types_match(impl_param, trait_param) {
                            let mut message = format!(
                                "Method '{}' parameter {} has wrong type. Expected {:?}, found {:?}",
                                trait_method_name,
                                i + 1,
                                trait_param,
                                impl_param
                            );

                            if let Some(signature_repr) = &expected_signature_repr {
                                message.push_str(&format!(" (expected {})", signature_repr));
                            }

                            self.error_coded("E023", message, impl_block.span);
                        }
                    }

                    // Verificar tipo de retorno
                    if !self.generic_argument_types_match(
                        &impl_signature.return_type,
                        &substituted_trait_return,
                    ) {
                        let mut message = format!(
                            "Method '{}' has wrong return type. Expected {:?}, found {:?}",
                            trait_method_name, substituted_trait_return, impl_signature.return_type
                        );

                        if let Some(signature_repr) = &expected_signature_repr {
                            message.push_str(&format!(" (expected {})", signature_repr));
                        }

                        self.error_coded("E023", message, impl_block.span);
                    }
                }
                None => {
                    // Método não implementado - OK se tem default, erro caso contrário
                    let requires_impl = trait_signature_map
                        .get(trait_method_name)
                        .map(|signature| !signature.has_default_body)
                        .unwrap_or(!trait_method_info.has_default);

                    if requires_impl {
                        let message = if let Some(signature_repr) =
                            trait_signature_map.get(trait_method_name).map(|signature| {
                                Self::format_trait_signature(trait_method_name, signature)
                            }) {
                            format!(
                                "Type '{}' does not implement required trait method '{}' (expected signature: {}; no default implementation)",
                                impl_block.type_name, trait_method_name, signature_repr
                            )
                        } else {
                            format!(
                                "Type '{}' does not implement required trait method '{}' (no default implementation)",
                                impl_block.type_name, trait_method_name
                            )
                        };

                        self.error_coded("E016", message, impl_block.span);
                    }
                }
            }
        }

        // Registrar que este tipo implementa este trait
        self.trait_impl_type_args.insert(
            (trait_name.to_string(), impl_block.type_name.clone()),
            trait_concrete_args,
        );
        self.trait_impls
            .insert((trait_name.to_string(), impl_block.type_name.clone()), true);
    }

    /// Copia métodos padrão do trait para o tipo que o implementa
    fn copy_default_trait_methods(
        &mut self,
        trait_name: &str,
        type_name: &str,
        impl_block: &crate::ast::ImplBlock,
    ) {
        // Obter métodos do trait
        let trait_methods = match self.traits.get(trait_name).cloned() {
            Some(methods) => methods,
            None => return,
        };

        // Obter métodos já implementados
        let implemented_methods: std::collections::HashSet<String> =
            impl_block.methods.iter().map(|m| m.name.clone()).collect();

        // Para cada método do trait com implementação padrão não implementado
        for (method_name, trait_method_info) in trait_methods {
            // Se tem default e não foi implementado
            if trait_method_info.has_default && !implemented_methods.contains(&method_name) {
                // Criar assinatura substituindo self genérico pelo tipo concreto
                let mut concrete_params = Vec::new();
                for (i, param) in trait_method_info.signature.params.iter().enumerate() {
                    if i == 0 && trait_method_info.signature.self_kind.is_some() {
                        // Substituir self genérico pelo tipo concreto
                        concrete_params.push(Type::Struct {
                            name: type_name.to_string(),
                        });
                    } else {
                        concrete_params.push(param.clone());
                    }
                }

                let concrete_signature = FunctionSignature {
                    params: concrete_params,
                    return_type: trait_method_info.signature.return_type.clone(),
                    self_kind: trait_method_info.signature.self_kind,
                    is_async: trait_method_info.signature.is_async,
                };

                // Registrar método no tipo
                let type_methods = self.methods.entry(type_name.to_string()).or_default();
                type_methods.insert(method_name, concrete_signature);
            }
        }
    }
}
