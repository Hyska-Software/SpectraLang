impl CodeGenerator {
    /// Convert a call argument to the callee parameter's Cranelift type.
    /// Char/bool/exact-width parameters use narrow Cranelift types (I8/I16/I32)
    /// while Spectra constants and most values are I64; without conversion the
    /// Cranelift verifier rejects the call.
    fn coerce_call_arg(builder: &mut FunctionBuilder, value: Value, param_ty: &IRType) -> Value {
        use spectra_midend::ir::IntWidth;
        let target = match param_ty {
            IRType::Char => types::I32,
            IRType::Bool => types::I8,
            IRType::ExactInt { width, .. } => match width {
                IntWidth::I8 => types::I8,
                IntWidth::I16 => types::I16,
                IntWidth::I32 => types::I32,
                IntWidth::I64 | IntWidth::Isize | IntWidth::Usize => types::I64,
            },
            _ => return value,
        };
        let current = builder.func.dfg.value_type(value);
        if current == target {
            return value;
        }
        match (current, target) {
            (types::I64, types::I8 | types::I16 | types::I32) => {
                builder.ins().ireduce(target, value)
            }
            (types::I8 | types::I16 | types::I32, types::I64) => {
                builder.ins().uextend(target, value)
            }
            _ => value,
        }
    }

    fn host_argument_to_i64(builder: &mut FunctionBuilder, value: Value) -> BackendResult<Value> {        Ok(match builder.func.dfg.value_type(value) {
            types::I64 => value,
            types::I8 | types::I16 | types::I32 => builder.ins().sextend(types::I64, value),
            types::F64 => builder.ins().bitcast(types::I64, MemFlags::new(), value),
            types::F32 => {
                let promoted = builder.ins().fpromote(types::F64, value);
                builder.ins().bitcast(types::I64, MemFlags::new(), promoted)
            }
            other => return Err(BackendCodegenError::unsupported_host_argument_type(other)),
        })
    }

    fn host_name_pointer<M: Module>(
        module: &mut M,
        builder: &mut FunctionBuilder,
        record: HostNameRecord,
    ) -> Value {
        if let Some(data_id) = record.data_id {
            let gv = module.declare_data_in_func(data_id, builder.func);
            builder.ins().global_value(types::I64, gv)
        } else {
            builder.ins().iconst(types::I64, record.ptr as i64)
        }
    }

    fn host_cache_pointer<M: Module>(
        module: &mut M,
        builder: &mut FunctionBuilder,
        record: HostCallSiteRecord,
    ) -> Value {
        if let Some(data_id) = record.cache_data_id {
            let gv = module.declare_data_in_func(data_id, builder.func);
            builder.ins().global_value(types::I64, gv)
        } else {
            builder.ins().iconst(types::I64, record.cache_ptr as i64)
        }
    }

    fn convert_host_result_value(
        builder: &mut FunctionBuilder,
        raw_value: Value,
        result_type: Option<&IRType>,
    ) -> BackendResult<Value> {
        Ok(match result_type {
            Some(IRType::Float) => builder
                .ins()
                .bitcast(types::F64, MemFlags::new(), raw_value),
            Some(IRType::Bool) => builder.ins().ireduce(types::I8, raw_value),
            Some(IRType::Char) => builder.ins().ireduce(types::I32, raw_value),
            Some(IRType::ExactInt { .. }) => {
                let target = Self::ir_type_to_cranelift(result_type.unwrap())?;
                if target == types::I64 {
                    raw_value
                } else {
                    builder.ins().ireduce(target, raw_value)
                }
            }
            Some(IRType::ExactFloat { width }) => match width {
                spectra_midend::ir::FloatWidth::F32 => {
                    let f64_value = builder
                        .ins()
                        .bitcast(types::F64, MemFlags::new(), raw_value);
                    builder.ins().fdemote(types::F32, f64_value)
                }
                spectra_midend::ir::FloatWidth::F64 => {
                    builder
                        .ins()
                        .bitcast(types::F64, MemFlags::new(), raw_value)
                }
            },
            _ => raw_value,
        })
    }

    fn generate_hostcall_batch<M: Module>(
        module: &mut M,
        host_call_sites: &HashMap<String, HostCallSiteRecord>,
        host_invoke_cached_batch_func: FuncId,
        builder: &mut FunctionBuilder,
        instructions: &[Instruction],
        value_map: &mut DenseValueMap,
        batch_stats: &mut HostCallBatchStats,
    ) -> BackendResult<()> {
        let call_count = instructions.len();
        let descriptor_bytes = call_count
            .checked_mul(Self::R3105_BATCH_DESCRIPTOR_BYTES)
            .ok_or_else(|| {
                BackendCodegenError::invalid_ir("hostcall batch descriptor size overflow")
            })?;
        let mut argument_words = 0usize;
        let mut result_slots = 0usize;
        for instruction in instructions {
            let InstructionKind::HostCall { args, result, .. } = &instruction.kind else {
                return Err(BackendCodegenError::invalid_ir(
                    "non-hostcall in planned hostcall batch",
                ));
            };
            argument_words = argument_words.checked_add(args.len()).ok_or_else(|| {
                BackendCodegenError::invalid_ir("hostcall argument size overflow")
            })?;
            result_slots += usize::from(result.is_some());
        }
        let argument_bytes = argument_words
            .checked_mul(8)
            .ok_or_else(|| BackendCodegenError::invalid_ir("hostcall argument bytes overflow"))?;
        let result_bytes = result_slots
            .checked_mul(8)
            .ok_or_else(|| BackendCodegenError::invalid_ir("hostcall result bytes overflow"))?;
        if descriptor_bytes + argument_bytes + result_bytes > Self::R3105_MAX_BATCH_STACK_BYTES {
            return Err(BackendCodegenError::invalid_ir(
                "hostcall batch exceeds the bounded stack budget",
            ));
        }

        batch_stats.batched_sites += 1;
        batch_stats.batched_hostcalls += call_count;
        batch_stats.argument_arena_bytes += argument_bytes;
        batch_stats.result_arena_bytes += result_bytes;

        let descriptor_slot =
            builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                descriptor_bytes as u32,
                0,
            ));
        let argument_slot = if argument_bytes > 0 {
            Some(
                builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                    cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                    argument_bytes as u32,
                    0,
                )),
            )
        } else {
            None
        };
        let result_slot = if result_bytes > 0 {
            Some(
                builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                    cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                    result_bytes as u32,
                    0,
                )),
            )
        } else {
            None
        };
        let descriptor_base = builder.ins().stack_addr(types::I64, descriptor_slot, 0);
        let argument_base = argument_slot.map(|slot| builder.ins().stack_addr(types::I64, slot, 0));
        let result_base = result_slot.map(|slot| builder.ins().stack_addr(types::I64, slot, 0));

        let get_value = |value: &IRValue| -> BackendResult<Value> {
            value_map
                .get(value.id)
                .ok_or_else(|| BackendCodegenError::missing_value(value.id))
        };
        let mut argument_offset = 0i32;
        let mut result_offset = 0i32;
        for (index, instruction) in instructions.iter().enumerate() {
            let InstructionKind::HostCall {
                result, host, args, ..
            } = &instruction.kind
            else {
                unreachable!("hostcall batch was prevalidated");
            };
            let record = *host_call_sites.get(host).ok_or_else(|| {
                BackendCodegenError::cranelift(format!(
                    "host call name '{}' was not pre-interned",
                    host
                ))
            })?;
            let descriptor_offset = (index * Self::R3105_BATCH_DESCRIPTOR_BYTES) as i32;
            let cache_ptr = Self::host_cache_pointer(module, builder, record);
            let name_ptr = Self::host_name_pointer(module, builder, record.name);
            let name_len = builder.ins().iconst(types::I64, record.name.len as i64);
            let args_ptr = if args.is_empty() {
                builder.ins().iconst(types::I64, 0)
            } else {
                let base = argument_base.expect("non-empty batch arguments have an arena");
                builder.ins().iadd_imm(base, argument_offset as i64)
            };
            let arg_len = builder.ins().iconst(types::I64, args.len() as i64);
            let results_ptr = if result.is_some() {
                let base = result_base.expect("batch results have an arena");
                builder.ins().iadd_imm(base, result_offset as i64)
            } else {
                builder.ins().iconst(types::I64, 0)
            };
            let result_len = builder
                .ins()
                .iconst(types::I64, i64::from(result.is_some()));

            for (arg_index, arg) in args.iter().enumerate() {
                let value = Self::host_argument_to_i64(builder, get_value(arg)?)?;
                builder.ins().store(
                    MemFlags::new(),
                    value,
                    argument_base.expect("non-empty batch arguments have an arena"),
                    argument_offset + (arg_index as i32 * 8),
                );
            }
            builder.ins().store(
                MemFlags::new(),
                cache_ptr,
                descriptor_base,
                descriptor_offset,
            );
            builder.ins().store(
                MemFlags::new(),
                name_ptr,
                descriptor_base,
                descriptor_offset + 8,
            );
            builder.ins().store(
                MemFlags::new(),
                name_len,
                descriptor_base,
                descriptor_offset + 16,
            );
            builder.ins().store(
                MemFlags::new(),
                args_ptr,
                descriptor_base,
                descriptor_offset + 24,
            );
            builder.ins().store(
                MemFlags::new(),
                arg_len,
                descriptor_base,
                descriptor_offset + 32,
            );
            builder.ins().store(
                MemFlags::new(),
                results_ptr,
                descriptor_base,
                descriptor_offset + 40,
            );
            builder.ins().store(
                MemFlags::new(),
                result_len,
                descriptor_base,
                descriptor_offset + 48,
            );

            argument_offset += (args.len() as i32) * 8;
            if result.is_some() {
                result_offset += 8;
            }
        }

        let func_ref = module.declare_func_in_func(host_invoke_cached_batch_func, builder.func);
        let call_count_value = builder.ins().iconst(types::I64, call_count as i64);
        let call = builder
            .ins()
            .call(func_ref, &[descriptor_base, call_count_value]);
        let status = builder.inst_results(call)[0];
        let zero = builder.ins().iconst(types::I32, 0);
        let is_ok = builder.ins().icmp(IntCC::Equal, status, zero);
        let success_block = builder.create_block();
        let failure_block = builder.create_block();
        builder
            .ins()
            .brif(is_ok, success_block, &[], failure_block, &[]);

        builder.switch_to_block(failure_block);
        builder
            .ins()
            .trap(cranelift::codegen::ir::TrapCode::user(1).unwrap());
        builder.seal_block(failure_block);

        builder.switch_to_block(success_block);
        builder.seal_block(success_block);
        let mut loaded_result_offset = 0i32;
        for instruction in instructions {
            let InstructionKind::HostCall {
                result,
                result_type,
                ..
            } = &instruction.kind
            else {
                unreachable!("hostcall batch was prevalidated");
            };
            if let Some(result_value) = result {
                let raw_value = builder.ins().load(
                    types::I64,
                    MemFlags::new(),
                    result_base.expect("batch results have an arena"),
                    loaded_result_offset,
                );
                let value =
                    Self::convert_host_result_value(builder, raw_value, result_type.as_ref())?;
                value_map.insert(result_value.id, value);
                loaded_result_offset += 8;
            }
        }

        Ok(())
    }
}
