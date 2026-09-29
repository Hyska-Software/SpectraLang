use super::*;

impl ASTLowering {
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

    pub(crate) fn host_function_descriptor(
        &self,
        callee: &Expression,
    ) -> Option<HostFunctionDescriptor> {
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

    /// Shapes the arguments of a standard-library host call.
    ///
    /// The polymorphic print family (`io.print`/`println`/`eprint`/`eprintln`)
    /// consumes `(type_tag, value)` pairs so the runtime can pick a formatter
    /// per argument; every other host call takes its raw arguments. Both the
    /// direct call path and the qualified `io.println(x)` method-call path must
    /// shape them identically.
    pub(crate) fn host_call_arguments(
        &mut self,
        runtime_name: &str,
        arg_values: Vec<Value>,
        arg_exprs: &[Expression],
        ir_func: &mut IRFunction,
    ) -> Vec<Value> {
        let collection_key_call = matches!(
            runtime_name,
            "spectra.std.collections.hash_set_insert"
                | "spectra.std.collections.hash_set_contains"
                | "spectra.std.collections.hash_set_remove"
                | "spectra.std.collections.ordered_map_set"
                | "spectra.std.collections.ordered_map_get"
                | "spectra.std.collections.ordered_map_contains"
                | "spectra.std.collections.ordered_map_remove"
                | "spectra.std.collections.ordered_map_range_keys"
                | "spectra.std.collections.priority_queue_push"
        );
        if collection_key_call {
            let element_type = arg_exprs
                .get(1)
                .map(|argument| self.infer_expr_ir_type(argument));
            let sort_kind = element_type
                .as_ref()
                .and_then(Self::list_sort_kind_for_element_type);
            let sort_kind = sort_kind.unwrap_or_else(|| {
                self.error(format!(
                    "collection keys and priorities support only int, exact integers, float, bool, string, and char values; found {}",
                    element_type
                        .as_ref()
                        .map(|ty| format!("{ty:?}"))
                        .unwrap_or_else(|| "no key or value argument".to_string())
                ));
                spectra_contract::collection_sort::INVALID
            });
            let kind_value = self.builder.build_const_int(ir_func, sort_kind);
            let mut shaped = arg_values;
            shaped.push(kind_value);
            return shaped;
        }
        if runtime_name == "spectra.std.collections.list_sort" {
            let list_type = arg_exprs
                .first()
                .map(|argument| self.infer_expr_ir_type(argument));
            let sort_kind = list_type
                .as_ref()
                .and_then(Self::list_sort_kind_for_list_type);
            let sort_kind = sort_kind.unwrap_or_else(|| {
                self.error(format!(
                    "std.collections.list_sort supports only int, exact integers, float, bool, string, and char elements; found {}",
                    list_type
                        .as_ref()
                        .map(|ty| format!("{ty:?}"))
                        .unwrap_or_else(|| "no list argument".to_string())
                ));
                spectra_contract::collection_sort::INVALID
            });
            let kind_value = self.builder.build_const_int(ir_func, sort_kind);
            let mut shaped = arg_values;
            shaped.push(kind_value);
            return shaped;
        }
        if !matches!(
            runtime_name,
            "spectra.std.io.print"
                | "spectra.std.io.println"
                | "spectra.std.io.eprint"
                | "spectra.std.io.eprintln"
        ) {
            return arg_values;
        }
        let mut paired = Vec::with_capacity(arg_values.len() * 2);
        for (arg_val, arg_expr) in arg_values.iter().zip(arg_exprs.iter()) {
            let tag: i64 = match self.infer_expr_ir_type(arg_expr) {
                IRType::String => 1, // PRINT_TAG_STR
                IRType::Bool => 2,   // PRINT_TAG_BOOL
                IRType::Float => 3,  // PRINT_TAG_FLOAT
                _ => 0,              // PRINT_TAG_INT
            };
            let tag_val = self.builder.build_const_int(ir_func, tag);
            paired.push(tag_val);
            paired.push(*arg_val);
        }
        paired
    }

    fn list_sort_kind_for_list_type(list_type: &IRType) -> Option<i64> {
        if let Some(args) = Self::ir_generic_args_static(list_type, "List") {
            return Self::list_sort_kind_for_element_type(args.first()?);
        }

        let IRType::Struct { name, .. } = Self::ir_type_representation_static(list_type) else {
            return None;
        };
        let element_name = name.strip_prefix("List_")?;
        let element_type = match element_name {
            "int" => IRType::Int,
            "float" => IRType::Float,
            "bool" => IRType::Bool,
            "string" => IRType::String,
            "char" => IRType::Char,
            "i8" => Self::exact_int_sort_type(true, IRIntWidth::I8),
            "i16" => Self::exact_int_sort_type(true, IRIntWidth::I16),
            "i32" => Self::exact_int_sort_type(true, IRIntWidth::I32),
            "i64" => Self::exact_int_sort_type(true, IRIntWidth::I64),
            "u8" => Self::exact_int_sort_type(false, IRIntWidth::I8),
            "u16" => Self::exact_int_sort_type(false, IRIntWidth::I16),
            "u32" => Self::exact_int_sort_type(false, IRIntWidth::I32),
            "u64" => Self::exact_int_sort_type(false, IRIntWidth::I64),
            "isize" => Self::exact_int_sort_type(true, IRIntWidth::Isize),
            "usize" => Self::exact_int_sort_type(false, IRIntWidth::Usize),
            "f32" => IRType::ExactFloat {
                width: IRFloatWidth::F32,
            },
            "f64" => IRType::ExactFloat {
                width: IRFloatWidth::F64,
            },
            _ => return None,
        };
        Self::list_sort_kind_for_element_type(&element_type)
    }

    fn exact_int_sort_type(signed: bool, width: IRIntWidth) -> IRType {
        IRType::ExactInt { signed, width }
    }

    fn list_sort_kind_for_element_type(element_type: &IRType) -> Option<i64> {
        use spectra_contract::collection_sort as sort_kind;

        match Self::ir_type_representation_static(element_type) {
            IRType::Int => Some(sort_kind::INT),
            IRType::Float | IRType::ExactFloat { .. } => Some(sort_kind::FLOAT),
            IRType::Bool => Some(sort_kind::BOOL),
            IRType::String => Some(sort_kind::STRING),
            IRType::Char => Some(sort_kind::CHAR),
            IRType::ExactInt { signed, width } => {
                let width = match width {
                    IRIntWidth::I8 => 8,
                    IRIntWidth::I16 => 16,
                    IRIntWidth::I32 => 32,
                    IRIntWidth::I64 | IRIntWidth::Isize | IRIntWidth::Usize => 64,
                };
                Some(
                    if *signed {
                        sort_kind::SIGNED_EXACT_BASE
                    } else {
                        sort_kind::UNSIGNED_EXACT_BASE
                    } + width,
                )
            }
            _ => None,
        }
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
            let IRType::Enum { variants, .. } = Self::ir_type_representation_static(&enum_type)
            else {
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
                        .then(|| {
                            arguments
                                .get(1)
                                .map(|argument| self.infer_expr_ir_type(argument))
                        })
                        .flatten()
                })
            }
            "spectra.std.result.result_unwrap" | "spectra.std.result.result_unwrap_or" => {
                payload_type(first_type.clone(), "Ok").or_else(|| {
                    (descriptor.runtime_name.ends_with("unwrap_or"))
                        .then(|| {
                            arguments
                                .get(1)
                                .map(|argument| self.infer_expr_ir_type(argument))
                        })
                        .flatten()
                })
            }
            "spectra.std.result.result_unwrap_err" => payload_type(first_type.clone(), "Err"),
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
            "spectra.std.result.result_map" => callback_return.and_then(|payload| {
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
            }),
            "spectra.std.result.result_map_err" => callback_return.and_then(|error| {
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
            }),
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
            [
                "List_",
                "Vector_",
                "Set_",
                "Iterator_",
                "Stack_",
                "Queue_",
                "HashSet_",
                "PriorityQueue_",
            ]
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
            let suffix = name
                .strip_prefix("Map_")
                .or_else(|| name.strip_prefix("OrderedMap_"))?;
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
        if matches!(
            operation,
            "list_new" | "list_with_capacity" | "vector_new" | "vector_with_capacity"
        ) {
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
        } else if matches!(
            operation,
            "list_iter"
                | "vector_iter"
                | "set_iter"
                | "stack_iter"
                | "queue_iter"
                | "hash_set_iter"
                | "ordered_map_iter"
                | "ordered_map_range_keys"
                | "bitset_iter"
                | "map_iter"
                | "map_values_iter"
        ) {
            if let Some(first_type) = first_type.as_ref() {
                if operation == "bitset_iter" {
                    let payload = IRType::Int;
                    descriptor.return_type = IRType::Generic {
                        name: "Iterator".to_string(),
                        args: vec![payload.clone()],
                        representation: Box::new(IRType::Struct {
                            name: format!("Iterator_{}", self.ir_type_to_ast_name(&payload)),
                            fields: Vec::new(),
                        }),
                    };
                    return descriptor;
                }
                let collection = if matches!(operation, "map_iter" | "map_values_iter") {
                    "Map"
                } else if matches!(operation, "ordered_map_iter" | "ordered_map_range_keys") {
                    "OrderedMap"
                } else if operation == "hash_set_iter" {
                    "HashSet"
                } else if operation == "set_iter" {
                    "Set"
                } else if operation == "stack_iter" {
                    "Stack"
                } else if operation == "queue_iter" {
                    "Queue"
                } else if operation == "vector_iter" {
                    "Vector"
                } else {
                    "List"
                };
                let payload = if matches!(
                    operation,
                    "map_iter" | "ordered_map_iter" | "ordered_map_range_keys"
                ) {
                    map_types_for_type(first_type).map(|(key, _)| key)
                } else if operation == "map_values_iter" {
                    map_types_for_type(first_type).map(|(_, value)| value)
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
        } else if matches!(
            operation,
            "set_get"
                | "vector_get"
                | "vector_pop"
                | "vector_remove_at"
                | "stack_pop"
                | "stack_peek"
                | "queue_dequeue"
                | "queue_peek"
                | "priority_queue_peek"
                | "priority_queue_pop"
                | "iterator_next"
        ) {
            if let Some(first_type) = first_type.as_ref() {
                let collection = if operation == "set_get" {
                    "Set"
                } else if operation.starts_with("vector_") {
                    "Vector"
                } else if matches!(operation, "priority_queue_peek" | "priority_queue_pop") {
                    "PriorityQueue"
                } else if matches!(operation, "stack_pop" | "stack_peek") {
                    "Stack"
                } else if matches!(operation, "queue_dequeue" | "queue_peek") {
                    "Queue"
                } else {
                    "Iterator"
                };
                if let Some(element_type) = collection_element_for_type(first_type, collection) {
                    descriptor.return_type = option_type(element_type);
                }
            }
        } else if matches!(
            operation,
            "list_get"
                | "list_pop"
                | "list_pop_front"
                | "list_remove_at"
                | "list_get_option"
                | "list_pop_option"
                | "list_pop_front_option"
                | "list_remove_at_option"
                | "vector_get"
                | "vector_pop"
                | "vector_remove_at"
        ) {
            if let Some(first_type) = first_type.as_ref() {
                if let Some(element_type) = collection_element_for_type(first_type, "List") {
                    descriptor.return_type = option_type(element_type);
                }
            }
        } else if matches!(
            operation,
            "map_new"
                | "map_with_capacity"
                | "vector_new"
                | "vector_with_capacity"
                | "set_new"
                | "set_with_capacity"
                | "stack_new"
                | "stack_with_capacity"
                | "queue_new"
                | "queue_with_capacity"
                | "hash_set_new"
                | "hash_set_with_capacity"
                | "ordered_map_new"
                | "priority_queue_new"
                | "priority_queue_new_min"
                | "priority_queue_with_capacity"
        ) {
            if let Some(annotation) = self.current_expected_annotation.as_ref() {
                let expected = self.lower_type_annotation(annotation);
                if !Self::ir_type_contains_unknown(&expected) {
                    descriptor.return_type = expected;
                }
            }
        } else if matches!(
            operation,
            "map_get" | "map_remove" | "map_get_option" | "map_remove_option"
        ) {
            if let Some(first_type) = first_type.as_ref() {
                if let Some((_, value_type)) = map_types_for_type(first_type) {
                    descriptor.return_type = option_type(value_type);
                }
            }
        } else if matches!(operation, "ordered_map_get" | "ordered_map_remove") {
            if let Some(first_type) = first_type.as_ref() {
                if let Some((_, value_type)) = map_types_for_type(first_type) {
                    descriptor.return_type = option_type(value_type);
                }
            }
        } else if matches!(operation, "env_get" | "env_arg") {
            descriptor.return_type = option_type(IRType::String);
        }
        descriptor
    }

    /// Selects an internal collection ABI when the source types prove that a
    /// map operation only handles scalar words. Scalar keys cannot be packed
    /// strings, so the runtime can skip the allocation-table probe that
    /// protects the polymorphic collection path. Unknown, aggregate, and
    /// string payloads deliberately keep the public host name.
    pub(crate) fn specialized_collection_host_runtime_name(
        &mut self,
        runtime_name: &'static str,
        arguments: &[Expression],
    ) -> &'static str {
        const MAP_SET_SCALAR: &str = "spectra.compiler.collections.map_set_scalar";
        const MAP_CONTAINS_SCALAR: &str = "spectra.compiler.collections.map_contains_scalar";
        const MAP_GET_SCALAR: &str = "spectra.compiler.collections.map_get_scalar";
        const MAP_REMOVE_SCALAR: &str = "spectra.compiler.collections.map_remove_scalar";

        let mut is_scalar = |argument: Option<&Expression>| {
            argument.is_some_and(|argument| {
                matches!(
                    self.infer_expr_ir_type(argument),
                    IRType::Int
                        | IRType::Float
                        | IRType::Bool
                        | IRType::Char
                        | IRType::ExactInt { .. }
                        | IRType::ExactFloat { .. }
                )
            })
        };

        match runtime_name {
            "spectra.std.collections.map_set"
                if is_scalar(arguments.get(1)) && is_scalar(arguments.get(2)) =>
            {
                MAP_SET_SCALAR
            }
            "spectra.std.collections.map_contains" if is_scalar(arguments.get(1)) => {
                MAP_CONTAINS_SCALAR
            }
            "spectra.std.collections.map_get" | "spectra.std.collections.map_get_option"
                if is_scalar(arguments.get(1)) =>
            {
                MAP_GET_SCALAR
            }
            "spectra.std.collections.map_remove" | "spectra.std.collections.map_remove_option"
                if is_scalar(arguments.get(1)) =>
            {
                MAP_REMOVE_SCALAR
            }
            _ => runtime_name,
        }
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

    pub(crate) fn host_function_descriptor_for_path(
        &self,
        path: &[String],
    ) -> Option<HostFunctionDescriptor> {
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
                let converted = self
                    .builder
                    .build_cast(ir_func, value, ty.clone(), IRType::Float);
                (converted, IRType::Float)
            }
            ty @ (IRType::ExactInt { .. } | IRType::Char) => {
                let converted = self
                    .builder
                    .build_cast(ir_func, value, ty.clone(), IRType::Int);
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

        if let Some(equal) =
            self.lower_structural_value_equality(lhs, rhs, lhs_type, rhs_type, ir_func)
        {
            return if negate {
                self.builder.build_not(ir_func, equal)
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

    /// Compare aggregate values by their contents. Aggregates are pointer
    /// represented in the backend, so comparing their SSA handles only tests
    /// allocation identity and is not value equality.
    fn lower_structural_value_equality(
        &mut self,
        lhs: Value,
        rhs: Value,
        lhs_type: &IRType,
        rhs_type: &IRType,
        ir_func: &mut IRFunction,
    ) -> Option<Value> {
        let lhs_representation = Self::ir_type_representation_static(lhs_type);
        let rhs_representation = Self::ir_type_representation_static(rhs_type);

        match (lhs_representation, rhs_representation) {
            (
                IRType::Enum {
                    name: lhs_name,
                    variants: lhs_variants,
                },
                IRType::Enum {
                    name: rhs_name,
                    variants: rhs_variants,
                },
            ) if lhs_name == rhs_name && !lhs_variants.is_empty() && !rhs_variants.is_empty() => {
                let variants = self.enum_variants_for_type(lhs_type)?;
                if variants.is_empty() {
                    return None;
                }
                Some(self.lower_enum_value_equality(lhs, rhs, &variants, ir_func))
            }
            (
                IRType::Tuple {
                    elements: lhs_fields,
                },
                IRType::Tuple {
                    elements: rhs_fields,
                },
            ) if lhs_fields == rhs_fields => {
                Some(self.lower_aggregate_field_equality(lhs, rhs, lhs_fields, 0, ir_func))
            }
            (
                IRType::Struct {
                    name: lhs_name,
                    fields: lhs_fields,
                },
                IRType::Struct {
                    name: rhs_name,
                    fields: rhs_fields,
                },
            ) if lhs_name == rhs_name && lhs_fields == rhs_fields => {
                if lhs_fields.is_empty() {
                    // Empty nominal shapes are also used for recursive type
                    // edges. Resolve only a registered empty record here;
                    // leave unresolved or recursive shapes opaque instead
                    // of treating them as equal or infinitely expanding them.
                    return match self.struct_definitions.get(lhs_name) {
                        Some(fields) if fields.is_empty() => {
                            Some(self.builder.build_const_bool(ir_func, true))
                        }
                        _ => None,
                    };
                }
                let field_types: Vec<IRType> = lhs_fields
                    .iter()
                    .map(|(_, field_type)| field_type.clone())
                    .collect();
                Some(self.lower_aggregate_field_equality(lhs, rhs, &field_types, 0, ir_func))
            }
            _ => None,
        }
    }

    fn lower_aggregate_field_equality(
        &mut self,
        lhs: Value,
        rhs: Value,
        field_types: &[IRType],
        base_offset: i64,
        ir_func: &mut IRFunction,
    ) -> Value {
        let field_layout = layout::layout_of(field_types.iter());
        let mut equal = self.builder.build_const_bool(ir_func, true);

        for (index, field_type) in field_types.iter().enumerate() {
            let byte_offset = base_offset + field_layout.offsets[index] as i64;
            let lhs_field_ptr = self.builder.build_field_ptr(ir_func, lhs, byte_offset);
            let rhs_field_ptr = self.builder.build_field_ptr(ir_func, rhs, byte_offset);
            let lhs_field =
                self.builder
                    .build_load_typed(ir_func, lhs_field_ptr, field_type.clone());
            let rhs_field =
                self.builder
                    .build_load_typed(ir_func, rhs_field_ptr, field_type.clone());
            let field_equal = self
                .lower_value_equality(lhs_field, rhs_field, field_type, field_type, false, ir_func);
            equal = self.builder.build_and(ir_func, equal, field_equal);
        }

        equal
    }

    fn lower_enum_value_equality(
        &mut self,
        lhs: Value,
        rhs: Value,
        variants: &[(String, usize, Option<Vec<IRType>>)],
        ir_func: &mut IRFunction,
    ) -> Value {
        let tag_offset = 0;
        let lhs_tag_ptr = self.builder.build_field_ptr(ir_func, lhs, tag_offset);
        let rhs_tag_ptr = self.builder.build_field_ptr(ir_func, rhs, tag_offset);
        let lhs_tag = self
            .builder
            .build_load_typed(ir_func, lhs_tag_ptr, IRType::Int);
        let rhs_tag = self
            .builder
            .build_load_typed(ir_func, rhs_tag_ptr, IRType::Int);
        let tags_equal = self.builder.build_eq(ir_func, lhs_tag, rhs_tag);

        let label = ir_func.blocks.len();
        let dispatch = ir_func.add_block(format!("enum_eq_{label}_dispatch"));
        let tags_differ = ir_func.add_block(format!("enum_eq_{label}_tags_differ"));
        let default_case = ir_func.add_block(format!("enum_eq_{label}_default"));
        let merge = ir_func.add_block(format!("enum_eq_{label}_merge"));
        self.builder
            .build_cond_branch(ir_func, tags_equal, dispatch, tags_differ);

        self.builder.set_current_block(tags_differ);
        let false_value = self.builder.build_const_bool(ir_func, false);
        self.builder.build_branch(ir_func, merge);
        let mut incoming = vec![(false_value, tags_differ)];

        let mut check_block = dispatch;
        for (index, (_, tag, payload_types)) in variants.iter().enumerate() {
            let case_block = ir_func.add_block(format!("enum_eq_{label}_case_{index}"));
            let next_check = if index + 1 < variants.len() {
                ir_func.add_block(format!("enum_eq_{label}_check_{}", index + 1))
            } else {
                default_case
            };

            self.builder.set_current_block(check_block);
            let expected_tag = self.builder.build_const_int(ir_func, *tag as i64);
            let matches_variant = self.builder.build_eq(ir_func, lhs_tag, expected_tag);
            self.builder
                .build_cond_branch(ir_func, matches_variant, case_block, next_check);

            self.builder.set_current_block(case_block);
            let case_equal = if let Some(payload_types) = payload_types {
                self.lower_aggregate_field_equality(lhs, rhs, payload_types, 8, ir_func)
            } else {
                self.builder.build_const_bool(ir_func, true)
            };
            let case_final = self.builder.get_current_block().unwrap_or(case_block);
            let case_terminated = ir_func
                .get_block(case_final)
                .map(|block| block.terminator.is_some())
                .unwrap_or(false);
            if !case_terminated {
                self.builder.build_branch(ir_func, merge);
                incoming.push((case_equal, case_final));
            }

            check_block = next_check;
        }

        self.builder.set_current_block(default_case);
        let invalid_tag = self.builder.build_const_bool(ir_func, false);
        self.builder.build_branch(ir_func, merge);
        incoming.push((invalid_tag, default_case));

        self.builder.set_current_block(merge);
        self.builder.build_phi(ir_func, incoming)
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
