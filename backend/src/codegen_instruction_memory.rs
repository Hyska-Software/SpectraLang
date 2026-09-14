impl CodeGenerator {
    /// The `i64` word a scalar occupies in the runtime's slot representation.
    ///
    /// The runtime ABI carries scalars as `i64` words: an `f64` by its bit
    /// pattern, a narrower integer widened, a pointer (already `i64` in this
    /// backend) unchanged. Any instruction that hands a *value* to a
    /// `I64_I64` runtime import needs this, not the raw value: Cranelift's
    /// verifier rejects a call whose argument type does not match the declared
    /// signature, and the failure surfaces only for the non-`i64` scalar
    /// shapes (`Result<float, E>`, `Option<float>`).
    pub(crate) fn scalar_word(builder: &mut FunctionBuilder, value: Value) -> Value {
        match builder.func.dfg.value_type(value) {
            types::I64 => value,
            types::F64 => builder.ins().bitcast(types::I64, MemFlags::new(), value),
            types::F32 => {
                let promoted = builder.ins().fpromote(types::F64, value);
                builder.ins().bitcast(types::I64, MemFlags::new(), promoted)
            }
            types::I8 | types::I16 | types::I32 => builder.ins().uextend(types::I64, value),
            _ => value,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_memory_instruction<M: Module>(
        module: &mut M,
        hostcall: &mut HostCallLoweringContext<'_>,
        builder: &mut FunctionBuilder,
        kind: &InstructionKind,
        value_map: &mut DenseValueMap,
        allocation_vars: &mut Vec<Variable>,
        stack_array_lengths: &mut HashMap<usize, i64>,
        string_literal_lengths: &mut HashMap<usize, i64>,
        stack_allocas: &HashSet<usize>,
        scalar_alloca_vars: &HashMap<usize, Variable>,
        global_data: &HashMap<String, DataId>,
        frame_var: Variable,
        track_allocations: bool,
        frame_locals: bool,
    ) -> BackendResult<()> {
        let get_value = |v: &IRValue| -> BackendResult<Value> {
            value_map
                .get(v.id)
                .ok_or_else(|| BackendCodegenError::missing_value(v.id))
        };

        match kind {
            InstructionKind::ManualAlloc { result, size } => {
                let size_value = builder.ins().iconst(types::I64, *size);
                let func_ref = module.declare_func_in_func(
                    hostcall.runtime_func(RuntimeImport::ManualAlloc),
                    builder.func,
                );
                let call = builder.ins().call(func_ref, &[size_value]);
                let results = builder.inst_results(call);
                if let Some(&ptr) = results.first() {
                    value_map.insert(result.id, ptr);
                }
            }

            // Escape a manual allocation to the base frame so it survives
            // frame_exit (used by dyn Trait vtables, R-210).
            //
            // `spectra_rt_manual_escape` is `(ptr: i64, frame: i64)` and
            // treats a value that is not a tracked allocation as a no-op, so a
            // scalar payload escapes by its word. An escaped f64 reaches the
            // runtime as its bit pattern; passing the raw f64 instead is a
            // Cranelift verifier error ("arg 0 has type f64, expected i64"),
            // which is what an enum payload of a float type hit — the shape
            // `fn f() -> Result<float, E> { Result::Ok(1.5) }` produced before
            // this coercion existed.
            InstructionKind::EscapeManualAlloc { ptr } => {
                let ptr_val = get_value(ptr)?;
                let word = Self::scalar_word(builder, ptr_val);
                let escape_ref = module.declare_func_in_func(
                    hostcall.runtime_func(RuntimeImport::ManualEscape),
                    builder.func,
                );
                let frame_val = builder.use_var(frame_var);
                builder.ins().call(escape_ref, &[word, frame_val]);
            }

            // Memory operations
            InstructionKind::Alloca { result, ty } => {                if scalar_alloca_vars.contains_key(&result.id) {
                    // The address is proven not to escape; loads/stores use the
                    // Cranelift variable directly and no pointer value is needed.
                    return Ok(());
                }
                let size_bytes = Self::type_size_bytes(ty) as i64;
                // Inside a coroutine poll the frame outlives the poll
                // activation: an address kept in a frame slot must therefore
                // reference frame-owned storage, not a stack slot or a
                // per-poll manual allocation.
                if frame_locals {
                    let frame = value_map
                        .get(0)
                        .ok_or_else(|| BackendCodegenError::missing_value(0))?;
                    let slot = builder.ins().iconst(types::I64, result.id as i64);
                    let size = builder.ins().iconst(types::I64, size_bytes);
                    let ptr = Self::runtime_call(
                        module,
                        hostcall,
                        builder,
                        RuntimeImport::CoroutineLocalPtr,
                        &[frame, slot, size],
                    )
                    .ok_or_else(|| {
                        BackendCodegenError::cranelift(
                            "coroutine local allocation returned no value",
                        )
                    })?;
                    value_map.insert(result.id, ptr);
                    return Ok(());
                }
                if stack_allocas.contains(&result.id) {
                    let slot =
                        builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            size_bytes as u32,
                            0,
                        ));
                    let ptr = builder.ins().stack_addr(types::I64, slot, 0);
                    value_map.insert(result.id, ptr);
                    if let IRType::Array { element_type, size } = ty {
                        if matches!(
                            **element_type,
                            IRType::Int | IRType::Char | IRType::ExactInt { .. }
                        ) {
                            stack_array_lengths.insert(result.id, *size as i64);
                            string_literal_lengths.insert(result.id, *size as i64);
                        }
                    }
                    return Ok(());
                }

                let size_value = builder.ins().iconst(types::I64, size_bytes);
                let func_ref = module.declare_func_in_func(
                    hostcall.runtime_func(RuntimeImport::ManualAlloc),
                    builder.func,
                );
                let call = builder.ins().call(func_ref, &[size_value]);
                let results = builder.inst_results(call);
                if let Some(&ptr) = results.first() {
                    value_map.insert(result.id, ptr);

                    if track_allocations {
                        // cranelift 0.130+: declare_var returns a Variable
                        let var = builder.declare_var(types::I64);
                        builder.def_var(var, ptr);
                        allocation_vars.push(var);
                    }

                    if let IRType::Array { element_type, size } = ty {
                        if matches!(
                            **element_type,
                            IRType::Int | IRType::Char | IRType::ExactInt { .. }
                        ) {
                            string_literal_lengths.insert(result.id, *size as i64);
                        }
                    }
                } else {
                    return Err(BackendCodegenError::invalid_ir(
                        "runtime allocation did not return a pointer",
                    ));
                }
            }

            InstructionKind::GlobalAddr { result, name, ty } => {
                let data_id = global_data.get(name).ok_or_else(|| {
                    BackendCodegenError::invalid_ir(format!(
                        "global address references unknown global '{}'",
                        name
                    ))
                })?;
                let _ = Self::ir_type_to_cranelift(ty)?;
                let global = module.declare_data_in_func(*data_id, builder.func);
                let pointer = builder.ins().global_value(types::I64, global);
                value_map.insert(result.id, pointer);
            }

            InstructionKind::Load { result, ptr, ty } => {
                if let Some(variable) = scalar_alloca_vars.get(&ptr.id) {
                    let result_val = builder.use_var(*variable);
                    value_map.insert(result.id, result_val);
                    return Ok(());
                }
                let ptr_val = get_value(ptr)?;
                let cranelift_ty = Self::ir_type_to_cranelift(ty)?;
                let result_val = builder
                    .ins()
                    .load(cranelift_ty, MemFlags::new(), ptr_val, 0);
                value_map.insert(result.id, result_val);
            }

            InstructionKind::Store { ptr, value } => {
                if let Some(variable) = scalar_alloca_vars.get(&ptr.id) {
                    let value_val = get_value(value)?;
                    builder.def_var(*variable, value_val);
                    return Ok(());
                }
                let ptr_val = get_value(ptr)?;
                let value_val = get_value(value)?;
                builder.ins().store(MemFlags::new(), value_val, ptr_val, 0);
            }

            InstructionKind::GetElementPtr {
                result,
                ptr,
                index,
                element_type,
            } => {
                let ptr_val = get_value(ptr)?;

                let index_val = get_value(index)?;

                // Calcular o tamanho do elemento em bytes
                let elem_size = Self::type_size_bytes(element_type) as i64;

                // offset = index * elem_size
                let elem_size_val = builder.ins().iconst(types::I64, elem_size);
                let offset = builder.ins().imul(index_val, elem_size_val);

                // ptr + offset
                let result_val = builder.ins().iadd(ptr_val, offset);
                value_map.insert(result.id, result_val);
            }

            // Field pointer with a constant byte offset (padded aggregate layout)
            InstructionKind::FieldPtr { result, ptr, offset } => {
                let ptr_val = get_value(ptr)?;
                let offset_val = builder.ins().iconst(types::I64, *offset );
                let result_val = builder.ins().iadd(ptr_val, offset_val);
                value_map.insert(result.id, result_val);
            }

            _ => unreachable!("memory instruction category mismatch"),
        }
        Ok(())
    }
}
