impl CodeGenerator {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn generate_instruction<M: Module>(
        module: &mut M,
        function_map: &HashMap<String, FuncId>,
        function_params: &HashMap<String, Vec<IRType>>,
        hostcall: &mut HostCallLoweringContext<'_>,
        builder: &mut FunctionBuilder,
        instr: &Instruction,
        value_map: &mut DenseValueMap,
        allocation_vars: &mut Vec<Variable>,
        stack_array_lengths: &mut HashMap<usize, i64>,
        string_literal_lengths: &mut HashMap<usize, i64>,
        stack_allocas: &HashSet<usize>,
        scalar_alloca_vars: &HashMap<usize, Variable>,
        global_data: &HashMap<String, DataId>,
        frame_var: Variable,
        track_allocations: bool,
        current_block_id: usize,
        block_map: &HashMap<usize, Block>,
        phi_map: &HashMap<usize, Vec<PhiDescriptor>>,
        emitted_tail_call: &mut bool,
    ) -> BackendResult<()> {
        let kind = &instr.kind;
        match kind {
            InstructionKind::Add { .. }
            | InstructionKind::Sub { .. }
            | InstructionKind::Mul { .. }
            | InstructionKind::Div { .. }
            | InstructionKind::Rem { .. }
            | InstructionKind::Eq { .. }
            | InstructionKind::Ne { .. }
            | InstructionKind::Lt { .. }
            | InstructionKind::Le { .. }
            | InstructionKind::Gt { .. }
            | InstructionKind::Ge { .. }
            | InstructionKind::And { .. }
            | InstructionKind::Or { .. }
            | InstructionKind::Not { .. } => {
                Self::generate_arithmetic_instruction(module, hostcall, builder, kind, value_map)
            }
            InstructionKind::ManualAlloc { .. }
            | InstructionKind::EscapeManualAlloc { .. }
            | InstructionKind::Alloca { .. }
            | InstructionKind::GlobalAddr { .. }
            | InstructionKind::Load { .. }
            | InstructionKind::Store { .. }
            | InstructionKind::GetElementPtr { .. }
            | InstructionKind::FieldPtr { .. } => Self::generate_memory_instruction(
                module,
                hostcall,
                builder,
                kind,
                value_map,
                allocation_vars,
                stack_array_lengths,
                string_literal_lengths,
                stack_allocas,
                scalar_alloca_vars,
                global_data,
                frame_var,
                track_allocations,
            ),
            InstructionKind::Call { .. } | InstructionKind::AutodiffStep { .. } => {
                Self::generate_call_instruction(
                    module,
                    function_map,
                    function_params,
                    hostcall,
                    builder,
                    kind,
                    value_map,
                    emitted_tail_call,
                )
            }
            InstructionKind::HostCall { .. } => Self::generate_host_instruction(
                module,
                hostcall,
                builder,
                kind,
                value_map,
                stack_array_lengths,
                string_literal_lengths,
            ),
            InstructionKind::AsyncSuspend { .. }
            | InstructionKind::AsyncResume { .. }
            | InstructionKind::AsyncReady { .. } => {
                Self::generate_async_instruction(builder, kind, value_map)
            }
            InstructionKind::FuncAddr { .. } | InstructionKind::CallIndirect { .. } => {
                Self::generate_indirect_instruction(
                    module,
                    function_map,
                    hostcall,
                    builder,
                    kind,
                    value_map,
                )
            }
            InstructionKind::Copy { .. } | InstructionKind::Cast { .. } => {
                Self::generate_cast_instruction(builder, kind, value_map)
            }
            InstructionKind::MakeDynFatPtr { .. }
            | InstructionKind::LoadDynDataPtr { .. }
            | InstructionKind::LoadDynVtablePtr { .. }
            | InstructionKind::LoadVtableSlot { .. } => Self::generate_dyn_instruction(
                module,
                hostcall,
                builder,
                kind,
                value_map,
                frame_var,
            ),
            InstructionKind::Phi { .. }
            | InstructionKind::ConstInt { .. }
            | InstructionKind::ConstIntTyped { .. }
            | InstructionKind::ConstFloat { .. }
            | InstructionKind::ConstFloatTyped { .. }
            | InstructionKind::ConstBool { .. }
            | InstructionKind::ConstString { .. } => Self::generate_value_instruction(
                module,
                hostcall,
                builder,
                kind,
                value_map,
                string_literal_lengths,
                block_map,
                phi_map,
                current_block_id,
            ),
        }
    }
}
