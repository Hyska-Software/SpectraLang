use super::*;

impl ASTLowering {
    pub(crate) fn lower_expression_method(
        &mut self,
        expr: &Expression,
        ir_func: &mut IRFunction,
    ) -> Value {
        match &expr.kind {
            ExpressionKind::MethodCall {
                object,
                method_name,
                arguments,
                type_name,
            } => {
                // Check if this is actually a qualified stdlib function call
                // like `std.string.len(x)` parsed as MethodCall { object: std.string, method: "len" }
                if let Some(desc) = self.std_method_host_function_descriptor_for_call(
                    object,
                    method_name,
                    arguments,
                ) {
                    let mut call_args = Vec::new();
                    for arg in arguments {
                        call_args.push(self.lower_expression(arg, ir_func));
                    }
                    let call_args = self.host_call_arguments(
                        &desc.runtime_name,
                        call_args,
                        arguments,
                        ir_func,
                    );
                    let runtime_name = self.specialized_collection_host_runtime_name(
                        desc.runtime_name,
                        arguments,
                    );
                    let result = self.builder.build_typed_host_call(
                        ir_func,
                        runtime_name.to_string(),
                        call_args,
                        desc.return_type.clone(),
                        desc.returns_value,
                    );
                    return if desc.returns_value {
                        self.require_value(
                            result,
                            "qualified standard-library method did not produce its declared result",
                        )
                    } else {
                        // Discarded Void results keep a plain zero (unobservable;
                        // any use is untyped at the semantic level).
                        result.unwrap_or_else(|| self.builder.build_const_int(ir_func, 0))
                    };
                }

                if let Some(function_name) =
                    self.imported_user_function_name(object, method_name)
                {
                    let mut call_args: Vec<Value> = arguments
                        .iter()
                        .map(|argument| self.lower_expression(argument, ir_func))
                        .collect();
                    let resolved = self.resolve_user_function_symbol(&function_name);
                    self.append_hidden_size_args(
                        &[resolved.as_str(), function_name.as_str()],
                        arguments,
                        &mut call_args,
                        ir_func,
                    );
                    if self.user_function_returns_unit(&function_name) {
                        self.builder
                            .build_call(ir_func, resolved.clone(), call_args, false);
                        return self.builder.build_const_int(ir_func, 0);
                    }
                    return self.require_value(
                        self.builder
                            .build_call(ir_func, resolved, call_args, true),
                        "qualified user-module function call did not produce its declared result",
                    );
                }

                // Lower method call to function call: obj.method(args) -> Type_method(obj, args)

                // 1. Lower o objeto (self será o primeiro argumento)
                let obj_value = self.lower_expression(object, ir_func);

                // 1a. If the object is a dyn Trait, dispatch via vtable.
                let obj_ir_type = self.infer_expr_ir_type(object);
                if let IRType::DynTrait { trait_name, .. } = &obj_ir_type {
                    return self.lower_dyn_method_call(
                        obj_value,
                        trait_name.clone(),
                        method_name,
                        arguments,
                        ir_func,
                    );
                }

                // 2. Determinar o tipo do objeto
                // Generic applications carry the concrete ABI nominal in
                // their representation (`Boxed_int`), while unit enums are
                // represented as plain integer tags. Prefer the IR nominal
                // when available, and use the semantic `type_name` for the
                // unit-enum case.
                let obj_type_name = self
                    .ir_nominal_name(&obj_ir_type)
                    .map(str::to_string)
                    .or_else(|| type_name.clone())
                    .or_else(|| {
                        self.resolved_expression_types
                            .get(&object.span)
                            .and_then(|ty| match ty {
                                spectra_compiler::ast::Type::Struct { name }
                                | spectra_compiler::ast::Type::Enum { name }
                                | spectra_compiler::ast::Type::Applied { name, .. } => {
                                    Some(name.clone())
                                }
                                _ => None,
                            })
                    })
                    .or_else(|| match &obj_ir_type {
                        IRType::Struct { name, .. } | IRType::Enum { name, .. } => {
                            Some(name.clone())
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| {
                        // Keep the diagnostic on the normal lowering error
                        // path rather than indexing an absent nominal name.
                        "unknown".to_string()
                    });
                if obj_type_name == "unknown" {
                    return self.invalid_value(format!(
                        "Could not determine object type for method call '{method_name}' (inferred type: {:?})",
                        obj_ir_type
                    ));
                }

                if method_name == "to_json" {
                    if self.json_struct_schemas.contains_key(&obj_type_name) {
                        let mut stack = Vec::new();
                        return self.lower_derive_encode_struct(
                            &obj_type_name,
                            obj_value,
                            ir_func,
                            &mut stack,
                        );
                    }
                    if self.json_enum_schemas.contains_key(&obj_type_name) {
                        return self.lower_enum_to_json(&obj_type_name, obj_value, ir_func);
                    }
                }

                // 3. Construir nome da função: Type_method
                let function_name = format!("{}_{}", obj_type_name, method_name);

                // 3a. R-211: if the object is an instantiation of a generic struct
                // with a template impl, request the per-instantiation specialization.
                // The mangled name (instantiated_struct_method) matches the call.
                if let Some((base_name, concrete_types)) =
                    self.instantiated_structs.get(&obj_type_name).cloned()
                {
                    let generic_key = format!("{}_{}", base_name, method_name);
                    if self.generic_impl_methods.contains_key(&generic_key) {
                        if concrete_types.is_empty() {
                            self.error(format!(
                                "Method '{}' on generic struct '{}' requires concrete type arguments",
                                method_name, base_name
                            ));
                        } else {
                            let request = MethodMonomorphizationRequest {
                                instantiated_struct: obj_type_name.clone(),
                                method_name: method_name.clone(),
                                concrete_types,
                            };
                            let mangled = request.mangled_name();
                            // Publish the specialized signature before the
                            // call is emitted. A generic `set` method has a
                            // `Void` return, and without this early fact the
                            // caller records a phantom SSA result before the
                            // pending specialization is lowered.
                            if let Some((method, type_params)) =
                                self.generic_impl_methods.get(&generic_key).cloned()
                            {
                                let mut type_map = HashMap::new();
                                for (type_param, concrete_type) in
                                    type_params.iter().zip(request.concrete_types.iter())
                                {
                                    type_map.insert(type_param.name.clone(), concrete_type.clone());
                                }
                                let return_type = method
                                    .return_type
                                    .as_ref()
                                    .map(|annotation| {
                                        self.lower_type_annotation_with_map(annotation, &type_map)
                                    })
                                    .unwrap_or(IRType::Void);
                                let return_type = if method.is_async {
                                    IRType::Task {
                                        output: Box::new(return_type),
                                    }
                                } else {
                                    return_type
                                };
                                self.function_return_types
                                    .entry(mangled.clone())
                                    .or_insert(return_type);
                            }
                            if !self.generated_specializations.contains_key(&mangled) {
                                self.pending_method_specializations.push(request);
                            }
                        }
                    }
                }

                // 4. Lower argumentos
                let mut call_args = vec![obj_value]; // self é o primeiro argumento
                for arg in arguments {
                    let arg_value = self.lower_expression(arg, ir_func);
                    call_args.push(arg_value);
                }

                // 5. Fazer a chamada de função
                let resolved_symbol = self.resolve_user_function_symbol(&function_name);
                let mut hidden_positions = self.hidden_size_positions(&function_name);
                if hidden_positions.is_empty() {
                    hidden_positions = self.hidden_size_positions(&resolved_symbol);
                }
                for position in hidden_positions {
                    // `self` is position 0 and is never an array; later
                    // positions map to `arguments[position - 1]`.
                    let value = if position == 0 {
                        self.builder.build_const_int(ir_func, 0)
                    } else {
                        match arguments.get(position - 1) {
                            Some(argument) => self.hidden_size_argument(argument, ir_func),
                            None => self.builder.build_const_int(ir_func, 0),
                        }
                    };
                    call_args.push(value);
                }
                if self.user_function_returns_unit(&function_name)
                    || self.user_function_returns_unit(&resolved_symbol)
                {
                    self.builder
                        .build_call(ir_func, resolved_symbol, call_args, false);
                    return self.builder.build_const_int(ir_func, 0);
                }
                self.require_value(
                    self.builder
                        .build_call(ir_func, resolved_symbol, call_args, true),
                    "method call did not produce its declared result",
                )
            }
            _ => unreachable!("lowering expression category mismatch"),
        }
    }
}
