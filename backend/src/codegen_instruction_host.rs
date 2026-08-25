impl CodeGenerator {
    fn generate_host_instruction<M: Module>(
        module: &mut M,
        hostcall: &mut HostCallLoweringContext<'_>,
        builder: &mut FunctionBuilder,
        kind: &InstructionKind,
        value_map: &mut DenseValueMap,
        stack_array_lengths: &mut HashMap<usize, i64>,
        string_literal_lengths: &mut HashMap<usize, i64>,
    ) -> BackendResult<()> {
        let get_value = |v: &IRValue| -> BackendResult<Value> {
            value_map
                .get(v.id)
                .ok_or_else(|| BackendCodegenError::missing_value(v.id))
        };

        match kind {
            InstructionKind::HostCall {
                result,
                host,
                args,
                result_type,
            } => {
                let fast_hostcall = resolve_host_call(host, args.len());

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::ConcurrentReset)
                ) && args.is_empty()
                {
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::ConcurrentReset),
                        builder.func,
                    );
                    builder.ins().call(func_ref, &[]);
                    return Ok(());
                }

                if matches!(fast_hostcall, HostCallClass::Fast(FastHostCall::StringLen))
                    && args.len() == 1
                {
                    let ptr = get_value(&args[0])?;
                    if let Some(result_value) = result {
                        let value = if let Some(alloc_len) =
                            string_literal_lengths.get(&args[0].id).copied()
                        {
                            // String literal: known length, return constant
                            // (alloc_len includes the trailing null terminator, so the
                            // actual byte count is alloc_len - 1).
                            builder.ins().iconst(types::I64, alloc_len - 1)
                        } else {
                            Self::emit_string_len_inline(builder, ptr)
                        };
                        value_map.insert(result_value.id, value);
                    }
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::StringCharAt)
                ) && args.len() == 2
                {
                    let ptr = get_value(&args[0])?;
                    let index = get_value(&args[1])?;
                    if let Some(result_value) = result {
                        let value = if let Some(length) = stack_array_lengths.get(&args[0].id) {
                            Self::emit_stack_string_char_at_inline(builder, ptr, index, *length)
                        } else if let Some(length) =
                            string_literal_lengths.get(&args[0].id).copied()
                        {
                            // String literal with known length: emit direct O(1) load
                            // (re-use the stack inline emitter — it only needs the
                            // allocation length to do bounds checks, not stack residency).
                            Self::emit_stack_string_char_at_inline(builder, ptr, index, length)
                        } else {
                            Self::emit_string_char_at_inline(builder, ptr, index)
                        };
                        value_map.insert(result_value.id, value);
                    }
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::ConcurrentSpawnJoin)
                ) && args.len() == 1
                {
                    let value = get_value(&args[0])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::ConcurrentSpawnJoin),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[value]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::ConcurrentSpawn)
                ) && args.len() == 1
                {
                    let value = get_value(&args[0])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::ConcurrentSpawn),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[value]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::ConcurrentSpawnBatch)
                ) && args.len() == 2
                {
                    let first_value = get_value(&args[0])?;
                    let count = get_value(&args[1])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::ConcurrentSpawnBatch),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[first_value, count]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::ConcurrentSpawnFn)
                ) && args.len() == 2
                {
                    let fn_ptr = get_value(&args[0])?;
                    let arg = get_value(&args[1])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::ConcurrentSpawnFn),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[fn_ptr, arg]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::ConcurrentJoinBatchSum)
                ) && args.len() == 1
                {
                    let batch_id = get_value(&args[0])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::ConcurrentJoinBatchSum),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[batch_id]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::ConcurrentJoin)
                ) && args.len() == 1
                {
                    let task_id = get_value(&args[0])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::ConcurrentJoin),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[task_id]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(fast_hostcall, HostCallClass::Fast(FastHostCall::BuilderNew))
                    && args.len() == 1
                {
                    let capacity = get_value(&args[0])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::BuilderNew),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[capacity]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::BuilderPush)
                ) && args.len() == 2
                {
                    let handle = get_value(&args[0])?;
                    let str_ptr = get_value(&args[1])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::BuilderPush),
                        builder.func,
                    );
                    builder.ins().call(func_ref, &[handle, str_ptr]);
                    return Ok(());
                }

                if matches!(fast_hostcall, HostCallClass::Fast(FastHostCall::BuilderLen))
                    && args.len() == 1
                {
                    let handle = get_value(&args[0])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::BuilderLen),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[handle]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::BuilderFinish)
                ) && args.len() == 1
                {
                    let handle = get_value(&args[0])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::BuilderFinish),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[handle]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::BuilderFree)
                ) && args.len() == 1
                {
                    let handle = get_value(&args[0])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::BuilderFree),
                        builder.func,
                    );
                    builder.ins().call(func_ref, &[handle]);
                    return Ok(());
                }

                if matches!(fast_hostcall, HostCallClass::Fast(FastHostCall::MapSet))
                    && args.len() == 3
                {
                    let handle = get_value(&args[0])?;
                    let key = get_value(&args[1])?;
                    let value = get_value(&args[2])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::MapSet),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[handle, key, value]);
                    let _results = builder.inst_results(call);
                    return Ok(());
                }

                if matches!(fast_hostcall, HostCallClass::Fast(FastHostCall::MapGet))
                    && args.len() == 2
                {
                    let handle = get_value(&args[0])?;
                    let key = get_value(&args[1])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::MapGet),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[handle, key]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::MapContains)
                ) && args.len() == 2
                {
                    let handle = get_value(&args[0])?;
                    let key = get_value(&args[1])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::MapContains),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[handle, key]);
                    let ret = builder.inst_results(call).first().copied();
                    if let Some(result_value) = result {
                        if let Some(ret) = ret {
                            let value = match result_type.as_ref() {
                                Some(IRType::Bool) => builder.ins().ireduce(types::I8, ret),
                                _ => ret,
                            };
                            value_map.insert(result_value.id, value);
                        }
                    }
                    return Ok(());
                }

                if matches!(fast_hostcall, HostCallClass::Fast(FastHostCall::MapNew))
                    && args.is_empty()
                {
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::MapNew),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(fast_hostcall, HostCallClass::Fast(FastHostCall::MapRemove))
                    && args.len() == 2
                {
                    let handle = get_value(&args[0])?;
                    let key = get_value(&args[1])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::MapRemove),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[handle, key]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(fast_hostcall, HostCallClass::Fast(FastHostCall::MapLen))
                    && args.len() == 1
                {
                    let handle = get_value(&args[0])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::MapLen),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[handle]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(fast_hostcall, HostCallClass::Fast(FastHostCall::MapClear))
                    && args.len() == 1
                {
                    let handle = get_value(&args[0])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::MapClear),
                        builder.func,
                    );
                    builder.ins().call(func_ref, &[handle]);
                    return Ok(());
                }

                if matches!(fast_hostcall, HostCallClass::Fast(FastHostCall::MapFree))
                    && args.len() == 1
                {
                    let handle = get_value(&args[0])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::MapFree),
                        builder.func,
                    );
                    builder.ins().call(func_ref, &[handle]);
                    return Ok(());
                }

                if matches!(fast_hostcall, HostCallClass::Fast(FastHostCall::ChannelNew))
                    && args.is_empty()
                {
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::ChannelNew),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::ChannelSend)
                ) && args.len() == 2
                {
                    let channel = get_value(&args[0])?;
                    let value = get_value(&args[1])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::ChannelSend),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[channel, value]);
                    let ret = builder.inst_results(call).first().copied();
                    if let Some(result_value) = result {
                        if let Some(ret) = ret {
                            let value = match result_type.as_ref() {
                                Some(IRType::Bool) => builder.ins().ireduce(types::I8, ret),
                                _ => ret,
                            };
                            value_map.insert(result_value.id, value);
                        }
                    }
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::ChannelRecv)
                ) && args.len() == 1
                {
                    let channel = get_value(&args[0])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::ChannelRecv),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[channel]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::ChannelClose)
                ) && args.len() == 1
                {
                    let channel = get_value(&args[0])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::ChannelClose),
                        builder.func,
                    );
                    builder.ins().call(func_ref, &[channel]);
                    return Ok(());
                }

                if matches!(fast_hostcall, HostCallClass::Fast(FastHostCall::ChannelLen))
                    && args.len() == 1
                {
                    let channel = get_value(&args[0])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::ChannelLen),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[channel]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(fast_hostcall, HostCallClass::Fast(FastHostCall::MlLinear))
                    && args.len() == 3
                {
                    let input = get_value(&args[0])?;
                    let weight = get_value(&args[1])?;
                    let bias = get_value(&args[2])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::MlLinear),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[input, weight, bias]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(fast_hostcall, HostCallClass::Fast(FastHostCall::MlMseLoss))
                    && args.len() == 2
                {
                    let prediction = get_value(&args[0])?;
                    let target = get_value(&args[1])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::MlMseLoss),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[prediction, target]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::TensorBackward)
                ) && args.len() == 1
                {
                    let loss = get_value(&args[0])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::TensorBackward),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[loss]);
                    let _results = builder.inst_results(call);
                    return Ok(());
                }

                if matches!(fast_hostcall, HostCallClass::Fast(FastHostCall::MlSgdStep))
                    && args.len() == 2
                {
                    let param = get_value(&args[0])?;
                    let lr = get_value(&args[1])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::MlSgdStep),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[param, lr]);
                    let _results = builder.inst_results(call);
                    return Ok(());
                }

                if matches!(
                    fast_hostcall,
                    HostCallClass::Fast(FastHostCall::TensorFullF)
                ) && args.len() == 2
                {
                    let n = get_value(&args[0])?;
                    let value = get_value(&args[1])?;
                    let func_ref = module.declare_func_in_func(
                        hostcall.fast_func(FastHostCall::TensorFullF),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[n, value]);
                    let results = builder.inst_results(call);
                    if let Some(result_value) = result {
                        if let Some(ret) = results.first() {
                            value_map.insert(result_value.id, *ret);
                        }
                    }
                    return Ok(());
                }

                let record = hostcall.host_call_site(host).ok_or_else(|| {
                    BackendCodegenError::cranelift(format!(
                        "host call name '{}' was not pre-interned",
                        host
                    ))
                })?;
                let cache_ptr = Self::host_cache_pointer(module, builder, record);
                let name_ptr = Self::host_name_pointer(module, builder, record.name);
                let name_len = builder.ins().iconst(types::I64, record.name.len as i64);

                let (args_ptr, args_count, args_allocation) = if args.is_empty() {
                    (
                        builder.ins().iconst(types::I64, 0),
                        builder.ins().iconst(types::I64, 0),
                        None,
                    )
                } else {
                    let size = builder.ins().iconst(types::I64, (args.len() as i64) * 8);
                    let alloc_ref = module.declare_func_in_func(
                        hostcall.runtime_func(RuntimeImport::ManualAlloc),
                        builder.func,
                    );
                    let call = builder.ins().call(alloc_ref, &[size]);
                    let ptr = builder.inst_results(call)[0];

                    for (idx, arg) in args.iter().enumerate() {
                        let mut value = get_value(arg)?;
                        let ty = builder.func.dfg.value_type(value);
                        value = match ty {
                            types::I64 => value,
                            types::I8 | types::I16 | types::I32 => {
                                builder.ins().sextend(types::I64, value)
                            }
                            types::F64 => {
                                // Reinterpret float bits as i64 so the runtime
                                // can receive and convert them.
                                builder.ins().bitcast(types::I64, MemFlags::new(), value)
                            }
                            types::F32 => {
                                let promoted = builder.ins().fpromote(types::F64, value);
                                builder.ins().bitcast(types::I64, MemFlags::new(), promoted)
                            }
                            other => {
                                return Err(BackendCodegenError::unsupported_host_argument_type(
                                    other,
                                ))
                            }
                        };
                        let offset = (idx as i32) * 8;
                        builder.ins().store(MemFlags::new(), value, ptr, offset);
                    }

                    let count = builder.ins().iconst(types::I64, args.len() as i64);
                    (ptr, count, Some(ptr))
                };

                let (results_ptr, result_len_val, result_allocation) = if result.is_some() {
                    let size = builder.ins().iconst(types::I64, 8);
                    let alloc_ref = module.declare_func_in_func(
                        hostcall.runtime_func(RuntimeImport::ManualAlloc),
                        builder.func,
                    );
                    let call = builder.ins().call(alloc_ref, &[size]);
                    let ptr = builder.inst_results(call)[0];
                    let len = builder.ins().iconst(types::I64, 1);
                    (ptr, len, Some(ptr))
                } else {
                    (
                        builder.ins().iconst(types::I64, 0),
                        builder.ins().iconst(types::I64, 0),
                        None,
                    )
                };

                let func_ref = module.declare_func_in_func(
                    hostcall.runtime_func(RuntimeImport::HostInvokeCached),
                    builder.func,
                );
                let call = builder.ins().call(
                    func_ref,
                    &[
                        cache_ptr,
                        name_ptr,
                        name_len,
                        args_ptr,
                        args_count,
                        results_ptr,
                        result_len_val,
                    ],
                );
                let status = builder.inst_results(call)[0];

                let zero = builder.ins().iconst(types::I32, 0);
                let is_ok = builder.ins().icmp(IntCC::Equal, status, zero);
                let success_block = builder.create_block();
                let failure_block = builder.create_block();
                builder
                    .ins()
                    .brif(is_ok, success_block, &[], failure_block, &[]);

                builder.switch_to_block(failure_block);
                if let Some(ptr) = result_allocation {
                    let free_ref = module.declare_func_in_func(
                        hostcall.runtime_func(RuntimeImport::ManualFree),
                        builder.func,
                    );
                    builder.ins().call(free_ref, &[ptr]);
                }
                if let Some(ptr) = args_allocation {
                    let free_ref = module.declare_func_in_func(
                        hostcall.runtime_func(RuntimeImport::ManualFree),
                        builder.func,
                    );
                    builder.ins().call(free_ref, &[ptr]);
                }
                Self::emit_runtime_panic(
                    module,
                    hostcall,
                    builder,
                    &format!("host call '{host}' failed"),
                )?;
                builder.seal_block(failure_block);

                builder.switch_to_block(success_block);
                builder.seal_block(success_block);

                if let (Some(result_value), Some(ptr)) = (result, result_allocation) {
                    let raw_value = builder.ins().load(types::I64, MemFlags::new(), ptr, 0);
                    let value = match result_type {
                        Some(IRType::Float) => {
                            builder
                                .ins()
                                .bitcast(types::F64, MemFlags::new(), raw_value)
                        }
                        Some(IRType::Bool) => builder.ins().ireduce(types::I8, raw_value),
                        Some(IRType::Char) => builder.ins().ireduce(types::I32, raw_value),
                        Some(IRType::ExactInt { .. }) => {
                            // Host calls always materialize scalar results in the
                            // canonical i64 slot.  Do not emit `ireduce(i64,
                            // i64)`: Cranelift rejects a same-width reduction
                            // during verification (this surfaced for checked_i64
                            // even though the Spectra cast itself was valid).
                            let target = Self::ir_type_to_cranelift(result_type.as_ref().unwrap())?;
                            if target == types::I64 {
                                raw_value
                            } else {
                                builder.ins().ireduce(target, raw_value)
                            }
                        }
                        Some(IRType::ExactFloat { width }) => match width {
                            spectra_midend::ir::FloatWidth::F32 => {
                                let f64_value =
                                    builder
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
                    };
                    value_map.insert(result_value.id, value);

                    let free_ref = module.declare_func_in_func(
                        hostcall.runtime_func(RuntimeImport::ManualFree),
                        builder.func,
                    );
                    builder.ins().call(free_ref, &[ptr]);
                }

                if let Some(ptr) = args_allocation {
                    let free_ref = module.declare_func_in_func(
                        hostcall.runtime_func(RuntimeImport::ManualFree),
                        builder.func,
                    );
                    builder.ins().call(free_ref, &[ptr]);
                }
            }
            _ => unreachable!("host instruction category mismatch"),
        }
        Ok(())
    }
}
