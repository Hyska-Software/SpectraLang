#[cfg(test)]
mod tests {
    use super::*;
    use crate::BackendErrorKind;
    use spectra_midend::ir::Parameter;

    #[test]
    fn r3104_dense_value_map_handles_dense_and_missing_ids() {
        let mut values = DenseValueMap::with_capacity(3);
        assert!(values.get(0).is_none());

        let value = cranelift::prelude::Value::from_u32(1);
        values.insert(2, value);
        assert_eq!(values.get(2), Some(value));
        assert!(values.get(1).is_none());
        assert!(values.get(99).is_none());
    }

    #[test]
    fn r3104_dense_value_map_resizes_for_sparse_synthetic_ids() {
        let mut values = DenseValueMap::with_capacity(1);
        let value = cranelift::prelude::Value::from_u32(2);
        values.insert(17, value);
        assert_eq!(values.get(17), Some(value));
        assert!(values.get(16).is_none());
    }

    #[test]
    fn r3104_jit_preinterns_duplicate_host_names_once() {
        let mut codegen = CodeGenerator::new();
        let mut module = IRModule::new("r3104_host_names");
        let mut function = IRFunction::new("main", vec![], IRType::Void);
        let entry = function.add_block("entry");
        let block = function.get_block_mut(entry).unwrap();
        for _ in 0..2 {
            block.add_instruction(InstructionKind::HostCall {
                result: None,
                host: "spectra.test.duplicate".to_string(),
                args: vec![],
                result_type: None,
            });
        }
        block.set_terminator(Terminator::Return { value: None });
        module.add_function(function);

        codegen.pre_intern_host_names(&module);
        assert_eq!(codegen.host_call_sites.len(), 1);
        assert!(codegen
            .host_call_sites
            .contains_key("spectra.test.duplicate"));
        let record = codegen
            .host_call_sites
            .get("spectra.test.duplicate")
            .copied()
            .expect("deduplicated host-call record");
        assert_ne!(record.cache_ptr, 0);
        assert!(record.cache_data_id.is_none());
        assert_eq!(codegen.host_call_cache_storage.len(), 1);
    }

    #[test]
    fn r3104_parameter_lookup_uses_actual_ir_id() {
        let mut codegen = CodeGenerator::new();
        let mut function = IRFunction::new(
            "sparse_parameter",
            vec![Parameter {
                id: 7,
                name: "value".to_string(),
                ty: IRType::Int,
            }],
            IRType::Int,
        );
        let entry = function.add_block("entry");
        function
            .get_block_mut(entry)
            .unwrap()
            .set_terminator(Terminator::Return {
                value: Some(IRValue { id: 7 }),
            });

        assert!(codegen.declare_function(&function).is_ok());
        let function_params: HashMap<String, Vec<IRType>> = std::iter::once((
            function.name.clone(),
            function
                .params
                .iter()
                .map(|param| param.ty.clone())
                .collect(),
        ))
        .collect();
        assert!(codegen
            .define_function(&function, &function_params)
            .is_ok());
    }

    #[test]
    fn test_codegen_creation() {
        let codegen = CodeGenerator::new();
        assert!(codegen.function_map.is_empty());
    }

    #[test]
    fn local_scalar_array_allocas_use_native_stack_when_not_escaping() {
        let function = IRFunction {
            name: "stackable".to_string(),
            params: vec![],
            return_type: IRType::Int,
            source_span: None,
            locals: vec![],
            next_value_id: 5,
            next_block_id: 1,
            blocks: vec![IRBasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![
                    Instruction {
                        id: 0,
                        kind: InstructionKind::Alloca {
                            result: IRValue { id: 0 },
                            ty: IRType::Array {
                                element_type: Box::new(IRType::Int),
                                size: 4,
                            },
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 1,
                        kind: InstructionKind::ConstInt {
                            result: IRValue { id: 1 },
                            value: 0,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 2,
                        kind: InstructionKind::GetElementPtr {
                            result: IRValue { id: 2 },
                            ptr: IRValue { id: 0 },
                            index: IRValue { id: 1 },
                            element_type: IRType::Int,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 3,
                        kind: InstructionKind::ConstInt {
                            result: IRValue { id: 3 },
                            value: 42,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 4,
                        kind: InstructionKind::Store {
                            ptr: IRValue { id: 2 },
                            value: IRValue { id: 3 },
                        },
                        source_span: None,
                    },
                ],
                terminator: Some(Terminator::Return {
                    value: Some(IRValue { id: 3 }),
                }),
            }],
        };

        let stack_allocas = CodeGenerator::collect_stack_allocas(&function);
        assert!(stack_allocas.contains(&0));
    }

    #[test]
    fn scalar_alloca_promotion_requires_load_store_only_access() {
        let function = IRFunction {
            name: "promotable_scalar".to_string(),
            params: vec![],
            return_type: IRType::Int,
            source_span: None,
            locals: vec![],
            next_value_id: 3,
            next_block_id: 1,
            blocks: vec![IRBasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![
                    Instruction {
                        id: 0,
                        kind: InstructionKind::Alloca {
                            result: IRValue { id: 0 },
                            ty: IRType::Int,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 1,
                        kind: InstructionKind::ConstInt {
                            result: IRValue { id: 1 },
                            value: 42,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 2,
                        kind: InstructionKind::Store {
                            ptr: IRValue { id: 0 },
                            value: IRValue { id: 1 },
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 3,
                        kind: InstructionKind::Load {
                            result: IRValue { id: 2 },
                            ptr: IRValue { id: 0 },
                            ty: IRType::Int,
                        },
                        source_span: None,
                    },
                ],
                terminator: Some(Terminator::Return {
                    value: Some(IRValue { id: 2 }),
                }),
            }],
        };

        let promoted = CodeGenerator::collect_promotable_scalar_allocas(&function);
        assert!(promoted.contains_key(&0));
    }

    #[test]
    fn scalar_alloca_promotion_rejects_load_before_store() {
        let function = IRFunction {
            name: "uninitialized_scalar".to_string(),
            params: vec![],
            return_type: IRType::Int,
            source_span: None,
            locals: vec![],
            next_value_id: 3,
            next_block_id: 1,
            blocks: vec![IRBasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![
                    Instruction {
                        id: 0,
                        kind: InstructionKind::Alloca {
                            result: IRValue { id: 0 },
                            ty: IRType::Int,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 1,
                        kind: InstructionKind::Load {
                            result: IRValue { id: 1 },
                            ptr: IRValue { id: 0 },
                            ty: IRType::Int,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 2,
                        kind: InstructionKind::ConstInt {
                            result: IRValue { id: 2 },
                            value: 42,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 3,
                        kind: InstructionKind::Store {
                            ptr: IRValue { id: 0 },
                            value: IRValue { id: 2 },
                        },
                        source_span: None,
                    },
                ],
                terminator: Some(Terminator::Return {
                    value: Some(IRValue { id: 1 }),
                }),
            }],
        };

        let promoted = CodeGenerator::collect_promotable_scalar_allocas(&function);
        assert!(!promoted.contains_key(&0));
    }

    #[test]
    fn scalar_alloca_promotion_rejects_escaping_pointer() {
        let function = IRFunction {
            name: "escaping_scalar".to_string(),
            params: vec![],
            return_type: IRType::Void,
            source_span: None,
            locals: vec![],
            next_value_id: 1,
            next_block_id: 1,
            blocks: vec![IRBasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![
                    Instruction {
                        id: 0,
                        kind: InstructionKind::Alloca {
                            result: IRValue { id: 0 },
                            ty: IRType::Int,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 1,
                        kind: InstructionKind::Call {
                            result: None,
                            function: "consume_pointer".to_string(),
                            args: vec![IRValue { id: 0 }],
                        },
                        source_span: None,
                    },
                ],
                terminator: Some(Terminator::Return { value: None }),
            }],
        };

        let promoted = CodeGenerator::collect_promotable_scalar_allocas(&function);
        assert!(!promoted.contains_key(&0));
    }

    #[test]
    fn escaping_alloca_stays_on_manual_runtime_heap() {
        let function = IRFunction {
            name: "escaping".to_string(),
            params: vec![],
            return_type: IRType::Array {
                element_type: Box::new(IRType::Int),
                size: 4,
            },
            source_span: None,
            locals: vec![],
            next_value_id: 1,
            next_block_id: 1,
            blocks: vec![IRBasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![Instruction {
                    id: 0,
                    kind: InstructionKind::Alloca {
                        result: IRValue { id: 0 },
                        ty: IRType::Array {
                            element_type: Box::new(IRType::Int),
                            size: 4,
                        },
                    },
                    source_span: None,
                }],
                terminator: Some(Terminator::Return {
                    value: Some(IRValue { id: 0 }),
                }),
            }],
        };

        let stack_allocas = CodeGenerator::collect_stack_allocas(&function);
        assert!(!stack_allocas.contains(&0));
    }

    #[test]
    fn struct_contained_stack_alloca_does_not_escape() {
        let array_type = IRType::Array {
            element_type: Box::new(IRType::Int),
            size: 4,
        };
        let holder_type = IRType::Struct {
            name: "Holder".to_string(),
            fields: vec![("items".to_string(), array_type.clone())],
        };
        let function = IRFunction {
            name: "contained".to_string(),
            params: vec![],
            return_type: IRType::Int,
            source_span: None,
            locals: vec![],
            next_value_id: 5,
            next_block_id: 1,
            blocks: vec![IRBasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![
                    Instruction {
                        id: 0,
                        kind: InstructionKind::Alloca {
                            result: IRValue { id: 0 },
                            ty: holder_type,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 1,
                        kind: InstructionKind::Alloca {
                            result: IRValue { id: 1 },
                            ty: array_type,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 2,
                        kind: InstructionKind::ConstInt {
                            result: IRValue { id: 2 },
                            value: 0,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 3,
                        kind: InstructionKind::GetElementPtr {
                            result: IRValue { id: 3 },
                            ptr: IRValue { id: 0 },
                            index: IRValue { id: 2 },
                            element_type: IRType::Int,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 4,
                        kind: InstructionKind::Store {
                            ptr: IRValue { id: 3 },
                            value: IRValue { id: 1 },
                        },
                        source_span: None,
                    },
                ],
                terminator: Some(Terminator::Return {
                    value: Some(IRValue { id: 2 }),
                }),
            }],
        };

        let stack_allocas = CodeGenerator::collect_stack_allocas(&function);
        assert!(stack_allocas.contains(&0));
        assert!(stack_allocas.contains(&1));
        assert!(!CodeGenerator::function_needs_manual_frame(
            &function,
            &stack_allocas
        ));
    }

    #[test]
    fn escaping_container_forces_contained_alloca_to_escape() {
        let array_type = IRType::Array {
            element_type: Box::new(IRType::Int),
            size: 4,
        };
        let holder_type = IRType::Struct {
            name: "Holder".to_string(),
            fields: vec![("items".to_string(), array_type.clone())],
        };
        let function = IRFunction {
            name: "escaping_container".to_string(),
            params: vec![],
            return_type: IRType::Struct {
                name: "Holder".to_string(),
                fields: vec![("items".to_string(), array_type.clone())],
            },
            source_span: None,
            locals: vec![],
            next_value_id: 5,
            next_block_id: 1,
            blocks: vec![IRBasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![
                    Instruction {
                        id: 0,
                        kind: InstructionKind::Alloca {
                            result: IRValue { id: 0 },
                            ty: holder_type,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 1,
                        kind: InstructionKind::Alloca {
                            result: IRValue { id: 1 },
                            ty: array_type,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 2,
                        kind: InstructionKind::ConstInt {
                            result: IRValue { id: 2 },
                            value: 0,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 3,
                        kind: InstructionKind::GetElementPtr {
                            result: IRValue { id: 3 },
                            ptr: IRValue { id: 0 },
                            index: IRValue { id: 2 },
                            element_type: IRType::Int,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 4,
                        kind: InstructionKind::Store {
                            ptr: IRValue { id: 3 },
                            value: IRValue { id: 1 },
                        },
                        source_span: None,
                    },
                ],
                terminator: Some(Terminator::Return {
                    value: Some(IRValue { id: 0 }),
                }),
            }],
        };

        let stack_allocas = CodeGenerator::collect_stack_allocas(&function);
        assert!(!stack_allocas.contains(&0));
        assert!(!stack_allocas.contains(&1));
        assert!(CodeGenerator::function_needs_manual_frame(
            &function,
            &stack_allocas
        ));
    }

    #[test]
    fn test_type_conversion() {
        assert_eq!(
            CodeGenerator::ir_type_to_cranelift(&IRType::Bool).unwrap(),
            types::I8
        );
        assert_eq!(
            CodeGenerator::ir_type_to_cranelift(&IRType::Int).unwrap(),
            types::I64
        );
        assert_eq!(
            CodeGenerator::ir_type_to_cranelift(&IRType::Float).unwrap(),
            types::F64
        );
        assert_eq!(
            CodeGenerator::ir_type_to_cranelift(&IRType::Task {
                output: Box::new(IRType::Int),
            })
            .unwrap(),
            types::I64
        );
    }

    #[test]
    fn test_simple_function_generation() {
        let mut codegen = CodeGenerator::new();

        let func = IRFunction::new(
            "test_func",
            vec![Parameter {
                id: 0,
                name: "a".to_string(),
                ty: IRType::Int,
            }],
            IRType::Int,
        );

        let result = codegen.declare_function(&func);
        assert!(result.is_ok());
    }

    #[test]
    fn test_arithmetic_instructions() {
        use spectra_midend::ir::{InstructionKind, Terminator, Value};

        let mut codegen = CodeGenerator::new();

        // Create function: fn add(a: int, b: int) -> int { return a + b; }
        let mut func = IRFunction::new(
            "add",
            vec![
                Parameter {
                    id: 0,
                    name: "a".to_string(),
                    ty: IRType::Int,
                },
                Parameter {
                    id: 1,
                    name: "b".to_string(),
                    ty: IRType::Int,
                },
            ],
            IRType::Int,
        );

        // Create entry block
        let entry_block_id = func.add_block("entry");
        let entry_block = func.get_block_mut(entry_block_id).unwrap();

        // Add instruction: result = a + b
        let result_value = Value { id: 2 };
        entry_block.add_instruction(InstructionKind::Add {
            result: result_value,
            lhs: Value { id: 0 }, // a
            rhs: Value { id: 1 }, // b
        });

        // Return instruction
        entry_block.set_terminator(Terminator::Return {
            value: Some(result_value),
        });

        // Generate code
        let result = codegen.declare_function(&func);
        assert!(result.is_ok());

        let result = codegen.define_function(&func, &std::collections::HashMap::new());
        assert!(result.is_ok());
    }

    #[test]
    fn r2007_missing_branch_target_returns_typed_error() {
        use spectra_midend::ir::Terminator;

        let mut codegen = CodeGenerator::new();
        let mut func = IRFunction::new("missing_branch_target", vec![], IRType::Void);
        let entry_block_id = func.add_block("entry");
        let entry_block = func.get_block_mut(entry_block_id).unwrap();
        entry_block.set_terminator(Terminator::Branch { target: 999 });

        assert!(codegen.declare_function(&func).is_ok());
        let err = codegen
            .define_function(&func, &std::collections::HashMap::new())
            .expect_err("missing target block must be reported, not panic");
        assert_eq!(err.kind(), &BackendErrorKind::MissingBlock);
        assert!(err.message().contains("999"));
    }

    #[test]
    fn r2007_missing_phi_incoming_returns_typed_error() {
        use spectra_midend::ir::{InstructionKind, Terminator, Value};

        let mut codegen = CodeGenerator::new();
        let mut func = IRFunction::new("missing_phi_incoming", vec![], IRType::Int);
        let entry_block_id = func.add_block("entry");
        let join_block_id = func.add_block("join");

        func.get_block_mut(entry_block_id)
            .unwrap()
            .set_terminator(Terminator::Branch {
                target: join_block_id,
            });

        let phi_result = Value { id: 0 };
        let join_block = func.get_block_mut(join_block_id).unwrap();
        join_block.add_instruction(InstructionKind::Phi {
            result: phi_result,
            incoming: vec![(Value { id: 1 }, 123)],
        });
        join_block.set_terminator(Terminator::Return {
            value: Some(phi_result),
        });

        assert!(codegen.declare_function(&func).is_ok());
        let err = codegen
            .define_function(&func, &std::collections::HashMap::new())
            .expect_err("missing phi incoming must be reported, not panic");
        assert_eq!(err.kind(), &BackendErrorKind::MissingPhiIncoming);
        assert!(err.message().contains(&join_block_id.to_string()));
    }

    #[test]
    fn test_comparison_instructions() {
        use spectra_midend::ir::{InstructionKind, Terminator, Value};

        let mut codegen = CodeGenerator::new();

        // Create function: fn is_greater(a: int, b: int) -> bool { return a > b; }
        let mut func = IRFunction::new(
            "is_greater",
            vec![
                Parameter {
                    id: 0,
                    name: "a".to_string(),
                    ty: IRType::Int,
                },
                Parameter {
                    id: 1,
                    name: "b".to_string(),
                    ty: IRType::Int,
                },
            ],
            IRType::Bool,
        );

        // Create entry block
        let entry_block_id = func.add_block("entry");
        let entry_block = func.get_block_mut(entry_block_id).unwrap();

        // Comparison: result = a > b
        let result_value = Value { id: 2 };
        entry_block.add_instruction(InstructionKind::Gt {
            result: result_value,
            lhs: Value { id: 0 },
            rhs: Value { id: 1 },
        });

        // Return
        entry_block.set_terminator(Terminator::Return {
            value: Some(result_value),
        });

        // Generate code
        assert!(codegen.declare_function(&func).is_ok());
        assert!(codegen.define_function(&func, &std::collections::HashMap::new()).is_ok());
    }

    #[test]
    fn r3105_batch_planner_groups_independent_generic_hostcalls() {
        use spectra_midend::ir::{InstructionKind, Terminator, Value};

        let mut codegen = CodeGenerator::new();
        let mut func = IRFunction::new("hostcall_batch_stats", vec![], IRType::Int);
        let entry = func.add_block("entry");
        let block = func.get_block_mut(entry).unwrap();
        let first_arg = Value { id: 0 };
        let second_arg = Value { id: 1 };
        let third_arg = Value { id: 2 };
        block.add_instruction(InstructionKind::ConstInt {
            result: first_arg,
            value: -7,
        });
        block.add_instruction(InstructionKind::ConstInt {
            result: second_arg,
            value: 3,
        });
        block.add_instruction(InstructionKind::ConstInt {
            result: third_arg,
            value: 9,
        });
        let first_result = Value { id: 3 };
        block.add_instruction(InstructionKind::HostCall {
            result: Some(first_result),
            host: "spectra.std.math.abs".to_string(),
            args: vec![first_arg],
            result_type: Some(IRType::Int),
        });
        let second_result = Value { id: 4 };
        block.add_instruction(InstructionKind::HostCall {
            result: Some(second_result),
            host: "spectra.std.math.max".to_string(),
            args: vec![second_arg, third_arg],
            result_type: Some(IRType::Int),
        });
        block.set_terminator(Terminator::Return {
            value: Some(second_result),
        });

        codegen.pre_intern_host_names_for_test(&func);
        assert!(codegen.declare_function(&func).is_ok());
        assert!(codegen.define_function(&func, &std::collections::HashMap::new()).is_ok());
        let stats = codegen.hostcall_batch_stats();
        assert_eq!(stats.batched_sites, 1);
        assert_eq!(stats.batched_hostcalls, 2);
        assert_eq!(stats.fallback_hostcalls, 0);
        assert!(stats.argument_arena_bytes > 0);
        assert!(stats.result_arena_bytes > 0);
    }

    #[test]
    fn r3105_batch_planner_falls_back_on_result_dependency() {
        use spectra_midend::ir::{InstructionKind, Terminator, Value};

        let mut codegen = CodeGenerator::new();
        let mut func = IRFunction::new("hostcall_batch_dependency", vec![], IRType::Int);
        let entry = func.add_block("entry");
        let block = func.get_block_mut(entry).unwrap();
        let input = Value { id: 0 };
        block.add_instruction(InstructionKind::ConstInt {
            result: input,
            value: -7,
        });
        let first_result = Value { id: 1 };
        block.add_instruction(InstructionKind::HostCall {
            result: Some(first_result),
            host: "spectra.std.math.abs".to_string(),
            args: vec![input],
            result_type: Some(IRType::Int),
        });
        let second_result = Value { id: 2 };
        block.add_instruction(InstructionKind::HostCall {
            result: Some(second_result),
            host: "spectra.std.math.abs".to_string(),
            args: vec![first_result],
            result_type: Some(IRType::Int),
        });
        block.set_terminator(Terminator::Return {
            value: Some(second_result),
        });

        codegen.pre_intern_host_names_for_test(&func);
        assert!(codegen.declare_function(&func).is_ok());
        assert!(codegen.define_function(&func, &std::collections::HashMap::new()).is_ok());
        let stats = codegen.hostcall_batch_stats();
        assert_eq!(stats.batched_sites, 0);
        assert_eq!(stats.batched_hostcalls, 0);
        assert_eq!(stats.fallback_hostcalls, 2);
    }

    #[test]
    fn test_logical_instructions() {
        use spectra_midend::ir::{InstructionKind, Terminator, Value};

        let mut codegen = CodeGenerator::new();

        // Create function: fn and_op(a: bool, b: bool) -> bool { return a && b; }
        let mut func = IRFunction::new(
            "and_op",
            vec![
                Parameter {
                    id: 0,
                    name: "a".to_string(),
                    ty: IRType::Bool,
                },
                Parameter {
                    id: 1,
                    name: "b".to_string(),
                    ty: IRType::Bool,
                },
            ],
            IRType::Bool,
        );

        // Create entry block
        let entry_block_id = func.add_block("entry");
        let entry_block = func.get_block_mut(entry_block_id).unwrap();

        // Logical AND: result = a && b
        let result_value = Value { id: 2 };
        entry_block.add_instruction(InstructionKind::And {
            result: result_value,
            lhs: Value { id: 0 },
            rhs: Value { id: 1 },
        });

        // Return
        entry_block.set_terminator(Terminator::Return {
            value: Some(result_value),
        });

        // Generate code
        assert!(codegen.declare_function(&func).is_ok());
        assert!(codegen.define_function(&func, &std::collections::HashMap::new()).is_ok());
    }

    #[test]
    fn test_typed_host_float_result_cast_to_int_codegen() {
        use spectra_midend::ir::{InstructionKind, Terminator, Value};

        let mut codegen = CodeGenerator::new();
        let mut func = IRFunction::new("host_float_to_int", vec![], IRType::Int);

        let entry_block_id = func.add_block("entry");
        let entry_block = func.get_block_mut(entry_block_id).unwrap();

        let float_arg = Value { id: 0 };
        entry_block.add_instruction(InstructionKind::ConstFloat {
            result: float_arg,
            value: 9.9,
        });

        let host_result = Value { id: 1 };
        entry_block.add_instruction(InstructionKind::HostCall {
            result: Some(host_result),
            host: "spectra.std.math.floor_f".to_string(),
            args: vec![float_arg],
            result_type: Some(IRType::Float),
        });

        let cast_result = Value { id: 2 };
        entry_block.add_instruction(InstructionKind::Cast {
            result: cast_result,
            operand: host_result,
            from_ty: IRType::Float,
            to_ty: IRType::Int,
        });

        entry_block.set_terminator(Terminator::Return {
            value: Some(cast_result),
        });

        codegen.pre_intern_host_names_for_test(&func);
        assert!(codegen.declare_function(&func).is_ok());
        assert!(codegen.define_function(&func, &std::collections::HashMap::new()).is_ok());
    }

    #[test]
    fn test_async_ready_suspend_resume_codegen() {
        use spectra_midend::ir::{InstructionKind, Terminator, Value};

        let mut codegen = CodeGenerator::new();
        let mut func = IRFunction::new(
            "async_minimal",
            vec![],
            IRType::Task {
                output: Box::new(IRType::Int),
            },
        );

        let entry_block_id = func.add_block("entry");
        let entry_block = func.get_block_mut(entry_block_id).unwrap();

        let payload = Value { id: 0 };
        entry_block.add_instruction(InstructionKind::ConstInt {
            result: payload,
            value: 7,
        });

        let task = Value { id: 1 };
        entry_block.add_instruction(InstructionKind::AsyncReady {
            result: task,
            value: Some(payload),
            output_type: IRType::Int,
        });
        entry_block.add_instruction(InstructionKind::AsyncSuspend { task, state: 0 });
        entry_block.add_instruction(InstructionKind::AsyncResume { task, state: 0 });
        entry_block.set_terminator(Terminator::Return { value: Some(task) });

        assert!(codegen.declare_function(&func).is_ok());
        assert!(codegen.define_function(&func, &std::collections::HashMap::new()).is_ok());
    }
}
