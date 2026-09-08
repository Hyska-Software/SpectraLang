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
        let mut slots = source_params
            .iter()
            .enumerate()
            .map(|(id, p)| AsyncFrameSlot {
                id: id + 3,
                name: p.name.clone(),
                ty: p.ty.clone(),
            })
            .collect::<Vec<_>>();
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

        // Reserve stable slots for SSA values that cross a boundary. This is
        // intentionally conservative: an unused slot is harmless and makes
        // layout ids deterministic across branch/loop shapes.
        let mut seen_values = std::collections::BTreeSet::new();
        for block in &body.blocks {
            for instruction in &block.instructions {
                if let Some(value) = instruction_result(instruction) {
                    seen_values.insert(value.id);
                }
            }
        }
        for id in seen_values {
            if !slots.iter().any(|slot| slot.id == id) {
                slots.push(AsyncFrameSlot {
                    id,
                    name: format!("ssa{id}"),
                    ty: type_hints.get(&id).cloned().unwrap_or(IRType::Int),
                });
            }
        }

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
            let child_result = body.next_value();
            let ready_test = body.next_value();
            let mut pre = before;
            let mut cross_values = (0..source_params.len()).map(|id| id + 3).collect::<Vec<_>>();
            cross_values.extend(pre.iter().filter_map(instruction_result).map(|value| value.id));
            cross_values.extend(3..result.id);
            cross_values.push(child.id);
            cross_values.sort_unstable();
            cross_values.dedup();
            for value_id in cross_values.iter().copied() {
                pre.push(Instruction { id: pre.len(), kind: InstructionKind::FrameStore { frame, slot: value_id, value: Value { id: value_id } }, source_span: None });
            }
            pre.push(Instruction { id: pre.len(), kind: InstructionKind::CoroutinePollChild { status, result: Some(child_result), task: child, output_type: output_type.clone() }, source_span: None });
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
            let resumed_child = body.next_value();
            let mut continuation_instructions = cross_values.iter().enumerate().map(|(index, value_id)| Instruction {
                id: index,
                kind: InstructionKind::FrameLoad { result: Value { id: *value_id }, frame, slot: *value_id, ty: type_hints.get(value_id).cloned().unwrap_or(IRType::Int) },
                source_span: None,
            }).collect::<Vec<_>>();
            continuation_instructions.push(Instruction { id: continuation_instructions.len(), kind: InstructionKind::FrameLoad { result: resumed_child, frame, slot: child.id, ty: IRType::Int }, source_span: None });
            continuation_instructions.push(Instruction { id: continuation_instructions.len(), kind: InstructionKind::CoroutinePollChild { status: status2, result: Some(result), task: resumed_child, output_type: output_type.clone() }, source_span: None });
            continuation_instructions.push(Instruction { id: continuation_instructions.len(), kind: InstructionKind::ConstInt { result: ready_const2, value: POLL_READY }, source_span: None });
            continuation_instructions.push(Instruction { id: continuation_instructions.len(), kind: InstructionKind::Eq { result: ready_test2, lhs: status2, rhs: ready_const2 }, source_span: None });
            if let Some(block) = body.get_block_mut(continuation) {
                block.instructions = continuation_instructions;
                block.terminator = Some(Terminator::Switch { value: status2, cases: vec![(POLL_READY, ready_block), (POLL_FAILED, failed), (POLL_CANCELLED, cancelled)], default: pending });
            }
            if let Some(block) = body.get_block_mut(ready_block) {
                block.instructions = after;
                block.terminator = old.terminator;
            }

            let pending_child = body.next_value();
            let pending_status = body.next_value();
            if let Some(block) = body.get_block_mut(pending) {
                block.instructions = vec![Instruction {
                    id: 0,
                    kind: InstructionKind::FrameLoad { result: pending_child, frame, slot: child.id, ty: IRType::Int },
                    source_span: None,
                }];
                block.instructions.push(Instruction { id: block.instructions.len(), kind: InstructionKind::StateStore { frame, state: state_id }, source_span: None });
                block.instructions.push(Instruction { id: block.instructions.len(), kind: InstructionKind::CoroutineSubscribe { task: pending_child, parent: task }, source_span: None });
                block.instructions.push(Instruction { id: block.instructions.len(), kind: InstructionKind::CoroutineWake { task: pending_child }, source_span: None });
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
                | InstructionKind::GetElementPtr { result, ptr, .. }
                | InstructionKind::FieldPtr { result, ptr, .. } => { shift(result, amount); shift(ptr, amount); }
                InstructionKind::Store { ptr, value } => { shift(ptr, amount); shift(value, amount); }
                InstructionKind::Call { result, args, .. }
                | InstructionKind::HostCall { result, args, .. }
                | InstructionKind::CallIndirect { result, args, .. } => {
                    if let Some(value) = result { shift(value, amount); }
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
                | InstructionKind::FuncAddr { result, .. }
                | InstructionKind::AsyncReady { result, .. } => shift(result, amount),
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
                InstructionKind::FrameAlloc { result, .. } | InstructionKind::FrameLoad { result, .. }
                | InstructionKind::StateLoad { result, .. } | InstructionKind::CoroutineCreate { result, .. } => shift(result, amount),
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
