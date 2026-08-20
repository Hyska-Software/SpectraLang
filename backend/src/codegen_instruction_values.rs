impl CodeGenerator {
    #[allow(clippy::too_many_arguments)]
    fn generate_value_instruction<M: Module>(
        module: &mut M,
        hostcall: &mut HostCallLoweringContext<'_>,
        builder: &mut FunctionBuilder,
        kind: &InstructionKind,
        value_map: &mut DenseValueMap,
        string_literal_lengths: &mut HashMap<usize, i64>,
        block_map: &HashMap<usize, Block>,
        phi_map: &HashMap<usize, Vec<PhiDescriptor>>,
        current_block_id: usize,
    ) -> BackendResult<()> {
        let string_literal_data = &mut hostcall.string_literal_data;
        let string_literal_storage = &mut hostcall.string_literal_storage;

        match kind {
            // PHI nodes are lowered to Cranelift block parameters.
            // The block was already created with the appropriate parameters in
            // define_function(). Here we simply read the parameter that
            // corresponds to this PHI and expose it in the value_map.
            InstructionKind::Phi { result, .. } => {
                let block = *block_map
                    .get(&current_block_id)
                    .ok_or_else(|| BackendCodegenError::missing_block(current_block_id))?;
                if let Some(phis) = phi_map.get(&current_block_id) {
                    for (idx, phi) in phis.iter().enumerate() {
                        if phi.result_id == result.id {
                            let phi_val = builder.block_params(block)[idx];
                            value_map.insert(result.id, phi_val);
                            break;
                        }
                    }
                }
            }

            // Constant instructions
            InstructionKind::ConstInt { result, value } => {
                let result_val = builder.ins().iconst(types::I64, *value);
                value_map.insert(result.id, result_val);
            }

            InstructionKind::ConstIntTyped { result, value, ty } => {
                let cl_ty = Self::ir_type_to_cranelift(ty)?;
                let result_val = builder.ins().iconst(cl_ty, *value);
                value_map.insert(result.id, result_val);
            }

            InstructionKind::ConstFloat { result, value } => {
                let result_val = builder.ins().f64const(*value);
                value_map.insert(result.id, result_val);
            }

            InstructionKind::ConstFloatTyped { result, value, ty } => {
                let result_val = match Self::ir_type_to_cranelift(ty)? {
                    types::F32 => builder.ins().f32const(*value as f32),
                    types::F64 => builder.ins().f64const(*value),
                    other => {
                        return Err(BackendCodegenError::invalid_ir(format!(
                            "floating constant requires f32/f64, got {other:?}"
                        )))
                    }
                };
                value_map.insert(result.id, result_val);
            }

            InstructionKind::ConstBool { result, value } => {
                let result_val = builder.ins().iconst(types::I8, if *value { 1 } else { 0 });
                value_map.insert(result.id, result_val);
            }

            InstructionKind::ConstString { result, value } => {
                // R-3126: resolve to a stable pointer. In JIT mode this
                // allocates a heap buffer (kept alive in
                // `string_literal_storage`); in AOT mode the entry was
                // pre-populated by `pre_intern_string_literals` and we
                // emit a `global_value` referencing the `.rodata` section.
                let record =
                    intern_string_literal(string_literal_data, string_literal_storage, value);
                let ptr_val = if let Some(data_id) = record.data_id {
                    let gv = module.declare_data_in_func(data_id, builder.func);
                    builder.ins().global_value(types::I64, gv)
                } else {
                    builder.ins().iconst(types::I64, record.ptr as i64)
                };
                value_map.insert(result.id, ptr_val);
                string_literal_lengths.insert(result.id, record.len_with_null);
            }
            _ => unreachable!("value instruction category mismatch"),
        }
        Ok(())
    }
}
