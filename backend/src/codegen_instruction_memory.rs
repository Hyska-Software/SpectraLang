impl CodeGenerator {
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
            InstructionKind::EscapeManualAlloc { ptr } => {
                let ptr_val = get_value(ptr)?;
                let escape_ref = module.declare_func_in_func(
                    hostcall.runtime_func(RuntimeImport::ManualEscape),
                    builder.func,
                );
                let frame_val = builder.use_var(frame_var);
                builder.ins().call(escape_ref, &[ptr_val, frame_val]);
            }

            // Memory operations
            InstructionKind::Alloca { result, ty } => {                if scalar_alloca_vars.contains_key(&result.id) {
                    // The address is proven not to escape; loads/stores use the
                    // Cranelift variable directly and no pointer value is needed.
                    return Ok(());
                }
                let size_bytes = Self::type_size_bytes(ty) as i64;
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
