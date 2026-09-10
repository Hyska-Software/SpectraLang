/// Function metadata maps shared by call lowering.
struct CallFunctionTables<'a> {
    ids: &'a HashMap<String, FuncId>,
    params: &'a HashMap<String, Vec<IRType>>,
}

impl CodeGenerator {
    fn generate_call_instruction<M: Module>(
        module: &mut M,
        tables: &CallFunctionTables<'_>,
        hostcall: &mut HostCallLoweringContext<'_>,
        builder: &mut FunctionBuilder,
        kind: &InstructionKind,
        value_map: &mut DenseValueMap,
        emitted_tail_call: &mut bool,
    ) -> BackendResult<()> {
        *emitted_tail_call = false;
        let get_value = |v: &IRValue| -> BackendResult<Value> {
            value_map
                .get(v.id)
                .ok_or_else(|| BackendCodegenError::missing_value(v.id))
        };

        match kind {
            InstructionKind::Call {
                result,
                function,
                args,
                is_tail,
            } => {
                let func_id = *tables.ids
                    .get(function)
                    .ok_or_else(|| BackendCodegenError::missing_function(function))?;

                let func_ref = module.declare_func_in_func(func_id, builder.func);

                let param_types = tables.params.get(function.as_str());
                let arg_values: Result<Vec<_>, _> = args
                    .iter()
                    .enumerate()
                    .map(|(idx, arg)| {
                        let value = get_value(arg)?;
                        Ok(match param_types.and_then(|params| params.get(idx)) {
                            Some(param_ty) => Self::coerce_call_arg(builder, value, param_ty),
                            None => value,
                        })
                    })
                    .collect();
                let arg_values = arg_values?;

                // Direct self-tail-recursion: fuse this call and the trailing
                // `Return` of its result into Cranelift's native `return_call`.
                // The verifier requires caller and callee to share a calling
                // convention that supports tail calls; both are true because
                // the declaration path switches self-recursive functions to
                // `CallConv::Tail` (see `uses_tail_call_convention`). When any
                // of that does not hold we fall back to the plain call below.
                if *is_tail && builder.func.signature.call_conv.supports_tail_calls() {
                    let callee_sig =
                        builder.func.dfg.ext_funcs[func_ref].signature;
                    if builder.func.dfg.signatures[callee_sig].call_conv
                        == builder.func.signature.call_conv
                    {
                        builder.ins().return_call(func_ref, &arg_values);
                        *emitted_tail_call = true;
                        return Ok(());
                    }
                }

                let call = builder.ins().call(func_ref, &arg_values);

                if let Some(result) = result {
                    let results = builder.inst_results(call);
                    if !results.is_empty() {
                        value_map.insert(result.id, results[0]);
                    }
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
                let output_value = get_value(output)?;
                if operation == "grad_handle" {
                    let func_ref = module.declare_func_in_func(
                        hostcall.runtime_func(RuntimeImport::TensorGradHandle),
                        builder.func,
                    );
                    let call = builder.ins().call(func_ref, &[output_value]);
                    if let Some(result) = result {
                        value_map.insert(result.id, builder.inst_results(call)[0]);
                    }
                    return Ok(());
                }
                let operation_name = operation.strip_prefix("grad_apply_").ok_or_else(|| {
                    BackendCodegenError::invalid_ir(format!(
                        "E3004: invalid autodiff step {operation}"
                    ))
                })?;
                let opcode = match operation_name {
                    "add" => 0,
                    "sub" => 1,
                    "mul" => 2,
                    "div" => 3,
                    "neg" => 4,
                    "exp" => 5,
                    "log" => 6,
                    "relu" => 7,
                    "sigmoid" => 8,
                    "sum_t" => 9,
                    "mean_t" => 10,
                    "dot_t" => 11,
                    "matmul" => 12,
                    "transpose" => 13,
                    "reshape" => 14,
                    "linear" => 15,
                    "mse_loss" => 16,
                    "tanh" => 17,
                    "sqrt" => 18,
                    "bce_loss" => 19,
                    "conv2d" => 20,
                    "max_pool2d" => 21,
                    "dropout" => 22,
                    "matmul_batched" => 23,
                    "concat" => 24,
                    "stack" => 25,
                    "slice" => 26,
                    "permute" => 27,
                    other => {
                        return Err(BackendCodegenError::invalid_ir(format!(
                            "E3004: no reverse kernel for {other}"
                        )))
                    }
                };
                if inputs.len() > 3 || targets.len() > 3 {
                    return Err(BackendCodegenError::invalid_ir(
                        "E3004: autodiff step has too many operands",
                    ));
                }
                let zero = builder.ins().iconst(types::I64, 0);
                let upstream_value = upstream
                    .map(|value| get_value(&value))
                    .transpose()?
                    .unwrap_or(zero);
                let mut params = vec![
                    builder.ins().iconst(types::I64, opcode),
                    output_value,
                    upstream_value,
                ];
                for index in 0..3 {
                    let value = targets.get(index).or_else(|| inputs.get(index));
                    params.push(value.map(get_value).transpose()?.unwrap_or(zero));
                }
                let func_ref = module.declare_func_in_func(
                    hostcall.runtime_func(RuntimeImport::TensorAutodiffApply),
                    builder.func,
                );
                builder.ins().call(func_ref, &params);
                if result.is_some() {
                    return Err(BackendCodegenError::invalid_ir(
                        "E3004: reverse apply step cannot produce a value",
                    ));
                }
            }
            _ => unreachable!("call instruction category mismatch"),
        }
        Ok(())
    }
}
