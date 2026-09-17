use super::*;

impl ASTLowering {
    /// Infer the concrete type argument for each type parameter of a generic
    /// function by structurally unifying its parameter annotations with the
    /// actual argument types.
    ///
    /// Binding a type parameter to the whole argument type (the historical
    /// positional inference) is wrong whenever the parameter is nested, e.g.
    /// `func f<T>(items: [T], n: int)`: the old behavior bound `T` to `[int]`
    /// and each recursive call then bound it to a deeper array, producing a
    /// fresh specialization per level until the monomorphization limit.
    ///
    /// Parameters whose type parameter cannot be inferred structurally keep
    /// the positional fallback so existing specializations stay stable.
    pub(crate) fn infer_generic_concrete_types(
        &mut self,
        generic_name: &str,
        arguments: &[Expression],
    ) -> Vec<IRType> {
        let arg_types: Vec<IRType> = arguments
            .iter()
            .map(|arg| self.infer_expr_ir_type(arg))
            .collect();

        let Some(generic_func) = self.generic_functions.get(generic_name) else {
            return arg_types;
        };

        let type_param_names: HashSet<String> = generic_func
            .type_params
            .iter()
            .map(|param| param.name.clone())
            .collect();

        let mut bindings: HashMap<String, IRType> = HashMap::new();
        for (param, arg_type) in generic_func.params.iter().zip(arg_types.iter()) {
            if let Some(annotation) = param.ty.as_ref() {
                self.unify_annotation_with_ir_type(
                    annotation,
                    arg_type,
                    &type_param_names,
                    &mut bindings,
                );
            }
        }

        generic_func
            .type_params
            .iter()
            .enumerate()
            .map(|(idx, type_param)| {
                bindings
                    .get(&type_param.name)
                    .cloned()
                    .or_else(|| arg_types.get(idx).cloned())
                    .unwrap_or(IRType::Unknown)
            })
            .collect()
    }

    /// Recursively bind type parameters of `annotation` from the shape of
    /// `ty`, descending through arrays, tuples, functions, and generic
    /// applications whose names match.
    fn unify_annotation_with_ir_type(
        &self,
        annotation: &TypeAnnotation,
        ty: &IRType,
        type_param_names: &HashSet<String>,
        bindings: &mut HashMap<String, IRType>,
    ) {
        match (&annotation.kind, ty) {
            (TypeAnnotationKind::Simple { segments }, _) if segments.len() == 1 => {
                let name = &segments[0];
                if type_param_names.contains(name) {
                    bindings.entry(name.clone()).or_insert_with(|| ty.clone());
                }
            }
            (
                TypeAnnotationKind::Generic { name, type_args },
                IRType::Array { element_type, .. },
            ) if name == "array" => {
                if let Some(first) = type_args.first() {
                    self.unify_annotation_with_ir_type(
                        first,
                        element_type,
                        type_param_names,
                        bindings,
                    );
                }
            }
            (
                TypeAnnotationKind::Generic { name, type_args },
                IRType::Generic {
                    name: actual_name,
                    args,
                    ..
                },
            ) if name == actual_name => {
                for (type_arg, concrete) in type_args.iter().zip(args.iter()) {
                    self.unify_annotation_with_ir_type(
                        type_arg,
                        concrete,
                        type_param_names,
                        bindings,
                    );
                }
            }
            // A value of a generic struct is often carried as its
            // monomorphized nominal type (`Struct { name: "Chain_int" }`)
            // instead of the `Generic` application. Recover the concrete
            // arguments recorded when the specialization was instantiated.
            (
                TypeAnnotationKind::Generic { name, type_args },
                IRType::Struct {
                    name: actual_name, ..
                },
            ) => {
                if let Some((base, concrete_args)) = self.instantiated_structs.get(actual_name) {
                    if base == name {
                        for (type_arg, concrete) in type_args.iter().zip(concrete_args.iter()) {
                            self.unify_annotation_with_ir_type(
                                type_arg,
                                concrete,
                                type_param_names,
                                bindings,
                            );
                        }
                    }
                }
            }
            (TypeAnnotationKind::Tuple { elements }, IRType::Tuple { elements: concrete }) => {
                for (element, concrete_element) in elements.iter().zip(concrete.iter()) {
                    self.unify_annotation_with_ir_type(
                        element,
                        concrete_element,
                        type_param_names,
                        bindings,
                    );
                }
            }
            (
                TypeAnnotationKind::Function {
                    params,
                    return_type,
                },
                IRType::Function {
                    params: concrete_params,
                    return_type: concrete_return,
                },
            ) => {
                for (param, concrete_param) in params.iter().zip(concrete_params.iter()) {
                    self.unify_annotation_with_ir_type(
                        param,
                        concrete_param,
                        type_param_names,
                        bindings,
                    );
                }
                self.unify_annotation_with_ir_type(
                    return_type,
                    concrete_return,
                    type_param_names,
                    bindings,
                );
            }
            _ => {}
        }
    }

    /// Process all pending monomorphization requests
    pub(crate) fn process_monomorphization_requests(&mut self, ir_module: &mut IRModule) {
        // Safety limit: prevent infinite expansion from recursive/mutually-recursive
        // generics (e.g., Foo<T> → Foo<List<T>> → Foo<List<List<T>>> …).
        const MAX_SPECIALIZATIONS: usize = 512;
        let mut total_processed: usize = 0;

        // Process each pending specialization
        while let Some(request) = self.pending_specializations.pop() {
            if total_processed >= MAX_SPECIALIZATIONS {
                self.error(format!(
                    "E3011: monomorphization limit ({}) exceeded for '{}'; remaining specializations rejected",
                    MAX_SPECIALIZATIONS, request.generic_name
                ));
                self.pending_specializations.clear();
                break;
            }

            let mangled = request.mangled_name();

            // Skip if already generated
            if self.generated_specializations.contains_key(&mangled) {
                continue;
            }

            // Get the generic function AST
            if let Some(generic_func) = self.generic_functions.get(&request.generic_name).cloned() {
                // generating specialization

                // Generate specialized function
                let specialized_func = self.specialize_function(&generic_func, &request);

                // Add to module
                ir_module.add_function(specialized_func.clone());

                // Mark as generated
                self.generated_specializations
                    .insert(mangled.clone(), specialized_func.name);

                total_processed += 1;
            } else {
                eprintln!(
                    "generic function '{}' not found for monomorphization",
                    request.generic_name
                );
            }
        }
    }

    /// Create a specialized version of a generic function
    pub(crate) fn specialize_function(
        &mut self,
        generic_func: &ASTFunction,
        request: &MonomorphizationRequest,
    ) -> IRFunction {
        // Create type substitution map: type_param_name -> IRType
        let mut type_map: HashMap<String, IRType> = HashMap::new();
        for (i, type_param) in generic_func.type_params.iter().enumerate() {
            if let Some(concrete_type) = request.concrete_types.get(i) {
                type_map.insert(type_param.name.clone(), concrete_type.clone());
            }
        }

        // Validate trait bounds
        for (i, type_param) in generic_func.type_params.iter().enumerate() {
            if let Some(concrete_type) = request.concrete_types.get(i) {
                // Check each trait bound
                for bound in &type_param.bounds {
                    if !self.type_satisfies_trait(concrete_type, bound) {
                        let type_name = self.ir_type_to_ast_name(concrete_type);
                        self.error(format!(
                            "Trait bound violation: Type '{}' does not implement trait '{}' required by type parameter '{}'. \
                             Function '{}' requires {} to have trait {}. \
                             Specialization: {}",
                            type_name, bound, type_param.name,
                            generic_func.name, type_param.name, bound,
                            request.mangled_name()
                        ));
                    }
                }
            }
        }

        let mangled_name = request.mangled_name();

        // Create specialized function by copying generic and renaming
        let mut specialized = generic_func.clone();
        specialized.name = mangled_name.clone();
        specialized.type_params.clear(); // Remove generic parameters

        // Substitute type parameters in function signature
        for param in &mut specialized.params {
            if let Some(ref mut ty) = param.ty {
                self.substitute_type_in_annotation(ty, &type_map);
            }
        }

        if let Some(ref mut return_ty) = specialized.return_type {
            self.substitute_type_in_annotation(return_ty, &type_map);
        }

        // Set type substitution map for lowering
        self.type_substitution_map = type_map;

        // Lower the specialized function
        let result = self.lower_function(&specialized);

        // Clear type substitution map after lowering
        self.type_substitution_map.clear();

        result
    }

    /// Process a pending generic impl method specialization request (R-211).
    pub(crate) fn process_method_specialization(
        &mut self,
        request: &MethodMonomorphizationRequest,
        ir_module: &mut IRModule,
    ) {
        const MAX_METHOD_SPECIALIZATIONS: usize = 512;
        if self.generated_specializations.len() > MAX_METHOD_SPECIALIZATIONS {
            self.error(format!(
                "E3011: generic impl method specialization limit ({}) exceeded for '{}' on '{}'; remaining specializations rejected",
                MAX_METHOD_SPECIALIZATIONS, request.method_name, request.instantiated_struct
            ));
            self.pending_method_specializations.clear();
            return;
        }

        let mangled = request.mangled_name();
        if self.generated_specializations.contains_key(&mangled) {
            return;
        }

        let (base_name, _concrete_ir_types) =
            match self.instantiated_structs.get(&request.instantiated_struct) {
                Some(entry) => entry.clone(),
                None => {
                    // The instantiation may not have been registered yet (e.g. the
                    // base struct was used without specialization); fall back to the
                    // instantiated name itself with the request's concrete types.
                    (
                        request.instantiated_struct.clone(),
                        request.concrete_types.clone(),
                    )
                }
            };

        let key = format!("{}_{}", base_name, request.method_name);
        let Some((method, type_params)) = self.generic_impl_methods.get(&key).cloned() else {
            self.error(format!(
                "Generic impl method '{key}' not found for specialization '{mangled}'"
            ));
            return;
        };

        let specialized = self.specialize_method(
            &method,
            &type_params,
            &request.concrete_types,
            &request.instantiated_struct,
        );
        ir_module.add_function(specialized.clone());
        self.generated_specializations
            .insert(mangled.clone(), specialized.name.clone());
    }

    /// Create a specialized copy of a generic impl method with the impl type
    /// parameters substituted by concrete types. `instantiated_struct` is the
    /// concrete struct name used to type `self` and to name the function.
    pub(crate) fn specialize_method(
        &mut self,
        method: &ASTMethod,
        type_params: &[TypeParameter],
        concrete_types: &[IRType],
        instantiated_struct: &str,
    ) -> IRFunction {
        let mut type_map: HashMap<String, IRType> = HashMap::new();
        for (i, type_param) in type_params.iter().enumerate() {
            if let Some(concrete_type) = concrete_types.get(i) {
                type_map.insert(type_param.name.clone(), concrete_type.clone());
            }
        }

        let mut specialized = method.clone();
        for param in &mut specialized.params {
            if let Some(ref mut ty) = param.type_annotation {
                self.substitute_type_in_annotation(ty, &type_map);
            }
        }
        if let Some(ref mut return_ty) = specialized.return_type {
            self.substitute_type_in_annotation(return_ty, &type_map);
        }

        // Ensure the instantiated struct definition exists so `self` fields resolve.
        let base_name = match self.instantiated_structs.get(instantiated_struct).cloned() {
            Some((base, _)) => base,
            None => instantiated_struct.to_string(),
        };
        if !self.struct_definitions.contains_key(instantiated_struct)
            && self.generic_structs.contains_key(&base_name)
        {
            let unknown_args: Vec<TypeAnnotation> = concrete_types
                .iter()
                .map(|ty| TypeAnnotation {
                    kind: TypeAnnotationKind::Simple {
                        segments: vec![self.ir_type_to_ast_name(ty)],
                    },
                    span: Span::dummy(),
                })
                .collect();
            self.ensure_struct_definition(&base_name, &unknown_args);
        }

        self.type_substitution_map = type_map;
        let result = self.lower_method(&specialized, instantiated_struct);
        self.type_substitution_map.clear();
        result
    }

    /// Substitute type parameters in a TypeAnnotation
    pub(crate) fn substitute_type_in_annotation(
        &self,
        annotation: &mut TypeAnnotation,
        type_map: &HashMap<String, IRType>,
    ) {
        use spectra_compiler::ast::TypeAnnotationKind;

        match &mut annotation.kind {
            TypeAnnotationKind::Simple { segments } => {
                // Check if this is a type parameter (e.g., "T")
                if segments.len() == 1 {
                    let name = &segments[0];
                    if let Some(concrete_type) = type_map.get(name) {
                        // Replace the whole annotation with the concrete IR
                        // type: a flat name would drop the structure of
                        // compound types (`Stack<int>` -> bare `Stack`,
                        // `[int]` -> `unknown`) and the re-lowering would
                        // resolve the argument to Unknown.
                        *annotation = self.ir_type_to_annotation(concrete_type);
                    }
                }
            }
            TypeAnnotationKind::Tuple { elements } => {
                // Recursively substitute in tuple elements
                for elem in elements {
                    self.substitute_type_in_annotation(elem, type_map);
                }
            }
            TypeAnnotationKind::Function {
                params,
                return_type,
            } => {
                for param in params {
                    self.substitute_type_in_annotation(param, type_map);
                }
                self.substitute_type_in_annotation(return_type, type_map);
            }
            TypeAnnotationKind::Generic { name: _, type_args } => {
                for arg in type_args {
                    self.substitute_type_in_annotation(arg, type_map);
                }
            }
            TypeAnnotationKind::DynTrait { .. } => {}
        }
    }

    /// Convert IRType to AST type name for substitution
    pub(crate) fn ir_type_to_ast_name(&self, ty: &IRType) -> String {
        match ty {
            IRType::Int => "int".to_string(),
            IRType::Float => "float".to_string(),
            IRType::Bool => "bool".to_string(),
            IRType::String => "string".to_string(),
            IRType::Char => "char".to_string(),
            IRType::Range => "Range".to_string(),
            IRType::Struct { name, .. } => name.clone(),
            IRType::Enum { name, .. } => name.clone(),
            IRType::Generic { name, .. } => name.clone(),
            IRType::Pointer(inner) => format!("ptr<{}>", self.ir_type_to_ast_name(inner)),
            _ => "unknown".to_string(),
        }
    }

    pub(crate) fn ir_type_contains_unknown(ty: &IRType) -> bool {
        match ty {
            IRType::Unknown => true,
            IRType::Pointer(inner) | IRType::Task { output: inner } => {
                Self::ir_type_contains_unknown(inner)
            }
            IRType::Array { element_type, .. } => Self::ir_type_contains_unknown(element_type),
            IRType::Tuple { elements } => elements.iter().any(Self::ir_type_contains_unknown),
            IRType::Struct { fields, .. } => fields
                .iter()
                .any(|(_, field)| Self::ir_type_contains_unknown(field)),
            IRType::Enum { variants, .. } => variants.iter().any(|(_, payload)| {
                payload
                    .as_ref()
                    .is_some_and(|types| types.iter().any(Self::ir_type_contains_unknown))
            }),
            IRType::Generic {
                args,
                representation,
                ..
            } => {
                args.iter().any(Self::ir_type_contains_unknown)
                    || Self::ir_type_contains_unknown(representation)
            }
            IRType::Function {
                params,
                return_type,
            } => {
                params.iter().any(Self::ir_type_contains_unknown)
                    || Self::ir_type_contains_unknown(return_type)
            }
            IRType::Tensor { dtype, .. } => Self::ir_type_contains_unknown(dtype),
            IRType::Void
            | IRType::Int
            | IRType::Float
            | IRType::ExactInt { .. }
            | IRType::ExactFloat { .. }
            | IRType::Bool
            | IRType::String
            | IRType::Char
            | IRType::Range
            | IRType::DynTrait { .. } => false,
        }
    }

    /// Check if a concrete type satisfies a trait bound
    pub(crate) fn type_satisfies_trait(&self, concrete_type: &IRType, trait_name: &str) -> bool {
        if trait_name == "Send" || trait_name == "Sync" {
            return self.ir_type_satisfies_auto_trait(concrete_type, trait_name);
        }
        let type_name = self.ir_type_to_ast_name(concrete_type);

        // Check if we have recorded this implementation
        let key = (type_name, trait_name.to_string());
        self.trait_implementations
            .get(&key)
            .copied()
            .unwrap_or(false)
    }

    pub(crate) fn ir_type_satisfies_auto_trait(
        &self,
        concrete_type: &IRType,
        trait_name: &str,
    ) -> bool {
        match concrete_type {
            IRType::Unknown => false,
            IRType::Void
            | IRType::Int
            | IRType::Float
            | IRType::ExactInt { .. }
            | IRType::ExactFloat { .. }
            | IRType::Bool
            | IRType::String
            | IRType::Char
            | IRType::Range
            | IRType::Pointer(_) => true,
            IRType::Array {
                element_type: element,
                ..
            }
            | IRType::Task { output: element } => {
                self.ir_type_satisfies_auto_trait(element, trait_name)
            }
            IRType::Tuple { elements } => elements
                .iter()
                .all(|element| self.ir_type_satisfies_auto_trait(element, trait_name)),
            IRType::Struct { name, fields } => {
                if self.type_name_fails_auto_trait(name, trait_name) {
                    return false;
                }
                fields
                    .iter()
                    .all(|(_, field)| self.ir_type_satisfies_auto_trait(field, trait_name))
            }
            IRType::Enum { name, variants } => {
                if self.type_name_fails_auto_trait(name, trait_name) {
                    return false;
                }
                variants.iter().all(|(_, payload)| {
                    payload
                        .as_ref()
                        .map(|types| {
                            types
                                .iter()
                                .all(|ty| self.ir_type_satisfies_auto_trait(ty, trait_name))
                        })
                        .unwrap_or(true)
                })
            }
            IRType::Generic {
                name,
                args,
                representation,
            } => {
                if self.type_name_fails_auto_trait(name, trait_name) {
                    return false;
                }
                args.iter()
                    .all(|arg| self.ir_type_satisfies_auto_trait(arg, trait_name))
                    && self.ir_type_satisfies_auto_trait(representation, trait_name)
            }
            IRType::Function { .. } => false,
            IRType::DynTrait {
                trait_name: dyn_name,
                auto_traits,
            } => {
                auto_traits.iter().any(|bound| bound == trait_name)
                    && !self.type_name_fails_auto_trait(dyn_name, trait_name)
            }
            IRType::Tensor { .. } => true,
        }
    }

    pub(crate) fn type_name_fails_auto_trait(&self, name: &str, trait_name: &str) -> bool {
        match trait_name {
            "Send" => matches!(
                name,
                "RefCell"
                    | "Cell"
                    | "UnsafeCell"
                    | "Rc"
                    | "RawPtr"
                    | "LocalOnly"
                    | "NonSend"
                    | "NotSend"
                    | "LocalHandle"
            ),
            "Sync" => matches!(
                name,
                "RefCell" | "Cell" | "UnsafeCell" | "Rc" | "RawPtr" | "LocalOnly" | "LocalHandle"
            ),
            _ => false,
        }
    }

    pub(crate) fn merge_array_element_types(
        &self,
        left: &IRType,
        right: &IRType,
    ) -> Option<IRType> {
        if left == right {
            return Some(left.clone());
        }

        match (left, right) {
            (IRType::Int, IRType::Float) | (IRType::Float, IRType::Int) => Some(IRType::Float),
            (IRType::Pointer(l), IRType::Pointer(r)) => self
                .merge_array_element_types(l.as_ref(), r.as_ref())
                .map(|merged| IRType::Pointer(Box::new(merged))),
            (
                IRType::Array {
                    element_type: l_elem,
                    size: l_size,
                },
                IRType::Array {
                    element_type: r_elem,
                    size: r_size,
                },
            ) => {
                if l_size != r_size {
                    None
                } else {
                    self.merge_array_element_types(l_elem.as_ref(), r_elem.as_ref())
                        .map(|merged| IRType::Array {
                            element_type: Box::new(merged),
                            size: *l_size,
                        })
                }
            }
            (
                IRType::Struct {
                    name: l_name,
                    fields: l_fields,
                },
                IRType::Struct {
                    name: r_name,
                    fields: r_fields,
                },
            ) => {
                if l_name == r_name && l_fields == r_fields {
                    Some(IRType::Struct {
                        name: l_name.clone(),
                        fields: l_fields.clone(),
                    })
                } else {
                    None
                }
            }
            (
                IRType::Enum {
                    name: l_name,
                    variants: l_variants,
                },
                IRType::Enum {
                    name: r_name,
                    variants: r_variants,
                },
            ) => {
                if l_name == r_name && l_variants == r_variants {
                    Some(IRType::Enum {
                        name: l_name.clone(),
                        variants: l_variants.clone(),
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub(crate) fn infer_array_element_type(&mut self, elements: &[Expression]) -> IRType {
        if elements.is_empty() {
            if let Some(annotation) = self.current_expected_annotation.as_ref() {
                if let IRType::Array { element_type, .. } = self.lower_type_annotation(annotation) {
                    if !Self::ir_type_contains_unknown(&element_type) {
                        return *element_type;
                    }
                }
            }
            return IRType::Unknown;
        }

        let mut element_type = self.infer_expr_ir_type(&elements[0]);

        for expr in elements.iter().skip(1) {
            let next_type = self.infer_expr_ir_type(expr);
            match self.merge_array_element_types(&element_type, &next_type) {
                Some(merged) => {
                    element_type = merged;
                }
                None => {
                    self.error("array literal elements have incompatible types");
                    return IRType::Unknown;
                }
            }
        }

        element_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monomorphization_overflow_is_a_coded_error() {
        let span = spectra_compiler::span::Span::dummy();
        let mut lowering = ASTLowering::new();
        lowering.generic_functions.insert(
            "synth_id".to_string(),
            spectra_compiler::ast::Function {
                name: "synth_id".to_string(),
                span,
                visibility: spectra_compiler::ast::Visibility::Private,
                attributes: Vec::new(),
                is_async: false,
                type_params: vec![spectra_compiler::ast::TypeParameter {
                    name: "T".to_string(),
                    bounds: Vec::new(),
                    span,
                }],
                params: Vec::new(),
                return_type: None,
                body: spectra_compiler::ast::Block {
                    span,
                    statements: Vec::new(),
                },
            },
        );
        for i in 0..513 {
            lowering
                .pending_specializations
                .push(MonomorphizationRequest {
                    generic_name: "synth_id".to_string(),
                    concrete_types: vec![IRType::Struct {
                        name: format!("OverflowProbe{i}"),
                        fields: Vec::new(),
                    }],
                });
        }
        let mut module = IRModule::new("overflow_probe");
        lowering.process_monomorphization_requests(&mut module);
        assert!(
            lowering.pending_specializations.is_empty(),
            "overflow must drain the pending queue"
        );
        let coded = lowering.errors.iter().any(|error| {
            error.message.starts_with("E3011")
                && error.message.contains("synth_id")
                && error.message.contains("512")
        });
        assert!(
            coded,
            "expected E3011 overflow error naming synth_id and 512, got: {:?}",
            lowering
                .errors
                .iter()
                .map(|error| &error.message)
                .collect::<Vec<_>>()
        );
    }
}
