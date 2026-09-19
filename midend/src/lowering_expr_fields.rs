use super::*;

impl ASTLowering {
    pub(crate) fn lower_expression_field(
        &mut self,
        expr: &Expression,
        ir_func: &mut IRFunction,
    ) -> Value {
        match &expr.kind {
            ExpressionKind::FieldAccess { object, field } => {
                // A module-qualified function value (`handlers.health`) has no
                // struct receiver: materialize it as a zero-capture closure
                // like a bare named function (see lower_named_function_value).
                if let Some(path) = qualified_namespace_path(expr) {
                    if self.imported_function_symbols.contains_key(&path)
                        || (self.function_parameter_types.contains_key(&path)
                            && self.function_return_types.contains_key(&path))
                    {
                        return self.lower_named_function_value(&path, ir_func);
                    }
                }
                // Se o objeto é um identificador, buscar no struct_var_map.
                // A reassigned local keeps its current pointer in its promoted
                // slot, so let the generic path below resolve the base through
                // the slot instead of the (block-scoped, possibly stale) map.
                if let ExpressionKind::Identifier(name) = &object.kind {
                    if let Some((struct_ptr, struct_name)) = self
                        .struct_var_map
                        .get(name)
                        .filter(|_| !self.alloca_map.contains_key(name))
                    {
                        // Buscar definição do struct
                        if let Some(field_defs) = self.struct_definitions.get(&struct_name) {
                            // Encontrar índice do campo
                            if let Some((field_idx, (_, field_type))) = field_defs
                                .iter()
                                .enumerate()
                                .find(|(_, (fname, _))| fname == field)
                            {
                                // Padded layout offsets are total over the field
                                // list; a miss here would silently compute a
                                // wrong pointer, so fail loudly instead.
                                let Some(byte_offset) =
                                    layout::layout_of(field_defs.iter().map(|(_, ty)| ty))
                                        .offsets
                                        .get(field_idx)
                                        .copied()
                                else {
                                    return self.invalid_value(format!(
                                        "field layout for '{struct_name}.{field}' has no offset for field type {field_type:?}"
                                    ));
                                };
                                let byte_offset = byte_offset as i64;
                                let field_ptr =
                                    self.builder
                                        .build_field_ptr(ir_func, struct_ptr, byte_offset);

                                // Load do campo
                                return self.builder.build_load_typed(
                                    ir_func,
                                    field_ptr,
                                    field_type.clone(),
                                );
                            }
                        }
                    }
                }
                let object_ptr = self.lower_expression(object, ir_func);
                let object_type = self.infer_expr_ir_type(object);
                if let Some(field_defs) = self.struct_fields_for_type(&object_type) {
                    if let Some((field_idx, field_ty)) = field_defs
                        .iter()
                        .enumerate()
                        .find(|(_, (fname, _))| fname == field)
                        .map(|(idx, (_, ty))| (idx, ty.clone()))
                    {
                        let Some(byte_offset) =
                            layout::layout_of(field_defs.iter().map(|(_, ty)| ty))
                                .offsets
                                .get(field_idx)
                                .copied()
                        else {
                            let struct_name =
                                self.ir_nominal_name(&object_type).unwrap_or("unknown");
                            return self.invalid_value(format!(
                                "field layout for '{struct_name}.{field}' has no offset for field type {field_ty:?}"
                            ));
                        };
                        let byte_offset = byte_offset as i64;
                        let field_ptr =
                            self.builder
                                .build_field_ptr(ir_func, object_ptr, byte_offset);
                        return self
                            .builder
                            .build_load_typed(ir_func, field_ptr, field_ty.clone());
                    }
                }

                self.invalid_value(format!("unresolved field '{}' during lowering", field))
            }
            _ => unreachable!("lowering expression category mismatch"),
        }
    }
}
