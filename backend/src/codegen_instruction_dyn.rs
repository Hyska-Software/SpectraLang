impl CodeGenerator {
    fn generate_dyn_instruction<M: Module>(
        module: &mut M,
        hostcall: &mut HostCallLoweringContext<'_>,
        builder: &mut FunctionBuilder,
        kind: &InstructionKind,
        value_map: &mut DenseValueMap,
        frame_var: Variable,
    ) -> BackendResult<()> {
        let get_value = |v: &IRValue| -> BackendResult<Value> {
            value_map
                .get(v.id)
                .ok_or_else(|| BackendCodegenError::missing_value(v.id))
        };

        match kind {
            // dyn Trait fat pointer operations
            InstructionKind::MakeDynFatPtr {
                result,
                data_ptr,
                vtable_ptr,
            } => {
                // The IR value is an address of a stable 16-byte pair, not
                // an inline two-register aggregate.  A stack slot would
                // dangle as soon as a function/block returning the dyn value
                // exits, so use the same frame-tracked manual heap as the
                // vtable and escape the pair to the base frame.
                let size_value = builder.ins().iconst(types::I64, 16);
                let alloc_ref = module.declare_func_in_func(
                    hostcall.runtime_func(RuntimeImport::ManualAlloc),
                    builder.func,
                );
                let alloc_call = builder.ins().call(alloc_ref, &[size_value]);
                let pair_ptr = *builder
                    .inst_results(alloc_call)
                    .first()
                    .ok_or_else(|| {
                        BackendCodegenError::invalid_ir(
                            "dyn fat pointer allocation did not return a pointer",
                        )
                    })?;
                let data_val = get_value(data_ptr)?;
                let vtable_val = get_value(vtable_ptr)?;
                builder.ins().store(
                    cranelift_codegen::ir::MemFlags::new(),
                    data_val,
                    pair_ptr,
                    0,
                );
                builder.ins().store(
                    cranelift_codegen::ir::MemFlags::new(),
                    vtable_val,
                    pair_ptr,
                    8,
                );
                let escape_ref = module.declare_func_in_func(
                    hostcall.runtime_func(RuntimeImport::ManualEscape),
                    builder.func,
                );
                let frame_value = builder.use_var(frame_var);
                builder.ins().call(escape_ref, &[pair_ptr, frame_value]);
                value_map.insert(result.id, pair_ptr);
            }

            InstructionKind::LoadDynDataPtr { result, fat_ptr } => {
                let ptr_val = get_value(fat_ptr)?;
                let data = builder.ins().load(
                    types::I64,
                    cranelift_codegen::ir::MemFlags::new(),
                    ptr_val,
                    0,
                );
                value_map.insert(result.id, data);
            }

            InstructionKind::LoadDynVtablePtr { result, fat_ptr } => {
                let ptr_val = get_value(fat_ptr)?;
                let vtable = builder.ins().load(
                    types::I64,
                    cranelift_codegen::ir::MemFlags::new(),
                    ptr_val,
                    8,
                );
                value_map.insert(result.id, vtable);
            }

            InstructionKind::LoadVtableSlot {
                result,
                vtable_ptr,
                slot_index,
            } => {
                let vptr_val = get_value(vtable_ptr)?;
                let offset = (*slot_index as i32) * 8;
                let fn_ptr = builder.ins().load(
                    types::I64,
                    cranelift_codegen::ir::MemFlags::new(),
                    vptr_val,
                    offset,
                );
                value_map.insert(result.id, fn_ptr);
            }
            _ => unreachable!("dynamic dispatch instruction category mismatch"),
        }
        Ok(())
    }
}
