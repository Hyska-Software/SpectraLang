use std::fmt::Write;

use super::{BasicBlock, Function, InstructionKind, Module, Terminator, Type, Value};

/// Render the provided IR module into a developer-friendly string.
pub fn format_module(module: &Module) -> String {
    let mut output = String::new();

    writeln!(&mut output, "module {} {{", module.name).unwrap();

    if !module.globals.is_empty() {
        writeln!(&mut output, "  globals:").unwrap();
        for global in &module.globals {
            let mut line = String::new();
            write!(&mut line, "    {} {}", fmt_type(&global.ty), global.name).unwrap();
            if let Some(init) = &global.initializer {
                write!(&mut line, " = {:?}", init).unwrap();
            }
            if global.is_mutable {
                line.push_str(" [mutable]");
            }
            writeln!(&mut output, "{}", line).unwrap();
        }
    }

    for function in &module.functions {
        writeln!(&mut output).unwrap();
        format_function(&mut output, function).unwrap();
    }

    writeln!(&mut output, "}}").unwrap();

    output
}

fn format_function(output: &mut String, function: &Function) -> std::fmt::Result {
    let params = function
        .params
        .iter()
        .map(|p| format!("{} {}", fmt_type(&p.ty), p.name))
        .collect::<Vec<_>>()
        .join(", ");

    writeln!(
        output,
        "  fn {}({}) -> {} {{",
        function.name,
        params,
        fmt_type(&function.return_type)
    )?;

    for block in &function.blocks {
        format_block(output, block)?;
    }

    writeln!(output, "  }}")
}

fn format_block(output: &mut String, block: &BasicBlock) -> std::fmt::Result {
    writeln!(output, "    {}:", block.label)?;

    for instr in &block.instructions {
        let text = match &instr.kind {
            InstructionKind::Add { result, lhs, rhs } => {
                format!(
                    "{} = add {}, {}",
                    fmt_value(*result),
                    fmt_value(*lhs),
                    fmt_value(*rhs)
                )
            }
            InstructionKind::Sub { result, lhs, rhs } => {
                format!(
                    "{} = sub {}, {}",
                    fmt_value(*result),
                    fmt_value(*lhs),
                    fmt_value(*rhs)
                )
            }
            InstructionKind::Mul { result, lhs, rhs } => {
                format!(
                    "{} = mul {}, {}",
                    fmt_value(*result),
                    fmt_value(*lhs),
                    fmt_value(*rhs)
                )
            }
            InstructionKind::Div { result, lhs, rhs } => {
                format!(
                    "{} = div {}, {}",
                    fmt_value(*result),
                    fmt_value(*lhs),
                    fmt_value(*rhs)
                )
            }
            InstructionKind::Rem { result, lhs, rhs } => {
                format!(
                    "{} = rem {}, {}",
                    fmt_value(*result),
                    fmt_value(*lhs),
                    fmt_value(*rhs)
                )
            }
            InstructionKind::Eq { result, lhs, rhs } => {
                format!(
                    "{} = eq {}, {}",
                    fmt_value(*result),
                    fmt_value(*lhs),
                    fmt_value(*rhs)
                )
            }
            InstructionKind::Ne { result, lhs, rhs } => {
                format!(
                    "{} = ne {}, {}",
                    fmt_value(*result),
                    fmt_value(*lhs),
                    fmt_value(*rhs)
                )
            }
            InstructionKind::Lt { result, lhs, rhs } => {
                format!(
                    "{} = lt {}, {}",
                    fmt_value(*result),
                    fmt_value(*lhs),
                    fmt_value(*rhs)
                )
            }
            InstructionKind::Le { result, lhs, rhs } => {
                format!(
                    "{} = le {}, {}",
                    fmt_value(*result),
                    fmt_value(*lhs),
                    fmt_value(*rhs)
                )
            }
            InstructionKind::Gt { result, lhs, rhs } => {
                format!(
                    "{} = gt {}, {}",
                    fmt_value(*result),
                    fmt_value(*lhs),
                    fmt_value(*rhs)
                )
            }
            InstructionKind::Ge { result, lhs, rhs } => {
                format!(
                    "{} = ge {}, {}",
                    fmt_value(*result),
                    fmt_value(*lhs),
                    fmt_value(*rhs)
                )
            }
            InstructionKind::And { result, lhs, rhs } => {
                format!(
                    "{} = and {}, {}",
                    fmt_value(*result),
                    fmt_value(*lhs),
                    fmt_value(*rhs)
                )
            }
            InstructionKind::Or { result, lhs, rhs } => {
                format!(
                    "{} = or {}, {}",
                    fmt_value(*result),
                    fmt_value(*lhs),
                    fmt_value(*rhs)
                )
            }
            InstructionKind::Not { result, operand } => {
                format!("{} = not {}", fmt_value(*result), fmt_value(*operand))
            }
            InstructionKind::Alloca { result, ty } => {
                format!("{} = alloca {}", fmt_value(*result), fmt_type(ty))
            }
            InstructionKind::GlobalAddr { result, name, ty } => format!(
                "{} = global_addr {} : {}",
                fmt_value(*result),
                name,
                fmt_type(ty)
            ),
            InstructionKind::Load { result, ptr, ty } => {
                format!(
                    "{} = load({}) {}",
                    fmt_value(*result),
                    fmt_type(ty),
                    fmt_value(*ptr)
                )
            }
            InstructionKind::Store { ptr, value } => {
                format!("store {}, {}", fmt_value(*ptr), fmt_value(*value))
            }
            InstructionKind::GetElementPtr {
                result,
                ptr,
                index,
                element_type,
            } => format!(
                "{} = gep {}, {}, {}",
                fmt_value(*result),
                fmt_value(*ptr),
                fmt_value(*index),
                fmt_type(element_type)
            ),
            InstructionKind::FieldPtr {
                result,
                ptr,
                offset,
            } => format!(
                "{} = field_ptr {}, {}",
                fmt_value(*result),
                fmt_value(*ptr),
                offset
            ),
            InstructionKind::ManualAlloc { result, size } => {
                format!("{} = manual_alloc {}", fmt_value(*result), size)
            }
            InstructionKind::EscapeManualAlloc { ptr } => {
                format!("escape_manual_alloc {}", fmt_value(*ptr))
            }
            InstructionKind::Call {
                result,
                function,
                args,
            } => {
                let arg_list = args
                    .iter()
                    .map(|arg| fmt_value(*arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                match result {
                    Some(value) => {
                        format!("{} = call {}({})", fmt_value(*value), function, arg_list)
                    }
                    None => format!("call {}({})", function, arg_list),
                }
            }
            InstructionKind::HostCall {
                result, host, args, ..
            } => {
                let arg_list = args
                    .iter()
                    .map(|arg| fmt_value(*arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                match result {
                    Some(value) => {
                        format!("{} = hostcall {}({})", fmt_value(*value), host, arg_list)
                    }
                    None => format!("hostcall {}({})", host, arg_list),
                }
            }
            InstructionKind::AutodiffStep {
                result,
                operation,
                output,
                upstream,
                inputs,
                targets,
            } => {
                let upstream = upstream
                    .map(fmt_value)
                    .unwrap_or_else(|| "seed".to_string());
                let inputs = inputs
                    .iter()
                    .map(|v| fmt_value(*v))
                    .collect::<Vec<_>>()
                    .join(", ");
                let targets = targets
                    .iter()
                    .map(|v| fmt_value(*v))
                    .collect::<Vec<_>>()
                    .join(", ");
                let text = format!(
                    "autodiff.{} output={} upstream={} inputs=[{}] targets=[{}]",
                    operation,
                    fmt_value(*output),
                    upstream,
                    inputs,
                    targets
                );
                match result {
                    Some(value) => format!("{} = {}", fmt_value(*value), text),
                    None => text,
                }
            }
            InstructionKind::Phi { result, incoming } => {
                let incoming_str = incoming
                    .iter()
                    .map(|(value, block)| format!("[{}, bb{}]", fmt_value(*value), block))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{} = phi {}", fmt_value(*result), incoming_str)
            }
            InstructionKind::Copy { result, source } => {
                format!("{} = copy {}", fmt_value(*result), fmt_value(*source))
            }
            InstructionKind::FuncAddr { result, function } => {
                format!("{} = func_addr {}", fmt_value(*result), function)
            }
            InstructionKind::CallIndirect {
                result,
                fn_ptr,
                args,
                signature_params: _,
                signature_return: _,
            } => {
                let arg_list = args
                    .iter()
                    .map(|arg| fmt_value(*arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                match result {
                    Some(value) => {
                        format!(
                            "{} = call_indirect {}({})",
                            fmt_value(*value),
                            fmt_value(*fn_ptr),
                            arg_list
                        )
                    }
                    None => format!("call_indirect {}({})", fmt_value(*fn_ptr), arg_list),
                }
            }
            InstructionKind::AsyncSuspend { task, state } => {
                format!("async.suspend state{} {}", state, fmt_value(*task))
            }
            InstructionKind::AsyncResume { task, state } => {
                format!("async.resume state{} {}", state, fmt_value(*task))
            }
            InstructionKind::AsyncReady {
                result,
                value,
                output_type,
            } => {
                let value = value.map(fmt_value).unwrap_or_else(|| "unit".to_string());
                format!(
                    "{} = async.ready<{}> {}",
                    fmt_value(*result),
                    fmt_type(output_type),
                    value
                )
            }
            InstructionKind::ConstInt { result, value } => {
                format!("{} = const.int {}", fmt_value(*result), value)
            }
            InstructionKind::ConstIntTyped { result, value, ty } => {
                format!(
                    "{} = const.int<{}> {}",
                    fmt_value(*result),
                    fmt_type(ty),
                    value
                )
            }
            InstructionKind::ConstFloat { result, value } => {
                format!("{} = const.float {}", fmt_value(*result), value)
            }
            InstructionKind::ConstFloatTyped { result, value, ty } => {
                format!(
                    "{} = const.float<{}> {}",
                    fmt_value(*result),
                    fmt_type(ty),
                    value
                )
            }
            InstructionKind::ConstBool { result, value } => {
                format!("{} = const.bool {}", fmt_value(*result), value)
            }
            InstructionKind::ConstString { result, value } => {
                let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
                format!("{} = const.string \"{}\"", fmt_value(*result), escaped)
            }
            InstructionKind::Cast {
                result,
                operand,
                from_ty,
                to_ty,
            } => {
                format!(
                    "{} = cast({} -> {}) {}",
                    fmt_value(*result),
                    fmt_type(from_ty),
                    fmt_type(to_ty),
                    fmt_value(*operand)
                )
            }
            InstructionKind::MakeDynFatPtr {
                result,
                data_ptr,
                vtable_ptr,
            } => {
                format!(
                    "{} = make_fat_ptr({}, {})",
                    fmt_value(*result),
                    fmt_value(*data_ptr),
                    fmt_value(*vtable_ptr)
                )
            }
            InstructionKind::LoadDynDataPtr { result, fat_ptr } => {
                format!(
                    "{} = load_data_ptr {}",
                    fmt_value(*result),
                    fmt_value(*fat_ptr)
                )
            }
            InstructionKind::LoadDynVtablePtr { result, fat_ptr } => {
                format!(
                    "{} = load_vtable_ptr {}",
                    fmt_value(*result),
                    fmt_value(*fat_ptr)
                )
            }
            InstructionKind::LoadVtableSlot {
                result,
                vtable_ptr,
                slot_index,
            } => {
                format!(
                    "{} = vtable_slot({}) {}",
                    fmt_value(*result),
                    slot_index,
                    fmt_value(*vtable_ptr)
                )
            }
        };

        writeln!(output, "      {}", text)?;
    }

    if let Some(term) = &block.terminator {
        writeln!(output, "      {}", format_terminator(term))?;
    } else {
        writeln!(output, "      <no terminator>")?;
    }

    Ok(())
}

fn format_terminator(term: &Terminator) -> String {
    match term {
        Terminator::Return { value } => match value {
            Some(val) => format!("ret {}", fmt_value(*val)),
            None => "ret".to_string(),
        },
        Terminator::Branch { target } => format!("br bb{}", target),
        Terminator::CondBranch {
            condition,
            true_block,
            false_block,
        } => format!(
            "br.if {} -> bb{}, else bb{}",
            fmt_value(*condition),
            true_block,
            false_block
        ),
        Terminator::Switch {
            value,
            cases,
            default,
        } => {
            let mut parts = cases
                .iter()
                .map(|(constant, block)| format!("{}: bb{}", constant, block))
                .collect::<Vec<_>>();
            parts.push(format!("default: bb{}", default));
            format!("switch {} {{ {} }}", fmt_value(*value), parts.join(", "))
        }
        Terminator::Unreachable => "unreachable".to_string(),
    }
}

fn fmt_type(ty: &Type) -> String {
    match ty {
        Type::Unknown => "unknown".to_string(),
        Type::Void => "void".to_string(),
        Type::Int => "int".to_string(),
        Type::Float => "float".to_string(),
        Type::ExactInt { signed, width } => format!(
            "{}{}",
            if *signed { "i" } else { "u" },
            match width {
                crate::ir::IntWidth::I8 => "8",
                crate::ir::IntWidth::I16 => "16",
                crate::ir::IntWidth::I32 => "32",
                crate::ir::IntWidth::I64 => "64",
                crate::ir::IntWidth::Isize | crate::ir::IntWidth::Usize => "size",
            }
        ),
        Type::ExactFloat { width } => match width {
            crate::ir::FloatWidth::F32 => "f32".to_string(),
            crate::ir::FloatWidth::F64 => "f64".to_string(),
        },
        Type::Bool => "bool".to_string(),
        Type::String => "string".to_string(),
        Type::Char => "char".to_string(),
        Type::Pointer(inner) => format!("*{}", fmt_type(inner)),
        Type::Array { element_type, size } => format!("[{} x {}]", size, fmt_type(element_type)),
        Type::Tuple { elements } => {
            let elems = elements.iter().map(fmt_type).collect::<Vec<_>>().join(", ");
            format!("({})", elems)
        }
        Type::Struct { name, .. } => format!("struct {}", name),
        Type::Enum { name, .. } => format!("enum {}", name),
        Type::Generic {
            name,
            args,
            representation,
        } => format!(
            "{}<{}> [{}]",
            name,
            args.iter().map(fmt_type).collect::<Vec<_>>().join(", "),
            fmt_type(representation)
        ),
        Type::Function {
            params,
            return_type,
        } => {
            let params = params.iter().map(fmt_type).collect::<Vec<_>>().join(", ");
            format!("fn({}) -> {}", params, fmt_type(return_type))
        }
        Type::Task { output } => format!("Task<{}>", fmt_type(output)),
        Type::Range => "Range".to_string(),
        Type::Tensor {
            dtype,
            rank,
            dims,
            layout,
            device,
        } => {
            let mut parts = vec![fmt_type(dtype)];
            if let Some(rank) = rank {
                parts.push(format!("rank{}", rank));
            } else {
                parts.push("dynamic".to_string());
            }
            if let Some(dims) = dims {
                for dim in dims {
                    parts.push(match dim {
                        Some(size) => format!("dim{}", size),
                        None => "dyn".to_string(),
                    });
                }
            }
            if let Some(layout) = layout {
                parts.push(layout.clone());
            }
            if let Some(device) = device {
                parts.push(device.clone());
            }
            format!("Tensor<{}>", parts.join(", "))
        }
        Type::DynTrait {
            trait_name,
            auto_traits,
        } => {
            if auto_traits.is_empty() {
                format!("dyn {}", trait_name)
            } else {
                format!("dyn {} + {}", trait_name, auto_traits.join(" + "))
            }
        }
    }
}

fn fmt_value(value: Value) -> String {
    format!("%v{}", value.id)
}
