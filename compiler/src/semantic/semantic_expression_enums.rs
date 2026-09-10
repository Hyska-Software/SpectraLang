use super::*;

impl SemanticAnalyzer {
    pub(crate) fn analyze_expression_enum(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::EnumVariant {
                module_path,
                enum_name,
                type_args,
                variant_name,
                data,
                struct_data,
                ..
            } => {
                // ------------------------------------------------------------------
                // Qualified path resolution: module::item or module::Enum::Variant
                // ------------------------------------------------------------------
                let is_qualified =
                    module_path.is_some() || self.module_namespaces.contains(enum_name.as_str());

                if is_qualified {
                    let module_name = module_path.clone().unwrap_or_else(|| enum_name.clone());
                    let item_name = if module_path.is_some() {
                        enum_name.clone()
                    } else {
                        variant_name.clone()
                    };
                    let inner_variant = if module_path.is_some() {
                        variant_name.clone()
                    } else {
                        String::new()
                    };

                    let exports_cloned: Option<ModuleExports> = self
                        .registry
                        .read()
                        .unwrap_or_else(|p| p.into_inner())
                        .get_module(&module_name)
                        .cloned();

                    if let Some(exports) = exports_cloned {
                        // Cross-module function: module::function(args)
                        if let Some(func) = exports.functions.get(&item_name) {
                            if let Some(args) = data {
                                for arg in args {
                                    self.analyze_expression(arg);
                                }
                                if args.len() != func.params.len() {
                                    self.error(
                                        format!(
                                            "Function '{}' from module '{}' expects {} argument(s), got {}",
                                            item_name, module_name, func.params.len(), args.len()
                                        ),
                                        expr.span,
                                    );
                                } else {
                                    for (idx, (arg, expected)) in
                                        args.iter().zip(func.params.iter()).enumerate()
                                    {
                                        let arg_ty = self.infer_expression_type(arg);
                                        if arg_ty != *expected
                                            && arg_ty != Type::Unknown
                                            && *expected != Type::Unknown
                                        {
                                            self.error(
                                                format!(
                                                    "Argument {} of '{}' expects {}, found {}",
                                                    idx + 1,
                                                    item_name,
                                                    type_name(expected),
                                                    type_name(&arg_ty)
                                                ),
                                                arg.span,
                                            );
                                        }
                                    }
                                }
                            }
                            self.symbol_resolutions.insert(
                                expr.span,
                                SymbolInfo {
                                    is_local: false,
                                    def_span: None,
                                    ty: func.return_type.clone(),
                                },
                            );
                            // Make the function available for later direct calls
                            self.functions.entry(item_name.clone()).or_insert_with(|| {
                                FunctionSignature {
                                    params: func.params.clone(),
                                    return_type: func.return_type.clone(),
                                    self_kind: None,
                                    is_async: func.is_async,
                                }
                            });
                            // Register for the midend so lowering knows the return type.
                            self.qualified_fn_types
                                .push((item_name.clone(), func.return_type.clone()));
                            return;
                        }

                        // Cross-module type: struct or enum
                        if let Some(type_export) = exports.types.get(&item_name) {
                            if let Some(args) = data {
                                if let Some(method_export) = exports
                                    .methods
                                    .get(&item_name)
                                    .and_then(|methods| methods.get(&inner_variant))
                                {
                                    if method_export.self_kind.is_some() {
                                        self.error_coded_with_hint(
                                            "E018",
                                            format!(
                                                "Method '{}::{}' takes 'self'; call it on a value",
                                                item_name, inner_variant
                                            ),
                                            expr.span,
                                            format!(
                                                "Use `value.{}(...)` instead of `{}::{}(...)`.",
                                                inner_variant, item_name, inner_variant
                                            ),
                                        );
                                        return;
                                    }

                                    for arg in args {
                                        self.analyze_expression(arg);
                                    }
                                    if args.len() != method_export.params.len() {
                                        self.error(
                                            format!(
                                                "Associated function '{}::{}' expects {} argument(s), got {}",
                                                item_name,
                                                inner_variant,
                                                method_export.params.len(),
                                                args.len()
                                            ),
                                            expr.span,
                                        );
                                    } else {
                                        for (idx, (arg, expected)) in
                                            args.iter().zip(method_export.params.iter()).enumerate()
                                        {
                                            let arg_ty = self.infer_expression_type(arg);
                                            if !self.generic_argument_types_match(&arg_ty, expected)
                                                && arg_ty != Type::Unknown
                                                && *expected != Type::Unknown
                                            {
                                                self.error(
                                                    format!(
                                                        "Argument {} of '{}::{}' expects {}, found {}",
                                                        idx + 1,
                                                        item_name,
                                                        inner_variant,
                                                        type_name(expected),
                                                        type_name(&arg_ty)
                                                    ),
                                                    arg.span,
                                                );
                                            }
                                        }
                                    }

                                    self.symbol_resolutions.insert(
                                        expr.span,
                                        SymbolInfo {
                                            is_local: false,
                                            def_span: None,
                                            ty: method_export.return_type.clone(),
                                        },
                                    );
                                    self.qualified_fn_types.push((
                                        format!("{}_{}", item_name, inner_variant),
                                        method_export.return_type.clone(),
                                    ));
                                    return;
                                }
                            }

                            if type_export.is_enum {
                                // Validate as local enum variant
                                if let Some(ref variants) = type_export.enum_variants {
                                    if !variants.contains_key(&inner_variant) {
                                        self.error(
                                            format!(
                                                "Enum '{}' from module '{}' has no variant '{}'",
                                                item_name, module_name, inner_variant
                                            ),
                                            expr.span,
                                        );
                                        return;
                                    }
                                    let expected_payload =
                                        variants.get(&inner_variant).cloned().flatten();
                                    if let Some(args) = data {
                                        if let Some(ref expected_types) = expected_payload {
                                            if args.len() != expected_types.len() {
                                                self.error(
                                                    format!(
                                                        "Variant '{}' expects {} argument(s), got {}",
                                                        inner_variant, expected_types.len(), args.len()
                                                    ),
                                                    expr.span,
                                                );
                                            }
                                            for (idx, (arg, expected_ty_ann)) in
                                                args.iter().zip(expected_types.iter()).enumerate()
                                            {
                                                self.analyze_expression(arg);
                                                let expected_ty = self.type_annotation_to_type(
                                                    &Some(expected_ty_ann.clone()),
                                                );
                                                let arg_ty = self.infer_expression_type(arg);
                                                if arg_ty != expected_ty
                                                    && arg_ty != Type::Unknown
                                                    && expected_ty != Type::Unknown
                                                {
                                                    self.error(
                                                        format!(
                                                            "Argument {} of variant '{}' expects {}, found {}",
                                                            idx + 1, inner_variant, type_name(&expected_ty), type_name(&arg_ty)
                                                        ),
                                                        arg.span,
                                                    );
                                                }
                                            }
                                        } else {
                                            self.error(
                                                format!(
                                                    "Variant '{}' does not accept arguments",
                                                    inner_variant
                                                ),
                                                expr.span,
                                            );
                                        }
                                    }
                                    if let Some(ref _fields) = struct_data {
                                        self.error(
                                            format!(
                                                "Enum variant '{}' does not support struct-style fields",
                                                inner_variant
                                            ),
                                            expr.span,
                                        );
                                    }
                                }
                                self.symbol_resolutions.insert(
                                    expr.span,
                                    SymbolInfo {
                                        is_local: false,
                                        def_span: None,
                                        ty: Type::Enum {
                                            name: item_name.clone(),
                                        },
                                    },
                                );
                                return;
                            } else {
                                // Struct literal: module::Struct { fields }
                                if let Some(ref fields) = struct_data {
                                    if let Some(ref field_types) = type_export.struct_fields {
                                        for (field_name, field_value) in fields.iter() {
                                            self.analyze_expression(field_value);
                                            if let Some(expected_ty_ann) =
                                                field_types.get(field_name)
                                            {
                                                let expected_ty = self.type_annotation_to_type(
                                                    &Some(expected_ty_ann.clone()),
                                                );
                                                let val_ty =
                                                    self.infer_expression_type(field_value);
                                                if val_ty != expected_ty
                                                    && val_ty != Type::Unknown
                                                    && expected_ty != Type::Unknown
                                                {
                                                    self.error(
                                                        format!(
                                                            "Field '{}' of struct '{}' expects {}, found {}",
                                                            field_name, item_name, type_name(&expected_ty), type_name(&val_ty)
                                                        ),
                                                        field_value.span,
                                                    );
                                                }
                                            } else {
                                                self.error(
                                                    format!(
                                                        "Struct '{}' from module '{}' has no field '{}'",
                                                        item_name, module_name, field_name
                                                    ),
                                                    field_value.span,
                                                );
                                            }
                                        }
                                        for (expected_field, _) in field_types.iter() {
                                            if !fields.iter().any(|(n, _)| n == expected_field) {
                                                self.error(
                                                    format!(
                                                        "Missing field '{}' in struct '{}' from module '{}'",
                                                        expected_field, item_name, module_name
                                                    ),
                                                    expr.span,
                                                );
                                            }
                                        }
                                    }
                                } else {
                                    self.error(
                                        format!(
                                            "Struct '{}' from module '{}' requires field initialization",
                                            item_name, module_name
                                        ),
                                        expr.span,
                                    );
                                    return;
                                }
                                self.symbol_resolutions.insert(
                                    expr.span,
                                    SymbolInfo {
                                        is_local: false,
                                        def_span: None,
                                        ty: Type::Struct {
                                            name: item_name.clone(),
                                        },
                                    },
                                );
                                return;
                            }
                        }
                    }

                    self.error(
                        format!("Module '{}' does not export '{}'", module_name, item_name),
                        expr.span,
                    );
                    return;
                }

                // ------------------------------------------------------------------
                // UFCS: Trait::method(obj, args) — the first argument is the receiver.
                // ------------------------------------------------------------------
                if self.traits.contains_key(enum_name.as_str()) {
                    let trait_name = enum_name.as_str();
                    let call_args = data.as_deref().unwrap_or(&[]);
                    let trait_methods = self.traits.get(trait_name).cloned().unwrap_or_default();

                    if let Some(method_info) = trait_methods.get(variant_name.as_str()) {
                        if call_args.is_empty() {
                            self.error_coded(
                                "E017",
                                format!(
                                    "UFCS call '{}::{}' requires at least the receiver as its first argument",
                                    trait_name, variant_name
                                ),
                                expr.span,
                            );
                            return;
                        }

                        let receiver_ty = self.infer_expression_type(&call_args[0]);
                        let receiver_name = match &receiver_ty {
                            Type::Struct { name } => Some(name.clone()),
                            Type::Enum { name, .. } => Some(name.clone()),
                            Type::DynTrait { trait_name: t, .. } if t == trait_name => None,
                            _ => None,
                        };

                        if let Some(receiver_name) = receiver_name.as_ref() {
                            if !self
                                .trait_impls
                                .contains_key(&(trait_name.to_string(), receiver_name.clone()))
                            {
                                self.error_coded(
                                    "E016",
                                    format!(
                                        "Type '{}' does not implement trait '{}' required by UFCS call '{}::{}'",
                                        receiver_name, trait_name, trait_name, variant_name
                                    ),
                                    expr.span,
                                );
                                return;
                            }
                        } else if !matches!(&receiver_ty, Type::DynTrait { .. }) {
                            self.error_coded(
                                "E016",
                                format!(
                                    "UFCS call '{}::{}' requires a receiver implementing '{}'",
                                    trait_name, variant_name, trait_name
                                ),
                                expr.span,
                            );
                            return;
                        }

                        // Validate the remaining arguments against the trait signature.
                        // Generic trait implementations carry concrete arguments
                        // (e.g. `PairView<int, string> for IntText`); apply them to
                        // the UFCS return and parameter types before checking.
                        let concrete_signature = receiver_name
                            .as_ref()
                            .and_then(|receiver_name| {
                                self.trait_impl_type_args
                                    .get(&(trait_name.to_string(), receiver_name.clone()))
                            })
                            .map(|concrete_args| {
                                let trait_params = self
                                    .trait_type_params
                                    .get(trait_name)
                                    .cloned()
                                    .unwrap_or_default();
                                let mut signature = method_info.signature.clone();
                                signature.params = signature
                                    .params
                                    .iter()
                                    .map(|param| {
                                        self.substitute_generic_types(
                                            param,
                                            &trait_params,
                                            concrete_args,
                                        )
                                    })
                                    .collect();
                                signature.return_type = self.substitute_generic_types(
                                    &signature.return_type,
                                    &trait_params,
                                    concrete_args,
                                );
                                signature
                            })
                            .unwrap_or_else(|| method_info.signature.clone());
                        let sig = &concrete_signature;
                        let expected = sig.params.len().saturating_sub(1);
                        if call_args.len() - 1 != expected {
                            self.error_coded(
                                "E023",
                                format!(
                                    "Trait method '{}::{}' expects {} argument(s) after the receiver, found {}",
                                    trait_name,
                                    variant_name,
                                    expected,
                                    call_args.len() - 1
                                ),
                                expr.span,
                            );
                            return;
                        }
                        for (i, arg) in call_args.iter().skip(1).enumerate() {
                            let arg_ty = self.infer_expression_type(arg);
                            if let Some(expected_ty) = sig.params.get(i + 1) {
                                if !self.generic_argument_types_match(&arg_ty, expected_ty)
                                    && arg_ty != Type::Unknown
                                    && *expected_ty != Type::Unknown
                                {
                                    self.error_coded(
                                        "E023",
                                        format!(
                                            "Argument {} of UFCS call '{}::{}' has type {}, expected {}",
                                            i + 2,
                                            trait_name,
                                            variant_name,
                                            type_name(&arg_ty),
                                            type_name(expected_ty)
                                        ),
                                        arg.span,
                                    );
                                }
                            }
                        }

                        self.symbol_resolutions.insert(
                            expr.span,
                            SymbolInfo {
                                is_local: false,
                                def_span: None,
                                ty: sig.return_type.clone(),
                            },
                        );
                    } else {
                        self.error_coded(
                            "E017",
                            format!("Trait '{}' has no method '{}'", trait_name, variant_name),
                            expr.span,
                        );
                    }
                    return;
                }

                // ------------------------------------------------------------------
                // Local enum / struct static method (existing behaviour)
                // ------------------------------------------------------------------
                if let Some(args) = data {
                    for arg in args {
                        self.analyze_expression(arg);
                    }
                }
                if let Some(fields) = struct_data {
                    for (_, value) in fields {
                        self.analyze_expression(value);
                    }
                }

                let enum_info = match self.enum_infos.get(enum_name).cloned() {
                    Some(info) => info,
                    None => {
                        // Check if this is a struct static method call: StructName::method(...)
                        if self.struct_infos.contains_key(enum_name.as_str()) {
                            let signature = self
                                .methods
                                .get(enum_name.as_str())
                                .and_then(|mm| mm.get(variant_name.as_str()))
                                .cloned();

                            if let Some(signature) = &signature {
                                if signature.self_kind.is_some() {
                                    self.error_coded_with_hint(
                                        "E018",
                                        format!(
                                            "Method '{}::{}' takes 'self'; call it on a value",
                                            enum_name, variant_name
                                        ),
                                        expr.span,
                                        format!(
                                            "Use `value.{}(...)` instead of `{}::{}(...)`.",
                                            variant_name, enum_name, variant_name
                                        ),
                                    );
                                    return;
                                }

                                let args = data.as_deref().unwrap_or(&[]);
                                if args.len() != signature.params.len() {
                                    self.error(
                                        format!(
                                            "Associated function '{}::{}' expects {} argument(s), got {}",
                                            enum_name,
                                            variant_name,
                                            signature.params.len(),
                                            args.len()
                                        ),
                                        expr.span,
                                    );
                                } else {
                                    for (idx, (arg, expected)) in
                                        args.iter().zip(signature.params.iter()).enumerate()
                                    {
                                        let arg_ty = self.infer_expression_type(arg);
                                        if !self.generic_argument_types_match(&arg_ty, expected)
                                            && arg_ty != Type::Unknown
                                            && *expected != Type::Unknown
                                        {
                                            self.error(
                                                format!(
                                                    "Argument {} of '{}::{}' expects {}, found {}",
                                                    idx + 1,
                                                    enum_name,
                                                    variant_name,
                                                    type_name(expected),
                                                    type_name(&arg_ty)
                                                ),
                                                arg.span,
                                            );
                                        }
                                    }
                                    self.validate_derived_from_json_literal(
                                        enum_name,
                                        variant_name,
                                        args,
                                        expr.span,
                                    );
                                }
                            }

                            let return_type = signature
                                .map(|sig| sig.return_type.clone())
                                .unwrap_or(Type::Struct {
                                    name: enum_name.clone(),
                                });
                            self.symbol_resolutions.insert(
                                expr.span,
                                SymbolInfo {
                                    is_local: false,
                                    def_span: None,
                                    ty: return_type,
                                },
                            );
                            return;
                        }
                        self.error(format!("Enum '{}' is not defined", enum_name), expr.span);
                        return;
                    }
                };

                let expected_type_arg_count = enum_info.type_params.len();
                if !type_args.is_empty() {
                    if expected_type_arg_count == 0 {
                        self.error(
                            format!(
                                "Enum '{}' does not accept type arguments, but {} were provided",
                                enum_name,
                                type_args.len()
                            ),
                            expr.span,
                        );
                    } else if type_args.len() != expected_type_arg_count {
                        self.error(
                            format!(
                                "Enum '{}' expects {} type argument(s), but {} were provided",
                                enum_name,
                                expected_type_arg_count,
                                type_args.len()
                            ),
                            expr.span,
                        );
                    }
                }

                let variant_info = match enum_info.variants.get(variant_name).cloned() {
                    Some(info) => info,
                    None => {
                        self.error(
                            format!(
                                "Enum '{}' has no variant named '{}'",
                                enum_name, variant_name
                            ),
                            expr.span,
                        );
                        return;
                    }
                };

                // Resolve generic payload annotations before validating a
                // constructor. Built-in Option/Result store `T`/`E` in their
                // variant definitions, but those parameters are not pushed on
                // the ordinary lexical generic stack. Without this local
                // substitution, a valid `Option::Some(42)` was compared with
                // Unknown and rejected as soon as Unknown stopped being a
                // wildcard.
                let mut variant_substitutions = HashMap::new();
                if type_args.len() == enum_info.type_params.len() {
                    for (name, annotation) in enum_info.type_params.iter().zip(type_args) {
                        variant_substitutions.insert(
                            name.clone(),
                            self.type_annotation_to_type(&Some(annotation.clone())),
                        );
                    }
                }
                if let Some(Type::Enum {
                    name: expected_name,
                }) = self.current_expected_type.clone()
                {
                    if let Some((base_name, _, expected_substitutions)) =
                        self.specialized_enum_context(&expected_name)
                    {
                        if base_name == *enum_name {
                            variant_substitutions.extend(expected_substitutions);
                        }
                    }
                }
                if let (Some(expected_params), Some(actual_args)) =
                    (&variant_info.data, data.as_ref())
                {
                    for (expected_ann, arg_expr) in expected_params.iter().zip(actual_args) {
                        let arg_type = self.infer_expression_type(arg_expr);
                        self.unify_type_annotation(
                            expected_ann,
                            &arg_type,
                            &mut variant_substitutions,
                        );
                    }
                }
                if let (Some(expected_fields), Some(actual_fields)) =
                    (&variant_info.struct_data, struct_data.as_ref())
                {
                    // Named payloads participate in generic inference just
                    // like tuple payloads.  This pass also runs when the
                    // constructor is nested in a function call whose
                    // expected type is not kept in the later AST-analysis
                    // traversal, so infer T from the actual field value here.
                    for (field_name, expected_ann) in expected_fields {
                        if let Some((_, field_expr)) = actual_fields
                            .iter()
                            .find(|(actual_name, _)| actual_name == field_name)
                        {
                            let actual_type = self.infer_expression_type(field_expr);
                            self.unify_type_annotation(
                                expected_ann,
                                &actual_type,
                                &mut variant_substitutions,
                            );
                        }
                    }
                }

                self.symbol_resolutions.insert(
                    expr.span,
                    SymbolInfo {
                        is_local: false,
                        def_span: Some(variant_info.span),
                        ty: Type::Enum {
                            name: enum_name.clone(),
                        },
                    },
                );

                match (
                    &variant_info.data,
                    &variant_info.struct_data,
                    data,
                    struct_data,
                ) {
                    (Some(_), _, _, Some(_)) => {
                        self.error(
                            format!(
                                "Variant '{}::{}' expects tuple-style values, not named fields",
                                enum_name, variant_name
                            ),
                            expr.span,
                        );
                    }
                    (_, Some(_), Some(_), _) => {
                        self.error(
                            format!(
                                "Variant '{}::{}' expects named fields, not tuple-style values",
                                enum_name, variant_name
                            ),
                            expr.span,
                        );
                    }
                    (Some(expected_params), _, Some(actual_args), None) => {
                        if expected_params.len() != actual_args.len() {
                            self.error(
                                format!(
                                    "Variant '{}::{}' expects {} value(s), but {} were provided",
                                    enum_name,
                                    variant_name,
                                    expected_params.len(),
                                    actual_args.len()
                                ),
                                expr.span,
                            );
                        }

                        for (idx, (expected_ann, arg_expr)) in
                            expected_params.iter().zip(actual_args.iter()).enumerate()
                        {
                            let arg_type = self.infer_expression_type(arg_expr);
                            let expected_type = self.type_annotation_to_type_with_substitutions(
                                expected_ann,
                                &variant_substitutions,
                            );

                            if !self.generic_argument_types_match(&arg_type, &expected_type) {
                                self.error(
                                    format!(
                                        "Argument {} for variant '{}::{}' has type {:?}, but {:?} was expected",
                                        idx + 1,
                                        enum_name,
                                        variant_name,
                                        arg_type,
                                        expected_type
                                    ),
                                    arg_expr.span,
                                );
                            }
                        }
                    }
                    (None, Some(expected_fields), None, Some(actual_fields)) => {
                        let expected_map: HashMap<String, crate::ast::TypeAnnotation> =
                            expected_fields.iter().cloned().collect();
                        let mut seen = HashSet::new();

                        for (field_name, field_expr) in actual_fields {
                            if !seen.insert(field_name.clone()) {
                                self.error(
                                    format!(
                                        "Field '{}' appears more than once in variant '{}::{}'",
                                        field_name, enum_name, variant_name
                                    ),
                                    field_expr.span,
                                );
                                continue;
                            }

                            let Some(expected_ann) = expected_map.get(field_name) else {
                                self.error(
                                    format!(
                                        "Variant '{}::{}' has no field named '{}'",
                                        enum_name, variant_name, field_name
                                    ),
                                    field_expr.span,
                                );
                                continue;
                            };

                            let actual_type = self.infer_expression_type(field_expr);
                            let expected_type = self.type_annotation_to_type_with_substitutions(
                                expected_ann,
                                &variant_substitutions,
                            );
                            if !self.generic_argument_types_match(&actual_type, &expected_type) {
                                self.error(
                                    format!(
                                        "Field '{}' of variant '{}::{}' has type {:?}, but {:?} was expected",
                                        field_name,
                                        enum_name,
                                        variant_name,
                                        actual_type,
                                        expected_type
                                    ),
                                    field_expr.span,
                                );
                            }
                        }

                        for (field_name, _) in expected_fields {
                            if !seen.contains(field_name) {
                                self.error(
                                    format!(
                                        "Variant '{}::{}' is missing field '{}'",
                                        enum_name, variant_name, field_name
                                    ),
                                    expr.span,
                                );
                            }
                        }
                    }
                    (Some(expected_params), _, None, None) => {
                        self.error(
                            format!(
                                "Variant '{}::{}' expects {} value(s)",
                                enum_name,
                                variant_name,
                                expected_params.len()
                            ),
                            expr.span,
                        );
                    }
                    (None, Some(expected_fields), None, None) => {
                        self.error(
                            format!(
                                "Variant '{}::{}' expects {} named field(s)",
                                enum_name,
                                variant_name,
                                expected_fields.len()
                            ),
                            expr.span,
                        );
                    }
                    (None, None, Some(actual_args), None) => {
                        if !actual_args.is_empty() {
                            self.error(
                                format!(
                                    "Variant '{}::{}' does not take any values",
                                    enum_name, variant_name
                                ),
                                expr.span,
                            );
                        }
                    }
                    (None, None, None, Some(actual_fields)) => {
                        if !actual_fields.is_empty() {
                            self.error(
                                format!(
                                    "Variant '{}::{}' does not take named fields",
                                    enum_name, variant_name
                                ),
                                expr.span,
                            );
                        }
                    }
                    (None, None, Some(actual_args), Some(actual_fields)) => {
                        if !actual_args.is_empty() || !actual_fields.is_empty() {
                            self.error(
                                format!(
                                    "Variant '{}::{}' does not accept both positional and named values",
                                    enum_name, variant_name
                                ),
                                expr.span,
                            );
                        }
                    }
                    (None, None, None, None) => {}
                }
            }
            _ => unreachable!("expression category mismatch"),
        }
    }
}
