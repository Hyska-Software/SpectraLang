use super::*;

impl ASTLowering {
    /// Infer concrete types from argument expressions
    /// This is a simplified type inference for monomorphization
    pub(crate) fn infer_argument_types(&mut self, arguments: &[Expression]) -> Vec<IRType> {
        arguments
            .iter()
            .map(|arg| {
                // Try to infer type from expression
                match &arg.kind {
                    ExpressionKind::NumberLiteral(n) => {
                        // Try to determine if int or float
                        if spectra_compiler::numeric::number_literal_is_float(n) {
                            IRType::Float
                        } else {
                            IRType::Int
                        }
                    }
                    ExpressionKind::BoolLiteral(_) => IRType::Bool,
                    ExpressionKind::StringLiteral(_) => IRType::String,
                    ExpressionKind::Identifier(name) => {
                        // Try to find in struct_var_map
                        if let Some((_, struct_name)) = self.struct_var_map.get(name) {
                            // Get fields from struct_definitions
                            let fields = self
                                .struct_definitions
                                .get(&struct_name)
                                .cloned()
                                .unwrap_or_default();
                            IRType::Struct {
                                name: struct_name,
                                fields,
                            }
                        } else if let Some(info) = self.array_map.get(name) {
                            IRType::Array {
                                element_type: Box::new(info.element_type.clone()),
                                size: info.size,
                            }
                        } else if let Some(ty) = self.variable_types.get(name) {
                            ty
                        } else {
                            // An unresolved argument must remain poison.  Defaulting
                            // it to `int` changes generic monomorphization and can
                            // make an invalid call look well-typed to the backend.
                            IRType::Unknown
                        }
                    }
                    ExpressionKind::StructLiteral {
                        name, type_args, ..
                    } => self
                        .resolve_struct_type(name, type_args)
                        .unwrap_or(IRType::Unknown),
                    _ => IRType::Unknown,
                }
            })
            .collect()
    }

    pub(crate) fn resolve_call_path(&self, callee: &Expression) -> Option<Vec<String>> {
        match &callee.kind {
            ExpressionKind::Identifier(name) => Some(vec![name.clone()]),
            ExpressionKind::FieldAccess { object, field } => {
                let mut path = self.resolve_call_path(object)?;
                path.push(field.clone());
                Some(path)
            }
            _ => None,
        }
    }

    pub(crate) fn host_function_descriptor(&self, callee: &Expression) -> Option<HostFunctionDescriptor> {
        let path = self.resolve_call_path(callee)?;
        self.host_function_descriptor_for_path(&path)
    }

    /// Refine stdlib functions whose public signature uses `unknown` from the
    /// concrete generic enum payload at the call site.  The host ABI returns a
    /// canonical i64 word, but the IR type still matters for string equality,
    /// float bitcasts, and bool narrowing after `Option`/`Result` unwrapping.
    pub(crate) fn host_function_descriptor_for_call(
        &mut self,
        callee: &Expression,
        arguments: &[Expression],
    ) -> Option<HostFunctionDescriptor> {
        let descriptor = self.host_function_descriptor(callee)?;
        Some(self.refine_host_function_descriptor(descriptor, arguments))
    }

    pub(crate) fn refine_host_function_descriptor(
        &mut self,
        mut descriptor: HostFunctionDescriptor,
        arguments: &[Expression],
    ) -> HostFunctionDescriptor {
        let first_type = arguments
            .first()
            .map(|argument| self.infer_expr_ir_type(argument));

        let payload_type = |enum_type: Option<IRType>, variant: &str| {
            let enum_type = enum_type?;
            let IRType::Enum { variants, .. } = Self::ir_type_representation_static(&enum_type) else {
                return None;
            };
            variants.iter().find_map(|(name, payload)| {
                (name == variant)
                    .then(|| payload.as_ref().and_then(|types| types.first()).cloned())
                    .flatten()
                    .filter(|ty| !Self::ir_type_contains_unknown(ty))
            })
        };

        let refined_type = match descriptor.runtime_name {
            "spectra.std.option.option_unwrap" | "spectra.std.option.option_unwrap_or" => {
                payload_type(first_type.clone(), "Some").or_else(|| {
                    (descriptor.runtime_name.ends_with("unwrap_or"))
                        .then(|| arguments.get(1).map(|argument| self.infer_expr_ir_type(argument)))
                        .flatten()
                })
            }
            "spectra.std.result.result_unwrap"
            | "spectra.std.result.result_unwrap_or" => {
                payload_type(first_type.clone(), "Ok").or_else(|| {
                    (descriptor.runtime_name.ends_with("unwrap_or"))
                        .then(|| arguments.get(1).map(|argument| self.infer_expr_ir_type(argument)))
                        .flatten()
                })
            }
            "spectra.std.result.result_unwrap_err" => {
                payload_type(first_type.clone(), "Err")
            }
            _ => None,
        };

        if let Some(refined_type) = refined_type {
            descriptor.return_type = refined_type;
        }

        // Higher-order Option/Result transforms preserve the tagged aggregate
        // representation while changing the mapped payload type.  Derive the
        // concrete IR enum from the callback return and the untouched payload
        // on the other variant; otherwise the host result is lowered as an
        // integer and immediately loses its type information.
        let callback_return = arguments.get(1).and_then(|argument| {
            if let IRType::Function { return_type, .. } = self.infer_expr_ir_type(argument) {
                Some(*return_type)
            } else {
                None
            }
        });
        let mapped_enum = match descriptor.runtime_name {
            "spectra.std.option.option_map" => callback_return.map(|payload| {
                let representation = IRType::Enum {
                    name: format!("Option_{}", self.ir_type_to_ast_name(&payload)),
                    variants: vec![
                        ("Some".to_string(), Some(vec![payload.clone()])),
                        ("None".to_string(), None),
                    ],
                };
                IRType::Generic {
                    name: "Option".to_string(),
                    args: vec![payload],
                    representation: Box::new(representation),
                }
            }),
            "spectra.std.result.result_map" => {
                callback_return.and_then(|payload| {
                    let error = payload_type(first_type.clone(), "Err")?;
                    let representation = IRType::Enum {
                        name: format!(
                            "Result_{}_{}",
                            self.ir_type_to_ast_name(&payload),
                            self.ir_type_to_ast_name(&error)
                        ),
                        variants: vec![
                            ("Ok".to_string(), Some(vec![payload.clone()])),
                            ("Err".to_string(), Some(vec![error.clone()])),
                        ],
                    };
                    Some(IRType::Generic {
                        name: "Result".to_string(),
                        args: vec![payload, error],
                        representation: Box::new(representation),
                    })
                })
            }
            "spectra.std.result.result_map_err" => {
                callback_return.and_then(|error| {
                    let ok = payload_type(first_type.clone(), "Ok")?;
                    let representation = IRType::Enum {
                        name: format!(
                            "Result_{}_{}",
                            self.ir_type_to_ast_name(&ok),
                            self.ir_type_to_ast_name(&error)
                        ),
                        variants: vec![
                            ("Ok".to_string(), Some(vec![ok.clone()])),
                            ("Err".to_string(), Some(vec![error.clone()])),
                        ],
                    };
                    Some(IRType::Generic {
                        name: "Result".to_string(),
                        args: vec![ok, error],
                        representation: Box::new(representation),
                    })
                })
            }
            _ => None,
        };
        if let Some(mapped_enum) = mapped_enum {
            descriptor.return_type = mapped_enum;
        }

        let collection_element_type = |name: &str| -> Option<IRType> {
            let part = |value: &str| -> IRType {
                match value {
                    "int" => IRType::Int,
                    "float" => IRType::Float,
                    "bool" => IRType::Bool,
                    "string" => IRType::String,
                    "char" => IRType::Char,
                    "i8" => IRType::ExactInt {
                        signed: true,
                        width: IRIntWidth::I8,
                    },
                    "i16" => IRType::ExactInt {
                        signed: true,
                        width: IRIntWidth::I16,
                    },
                    "i32" => IRType::ExactInt {
                        signed: true,
                        width: IRIntWidth::I32,
                    },
                    "i64" => IRType::ExactInt {
                        signed: true,
                        width: IRIntWidth::I64,
                    },
                    "u8" => IRType::ExactInt {
                        signed: false,
                        width: IRIntWidth::I8,
                    },
                    "u16" => IRType::ExactInt {
                        signed: false,
                        width: IRIntWidth::I16,
                    },
                    "u32" => IRType::ExactInt {
                        signed: false,
                        width: IRIntWidth::I32,
                    },
                    "u64" => IRType::ExactInt {
                        signed: false,
                        width: IRIntWidth::I64,
                    },
                    other => IRType::Struct {
                        name: other.to_string(),
                        fields: Vec::new(),
                    },
                }
            };
            ["List_", "Set_", "Iterator_"]
                .iter()
                .find_map(|prefix| name.strip_prefix(prefix).map(part))
        };
        let option_type = |payload: IRType| {
            let representation = IRType::Enum {
                name: format!("Option_{}", self.ir_type_to_ast_name(&payload)),
                variants: vec![
                    ("Some".to_string(), Some(vec![payload.clone()])),
                    ("None".to_string(), None),
                ],
            };
            IRType::Generic {
                name: "Option".to_string(),
                args: vec![payload],
                representation: Box::new(representation),
            }
        };
        let collection_element_for_type = |ty: &IRType, collection: &str| -> Option<IRType> {
            if let Some(args) = Self::ir_generic_args_static(ty, collection) {
                return args.first().cloned();
            }
            match Self::ir_type_representation_static(ty) {
                IRType::Struct { name, .. } => collection_element_type(name),
                _ => None,
            }
        };
        let map_types_for_type = |ty: &IRType| -> Option<(IRType, IRType)> {
            if let Some(args) = Self::ir_generic_args_static(ty, "Map") {
                return Some((args.first()?.clone(), args.get(1)?.clone()));
            }
            let IRType::Struct { name, .. } = Self::ir_type_representation_static(ty) else {
                return None;
            };
            let suffix = name.strip_prefix("Map_")?;
            let (key, value) = suffix.split_once('_')?;
            Some((
                collection_element_type(&format!("List_{key}"))?,
                collection_element_type(&format!("List_{value}"))?,
            ))
        };
        let operation = descriptor
            .runtime_name
            .rsplit('.')
            .next()
            .unwrap_or_default();
        let compatibility_collection_read = descriptor
            .runtime_name
            .starts_with("spectra.std.compat.collections.");
        let compatibility_env_read = descriptor
            .runtime_name
            .starts_with("spectra.std.compat.env.");

        if operation == "list_new" {
            if let Some(annotation) = self.current_expected_annotation.as_ref() {
                let expected = self.lower_type_annotation(annotation);
                if !Self::ir_type_contains_unknown(&expected) {
                    descriptor.return_type = expected;
                }
            }
        } else if operation == "list_map" {
            if let Some(IRType::Function { return_type, .. }) = arguments
                .get(1)
                .map(|argument| self.infer_expr_ir_type(argument))
            {
                if !Self::ir_type_contains_unknown(&return_type) {
                    descriptor.return_type = IRType::Generic {
                        name: "List".to_string(),
                        args: vec![(*return_type).clone()],
                        representation: Box::new(IRType::Struct {
                            name: format!("List_{}", self.ir_type_to_ast_name(&return_type)),
                            fields: Vec::new(),
                        }),
                    };
                }
            }
        } else if operation == "list_filter" {
            if let Some(first_type) = first_type.as_ref() {
                if Self::ir_generic_args_static(first_type, "List").is_some()
                    || matches!(Self::ir_type_representation_static(first_type), IRType::Struct { name, .. } if name.starts_with("List_"))
                {
                    descriptor.return_type = first_type.clone();
                }
            }
        } else if operation == "list_reduce" {
            if let Some(accumulator) = arguments
                .get(1)
                .map(|argument| self.infer_expr_ir_type(argument))
            {
                if !Self::ir_type_contains_unknown(&accumulator) {
                    descriptor.return_type = accumulator;
                }
            }
        } else if matches!(operation, "list_iter" | "set_iter" | "map_iter") {
            if let Some(first_type) = first_type.as_ref() {
                let collection = if operation == "map_iter" { "Map" } else if operation == "set_iter" { "Set" } else { "List" };
                let payload = if operation == "map_iter" {
                    map_types_for_type(first_type).map(|(key, _)| key)
                } else {
                    collection_element_for_type(first_type, collection)
                };
                if let Some(payload) = payload {
                    descriptor.return_type = IRType::Generic {
                        name: "Iterator".to_string(),
                        args: vec![payload.clone()],
                        representation: Box::new(IRType::Struct {
                            name: format!("Iterator_{}", self.ir_type_to_ast_name(&payload)),
                            fields: Vec::new(),
                        }),
                    };
                }
            }
        } else if matches!(operation, "set_get" | "iterator_next") {
            if let Some(first_type) = first_type.as_ref() {
                let collection = if operation == "set_get" { "Set" } else { "Iterator" };
                if let Some(element_type) = collection_element_for_type(first_type, collection) {
                    descriptor.return_type = option_type(element_type);
                }
            }
        } else if matches!(
            operation,
            "list_get" | "list_pop" | "list_pop_front" | "list_remove_at"
        ) {
            if let Some(first_type) = first_type.as_ref() {
                if let Some(element_type) = collection_element_for_type(first_type, "List") {
                    descriptor.return_type = if compatibility_collection_read {
                        element_type
                    } else {
                        option_type(element_type)
                    };
                }
            }
        } else if matches!(
            operation,
            "list_get_option"
                | "list_pop_option"
                | "list_pop_front_option"
                | "list_remove_at_option"
        ) {
            if let Some(first_type) = first_type.as_ref() {
                if let Some(element_type) = collection_element_for_type(first_type, "List") {
                    descriptor.return_type = option_type(element_type);
                }
            }
        } else if operation == "map_new" {
            if let Some(annotation) = self.current_expected_annotation.as_ref() {
                let expected = self.lower_type_annotation(annotation);
                if !Self::ir_type_contains_unknown(&expected) {
                    descriptor.return_type = expected;
                }
            }
        } else if matches!(operation, "map_get" | "map_remove") {
            if let Some(first_type) = first_type.as_ref() {
                if let Some((_, value_type)) = map_types_for_type(first_type) {
                    descriptor.return_type = if compatibility_collection_read {
                        value_type
                    } else {
                        option_type(value_type)
                    };
                }
            }
        } else if matches!(operation, "map_get_option" | "map_remove_option") {
            if let Some(first_type) = first_type.as_ref() {
                if let Some((_, value_type)) = map_types_for_type(first_type) {
                    descriptor.return_type = option_type(value_type);
                }
            }
        } else if matches!(operation, "env_get" | "env_arg") && !compatibility_env_read {
            descriptor.return_type = option_type(IRType::String);
        }
        descriptor
    }

    pub(crate) fn std_method_host_function_descriptor(
        &self,
        object: &Expression,
        method_name: &str,
    ) -> Option<HostFunctionDescriptor> {
        let mut path = self.resolve_call_path(object)?;
        path.push(method_name.to_string());
        self.host_function_descriptor_for_path(&path)
    }

    pub(crate) fn std_method_host_function_descriptor_for_call(
        &mut self,
        object: &Expression,
        method_name: &str,
        arguments: &[Expression],
    ) -> Option<HostFunctionDescriptor> {
        let descriptor = self.std_method_host_function_descriptor(object, method_name)?;
        Some(self.refine_host_function_descriptor(descriptor, arguments))
    }

    pub(crate) fn host_function_descriptor_for_path(&self, path: &[String]) -> Option<HostFunctionDescriptor> {
        // Direct path lookup (e.g. std.io.print).
        if let Some(desc) = lookup_std_host_function(path) {
            return Some(desc);
        }
        // Fallback: resolve single-segment bare names via std_import_aliases
        // (e.g. `print` after `import std.io`).
        if path.len() == 1 {
            if let Some(full_path) = self.std_import_aliases.get(&path[0]) {
                return lookup_std_host_function(full_path);
            }
        }
        // Fallback: resolve two-segment alias.function paths via std_import_aliases
        // (e.g. `str.len` after `import std.string as str;`).
        if path.len() == 2 {
            let alias_key = &path[0];
            let func_name = &path[1];
            if let Some(full_prefix) = self.std_import_aliases.get(alias_key) {
                // full_prefix is e.g. ["spectra","std","string","len"] for a bare import,
                // but for alias lookup we need the module prefix (all but last segment)
                // stored per-alias. Try building ["std", module, func].
                // We use the stdlib_path stored in the module exports via aliases:
                // find any alias key matching alias.func and resolve.
                let composed_key = format!("{}.{}", alias_key, func_name);
                if let Some(full_path2) = self.std_import_aliases.get(&composed_key) {
                    return lookup_std_host_function(full_path2);
                }
                // Also try: the full_prefix ends with the bare func name, so the
                // module prefix is full_prefix[..len-1].join and we append func_name.
                if full_prefix.len() >= 2 {
                    let module_prefix = &full_prefix[..full_prefix.len() - 1];
                    let mut resolved = module_prefix.to_vec();
                    resolved.push(func_name.clone());
                    if let Some(desc) = lookup_std_host_function(&resolved) {
                        return Some(desc);
                    }
                }
            }
        }
        None
    }

    pub(crate) fn lower_value_to_string(
        &mut self,
        value: Value,
        value_type: IRType,
        ir_func: &mut IRFunction,
    ) -> Value {
        let (value, value_type) = match value_type {
            IRType::String => return value,
            ty @ IRType::ExactFloat { .. } => {
                let converted = self.builder.build_cast(
                    ir_func,
                    value,
                    ty.clone(),
                    IRType::Float,
                );
                (converted, IRType::Float)
            }
            ty @ (IRType::ExactInt { .. } | IRType::Char) => {
                let converted = self.builder.build_cast(
                    ir_func,
                    value,
                    ty.clone(),
                    IRType::Int,
                );
                (converted, IRType::Int)
            }
            ty => (value, ty),
        };
        let runtime_fn = match value_type {
            IRType::Float => "spectra.std.convert.float_to_string",
            IRType::Bool => "spectra.std.convert.bool_to_string",
            _ => "spectra.std.convert.int_to_string",
        };

        self.require_value(
            self.builder.build_typed_host_call(
                ir_func,
                runtime_fn.to_string(),
                vec![value],
                IRType::String,
                true,
            ),
            "value-to-string host call did not produce its declared result",
        )
    }

    pub(crate) fn lower_value_equality(
        &mut self,
        lhs: Value,
        rhs: Value,
        lhs_type: &IRType,
        rhs_type: &IRType,
        negate: bool,
        ir_func: &mut IRFunction,
    ) -> Value {
        if matches!(lhs_type, IRType::String) || matches!(rhs_type, IRType::String) {
            let lhs = self.lower_value_to_string(lhs, lhs_type.clone(), ir_func);
            let rhs = self.lower_value_to_string(rhs, rhs_type.clone(), ir_func);
            let equal = self.require_value(
                self.builder.build_typed_host_call(
                    ir_func,
                    "spectra.std.string.eq".to_string(),
                    vec![lhs, rhs],
                    IRType::Bool,
                    true,
                ),
                "string equality host call did not produce its declared result",
            );
            return if negate {
                let false_value = self.builder.build_const_bool(ir_func, false);
                self.builder.build_eq(ir_func, equal, false_value)
            } else {
                equal
            };
        }

        if matches!(lhs_type, IRType::Range) && matches!(rhs_type, IRType::Range) {
            let equal = self.require_value(
                self.builder.build_typed_host_call(
                    ir_func,
                    "spectra.std.range.eq".to_string(),
                    vec![lhs, rhs],
                    IRType::Bool,
                    true,
                ),
                "range equality host call did not produce its declared result",
            );
            return if negate {
                let false_value = self.builder.build_const_bool(ir_func, false);
                self.builder.build_eq(ir_func, equal, false_value)
            } else {
                equal
            };
        }

        // Numeric literals are intentionally inferred as the language's
        // default `int`/`float` types.  Aggregate fields and exact-width
        // loads, however, retain their declared ABI type.  Normalize the
        // right-hand side to the left-hand side before emitting Cranelift's
        // comparison so f32-vs-f64 and i32-vs-i64 never reach the verifier.
        let rhs = if lhs_type != rhs_type {
            self.coerce_value_to_type(rhs, rhs_type, lhs_type, ir_func)
        } else {
            rhs
        };

        if negate {
            self.builder.build_ne(ir_func, lhs, rhs)
        } else {
            self.builder.build_eq(ir_func, lhs, rhs)
        }
    }

    /// Convert a lowered value to the declared type of an aggregate field,
    /// tuple element, array element, or assignment target.  Semantic analysis
    /// already rejects incompatible source programs; this helper only
    /// materializes the ABI conversion required by the backend for numeric
    /// literals and exact-width values.
    pub(crate) fn coerce_value_to_type(
        &mut self,
        value: Value,
        from_ty: &IRType,
        to_ty: &IRType,
        ir_func: &mut IRFunction,
    ) -> Value {
        if from_ty == to_ty {
            return value;
        }

        let numeric = |ty: &IRType| {
            matches!(
                ty,
                IRType::Int
                    | IRType::Float
                    | IRType::ExactInt { .. }
                    | IRType::ExactFloat { .. }
                    | IRType::Char
            )
        };

        if numeric(from_ty) && numeric(to_ty) {
            return self
                .builder
                .build_cast(ir_func, value, from_ty.clone(), to_ty.clone());
        }

        value
    }

    pub(crate) fn lower_expression_as_type(
        &mut self,
        expr: &Expression,
        expected_type: &IRType,
        ir_func: &mut IRFunction,
    ) -> Value {
        let value = self.lower_expression(expr, ir_func);
        let actual_type = self.infer_expr_ir_type(expr);
        self.coerce_value_to_type(value, &actual_type, expected_type, ir_func)
    }

}
