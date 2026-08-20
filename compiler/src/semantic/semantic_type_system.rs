impl SemanticAnalyzer {
    fn module_export_names(exports: &ModuleExports) -> String {
        let mut names = exports
            .functions
            .keys()
            .chain(exports.types.keys())
            .cloned()
            .collect::<Vec<_>>();
        for (type_name, methods) in &exports.methods {
            for method_name in methods.keys() {
                names.push(format!("{}::{}", type_name, method_name));
            }
        }
        names.sort();
        names.dedup();

        if names.is_empty() {
            "<none>".to_string()
        } else {
            names.join(", ")
        }
    }

    fn namespace_export_names(&self, namespace: &str) -> Vec<String> {
        let prefix = format!("{}.", namespace);
        let mut names = self
            .functions
            .keys()
            .filter_map(|name| name.strip_prefix(&prefix).map(str::to_string))
            .filter(|name| !name.contains('.'))
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        names
    }

    fn missing_method_diagnostic(&self, type_name: &str, method_name: &str) -> String {
        let mut candidates = self
            .methods
            .get(type_name)
            .map(|methods| methods.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        candidates.sort();
        candidates.dedup();

        if candidates.is_empty() {
            format!(
                "Method '{}' not found for type '{}'; candidate impl blocks in scope: <none>",
                method_name, type_name
            )
        } else {
            format!(
                "Method '{}' not found for type '{}'; candidate impl blocks in scope for '{}': {}",
                method_name,
                type_name,
                type_name,
                candidates.join(", ")
            )
        }
    }

    fn report_unknown_qualified_member(
        &mut self,
        module_path: &str,
        member_name: &str,
        exports: &ModuleExports,
        span: Span,
    ) {
        self.error_coded_with_hint(
            "E011",
            format!(
                "Module '{}' does not export member '{}'",
                module_path, member_name
            ),
            span,
            format!(
                "Available exports from '{}': {}",
                module_path,
                Self::module_export_names(exports)
            ),
        );
    }

    fn report_unknown_qualified_member_names(
        &mut self,
        module_path: &str,
        member_name: &str,
        export_names: Vec<String>,
        span: Span,
    ) {
        let exports = if export_names.is_empty() {
            "<none>".to_string()
        } else {
            export_names.join(", ")
        };
        self.error_coded_with_hint(
            "E011",
            format!(
                "Module '{}' does not export member '{}'",
                module_path, member_name
            ),
            span,
            format!("Available exports from '{}': {}", module_path, exports),
        );
    }

    fn substitute_type_parameters(&self, ty: &Type, substitutions: &HashMap<String, Type>) -> Type {
        match ty {
            Type::TypeParameter { name } => substitutions
                .get(name)
                .cloned()
                .unwrap_or_else(|| ty.clone()),
            Type::Array { element_type, size } => Type::Array {
                element_type: Box::new(
                    self.substitute_type_parameters(element_type, substitutions),
                ),
                size: *size,
            },
            Type::Tuple { elements } => Type::Tuple {
                elements: elements
                    .iter()
                    .map(|elem| self.substitute_type_parameters(elem, substitutions))
                    .collect(),
            },
            Type::Applied { name, args } => Type::Applied {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.substitute_type_parameters(arg, substitutions))
                    .collect(),
            },
            Type::Fn {
                params,
                return_type,
            } => Type::Fn {
                params: params
                    .iter()
                    .map(|param| self.substitute_type_parameters(param, substitutions))
                    .collect(),
                return_type: Box::new(self.substitute_type_parameters(return_type, substitutions)),
            },
            other => other.clone(),
        }
    }

    fn validate_method_call_signature(
        &mut self,
        method_name: &str,
        signature: &FunctionSignature,
        receiver_type: &Type,
        arguments: &[Expression],
        call_span: Span,
    ) {
        let has_self = signature.self_kind.is_some();

        if !has_self {
            self.error_coded_with_hint(
                "E018",
                format!(
                    "Method '{}' does not take 'self'; call it as an associated function",
                    method_name
                ),
                call_span,
                "Call this item as `Type::method(...)` or update the signature to accept `self`.",
            );
            return;
        }

        if let Some(expected_self_type) = signature.params.first() {
            if !self.types_match(receiver_type, expected_self_type) {
                self.error_with_details(
                    format!(
                        "Method '{}' expects receiver of type {:?}, but found {:?}",
                        method_name, expected_self_type, receiver_type
                    ),
                    call_span,
                    format!("receiver expression resolved to {:?}", receiver_type),
                    "Convert or borrow the receiver to match the method signature.",
                );
            }
        }

        let arg_offset = if has_self { 1 } else { 0 };
        let expected_args = signature.params.len().saturating_sub(arg_offset);

        if arguments.len() != expected_args {
            self.error_coded_with_details(
                "E023",
                format!(
                    "Method '{}' expects {} argument(s), but {} were provided",
                    method_name,
                    expected_args,
                    arguments.len()
                ),
                call_span,
                format!("expected {} argument(s) after the receiver", expected_args),
                "Review the method's signature and adjust the call arity.",
            );
        }

        for (i, arg) in arguments.iter().enumerate() {
            let arg_type = self.infer_expression_type(arg);
            let expected_index = i + arg_offset;
            if let Some(expected_type) = signature.params.get(expected_index) {
                if !self.types_match(&arg_type, expected_type) {
                    let hint = self.conversion_hint(&arg_type, expected_type);
                    self.push_semantic_error(
                        format!(
                            "Method '{}' argument {} has type {:?}, but {:?} was expected",
                            method_name,
                            i + 1,
                            arg_type,
                            expected_type
                        ),
                        arg.span,
                        Some(format!("argument {} resolved to {:?}", i + 1, arg_type)),
                        hint,
                    );
                }
            }
        }
    }

    fn type_annotation_to_type(&self, type_ann: &Option<crate::ast::TypeAnnotation>) -> Type {
        use crate::ast::TypeAnnotationKind;

        match type_ann {
            Some(ann) => match &ann.kind {
                TypeAnnotationKind::Simple { segments } if segments.len() == 1 => {
                    match segments[0].as_str() {
                        primitive if Self::primitive_numeric_alias(primitive).is_some() => {
                            Self::primitive_numeric_alias(primitive).unwrap()
                        }
                        "bool" => Type::Bool,
                        "string" => Type::String,
                        "char" => Type::Char,
                        "array" => Type::Array {
                            element_type: Box::new(Type::Unknown),
                            size: None,
                        },
                        "Range" => Type::Range,
                        "Self" => Type::SelfType, // Self type
                        other => {
                            if self.is_generic_param(other) {
                                Type::TypeParameter {
                                    name: other.to_string(),
                                }
                            } else if self.struct_infos.contains_key(other) {
                                Type::Struct {
                                    name: other.to_string(),
                                }
                            } else if self.enum_infos.contains_key(other) {
                                Type::Enum {
                                    name: other.to_string(),
                                }
                            } else if let Some(alias) = self.type_aliases.get(other) {
                                self.type_annotation_to_type(&Some(alias.clone()))
                            } else {
                                Type::Unknown
                            }
                        }
                    }
                }
                TypeAnnotationKind::Tuple { elements } => {
                    let element_types: Vec<Type> = elements
                        .iter()
                        .map(|elem_ann| self.type_annotation_to_type(&Some(elem_ann.clone())))
                        .collect();
                    Type::Tuple {
                        elements: element_types,
                    }
                }
                TypeAnnotationKind::Function {
                    params,
                    return_type,
                } => {
                    let param_types = params
                        .iter()
                        .map(|param_ann| self.type_annotation_to_type(&Some(param_ann.clone())))
                        .collect();
                    let return_type = self.type_annotation_to_type(&Some((**return_type).clone()));
                    Type::Fn {
                        params: param_types,
                        return_type: Box::new(return_type),
                    }
                }
                TypeAnnotationKind::Generic { name, type_args }
                    if name == "Tensor" && !type_args.is_empty() =>
                {
                    let dtype = self.type_annotation_to_type(&Some(type_args[0].clone()));
                    let meta = Self::parse_tensor_metadata(&type_args[1..]);
                    Type::Tensor {
                        dtype: Box::new(dtype),
                        rank: meta.rank,
                        dims: meta.dims,
                        layout: meta.layout,
                        device: meta.device,
                    }
                }
                TypeAnnotationKind::Generic { name, type_args } if name == "array" => {
                    let element_type = type_args
                        .first()
                        .map(|ann| self.type_annotation_to_type(&Some(ann.clone())))
                        .unwrap_or(Type::Unknown);
                    Type::Array {
                        element_type: Box::new(element_type),
                        size: None,
                    }
                }
                TypeAnnotationKind::Generic { name, type_args } if name == "Task" => {
                    let output = type_args
                        .first()
                        .map(|ann| self.type_annotation_to_type(&Some(ann.clone())))
                        .unwrap_or(Type::Unknown);
                    Type::Task {
                        output: Box::new(output),
                    }
                }
                TypeAnnotationKind::Generic { name, type_args } if name == "Box" => {
                    match type_args.first() {
                        Some(crate::ast::TypeAnnotation {
                            kind:
                                TypeAnnotationKind::DynTrait {
                                    trait_name,
                                    auto_traits,
                                },
                            ..
                        }) => Type::DynTrait {
                            trait_name: trait_name.clone(),
                            auto_traits: auto_traits.clone(),
                        },
                        _ => Type::Unknown,
                    }
                }
                TypeAnnotationKind::Generic { name, type_args }
                    if self.generic_enums.contains_key(name) =>
                {
                    Type::Applied {
                        name: name.clone(),
                        args: type_args
                            .iter()
                            .map(|arg| self.type_annotation_to_type(&Some(arg.clone())))
                            .collect(),
                    }
                }
                TypeAnnotationKind::Generic { name, type_args }
                    if self.generic_structs.contains_key(name) =>
                {
                    Type::Applied {
                        name: name.clone(),
                        args: type_args
                            .iter()
                            .map(|arg| self.type_annotation_to_type(&Some(arg.clone())))
                            .collect(),
                    }
                }
                TypeAnnotationKind::DynTrait {
                    trait_name,
                    auto_traits,
                } => Type::DynTrait {
                    trait_name: trait_name.clone(),
                    auto_traits: auto_traits.clone(),
                },
                _ => Type::Unknown,
            },
            None => Type::Unknown,
        }
    }

    fn async_task_type(is_async: bool, output: Type) -> Type {
        if is_async {
            Type::Task {
                output: Box::new(output),
            }
        } else {
            output
        }
    }

    fn type_mangle_part_from_type(&self, ty: &Type) -> String {
        match ty {
            Type::Array { element_type, size } => match size {
                Some(size) => format!(
                    "array_{}_{}",
                    self.type_mangle_part_from_type(element_type),
                    size
                ),
                None => format!("array_{}", self.type_mangle_part_from_type(element_type)),
            },
            Type::Tuple { elements } => format!(
                "tuple_{}",
                elements
                    .iter()
                    .map(|element| self.type_mangle_part_from_type(element))
                    .collect::<Vec<_>>()
                    .join("_")
            ),
            Type::Task { output } => {
                format!("Task_{}", self.type_mangle_part_from_type(output))
            }
            Type::Applied { name, args } => format!(
                "{}_{}",
                name,
                args.iter()
                    .map(|arg| self.type_mangle_part_from_type(arg))
                    .collect::<Vec<_>>()
                    .join("_")
            ),
            Type::DynTrait {
                trait_name,
                auto_traits,
            } => format!(
                "dyn_{}{}",
                trait_name,
                auto_traits
                    .iter()
                    .map(|bound| format!("_{}", bound))
                    .collect::<String>()
            ),
            _ => type_name(ty),
        }
    }

    /// Return the compatibility nominal key used by the existing struct/enum
    /// registries.  The semantic type itself remains structural (`Applied`),
    /// while lookup tables may still use their specialized mangled key during
    /// the migration.
    fn nominal_lookup_name(&self, ty: &Type) -> Option<String> {
        match ty {
            Type::Struct { name } | Type::Enum { name } => Some(name.clone()),
            Type::Applied { name, args } => {
                if args.is_empty() {
                    Some(name.clone())
                } else {
                    Some(format!(
                        "{}_{}",
                        name,
                        args.iter()
                            .map(|arg| self.type_mangle_part_from_type(arg))
                            .collect::<Vec<_>>()
                            .join("_")
                    ))
                }
            }
            _ => None,
        }
    }

    /// R-211: resolve the method signature for an instantiated generic struct
    /// (e.g. `Par_int`) from the template impl (registered under `Par`),
    /// substituting the type parameters with the concrete type arguments.
    fn instantiated_method_signature(
        &self,
        type_name: &str,
        method_name: &str,
    ) -> Option<FunctionSignature> {
        let base_name = self
            .generic_structs
            .keys()
            .find(|base| {
                type_name.len() > base.len() + 1
                    && type_name.starts_with(&format!("{}_", base))
            })?
            .clone();
        let args_part = &type_name[base_name.len() + 1..];
        let concrete_types: Vec<Type> = args_part
            .split('_')
            .map(|part| self.type_from_mangle_part(part))
            .collect();
        let type_params = self
            .generic_structs
            .get(&base_name)?
            .0
            .clone();

        let signature = self
            .methods
            .get(&base_name)?
            .get(method_name)?
            .clone();

        let mut params = signature.params.clone();
        if signature.self_kind.is_some() && !params.is_empty() {
            params[0] = Type::Struct {
                name: type_name.to_string(),
            };
        }
        let return_type = self.substitute_generic_types(&signature.return_type, &type_params, &concrete_types);
        for param in params.iter_mut().skip(1) {
            *param = self.substitute_generic_types(param, &type_params, &concrete_types);
        }

        Some(FunctionSignature {
            params,
            return_type,
            self_kind: signature.self_kind,
            is_async: signature.is_async,
        })
    }

    /// Substitute type parameters in a `Type` using the type parameter names and
    /// the concrete types in declaration order.
    fn substitute_generic_types(
        &self,
        ty: &Type,
        type_params: &[crate::ast::TypeParameter],
        concrete_types: &[Type],
    ) -> Type {
        match ty {
            Type::TypeParameter { name } => type_params
                .iter()
                .position(|p| &p.name == name)
                .and_then(|idx| concrete_types.get(idx).cloned())
                .unwrap_or_else(|| ty.clone()),
            Type::Struct { name } => {
                if let Some(idx) = type_params.iter().position(|p| &p.name == name) {
                    return concrete_types.get(idx).cloned().unwrap_or_else(|| ty.clone());
                }
                self.substitute_mangled_generic_name(name, type_params, concrete_types)
                    .map(|name| Type::Struct { name })
                    .unwrap_or_else(|| ty.clone())
            }
            Type::Enum { name } => self
                .substitute_mangled_generic_name(name, type_params, concrete_types)
                .map(|name| Type::Enum { name })
                .unwrap_or_else(|| ty.clone()),
            Type::Applied { name, args } => Type::Applied {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.substitute_generic_types(arg, type_params, concrete_types))
                    .collect(),
            },
            Type::Array { element_type, size } => Type::Array {
                element_type: Box::new(self.substitute_generic_types(
                    element_type,
                    type_params,
                    concrete_types,
                )),
                size: *size,
            },
            Type::Tuple { elements } => Type::Tuple {
                elements: elements
                    .iter()
                    .map(|element| {
                        self.substitute_generic_types(element, type_params, concrete_types)
                    })
                    .collect(),
            },
            Type::Fn {
                params,
                return_type,
            } => Type::Fn {
                params: params
                    .iter()
                    .map(|param| {
                        self.substitute_generic_types(param, type_params, concrete_types)
                    })
                    .collect(),
                return_type: Box::new(self.substitute_generic_types(
                    return_type,
                    type_params,
                    concrete_types,
                )),
            },
            Type::Task { output } => Type::Task {
                output: Box::new(self.substitute_generic_types(
                    output,
                    type_params,
                    concrete_types,
                )),
            },
            other => other.clone(),
        }
    }

    fn substitute_mangled_generic_name(
        &self,
        name: &str,
        type_params: &[crate::ast::TypeParameter],
        concrete_types: &[Type],
    ) -> Option<String> {
        let mut candidates = self
            .generic_structs
            .keys()
            .chain(self.generic_enums.keys())
            .filter_map(|base| {
                name.strip_prefix(&format!("{}_", base))
                    .map(|suffix| (base.as_str(), suffix))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0.len()));

        for (base, suffix) in candidates {
            let generic_param_count = self
                .generic_structs
                .get(base)
                .map(|(params, _)| params.len())
                .or_else(|| self.generic_enums.get(base).map(|(params, _)| params.len()))?;
            let parts = if generic_param_count == 1 {
                vec![suffix]
            } else {
                suffix.split('_').collect::<Vec<_>>()
            };
            if parts.len() != generic_param_count {
                continue;
            }

            let mut changed = false;
            let mapped_parts = parts
                .into_iter()
                .map(|part| {
                    if let Some(idx) = type_params.iter().position(|param| param.name == part) {
                        if let Some(concrete) = concrete_types.get(idx) {
                            changed = true;
                            return self.type_mangle_part_from_type(concrete);
                        }
                    }
                    part.to_string()
                })
                .collect::<Vec<_>>();
            if changed {
                return Some(format!("{}_{}", base, mapped_parts.join("_")));
            }
        }
        None
    }

    fn type_from_mangle_part(&self, part: &str) -> Type {        match part {
            "int" => Type::Int,
            "float" => Type::Float,
            "i8" => Type::ExactInt { signed: true, width: IntWidth::I8 },
            "i16" => Type::ExactInt { signed: true, width: IntWidth::I16 },
            "i32" => Type::ExactInt { signed: true, width: IntWidth::I32 },
            "i64" => Type::ExactInt { signed: true, width: IntWidth::I64 },
            "isize" => Type::ExactInt { signed: true, width: IntWidth::Isize },
            "u8" => Type::ExactInt { signed: false, width: IntWidth::I8 },
            "u16" => Type::ExactInt { signed: false, width: IntWidth::I16 },
            "u32" => Type::ExactInt { signed: false, width: IntWidth::I32 },
            "u64" => Type::ExactInt { signed: false, width: IntWidth::I64 },
            "usize" => Type::ExactInt { signed: false, width: IntWidth::Usize },
            "f32" => Type::ExactFloat { width: FloatWidth::F32 },
            "f64" => Type::ExactFloat { width: FloatWidth::F64 },
            "bool" => Type::Bool,
            "string" => Type::String,
            "char" => Type::Char,
            other if self.struct_infos.contains_key(other) => Type::Struct {
                name: other.to_string(),
            },
            other
                if self.enum_infos.contains_key(other)
                    || self.generic_enums.contains_key(other) =>
            {
                Type::Enum {
                    name: other.to_string(),
                }
            }
            other => Type::Enum {
                name: other.to_string(),
            },
        }
    }

    fn specialized_enum_context(
        &self,
        enum_type_name: &str,
    ) -> Option<(String, EnumInfo, HashMap<String, Type>)> {
        let mut candidates = self
            .generic_enums
            .iter()
            .filter_map(|(base, (params, _))| {
                let prefix = format!("{}_", base);
                enum_type_name
                    .strip_prefix(&prefix)
                    .map(|suffix| (base.clone(), params.clone(), suffix.to_string()))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0.len()));

        for (base, params, suffix) in candidates {
            let parts = if params.len() == 1 {
                vec![suffix.as_str()]
            } else {
                suffix.split('_').collect::<Vec<_>>()
            };
            if parts.len() != params.len() {
                continue;
            }
            let Some(info) = self.enum_infos.get(&base).cloned() else {
                continue;
            };
            let substitutions = params
                .iter()
                .zip(parts.iter())
                .map(|(param, part)| (param.name.clone(), self.type_from_mangle_part(part)))
                .collect::<HashMap<_, _>>();
            return Some((base, info, substitutions));
        }

        if let Some(info) = self.enum_infos.get(enum_type_name) {
            return Some((enum_type_name.to_string(), info.clone(), HashMap::new()));
        }

        None
    }

    /// Resolve either the legacy mangled nominal form or the structural
    /// `Enum<T, ...>` form used by the semantic type system.
    fn specialized_enum_context_for_type(
        &self,
        ty: &Type,
    ) -> Option<(String, EnumInfo, HashMap<String, Type>)> {
        match ty {
            Type::Enum { name } => self.specialized_enum_context(name),
            Type::Applied { name, args } => {
                let (params, _) = self.generic_enums.get(name)?;
                if params.len() != args.len() {
                    return None;
                }
                let info = self.enum_infos.get(name)?.clone();
                let substitutions = params
                    .iter()
                    .zip(args.iter())
                    .map(|(param, arg)| (param.name.clone(), arg.clone()))
                    .collect();
                Some((name.clone(), info, substitutions))
            }
            _ => None,
        }
    }

    fn specialized_struct_context(
        &self,
        struct_type_name: &str,
    ) -> Option<(String, StructInfo, HashMap<String, Type>)> {
        let mut candidates = self
            .generic_structs
            .iter()
            .filter_map(|(base, (params, _))| {
                let prefix = format!("{}_", base);
                struct_type_name
                    .strip_prefix(&prefix)
                    .map(|suffix| (base.clone(), params.clone(), suffix.to_string()))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0.len()));

        for (base, params, suffix) in candidates {
            let parts = if params.len() == 1 {
                vec![suffix.as_str()]
            } else {
                suffix.split('_').collect::<Vec<_>>()
            };
            if parts.len() != params.len() {
                continue;
            }
            let Some(info) = self.struct_infos.get(&base).cloned() else {
                continue;
            };
            let substitutions = params
                .iter()
                .zip(parts.iter())
                .map(|(param, part)| (param.name.clone(), self.type_from_mangle_part(part)))
                .collect::<HashMap<_, _>>();
            return Some((base, info, substitutions));
        }

        self.struct_infos
            .get(struct_type_name)
            .cloned()
            .map(|info| (struct_type_name.to_string(), info, HashMap::new()))
    }

    /// Resolve either the legacy mangled nominal form or the structural
    /// `Struct<T, ...>` form used by the semantic type system.
    fn specialized_struct_context_for_type(
        &self,
        ty: &Type,
    ) -> Option<(String, StructInfo, HashMap<String, Type>)> {
        match ty {
            Type::Struct { name } => self.specialized_struct_context(name),
            Type::Applied { name, args } => {
                let (params, _) = self.generic_structs.get(name)?;
                if params.len() != args.len() {
                    return None;
                }
                let info = self.struct_infos.get(name)?.clone();
                let substitutions = params
                    .iter()
                    .zip(args.iter())
                    .map(|(param, arg)| (param.name.clone(), arg.clone()))
                    .collect();
                Some((name.clone(), info, substitutions))
            }
            _ => None,
        }
    }

    fn type_annotation_to_type_with_substitutions(
        &self,
        ann: &crate::ast::TypeAnnotation,
        substitutions: &HashMap<String, Type>,
    ) -> Type {
        use crate::ast::TypeAnnotationKind;

        match &ann.kind {
            TypeAnnotationKind::Simple { segments } if segments.len() == 1 => substitutions
                .get(&segments[0])
                .cloned()
                .unwrap_or_else(|| self.type_annotation_to_type(&Some(ann.clone()))),
            TypeAnnotationKind::Tuple { elements } => Type::Tuple {
                elements: elements
                    .iter()
                    .map(|elem| {
                        self.type_annotation_to_type_with_substitutions(elem, substitutions)
                    })
                    .collect(),
            },
            TypeAnnotationKind::Function {
                params,
                return_type,
            } => Type::Fn {
                params: params
                    .iter()
                    .map(|param| {
                        self.type_annotation_to_type_with_substitutions(param, substitutions)
                    })
                    .collect(),
                return_type: Box::new(
                    self.type_annotation_to_type_with_substitutions(return_type, substitutions),
                ),
            },
            TypeAnnotationKind::Generic { name, type_args } if name == "array" => {
                let element_type = type_args
                    .first()
                    .map(|arg| self.type_annotation_to_type_with_substitutions(arg, substitutions))
                    .unwrap_or(Type::Unknown);
                Type::Array {
                    element_type: Box::new(element_type),
                    size: None,
                }
            }
            TypeAnnotationKind::Generic { name, type_args } if name == "Task" => {
                let output = type_args
                    .first()
                    .map(|arg| self.type_annotation_to_type_with_substitutions(arg, substitutions))
                    .unwrap_or(Type::Unknown);
                Type::Task {
                    output: Box::new(output),
                }
            }
            TypeAnnotationKind::Generic { name, type_args } if name == "Box" => {
                match type_args.first() {
                    Some(crate::ast::TypeAnnotation {
                        kind:
                            TypeAnnotationKind::DynTrait {
                                trait_name,
                                auto_traits,
                            },
                        ..
                    }) => Type::DynTrait {
                        trait_name: trait_name.clone(),
                        auto_traits: auto_traits.clone(),
                    },
                    _ => Type::Unknown,
                }
            }
            TypeAnnotationKind::Generic { name, type_args }
                if self.generic_structs.contains_key(name) =>
            {
                let concrete_types = type_args
                    .iter()
                    .map(|arg| {
                        self.type_annotation_to_type_with_substitutions(arg, substitutions)
                    })
                    .collect::<Vec<_>>();
                Type::Applied {
                    name: name.clone(),
                    args: concrete_types,
                }
            }
            TypeAnnotationKind::Generic { name, type_args }
                if self.generic_enums.contains_key(name) =>
            {
                let concrete_types = type_args
                    .iter()
                    .map(|arg| {
                        self.type_annotation_to_type_with_substitutions(arg, substitutions)
                    })
                    .collect::<Vec<_>>();
                Type::Applied {
                    name: name.clone(),
                    args: concrete_types,
                }
            }
            _ => self.type_annotation_to_type(&Some(ann.clone())),
        }
    }

    /// Like `type_annotation_to_type` but emits a semantic error when a named
    /// type annotation resolves to `Unknown` (i.e. the type is not declared).
    /// Use this in pass-2 body analysis where all user types must already be
    /// registered; do **not** use it in pass-1 declaration collection.
    fn type_annotation_to_type_checked(
        &mut self,
        type_ann: &Option<crate::ast::TypeAnnotation>,
    ) -> Type {
        use crate::ast::TypeAnnotationKind;

        let resolved = self.type_annotation_to_type(type_ann);

        if let Some(ann) = type_ann {
            match &ann.kind {
                TypeAnnotationKind::Tuple { elements } => {
                    for element in elements {
                        self.type_annotation_to_type_checked(&Some(element.clone()));
                    }
                }
                TypeAnnotationKind::Function {
                    params,
                    return_type,
                } => {
                    for param in params {
                        self.type_annotation_to_type_checked(&Some(param.clone()));
                    }
                    self.type_annotation_to_type_checked(&Some((**return_type).clone()));
                }
                TypeAnnotationKind::Generic { name, type_args } => {
                    let known_generic = matches!(name.as_str(), "Tensor" | "array" | "Task" | "Box")
                        || self.generic_enums.contains_key(name)
                        || self.generic_structs.contains_key(name);
                    if !known_generic {
                        self.error_coded_with_hint(
                            "E010",
                            format!("Unknown generic type '{}'", name),
                            ann.span,
                            "Declare the generic type or use one of the supported standard types.",
                        );
                    }

                    // Tensor metadata arguments (rank, dimensions, layout and
                    // device) are not types. Every other generic argument is
                    // recursively checked so `Option<MissingType>` cannot
                    // become a mangled `Option_unknown` type silently.
                    let checked_args = if name == "Tensor" {
                        type_args.iter().take(1).cloned().collect::<Vec<_>>()
                    } else {
                        type_args.clone()
                    };
                    for arg in checked_args {
                        self.type_annotation_to_type_checked(&Some(arg));
                    }
                }
                TypeAnnotationKind::Simple { .. } | TypeAnnotationKind::DynTrait { .. } => {}
            }
        }

        if resolved == Type::Unknown {
            if let Some(ann) = type_ann {
                if let TypeAnnotationKind::Simple { segments } = &ann.kind {
                    if segments.len() == 1 {
                        let name = &segments[0];
                        if name == "f16" || name == "bf16" {
                            self.error_with_hint(
                                format!("E2901: scalar type '{}' is not supported; use f32/f64 or tensor precision", name),
                                ann.span,
                                "f16 and bf16 remain available only as tensor precision metadata",
                            );
                            return resolved;
                        }
                        // Primitive keywords and `_` are handled by the base
                        // method; reaching here means the type is truly unknown.
                        self.error_with_hint(
                            format!("Unknown type '{}'", name),
                            ann.span,
                            format!(
                                "Make sure '{}' is declared as a struct, enum, or type alias.",
                                name
                            ),
                        );
                    }
                }
            }
        }

        resolved
    }

}
