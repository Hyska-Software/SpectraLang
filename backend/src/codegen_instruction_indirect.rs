impl CodeGenerator {
    fn generate_indirect_instruction<M: Module>(
        module: &mut M,
        function_map: &HashMap<String, FuncId>,
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
            InstructionKind::FuncAddr { result, function } => {
                let func_id = *function_map
                    .get(function)
                    .ok_or_else(|| BackendCodegenError::missing_function(function))?;

                let func_addr = if let Some(address) = hostcall
                    .finalized_function_ptrs
                    .and_then(|ptrs| ptrs.get(function))
                {
                    builder.ins().iconst(
                        module.target_config().pointer_type(),
                        *address,
                    )
                } else {
                    let func_ref = module.declare_func_in_func(func_id, builder.func);
                    builder
                        .ins()
                        .func_addr(module.target_config().pointer_type(), func_ref)
                };
                value_map.insert(result.id, func_addr);
            }

            InstructionKind::CallIndirect {
                result,
                fn_ptr,
                args,
                signature_params,
                signature_return,
            } => {
                let ptr_val = get_value(fn_ptr)?;

                let mut sig = module.make_signature();
                for param_ty in signature_params {
                    let cl_type = Self::ir_type_to_cranelift(param_ty)?;
                    sig.params.push(AbiParam::new(cl_type));
                }

                let ret_ty = Self::ir_type_to_cranelift(signature_return)?;
                if ret_ty != types::I8 || **signature_return != IRType::Void {
                    sig.returns.push(AbiParam::new(ret_ty));
                }

                let sig_ref = builder.import_signature(sig);

                let arg_values: Result<Vec<_>, _> = args
                    .iter()
                    .enumerate()
                    .map(|(idx, arg)| {
                        let value = get_value(arg)?;
                        Ok(match signature_params.get(idx) {
                            Some(param_ty) => Self::coerce_call_arg(builder, value, param_ty),
                            None => value,
                        })
                    })
                    .collect();
                let arg_values = arg_values?;

                let call = builder.ins().call_indirect(sig_ref, ptr_val, &arg_values);

                if let Some(result) = result {
                    let results = builder.inst_results(call);
                    if !results.is_empty() {
                        value_map.insert(result.id, results[0]);
                    }
                }
            }
            _ => unreachable!("indirect instruction category mismatch"),
        }
        Ok(())
    }
}
