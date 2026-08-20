impl CodeGenerator {
    fn generate_call_instruction<M: Module>(
        module: &mut M,
        function_map: &HashMap<String, FuncId>,
        function_params: &HashMap<String, Vec<IRType>>,
        hostcall: &mut HostCallLoweringContext<'_>,
        builder: &mut FunctionBuilder,
        kind: &InstructionKind,
        value_map: &mut DenseValueMap,
    ) -> BackendResult<()> {
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
            } => {
                let func_id = *function_map
                    .get(function)
                    .ok_or_else(|| BackendCodegenError::missing_function(function))?;

                let func_ref = module.declare_func_in_func(func_id, builder.func);

                let param_types = function_params.get(function.as_str());
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
