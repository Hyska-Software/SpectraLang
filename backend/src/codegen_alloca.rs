impl CodeGenerator {
    pub(crate) fn collect_stack_allocas(ir_func: &IRFunction) -> HashSet<usize> {
        let mut stack_allocas = HashSet::new();
        let mut alloca_types = HashMap::new();
        let mut derived_from_alloca = HashMap::new();
        let mut contained_roots: HashMap<usize, Vec<usize>> = HashMap::new();

        for block in &ir_func.blocks {
            for instruction in &block.instructions {
                if let InstructionKind::Alloca { result, ty } = &instruction.kind {
                    if Self::is_stack_alloca_type(ty) {
                        stack_allocas.insert(result.id);
                        alloca_types.insert(result.id, ty.clone());
                        derived_from_alloca.insert(result.id, result.id);
                    }
                }
            }
        }

        let mut changed = true;
        while changed {
            changed = false;
            for block in &ir_func.blocks {
                for instruction in &block.instructions {
                    if let InstructionKind::GetElementPtr { result, ptr, .. } = &instruction.kind {
                        if let Some(root) = derived_from_alloca.get(&ptr.id).copied() {
                            if derived_from_alloca.insert(result.id, root).is_none() {
                                changed = true;
                            }
                        }
                    }
                }
            }
        }

        for block in &ir_func.blocks {
            for instruction in &block.instructions {
                if let InstructionKind::Store { ptr, value } = &instruction.kind {
                    if let Some(value_root) = derived_from_alloca.get(&value.id).copied() {
                        if let Some(ptr_root) = derived_from_alloca.get(&ptr.id).copied() {
                            if ptr_root != value_root {
                                contained_roots
                                    .entry(ptr_root)
                                    .or_default()
                                    .push(value_root);
                            }
                        }
                    }
                }
            }
        }

        for block in &ir_func.blocks {
            if let Some(Terminator::Return { value: Some(value) }) = &block.terminator {
                if let Some(root) = derived_from_alloca.get(&value.id) {
                    stack_allocas.remove(root);
                }
            }

            for instruction in &block.instructions {
                match &instruction.kind {
                    InstructionKind::Store { ptr, value } => {
                        if let Some(root) = derived_from_alloca.get(&value.id) {
                            if !derived_from_alloca.contains_key(&ptr.id) {
                                stack_allocas.remove(root);
                            }
                        }
                    }
                    InstructionKind::HostCall { args, .. }
                    | InstructionKind::CallIndirect { args, .. } => {
                        for arg in args {
                            if let Some(root) = derived_from_alloca.get(&arg.id) {
                                stack_allocas.remove(root);
                            }
                        }
                    }
                    InstructionKind::AsyncReady {
                        value: Some(value), ..
                    } => {
                        if let Some(root) = derived_from_alloca.get(&value.id) {
                            stack_allocas.remove(root);
                        }
                    }
                    InstructionKind::MakeDynFatPtr { data_ptr, .. } => {
                        if let Some(root) = derived_from_alloca.get(&data_ptr.id) {
                            stack_allocas.remove(root);
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut changed = true;
        while changed {
            changed = false;
            for (container, values) in &contained_roots {
                if !stack_allocas.contains(container) {
                    for value_root in values {
                        if stack_allocas.remove(value_root) {
                            changed = true;
                        }
                    }
                }
            }
        }

        stack_allocas
            .into_iter()
            .filter(|id| alloca_types.contains_key(id))
            .collect()
    }

    /// Return scalar stack allocas that can be represented by Cranelift
    /// variables instead of memory.  This is deliberately conservative: the
    /// promotion is allowed only when the alloca's value is used exclusively as
    /// the pointer of a load/store pair.  Any pointer arithmetic, copy,
    /// control-flow transport, return, or call argument keeps the original
    /// address-based lowering.
    #[cfg(test)]
    pub(crate) fn collect_promotable_scalar_allocas(
        ir_func: &IRFunction,
    ) -> HashMap<usize, IRType> {
        let stack_allocas = Self::collect_stack_allocas(ir_func);
        Self::collect_promotable_scalar_allocas_with_stack_allocas(ir_func, &stack_allocas)
    }

    pub(crate) fn collect_promotable_scalar_allocas_with_stack_allocas(
        ir_func: &IRFunction,
        stack_allocas: &HashSet<usize>,
    ) -> HashMap<usize, IRType> {
        let mut candidates = HashMap::new();
        for block in &ir_func.blocks {
            for instruction in &block.instructions {
                if let InstructionKind::Alloca { result, ty } = &instruction.kind {
                    if stack_allocas.contains(&result.id)
                        && matches!(
                            ty,
                            IRType::Int | IRType::Float | IRType::Bool | IRType::Char
                        )
                    {
                        candidates.insert(result.id, ty.clone());
                    }
                }
            }
        }

        if candidates.is_empty() {
            // Unlike the old `retain` loop, the linear scanner must not walk
            // every instruction when this function has no scalar stack
            // allocas to consider.
            return candidates;
        }

        let candidate_capacity = candidates
            .keys()
            .copied()
            .max()
            .map_or(ir_func.next_value_id, |max_id| {
                ir_func.next_value_id.max(max_id.saturating_add(1))
            });
        let mut candidate_mask = vec![false; candidate_capacity];
        for id in candidates.keys().copied() {
            candidate_mask[id] = true;
        }

        // Scan each instruction once and mark only the scalar allocas that
        // escape through an operand.  The former implementation retained
        // every candidate for every instruction, which made this analysis
        // quadratic for functions containing many scalar allocas.
        let mut escaped = vec![false; candidate_capacity];
        let mut entry_stores = vec![false; candidate_capacity];
        let mut local_stores = vec![false; candidate_capacity];
        let mut load_sites = vec![Vec::<usize>::new(); candidate_capacity];
        let mut store_blocks = vec![Vec::<usize>::new(); candidate_capacity];
        for (block_index, block) in ir_func.blocks.iter().enumerate() {
            local_stores.fill(false);
            for instruction in &block.instructions {
                Self::mark_scalar_alloca_escapes(&candidate_mask, &mut escaped, &instruction.kind);
                match &instruction.kind {
                    InstructionKind::Load { ptr, .. }
                        if candidate_mask.get(ptr.id).copied().unwrap_or(false) =>
                    {
                        // Loads after a local store are already proven safe.
                        // Other loads are retained for the small CFG check
                        // after this linear operand scan.
                        if !local_stores[ptr.id] {
                            load_sites[ptr.id].push(block_index);
                        }
                    }
                    InstructionKind::Store { ptr, .. }
                        if candidate_mask.get(ptr.id).copied().unwrap_or(false) =>
                    {
                        local_stores[ptr.id] = true;
                        if block_index == 0 {
                            entry_stores[ptr.id] = true;
                        }
                        if !store_blocks[ptr.id].contains(&block_index) {
                            store_blocks[ptr.id].push(block_index);
                        }
                    }
                    _ => {}
                }
            }
            if let Some(terminator) = &block.terminator {
                Self::mark_scalar_alloca_escapes_terminator(
                    &candidate_mask,
                    &mut escaped,
                    terminator,
                );
            }
        }

        candidates.retain(|id, _| !escaped.get(*id).copied().unwrap_or(false));
        Self::filter_uninitialized_scalar_allocas(
            ir_func,
            &mut candidates,
            &candidate_mask,
            &entry_stores,
            &load_sites,
            &store_blocks,
        );
        candidates
    }

    fn mark_scalar_alloca_value(candidate_mask: &[bool], escaped: &mut [bool], value: &IRValue) {
        if candidate_mask.get(value.id).copied().unwrap_or(false) {
            escaped[value.id] = true;
        }
    }

    fn mark_scalar_alloca_values(
        candidate_mask: &[bool],
        escaped: &mut [bool],
        values: &[IRValue],
    ) {
        for value in values {
            Self::mark_scalar_alloca_value(candidate_mask, escaped, value);
        }
    }

    fn mark_scalar_alloca_escapes(
        candidate_mask: &[bool],
        escaped: &mut [bool],
        kind: &InstructionKind,
    ) {
        match kind {
            InstructionKind::Load { .. } => {}
            InstructionKind::Store { value, .. } => {
                Self::mark_scalar_alloca_value(candidate_mask, escaped, value)
            }
            InstructionKind::GetElementPtr { ptr, .. } => {
                Self::mark_scalar_alloca_value(candidate_mask, escaped, ptr)
            }
            InstructionKind::Call { args, .. } | InstructionKind::HostCall { args, .. } => {
                Self::mark_scalar_alloca_values(candidate_mask, escaped, args)
            }
            InstructionKind::AutodiffStep {
                output,
                upstream,
                inputs,
                targets,
                ..
            } => {
                Self::mark_scalar_alloca_value(candidate_mask, escaped, output);
                if let Some(value) = upstream {
                    Self::mark_scalar_alloca_value(candidate_mask, escaped, value);
                }
                Self::mark_scalar_alloca_values(candidate_mask, escaped, inputs);
                Self::mark_scalar_alloca_values(candidate_mask, escaped, targets);
            }
            InstructionKind::CallIndirect { fn_ptr, args, .. } => {
                Self::mark_scalar_alloca_value(candidate_mask, escaped, fn_ptr);
                Self::mark_scalar_alloca_values(candidate_mask, escaped, args);
            }
            InstructionKind::AsyncReady { value, .. } => {
                if let Some(value) = value {
                    Self::mark_scalar_alloca_value(candidate_mask, escaped, value);
                }
            }
            InstructionKind::Phi { incoming, .. } => {
                for (value, _) in incoming {
                    Self::mark_scalar_alloca_value(candidate_mask, escaped, value);
                }
            }
            InstructionKind::Copy { source, .. }
            | InstructionKind::Cast {
                operand: source, ..
            }
            | InstructionKind::LoadDynDataPtr {
                fat_ptr: source, ..
            }
            | InstructionKind::LoadDynVtablePtr {
                fat_ptr: source, ..
            }
            | InstructionKind::FieldPtr { ptr: source, .. } => {
                Self::mark_scalar_alloca_value(candidate_mask, escaped, source)
            }
            InstructionKind::MakeDynFatPtr {
                data_ptr,
                vtable_ptr,
                ..
            } => {
                Self::mark_scalar_alloca_value(candidate_mask, escaped, data_ptr);
                Self::mark_scalar_alloca_value(candidate_mask, escaped, vtable_ptr);
            }
            InstructionKind::LoadVtableSlot { vtable_ptr, .. } => {
                Self::mark_scalar_alloca_value(candidate_mask, escaped, vtable_ptr)
            }
            InstructionKind::Add { lhs, rhs, .. }
            | InstructionKind::Sub { lhs, rhs, .. }
            | InstructionKind::Mul { lhs, rhs, .. }
            | InstructionKind::Div { lhs, rhs, .. }
            | InstructionKind::Rem { lhs, rhs, .. }
            | InstructionKind::Eq { lhs, rhs, .. }
            | InstructionKind::Ne { lhs, rhs, .. }
            | InstructionKind::Lt { lhs, rhs, .. }
            | InstructionKind::Le { lhs, rhs, .. }
            | InstructionKind::Gt { lhs, rhs, .. }
            | InstructionKind::Ge { lhs, rhs, .. }
            | InstructionKind::And { lhs, rhs, .. }
            | InstructionKind::Or { lhs, rhs, .. } => {
                Self::mark_scalar_alloca_value(candidate_mask, escaped, lhs);
                Self::mark_scalar_alloca_value(candidate_mask, escaped, rhs);
            }
            InstructionKind::Not { operand, .. } => {
                Self::mark_scalar_alloca_value(candidate_mask, escaped, operand)
            }
            InstructionKind::Alloca { .. }
            | InstructionKind::GlobalAddr { .. }
            | InstructionKind::ManualAlloc { .. }
            | InstructionKind::FuncAddr { .. }
            | InstructionKind::ConstInt { .. }
            | InstructionKind::ConstIntTyped { .. }
            | InstructionKind::ConstFloat { .. }
            | InstructionKind::ConstFloatTyped { .. }
            | InstructionKind::ConstBool { .. }
            | InstructionKind::ConstString { .. }
            | InstructionKind::EscapeManualAlloc { .. } => {}
        }
    }

    fn mark_scalar_alloca_escapes_terminator(
        candidate_mask: &[bool],
        escaped: &mut [bool],
        terminator: &Terminator,
    ) {
        match terminator {
            Terminator::Return { value } => {
                if let Some(value) = value {
                    Self::mark_scalar_alloca_value(candidate_mask, escaped, value);
                }
            }
            Terminator::CondBranch { condition, .. } => {
                Self::mark_scalar_alloca_value(candidate_mask, escaped, condition)
            }
            Terminator::Switch { value, .. } => {
                Self::mark_scalar_alloca_value(candidate_mask, escaped, value)
            }
            Terminator::Branch { .. } | Terminator::Unreachable => {}
        }
    }

    fn filter_uninitialized_scalar_allocas(
        ir_func: &IRFunction,
        candidates: &mut HashMap<usize, IRType>,
        candidate_mask: &[bool],
        entry_stores: &[bool],
        load_sites: &[Vec<usize>],
        store_blocks: &[Vec<usize>],
    ) {
        if candidates.is_empty() || ir_func.blocks.is_empty() {
            return;
        }

        let mut ambiguous_loads = Vec::new();
        let mut uninitialized = vec![false; candidate_mask.len()];
        for id in 0..candidate_mask.len() {
            if !candidate_mask[id] {
                continue;
            }
            for &load_block in &load_sites[id] {
                if load_block == 0 {
                    // A load before a store in the entry block cannot be
                    // dominated by a later store in that same block.
                    uninitialized[id] = true;
                } else if !entry_stores[id] {
                    // A non-entry store may still dominate this load; defer
                    // only these uncommon cross-block cases to the CFG pass.
                    ambiguous_loads.push((id, load_block));
                }
            }
        }

        if !ambiguous_loads.is_empty() {
            let block_indices: HashMap<usize, usize> = ir_func
                .blocks
                .iter()
                .enumerate()
                .map(|(index, block)| (block.id, index))
                .collect();
            let mut predecessors = vec![Vec::<usize>::new(); ir_func.blocks.len()];
            for (index, block) in ir_func.blocks.iter().enumerate() {
                let Some(terminator) = &block.terminator else {
                    continue;
                };
                let mut add_predecessor = |target: usize| {
                    if let Some(&target_index) = block_indices.get(&target) {
                        predecessors[target_index].push(index);
                    }
                };
                match terminator {
                    Terminator::Branch { target } => add_predecessor(*target),
                    Terminator::CondBranch {
                        true_block,
                        false_block,
                        ..
                    } => {
                        add_predecessor(*true_block);
                        add_predecessor(*false_block);
                    }
                    Terminator::Switch { cases, default, .. } => {
                        for (_, target) in cases {
                            add_predecessor(*target);
                        }
                        add_predecessor(*default);
                    }
                    Terminator::Return { .. } | Terminator::Unreachable => {}
                }
            }

            let all_blocks: HashSet<usize> = (0..ir_func.blocks.len()).collect();
            let mut dominators = vec![HashSet::new(); ir_func.blocks.len()];
            dominators[0].insert(0);
            for index in 1..ir_func.blocks.len() {
                if !predecessors[index].is_empty() {
                    dominators[index] = all_blocks.clone();
                }
            }
            loop {
                let mut changed = false;
                for index in 1..ir_func.blocks.len() {
                    let mut next = if predecessors[index].is_empty() {
                        HashSet::new()
                    } else {
                        let mut intersection = dominators[predecessors[index][0]].clone();
                        for predecessor in predecessors[index].iter().skip(1) {
                            intersection
                                .retain(|dominator| dominators[*predecessor].contains(dominator));
                        }
                        intersection
                    };
                    if !next.is_empty() {
                        next.insert(index);
                    }
                    if dominators[index] != next {
                        dominators[index] = next;
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }

            for (id, load_block) in ambiguous_loads {
                let dominated_by_store = store_blocks[id].iter().any(|store_block| {
                    *store_block != load_block && dominators[load_block].contains(store_block)
                });
                if !dominated_by_store {
                    uninitialized[id] = true;
                }
            }
        }

        candidates.retain(|id, _| !uninitialized.get(*id).copied().unwrap_or(false));
    }

    pub(crate) fn function_needs_manual_frame(
        ir_func: &IRFunction,
        stack_allocas: &HashSet<usize>,
    ) -> bool {
        for block in &ir_func.blocks {
            for instruction in &block.instructions {
                match &instruction.kind {
                    InstructionKind::Alloca { result, .. }
                        if !stack_allocas.contains(&result.id) =>
                    {
                        return true;
                    }
                    InstructionKind::HostCall { .. } => return true,
                    InstructionKind::MakeDynFatPtr { .. } => return true,
                    _ => {}
                }
            }
        }
        false
    }

    fn is_stack_alloca_type(ty: &IRType) -> bool {
        match ty {
            IRType::Int | IRType::Float | IRType::Bool | IRType::Char => true,
            IRType::Array { element_type, size } => {
                *size <= 4096 && Self::is_stack_alloca_type(element_type)
            }
            IRType::Struct { fields, .. } => {
                fields.len() <= 64
                    && fields
                        .iter()
                        .all(|(_, field_ty)| Self::is_stack_alloca_type(field_ty))
            }
            _ => false,
        }
    }

    const R3105_MAX_BATCH_CALLS: usize = 8;
    const R3105_MAX_BATCH_STACK_BYTES: usize = 4096;
    /// Cache-aware descriptors carry one additional pointer. The public
    /// batch statistics intentionally continue to report only arena sizes.
    const R3105_BATCH_DESCRIPTOR_BYTES: usize = 7 * std::mem::size_of::<i64>();

    fn hostcall_batch_scalar_type(ty: Type) -> bool {
        ty == types::I8
            || ty == types::I16
            || ty == types::I32
            || ty == types::I64
            || ty == types::F32
            || ty == types::F64
    }

    fn hostcall_batch_result_type(result_type: Option<&IRType>) -> bool {
        matches!(
            result_type,
            None | Some(
                IRType::Int
                    | IRType::Float
                    | IRType::ExactInt { .. }
                    | IRType::ExactFloat { .. }
                    | IRType::Bool
                    | IRType::Char
            )
        )
    }

    /// Return the exclusive end of a safe batch beginning at `start`, or
    /// `start` when the current instruction must use individual lowering.
    fn hostcall_batch_end(
        instructions: &[Instruction],
        start: usize,
        value_map: &DenseValueMap,
        builder: &FunctionBuilder,
        host_call_sites: &HashMap<String, HostCallSiteRecord>,
    ) -> usize {
        let Some(InstructionKind::HostCall { host, .. }) =
            instructions.get(start).map(|instruction| &instruction.kind)
        else {
            return start;
        };
        if !classify_host_call(host).batch_eligible() {
            return start;
        }

        let mut produced_results = HashSet::new();
        let mut argument_words = 0usize;
        let mut result_slots = 0usize;
        let mut end = start;

        while end < instructions.len() && end - start < Self::R3105_MAX_BATCH_CALLS {
            let InstructionKind::HostCall {
                host,
                args,
                result,
                result_type,
                ..
            } = &instructions[end].kind
            else {
                break;
            };
            let fast_hostcall = matches!(classify_host_call(host), HostCallClass::Fast(_));
            let known_hostcall = host_call_sites.contains_key(host);
            let scalar_result = Self::hostcall_batch_result_type(result_type.as_ref());
            let dependent_argument = args.iter().any(|arg| produced_results.contains(&arg.id));
            let unsupported_argument = args.iter().any(|arg| {
                value_map
                    .get(arg.id)
                    .map(|value| {
                        !Self::hostcall_batch_scalar_type(builder.func.dfg.value_type(value))
                    })
                    .unwrap_or(true)
            });
            if fast_hostcall
                || !known_hostcall
                || !scalar_result
                || dependent_argument
                || unsupported_argument
            {
                break;
            }

            let Some(next_argument_words) = argument_words.checked_add(args.len()) else {
                break;
            };
            let next_result_slots = result_slots + usize::from(result.is_some());
            let Some(descriptor_bytes) =
                (end - start + 1).checked_mul(Self::R3105_BATCH_DESCRIPTOR_BYTES)
            else {
                break;
            };
            let Some(argument_bytes) = next_argument_words.checked_mul(8) else {
                break;
            };
            let Some(result_bytes) = next_result_slots.checked_mul(8) else {
                break;
            };
            if descriptor_bytes
                .saturating_add(argument_bytes)
                .saturating_add(result_bytes)
                > Self::R3105_MAX_BATCH_STACK_BYTES
            {
                break;
            }

            argument_words = next_argument_words;
            result_slots = next_result_slots;
            if let Some(result) = result {
                produced_results.insert(result.id);
            }
            end += 1;
        }

        if end - start >= 2 {
            end
        } else {
            start
        }
    }

}
