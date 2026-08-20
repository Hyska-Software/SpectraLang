impl CodeGenerator {
    fn generate_cast_instruction(
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
            // Copy operation
            InstructionKind::Copy { result, source } => {
                let source_val = get_value(source)?;
                value_map.insert(result.id, source_val);
            }

            // Cast instruction
            InstructionKind::Cast {
                result,
                operand,
                from_ty,
                to_ty,
            } => {
                let operand_val = get_value(operand)?;
                let operand_cl_ty = builder.func.dfg.value_type(operand_val);
                let cl_val = match (from_ty, to_ty) {
                    (IRType::Int, IRType::Float) => {
                        builder.ins().fcvt_from_sint(types::F64, operand_val)
                    }
                    (IRType::Float, IRType::Int) => {
                        builder.ins().fcvt_to_sint_sat(types::I64, operand_val)
                    }
                    (IRType::Char, IRType::Int) => match operand_cl_ty {
                        types::I8 | types::I16 | types::I32 => {
                            builder.ins().uextend(types::I64, operand_val)
                        }
                        types::I64 => operand_val,
                        _ => operand_val,
                    },
                    (IRType::Int, IRType::Char) => match operand_cl_ty {
                        types::I64 => builder.ins().ireduce(types::I32, operand_val),
                        types::I32 => operand_val,
                        types::I8 | types::I16 => builder.ins().uextend(types::I32, operand_val),
                        _ => operand_val,
                    },
                    (IRType::ExactInt { signed, .. }, IRType::ExactFloat { width: _ }) => {
                        let target = Self::ir_type_to_cranelift(to_ty)?;
                        if *signed {
                            builder.ins().fcvt_from_sint(target, operand_val)
                        } else {
                            builder.ins().fcvt_from_uint(target, operand_val)
                        }
                    }
                    (IRType::ExactInt { signed, .. }, IRType::Float) => {
                        if *signed {
                            builder.ins().fcvt_from_sint(types::F64, operand_val)
                        } else {
                            builder.ins().fcvt_from_uint(types::F64, operand_val)
                        }
                    }
                    (IRType::ExactInt { signed, .. }, IRType::Int) => {
                        if operand_cl_ty == types::I64 {
                            operand_val
                        } else if *signed {
                            builder.ins().sextend(types::I64, operand_val)
                        } else {
                            builder.ins().uextend(types::I64, operand_val)
                        }
                    }
                    (IRType::Int, IRType::ExactInt { .. }) => {
                        let target = Self::ir_type_to_cranelift(to_ty)?;
                        if builder.func.dfg.value_type(operand_val) == target {
                            operand_val
                        } else {
                            builder.ins().ireduce(target, operand_val)
                        }
                    }
                    (IRType::Int, IRType::ExactFloat { .. }) => builder
                        .ins()
                        .fcvt_from_sint(Self::ir_type_to_cranelift(to_ty)?, operand_val),
                    (IRType::ExactFloat { .. }, IRType::ExactInt { signed, .. }) => {
                        let target = Self::ir_type_to_cranelift(to_ty)?;
                        if *signed {
                            builder.ins().fcvt_to_sint_sat(target, operand_val)
                        } else {
                            builder.ins().fcvt_to_uint_sat(target, operand_val)
                        }
                    }
                    (IRType::ExactFloat { .. }, IRType::Float) => {
                        if operand_cl_ty == types::F32 {
                            builder.ins().fpromote(types::F64, operand_val)
                        } else {
                            operand_val
                        }
                    }
                    (IRType::Float, IRType::ExactFloat { .. }) => {
                        let target = Self::ir_type_to_cranelift(to_ty)?;
                        if target == types::F32 {
                            builder.ins().fdemote(types::F32, operand_val)
                        } else {
                            operand_val
                        }
                    }
                    (IRType::ExactFloat { .. }, IRType::Int) => {
                        builder.ins().fcvt_to_sint_sat(types::I64, operand_val)
                    }
                    (
                        IRType::ExactFloat { width: from_width },
                        IRType::ExactFloat { width: to_width },
                    ) => match (from_width, to_width) {
                        (
                            spectra_midend::ir::FloatWidth::F32,
                            spectra_midend::ir::FloatWidth::F64,
                        ) => builder.ins().fpromote(types::F64, operand_val),
                        (
                            spectra_midend::ir::FloatWidth::F64,
                            spectra_midend::ir::FloatWidth::F32,
                        ) => builder.ins().fdemote(types::F32, operand_val),
                        _ => operand_val,
                    },
                    (
                        IRType::ExactInt {
                            signed: from_signed,
                            width: _from_width,
                        },
                        IRType::ExactInt {
                            signed: to_signed,
                            width: _to_width,
                        },
                    ) => {
                        let target = Self::ir_type_to_cranelift(to_ty)?;
                        let source_bits = Self::type_size_bytes(from_ty) * 8;
                        let target_bits = Self::type_size_bytes(to_ty) * 8;
                        if source_bits > target_bits {
                            builder.ins().ireduce(target, operand_val)
                        } else if source_bits < target_bits {
                            if *from_signed {
                                builder.ins().sextend(target, operand_val)
                            } else {
                                builder.ins().uextend(target, operand_val)
                            }
                        } else if *from_signed == *to_signed {
                            operand_val
                        } else if *to_signed {
                            builder.ins().sextend(target, operand_val)
                        } else {
                            builder.ins().uextend(target, operand_val)
                        }
                    }
                    _ => operand_val, // same-type or struct->dyn: pass through
                };
                value_map.insert(result.id, cl_val);
            }
            _ => unreachable!("copy/cast instruction category mismatch"),
        }
        Ok(())
    }
}
