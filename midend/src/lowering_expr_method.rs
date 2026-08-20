impl ASTLowering {
    fn lower_expression_method(&mut self, expr: &Expression, ir_func: &mut IRFunction) -> Value {
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
                    let result = self.builder.build_typed_host_call(
                        ir_func,
                        desc.runtime_name.to_string(),
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
                        result.unwrap_or_else(|| self.builder.build_const_int(ir_func, 0))
                    };
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
                let obj_type_name = if let Some(name) = type_name {
                    // Tipo já foi preenchido pelo semantic analyzer
                    name.clone()
                } else {
                    match self.infer_expr_ir_type(object) {
                        IRType::Struct { name, .. } => name,
                        IRType::Enum { name, .. } => name,
                        other => {
                            return self.invalid_value(format!(
                                "Could not determine object type for method call '{method_name}' (inferred type: {:?})",
                                other
                            ));
                        }
                    }
                };

                if method_name == "to_json" {
                    return self.lower_string_literal("{}", ir_func);
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
                self.require_value(
                    self.builder.build_call(ir_func, function_name, call_args, true),
                    "method call did not produce its declared result",
                )
            }
            _ => unreachable!("lowering expression category mismatch"),
        }
    }
}
