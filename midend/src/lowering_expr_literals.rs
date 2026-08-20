impl ASTLowering {
    fn lower_expression_literals(&mut self, expr: &Expression, ir_func: &mut IRFunction) -> Value {
        match &expr.kind {
            ExpressionKind::NumberLiteral(n) => {
                // Try to parse as integer first, then float
                if let Ok(int_val) = n.parse::<i64>() {
                    if let Some(annotation) = self.current_expected_annotation.clone() {
                        let ty = self.lower_type_annotation(&annotation);
                        if matches!(ty, IRType::ExactInt { .. }) {
                            self.builder.build_const_int_typed(ir_func, int_val, ty)
                        } else {
                            self.builder.build_const_int(ir_func, int_val)
                        }
                    } else {
                        self.builder.build_const_int(ir_func, int_val)
                    }
                } else if let Ok(float_val) = n.parse::<f64>() {
                    if let Some(annotation) = self.current_expected_annotation.clone() {
                        let ty = self.lower_type_annotation(&annotation);
                        if matches!(ty, IRType::ExactFloat { .. }) {
                            self.builder.build_const_float_typed(ir_func, float_val, ty)
                        } else {
                            self.builder.build_const_float(ir_func, float_val)
                        }
                    } else {
                        self.builder.build_const_float(ir_func, float_val)
                    }
                } else {
                    self.invalid_value(format!("numeric literal '{}' could not be lowered", n))
                }
            }
            ExpressionKind::StringLiteral(s) => self.lower_string_literal(s, ir_func),
            ExpressionKind::BoolLiteral(b) => self.builder.build_const_bool(ir_func, *b),
            ExpressionKind::Identifier(name) => {
                // Check if this is an array - return pointer directly
                if let Some(value) = self.const_values.get(name).cloned() {
                    self.emit_const_value(&value, ir_func)
                }
                // Module-level mutable statics are addressed through an IR
                // global and loaded on every use; they are not copied into a
                // function-local SSA map.
                else if let Some(value) = self.lower_global_value(name, ir_func) {
                    value
                }
                // Check if this is an array - return pointer directly
                else if let Some(info) = self.array_map.get(name) {
                    info.ptr
                }
                // Check if this is a struct variable
                else if let Some((struct_ptr, _)) = self.struct_var_map.get(name) {
                    // Struct variables are represented as pointers — return the pointer directly.
                    // Field access via FieldAccess/struct_var_map uses the pointer for GEP;
                    // method calls receive the pointer as `self`.
                    struct_ptr
                }
                // Check if variable is in memory (mutable)
                else if let Some(&alloca_ptr) = self.alloca_map.get(name) {
                    // Load from memory
                    self.builder.build_load(ir_func, alloca_ptr)
                } else if let Some(value) = self.value_map.get(name) {
                    // Use SSA value directly
                    value
                } else if self.function_parameter_types.contains_key(name)
                    && self.function_return_types.contains_key(name)
                {
                    self.lower_named_function_value(name, ir_func)
                } else {
                    self.invalid_value(format!(
                        "unresolved identifier '{}' during lowering",
                        name
                    ))
                }
            }
            _ => unreachable!("lowering expression category mismatch"),
        }
    }
}
