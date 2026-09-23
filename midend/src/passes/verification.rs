use std::collections::{HashMap, HashSet};

use crate::ir::{Instruction, InstructionKind, Module, Terminator, Type, Value};

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

        // Best-effort static types for SSA values, used by the structural
        // type rules below. Values whose type cannot be derived are simply
        // absent from the map, and every type rule is skipped when a type is
        // unknown: the verifier only rejects a program when it can prove a
        // mismatch, never on a wrong type guess.
        let value_types = collect_value_types(&function, &function_signatures);

        // Object types for the Load/Store agreement rules: only addresses
        // whose pointee lowering positively records plus explicit
        // `Pointer(T)` parameters participate.
        //
        // Unwrapping depends on how the address was recorded:
        //  * `Alloca`/`GlobalAddr`/`GetElementPtr` carry the *object* type
        //    directly (even when the object is itself a pointer), so they
        //    are used as-is.
        //  * `Load`/`FrameLoad` record the type of the *loaded value*; when
        //    that loaded value is itself a `Pointer(T)` and is used as an
        //    address, its pointee is `T`.
        //  * A `Pointer(T)` parameter's value points at `T`.
        //
        // A *bare* (non-`Pointer`) parameter used as an address is an
        // out-parameter whose declared type describes the address word, not
        // the pointee — the synthesized agent-tool wrappers pass `Int`
        // out-slots that receive `String`s and work because every address
        // is one machine word — so bare parameters are deliberately
        // excluded from these rules.
        let mut address_types: HashMap<usize, Type> = HashMap::new();
        for parameter in &function.params {
            if let Type::Pointer(inner) = &parameter.ty {
                if !type_contains_unknown(inner) && **inner != Type::Void {
                    address_types.insert(parameter.id, inner.as_ref().clone());
                }
            }
        }
        for block in &function.blocks {
            for instruction in &block.instructions {
                let recorded = match &instruction.kind {
                    InstructionKind::Alloca { result, ty, .. }
                    | InstructionKind::GlobalAddr { result, ty, .. }
                    | InstructionKind::GetElementPtr {
                        result, element_type: ty, ..
                    } => Some((result, ty.clone(), false)),
                    InstructionKind::Load { result, ty, .. }
                    | InstructionKind::FrameLoad { result, ty, .. } => {
                        Some((result, ty.clone(), true))
                    }
                    _ => None,
                };
                if let Some((result, ty, loaded_value)) = recorded {
                    if ty == Type::Void || type_contains_unknown(&ty) {
                        continue;
                    }
                    let pointee = if loaded_value {
                        match &ty {
                            Type::Pointer(inner) => inner.as_ref().clone(),
                            other => other.clone(),
                        }
                    } else {
                        ty
                    };
                    address_types.insert(result.id, pointee);
                }
            }
        }

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
                        // Structural return rule: value presence and type must
                        // match the function's declared IR return type. An
                        // `Unknown` return type is already reported by the
                        // unresolved-type rules, so it is skipped here.
                        if function.return_type != Type::Unknown {
                            match (value, &function.return_type) {
                                (None, Type::Void) => {}
                                (None, return_type) => {
                                    errors.push(format!(
                                        "Function '{}' returns no value from block '{}' but its IR return type is {:?}",
                                        function.name, block.label, return_type
                                    ));
                                }
                                (Some(_), Type::Void) => {
                                    errors.push(format!(
                                        "Function '{}' returns a value from block '{}' but its IR return type is Void",
                                        function.name, block.label
                                    ));
                                }
                                (Some(value), return_type) => {
                                    if let Some(value_type) = value_types.get(&value.id) {
                                        if !value_types_compatible(value_type, return_type) {
                                            errors.push(format!(
                                                "Function '{}' returns value {} of type {:?} from block '{}' but its IR return type is {:?}",
                                                function.name,
                                                value.id,
                                                value_type,
                                                block.label,
                                                return_type
                                            ));
                                        }
                                    }
                                }
                            }
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
                        // The branch condition must be integer-backed on
                        // the machine (Bool is I8, match lowering emits
                        // Int conditions, brif accepts any integer
                        // register). Skipped when the condition's type
                        // cannot be derived; floats and other non-integer
                        // classes are rejected.
                        if let Some(condition_type) = value_types.get(&condition.id) {
                            if !is_branch_condition_like(condition_type) {
                                errors.push(format!(
                                    "Function '{}', block '{}' has a branch condition of type {:?}, expected a boolean-like value",
                                    function.name, block.label, condition_type
                                ));
                            }
                        }
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
                        // The switch scrutinee must have an integer-backed
                        // type (`Int`, `ExactInt`, or `Char`, which is an
                        // integer code point). Skipped when unknown.
                        if let Some(scrutinee_type) = value_types.get(&value.id) {
                            if !is_integer_like(scrutinee_type) {
                                errors.push(format!(
                                    "Function '{}', block '{}' switches on a value of type {:?}, expected an integer-backed type",
                                    function.name, block.label, scrutinee_type
                                ));
                            }
                        }
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
                        // Argument types must agree with the callee signature
                        // where both sides are known and meaningful. `Void`
                        // and unresolved parameter types are skipped.
                        for (index, (arg, parameter_type)) in
                            args.iter().zip(parameters.iter()).enumerate()
                        {
                            if *parameter_type == Type::Void
                                || type_contains_unknown(parameter_type)
                            {
                                continue;
                            }
                            if let Some(arg_type) = value_types.get(&arg.id) {
                                if !value_types_compatible(arg_type, parameter_type) {
                                    errors.push(format!(
                                        "Function '{}', block '{}' passes a value of type {:?} as argument {} of '{}', expected {:?}",
                                        function.name,
                                        block.label,
                                        arg_type,
                                        index,
                                        callee,
                                        parameter_type
                                    ));
                                }
                            }
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

                // Operand type agreement: the two operands of an arithmetic,
                // comparison, or logical operation must share one machine
                // class — with one exception: mixed-width floats, which the
                // backend's arithmetic emitter promotes to F64
                // (`promote_float_operands`). Lowering inserts `Cast` for
                // legal conversions (int width, signedness, char), so any
                // other difference here is an IR defect.
                if let Some((lhs, rhs)) = binary_operand_pair(&instruction.kind) {
                    if let (Some(lhs_type), Some(rhs_type)) =
                        (value_types.get(&lhs.id), value_types.get(&rhs.id))
                    {
                        if !binary_operand_types_compatible(lhs_type, rhs_type) {
                            errors.push(format!(
                                "Function '{}', block '{}' has {} operands with mismatched IR types ({:?} vs {:?})",
                                function.name,
                                block.label,
                                instruction_opcode(&instruction.kind),
                                lhs_type,
                                rhs_type
                            ));
                        }
                    }
                }

                // Load/store address agreement. Per `address_types` above,
                // only addresses whose pointee lowering positively records
                // (or explicit `Pointer(..)` parameters) participate; other
                // addresses — notably bare out-parameter words — are skipped.
                match &instruction.kind {
                    InstructionKind::Load { ptr, ty, .. } => {
                        if let Some(pointee) = address_types.get(&ptr.id) {
                            if !value_types_compatible(ty, pointee) {
                                errors.push(format!(
                                    "Function '{}', block '{}' loads type {:?} through an address of type {:?}",
                                    function.name, block.label, ty, pointee
                                ));
                            }
                        }
                    }
                    InstructionKind::Store { ptr, value } => {
                        if let (Some(pointee), Some(value_type)) =
                            (address_types.get(&ptr.id), value_types.get(&value.id))
                        {
                            if !value_types_compatible(value_type, pointee) {
                                errors.push(format!(
                                    "Function '{}', block '{}' stores a value of type {:?} into an address of type {:?}",
                                    function.name, block.label, value_type, pointee
                                ));
                            }
                        }
                    }
                    _ => {}
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

                    // Phi completeness: every actual predecessor of this block
                    // must contribute exactly one incoming entry. The loop
                    // above rejects unknown and duplicate predecessors; this
                    // closes the missing-entry direction.
                    if !incoming.is_empty() {
                        if let Some(block_predecessors) = predecessors.get(&block.id) {
                            for pred in block_predecessors {
                                if !incoming.iter().any(|(_, entry)| entry == pred) {
                                    errors.push(format!(
                                        "Function '{}', block '{}' has a phi without an incoming entry for predecessor block {}",
                                        function.name, block.label, pred
                                    ));
                                }
                            }
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
            output,
            upstream,
            inputs,
            targets,
            ..
        } => {
            let mut operands = Vec::with_capacity(inputs.len() + targets.len() + 2);
            // `output` is the forward value whose saved creator the reverse
            // kernel consumes, so it must be covered by the
            // availability/dominance checks like every other operand.
            operands.push(*output);
            if let Some(value) = upstream {
                operands.push(*value);
            }
            operands.extend(inputs.iter().copied());
            operands.extend(targets.iter().copied());
            operands
        }
    }
}

/// True for integer-backed IR types (the `Switch` scrutinee contract: enum
/// tags, code points and exact ints — not `Bool`, whose lowering goes
/// through `CondBranch`).
fn is_integer_like(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::ExactInt { .. } | Type::Char)
}

/// Machine-level integer classes accepted as a `CondBranch` condition.
/// The backend lowers `brif` against any integer register, and match
/// lowering routinely emits `Int`-typed conditions, so "boolean-like"
/// means integer-backed on the machine — only floating-point (or
/// non-integer) conditions are rejected.
fn is_branch_condition_like(ty: &Type) -> bool {
    matches!(
        machine_class(ty),
        Some(
            MachineClass::I8 | MachineClass::I16 | MachineClass::I32 | MachineClass::I64
        )
    )
}

/// Machine-level representation classes. This MUST mirror
/// `backend/src/codegen_strings.rs::ir_type_to_cranelift` exactly: the
/// verifier's type rules catch what Cranelift would reject (or silently
/// miscompile), and Cranelift only ever sees these classes — nominal IR
/// types beyond them are metadata.
///
/// * `Void`/`Bool` are `I8`.
/// * Everything the backend represents as a pointer or discriminant word —
///   `Int`, `String`, `Pointer`, `Array`, `Tuple`, `Struct`, `Enum`,
///   `Function` (closure object), `Tensor` (handle), `Task`, `Range`,
///   `DynTrait` (fat-pointer address) — is `I64`. This is what makes the
///   real lowering shapes legal: closures passed where `Function` is
///   declared, frame reloads typed `Int` for `DynTrait` parameters,
///   `Pointer(T)` receivers vs by-value `T` parameters, boxed aggregates.
/// * `ExactInt` maps by width; `Char` is `I32`; floats map by width.
/// * `Generic` recursively maps through its `representation`.
///
/// Returns `None` only for `Unknown` (never compared) and malformed
/// self-referential `Generic` types.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MachineClass {
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

fn machine_class(ty: &Type) -> Option<MachineClass> {
    match ty {
        Type::Unknown => None,
        Type::Void | Type::Bool => Some(MachineClass::I8),
        Type::Int
        | Type::String
        | Type::Pointer(_)
        | Type::Array { .. }
        | Type::Tuple { .. }
        | Type::Struct { .. }
        | Type::Enum { .. }
        | Type::Function { .. }
        | Type::Tensor { .. }
        | Type::Task { .. }
        | Type::Range
        | Type::DynTrait { .. } => Some(MachineClass::I64),
        Type::ExactInt { width, .. } => Some(match width {
            crate::ir::IntWidth::I8 => MachineClass::I8,
            crate::ir::IntWidth::I16 => MachineClass::I16,
            crate::ir::IntWidth::I32 => MachineClass::I32,
            crate::ir::IntWidth::I64
            | crate::ir::IntWidth::Isize
            | crate::ir::IntWidth::Usize => MachineClass::I64,
        }),
        Type::Char => Some(MachineClass::I32),
        Type::Float => Some(MachineClass::F64),
        Type::ExactFloat { width } => Some(match width {
            crate::ir::FloatWidth::F32 => MachineClass::F32,
            crate::ir::FloatWidth::F64 => MachineClass::F64,
        }),
        Type::Generic {
            representation, ..
        } => {
            if **representation == *ty {
                None
            } else {
                machine_class(representation)
            }
        }
    }
}

/// Types the verifier accepts as interchangeable for a returned, stored, or
/// passed value: identical nominal types, or the **same machine
/// representation class** (see [`machine_class`], which mirrors
/// `ir_type_to_cranelift`).
///
/// The IR is deliberately word-level: closures are heap arrays, structs,
/// tuples, enums and ranges are boxed pointers, tensors are handles, and the
/// async lowering reloads frame slots as `Int` words. Nominal type
/// differences inside one machine class are therefore metadata, not errors —
/// demanding nominal equality rejected dozens of valid lowering shapes
/// (closure object as `Function`, `Pointer(T)` vs `T` receivers,
/// tag-tuple enum payloads, `Tensor` refinements, `Int`-typed match branch
/// conditions). Genuine machine mismatches still fail loudly: `I8` vs `I16`
/// narrow-int arguments, `I64` vs `F64`, float-vs-integer calls and
/// returns — exactly the mismatches that reach Cranelift's verifier as
/// opaque `error[codegen]` failures without a source span.
///
/// `Unknown` types never reach this function (callers skip when the type
/// cannot be derived).
fn value_types_compatible(lhs: &Type, rhs: &Type) -> bool {
    if lhs == rhs {
        return true;
    }
    match (machine_class(lhs), machine_class(rhs)) {
        (Some(lhs_class), Some(rhs_class)) => lhs_class == rhs_class,
        _ => false,
    }
}

/// True when both operands are floats of any widths. The backend's
/// arithmetic emitter promotes `F32` to `F64` whenever the other operand
/// is `F64` (`promote_float_operands`), so mixed-width float arithmetic,
/// comparison and equality are supported shapes — an `f32` frame slot
/// multiplied by a `Float` literal is real lowering output. Only the
/// *binary* rules use this; calls, returns and memory stay width-strict.
fn float_widths_mixable(lhs: &Type, rhs: &Type) -> bool {
    let is_float_class = |ty: &Type| matches!(machine_class(ty), Some(MachineClass::F32 | MachineClass::F64));
    is_float_class(lhs) && is_float_class(rhs)
}

/// Binary operand agreement: one IR type, or any mix of float widths
/// (the backend promotes to `F64`, see [`float_widths_mixable`]).
fn binary_operand_types_compatible(lhs: &Type, rhs: &Type) -> bool {
    value_types_compatible(lhs, rhs) || float_widths_mixable(lhs, rhs)
}

/// `(lhs, rhs)` for the binary arithmetic/comparison/logical opcodes whose
/// two operands must share one IR type.
fn binary_operand_pair(kind: &InstructionKind) -> Option<(Value, Value)> {
    match kind {
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
        | InstructionKind::Or { lhs, rhs, .. } => Some((*lhs, *rhs)),
        _ => None,
    }
}

/// Short opcode name used in the operand-type diagnostics.
fn instruction_opcode(kind: &InstructionKind) -> &'static str {
    match kind {
        InstructionKind::Add { .. } => "Add",
        InstructionKind::Sub { .. } => "Sub",
        InstructionKind::Mul { .. } => "Mul",
        InstructionKind::Div { .. } => "Div",
        InstructionKind::Rem { .. } => "Rem",
        InstructionKind::Eq { .. } => "Eq",
        InstructionKind::Ne { .. } => "Ne",
        InstructionKind::Lt { .. } => "Lt",
        InstructionKind::Le { .. } => "Le",
        InstructionKind::Gt { .. } => "Gt",
        InstructionKind::Ge { .. } => "Ge",
        InstructionKind::And { .. } => "And",
        InstructionKind::Or { .. } => "Or",
        _ => "binary",
    }
}

/// Prefer the more specific of two compatible types: an untyped `Int`
/// combined with an exact-width integer takes the exact width.
fn refine_numeric_type(lhs: &Type, rhs: &Type) -> Type {
    if lhs == rhs {
        lhs.clone()
    } else if matches!(lhs, Type::Int) {
        rhs.clone()
    } else {
        lhs.clone()
    }
}

/// Object type of an address-producing instruction whose pointee lowering
/// positively records: allocas, globals, loads, frame slots, and element
/// GEPs all carry the type of the object they designate. Explicit
/// `Pointer(..)` parameters are handled separately by the caller.
fn address_object_type(kind: &InstructionKind) -> Option<(Value, Type)> {
    match kind {
        InstructionKind::Alloca { result, ty, .. }
        | InstructionKind::GlobalAddr { result, ty, .. }
        | InstructionKind::Load { result, ty, .. }
        | InstructionKind::FrameLoad { result, ty, .. }
        | InstructionKind::GetElementPtr {
            result, element_type: ty, ..
        } => Some((*result, ty.clone())),
        _ => None,
    }
}

/// Types that follow from the instruction itself without consulting other
/// values. Returns `None` for kinds whose result type depends on operands
/// (`Copy`, `Phi`, arithmetic) or that stay opaque to this verifier
/// (function/heap handles, coroutine frames, fat pointers, autodiff nodes).
fn definite_value_type(kind: &InstructionKind) -> Option<(Value, Type)> {
    // Address-valued instructions carry the type of the object they
    // designate rather than an explicit `Pointer(..)`: this IR passes
    // aggregate addresses as the values themselves ("structs are
    // pointers") and `Load` re-declares the loaded type, so the object
    // type is what keeps the Load/Store rules aligned with lowering.
    if let Some(object) = address_object_type(kind) {
        return Some(object);
    }
    match kind {
        InstructionKind::ConstInt { result, .. } => Some((*result, Type::Int)),
        InstructionKind::ConstIntTyped { result, ty, .. } => Some((*result, ty.clone())),
        InstructionKind::ConstFloat { result, .. } => Some((*result, Type::Float)),
        InstructionKind::ConstFloatTyped { result, ty, .. } => Some((*result, ty.clone())),
        InstructionKind::ConstBool { result, .. } => Some((*result, Type::Bool)),
        InstructionKind::ConstString { result, .. } => Some((*result, Type::String)),
        InstructionKind::Cast { result, to_ty, .. } => Some((*result, to_ty.clone())),
        InstructionKind::Eq { result, .. }
        | InstructionKind::Ne { result, .. }
        | InstructionKind::Lt { result, .. }
        | InstructionKind::Le { result, .. }
        | InstructionKind::Gt { result, .. }
        | InstructionKind::Ge { result, .. } => Some((*result, Type::Bool)),
        InstructionKind::HostCall {
            result: Some(result),
            result_type: Some(ty),
            ..
        } => Some((*result, ty.clone())),
        _ => None,
    }
}

/// Best-effort map from SSA value id to IR type, used by the structural type
/// rules. Phase one records types fixed by the definition itself; phase two
/// propagates operand-dependent result types (copy, phi, arithmetic, logical
/// not) to a fixed point. The map only grows, so the loop terminates.
fn collect_value_types(
    function: &crate::ir::Function,
    function_signatures: &HashMap<String, (Vec<Type>, Type)>,
) -> HashMap<usize, Type> {
    let mut types: HashMap<usize, Type> = HashMap::new();
    let usable = |ty: &Type| *ty != Type::Void && !type_contains_unknown(ty);

    for parameter in &function.params {
        if usable(&parameter.ty) {
            types.insert(parameter.id, parameter.ty.clone());
        }
    }

    for block in &function.blocks {
        for instruction in &block.instructions {
            if let Some((result, ty)) = definite_value_type(&instruction.kind) {
                if usable(&ty) {
                    types.insert(result.id, ty);
                }
                continue;
            }
            match &instruction.kind {
                InstructionKind::Call {
                    result: Some(result),
                    function: callee,
                    ..
                } => {
                    if let Some((_, return_type)) = function_signatures.get(callee) {
                        if usable(return_type) {
                            types.insert(result.id, return_type.clone());
                        }
                    }
                }
                InstructionKind::CallIndirect {
                    result: Some(result),
                    signature_return,
                    ..
                } => {
                    if usable(signature_return) {
                        types.insert(result.id, signature_return.as_ref().clone());
                    }
                }
                _ => {}
            }
        }
    }

    loop {
        let mut learned = 0usize;
        for block in &function.blocks {
            for instruction in &block.instructions {
                let (result, ty) = match &instruction.kind {
                    InstructionKind::Copy { result, source } => {
                        let Some(source_type) = types.get(&source.id) else {
                            continue;
                        };
                        (*result, source_type.clone())
                    }
                    InstructionKind::Phi { result, incoming } => {
                        let mut picked: Option<Type> = None;
                        let mut consistent = true;
                        for (value, _) in incoming {
                            let Some(value_type) = types.get(&value.id) else {
                                consistent = false;
                                break;
                            };
                            match &picked {
                                None => picked = Some(value_type.clone()),
                                Some(chosen) if value_types_compatible(chosen, value_type) => {
                                    picked = Some(refine_numeric_type(chosen, value_type));
                                }
                                Some(_) => {
                                    consistent = false;
                                    break;
                                }
                            }
                        }
                        if !consistent {
                            continue;
                        }
                        let Some(picked) = picked else {
                            continue;
                        };
                        (*result, picked)
                    }
                    InstructionKind::Add { result, lhs, rhs, .. }
                    | InstructionKind::Sub { result, lhs, rhs, .. }
                    | InstructionKind::Mul { result, lhs, rhs, .. }
                    | InstructionKind::Div { result, lhs, rhs, .. }
                    | InstructionKind::Rem { result, lhs, rhs, .. }
                    | InstructionKind::And { result, lhs, rhs }
                    | InstructionKind::Or { result, lhs, rhs } => {
                        let (Some(lhs_type), Some(rhs_type)) =
                            (types.get(&lhs.id), types.get(&rhs.id))
                        else {
                            continue;
                        };
                        if !binary_operand_types_compatible(lhs_type, rhs_type) {
                            continue;
                        }
                        // Mixed float widths evaluate at the promoted
                        // width: the backend promotes F32 to F64.
                        let refined = if float_widths_mixable(lhs_type, rhs_type)
                            && machine_class(lhs_type) != machine_class(rhs_type)
                        {
                            Type::Float
                        } else {
                            refine_numeric_type(lhs_type, rhs_type)
                        };
                        (*result, refined)
                    }
                    InstructionKind::Not { result, operand } => {
                        let Some(operand_type) = types.get(&operand.id) else {
                            continue;
                        };
                        (*result, operand_type.clone())
                    }
                    _ => continue,
                };
                if usable(&ty) && !types.contains_key(&result.id) {
                    types.insert(result.id, ty);
                    learned += 1;
                }
            }
        }
        if learned == 0 {
            break;
        }
    }

    types
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
    fn rejects_return_value_type_mismatch() {
        let mut module = IRModule::new("test");
        let mut function = Function::new("returns_int", Vec::new(), Type::Int);
        let entry = function.add_block("entry");
        let mut builder = IRBuilder::new();
        builder.set_current_block(entry);
        let truth = builder.build_const_bool(&mut function, true);
        builder.build_return(&mut function, Some(truth));
        module.add_function(function);

        let errors = verify_module(&module)
            .expect_err("bool value returned from an int function must fail");
        assert!(errors
            .iter()
            .any(|error| error.contains("but its IR return type is Int")));
    }

    #[test]
    fn rejects_missing_return_value_in_non_void_function() {
        let mut module = IRModule::new("test");
        let mut function = Function::new("needs_int", Vec::new(), Type::Int);
        let entry = function.add_block("entry");
        let mut builder = IRBuilder::new();
        builder.set_current_block(entry);
        builder.build_return(&mut function, None);
        module.add_function(function);

        let errors = verify_module(&module)
            .expect_err("a non-void function must return a value");
        assert!(errors
            .iter()
            .any(|error| error.contains("returns no value from block")));
    }

    #[test]
    fn rejects_value_returned_from_void_function() {
        let mut module = IRModule::new("test");
        let mut function = Function::new("returns_nothing", Vec::new(), Type::Void);
        let entry = function.add_block("entry");
        let mut builder = IRBuilder::new();
        builder.set_current_block(entry);
        let value = builder.build_const_int(&mut function, 1);
        builder.build_return(&mut function, Some(value));
        module.add_function(function);

        let errors = verify_module(&module)
            .expect_err("a void function must not return a value");
        assert!(errors
            .iter()
            .any(|error| error.contains("but its IR return type is Void")));
    }

    #[test]
    fn rejects_phi_missing_predecessor_entry() {
        let mut module = IRModule::new("test");
        let mut function = Function::new(
            "merge",
            vec![
                Parameter {
                    id: 0,
                    name: "condition".into(),
                    ty: Type::Bool,
                },
                Parameter {
                    id: 1,
                    name: "left_value".into(),
                    ty: Type::Int,
                },
                Parameter {
                    id: 2,
                    name: "right_value".into(),
                    ty: Type::Int,
                },
            ],
            Type::Void,
        );
        let entry = function.add_block("entry");
        let left = function.add_block("left");
        let right = function.add_block("right");
        let join = function.add_block("join");

        let mut builder = IRBuilder::new();
        builder.set_current_block(entry);
        builder.build_cond_branch(&mut function, Value { id: 0 }, left, right);
        builder.set_current_block(left);
        builder.build_branch(&mut function, join);
        builder.set_current_block(right);
        builder.build_branch(&mut function, join);
        builder.set_current_block(join);
        // The phi only carries an entry for `left`, but `right` also branches
        // into `join`: the phi is incomplete.
        builder.build_phi(&mut function, vec![(Value { id: 1 }, left)]);
        builder.build_return(&mut function, None);
        module.add_function(function);

        let errors = verify_module(&module)
            .expect_err("phi must cover every predecessor of its block");
        assert!(errors.iter().any(|error| {
            error.contains("phi without an incoming entry for predecessor block")
        }));
    }

    #[test]
    fn rejects_mismatched_binary_operand_types() {
        let mut module = IRModule::new("test");
        let mut function = Function::new("mismatch", Vec::new(), Type::Void);
        let entry = function.add_block("entry");
        if let Some(block) = function.get_block_mut(entry) {
            block.instructions.push(crate::ir::Instruction {
                id: 0,
                kind: InstructionKind::ConstInt {
                    result: Value { id: 0 },
                    value: 1,
                },
                source_span: None,
            });
            block.instructions.push(crate::ir::Instruction {
                id: 1,
                kind: InstructionKind::ConstBool {
                    result: Value { id: 1 },
                    value: true,
                },
                source_span: None,
            });
            block.instructions.push(crate::ir::Instruction {
                id: 2,
                kind: InstructionKind::Add {
                    result: Value { id: 2 },
                    lhs: Value { id: 0 },
                    rhs: Value { id: 1 },
                },
                source_span: None,
            });
            block.set_terminator(Terminator::Return { value: None });
        }
        module.add_function(function);

        let errors = verify_module(&module)
            .expect_err("Int + Bool operands must fail verification");
        assert!(errors
            .iter()
            .any(|error| error.contains("Add operands with mismatched IR types")));
    }

    #[test]
    fn rejects_non_boolean_branch_condition() {
        // Integer-backed conditions are legal (match lowering emits Int
        // conditions and brif accepts any integer register); a float
        // condition would reach Cranelift's verifier as an opaque failure.
        let mut module = IRModule::new("test");
        let mut function = Function::new("float_condition", Vec::new(), Type::Void);
        let entry = function.add_block("entry");
        let other = function.add_block("other");

        let mut builder = IRBuilder::new();
        builder.set_current_block(entry);
        let condition = builder.build_const_float(&mut function, 1.0);
        builder.build_cond_branch(&mut function, condition, other, other);
        builder.set_current_block(other);
        builder.build_return(&mut function, None);
        module.add_function(function);

        let errors = verify_module(&module)
            .expect_err("a floating-point branch condition must fail verification");
        assert!(errors
            .iter()
            .any(|error| error.contains("expected a boolean-like value")));
    }

    #[test]
    fn integer_branch_conditions_are_legal() {
        // Match lowering emits Int-typed conditions; they are I64 on the
        // machine and brif accepts them, so the verifier must not reject
        // the house lowering style.
        let mut module = IRModule::new("test");
        let mut function = Function::new("int_condition", Vec::new(), Type::Void);
        let entry = function.add_block("entry");
        let other = function.add_block("other");

        let mut builder = IRBuilder::new();
        builder.set_current_block(entry);
        let condition = builder.build_const_int(&mut function, 1);
        builder.build_cond_branch(&mut function, condition, other, other);
        builder.set_current_block(other);
        builder.build_return(&mut function, None);
        module.add_function(function);

        assert!(
            verify_module(&module).is_ok(),
            "an Int branch condition is legal IR: {:?}",
            verify_module(&module)
        );
    }

    #[test]
    fn mixed_width_float_operands_are_legal() {
        // The backend's arithmetic emitter promotes F32 to F64 when the
        // other operand is F64, so `f32 slot * Float literal` (real async
        // f32 lowering output) must not be rejected.
        let mut module = IRModule::new("test");
        let mut function = Function::new("mixed_floats", Vec::new(), Type::Void);
        let entry = function.add_block("entry");
        let mut builder = IRBuilder::new();
        builder.set_current_block(entry);
        let narrow = builder.build_const_float_typed(
            &mut function,
            1.5,
            Type::ExactFloat {
                width: crate::ir::FloatWidth::F32,
            },
        );
        let wide = builder.build_const_float(&mut function, 2.0);
        builder.build_mul(&mut function, narrow, wide);
        builder.build_return(&mut function, None);
        module.add_function(function);

        assert!(
            verify_module(&module).is_ok(),
            "F32 x F64 multiplication is promoted by the backend: {:?}",
            verify_module(&module)
        );
    }

    #[test]
    fn rejects_switch_on_non_integer_scrutinee() {
        let mut module = IRModule::new("test");
        let mut function = Function::new("bool_switch", Vec::new(), Type::Void);
        let entry = function.add_block("entry");
        let other = function.add_block("other");

        let mut builder = IRBuilder::new();
        builder.set_current_block(entry);
        let scrutinee = builder.build_const_bool(&mut function, true);
        if let Some(block) = function.get_block_mut(entry) {
            block.set_terminator(Terminator::Switch {
                value: scrutinee,
                cases: vec![(1, other)],
                default: other,
            });
        }
        builder.set_current_block(other);
        builder.build_return(&mut function, None);
        module.add_function(function);

        let errors = verify_module(&module)
            .expect_err("switching on a bool must fail verification");
        assert!(errors
            .iter()
            .any(|error| error.contains("expected an integer-backed type")));
    }

    #[test]
    fn rejects_call_argument_type_mismatch() {
        let mut module = IRModule::new("test");
        module.external_functions.push(crate::ir::ExternalFunction {
            name: "sink".into(),
            params: vec![Type::Float],
            return_type: Type::Void,
        });

        let mut function = Function::new("caller", Vec::new(), Type::Void);
        let entry = function.add_block("entry");
        let mut builder = IRBuilder::new();
        builder.set_current_block(entry);
        let argument = builder.build_const_int(&mut function, 1);
        builder.build_call(&mut function, "sink".into(), vec![argument], false);
        builder.build_return(&mut function, None);
        module.add_function(function);

        let errors = verify_module(&module)
            .expect_err("int argument to a float parameter must fail");
        assert!(errors
            .iter()
            .any(|error| error.contains("as argument 0 of 'sink', expected Float")));
    }

    #[test]
    fn rejects_load_and_store_type_mismatch() {
        let mut module = IRModule::new("test");
        let mut function = Function::new("memory_types", Vec::new(), Type::Void);
        let entry = function.add_block("entry");
        let mut builder = IRBuilder::new();
        builder.set_current_block(entry);
        let slot = builder.build_alloca(&mut function, Type::Int);
        let truth = builder.build_const_bool(&mut function, true);
        builder.build_store(&mut function, slot, truth);
        builder.build_load_typed(&mut function, slot, Type::Float);
        builder.build_return(&mut function, None);
        module.add_function(function);

        let errors = verify_module(&module)
            .expect_err("mismatched load/store types must fail verification");
        assert!(errors.iter().any(|error| {
            error.contains("stores a value of type Bool into an address of type Int")
        }));
        assert!(errors.iter().any(|error| {
            error.contains("loads type Float through an address of type Int")
        }));
    }

    #[test]
    fn rejects_autodiff_step_with_undefined_output() {
        // `AutodiffStep.output` is an operand: it must be covered by the
        // availability check, not only `upstream`/`inputs`/`targets`.
        let mut module = IRModule::new("test");
        let mut function = Function::new("autodiff_operand", Vec::new(), Type::Void);
        let entry = function.add_block("entry");
        if let Some(block) = function.get_block_mut(entry) {
            block.instructions.push(crate::ir::Instruction {
                id: 0,
                kind: InstructionKind::AutodiffStep {
                    result: None,
                    operation: "add".to_string(),
                    output: Value { id: 42 },
                    upstream: None,
                    inputs: Vec::new(),
                    targets: Vec::new(),
                },
                source_span: None,
            });
            block.set_terminator(Terminator::Return { value: None });
        }
        module.add_function(function);

        let errors = verify_module(&module)
            .expect_err("undefined AutodiffStep output must fail verification");
        assert!(errors
            .iter()
            .any(|error| error.contains("uses undefined value 42")));
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
