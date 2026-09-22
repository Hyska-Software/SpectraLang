use std::collections::{HashMap, HashSet};

use crate::ir::{Instruction, InstructionKind, Module, Terminator, Value};

/// Performs structural verification of the IR and returns a list of problems if any were found.
pub fn verify_module(module: &Module) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let mut global_names = HashSet::new();
    for global in &module.globals {
        if !global_names.insert(global.name.as_str()) {
            errors.push(format!(
                "Module contains duplicate global symbol '{}'",
                global.name
            ));
        }
    }

    let mut function_names = HashSet::new();
    for function in &module.functions {
        if !function_names.insert(function.name.as_str()) {
            errors.push(format!(
                "Module contains duplicate function symbol '{}'",
                function.name
            ));
        }
    }
    for function in &module.external_functions {
        if !function_names.insert(function.name.as_str()) {
            errors.push(format!(
                "Module contains duplicate function or external symbol '{}'",
                function.name
            ));
        }
    }

    let mut function_signatures: HashMap<String, (Vec<crate::ir::Type>, crate::ir::Type)> =
        HashMap::new();
    for function in &module.functions {
        function_signatures.insert(
            function.name.clone(),
            (function.params.iter().map(|parameter| parameter.ty.clone()).collect(), function.return_type.clone()),
        );
    }
    for function in &module.external_functions {
        function_signatures.insert(
            function.name.clone(),
            (function.params.clone(), function.return_type.clone()),
        );
    }

    for external in &module.external_functions {
        if type_contains_unknown(&external.return_type)
            || external.params.iter().any(type_contains_unknown)
        {
            errors.push(format!(
                "External function '{}' contains an unresolved IR type",
                external.name
            ));
        }
    }

    for global in &module.globals {
        if type_contains_unknown(&global.ty) {
            errors.push(format!(
                "Global '{}' contains an unresolved IR type",
                global.name
            ));
        }
    }

    for function in &module.functions {
        let mut unresolved_function_types = Vec::new();
        if type_contains_unknown(&function.return_type) {
            unresolved_function_types.push(format!("return ({:?})", function.return_type));
        }
        for param in &function.params {
            if type_contains_unknown(&param.ty) {
                unresolved_function_types
                    .push(format!("parameter '{}' ({:?})", param.name, param.ty));
            }
        }
        for local in &function.locals {
            if type_contains_unknown(&local.ty) {
                unresolved_function_types.push(format!("local '{}' ({:?})", local.name, local.ty));
            }
        }
        if !unresolved_function_types.is_empty() {
            errors.push(format!(
                "Function '{}' contains an unresolved IR type: {}",
                function.name,
                unresolved_function_types.join(", ")
            ));
        }

        if function.blocks.is_empty() {
            errors.push(format!(
                "Function '{}' has no basic blocks after lowering",
                function.name
            ));
            continue;
        }

        let block_ids: HashSet<usize> = function.blocks.iter().map(|block| block.id).collect();

        // Record actual CFG predecessors once so phi nodes can be checked
        // against real incoming edges rather than only against existing IDs.
        let mut predecessors: HashMap<usize, HashSet<usize>> = HashMap::new();
        for block in &function.blocks {
            let targets = block_successors(block.terminator.as_ref());
            for target in targets {
                predecessors.entry(target).or_default().insert(block.id);
            }
        }

        let entry_block = function.blocks[0].id;
        let reachable_blocks = reachable_blocks(function, &block_ids, entry_block);
        let dominators = compute_dominators(
            &block_ids,
            &predecessors,
            &reachable_blocks,
            entry_block,
        );

        // Keep every definition location.  A global set is sufficient to
        // detect a missing value, but it incorrectly accepts a use that occurs
        // before its definition or on a sibling CFG branch.  The location map
        // below is used for real SSA availability checks later in this pass.
        let mut definitions: HashMap<usize, Vec<ValueDefinition>> = HashMap::new();
        let mut frame_load_ids = HashSet::new();
        for parameter in &function.params {
            if parameter.id == Value::INVALID_ID {
                errors.push(format!(
                    "Function '{}' has invalid sentinel parameter value {}",
                    function.name,
                    Value::INVALID_ID
                ));
            }
            register_definition(
                &mut definitions,
                &mut errors,
                &function.name,
                "parameter",
                None,
                0,
                Value {
                    id: parameter.id,
                },
                false,
            );
        }

        for block in &function.blocks {
            for (instruction_index, instruction) in block.instructions.iter().enumerate() {
                if let Some(result) = instruction_result(instruction) {
                    let is_frame_load = matches!(instruction.kind, InstructionKind::FrameLoad { .. });
                    if is_frame_load {
                        frame_load_ids.insert(result.id);
                    }
                    register_definition(
                        &mut definitions,
                        &mut errors,
                        &function.name,
                        &block.label,
                        Some(block.id),
                        instruction_index,
                        result,
                        is_frame_load,
                    );
                }
                if let InstructionKind::CoroutinePollChild { status, .. } = &instruction.kind {
                    register_definition(
                        &mut definitions,
                        &mut errors,
                        &function.name,
                        &block.label,
                        Some(block.id),
                        instruction_index,
                        *status,
                        false,
                    );
                }
            }
        }

        if block_ids.len() != function.blocks.len() {
            errors.push(format!(
                "Function '{}' contains duplicated block identifiers",
                function.name
            ));
        }

        for block in &function.blocks {
            if block.terminator.is_none() {
                errors.push(format!(
                    "Function '{}', block '{}' is missing a terminator",
                    function.name, block.label
                ));
            }

            if let Some(term) = &block.terminator {
                match term {
                    Terminator::Branch { target } => {
                        if !block_ids.contains(target) {
                            errors.push(format!(
                                "Function '{}', block '{}' branches to unknown block id {}",
                                function.name, block.label, target
                            ));
                        }
                    }
                    Terminator::Return { value } => {
                        if let Some(value) = value {
                            check_value_available(
                                &mut errors,
                                &function.name,
                                &block.label,
                                *value,
                                block.id,
                                block.instructions.len(),
                                None,
                                &definitions,
                                &frame_load_ids,
                                &dominators,
                            );
                        }
                    }
                    Terminator::CondBranch {
                        condition,
                        true_block,
                        false_block,
                        ..
                    } => {
                        check_value_available(
                            &mut errors,
                            &function.name,
                            &block.label,
                            *condition,
                            block.id,
                            block.instructions.len(),
                            None,
                            &definitions,
                            &frame_load_ids,
                            &dominators,
                        );
                        if !block_ids.contains(true_block) {
                            errors.push(format!(
                                "Function '{}', block '{}' has conditional branch with unknown true target {}",
                                function.name, block.label, true_block
                            ));
                        }
                        if !block_ids.contains(false_block) {
                            errors.push(format!(
                                "Function '{}', block '{}' has conditional branch with unknown false target {}",
                                function.name, block.label, false_block
                            ));
                        }
                    }
                    Terminator::Switch {
                        value,
                        cases,
                        default,
                    } => {
                        check_value_available(
                            &mut errors,
                            &function.name,
                            &block.label,
                            *value,
                            block.id,
                            block.instructions.len(),
                            None,
                            &definitions,
                            &frame_load_ids,
                            &dominators,
                        );
                        if !block_ids.contains(default) {
                            errors.push(format!(
                                "Function '{}', block '{}' has switch with unknown default target {}",
                                function.name, block.label, default
                            ));
                        }
                        let mut case_values = HashSet::new();
                        for (case_value, target) in cases {
                            if !case_values.insert(*case_value) {
                                errors.push(format!(
                                    "Function '{}', block '{}' has duplicate switch case value {}",
                                    function.name, block.label, case_value
                                ));
                            }
                            if !block_ids.contains(target) {
                                errors.push(format!(
                                    "Function '{}', block '{}' has switch with unknown case target {}",
                                    function.name, block.label, target
                                ));
                            }
                        }
                    }
                    Terminator::Unreachable => {}
                }
            }

            for (instruction_index, instruction) in block.instructions.iter().enumerate() {
                match &instruction.kind {
                    InstructionKind::GlobalAddr { name, .. }
                        if !global_names.contains(name.as_str()) =>
                    {
                        errors.push(format!(
                            "Function '{}', block '{}' references unknown global '{}'",
                            function.name, block.label, name
                        ));
                    }
                    InstructionKind::Call {
                        function: callee, ..
                    } if !function_names.contains(callee.as_str()) => {
                        errors.push(format!(
                            "Function '{}', block '{}' calls unknown function '{}'",
                            function.name, block.label, callee
                        ));
                    }
                    InstructionKind::FuncAddr {
                        function: callee, ..
                    } if !function_names.contains(callee.as_str()) => {
                        errors.push(format!(
                            "Function '{}', block '{}' takes address of unknown function '{}'",
                            function.name, block.label, callee
                        ));
                    }
                    _ => {}
                }

                if let InstructionKind::Call {
                    function: callee,
                    args,
                    result,
                    ..
                } = &instruction.kind
                {
                    if let Some((parameters, return_type)) = function_signatures.get(callee) {
                        if args.len() != parameters.len() {
                            errors.push(format!(
                                "Function '{}', block '{}' calls '{}' with {} arguments, expected {}",
                                function.name,
                                block.label,
                                callee,
                                args.len(),
                                parameters.len()
                            ));
                        }
                        if *return_type == crate::ir::Type::Void && result.is_some() {
                            errors.push(format!(
                                "Function '{}', block '{}' records a result for void function call '{}'",
                                function.name, block.label, callee
                            ));
                        }
                    }
                }

                if let InstructionKind::CallIndirect {
                    args,
                    signature_params,
                    ..
                } = &instruction.kind
                {
                    if args.len() != signature_params.len() {
                        errors.push(format!(
                            "Function '{}', block '{}' performs an indirect call with {} arguments, expected {}",
                            function.name,
                            block.label,
                            args.len(),
                            signature_params.len()
                        ));
                    }
                }

                if let Some(unresolved) = instruction_unresolved_type(instruction) {
                    errors.push(format!(
                        "Function '{}', block '{}' contains unresolved IR type in {}",
                        function.name, block.label, unresolved
                    ));
                }

                if !matches!(instruction.kind, InstructionKind::Phi { .. }) {
                    for operand in instruction_operands(instruction) {
                        check_value_available(
                            &mut errors,
                            &function.name,
                            &block.label,
                            operand,
                            block.id,
                            instruction_index,
                            None,
                            &definitions,
                            &frame_load_ids,
                            &dominators,
                        );
                    }
                }

                if let InstructionKind::Phi {
                    result: _,
                    incoming,
                } = &instruction.kind
                {
                    if incoming.is_empty() {
                        errors.push(format!(
                            "Function '{}', block '{}' contains phi with no incoming edges",
                            function.name, block.label
                        ));
                    }

                    let mut seen = HashMap::new();
                    for (value, pred) in incoming {
                        if !block_ids.contains(pred) {
                            errors.push(format!(
                                "Function '{}', block '{}' contains phi referencing unknown predecessor block {}",
                                function.name, block.label, pred
                            ));
                        } else if !predecessors
                            .get(&block.id)
                            .is_some_and(|incoming| incoming.contains(pred))
                        {
                            errors.push(format!(
                                "Function '{}', block '{}' contains phi incoming from block {} which is not a predecessor",
                                function.name, block.label, pred
                            ));
                        } else if let Some(predecessor) = function.get_block(*pred) {
                            check_value_available(
                                &mut errors,
                                &function.name,
                                &block.label,
                                *value,
                                block.id,
                                predecessor.instructions.len(),
                                Some(*pred),
                                &definitions,
                                &frame_load_ids,
                                &dominators,
                            );
                        }

                        if let Some(existing) = seen.insert(*pred, *value) {
                            errors.push(format!(
                                "Function '{}', block '{}' contains phi with duplicate entries for predecessor block {} (values {} and {})",
                                function.name,
                                block.label,
                                pred,
                                existing.id,
                                value.id
                            ));
                        }
                    }
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn type_contains_unknown(ty: &crate::ir::Type) -> bool {
    use crate::ir::Type;

    match ty {
        Type::Unknown => true,
        Type::Pointer(inner) | Type::Task { output: inner } => type_contains_unknown(inner),
        Type::Array { element_type, .. } => type_contains_unknown(element_type),
        Type::Tuple { elements } => elements.iter().any(type_contains_unknown),
        Type::Struct { fields, .. } => fields.iter().any(|(_, field)| type_contains_unknown(field)),
        Type::Enum { variants, .. } => variants.iter().any(|(_, payload)| {
            payload
                .as_ref()
                .is_some_and(|types| types.iter().any(type_contains_unknown))
        }),
        Type::Generic {
            args,
            representation,
            ..
        } => args.iter().any(type_contains_unknown) || type_contains_unknown(representation),
        Type::Function {
            params,
            return_type,
        } => params.iter().any(type_contains_unknown) || type_contains_unknown(return_type),
        Type::Tensor { dtype, .. } => type_contains_unknown(dtype),
        Type::Void
        | Type::Int
        | Type::Float
        | Type::ExactInt { .. }
        | Type::ExactFloat { .. }
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::Range
        | Type::DynTrait { .. } => false,
    }
}

fn instruction_unresolved_type(instruction: &Instruction) -> Option<String> {
    use crate::ir::InstructionKind;

    let type_is_unresolved = |ty: &crate::ir::Type| type_contains_unknown(ty);
    match &instruction.kind {
        InstructionKind::Alloca { ty, .. }
        | InstructionKind::GlobalAddr { ty, .. }
        | InstructionKind::Load { ty, .. }
        | InstructionKind::GetElementPtr {
            element_type: ty, ..
        }
        | InstructionKind::ConstIntTyped { ty, .. }
        | InstructionKind::ConstFloatTyped { ty, .. } => {
            type_is_unresolved(ty).then(|| "memory or constant instruction".to_string())
        }
        InstructionKind::HostCall {
            host, result_type, ..
        } => result_type
            .as_ref()
            .filter(|ty| type_is_unresolved(ty))
            .map(|ty| format!("host call result '{}' ({ty:?})", host)),
        InstructionKind::CallIndirect {
            signature_params,
            signature_return,
            ..
        } => (signature_params.iter().any(type_is_unresolved)
            || type_is_unresolved(signature_return))
        .then(|| "indirect call signature".to_string()),
        InstructionKind::AsyncReady { output_type, .. } => {
            type_is_unresolved(output_type).then(|| "async result".to_string())
        }
        InstructionKind::Await { output_type, .. } => {
            type_is_unresolved(output_type).then(|| "await result".to_string())
        }
        InstructionKind::FrameLoad { ty, .. } => {
            type_is_unresolved(ty).then(|| "coroutine frame load".to_string())
        }
        InstructionKind::CoroutineCreate { output_type, .. } => {
            type_is_unresolved(output_type).then(|| "coroutine creation result".to_string())
        }
        InstructionKind::CoroutinePollChild { output_type, .. } => {
            type_is_unresolved(output_type).then(|| "child coroutine result".to_string())
        }
        InstructionKind::Cast { from_ty, to_ty, .. } => {
            (type_is_unresolved(from_ty) || type_is_unresolved(to_ty)).then(|| "cast".to_string())
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct ValueDefinition {
    block_id: Option<usize>,
    instruction_index: usize,
    is_frame_load: bool,
}

fn register_definition(
    definitions: &mut HashMap<usize, Vec<ValueDefinition>>,
    errors: &mut Vec<String>,
    function_name: &str,
    block_label: &str,
    block_id: Option<usize>,
    instruction_index: usize,
    value: Value,
    is_frame_load: bool,
) {
    if value.id == Value::INVALID_ID {
        errors.push(format!(
            "Function '{}', block '{}' contains invalid sentinel value {}",
            function_name,
            block_label,
            Value::INVALID_ID
        ));
    }

    let existing = definitions.entry(value.id).or_default();
    // Frame reloads are the one deliberate exception to the ordinary
    // single-definition rule. Async lowering may materialize the same logical
    // source value at multiple resume points; all ordinary SSA definitions must
    // still remain unique.
    if !existing.is_empty() && (!is_frame_load || existing.iter().any(|def| !def.is_frame_load)) {
        errors.push(format!(
            "Function '{}' defines value {} more than once",
            function_name, value.id
        ));
    }
    existing.push(ValueDefinition {
        block_id,
        instruction_index,
        is_frame_load,
    });
}

fn check_value_available(
    errors: &mut Vec<String>,
    function_name: &str,
    block_label: &str,
    value: Value,
    use_block: usize,
    use_index: usize,
    edge_predecessor: Option<usize>,
    definitions: &HashMap<usize, Vec<ValueDefinition>>,
    frame_load_ids: &HashSet<usize>,
    dominators: &HashMap<usize, HashSet<usize>>,
) {
    if value.id == Value::INVALID_ID {
        errors.push(format!(
            "Function '{}', block '{}' uses invalid sentinel value {}",
            function_name,
            block_label,
            Value::INVALID_ID
        ));
        return;
    }

    let Some(value_definitions) = definitions.get(&value.id) else {
        errors.push(format!(
            "Function '{}', block '{}' uses undefined value {}",
            function_name, block_label, value.id
        ));
        return;
    };

    // A frame reload is a task-local memory read rather than a conventional
    // SSA definition.  Its source slot is valid at every resume point, so the
    // generated ID may intentionally be encountered outside ordinary CFG
    // dominance. Fresh IDs are still used for all newly prepended reloads.
    if frame_load_ids.contains(&value.id)
        && value_definitions.iter().any(|definition| definition.is_frame_load)
    {
        return;
    }

    let context_block = edge_predecessor.unwrap_or(use_block);
    let available = value_definitions.iter().any(|definition| {
        let Some(definition_block) = definition.block_id else {
            // Function parameters dominate every block.
            return true;
        };

        if definition_block == context_block {
            definition.instruction_index < use_index
        } else {
            dominators
                .get(&context_block)
                .is_some_and(|dominated| dominated.contains(&definition_block))
        }
    });

    if !available {
        errors.push(format!(
            "Function '{}', block '{}' uses value {} before its definition or outside its defining CFG path",
            function_name, block_label, value.id
        ));
    }
}

fn block_successors(terminator: Option<&Terminator>) -> Vec<usize> {
    match terminator {
        Some(Terminator::Branch { target }) => vec![*target],
        Some(Terminator::CondBranch {
            true_block,
            false_block,
            ..
        }) => vec![*true_block, *false_block],
        Some(Terminator::Switch {
            cases, default, ..
        }) => cases
            .iter()
            .map(|(_, target)| *target)
            .chain(std::iter::once(*default))
            .collect(),
        Some(Terminator::Return { .. }) | Some(Terminator::Unreachable) | None => Vec::new(),
    }
}

fn reachable_blocks(
    function: &crate::ir::Function,
    block_ids: &HashSet<usize>,
    entry_block: usize,
) -> HashSet<usize> {
    let mut reachable = HashSet::new();
    let mut worklist = vec![entry_block];
    while let Some(block_id) = worklist.pop() {
        if !block_ids.contains(&block_id) || !reachable.insert(block_id) {
            continue;
        }
        if let Some(block) = function.get_block(block_id) {
            for successor in block_successors(block.terminator.as_ref()) {
                if block_ids.contains(&successor) {
                    worklist.push(successor);
                }
            }
        }
    }
    reachable
}

fn compute_dominators(
    block_ids: &HashSet<usize>,
    predecessors: &HashMap<usize, HashSet<usize>>,
    reachable: &HashSet<usize>,
    entry_block: usize,
) -> HashMap<usize, HashSet<usize>> {
    let mut dominators = HashMap::new();
    for block_id in block_ids {
        let initial = if *block_id == entry_block {
            HashSet::from([*block_id])
        } else if !reachable.contains(block_id) {
            // Unreachable blocks still get local ordering validation. Values
            // defined in the function entry are nevertheless in lexical scope
            // there (the lowering of an infinite loop may leave a dead exit
            // block containing stores that use entry allocas), so retain the
            // entry as a conservative dominator for those blocks.
            HashSet::from([entry_block, *block_id])
        } else {
            reachable.clone()
        };
        dominators.insert(*block_id, initial);
    }

    let mut changed = true;
    while changed {
        changed = false;
        for block_id in reachable {
            if *block_id == entry_block {
                continue;
            }

            let incoming = predecessors
                .get(block_id)
                .into_iter()
                .flat_map(|preds| preds.iter())
                .filter(|pred| reachable.contains(pred));
            let mut intersection = reachable.clone();
            let mut has_predecessor = false;
            for predecessor in incoming {
                has_predecessor = true;
                if let Some(predecessor_dominators) = dominators.get(predecessor) {
                    intersection.retain(|candidate| predecessor_dominators.contains(candidate));
                }
            }
            if !has_predecessor {
                intersection.clear();
            }
            intersection.insert(*block_id);

            if dominators.get(block_id) != Some(&intersection) {
                dominators.insert(*block_id, intersection);
                changed = true;
            }
        }
    }
    dominators
}

fn instruction_result(instruction: &Instruction) -> Option<Value> {
    match &instruction.kind {
        InstructionKind::Add { result, .. }
        | InstructionKind::Sub { result, .. }
        | InstructionKind::Mul { result, .. }
        | InstructionKind::Div { result, .. }
        | InstructionKind::Rem { result, .. }
        | InstructionKind::Eq { result, .. }
        | InstructionKind::Ne { result, .. }
        | InstructionKind::Lt { result, .. }
        | InstructionKind::Le { result, .. }
        | InstructionKind::Gt { result, .. }
        | InstructionKind::Ge { result, .. }
        | InstructionKind::And { result, .. }
        | InstructionKind::Or { result, .. }
        | InstructionKind::Not { result, .. }
        | InstructionKind::Alloca { result, .. }
        | InstructionKind::GlobalAddr { result, .. }
        | InstructionKind::ManualAlloc { result, .. }
        | InstructionKind::Load { result, .. }
        | InstructionKind::GetElementPtr { result, .. }
        | InstructionKind::FieldPtr { result, .. }
        | InstructionKind::FuncAddr { result, .. }
        | InstructionKind::AsyncReady { result, .. }
        | InstructionKind::Phi { result, .. }
        | InstructionKind::Copy { result, .. }
        | InstructionKind::ConstInt { result, .. }
        | InstructionKind::ConstIntTyped { result, .. }
        | InstructionKind::ConstFloat { result, .. }
        | InstructionKind::ConstFloatTyped { result, .. }
        | InstructionKind::ConstBool { result, .. }
        | InstructionKind::ConstString { result, .. }
        | InstructionKind::Cast { result, .. }
        | InstructionKind::MakeDynFatPtr { result, .. }
        | InstructionKind::LoadDynDataPtr { result, .. }
        | InstructionKind::LoadDynVtablePtr { result, .. }
        | InstructionKind::LoadVtableSlot { result, .. }
        | InstructionKind::FrameAlloc { result, .. }
        | InstructionKind::FrameLoad { result, .. }
        | InstructionKind::StateLoad { result, .. }
        | InstructionKind::CoroutineCreate { result, .. } => Some(*result),
        InstructionKind::AutodiffStep { result, .. }
        | InstructionKind::Call { result, .. }
        | InstructionKind::HostCall { result, .. }
        | InstructionKind::CallIndirect { result, .. } => result.as_ref().copied(),
        InstructionKind::CoroutinePollChild { result, .. } => *result,
        InstructionKind::Await { result, .. } => Some(*result),
        InstructionKind::Store { .. }
        | InstructionKind::EscapeManualAlloc { .. }
        | InstructionKind::FrameStore { .. }
        | InstructionKind::StateStore { .. }
        | InstructionKind::CoroutineSubscribe { .. }
        | InstructionKind::CoroutineWake { .. }
        | InstructionKind::CoroutineSuspend { .. }
        | InstructionKind::CoroutineComplete { .. }
        | InstructionKind::CoroutineError { .. }
        | InstructionKind::CoroutineCancelled { .. }
        | InstructionKind::CoroutinePollReturn { .. } => None,
    }
}

fn instruction_operands(instruction: &Instruction) -> Vec<Value> {
    match &instruction.kind {
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
        | InstructionKind::Or { lhs, rhs, .. } => vec![*lhs, *rhs],
        InstructionKind::Not { operand, .. }
        | InstructionKind::Load { ptr: operand, .. }
        | InstructionKind::Copy {
            source: operand, ..
        }
        | InstructionKind::Cast { operand, .. }
        | InstructionKind::LoadDynDataPtr {
            fat_ptr: operand, ..
        }
        | InstructionKind::LoadDynVtablePtr {
            fat_ptr: operand, ..
        }
        | InstructionKind::LoadVtableSlot {
            vtable_ptr: operand,
            ..
        }
        | InstructionKind::EscapeManualAlloc { ptr: operand, .. } => vec![*operand],
        InstructionKind::Store { ptr, value } => vec![*ptr, *value],
        InstructionKind::GetElementPtr {
            ptr, index, bound, ..
        } => {
            let mut operands = vec![*ptr, *index];
            if let Some(crate::ir::ArrayBound::Dynamic(value)) = bound {
                operands.push(*value);
            }
            operands
        }
        InstructionKind::FieldPtr { ptr, .. } => vec![*ptr],
        InstructionKind::Call { args, .. } | InstructionKind::HostCall { args, .. } => args.clone(),
        InstructionKind::CallIndirect { fn_ptr, args, .. } => {
            let mut operands = Vec::with_capacity(args.len() + 1);
            operands.push(*fn_ptr);
            operands.extend(args.iter().copied());
            operands
        }
        InstructionKind::AsyncReady { value, .. } => value.iter().copied().collect(),
        InstructionKind::Phi { incoming, .. } => incoming.iter().map(|(value, _)| *value).collect(),
        InstructionKind::MakeDynFatPtr {
            data_ptr,
            vtable_ptr,
            ..
        } => vec![*data_ptr, *vtable_ptr],
        InstructionKind::Await { task, .. } => vec![*task],
        InstructionKind::FrameAlloc { .. } | InstructionKind::StateLoad { .. } => Vec::new(),
        InstructionKind::FrameStore { frame, value, .. } => vec![*frame, *value],
        InstructionKind::FrameLoad { frame, .. } => vec![*frame],
        InstructionKind::StateStore { frame, .. } => vec![*frame],
        InstructionKind::CoroutineCreate { frame, .. } => vec![*frame],
        InstructionKind::CoroutinePollChild { task, .. } => vec![*task],
        InstructionKind::CoroutineSubscribe { task, parent } => vec![*task, *parent],
        InstructionKind::CoroutineWake { task }
        | InstructionKind::CoroutineSuspend { task, .. }
        | InstructionKind::CoroutineCancelled { task } => vec![*task],
        InstructionKind::CoroutineComplete { task, value } => {
            let mut operands = vec![*task];
            operands.extend(value.iter().copied());
            operands
        }
        InstructionKind::CoroutineError { task, error } => {
            let mut operands = vec![*task];
            operands.extend(error.iter().copied());
            operands
        }
        InstructionKind::CoroutinePollReturn { status } => vec![*status],
        InstructionKind::Alloca { .. }
        | InstructionKind::GlobalAddr { .. }
        | InstructionKind::ManualAlloc { .. }
        | InstructionKind::FuncAddr { .. }
        | InstructionKind::ConstInt { .. }
        | InstructionKind::ConstIntTyped { .. }
        | InstructionKind::ConstFloat { .. }
        | InstructionKind::ConstFloatTyped { .. }
        | InstructionKind::ConstBool { .. }
        | InstructionKind::ConstString { .. } => Vec::new(),
        InstructionKind::AutodiffStep {
            upstream,
            inputs,
            targets,
            ..
        } => {
            let mut operands = Vec::new();
            if let Some(value) = upstream {
                operands.push(*value);
            }
            operands.extend(inputs.iter().copied());
            operands.extend(targets.iter().copied());
            operands
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::IRBuilder;
    use crate::ir::{Function, Module as IRModule, Parameter, Terminator, Type, Value};

    #[test]
    fn detects_missing_terminator() {
        let mut module = IRModule::new("test");
        let mut function = Function::new(
            "foo",
            vec![Parameter {
                id: 0,
                name: "x".into(),
                ty: Type::Int,
            }],
            Type::Void,
        );

        let entry = function.add_block("entry");
        let mut builder = IRBuilder::new();
        builder.set_current_function(0);
        builder.set_current_block(entry);
        // Intentionally do not add terminator

        module.add_function(function);

        let result = verify_module(&module);
        assert!(result.is_err());
    }

    #[test]
    fn detects_unknown_branch_target() {
        let mut module = IRModule::new("test");
        let mut function = Function::new("foo", Vec::new(), Type::Void);
        let entry = function.add_block("entry");
        let other = function.add_block("other");

        let mut builder = IRBuilder::new();
        builder.set_current_function(0);
        builder.set_current_block(entry);
        builder.build_branch(&mut function, 42);
        if let Some(block) = function.get_block_mut(other) {
            block.set_terminator(Terminator::Unreachable);
        }

        module.add_function(function);

        let result = verify_module(&module);
        assert!(result.is_err());
    }

    #[test]
    fn allows_repeated_diagnostic_labels_with_unique_block_ids() {
        let mut module = IRModule::new("test");
        let mut function = Function::new("repeated_labels", Vec::new(), Type::Void);
        let first = function.add_block("if.then");
        let second = function.add_block("if.then");

        if let Some(block) = function.get_block_mut(first) {
            block.set_terminator(Terminator::Branch { target: second });
        }
        if let Some(block) = function.get_block_mut(second) {
            block.set_terminator(Terminator::Return { value: None });
        }
        module.add_function(function);

        verify_module(&module)
            .expect("diagnostic labels may repeat when block IDs remain unique");
    }

    #[test]
    fn detects_phi_with_duplicate_predecessor() {
        let mut module = IRModule::new("test");
        let mut function = Function::new("foo", Vec::new(), Type::Void);
        let entry = function.add_block("entry");
        let other = function.add_block("other");

        let mut builder = IRBuilder::new();
        builder.set_current_function(0);
        builder.set_current_block(entry);

        let incoming = vec![(Value { id: 0 }, other), (Value { id: 1 }, other)];
        builder.build_phi(&mut function, incoming);
        builder.build_return(&mut function, None);
        if let Some(block) = function.get_block_mut(other) {
            block.set_terminator(Terminator::Unreachable);
        }

        module.add_function(function);
        let result = verify_module(&module);
        assert!(result.is_err());
    }

    #[test]
    fn detects_undefined_instruction_operand() {
        let mut module = IRModule::new("test");
        let mut function = Function::new("foo", Vec::new(), Type::Void);
        let entry = function.add_block("entry");

        let mut builder = IRBuilder::new();
        builder.set_current_function(0);
        builder.set_current_block(entry);
        let slot = builder.build_alloca(&mut function, Type::Int);
        builder.build_store(&mut function, slot, Value { id: 13 });
        builder.build_return(&mut function, None);

        module.add_function(function);
        let result = verify_module(&module).expect_err("undefined value must fail verification");
        assert!(result
            .iter()
            .any(|error| error.contains("uses undefined value 13")));
    }

    #[test]
    fn rejects_unresolved_ir_types_before_backend() {
        let mut module = IRModule::new("test");
        let mut function = Function::new("unknown", Vec::new(), Type::Unknown);
        let entry = function.add_block("entry");

        let mut builder = IRBuilder::new();
        builder.set_current_function(0);
        builder.set_current_block(entry);
        builder.build_return(&mut function, None);
        module.add_function(function);

        let errors = verify_module(&module).expect_err("unknown IR type must be rejected");
        assert!(errors
            .iter()
            .any(|error| error.contains("contains an unresolved IR type")));
    }

    #[test]
    fn rejects_unknown_global_and_function_references() {
        let mut module = IRModule::new("test");
        let mut function = Function::new("main", Vec::new(), Type::Void);
        let entry = function.add_block("entry");

        let mut builder = IRBuilder::new();
        builder.set_current_function(0);
        builder.set_current_block(entry);
        builder.build_global_addr(&mut function, "MISSING".to_string(), Type::Int);
        builder.build_call(&mut function, "missing".to_string(), Vec::new(), false);
        builder.build_return(&mut function, None);
        module.add_function(function);

        let errors = verify_module(&module).expect_err("unknown symbols must fail verification");
        assert!(errors
            .iter()
            .any(|error| error.contains("references unknown global 'MISSING'")));
        assert!(errors
            .iter()
            .any(|error| error.contains("calls unknown function 'missing'")));
    }

    #[test]
    fn rejects_use_before_definition_in_the_same_block() {
        let mut module = IRModule::new("test");
        let mut function = Function::new("use_before_def", Vec::new(), Type::Void);
        let entry = function.add_block("entry");
        if let Some(block) = function.get_block_mut(entry) {
            block.instructions.push(crate::ir::Instruction {
                id: 0,
                kind: crate::ir::InstructionKind::Add {
                    result: Value { id: 0 },
                    lhs: Value { id: 1 },
                    rhs: Value { id: 1 },
                },
                source_span: None,
            });
            block.instructions.push(crate::ir::Instruction {
                id: 1,
                kind: crate::ir::InstructionKind::ConstInt {
                    result: Value { id: 1 },
                    value: 1,
                },
                source_span: None,
            });
            block.set_terminator(Terminator::Return { value: None });
        }
        module.add_function(function);

        let errors = verify_module(&module).expect_err("use-before-definition must fail");
        assert!(errors.iter().any(|error| {
            error.contains("uses value 1 before its definition")
        }));
    }

    #[test]
    fn rejects_value_defined_on_a_non_dominating_branch() {
        let mut module = IRModule::new("test");
        let mut function = Function::new(
            "non_dominating_use",
            vec![Parameter {
                id: 0,
                name: "condition".into(),
                ty: Type::Bool,
            }],
            Type::Void,
        );
        let entry = function.add_block("entry");
        let left = function.add_block("left");
        let right = function.add_block("right");
        let exit = function.add_block("exit");

        if let Some(block) = function.get_block_mut(entry) {
            block.set_terminator(Terminator::CondBranch {
                condition: Value { id: 0 },
                true_block: left,
                false_block: right,
            });
        }
        if let Some(block) = function.get_block_mut(left) {
            block.instructions.push(crate::ir::Instruction {
                id: 0,
                kind: crate::ir::InstructionKind::ConstInt {
                    result: Value { id: 1 },
                    value: 42,
                },
                source_span: None,
            });
            block.set_terminator(Terminator::Branch { target: exit });
        }
        if let Some(block) = function.get_block_mut(right) {
            block.set_terminator(Terminator::Branch { target: exit });
        }
        if let Some(block) = function.get_block_mut(exit) {
            block.set_terminator(Terminator::Return { value: Some(Value { id: 1 }) });
        }
        module.add_function(function);

        let errors = verify_module(&module).expect_err("non-dominating use must fail");
        assert!(errors.iter().any(|error| {
            error.contains("uses value 1 before its definition or outside its defining CFG path")
        }));
    }

    #[test]
    fn verifies_optional_coroutine_operands() {
        let mut module = IRModule::new("test");
        let mut function = Function::new("coroutine_operands", Vec::new(), Type::Void);
        let entry = function.add_block("entry");
        let task = if let Some(block) = function.get_block_mut(entry) {
            block.instructions.push(crate::ir::Instruction {
                id: 0,
                kind: crate::ir::InstructionKind::ConstInt {
                    result: Value { id: 0 },
                    value: 1,
                },
                source_span: None,
            });
            block.instructions.push(crate::ir::Instruction {
                id: 1,
                kind: crate::ir::InstructionKind::CoroutineComplete {
                    task: Value { id: 0 },
                    value: Some(Value { id: 99 }),
                },
                source_span: None,
            });
            block.set_terminator(Terminator::Return { value: None });
            Value { id: 0 }
        } else {
            unreachable!("the entry block was just created");
        };
        let _ = task;
        module.add_function(function);

        let errors = verify_module(&module).expect_err("missing coroutine payload must fail");
        assert!(errors
            .iter()
            .any(|error| error.contains("uses undefined value 99")));
    }

    #[test]
    fn rejects_direct_call_with_wrong_arity() {
        let mut module = IRModule::new("test");
        module.external_functions.push(crate::ir::ExternalFunction {
            name: "callee".into(),
            params: vec![Type::Int, Type::Int],
            return_type: Type::Void,
        });

        let mut function = Function::new("caller", Vec::new(), Type::Void);
        let entry = function.add_block("entry");
        let mut builder = IRBuilder::new();
        builder.set_current_block(entry);
        builder.build_call(&mut function, "callee".into(), Vec::new(), false);
        builder.build_return(&mut function, None);
        module.add_function(function);

        let errors = verify_module(&module).expect_err("wrong call arity must fail");
        assert!(errors.iter().any(|error| {
            error.contains("calls 'callee' with 0 arguments, expected 2")
        }));
    }

    #[test]
    fn passes_valid_module() {
        let mut module = IRModule::new("test");
        let mut function = Function::new("foo", Vec::new(), Type::Void);
        let entry = function.add_block("entry");
        let exit = function.add_block("exit");

        let mut builder = IRBuilder::new();
        builder.set_current_function(0);
        builder.set_current_block(entry);
        builder.build_branch(&mut function, exit);

        builder.set_current_block(exit);
        builder.build_return(&mut function, None);

        module.add_function(function);
        let result = verify_module(&module);
        assert!(result.is_ok());
    }
}
