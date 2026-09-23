use super::*;

impl ASTLowering {
    pub(crate) fn lower_expression_literals(
        &mut self,
        expr: &Expression,
        ir_func: &mut IRFunction,
    ) -> Value {
        match &expr.kind {
            ExpressionKind::NumberLiteral(n) => {
                let expected_ir_type = self
                    .current_expected_annotation
                    .clone()
                    .map(|annotation| self.lower_type_annotation(&annotation))
                    .or_else(|| self.current_expected_ir_type.clone())
                    .or_else(|| {
                        self.resolved_expression_types
                            .get(&expr.span)
                            .cloned()
                            .map(|semantic_type| self.lower_type(&semantic_type))
                            .filter(|ty| !Self::ir_type_contains_unknown(ty))
                    });
                // The semantic pass accepts literals above i64::MAX only in
                // an unsigned exact-width context. Preserve their bits rather
                // than letting the shared i64 parser reinterpret them as f64.
                if !spectra_compiler::numeric::number_literal_is_float(n) {
                    if let Some(ty) = expected_ir_type.clone() {
                        if matches!(
                            &ty,
                            IRType::ExactInt {
                                signed: false,
                                width: _
                            }
                        ) {
                            if let Some(value) =
                                spectra_compiler::numeric::parse_number_literal_as_i128(n)
                            {
                                if value >= 0 && (value as u128) <= u64::MAX as u128 {
                                    return self.builder.build_const_int_typed(
                                        ir_func,
                                        value as u64 as i64,
                                        ty,
                                    );
                                }
                            }
                        }
                    }
                }
                // The token carries raw text; the shared parser understands
                // radix prefixes, `_` separators, and scientific notation.
                match spectra_compiler::numeric::parse_number_literal(n) {
                    Some(spectra_compiler::numeric::ParsedNumber::Int(int_val)) => {
                        if let Some(ty) = expected_ir_type.clone() {
                            if matches!(ty, IRType::ExactInt { .. }) {
                                self.builder.build_const_int_typed(ir_func, int_val, ty)
                            } else {
                                self.builder.build_const_int(ir_func, int_val)
                            }
                        } else {
                            self.builder.build_const_int(ir_func, int_val)
                        }
                    }
                    Some(spectra_compiler::numeric::ParsedNumber::Float(float_val)) => {
                        if let Some(ty) = expected_ir_type {
                            if matches!(ty, IRType::ExactFloat { .. }) {
                                self.builder.build_const_float_typed(ir_func, float_val, ty)
                            } else {
                                self.builder.build_const_float(ir_func, float_val)
                            }
                        } else {
                            self.builder.build_const_float(ir_func, float_val)
                        }
                    }
                    None => {
                        self.invalid_value(format!("numeric literal '{}' could not be lowered", n))
                    }
                }
            }
            ExpressionKind::StringLiteral(s) => self.lower_string_literal(s, ir_func),
            ExpressionKind::BoolLiteral(b) => self.builder.build_const_bool(ir_func, *b),
            ExpressionKind::Identifier(name) => {
                // Check if this is an array - return pointer directly
                if let Some(value) = self.const_values.get(name).cloned() {
                    let saved_expected = self.current_expected_ir_type.clone();
                    if self.current_expected_ir_type.is_none() {
                        self.current_expected_ir_type = self.const_types.get(name).cloned();
                    }
                    let value = self.emit_const_value(&value, ir_func);
                    self.current_expected_ir_type = saved_expected;
                    value
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
                // A reassigned record local keeps its current pointer in its
                // promoted slot; the slot is function-scoped while
                // `struct_var_map` is block-scoped, so the slot must win or
                // reads after a rebinding scope would observe a stale pointer.
                else if let Some(value) = self.load_slot(name, ir_func) {
                    value
                }
                // Check if this is a struct variable
                else if let Some((struct_ptr, _)) = self.struct_var_map.get(name) {
                    // Struct variables are represented as pointers — return the pointer directly.
                    // Field access via FieldAccess/struct_var_map uses the pointer for GEP;
                    // method calls receive the pointer as `self`.
                    struct_ptr
                } else if let Some(value) = self.value_map.get(name) {
                    // Use SSA value directly
                    value
                } else if self.function_parameter_types.contains_key(name)
                    && self.function_return_types.contains_key(name)
                {
                    self.lower_named_function_value(name, ir_func)
                } else {
                    self.invalid_value(format!("unresolved identifier '{}' during lowering", name))
                }
            }
            _ => self.invalid_value(
                "lower_expression_literals called with a non-literal expression",
            ),
        }
    }
}
