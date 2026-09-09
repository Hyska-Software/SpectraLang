use super::*;
use crate::ir::{
    AsyncCoroutineLayout, AsyncFrameSlot, AsyncState, Instruction, InstructionKind,
};

const POLL_PENDING: i64 = 0;
const POLL_READY: i64 = 1;
const POLL_FAILED: i64 = 2;
const POLL_CANCELLED: i64 = 3;

impl ASTLowering {
    /// Turn a lowered async body into a body-free public ramp and a generated
    /// stackless poll/drop pair. The source body is lowered once, then moved
    /// wholesale into the poll function; the ramp only allocates/initializes a
    /// frame and creates a task.
    pub(crate) fn finish_async_coroutine(
        &mut self,
        mut body: IRFunction,
        source_params: Vec<Parameter>,
        output_type: IRType,
    ) -> IRFunction {
        let base = body.name.clone();
        let poll_name = format!("{base}__poll");
        let drop_name = format!("{base}__drop");
        shift_body_values(&mut body, 3);
        let mut type_hints = collect_value_types(&body);
        for (index, parameter) in source_params.iter().enumerate() {
            type_hints.insert(index + 3, parameter.ty.clone());
        }
        let poll_params = vec![
            Parameter { id: 0, name: "frame_ptr".into(), ty: IRType::Int },
            Parameter { id: 1, name: "task_id".into(), ty: IRType::Int },
            Parameter { id: 2, name: "poll_ctx".into(), ty: IRType::Int },
        ];
        body.params = poll_params;
        body.return_type = IRType::Int;
        body.name = poll_name.clone();
        body.suspension_barrier = true;
        body.next_value_id = body.next_value_id.max(body.params.len());

        // Frame slots are built comprehensively after all splits (see below):
        // every value id gets a slot, so any conservative reload set is safe.

        let dispatch = body.add_block("coroutine.dispatch");
        let mut states = vec![AsyncState { id: 0, resume_block: body.blocks.first().map(|b| b.id).unwrap_or(0), await_slot: None }];
        let frame = Value { id: 0 };
        let task = Value { id: 1 };
        let mut next_state = 1usize;

        // Split one source await at a time. The pre-await block evaluates the
        // child exactly once, then switches on its status. Pending stores the
        // values available at that point before returning to the runtime.
        loop {
            let found = body.blocks.iter().find_map(|block| {
                block.instructions.iter().position(|instruction| {
                    matches!(instruction.kind, InstructionKind::Await { .. })
                }).map(|position| (block.id, position))
            });
            let Some((block_id, position)) = found else { break };
            let block_index = body.blocks.iter().position(|block| block.id == block_id).unwrap();
            let old = body.blocks[block_index].clone();
            let InstructionKind::Await { result, task: child, output_type } = old.instructions[position].kind.clone() else { unreachable!() };
            let before = old.instructions[..position].to_vec();
            let after = old.instructions[position + 1..].to_vec();
            let continuation = body.add_block(format!("await{next_state}.resume"));
            let pending = body.add_block(format!("await{next_state}.pending"));
            let failed = body.add_block(format!("await{next_state}.failed"));
            let cancelled = body.add_block(format!("await{next_state}.cancelled"));
            let ready_block = body.add_block(format!("await{next_state}.ready"));
            let state_id = next_state;
            next_state += 1;

            let status = body.next_value();
            let ready_test = body.next_value();
            // The split itself only moves code and appends machine
            // instructions using ORIGINAL ids. A uniform post-split pass (see
            // below) prepends frame reloads with fresh ids wherever a block
            // uses values it does not define, which keeps the backend's
            // flow-insensitive value map sound on every resume path.
            let mut pre = before;
            pre.push(Instruction { id: pre.len(), kind: InstructionKind::CoroutinePollChild { status, result: None, task: child, output_type: output_type.clone() }, source_span: None });
            let ready_const = body.next_value();
            pre.push(Instruction { id: pre.len(), kind: InstructionKind::ConstInt { result: ready_const, value: POLL_READY }, source_span: None });
            pre.push(Instruction { id: pre.len(), kind: InstructionKind::Eq { result: ready_test, lhs: status, rhs: ready_const }, source_span: None });
            let mut switch_cases = vec![(POLL_READY, continuation), (POLL_FAILED, failed), (POLL_CANCELLED, cancelled)];
            switch_cases.sort_by_key(|(value, _)| *value);
            body.blocks[block_index].instructions = pre;
            body.blocks[block_index].terminator = Some(Terminator::Switch { value: status, cases: switch_cases, default: pending });
            let status2 = body.next_value();
            let ready_test2 = body.next_value();
            let ready_const2 = body.next_value();
            let mut continuation_instructions = Vec::new();
            continuation_instructions.push(Instruction { id: continuation_instructions.len(), kind: InstructionKind::CoroutinePollChild { status: status2, result: Some(result), task: child, output_type: output_type.clone() }, source_span: None });
            continuation_instructions.push(Instruction { id: continuation_instructions.len(), kind: InstructionKind::ConstInt { result: ready_const2, value: POLL_READY }, source_span: None });
            continuation_instructions.push(Instruction { id: continuation_instructions.len(), kind: InstructionKind::Eq { result: ready_test2, lhs: status2, rhs: ready_const2 }, source_span: None });
            if let Some(block) = body.get_block_mut(continuation) {
                block.instructions = continuation_instructions;
                block.terminator = Some(Terminator::Switch { value: status2, cases: vec![(POLL_READY, ready_block), (POLL_FAILED, failed), (POLL_CANCELLED, cancelled)], default: pending });
            }
            if let Some(block) = body.get_block_mut(ready_block) {
                block.instructions = after;
                block.terminator = old.terminator.clone();
            }

            let pending_status = body.next_value();
            if let Some(block) = body.get_block_mut(pending) {
                block.instructions.push(Instruction { id: block.instructions.len(), kind: InstructionKind::StateStore { frame, state: state_id }, source_span: None });
                block.instructions.push(Instruction { id: block.instructions.len(), kind: InstructionKind::CoroutineSubscribe { task: child, parent: task }, source_span: None });
                block.instructions.push(Instruction { id: block.instructions.len(), kind: InstructionKind::CoroutineWake { task: child }, source_span: None });
                block.instructions.push(Instruction { id: block.instructions.len(), kind: InstructionKind::CoroutineSuspend { task, state: state_id }, source_span: None });
                block.instructions.push(Instruction { id: block.instructions.len(), kind: InstructionKind::ConstInt { result: pending_status, value: POLL_PENDING }, source_span: None });
                block.instructions.push(Instruction { id: block.instructions.len(), kind: InstructionKind::CoroutinePollReturn { status: pending_status }, source_span: None });
                block.terminator = Some(Terminator::Return { value: Some(pending_status) });
            }
            let failed_status = body.next_value();
            let cancelled_status = body.next_value();
            for (target, status_code, kind, status_value) in [(failed, POLL_FAILED, 0u8, failed_status), (cancelled, POLL_CANCELLED, 1u8, cancelled_status)] {
                if let Some(block) = body.get_block_mut(target) {
                    block.instructions.push(Instruction { id: block.instructions.len(), kind: if kind == 0 { InstructionKind::CoroutineError { task, error: None } } else { InstructionKind::CoroutineCancelled { task } }, source_span: None });
                    block.instructions.push(Instruction { id: block.instructions.len(), kind: InstructionKind::ConstInt { result: status_value, value: status_code }, source_span: None });
                    block.instructions.push(Instruction { id: block.instructions.len(), kind: InstructionKind::CoroutinePollReturn { status: status_value }, source_span: None });
                    block.terminator = Some(Terminator::Return { value: Some(status_value) });
                }
            }
            states.push(AsyncState { id: state_id, resume_block: continuation, await_slot: Some(state_id) });
        }

        // Uniform suspension dataflow pass. For EVERY block, every operand it
        // uses but does not define (and which is not a function parameter) is
        // reloaded from the frame into a FRESH id prepended to the block, and
        // the uses are rewritten. Fresh ids keep the backend's
        // flow-insensitive value map sound: each is defined by its own load,
        // which dominates every use in the block and is generated immediately
        // before it. Original ids are never redefined, so blocks generated
        // earlier keep valid mappings. This covers pre blocks, continuations,
        // ready blocks, and untouched downstream blocks uniformly, on both
        // the fall-through path and every resume path.
        // Runtime soundness: slots are task-private and persist across polls;
        // every slot is written at its value's definition site (see below),
        // and each value id is defined exactly once, so a slot always holds
        // the reaching definition's value. Phi incoming edges keep their
        // original ids (predecessor-supplied block arguments).
        let post_param_ids: std::collections::BTreeSet<usize> =
            [0, 1, 2].into_iter().chain((0..source_params.len()).map(|id| id + 3)).collect();
        // Phase 1 (immutable): compute the reload set per block.
        let mut reload_plan: Vec<(usize, Vec<usize>)> = Vec::new();
        for block in body.blocks.iter() {
            let mut defs = std::collections::BTreeSet::new();
            let mut uses = Vec::new();
            for instruction in block.instructions.iter() {
                if matches!(instruction.kind, InstructionKind::Phi { .. }) {
                    if let Some(result) = instruction_result(instruction) {
                        defs.insert(result.id);
                    }
                    continue;
                }
                collect_instruction_uses(instruction, &mut uses);
                if let Some(result) = instruction_result(instruction) {
                    defs.insert(result.id);
                }
                if let InstructionKind::CoroutinePollChild { status, .. } = &instruction.kind {
                    defs.insert(status.id);
                }
            }
            if let Some(terminator) = block.terminator.as_ref() {
                collect_terminator_uses(terminator, &mut uses);
            }
            let mut need: Vec<usize> = uses
                .into_iter()
                .filter(|id| !defs.contains(id) && !post_param_ids.contains(id))
                .collect();
            need.sort_unstable();
            need.dedup();
            if !need.is_empty() {
                reload_plan.push((block.id, need));
            }
        }
        // Phase 2 (mutable): prepend fresh loads and rewrite uses.
        for (block_id, need) in reload_plan {
            let mut remap: std::collections::HashMap<usize, Value> = std::collections::HashMap::new();
            let mut loads = Vec::with_capacity(need.len());
            for orig_id in need {
                let fresh = body.next_value();
                loads.push(Instruction {
                    id: loads.len(),
                    kind: InstructionKind::FrameLoad {
                        result: fresh,
                        frame,
                        slot: orig_id,
                        ty: type_hints.get(&orig_id).cloned().unwrap_or(IRType::Int),
                    },
                    source_span: None,
                });
                remap.insert(orig_id, fresh);
            }
            let Some(block) = body.get_block_mut(block_id) else { continue };
            for instruction in block.instructions.iter_mut() {
                if matches!(instruction.kind, InstructionKind::Phi { .. }) {
                    continue;
                }
                remap_instruction_operands(&mut instruction.kind, &remap);
            }
            if let Some(terminator) = block.terminator.as_mut() {
                remap_terminator_operands(terminator, &remap);
            }
            loads.extend(block.instructions.drain(..));
            for (index, instruction) in loads.iter_mut().enumerate() {
                instruction.id = index;
            }
            block.instructions = loads;
        }

        // Store every SSA value at its definition site. The store sits in the
        // same block immediately after the defining instruction, so the stored
        // Cranelift value dominates the store in both generation and CFG
        // order. FrameLoad results are already frame contents and need no
        // write-back. Terminal await-machine blocks (pending/failed/cancelled)
        // only hold block-local temps and are skipped.
        for block in body.blocks.iter_mut() {
            if block.label.contains(".pending")
                || block.label.contains(".failed")
                || block.label.contains(".cancelled")
            {
                continue;
            }
            let mut with_stores = Vec::with_capacity(block.instructions.len() * 2);
            for instruction in block.instructions.iter() {
                with_stores.push(instruction.clone());
                let store_value = match &instruction.kind {
                    InstructionKind::FrameLoad { .. } | InstructionKind::StateLoad { .. } => None,
                    _ => instruction_result(instruction),
                };
                if let Some(value) = store_value {
                    with_stores.push(Instruction {
                        id: with_stores.len(),
                        kind: InstructionKind::FrameStore { frame, slot: value.id, value },
                        source_span: None,
                    });
                }
            }
            for (index, instruction) in with_stores.iter_mut().enumerate() {
                instruction.id = index;
            }
            block.instructions = with_stores;
        }

        // Every ordinary terminal return completes the parent task and returns
        // Ready. Pending/failed/cancelled blocks were already terminalized.
        let terminal_returns = body.blocks.iter().filter_map(|block| {
            if block.label.contains(".pending") || block.label.contains(".failed") || block.label.contains(".cancelled") {
                return None;
            }
            match block.terminator {
                Some(Terminator::Return { value }) => Some((block.id, value)),
                _ => None,
            }
        }).collect::<Vec<_>>();
        for (block_id, value) in terminal_returns {
            let status_value = body.next_value();
            if let Some(block) = body.get_block_mut(block_id) {
                block.terminator = None;
                block.instructions.push(Instruction { id: block.instructions.len(), kind: InstructionKind::CoroutineComplete { task, value }, source_span: None });
                block.instructions.push(Instruction { id: block.instructions.len(), kind: InstructionKind::ConstInt { result: status_value, value: POLL_READY }, source_span: None });
                block.instructions.push(Instruction { id: block.instructions.len(), kind: InstructionKind::CoroutinePollReturn { status: status_value }, source_span: None });
                block.terminator = Some(Terminator::Return { value: Some(status_value) });
            }
        }

        // A single dispatch block is the only poll entry. State zero targets
        // the original entry; each await state targets its continuation.
        let state_value = body.next_value();
        if let Some(block) = body.get_block_mut(dispatch) {
            for (index, parameter) in source_params.iter().enumerate() {
                block.instructions.push(Instruction {
                    id: block.instructions.len(),
                    kind: InstructionKind::FrameLoad {
                        result: Value { id: index + 3 },
                        frame,
                        slot: index + 3,
                        ty: parameter.ty.clone(),
                    },
                    source_span: None,
                });
            }
            block.instructions.push(Instruction { id: block.instructions.len(), kind: InstructionKind::StateLoad { result: state_value, frame }, source_span: None });
            block.terminator = Some(Terminator::Switch {
                value: state_value,
                cases: states.iter().map(|state| (state.id as i64, state.resume_block)).collect(),
                default: states[0].resume_block,
            });
        }
        if let Some(index) = body.blocks.iter().position(|block| block.id == dispatch) {
            let block = body.blocks.remove(index);
            body.blocks.insert(0, block);
        }

        // Comprehensive frame slots: every value id (params, original SSA,
        // and machine temps) owns the slot with its own id. Unused slots are
        // harmless; this keeps every conservative reload/store in range.
        let final_hints = collect_value_types(&body);
        let slots: Vec<AsyncFrameSlot> = (0..body.next_value_id)
            .map(|id| {
                let (name, ty) = match id {
                    0 => ("frame_ptr".to_string(), IRType::Int),
                    1 => ("task_id".to_string(), IRType::Int),
                    2 => ("poll_ctx".to_string(), IRType::Int),
                    _ => (
                        format!("ssa{id}"),
                        final_hints.get(&id).cloned().unwrap_or(IRType::Int),
                    ),
                };
                AsyncFrameSlot { id, name, ty }
            })
            .collect();

        let layout = AsyncCoroutineLayout {
            poll_name: poll_name.clone(),
            drop_name: drop_name.clone(),
            output_type: output_type.clone(),
            frame_slots: slots,
            source_slots: source_params
                .iter()
                .enumerate()
                .map(|(id, parameter)| AsyncFrameSlot {
                    id: id + 3,
                    name: parameter.name.clone(),
                    ty: parameter.ty.clone(),
                })
                .collect(),
            states,
            poll_params: vec![IRType::Int, IRType::Int, IRType::Int],
            status_pending: POLL_PENDING,
            status_ready: POLL_READY,
            status_failed: POLL_FAILED,
            status_cancelled: POLL_CANCELLED,
        };
        body.async_layout = Some(layout.clone());

        let mut drop_func = IRFunction::new(&drop_name, vec![Parameter { id: 0, name: "frame_ptr".into(), ty: IRType::Int }], IRType::Void);
        let drop_entry = drop_func.add_block("drop");
        drop_func.suspension_barrier = true;
        drop_func.async_layout = Some(layout.clone());
        drop_func.get_block_mut(drop_entry).unwrap().terminator = Some(Terminator::Return { value: None });
        self.pending_coroutines.push(drop_func);
        self.pending_coroutines.push(body);

        let mut ramp = IRFunction::new(&base, source_params.clone(), IRType::Task { output: Box::new(output_type.clone()) });
        ramp.async_layout = Some(layout);
        let slot_count = ramp.async_layout.as_ref().and_then(|layout| layout.frame_slots.iter().map(|slot| slot.id).max()).map(|id| id + 1).unwrap_or(0);
        let entry = ramp.add_block("entry");
        let frame = ramp.next_value();
        ramp.get_block_mut(entry).unwrap().instructions.push(Instruction { id: 0, kind: InstructionKind::FrameAlloc { result: frame, layout: poll_name.clone(), slot_count }, source_span: None });
        for (index, param) in source_params.iter().enumerate() {
            ramp.get_block_mut(entry).unwrap().instructions.push(Instruction { id: index + 1, kind: InstructionKind::FrameStore { frame, slot: index + 3, value: Value { id: param.id } }, source_span: None });
        }
        let task = ramp.next_value();
        ramp.get_block_mut(entry).unwrap().instructions.push(Instruction { id: source_params.len() + 1, kind: InstructionKind::CoroutineCreate { result: task, frame, poll: poll_name, drop: drop_name, output_type }, source_span: None });
        ramp.get_block_mut(entry).unwrap().terminator = Some(Terminator::Return { value: Some(task) });
        ramp
    }
}

fn instruction_result(instruction: &Instruction) -> Option<Value> {
    match &instruction.kind {
        InstructionKind::Add { result, .. } | InstructionKind::Sub { result, .. } | InstructionKind::Mul { result, .. } | InstructionKind::Div { result, .. } | InstructionKind::Rem { result, .. } | InstructionKind::Eq { result, .. } | InstructionKind::Ne { result, .. } | InstructionKind::Lt { result, .. } | InstructionKind::Le { result, .. } | InstructionKind::Gt { result, .. } | InstructionKind::Ge { result, .. } | InstructionKind::And { result, .. } | InstructionKind::Or { result, .. } | InstructionKind::Not { result, .. } | InstructionKind::Alloca { result, .. } | InstructionKind::GlobalAddr { result, .. } | InstructionKind::ManualAlloc { result, .. } | InstructionKind::Load { result, .. } | InstructionKind::GetElementPtr { result, .. } | InstructionKind::FieldPtr { result, .. } | InstructionKind::FuncAddr { result, .. } | InstructionKind::AsyncReady { result, .. } | InstructionKind::Phi { result, .. } | InstructionKind::Copy { result, .. } | InstructionKind::ConstInt { result, .. } | InstructionKind::ConstIntTyped { result, .. } | InstructionKind::ConstFloat { result, .. } | InstructionKind::ConstFloatTyped { result, .. } | InstructionKind::ConstBool { result, .. } | InstructionKind::ConstString { result, .. } | InstructionKind::Cast { result, .. } | InstructionKind::MakeDynFatPtr { result, .. } | InstructionKind::LoadDynDataPtr { result, .. } | InstructionKind::LoadDynVtablePtr { result, .. } | InstructionKind::LoadVtableSlot { result, .. } | InstructionKind::FrameAlloc { result, .. } | InstructionKind::FrameLoad { result, .. } | InstructionKind::StateLoad { result, .. } | InstructionKind::CoroutineCreate { result, .. } => Some(*result),
        InstructionKind::AutodiffStep { result, .. } | InstructionKind::Call { result, .. } | InstructionKind::HostCall { result, .. } | InstructionKind::CallIndirect { result, .. } => result.as_ref().copied(),
        InstructionKind::CoroutinePollChild { result, .. } => *result,
        InstructionKind::Await { result, .. } => Some(*result),
        InstructionKind::Store { .. } | InstructionKind::EscapeManualAlloc { .. } | InstructionKind::FrameStore { .. } | InstructionKind::StateStore { .. } | InstructionKind::CoroutineSubscribe { .. } | InstructionKind::CoroutineWake { .. } | InstructionKind::CoroutineSuspend { .. } | InstructionKind::CoroutineComplete { .. } | InstructionKind::CoroutineError { .. } | InstructionKind::CoroutineCancelled { .. } | InstructionKind::CoroutinePollReturn { .. } => None,
    }
}

/// Rewrite operand ids through `map`, leaving result ids untouched. Used when
/// moved suspension-boundary code must name a resume block's fresh reloads
/// instead of the original SSA ids.
fn remap_value(value: &mut Value, map: &std::collections::HashMap<usize, Value>) {
    if let Some(replacement) = map.get(&value.id) {
        value.id = replacement.id;
    }
}

fn remap_instruction_operands(kind: &mut InstructionKind, map: &std::collections::HashMap<usize, Value>) {
    match kind {
        InstructionKind::Add { lhs, rhs, .. } | InstructionKind::Sub { lhs, rhs, .. } | InstructionKind::Mul { lhs, rhs, .. } | InstructionKind::Div { lhs, rhs, .. } | InstructionKind::Rem { lhs, rhs, .. } | InstructionKind::Eq { lhs, rhs, .. } | InstructionKind::Ne { lhs, rhs, .. } | InstructionKind::Lt { lhs, rhs, .. } | InstructionKind::Le { lhs, rhs, .. } | InstructionKind::Gt { lhs, rhs, .. } | InstructionKind::Ge { lhs, rhs, .. } | InstructionKind::And { lhs, rhs, .. } | InstructionKind::Or { lhs, rhs, .. } => { remap_value(lhs, map); remap_value(rhs, map); }
        InstructionKind::Not { operand, .. } | InstructionKind::Load { ptr: operand, .. } | InstructionKind::Copy { source: operand, .. } | InstructionKind::Cast { operand, .. } | InstructionKind::LoadDynDataPtr { fat_ptr: operand, .. } | InstructionKind::LoadDynVtablePtr { fat_ptr: operand, .. } | InstructionKind::LoadVtableSlot { vtable_ptr: operand, .. } | InstructionKind::EscapeManualAlloc { ptr: operand, .. } => remap_value(operand, map),
        InstructionKind::Store { ptr, value } => { remap_value(ptr, map); remap_value(value, map); }
        InstructionKind::GetElementPtr { ptr, index, .. } => { remap_value(ptr, map); remap_value(index, map); }
        InstructionKind::FieldPtr { ptr, .. } => remap_value(ptr, map),
        InstructionKind::Call { args, .. } | InstructionKind::HostCall { args, .. } => { for arg in args.iter_mut() { remap_value(arg, map); } }
        InstructionKind::CallIndirect { fn_ptr, args, .. } => { remap_value(fn_ptr, map); for arg in args.iter_mut() { remap_value(arg, map); } }
        InstructionKind::AsyncReady { value, .. } => { if let Some(value) = value { remap_value(value, map); } }
        InstructionKind::Phi { incoming, .. } => { for (value, _) in incoming.iter_mut() { remap_value(value, map); } }
        InstructionKind::MakeDynFatPtr { data_ptr, vtable_ptr, .. } => { remap_value(data_ptr, map); remap_value(vtable_ptr, map); }
        InstructionKind::Await { task, .. } => remap_value(task, map),
        InstructionKind::FrameStore { frame, value, .. } => { remap_value(frame, map); remap_value(value, map); }
        InstructionKind::FrameLoad { frame, .. } => remap_value(frame, map),
        InstructionKind::StateStore { frame, .. } => remap_value(frame, map),
        InstructionKind::CoroutineCreate { frame, .. } => remap_value(frame, map),
        InstructionKind::CoroutinePollChild { task, .. } => remap_value(task, map),
        InstructionKind::CoroutineSubscribe { task, parent } => { remap_value(task, map); remap_value(parent, map); }
        InstructionKind::CoroutineWake { task } | InstructionKind::CoroutineSuspend { task, .. } | InstructionKind::CoroutineCancelled { task } => remap_value(task, map),
        InstructionKind::CoroutineComplete { task, value } => { remap_value(task, map); if let Some(value) = value { remap_value(value, map); } }
        InstructionKind::CoroutineError { task, error } => { remap_value(task, map); if let Some(error) = error { remap_value(error, map); } }
        InstructionKind::CoroutinePollReturn { status } => remap_value(status, map),
        InstructionKind::AutodiffStep { output, upstream, inputs, targets, .. } => { remap_value(output, map); if let Some(upstream) = upstream { remap_value(upstream, map); } for value in inputs.iter_mut().chain(targets.iter_mut()) { remap_value(value, map); } }
        _ => {}
    }
}

/// Collect operand ids read by an instruction (results excluded).
fn collect_instruction_uses(instruction: &Instruction, out: &mut Vec<usize>) {
    match &instruction.kind {
        InstructionKind::Add { lhs, rhs, .. } | InstructionKind::Sub { lhs, rhs, .. } | InstructionKind::Mul { lhs, rhs, .. } | InstructionKind::Div { lhs, rhs, .. } | InstructionKind::Rem { lhs, rhs, .. } | InstructionKind::Eq { lhs, rhs, .. } | InstructionKind::Ne { lhs, rhs, .. } | InstructionKind::Lt { lhs, rhs, .. } | InstructionKind::Le { lhs, rhs, .. } | InstructionKind::Gt { lhs, rhs, .. } | InstructionKind::Ge { lhs, rhs, .. } | InstructionKind::And { lhs, rhs, .. } | InstructionKind::Or { lhs, rhs, .. } => out.extend([lhs.id, rhs.id]),
        InstructionKind::Not { operand, .. } | InstructionKind::Load { ptr: operand, .. } | InstructionKind::Copy { source: operand, .. } | InstructionKind::Cast { operand, .. } | InstructionKind::LoadDynDataPtr { fat_ptr: operand, .. } | InstructionKind::LoadDynVtablePtr { fat_ptr: operand, .. } | InstructionKind::LoadVtableSlot { vtable_ptr: operand, .. } | InstructionKind::EscapeManualAlloc { ptr: operand, .. } => out.push(operand.id),
        InstructionKind::Store { ptr, value } => out.extend([ptr.id, value.id]),
        InstructionKind::GetElementPtr { ptr, index, .. } => out.extend([ptr.id, index.id]),
        InstructionKind::FieldPtr { ptr, .. } => out.push(ptr.id),
        InstructionKind::Call { args, .. } | InstructionKind::HostCall { args, .. } => out.extend(args.iter().map(|arg| arg.id)),
        InstructionKind::CallIndirect { fn_ptr, args, .. } => { out.push(fn_ptr.id); out.extend(args.iter().map(|arg| arg.id)); }
        InstructionKind::AsyncReady { value, .. } => out.extend(value.iter().map(|value| value.id)),
        InstructionKind::Phi { incoming, .. } => out.extend(incoming.iter().map(|(value, _)| value.id)),
        InstructionKind::MakeDynFatPtr { data_ptr, vtable_ptr, .. } => out.extend([data_ptr.id, vtable_ptr.id]),
        InstructionKind::Await { task, .. } => out.push(task.id),
        InstructionKind::FrameStore { frame, value, .. } => out.extend([frame.id, value.id]),
        InstructionKind::FrameLoad { frame, .. } => out.push(frame.id),
        InstructionKind::StateStore { frame, .. } => out.push(frame.id),
        InstructionKind::CoroutineCreate { frame, .. } => out.push(frame.id),
        InstructionKind::CoroutinePollChild { task, .. } => out.push(task.id),
        InstructionKind::CoroutineSubscribe { task, parent } => out.extend([task.id, parent.id]),
        InstructionKind::CoroutineWake { task } | InstructionKind::CoroutineSuspend { task, .. } | InstructionKind::CoroutineCancelled { task } => out.push(task.id),
        InstructionKind::CoroutineComplete { task, value } => { out.push(task.id); out.extend(value.iter().map(|value| value.id)); }
        InstructionKind::CoroutineError { task, error } => { out.push(task.id); out.extend(error.iter().map(|error| error.id)); }
        InstructionKind::CoroutinePollReturn { status } => out.push(status.id),
        InstructionKind::AutodiffStep { output, upstream, inputs, targets, .. } => { out.push(output.id); out.extend(upstream.iter().map(|value| value.id)); out.extend(inputs.iter().map(|value| value.id)); out.extend(targets.iter().map(|value| value.id)); }
        _ => {}
    }
}

/// Collect value ids read by a terminator (branch targets excluded).
fn collect_terminator_uses(terminator: &Terminator, out: &mut Vec<usize>) {
    match terminator {
        Terminator::Return { value } => out.extend(value.iter().map(|value| value.id)),
        Terminator::CondBranch { condition, .. } => out.push(condition.id),
        Terminator::Switch { value, .. } => out.push(value.id),
        _ => {}
    }
}

fn remap_terminator_operands(terminator: &mut Terminator, map: &std::collections::HashMap<usize, Value>) {
    match terminator {
        Terminator::Return { value } => { if let Some(value) = value { remap_value(value, map); } }
        Terminator::CondBranch { condition, .. } => remap_value(condition, map),
        Terminator::Switch { value, .. } => remap_value(value, map),
        _ => {}
    }
}
fn shift_body_values(function: &mut IRFunction, amount: usize) {
    fn shift(value: &mut Value, amount: usize) { value.id += amount; }
    for block in &mut function.blocks {
        for instruction in &mut block.instructions {
            match &mut instruction.kind {
                InstructionKind::Add { result, lhs, rhs }
                | InstructionKind::Sub { result, lhs, rhs }
                | InstructionKind::Mul { result, lhs, rhs }
                | InstructionKind::Div { result, lhs, rhs }
                | InstructionKind::Rem { result, lhs, rhs }
                | InstructionKind::Eq { result, lhs, rhs }
                | InstructionKind::Ne { result, lhs, rhs }
                | InstructionKind::Lt { result, lhs, rhs }
                | InstructionKind::Le { result, lhs, rhs }
                | InstructionKind::Gt { result, lhs, rhs }
                | InstructionKind::Ge { result, lhs, rhs }
                | InstructionKind::And { result, lhs, rhs }
                | InstructionKind::Or { result, lhs, rhs } => { shift(result, amount); shift(lhs, amount); shift(rhs, amount); }
                InstructionKind::Not { result, operand } => { shift(result, amount); shift(operand, amount); }
                InstructionKind::Copy { result, source } => { shift(result, amount); shift(source, amount); }
                InstructionKind::Cast { result, operand, .. } => { shift(result, amount); shift(operand, amount); }
                InstructionKind::Load { result, ptr, .. }
                | InstructionKind::FieldPtr { result, ptr, .. } => { shift(result, amount); shift(ptr, amount); }
                // The vtable slot index is a value id like any other operand: it must
                // move with the shift, otherwise dynamic dispatch inside a coroutine
                // indexes the vtable with a stale id (silent wrong callee/offset).
                InstructionKind::GetElementPtr { result, ptr, index, .. } => { shift(result, amount); shift(ptr, amount); shift(index, amount); }
                InstructionKind::Store { ptr, value } => { shift(ptr, amount); shift(value, amount); }
                InstructionKind::Call { result, args, .. }
                | InstructionKind::HostCall { result, args, .. } => {
                    if let Some(value) = result { shift(value, amount); }
                    for value in args { shift(value, amount); }
                }
                // Same staleness hazard as the GEP index: an unshifted callee id
                // resolves to whatever value now owns the old id.
                InstructionKind::CallIndirect { result, fn_ptr, args, .. } => {
                    if let Some(value) = result { shift(value, amount); }
                    shift(fn_ptr, amount);
                    for value in args { shift(value, amount); }
                }
                InstructionKind::Await { result, task, .. } => { shift(result, amount); shift(task, amount); }
                InstructionKind::ConstInt { result, .. }
                | InstructionKind::ConstIntTyped { result, .. }
                | InstructionKind::ConstFloat { result, .. }
                | InstructionKind::ConstFloatTyped { result, .. }
                | InstructionKind::ConstBool { result, .. }
                | InstructionKind::ConstString { result, .. }
                | InstructionKind::Alloca { result, .. }
                | InstructionKind::GlobalAddr { result, .. }
                | InstructionKind::ManualAlloc { result, .. }
                | InstructionKind::FuncAddr { result, .. } => shift(result, amount),
                // `value` is an operand id: without the shift an `async.ready(v)` payload
                // resolves to a stale id after the coroutine prologue renumbering.
                InstructionKind::AsyncReady { result, value, .. } => { shift(result, amount); if let Some(value) = value { shift(value, amount); } }
                InstructionKind::Phi { result, incoming } => { shift(result, amount); for (value, _) in incoming { shift(value, amount); } }
                InstructionKind::MakeDynFatPtr { result, data_ptr, vtable_ptr } => { shift(result, amount); shift(data_ptr, amount); shift(vtable_ptr, amount); }
                InstructionKind::LoadDynDataPtr { result, fat_ptr }
                | InstructionKind::LoadDynVtablePtr { result, fat_ptr } => { shift(result, amount); shift(fat_ptr, amount); }
                InstructionKind::LoadVtableSlot { result, vtable_ptr, .. } => { shift(result, amount); shift(vtable_ptr, amount); }
                InstructionKind::EscapeManualAlloc { ptr } => shift(ptr, amount),
                InstructionKind::AutodiffStep { result, output, upstream, inputs, targets, .. } => {
                    if let Some(value) = result { shift(value, amount); }
                    shift(output, amount);
                    if let Some(value) = upstream { shift(value, amount); }
                    for value in inputs.iter_mut().chain(targets.iter_mut()) { shift(value, amount); }
                }
                InstructionKind::FrameAlloc { result, .. } => shift(result, amount),
                // `frame` is an operand id wherever it appears: an unshifted frame handle
                // resolves to a stale value after renumbering.
                InstructionKind::FrameLoad { result, frame, .. }
                | InstructionKind::StateLoad { result, frame, .. }
                | InstructionKind::CoroutineCreate { result, frame, .. } => { shift(result, amount); shift(frame, amount); }
                InstructionKind::FrameStore { frame, value, .. } => { shift(frame, amount); shift(value, amount); }
                InstructionKind::StateStore { frame, .. } | InstructionKind::CoroutineWake { task: frame }
                | InstructionKind::CoroutineSuspend { task: frame, .. } | InstructionKind::CoroutineCancelled { task: frame } => shift(frame, amount),
                InstructionKind::CoroutinePollChild { status, result, task, .. } => { shift(status, amount); if let Some(value) = result { shift(value, amount); } shift(task, amount); }
                InstructionKind::CoroutineSubscribe { task, parent } => { shift(task, amount); shift(parent, amount); }
                InstructionKind::CoroutineComplete { task, value } | InstructionKind::CoroutineError { task, error: value } => { shift(task, amount); if let Some(value) = value { shift(value, amount); } }
                InstructionKind::CoroutinePollReturn { status } => shift(status, amount),
            }
        }
        if let Some(terminator) = &mut block.terminator {
            match terminator {
                Terminator::Return { value } => if let Some(value) = value { shift(value, amount); },
                Terminator::CondBranch { condition, .. } | Terminator::Switch { value: condition, .. } => shift(condition, amount),
                _ => {}
            }
        }
    }
    function.next_value_id += amount;
}
fn collect_value_types(function: &IRFunction) -> std::collections::HashMap<usize, IRType> {
    let mut types = std::collections::HashMap::new();
    for parameter in &function.params {
        types.insert(parameter.id, parameter.ty.clone());
    }
    for block in &function.blocks {
        for instruction in &block.instructions {
            match &instruction.kind {
                InstructionKind::ConstFloat { result, .. } | InstructionKind::ConstFloatTyped { result, .. } => { types.insert(result.id, IRType::Float); }
                InstructionKind::ConstBool { result, .. } => { types.insert(result.id, IRType::Bool); }
                InstructionKind::ConstString { result, .. } => { types.insert(result.id, IRType::String); }
                InstructionKind::Alloca { result, ty } => { types.insert(result.id, IRType::Pointer(Box::new(ty.clone()))); }
                InstructionKind::Load { result, ty, .. } | InstructionKind::FrameLoad { result, ty, .. } => { types.insert(result.id, ty.clone()); }
                InstructionKind::Cast { result, to_ty, .. } => { types.insert(result.id, to_ty.clone()); }
                InstructionKind::CoroutinePollChild { result, output_type, .. } => { if let Some(result) = result { types.insert(result.id, output_type.clone()); } }
                InstructionKind::Await { result, output_type, .. } => { types.insert(result.id, output_type.clone()); }
                InstructionKind::ConstInt { result, .. } | InstructionKind::ConstIntTyped { result, .. }
                | InstructionKind::Add { result, .. } | InstructionKind::Sub { result, .. }
                | InstructionKind::Mul { result, .. } | InstructionKind::Div { result, .. }
                | InstructionKind::Rem { result, .. } => { types.entry(result.id).or_insert(IRType::Int); }
                _ => {}
            }
        }
    }
    for block in &function.blocks {
        for instruction in &block.instructions {
            let (result, lhs, rhs) = match &instruction.kind {
                InstructionKind::Add { result, lhs, rhs }
                | InstructionKind::Sub { result, lhs, rhs }
                | InstructionKind::Mul { result, lhs, rhs }
                | InstructionKind::Div { result, lhs, rhs }
                | InstructionKind::Rem { result, lhs, rhs } => (result, lhs, rhs),
                _ => continue,
            };
            let ty = if matches!(types.get(&lhs.id), Some(IRType::Float))
                || matches!(types.get(&rhs.id), Some(IRType::Float))
            { IRType::Float } else { IRType::Int };
            types.insert(result.id, ty);
        }
    }
    types
}
