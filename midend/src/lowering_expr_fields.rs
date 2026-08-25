use super::*;

impl ASTLowering {
    pub(crate) fn lower_expression_field(&mut self, expr: &Expression, ir_func: &mut IRFunction) -> Value {
        match &expr.kind {
            ExpressionKind::FieldAccess { object, field } => {
                // Se o objeto é um identificador, buscar no struct_var_map
                if let ExpressionKind::Identifier(name) = &object.kind {
                    if let Some((struct_ptr, struct_name)) = self.struct_var_map.get(name) {
                        // Buscar definição do struct
                        if let Some(field_defs) = self.struct_definitions.get(&struct_name) {
                            // Encontrar índice do campo
                            if let Some((field_idx, (_, field_type))) = field_defs
                                .iter()
                                .enumerate()
                                .find(|(_, (fname, _))| fname == field)
                            {
                                // Field pointer com offset de layout padded
                                let byte_offset = layout::layout_of(
                                    field_defs.iter().map(|(_, ty)| ty),
                                )
                                .offsets
                                .get(field_idx)
                                .copied()
                                .unwrap_or(field_idx * 8) as i64;
                                let field_ptr = self
                                    .builder
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
                if let IRType::Struct {
                    fields: field_defs, ..
                } = self.ir_type_representation(&object_type)
                {
                    if let Some((field_idx, field_ty)) = field_defs
                        .iter()
                        .enumerate()
                        .find(|(_, (fname, _))| fname == field)
                        .map(|(idx, (_, ty))| (idx, ty.clone()))
                    {
                        let byte_offset = layout::layout_of(
                            field_defs.iter().map(|(_, ty)| ty),
                        )
                        .offsets
                        .get(field_idx)
                        .copied()
                        .unwrap_or(field_idx * 8) as i64;
                        let field_ptr =
                            self.builder
                                .build_field_ptr(ir_func, object_ptr, byte_offset);
                        return self
                            .builder
                            .build_load_typed(ir_func, field_ptr, field_ty.clone());
                    }
                }

                self.invalid_value(format!(
                    "unresolved field '{}' during lowering",
                    field
                ))
            }
            _ => unreachable!("lowering expression category mismatch"),
        }
    }
}
