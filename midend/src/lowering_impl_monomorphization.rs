impl ASTLowering {
    /// Process all pending monomorphization requests
    fn process_monomorphization_requests(&mut self, ir_module: &mut IRModule) {
        // Safety limit: prevent infinite expansion from recursive/mutually-recursive
        // generics (e.g., Foo<T> → Foo<List<T>> → Foo<List<List<T>>> …).
        const MAX_SPECIALIZATIONS: usize = 512;
        let mut total_processed: usize = 0;

        // Process each pending specialization
        while let Some(request) = self.pending_specializations.pop() {
            if total_processed >= MAX_SPECIALIZATIONS {
                eprintln!(
                    "monomorphization limit ({}) reached for '{}'; remaining specializations skipped.",
                    MAX_SPECIALIZATIONS, request.generic_name
                );
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
    fn specialize_function(
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
    fn process_method_specialization(
        &mut self,
        request: &MethodMonomorphizationRequest,
        ir_module: &mut IRModule,
    ) {
        const MAX_METHOD_SPECIALIZATIONS: usize = 512;
        if self.generated_specializations.len() > MAX_METHOD_SPECIALIZATIONS {
            eprintln!(
                "generic impl method specialization limit ({}) reached; remaining specializations skipped.",
                MAX_METHOD_SPECIALIZATIONS
            );
            self.pending_method_specializations.clear();
            return;
        }

        let mangled = request.mangled_name();
        if self.generated_specializations.contains_key(&mangled) {
            return;
        }

        let (base_name, _concrete_ir_types) = match self.instantiated_structs.get(&request.instantiated_struct) {
            Some(entry) => entry.clone(),
            None => {
                // The instantiation may not have been registered yet (e.g. the
                // base struct was used without specialization); fall back to the
                // instantiated name itself with the request's concrete types.
                (request.instantiated_struct.clone(), request.concrete_types.clone())
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
    fn specialize_method(
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
        let base_name = match self
            .instantiated_structs
            .get(instantiated_struct)
            .cloned()
        {
            Some((base, _)) => base,
            None => instantiated_struct.to_string(),
        };
        if !self.struct_definitions.contains_key(instantiated_struct)
            && self.generic_structs.contains_key(&base_name) {
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
    fn substitute_type_in_annotation(
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
                        // Replace with concrete type name
                        let concrete_name = self.ir_type_to_ast_name(concrete_type);
                        segments[0] = concrete_name;
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
    fn ir_type_to_ast_name(&self, ty: &IRType) -> String {
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

    fn ir_type_contains_unknown(ty: &IRType) -> bool {
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
                payload.as_ref().is_some_and(|types| {
                    types.iter().any(Self::ir_type_contains_unknown)
                })
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
    fn type_satisfies_trait(&self, concrete_type: &IRType, trait_name: &str) -> bool {
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

    fn ir_type_satisfies_auto_trait(&self, concrete_type: &IRType, trait_name: &str) -> bool {
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

    fn type_name_fails_auto_trait(&self, name: &str, trait_name: &str) -> bool {
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

    fn merge_array_element_types(&self, left: &IRType, right: &IRType) -> Option<IRType> {
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

    fn infer_array_element_type(&mut self, elements: &[Expression]) -> IRType {
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
