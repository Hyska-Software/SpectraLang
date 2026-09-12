impl CodeGenerator {
    fn emit_string_char_at_inline(
        builder: &mut FunctionBuilder,
        ptr: Value,
        index: Value,
    ) -> Value {
        let cursor_var = builder.declare_var(types::I64);
        let result_var = builder.declare_var(types::I64);
        let zero = builder.ins().iconst(types::I64, 0);
        let zero_byte = builder.ins().iconst(types::I8, 0);
        let missing = builder.ins().iconst(types::I64, -1);
        builder.def_var(cursor_var, zero);
        builder.def_var(result_var, missing);

        let null_check_block = builder.create_block();
        let loop_block = builder.create_block();
        let target_check_block = builder.create_block();
        let found_block = builder.create_block();
        let advance_block = builder.create_block();
        let done_block = builder.create_block();

        let negative = builder.ins().icmp(IntCC::SignedLessThan, index, zero);
        builder
            .ins()
            .brif(negative, done_block, &[], null_check_block, &[]);

        builder.switch_to_block(null_check_block);
        let is_null = builder.ins().icmp(IntCC::Equal, ptr, zero);
        builder
            .ins()
            .brif(is_null, done_block, &[], loop_block, &[]);
        builder.seal_block(null_check_block);

        builder.switch_to_block(loop_block);
        let cursor = builder.use_var(cursor_var);
        let offset = cursor;
        let addr = builder.ins().iadd(ptr, offset);
        let slot = builder.ins().load(types::I8, MemFlags::new(), addr, 0);
        let is_terminator = builder.ins().icmp(IntCC::Equal, slot, zero_byte);
        builder
            .ins()
            .brif(is_terminator, done_block, &[], target_check_block, &[]);

        builder.switch_to_block(target_check_block);
        let is_target = builder.ins().icmp(IntCC::Equal, cursor, index);
        builder
            .ins()
            .brif(is_target, found_block, &[], advance_block, &[]);
        builder.seal_block(target_check_block);

        builder.switch_to_block(found_block);

        let wide = builder.ins().sextend(types::I64, slot);
        let byte = builder.ins().band_imm(wide, 0xff);
        builder.def_var(result_var, byte);
        builder.ins().jump(done_block, &[]);
        builder.seal_block(found_block);

        builder.switch_to_block(advance_block);
        let next = builder.ins().iadd_imm(cursor, 1);
        builder.def_var(cursor_var, next);
        builder.ins().jump(loop_block, &[]);
        builder.seal_block(advance_block);
        builder.seal_block(loop_block);

        builder.switch_to_block(done_block);
        builder.seal_block(done_block);
        builder.use_var(result_var)
    }

    fn emit_stack_string_char_at_inline(
        builder: &mut FunctionBuilder,
        ptr: Value,
        index: Value,
        allocation_len: i64,
    ) -> Value {
        let result_var = builder.declare_var(types::I64);
        let zero = builder.ins().iconst(types::I64, 0);
        let zero_byte = builder.ins().iconst(types::I8, 0);
        let missing = builder.ins().iconst(types::I64, -1);
        builder.def_var(result_var, missing);

        let bounds_block = builder.create_block();
        let load_block = builder.create_block();
        let value_block = builder.create_block();
        let done_block = builder.create_block();

        let negative = builder.ins().icmp(IntCC::SignedLessThan, index, zero);
        builder
            .ins()
            .brif(negative, done_block, &[], bounds_block, &[]);

        builder.switch_to_block(bounds_block);
        let max_valid_index = builder
            .ins()
            .iconst(types::I64, allocation_len.saturating_sub(1));
        let out_of_bounds =
            builder
                .ins()
                .icmp(IntCC::SignedGreaterThanOrEqual, index, max_valid_index);
        builder
            .ins()
            .brif(out_of_bounds, done_block, &[], load_block, &[]);
        builder.seal_block(bounds_block);

        builder.switch_to_block(load_block);
        let offset = index;
        let addr = builder.ins().iadd(ptr, offset);
        let slot = builder.ins().load(types::I8, MemFlags::new(), addr, 0);
        let is_terminator = builder.ins().icmp(IntCC::Equal, slot, zero_byte);
        builder
            .ins()
            .brif(is_terminator, done_block, &[], value_block, &[]);
        builder.seal_block(load_block);

        builder.switch_to_block(value_block);

        let wide = builder.ins().sextend(types::I64, slot);
        let byte = builder.ins().band_imm(wide, 0xff);
        builder.def_var(result_var, byte);
        builder.ins().jump(done_block, &[]);
        builder.seal_block(value_block);

        builder.switch_to_block(done_block);
        builder.seal_block(done_block);
        builder.use_var(result_var)
    }

    fn emit_string_len_inline(builder: &mut FunctionBuilder, ptr: Value) -> Value {
        let count_var = builder.declare_var(types::I64);
        let zero = builder.ins().iconst(types::I64, 0);
        let zero_byte = builder.ins().iconst(types::I8, 0);
        builder.def_var(count_var, zero);

        let loop_block = builder.create_block();
        let advance_block = builder.create_block();
        let done_block = builder.create_block();

        let is_null = builder.ins().icmp(IntCC::Equal, ptr, zero);
        builder
            .ins()
            .brif(is_null, done_block, &[], loop_block, &[]);

        builder.switch_to_block(loop_block);

        let offset = builder.use_var(count_var);
        let addr = builder.ins().iadd(ptr, offset);
        let slot = builder.ins().load(types::I8, MemFlags::new(), addr, 0);
        let is_terminator = builder.ins().icmp(IntCC::Equal, slot, zero_byte);
        builder
            .ins()
            .brif(is_terminator, done_block, &[], advance_block, &[]);

        builder.switch_to_block(advance_block);
        let count = builder.use_var(count_var);
        let next = builder.ins().iadd_imm(count, 1);
        builder.def_var(count_var, next);
        builder.ins().jump(loop_block, &[]);
        builder.seal_block(advance_block);
        builder.seal_block(loop_block);

        builder.switch_to_block(done_block);
        builder.seal_block(done_block);
        builder.use_var(count_var)
    }

    /// Generate terminator instruction
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn generate_terminator_static<M: Module>(
        builder: &mut FunctionBuilder,
        terminator: &Terminator,
        value_map: &DenseValueMap,
        block_map: &HashMap<usize, Block>,
        module: &mut M,
        manual_free_func: FuncId,
        manual_frame_exit_func: FuncId,
        manual_escape_func: FuncId,
        frame_var: Variable,
        manual_frame_active: bool,
        current_block_id: usize,
        phi_map: &HashMap<usize, Vec<PhiDescriptor>>,
    ) -> BackendResult<()> {
        // Helper to get value from map
        let get_value = |v: &IRValue| -> BackendResult<Value> {
            value_map
                .get(v.id)
                .ok_or_else(|| BackendCodegenError::missing_value(v.id))
        };

        match terminator {
            Terminator::Unreachable => {
                builder
                    .ins()
                    // TrapCode::UnreachableCodeReached was removed in cranelift 0.114+;
                    // use user(1) as sentinel for explicit unreachable IR
                    .trap(cranelift::codegen::ir::TrapCode::user(1).unwrap());
            }

            Terminator::Return { value } => {
                let mut return_values: Vec<Value> = Vec::new();
                if let Some(val) = value {
                    let return_val = get_value(val)?;
                    return_values.push(return_val);

                    // Escape the return value from the current frame to the parent frame so it
                    // survives frame_exit below. Only applies to pointer-sized values (i64);
                    // booleans/i8 are scalars and the escape call would fail the Cranelift
                    // verifier with a type mismatch.
                    let return_type = builder.func.dfg.value_type(return_val);
                    if manual_frame_active && return_type == cranelift::prelude::types::I64 {
                        let escape_ref =
                            module.declare_func_in_func(manual_escape_func, builder.func);
                        let frame_val_for_escape = builder.use_var(frame_var);
                        builder
                            .ins()
                            .call(escape_ref, &[return_val, frame_val_for_escape]);
                    }
                }

                // Free all locally alloca'd allocations that are still in this frame.
                // Any allocation that was escaped above has already been moved to the parent
                // frame and will therefore not be found by frame_exit, so it is safe to call
                // frame_exit unconditionally for the remaining ones.
                if manual_frame_active {
                    let _ = manual_free_func; // kept for API compat; frame_exit handles cleanup
                    let frame_exit_ref =
                        module.declare_func_in_func(manual_frame_exit_func, builder.func);
                    let frame_val = builder.use_var(frame_var);
                    builder.ins().call(frame_exit_ref, &[frame_val]);
                }

                if return_values.is_empty() {
                    builder.ins().return_(&[]);
                } else {
                    builder.ins().return_(&return_values);
                }
            }

            Terminator::Branch { target } => {
                let target_block = *block_map
                    .get(target)
                    .ok_or_else(|| BackendCodegenError::missing_block(*target))?;
                let phi_args = get_phi_args(*target, current_block_id, phi_map, value_map)?;
                builder.ins().jump(target_block, &phi_args);
            }

            Terminator::CondBranch {
                condition,
                true_block,
                false_block,
            } => {
                let cond_val = get_value(condition)?;
                let true_bb = *block_map
                    .get(true_block)
                    .ok_or_else(|| BackendCodegenError::missing_block(*true_block))?;
                let false_bb = *block_map
                    .get(false_block)
                    .ok_or_else(|| BackendCodegenError::missing_block(*false_block))?;
                let true_args = get_phi_args(*true_block, current_block_id, phi_map, value_map)?;
                let false_args = get_phi_args(*false_block, current_block_id, phi_map, value_map)?;
                builder
                    .ins()
                    .brif(cond_val, true_bb, &true_args, false_bb, &false_args);
            }

            Terminator::Switch {
                value,
                cases,
                default,
            } => {
                let switch_val = get_value(value)?;
                let default_bb = *block_map
                    .get(default)
                    .ok_or_else(|| BackendCodegenError::missing_block(*default))?;
                let default_args = get_phi_args(*default, current_block_id, phi_map, value_map)?;

                // Create switch using series of conditional branches.
                // For intermediate "next_check" blocks we do not need PHI args
                // because they are internal control-flow blocks, not user BBs.
                for (idx, (case_val, target)) in cases.iter().enumerate() {
                    let target_bb = *block_map
                        .get(target)
                        .ok_or_else(|| BackendCodegenError::missing_block(*target))?;

                    let case_const = builder.ins().iconst(types::I64, *case_val);
                    let cmp = builder.ins().icmp(IntCC::Equal, switch_val, case_const);

                    let target_args = get_phi_args(*target, current_block_id, phi_map, value_map)?;

                    if idx < cases.len() - 1 {
                        let next_check = builder.create_block();
                        builder
                            .ins()
                            .brif(cmp, target_bb, &target_args, next_check, &[]);
                        builder.seal_block(next_check);
                        builder.switch_to_block(next_check);
                    } else {
                        builder
                            .ins()
                            .brif(cmp, target_bb, &target_args, default_bb, &default_args);
                    }
                }

                if cases.is_empty() {
                    builder.ins().jump(default_bb, &default_args);
                }
            }
        }

        Ok(())
    }

    /// Convert IR type to Cranelift type
    pub(crate) fn ir_type_to_cranelift(ty: &IRType) -> BackendResult<types::Type> {
        match ty {
            IRType::Unknown => Err(BackendCodegenError::unsupported_type(ty)),
            IRType::Void => Ok(types::I8),
            IRType::Bool => Ok(types::I8),
            IRType::Int => Ok(types::I64),
            IRType::Float => Ok(types::F64),
            IRType::ExactInt { width, .. } => Ok(match width {
                spectra_midend::ir::IntWidth::I8 => types::I8,
                spectra_midend::ir::IntWidth::I16 => types::I16,
                spectra_midend::ir::IntWidth::I32 => types::I32,
                spectra_midend::ir::IntWidth::I64
                | spectra_midend::ir::IntWidth::Isize
                | spectra_midend::ir::IntWidth::Usize => types::I64,
            }),
            IRType::ExactFloat { width } => Ok(match width {
                spectra_midend::ir::FloatWidth::F32 => types::F32,
                spectra_midend::ir::FloatWidth::F64 => types::F64,
            }),
            IRType::String => Ok(types::I64),
            IRType::Char => Ok(types::I32),
            IRType::Pointer(_) => Ok(types::I64),
            IRType::Array { .. } => Ok(types::I64), // Arrays são representados como ponteiros
            IRType::Tuple { .. } => Ok(types::I64), // Tuples são representadas como ponteiros
            IRType::Struct { .. } => Ok(types::I64), // Structs são representados como ponteiros
            IRType::Enum { .. } => Ok(types::I64), // Enums são representados como ponteiros ou ints
            IRType::Generic { representation, .. } => Self::ir_type_to_cranelift(representation),
            IRType::Function { .. } => Ok(types::I64),
            IRType::Tensor { .. } => Ok(types::I64),
            IRType::Task { .. } => Ok(types::I64),
            IRType::Range => Ok(types::I64),
            IRType::DynTrait { .. } => Ok(types::I64), // fat pointer represented as i64 address
        }
    }

    /// Get size in bytes of an IR type
    pub(crate) fn type_size_bytes(ty: &IRType) -> usize {
        spectra_midend::layout::type_size_bytes(ty)
    }

    /// Get pointer to a compiled function
    pub fn get_function_ptr(&mut self, name: &str) -> BackendResult<*const u8> {
        let func_id = self
            .function_map
            .get(name)
            .ok_or_else(|| BackendCodegenError::missing_function(name))?;

        Ok(self.module.get_finalized_function(*func_id))
    }

    /// Execute an entry point with signature `fn() -> int` or `fn() -> void`.
    ///
    /// # Safety
    ///
    /// `name` must identify a compiled function with the supported ABI, and
    /// `ir_module` must describe the same module used to create this generator.
    pub unsafe fn execute_entry_point(
        &mut self,
        name: &str,
        ir_module: &IRModule,
    ) -> BackendResult<Option<i64>> {
        let ptr = self.get_function_ptr(name)?;

        let return_type = ir_module
            .get_function(name)
            .map(|f| f.return_type.clone())
            .unwrap_or(IRType::Void);

        match return_type {
            IRType::Void => {
                let func: extern "C" fn() = std::mem::transmute(ptr);
                func();
                Ok(None)
            }
            IRType::Int => {
                let func: extern "C" fn() -> i64 = std::mem::transmute(ptr);
                Ok(Some(func()))
            }
            IRType::Bool => {
                let func: extern "C" fn() -> i8 = std::mem::transmute(ptr);
                Ok(Some(func() as i64))
            }
            other => Err(BackendCodegenError::unsupported_execution_return_type(
                other,
            )),
        }
    }
}
