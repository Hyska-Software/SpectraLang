use std::collections::{HashMap, HashSet};

use crate::ir::{Instruction, InstructionKind, Module, Terminator, Value};

/// Performs structural verification of the IR and returns a list of problems if any were found.
pub fn verify_module(module: &Module) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let global_names: HashSet<&str> = module
        .globals
        .iter()
        .map(|global| global.name.as_str())
        .collect();
    let function_names: HashSet<&str> = module
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .chain(
            module
                .external_functions
                .iter()
                .map(|function| function.name.as_str()),
        )
        .collect();

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
        let mut defined_values: HashSet<usize> =
            function.params.iter().map(|param| param.id).collect();

        for block in &function.blocks {
            for instruction in &block.instructions {
                if let Some(result) = instruction_result(instruction) {
                    if !defined_values.insert(result.id) {
                        errors.push(format!(
                            "Function '{}' defines value {} more than once",
                            function.name, result.id
                        ));
                    }
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
                            check_value_defined(
                                &mut errors,
                                &function.name,
                                &block.label,
                                *value,
                                &defined_values,
                            );
                        }
                    }
                    Terminator::CondBranch {
                        condition,
                        true_block,
                        false_block,
                        ..
                    } => {
                        check_value_defined(
                            &mut errors,
                            &function.name,
                            &block.label,
                            *condition,
                            &defined_values,
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
                        check_value_defined(
                            &mut errors,
                            &function.name,
                            &block.label,
                            *value,
                            &defined_values,
                        );
                        if !block_ids.contains(default) {
                            errors.push(format!(
                                "Function '{}', block '{}' has switch with unknown default target {}",
                                function.name, block.label, default
                            ));
                        }
                        for (_, target) in cases {
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

            for instruction in &block.instructions {
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

                if let Some(unresolved) = instruction_unresolved_type(instruction) {
                    errors.push(format!(
                        "Function '{}', block '{}' contains unresolved IR type in {}",
                        function.name, block.label, unresolved
                    ));
                }

                for operand in instruction_operands(instruction) {
                    check_value_defined(
                        &mut errors,
                        &function.name,
                        &block.label,
                        operand,
                        &defined_values,
                    );
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
        InstructionKind::Cast { from_ty, to_ty, .. } => {
            (type_is_unresolved(from_ty) || type_is_unresolved(to_ty)).then(|| "cast".to_string())
        }
        _ => None,
    }
}

fn check_value_defined(
    errors: &mut Vec<String>,
    function_name: &str,
    block_label: &str,
    value: Value,
    defined_values: &HashSet<usize>,
) {
    if !defined_values.contains(&value.id) {
        errors.push(format!(
            "Function '{}', block '{}' uses undefined value {}",
            function_name, block_label, value.id
        ));
    }
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
        | InstructionKind::LoadVtableSlot { result, .. } => Some(*result),
        InstructionKind::AutodiffStep { result, .. } => *result,
        InstructionKind::Call { result, .. }
        | InstructionKind::HostCall { result, .. }
        | InstructionKind::CallIndirect { result, .. } => *result,
        InstructionKind::Store { .. }
        | InstructionKind::EscapeManualAlloc { .. } => None,
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
        InstructionKind::GetElementPtr { ptr, index, .. } => vec![*ptr, *index],
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
