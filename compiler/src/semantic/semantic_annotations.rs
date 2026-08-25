impl SemanticAnalyzer {
    fn annotation_to_pattern(annotation: &crate::ast::TypeAnnotation) -> TypeAnnotationPattern {
        use crate::ast::TypeAnnotationKind;

        match &annotation.kind {
            TypeAnnotationKind::Simple { segments } => {
                TypeAnnotationPattern::Simple(segments.clone())
            }
            TypeAnnotationKind::Tuple { elements } => TypeAnnotationPattern::Tuple(
                elements.iter().map(Self::annotation_to_pattern).collect(),
            ),
            TypeAnnotationKind::Function { .. } => {
                TypeAnnotationPattern::Simple(vec!["fn".to_string()])
            }
            TypeAnnotationKind::Generic { name, .. } => {
                TypeAnnotationPattern::Simple(vec![name.clone()])
            }
            TypeAnnotationKind::DynTrait {
                trait_name,
                auto_traits,
            } => {
                TypeAnnotationPattern::Simple(vec![format_dyn_trait_name(trait_name, auto_traits)])
            }
        }
    }

    fn option_annotation_to_pattern(
        annotation: &Option<crate::ast::TypeAnnotation>,
    ) -> Option<TypeAnnotationPattern> {
        annotation.as_ref().map(Self::annotation_to_pattern)
    }

    fn format_parameter(param: &ParameterInfo) -> String {
        if param.is_self {
            if param.is_reference {
                if param.is_mutable {
                    "&mut self".to_string()
                } else {
                    "&self".to_string()
                }
            } else {
                "self".to_string()
            }
        } else if let Some(ty) = &param.ty {
            ty.to_string()
        } else {
            "_".to_string()
        }
    }

    fn annotation_mentions_self(annotation: &crate::ast::TypeAnnotation) -> bool {
        use crate::ast::TypeAnnotationKind;

        match &annotation.kind {
            TypeAnnotationKind::Simple { segments } => segments.iter().any(|part| part == "Self"),
            TypeAnnotationKind::Tuple { elements } => {
                elements.iter().any(Self::annotation_mentions_self)
            }
            TypeAnnotationKind::Function {
                params,
                return_type,
            } => {
                params.iter().any(Self::annotation_mentions_self)
                    || Self::annotation_mentions_self(return_type)
            }
            TypeAnnotationKind::Generic { type_args, .. } => {
                type_args.iter().any(Self::annotation_mentions_self)
            }
            TypeAnnotationKind::DynTrait { .. } => false,
        }
    }

    fn validate_async_trait_method_object_safety(
        &mut self,
        trait_name: &str,
        method: &crate::ast::TraitMethod,
        self_kind: Option<SelfParamKind>,
    ) {
        if !method.is_async {
            return;
        }

        let mut reason = None;
        match self_kind {
            Some(SelfParamKind::Reference { .. }) => {}
            Some(SelfParamKind::Value) => {
                reason = Some("uses by-value `self`; use `&self` or `&mut self`");
            }
            None => {
                reason = Some("does not declare a `self` receiver");
            }
        }

        if reason.is_none()
            && method
                .params
                .iter()
                .filter_map(|param| param.type_annotation.as_ref())
                .any(Self::annotation_mentions_self)
        {
            reason = Some("mentions `Self` in a parameter type");
        }

        if reason.is_none()
            && method
                .return_type
                .as_ref()
                .is_some_and(Self::annotation_mentions_self)
        {
            reason = Some("mentions `Self` in its return type");
        }

        if let Some(reason) = reason {
            self.error_coded_with_hint(
                "E2108",
                format!(
                    "Async trait method '{}::{}' is not object-safe: {}",
                    trait_name, method.name, reason
                ),
                method.span,
                "Object-safe async trait methods must use `&self` or `&mut self` and must not expose `Self` in parameter or return types.",
            );
        }
    }

    fn format_trait_signature(method_name: &str, signature: &TraitMethodSignature) -> String {
        let params = signature
            .params
            .iter()
            .map(Self::format_parameter)
            .collect::<Vec<_>>()
            .join(", ");

        let return_part = signature
            .return_type
            .as_ref()
            .map(|ty| format!(" returns {}", ty))
            .unwrap_or_else(|| " returns unit".to_string());

        let prefix = if signature.is_async { "async func" } else { "func" };
        format!("{} {}({}){}", prefix, method_name, params, return_part)
    }

    fn declare_symbol(&mut self, name: String, span: Span, ty: Type) -> bool {
        let is_local = self.symbols.len() > 1;
        // Check if already declared in current scope
        if let Some(current_scope) = self.symbols.last_mut() {
            if current_scope.contains_key(&name) {
                return false; // Already declared
            }
            let info = SymbolInfo {
                is_local,
                def_span: Some(span),
                ty,
            };
            current_scope.insert(name, info.clone());
            self.symbol_resolutions.insert(span, info);
            true
        } else {
            false
        }
    }

    fn lookup_symbol(&self, name: &str) -> Option<&SymbolInfo> {
        // Search from innermost to outermost scope
        for scope in self.symbols.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info);
            }
        }
        None
    }

    fn lookup_symbol_in_current_scope(&self, name: &str) -> Option<&SymbolInfo> {
        self.symbols.last().and_then(|scope| scope.get(name))
    }

    fn collect_pattern_binding_names(&self, pattern: &Pattern) -> Vec<String> {
        let mut names = Vec::new();
        self.collect_pattern_binding_names_into(pattern, &mut names);
        names
    }

    fn collect_pattern_binding_names_into(&self, pattern: &Pattern, names: &mut Vec<String>) {
        match pattern {
            Pattern::Identifier(name) => names.push(name.clone()),
            Pattern::Tuple(elements) => {
                for element in elements {
                    self.collect_pattern_binding_names_into(element, names);
                }
            }
            Pattern::Struct { fields, .. } => {
                for (_, pattern) in fields {
                    self.collect_pattern_binding_names_into(pattern, names);
                }
            }
            Pattern::EnumVariant {
                data, struct_data, ..
            } => {
                if let Some(patterns) = data {
                    for pattern in patterns {
                        self.collect_pattern_binding_names_into(pattern, names);
                    }
                }
                if let Some(fields) = struct_data {
                    for (_, pattern) in fields {
                        self.collect_pattern_binding_names_into(pattern, names);
                    }
                }
            }
            Pattern::Or(patterns) => {
                if let Some(first) = patterns.first() {
                    self.collect_pattern_binding_names_into(first, names);
                }
            }
            Pattern::Wildcard | Pattern::Literal(_) => {}
        }
    }

    fn record_symbol_resolution(&mut self, span: Span, info: SymbolInfo) {
        self.symbol_resolutions.insert(span, info);
    }

    /// Suggest a similar name from all in-scope symbols and known functions.
    /// Returns `Some("did you mean 'X'?")` when a close-enough match is found.
    fn suggest_name(&self, name: &str) -> Option<String> {
        let threshold = (name.len() / 3 + 1).min(3);
        let mut best: Option<(String, usize)> = None;

        let mut consider = |candidate: &str| {
            if candidate == name {
                return;
            }
            let d = levenshtein_distance(name, candidate);
            if d > 0 && d <= threshold
                && best.as_ref().is_none_or(|(_, bd)| d < *bd) {
                    best = Some((candidate.to_owned(), d));
                }
        };

        for scope in self.symbols.iter() {
            for key in scope.keys() {
                consider(key.as_str());
            }
        }
        for key in self.functions.keys() {
            consider(key.as_str());
        }

        best.map(|(s, _)| format!("did you mean '{}'?", s))
    }

    fn primitive_numeric_alias(name: &str) -> Option<Type> {
        match name {
            "int" => Some(Type::Int),
            "float" => Some(Type::Float),
            "i8" => Some(Type::ExactInt { signed: true, width: IntWidth::I8 }),
            "i16" => Some(Type::ExactInt { signed: true, width: IntWidth::I16 }),
            "i32" => Some(Type::ExactInt { signed: true, width: IntWidth::I32 }),
            "i64" => Some(Type::ExactInt { signed: true, width: IntWidth::I64 }),
            "isize" => Some(Type::ExactInt { signed: true, width: IntWidth::Isize }),
            "u8" => Some(Type::ExactInt { signed: false, width: IntWidth::I8 }),
            "u16" => Some(Type::ExactInt { signed: false, width: IntWidth::I16 }),
            "u32" => Some(Type::ExactInt { signed: false, width: IntWidth::I32 }),
            "u64" => Some(Type::ExactInt { signed: false, width: IntWidth::I64 }),
            "usize" => Some(Type::ExactInt { signed: false, width: IntWidth::Usize }),
            "f32" => Some(Type::ExactFloat { width: FloatWidth::F32 }),
            "f64" => Some(Type::ExactFloat { width: FloatWidth::F64 }),
            _ => None,
        }
    }

    fn is_builtin_type(name: &str) -> bool {
        Self::primitive_numeric_alias(name).is_some()
            || matches!(name, "bool" | "string" | "char" | "Self")
    }

    fn can_auto_promote(&self, from: &Type, to: &Type) -> bool {
        matches!((from, to), (Type::Int, Type::Float))
    }

    fn is_numeric_type(ty: &Type) -> bool {
        matches!(ty, Type::Int | Type::Float | Type::ExactInt { .. } | Type::ExactFloat { .. })
    }

    fn numeric_types_can_interact(&self, left: &Type, right: &Type) -> bool {
        Self::is_numeric_type(left) && Self::is_numeric_type(right)
    }

    fn numeric_result_type(&self, left: &Type, right: &Type) -> Type {
        if matches!(left, Type::Unknown) || matches!(right, Type::Unknown) {
            return Type::Unknown;
        }

        if matches!(left, Type::Float) || matches!(right, Type::Float) {
            Type::Float
        } else if matches!(left, Type::ExactFloat { .. }) || matches!(right, Type::ExactFloat { .. }) {
            if left == right { left.clone() } else { Type::ExactFloat { width: FloatWidth::F64 } }
        } else if matches!(left, Type::Int) && matches!(right, Type::Int) {
            Type::Int
        } else if matches!(left, Type::ExactInt { .. }) && left == right {
            left.clone()
        } else {
            Type::ExactInt { signed: true, width: IntWidth::I64 }
        }
    }

    fn lookup_type_visibility(&self, name: &str) -> Option<Visibility> {
        if let Some(info) = self.struct_infos.get(name) {
            return Some(info.visibility);
        }
        if let Some(info) = self.enum_infos.get(name) {
            return Some(info.visibility);
        }
        None
    }

    fn validate_public_type_annotation(
        &mut self,
        annotation: &crate::ast::TypeAnnotation,
        generics: &HashSet<String>,
        context: &str,
        span: Span,
    ) {
        use crate::ast::TypeAnnotationKind;

        match &annotation.kind {
            TypeAnnotationKind::Simple { segments } => {
                if let Some(name) = segments.last() {
                    if generics.contains(name) || Self::is_builtin_type(name) {
                        return;
                    }

                    if let Some(visibility) = self.lookup_type_visibility(name) {
                        if visibility == Visibility::Private {
                            self.error_with_context(
                                format!(
                                    "Type '{}' is private but exposed through a public item",
                                    name
                                ),
                                span,
                                format!("{} cannot mention private types", context),
                            );
                        }
                    }
                }
            }
            TypeAnnotationKind::Tuple { elements } => {
                for element in elements {
                    self.validate_public_type_annotation(element, generics, context, span);
                }
            }
            TypeAnnotationKind::Function {
                params,
                return_type,
            } => {
                for param in params {
                    self.validate_public_type_annotation(param, generics, context, span);
                }
                self.validate_public_type_annotation(return_type, generics, context, span);
            }
            TypeAnnotationKind::Generic { name, type_args } => {
                let _ = name;
                for arg in type_args {
                    self.validate_public_type_annotation(arg, generics, context, span);
                }
            }
            TypeAnnotationKind::DynTrait { .. } => {}
        }
    }

    fn enforce_visibility_rules(&mut self, item: &Item) {
        match item {
            Item::Struct(struct_def) if struct_def.visibility == Visibility::Public => {
                let generics: HashSet<String> = struct_def
                    .type_params
                    .iter()
                    .map(|tp| tp.name.clone())
                    .collect();

                for field in &struct_def.fields {
                    self.validate_public_type_annotation(
                        &field.ty,
                        &generics,
                        &format!("Public struct '{}' field '{}'", struct_def.name, field.name),
                        field.span,
                    );
                }
            }
            Item::Enum(enum_def) if enum_def.visibility == Visibility::Public => {
                let generics: HashSet<String> = enum_def
                    .type_params
                    .iter()
                    .map(|tp| tp.name.clone())
                    .collect();

                for variant in &enum_def.variants {
                    if let Some(data) = &variant.data {
                        for (index, annotation) in data.iter().enumerate() {
                            self.validate_public_type_annotation(
                                annotation,
                                &generics,
                                &format!(
                                    "Public enum '{}' variant '{}' field {}",
                                    enum_def.name,
                                    variant.name,
                                    index + 1
                                ),
                                variant.span,
                            );
                        }
                    }
                }
            }
            Item::Function(func) if func.visibility == Visibility::Public => {
                let generics: HashSet<String> =
                    func.type_params.iter().map(|tp| tp.name.clone()).collect();

                for param in &func.params {
                    if let Some(annotation) = &param.ty {
                        self.validate_public_type_annotation(
                            annotation,
                            &generics,
                            &format!("Public function '{}' parameter '{}'", func.name, param.name),
                            param.span,
                        );
                    }
                }

                if let Some(ret) = &func.return_type {
                    self.validate_public_type_annotation(
                        ret,
                        &generics,
                        &format!("Public function '{}' return type", func.name),
                        func.span,
                    );
                }
            }
            _ => {}
        }
    }

    fn conversion_hint(&self, actual: &Type, expected: &Type) -> Option<String> {
        match (actual, expected) {
            (Type::Float, Type::Int) => Some(
                "Implicit narrowing from float to int is not allowed; use an explicit conversion.".to_string(),
            ),
            (Type::String, Type::Int | Type::Float | Type::Bool) => Some(
                "Strings cannot be implicitly converted; parse or convert explicitly.".to_string(),
            ),
            (Type::Bool, Type::Int | Type::Float) => Some(
                "Booleans do not implicitly convert to numbers; use a conditional or explicit conversion.".to_string(),
            ),
            (Type::Int, Type::Bool) => Some(
                "Integers do not implicitly convert to booleans; compare against zero or use an explicit helper.".to_string(),
            ),
            _ => None,
        }
    }

    fn types_match(&self, actual: &Type, expected: &Type) -> bool {
        if self.can_auto_promote(actual, expected) {
            return true;
        }

        // Built-in generic enum signatures use the unspecialized enum name as
        // a typed pattern. This keeps Option/Result APIs concrete without
        // falling back to Type::Unknown as a wildcard.
        if Self::generic_enum_pattern_matches(actual, expected)
            || Self::generic_enum_pattern_matches(expected, actual)
            || Self::generic_collection_pattern_matches(actual, expected)
            || Self::generic_collection_pattern_matches(expected, actual)
        {
            return true;
        }

        match (actual, expected) {
            // Tipos idênticos
            (Type::Int, Type::Int) => true,
            (Type::Float, Type::Float) => true,
            (Type::String, Type::String) => true,
            (Type::Bool, Type::Bool) => true,
            (Type::Char, Type::Char) => true,
            (Type::Unit, Type::Unit) => true,
            (Type::ExactInt { .. }, Type::ExactInt { .. }) => actual == expected,
            (Type::ExactFloat { .. }, Type::ExactFloat { .. }) => actual == expected,
            // Untyped integer/float literals are checked against an exact
            // target at declaration/cast validation time.
            (Type::Int, Type::ExactInt { .. }) | (Type::Float, Type::ExactFloat { .. }) => true,

            // Structs com mesmo nome
            (Type::Struct { name: n1 }, Type::Struct { name: n2 }) => n1 == n2,

            // Generic applications use structural arguments while older
            // method/trait registries still expose the compatibility mangled
            // nominal form (`Pair<int, string>` ↔ `Pair_int_string`).
            (Type::Applied { .. }, Type::Struct { name } | Type::Enum { name }) => {
                self.nominal_lookup_name(actual).as_deref() == Some(name.as_str())
            }
            (Type::Struct { name } | Type::Enum { name }, Type::Applied { .. }) => {
                self.nominal_lookup_name(expected).as_deref() == Some(name.as_str())
            }

            // Generic applications are compared structurally.  The base name
            // alone is not enough: `List<int>` and `List<string>` are distinct
            // types even though both use the same runtime handle ABI.
            (
                Type::Applied {
                    name: n1,
                    args: a1,
                },
                Type::Applied {
                    name: n2,
                    args: a2,
                },
            ) => {
                n1 == n2
                    && a1.len() == a2.len()
                    && a1
                        .iter()
                        .zip(a2.iter())
                        .all(|(left, right)| self.types_match(left, right))
            }

            // Enums specialized from generics must match the full mangled type,
            // not just the generic base (Result_int_string != Result_int_int).
            (Type::Enum { name: n1, .. }, Type::Enum { name: n2, .. }) => n1 == n2,

            // Function types must agree on arity, parameter types, and return type.
            (
                Type::Fn {
                    params: p1,
                    return_type: r1,
                },
                Type::Fn {
                    params: p2,
                    return_type: r2,
                },
            ) => {
                p1.len() == p2.len()
                    && p1
                        .iter()
                        .zip(p2.iter())
                        .all(|(a, b)| self.types_match(a, b))
                    && self.types_match(r1, r2)
            }
            (Type::Task { output: a }, Type::Task { output: b }) => self.types_match(a, b),
            (Type::Range, Type::Range) => true,

            // Dynamic trait objects match structurally by trait name.
            (
                Type::DynTrait {
                    trait_name: a,
                    auto_traits: auto_a,
                },
                Type::DynTrait {
                    trait_name: b,
                    auto_traits: auto_b,
                },
            ) => a == b && auto_trait_bounds_satisfied(auto_a, auto_b),

            // Tensor handles carry compiler-visible dtype/rank metadata but
            // remain runtime handles, so they are compatible with int at FFI
            // and current std.tensor boundaries.
            (Type::Tensor { .. }, Type::Int) | (Type::Int, Type::Tensor { .. }) => true,
            (
                Type::Tensor {
                    dtype: dtype_a,
                    rank: rank_a,
                    dims: dims_a,
                    layout: layout_a,
                    device: device_a,
                },
                Type::Tensor {
                    dtype: dtype_b,
                    rank: rank_b,
                    dims: dims_b,
                    layout: layout_b,
                    device: device_b,
                },
            ) => {
                self.types_match(dtype_a, dtype_b)
                    && (rank_a.is_none() || rank_b.is_none() || rank_a == rank_b)
                    && Self::tensor_dims_match(dims_a, dims_b)
                    && (layout_a.is_none() || layout_b.is_none() || layout_a == layout_b)
                    && (device_a.is_none() || device_b.is_none() || device_a == device_b)
            }

            // Unknown is an inference failure, not a type wildcard. Generic
            // parameters are handled explicitly below and remain the only
            // intentionally polymorphic path.
            (Type::Unknown, _) | (_, Type::Unknown) => false,

            // Self type matches any Struct (will be resolved in context)
            (Type::SelfType, Type::Struct { .. }) | (Type::Struct { .. }, Type::SelfType) => true,
            (Type::SelfType, Type::Applied { .. }) | (Type::Applied { .. }, Type::SelfType) => true,
            (Type::SelfType, Type::SelfType) => true,

            // Tuples com mesmo tamanho e tipos compatíveis
            (Type::Tuple { elements: t1 }, Type::Tuple { elements: t2 }) => {
                t1.len() == t2.len()
                    && t1
                        .iter()
                        .zip(t2.iter())
                        .all(|(a, b)| self.types_match(a, b))
            }

            // Generic type parameters match only themselves. Concrete
            // compatibility for signatures that still carry type parameters is
            // decided by the bound-aware `generic_argument_types_match` at
            // call-argument validation sites; body and return checking stay
            // strict so a concrete value can never silently satisfy `T`.
            (Type::TypeParameter { name: a }, Type::TypeParameter { name: b }) => a == b,

            // Arrays com tipos de elemento compatíveis
            (
                Type::Array {
                    element_type: e1,
                    size: s1,
                },
                Type::Array {
                    element_type: e2,
                    size: s2,
                },
            ) => self.types_match(e1, e2) && (s1.is_none() || s2.is_none() || s1 == s2),

            _ => false,
        }
    }
    /// Call-argument compatibility when the expected signature still contains
    /// generic type parameters (user generic functions, unspecialized builtin
    /// returns, enum/struct constructor payloads, higher-order closures). A
    /// concrete argument is accepted when it satisfies every trait bound
    /// registered for the parameter; unconstrained parameters accept any
    /// concrete type. Strict body/return checking continues to use
    /// `types_match` directly.
    fn generic_argument_types_match(&self, actual: &Type, expected: &Type) -> bool {
        if self.types_match(actual, expected) {
            return true;
        }
        let bound_accepts =
            |semantic: &Self, param: &str, concrete: &Type| -> bool {
                if matches!(concrete, Type::Unknown | Type::TypeParameter { .. }) {
                    return true;
                }
                match semantic.get_generic_bounds(param) {
                    Some(bounds) => bounds
                        .iter()
                        .all(|bound| semantic.type_satisfies_trait_bound(concrete, bound)),
                    None => true,
                }
            };
        match (actual, expected) {
            (_, Type::TypeParameter { name }) => bound_accepts(self, name, actual),
            (Type::TypeParameter { name }, _) => bound_accepts(self, name, expected),
            // Closures and generic applications are compared structurally so a
            // parameter nested inside `Fn`/`Applied` stays compatible with the
            // concrete argument that instantiates it.
            (
                Type::Fn {
                    params: actual_params,
                    return_type: actual_return,
                },
                Type::Fn {
                    params: expected_params,
                    return_type: expected_return,
                },
            ) => {
                actual_params.len() == expected_params.len()
                    && actual_params
                        .iter()
                        .zip(expected_params.iter())
                        .all(|(a, b)| self.generic_argument_types_match(a, b))
                    && self.generic_argument_types_match(actual_return, expected_return)
            }
            (
                Type::Applied {
                    name: actual_name,
                    args: actual_args,
                },
                Type::Applied {
                    name: expected_name,
                    args: expected_args,
                },
            ) => {
                actual_name == expected_name
                    && actual_args.len() == expected_args.len()
                    && actual_args
                        .iter()
                        .zip(expected_args.iter())
                        .all(|(a, b)| self.generic_argument_types_match(a, b))
            }
            _ => false,
        }
    }

    /// Binding-site compatibility (`let` annotations and assignments): the
    /// value's type may still carry an unresolved generic parameter when
    /// inference could not fix it; such values stay tolerated. A fully
    /// concrete value must match strictly.
    fn inferred_binding_types_match(&self, actual: &Type, expected: &Type) -> bool {
        if self.types_match(actual, expected) {
            return true;
        }
        Self::type_contains_parameter(actual)
    }

    /// True when any generic type parameter survives inside `ty`.
    pub fn type_contains_parameter(ty: &Type) -> bool {
        match ty {
            Type::TypeParameter { .. } => true,
            Type::Applied { args, .. } => args.iter().any(Self::type_contains_parameter),
            Type::Fn {
                params,
                return_type,
            } => {
                params.iter().any(Self::type_contains_parameter)
                    || Self::type_contains_parameter(return_type)
            }
            Type::Task { output } => Self::type_contains_parameter(output),
            Type::Array { element_type, .. } => Self::type_contains_parameter(element_type),
            Type::Tuple { elements } => elements.iter().any(Self::type_contains_parameter),
            _ => false,
        }
    }


    fn return_types_match(&self, actual: &Type, expected: &Type) -> bool {
        if matches!(actual, Type::Unknown) || matches!(expected, Type::Unknown) {
            return false;
        }

        if self.can_auto_promote(actual, expected) {
            return true;
        }

        match (actual, expected) {
            (Type::TypeParameter { name: actual }, Type::TypeParameter { name: expected }) => {
                actual == expected
            }
            (Type::TypeParameter { .. }, _) | (_, Type::TypeParameter { .. }) => false,
            (
                Type::Fn {
                    params: actual_params,
                    return_type: actual_return,
                },
                Type::Fn {
                    params: expected_params,
                    return_type: expected_return,
                },
            ) => {
                actual_params.len() == expected_params.len()
                    && actual_params
                        .iter()
                        .zip(expected_params.iter())
                        .all(|(actual, expected)| self.return_types_match(actual, expected))
                    && self.return_types_match(actual_return, expected_return)
            }
            (Type::Task { output: actual }, Type::Task { output: expected }) => {
                self.return_types_match(actual, expected)
            }
            (Type::Range, Type::Range) => true,
            (
                Type::Array {
                    element_type: actual_element,
                    size: actual_size,
                },
                Type::Array {
                    element_type: expected_element,
                    size: expected_size,
                },
            ) => {
                self.return_types_match(actual_element, expected_element)
                    && (actual_size.is_none()
                        || expected_size.is_none()
                        || actual_size == expected_size)
            }
            (
                Type::Tuple {
                    elements: actual_elements,
                },
                Type::Tuple {
                    elements: expected_elements,
                },
            ) => {
                actual_elements.len() == expected_elements.len()
                    && actual_elements
                        .iter()
                        .zip(expected_elements.iter())
                        .all(|(actual, expected)| self.return_types_match(actual, expected))
            }
            (
                Type::Tensor {
                    dtype: actual_dtype,
                    rank: actual_rank,
                    dims: actual_dims,
                    layout: actual_layout,
                    device: actual_device,
                },
                Type::Tensor {
                    dtype: expected_dtype,
                    rank: expected_rank,
                    dims: expected_dims,
                    layout: expected_layout,
                    device: expected_device,
                },
            ) => {
                self.return_types_match(actual_dtype, expected_dtype)
                    && (actual_rank.is_none()
                        || expected_rank.is_none()
                        || actual_rank == expected_rank)
                    && Self::tensor_dims_match(actual_dims, expected_dims)
                    && (actual_layout.is_none()
                        || expected_layout.is_none()
                        || actual_layout == expected_layout)
                    && (actual_device.is_none()
                        || expected_device.is_none()
                        || actual_device == expected_device)
            }
            _ => self.types_match(actual, expected),
        }
    }

}

#[cfg(test)]
mod generic_wildcard_tests {
    use crate::pipeline::{CompilationOptions, CompilationPipeline};

    fn compile(source: &str) -> Result<(), Vec<String>> {
        let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
        match pipeline.compile(source, "test.spectra") {
            Ok(result) => {
                assert!(result.errors.is_empty(), "unexpected: {:?}", result.errors);
                Ok(())
            }
            Err(errors) => Err(errors.iter().map(|e| e.to_string()).collect()),
        }
    }

    /// A generic call whose concrete arguments disagree with a shared type
    /// parameter must fail now that `Type::TypeParameter` is no longer a
    /// wildcard inside `types_match`.
    #[test]
    fn generic_call_with_wrong_concrete_argument_fails() {
        let source = r#"
            module test
            func pick<T>(a: T, b: T) returns T {
                return a
            }
            public func main() returns int {
                let value = pick(1, "wrong")
                return 0
            }
        "#;
        let errors = compile(source).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("Argument 2 of function 'pick'")),
            "expected argument mismatch diagnostic, got: {errors:?}"
        );
    }

    #[test]
    fn generic_call_with_consistent_arguments_still_passes() {
        let source = r#"
            module test
            func pick<T>(a: T, b: T) returns T {
                return a
            }
            func identity<T>(value: T) returns T {
                return value
            }
            public func main() returns int {
                let number = pick(1, 2)
                let same = identity(number)
                return same
            }
        "#;
        compile(source).expect("consistent generic calls should compile cleanly");
    }
}
