use super::*;

impl ASTLowering {
    pub(crate) fn lower_expression_struct(&mut self, expr: &Expression, ir_func: &mut IRFunction) -> Value {
        match &expr.kind {
            ExpressionKind::StructLiteral {
                name,
                fields,
                type_args,
            } => {
                let (actual_name, field_defs) =
                    self.ensure_struct_definition(name, type_args.as_slice());

                // Criar tipo struct
                let struct_type = IRType::Struct {
                    name: actual_name.clone(),
                    fields: field_defs.clone(),
                };

                // Alocar espaço para o struct no stack
                let struct_ptr = self.builder.build_alloca(ir_func, struct_type);

                // Layout com padding para os campos
                let struct_layout = layout::layout_of(field_defs.iter().map(|(_, ty)| ty));

                // Inicializar cada campo
                for (field_name, field_expr) in fields.iter() {
                    let Some((field_idx, field_type)) = field_defs
                        .iter()
                        .enumerate()
                        .find(|(_, (fname, _))| fname == field_name)
                        .map(|(idx, (_, ty))| (idx, ty.clone()))
                    else {
                        return self.invalid_value(format!(
                            "field '{}' not found in struct '{}' definition",
                            field_name, actual_name
                        ));
                    };
                    let field_value =
                        self.lower_expression_as_type(field_expr, &field_type, ir_func);

                    let byte_offset = struct_layout
                        .offsets
                        .get(field_idx)
                        .copied()
                        .unwrap_or(field_idx * 8) as i64;
                    let field_ptr = self
                        .builder
                        .build_field_ptr(ir_func, struct_ptr, byte_offset);

                    self.builder.build_store(ir_func, field_ptr, field_value);
                }

                // Retornar ponteiro para o struct
                struct_ptr
            }
            _ => unreachable!("lowering expression category mismatch"),
        }
    }
}
