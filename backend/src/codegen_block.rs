impl CodeGenerator {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn generate_block<M: Module>(
        module: &mut M,
        function_map: &HashMap<String, FuncId>,
        function_params: &HashMap<String, Vec<IRType>>,
        hostcall: &mut HostCallLoweringContext<'_>,
        builder: &mut FunctionBuilder,
        ir_block: &IRBasicBlock,
        value_map: &mut DenseValueMap,
        block_map: &HashMap<usize, Block>,
        allocation_vars: &mut Vec<Variable>,
        stack_array_lengths: &mut HashMap<usize, i64>,
        string_literal_lengths: &mut HashMap<usize, i64>,
        stack_allocas: &HashSet<usize>,
        scalar_alloca_vars: &HashMap<usize, Variable>,
        global_data: &HashMap<String, DataId>,
        frame_var: Variable,
        manual_frame_active: bool,
        current_block_id: usize,
        phi_map: &HashMap<usize, Vec<PhiDescriptor>>,
        emitted_tail_call: &mut bool,
    ) -> BackendResult<()> {
        // Get Cranelift block
        let block = *block_map
            .get(&ir_block.id)
            .ok_or_else(|| BackendCodegenError::missing_block(ir_block.id))?;

        // Switch to block
        if builder.current_block() != Some(block) {
            builder.switch_to_block(block);
        }

        // Generate instructions. A batch is planned only across a contiguous
        // run of generic HostCall instructions in this block. All uncertain
        // cases fall back to the existing single-instruction lowering.
        let track_allocations = ir_block.id == 0;
        let mut instruction_index = 0;
        while instruction_index < ir_block.instructions.len() {
            let batch_end = Self::hostcall_batch_end(
                &ir_block.instructions,
                instruction_index,
                value_map,
                builder,
                hostcall.host_call_sites,
            );
            if batch_end > instruction_index {
                Self::generate_hostcall_batch(
                    module,
                    hostcall,
                    builder,
                    &ir_block.instructions[instruction_index..batch_end],
                    value_map,
                )?;
                instruction_index = batch_end;
                continue;
            }

            let instr = &ir_block.instructions[instruction_index];
            if let InstructionKind::HostCall { host, .. } = &instr.kind {
                if classify_host_call(host).batch_eligible() {
                    hostcall.batch_stats.fallback_hostcalls += 1;
                }
            }
            Self::generate_instruction(
                module,
                function_map,
                function_params,
                hostcall,
                builder,
                instr,
                value_map,
                allocation_vars,
                stack_array_lengths,
                string_literal_lengths,
                stack_allocas,
                scalar_alloca_vars,
                global_data,
                frame_var,
                track_allocations,
                ir_block.id,
                block_map,
                phi_map,
                emitted_tail_call,
            )?;
            instruction_index += 1;
        }

        // Generate terminator
        // A marked tail call already terminated this block with Cranelift's
        // native `return_call`; emitting the IR `Return` afterwards would
        // produce two terminators in one block.
        if !*emitted_tail_call {
            if let Some(terminator) = &ir_block.terminator {
                let manual_free_func = hostcall.bindings.get(RuntimeImport::ManualFree);
                let manual_frame_exit_func = hostcall.bindings.get(RuntimeImport::ManualFrameExit);
                let manual_escape_func = hostcall.bindings.get(RuntimeImport::ManualEscape);
                Self::generate_terminator_static(
                    builder,
                    terminator,
                    value_map,
                    block_map,
                    module,
                    manual_free_func,
                    manual_frame_exit_func,
                    manual_escape_func,
                    frame_var,
                    manual_frame_active,
                    current_block_id,
                    phi_map,
                )?;
            }
        }

        Ok(())
    }
}
