fn const_i64(builder: &mut FunctionBuilder, value: i64) -> Value {
    builder.ins().iconst(types::I64, value)
}

impl CodeGenerator {
    fn runtime_call<M: Module>(
        module: &mut M,
        hostcall: &HostCallLoweringContext<'_>,
        builder: &mut FunctionBuilder,
        import: RuntimeImport,
        args: &[Value],
    ) -> Option<Value> {
        let func_ref = module.declare_func_in_func(hostcall.runtime_func(import), builder.func);
        let call = builder.ins().call(func_ref, args);
        builder.inst_results(call).first().copied()
    }

    fn frame_store_value(builder: &mut FunctionBuilder, value: Value) -> Value {
        match builder.func.dfg.value_type(value) {
            types::F64 => builder.ins().bitcast(types::I64, MemFlags::new(), value),
            types::F32 => builder.ins().uextend(types::I64, value),
            ty if ty != types::I64 => builder.ins().uextend(types::I64, value),
            _ => value,
        }
    }

    fn frame_load_value(
        builder: &mut FunctionBuilder,
        raw: Value,
        ty: &IRType,
    ) -> BackendResult<Value> {
        let target = Self::ir_type_to_cranelift(ty)?;
        Ok(match target {
            types::F64 => builder.ins().bitcast(types::F64, MemFlags::new(), raw),
            types::F32 => {
                let narrow = builder.ins().ireduce(types::I32, raw);
                builder.ins().bitcast(types::F32, MemFlags::new(), narrow)
            }
            types::I64 => raw,
            target => builder.ins().ireduce(target, raw),
        })
    }

    pub(crate) fn seed_async_source_values<M: Module>(
        module: &mut M,
        hostcall: &HostCallLoweringContext<'_>,
        builder: &mut FunctionBuilder,
        ir_func: &IRFunction,
        value_map: &mut DenseValueMap,
    ) -> BackendResult<()> {
        let Some(layout) = ir_func.async_layout.as_ref() else { return Ok(()) };
        if !ir_func.suspension_barrier || ir_func.name != layout.poll_name || ir_func.params.len() <= 3 {
            return Ok(());
        }
        let frame = value_map
            .get(ir_func.params[0].id)
            .ok_or_else(|| BackendCodegenError::missing_value(ir_func.params[0].id))?;
        for param in ir_func.params.iter().skip(3) {
            let slot = const_i64(builder, param.id as i64);
            let raw = Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutineFrameLoad, &[frame, slot])
                .ok_or_else(|| BackendCodegenError::cranelift("async source frame load returned no value"))?;
            value_map.insert(param.id, Self::frame_load_value(builder, raw, &param.ty)?);
        }
        Ok(())
    }

    fn generate_async_instruction<M: Module>(
        module: &mut M,
        function_map: &HashMap<String, FuncId>,
        hostcall: &HostCallLoweringContext<'_>,
        builder: &mut FunctionBuilder,
        kind: &InstructionKind,
        value_map: &mut DenseValueMap,
    ) -> BackendResult<()> {
        let get_value = |v: &IRValue| -> BackendResult<Value> {
            value_map.get(v.id).ok_or_else(|| BackendCodegenError::missing_value(v.id))
        };
        match kind {
            InstructionKind::FrameAlloc { result, slot_count, .. } => {
                let slots = const_i64(builder, *slot_count as i64);
                let value = Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutineFrameAlloc, &[slots])
                    .ok_or_else(|| BackendCodegenError::cranelift("frame allocation import returned no value"))?;
                value_map.insert(result.id, value);
            }
            InstructionKind::FrameStore { frame, slot, value } => {
                let frame = get_value(frame)?;
                let value = Self::frame_store_value(builder, get_value(value)?);
                let slot = const_i64(builder, *slot as i64);
                Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutineFrameStore, &[frame, slot, value]);
            }
            InstructionKind::FrameLoad { result, frame, slot, ty } => {
                let frame = get_value(frame)?;
                let slot = const_i64(builder, *slot as i64);
                let raw = Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutineFrameLoad, &[frame, slot])
                    .ok_or_else(|| BackendCodegenError::cranelift("frame load import returned no value"))?;
                value_map.insert(result.id, Self::frame_load_value(builder, raw, ty)?);
            }
            InstructionKind::StateLoad { result, frame } => {
                let frame = get_value(frame)?;
                let value = Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutineStateLoad, &[frame])
                    .ok_or_else(|| BackendCodegenError::cranelift("state load import returned no value"))?;
                value_map.insert(result.id, value);
            }
            InstructionKind::StateStore { frame, state } => {
                let frame = get_value(frame)?;
                let state = const_i64(builder, *state as i64);
                Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutineStateStore, &[frame, state]);
            }
            InstructionKind::CoroutineCreate { result, frame, poll, drop, .. } => {
                let frame = get_value(frame)?;
                let poll_id = *function_map.get(poll).ok_or_else(|| BackendCodegenError::missing_function(poll))?;
                let drop_id = *function_map.get(drop).ok_or_else(|| BackendCodegenError::missing_function(drop))?;
                let ptr_ty = module.target_config().pointer_type();
                let poll_ref = module.declare_func_in_func(poll_id, builder.func);
                let drop_ref = module.declare_func_in_func(drop_id, builder.func);
                let poll_addr = builder.ins().func_addr(ptr_ty, poll_ref);
                let drop_addr = builder.ins().func_addr(ptr_ty, drop_ref);
                let task = Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutineCreate, &[frame, poll_addr, drop_addr])
                    .ok_or_else(|| BackendCodegenError::cranelift("coroutine create import returned no value"))?;
                value_map.insert(result.id, task);
            }
            InstructionKind::CoroutinePollChild { status, result, task, output_type } => {
                let task = get_value(task)?;
                let status_value = Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutinePollChild, &[task])
                    .ok_or_else(|| BackendCodegenError::cranelift("coroutine poll import returned no value"))?;
                value_map.insert(status.id, status_value);
                if let Some(result) = result {
                    let raw = Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutinePollResult, &[task])
                        .ok_or_else(|| BackendCodegenError::cranelift("coroutine result import returned no value"))?;
                    value_map.insert(result.id, Self::frame_load_value(builder, raw, output_type)?);
                }
            }
            InstructionKind::CoroutineSubscribe { task, parent } => {
                let task = get_value(task)?;
                let parent = get_value(parent)?;
                Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutineSubscribe, &[task, parent]);
            }
            InstructionKind::CoroutineWake { task } => {
                let task = get_value(task)?;
                Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutineWake, &[task]);
            }
            InstructionKind::CoroutineSuspend { task, state } => {
                let task = get_value(task)?;
                let state = const_i64(builder, *state as i64);
                Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutineSuspend, &[task, state]);
            }
            InstructionKind::CoroutineComplete { task, value } => {
                let task = get_value(task)?;
                let value = match value {
                    Some(value) => Self::frame_store_value(builder, get_value(value)?),
                    None => const_i64(builder, 0),
                };
                Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutineComplete, &[task, value]);
            }
            InstructionKind::CoroutineError { task, error } => {
                let task = get_value(task)?;
                let error = match error {
                    Some(error) => Self::frame_store_value(builder, get_value(error)?),
                    None => const_i64(builder, 0),
                };
                Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutineError, &[task, error]);
            }
            InstructionKind::CoroutineCancelled { task } => {
                let task = get_value(task)?;
                Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutineCancelled, &[task]);
            }
            InstructionKind::CoroutinePollReturn { status } => {
                let status = get_value(status)?;
                Self::runtime_call(module, hostcall, builder, RuntimeImport::CoroutinePollReturn, &[status]);
            }
            InstructionKind::AsyncReady { result, value, output_type } => {
                let raw = match value {
                    Some(value) => get_value(value)?,
                    None => const_i64(builder, 0),
                };
                let converted = match output_type {
                    IRType::Float | IRType::ExactFloat { .. } => {
                        if builder.func.dfg.value_type(raw) == types::F64 {
                            raw
                        } else {
                            builder.ins().bitcast(types::F64, MemFlags::new(), raw)
                        }
                    }
                    IRType::Bool | IRType::Char => {
                        if builder.func.dfg.value_type(raw) == types::I64 {
                            raw
                        } else {
                            builder.ins().uextend(types::I64, raw)
                        }
                    }
                    _ => raw,
                };
                value_map.insert(result.id, converted);
            }
            _ => return Err(BackendCodegenError::invalid_ir("async instruction category mismatch")),
        }
        Ok(())
    }
}
