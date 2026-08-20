impl CodeGenerator {
    fn generate_async_instruction(
        builder: &mut FunctionBuilder,
        kind: &InstructionKind,
        value_map: &mut DenseValueMap,
    ) -> BackendResult<()> {
        let get_value = |v: &IRValue| -> BackendResult<Value> {
            value_map
                .get(v.id)
                .ok_or_else(|| BackendCodegenError::missing_value(v.id))
        };

        match kind {
            InstructionKind::AsyncSuspend { .. } | InstructionKind::AsyncResume { .. } => {}
            InstructionKind::AsyncReady {
                result,
                value,
                output_type,
            } => {
                let raw = if let Some(value) = value {
                    get_value(value)?
                } else {
                    builder.ins().iconst(types::I64, 0)
                };
                let value = match output_type {
                    IRType::Float => {
                        if builder.func.dfg.value_type(raw) == types::F64 {
                            builder.ins().bitcast(types::I64, MemFlags::new(), raw)
                        } else {
                            raw
                        }
                    }
                    IRType::Bool | IRType::Char => {
                        let ty = builder.func.dfg.value_type(raw);
                        if ty == types::I64 {
                            raw
                        } else {
                            builder.ins().uextend(types::I64, raw)
                        }
                    }
                    _ => raw,
                };
                value_map.insert(result.id, value);
            }
            _ => unreachable!("async instruction category mismatch"),
        }
        Ok(())
    }
}
