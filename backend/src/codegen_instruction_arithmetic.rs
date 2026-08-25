impl CodeGenerator {
    fn generate_arithmetic_instruction<M: Module>(
        module: &mut M,
        hostcall: &mut HostCallLoweringContext<'_>,
        builder: &mut FunctionBuilder,
        kind: &InstructionKind,
        value_map: &mut DenseValueMap,
    ) -> BackendResult<()> {
        let get_value = |v: &IRValue| -> BackendResult<Value> {
            value_map
                .get(v.id)
                .ok_or_else(|| BackendCodegenError::missing_value(v.id))
        };

        let promote_float_operands =
            |builder: &mut FunctionBuilder, lhs: Value, rhs: Value| -> (Value, Value, bool) {
                let lhs_ty = builder.func.dfg.value_type(lhs);
                let rhs_ty = builder.func.dfg.value_type(rhs);
                if lhs_ty == types::F64 || rhs_ty == types::F64 {
                    let lhs = if lhs_ty == types::F32 {
                        builder.ins().fpromote(types::F64, lhs)
                    } else {
                        lhs
                    };
                    let rhs = if rhs_ty == types::F32 {
                        builder.ins().fpromote(types::F64, rhs)
                    } else {
                        rhs
                    };
                    (lhs, rhs, true)
                } else {
                    (lhs, rhs, lhs_ty == types::F32 && rhs_ty == types::F32)
                }
            };

        match kind {
            // Arithmetic operations
            InstructionKind::Add { result, lhs, rhs } => {
                let lhs_val = get_value(lhs)?;
                let rhs_val = get_value(rhs)?;
                let (lhs_val, rhs_val, is_float) =
                    promote_float_operands(builder, lhs_val, rhs_val);
                let result_val = if is_float {
                    builder.ins().fadd(lhs_val, rhs_val)
                } else {
                    builder.ins().iadd(lhs_val, rhs_val)
                };
                value_map.insert(result.id, result_val);
            }

            InstructionKind::Sub { result, lhs, rhs } => {
                let lhs_val = get_value(lhs)?;
                let rhs_val = get_value(rhs)?;
                let (lhs_val, rhs_val, is_float) =
                    promote_float_operands(builder, lhs_val, rhs_val);
                let result_val = if is_float {
                    builder.ins().fsub(lhs_val, rhs_val)
                } else {
                    builder.ins().isub(lhs_val, rhs_val)
                };
                value_map.insert(result.id, result_val);
            }

            InstructionKind::Mul { result, lhs, rhs } => {
                let lhs_val = get_value(lhs)?;
                let rhs_val = get_value(rhs)?;
                let (lhs_val, rhs_val, is_float) =
                    promote_float_operands(builder, lhs_val, rhs_val);
                let result_val = if is_float {
                    builder.ins().fmul(lhs_val, rhs_val)
                } else {
                    builder.ins().imul(lhs_val, rhs_val)
                };
                value_map.insert(result.id, result_val);
            }

            InstructionKind::Div { result, lhs, rhs } => {
                let lhs_val = get_value(lhs)?;
                let rhs_val = get_value(rhs)?;
                let (lhs_val, rhs_val, is_float) =
                    promote_float_operands(builder, lhs_val, rhs_val);
                let result_val = if is_float {
                    builder.ins().fdiv(lhs_val, rhs_val)
                } else {
                    Self::emit_checked_int_divrem(
                        module,
                        hostcall,
                        builder,
                        lhs_val,
                        rhs_val,
                        "integer division by zero",
                        false,
                    )?
                };
                value_map.insert(result.id, result_val);
            }

            InstructionKind::Rem { result, lhs, rhs } => {
                let lhs_val = get_value(lhs)?;
                let rhs_val = get_value(rhs)?;
                let (lhs_val, rhs_val, is_float) =
                    promote_float_operands(builder, lhs_val, rhs_val);
                let result_val = if is_float {
                    // fmod(x, 0) is IEEE 754 NaN: no zero-divisor check.
                    Self::emit_float_remainder(module, builder, lhs_val, rhs_val)?
                } else {
                    Self::emit_checked_int_divrem(
                        module,
                        hostcall,
                        builder,
                        lhs_val,
                        rhs_val,
                        "integer remainder by zero",
                        true,
                    )?
                };
                value_map.insert(result.id, result_val);
            }

            // Comparison operations
            InstructionKind::Eq { result, lhs, rhs } => {
                let lhs_val = get_value(lhs)?;
                let rhs_val = get_value(rhs)?;
                let (lhs_val, rhs_val, is_float) =
                    promote_float_operands(builder, lhs_val, rhs_val);
                let result_val = if is_float {
                    builder.ins().fcmp(FloatCC::Equal, lhs_val, rhs_val)
                } else {
                    builder.ins().icmp(IntCC::Equal, lhs_val, rhs_val)
                };
                value_map.insert(result.id, result_val);
            }

            InstructionKind::Ne { result, lhs, rhs } => {
                let lhs_val = get_value(lhs)?;
                let rhs_val = get_value(rhs)?;
                let (lhs_val, rhs_val, is_float) =
                    promote_float_operands(builder, lhs_val, rhs_val);
                let result_val = if is_float {
                    builder.ins().fcmp(FloatCC::NotEqual, lhs_val, rhs_val)
                } else {
                    builder.ins().icmp(IntCC::NotEqual, lhs_val, rhs_val)
                };
                value_map.insert(result.id, result_val);
            }

            InstructionKind::Lt { result, lhs, rhs } => {
                let lhs_val = get_value(lhs)?;
                let rhs_val = get_value(rhs)?;
                let (lhs_val, rhs_val, is_float) =
                    promote_float_operands(builder, lhs_val, rhs_val);
                let result_val = if is_float {
                    builder.ins().fcmp(FloatCC::LessThan, lhs_val, rhs_val)
                } else {
                    builder.ins().icmp(IntCC::SignedLessThan, lhs_val, rhs_val)
                };
                value_map.insert(result.id, result_val);
            }

            InstructionKind::Le { result, lhs, rhs } => {
                let lhs_val = get_value(lhs)?;
                let rhs_val = get_value(rhs)?;
                let (lhs_val, rhs_val, is_float) =
                    promote_float_operands(builder, lhs_val, rhs_val);
                let result_val = if is_float {
                    builder
                        .ins()
                        .fcmp(FloatCC::LessThanOrEqual, lhs_val, rhs_val)
                } else {
                    builder
                        .ins()
                        .icmp(IntCC::SignedLessThanOrEqual, lhs_val, rhs_val)
                };
                value_map.insert(result.id, result_val);
            }

            InstructionKind::Gt { result, lhs, rhs } => {
                let lhs_val = get_value(lhs)?;
                let rhs_val = get_value(rhs)?;
                let (lhs_val, rhs_val, is_float) =
                    promote_float_operands(builder, lhs_val, rhs_val);
                let result_val = if is_float {
                    builder.ins().fcmp(FloatCC::GreaterThan, lhs_val, rhs_val)
                } else {
                    builder
                        .ins()
                        .icmp(IntCC::SignedGreaterThan, lhs_val, rhs_val)
                };
                value_map.insert(result.id, result_val);
            }

            InstructionKind::Ge { result, lhs, rhs } => {
                let lhs_val = get_value(lhs)?;
                let rhs_val = get_value(rhs)?;
                let (lhs_val, rhs_val, is_float) =
                    promote_float_operands(builder, lhs_val, rhs_val);
                let result_val = if is_float {
                    builder
                        .ins()
                        .fcmp(FloatCC::GreaterThanOrEqual, lhs_val, rhs_val)
                } else {
                    builder
                        .ins()
                        .icmp(IntCC::SignedGreaterThanOrEqual, lhs_val, rhs_val)
                };
                value_map.insert(result.id, result_val);
            }

            // Logical operations
            InstructionKind::And { result, lhs, rhs } => {
                let lhs_val = get_value(lhs)?;
                let rhs_val = get_value(rhs)?;
                let result_val = builder.ins().band(lhs_val, rhs_val);
                value_map.insert(result.id, result_val);
            }

            InstructionKind::Or { result, lhs, rhs } => {
                let lhs_val = get_value(lhs)?;
                let rhs_val = get_value(rhs)?;
                let result_val = builder.ins().bor(lhs_val, rhs_val);
                value_map.insert(result.id, result_val);
            }

            InstructionKind::Not { result, operand } => {
                let operand_val = get_value(operand)?;
                let result_val = builder.ins().icmp_imm(IntCC::Equal, operand_val, 0);
                value_map.insert(result.id, result_val);
            }

            _ => unreachable!("arithmetic instruction category mismatch"),
        }
        Ok(())
    }

    /// Lowers float remainder (`Rem` on F32/F64 operands).
    ///
    /// Cranelift has no `frem` instruction, so remainder is emitted as a call
    /// to the platform C runtime: `fmodf` for F32 and `fmod` for F64. The
    /// import is declared with `Linkage::Import`, which the JIT resolves
    /// against loaded modules (UCRT/glibc) and AOT leaves to the native
    /// linker. Declarations are idempotent, so repeated `Rem` sites reuse the
    /// same `FuncId`.
    fn emit_float_remainder<M: Module>(
        module: &mut M,
        builder: &mut FunctionBuilder,
        lhs: Value,
        rhs: Value,
    ) -> BackendResult<Value> {
        let ty = builder.func.dfg.value_type(lhs);
        let (symbol, scalar_ty) = if ty == types::F32 {
            ("fmodf", types::F32)
        } else {
            ("fmod", types::F64)
        };
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(scalar_ty));
        sig.params.push(AbiParam::new(scalar_ty));
        sig.returns.push(AbiParam::new(scalar_ty));
        let func_id = module
            .declare_function(symbol, Linkage::Import, &sig)
            .map_err(|e| {
                BackendCodegenError::cranelift(format!(
                    "Failed to declare '{symbol}' import: {e:?}"
                ))
            })?;
        let func_ref = module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(func_ref, &[lhs, rhs]);
        Ok(builder.inst_results(call)[0])
    }
}
