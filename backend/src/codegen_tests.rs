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
            async_layout: None,
            suspension_barrier: false,
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
            async_layout: None,
            suspension_barrier: false,
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
            async_layout: None,
            suspension_barrier: false,
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
            async_layout: None,
            suspension_barrier: false,
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
                            is_tail: false,
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
            async_layout: None,
            suspension_barrier: false,
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
            async_layout: None,
            suspension_barrier: false,
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
            async_layout: None,
            suspension_barrier: false,
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
    fn test_async_ready_codegen() {
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
        entry_block.set_terminator(Terminator::Return { value: Some(task) });

        assert!(codegen.declare_function(&func).is_ok());
        assert!(codegen.define_function(&func, &std::collections::HashMap::new()).is_ok());
    }

    #[test]
    fn test_f32_gt_ge_rem_codegen() {
        use spectra_midend::ir::{FloatWidth, InstructionKind, Terminator, Value};

        let mut codegen = CodeGenerator::new();

        // Create function: fn f32_ops(a: f32, b: f32) -> f32
        let f32_ty = IRType::ExactFloat {
            width: FloatWidth::F32,
        };
        let mut func = IRFunction::new(
            "f32_ops",
            vec![
                Parameter {
                    id: 0,
                    name: "a".to_string(),
                    ty: f32_ty.clone(),
                },
                Parameter {
                    id: 1,
                    name: "b".to_string(),
                    ty: f32_ty.clone(),
                },
            ],
            f32_ty,
        );

        let entry_block_id = func.add_block("entry");
        let entry_block = func.get_block_mut(entry_block_id).unwrap();

        // gt = a > b (F32 operands must lower to fcmp, not icmp)
        let gt_value = Value { id: 2 };
        entry_block.add_instruction(InstructionKind::Gt {
            result: gt_value,
            lhs: Value { id: 0 },
            rhs: Value { id: 1 },
        });

        // ge = a >= b
        let ge_value = Value { id: 3 };
        entry_block.add_instruction(InstructionKind::Ge {
            result: ge_value,
            lhs: Value { id: 0 },
            rhs: Value { id: 1 },
        });

        // rem = a % b (F32 must lower to frem, not srem)
        let rem_value = Value { id: 4 };
        entry_block.add_instruction(InstructionKind::Rem {
            result: rem_value,
            lhs: Value { id: 0 },
            rhs: Value { id: 1 },
        });

        entry_block.set_terminator(Terminator::Return {
            value: Some(rem_value),
        });

        // declare + define finalizes the function through Cranelift's verifier.
        assert!(codegen.declare_function(&func).is_ok());
        assert!(codegen.define_function(&func, &std::collections::HashMap::new()).is_ok());
    }

    /// Builds `fn name() -> int { let q = 1 <op> 0; return q }` so the
    /// integer Div/Rem lowering must emit the zero-divisor check.
    fn int_divrem_by_zero_function(name: &str, is_remainder: bool) -> IRFunction {
        use spectra_midend::ir::{InstructionKind, Terminator, Value};

        let mut func = IRFunction::new(name, vec![], IRType::Int);
        let entry_block_id = func.add_block("entry");
        let entry_block = func.get_block_mut(entry_block_id).unwrap();
        let one = Value { id: 0 };
        let zero = Value { id: 1 };
        entry_block.add_instruction(InstructionKind::ConstInt {
            result: one,
            value: 1,
        });
        entry_block.add_instruction(InstructionKind::ConstInt {
            result: zero,
            value: 0,
        });
        let quotient = Value { id: 2 };
        if is_remainder {
            entry_block.add_instruction(InstructionKind::Rem {
                result: quotient,
                lhs: one,
                rhs: zero,
            });
        } else {
            entry_block.add_instruction(InstructionKind::Div {
                result: quotient,
                lhs: one,
                rhs: zero,
            });
        }
        entry_block.set_terminator(Terminator::Return {
            value: Some(quotient),
        });
        func
    }

    #[test]
    fn integer_div_by_zero_lowering_passes_verifier() {
        // The JIT lowering must finalize (Cranelift verifier) with the
        // zero-divisor branch and panic block in place.
        let mut codegen = CodeGenerator::new();
        let func = int_divrem_by_zero_function("div_by_zero", false);
        assert!(codegen.declare_function(&func).is_ok());
        assert!(codegen.define_function(&func, &std::collections::HashMap::new()).is_ok());

        let mut codegen = CodeGenerator::new();
        let func = int_divrem_by_zero_function("rem_by_zero", true);
        assert!(codegen.declare_function(&func).is_ok());
        assert!(codegen.define_function(&func, &std::collections::HashMap::new()).is_ok());
    }

    #[test]
    fn aot_div_and_rem_by_zero_reference_spectra_rt_panic() {
        for is_remainder in [false, true] {
            let name = if is_remainder {
                "aot_rem_by_zero"
            } else {
                "aot_div_by_zero"
            };
            let mut module = IRModule::new(name);
            module.add_function(int_divrem_by_zero_function(name, is_remainder));

            let bytes = crate::AotCodeGenerator::new()
                .compile_to_object(&module, &crate::AotOptions::default())
                .expect("AOT compile of div-by-zero module");
            let haystack = String::from_utf8_lossy(&bytes);
            // The object must carry the undefined import plus the interned
            // panic message literal.
            assert!(
                haystack.contains("spectra_rt_panic"),
                "AOT object does not reference spectra_rt_panic"
            );
            let expected_message = if is_remainder {
                "integer remainder by zero"
            } else {
                "integer division by zero"
            };
            // Panic literals are embedded as packed UTF-8 bytes with a
            // single-byte NUL terminator, so search for the raw byte pattern.
            let expected: Vec<u8> = expected_message
                .bytes()
                .chain(std::iter::once(0))
                .collect();
            assert!(
                bytes.windows(expected.len()).any(|window| window == expected),
                "AOT object does not embed the panic message"
            );
        }
    }
    // -----------------------------------------------------------------------
    // Self-tail-recursion → Cranelift `return_call` (Onda 3)
    // -----------------------------------------------------------------------

    /// Builds `fn loop_sum(n: int, acc: int) -> int`:
    ///   if n <= 0 { return acc; }
    ///   return loop_sum(n - 1, acc + n);  // marked tail self-call
    fn tail_recursion_loop_sum() -> IRFunction {
        let mut function = IRFunction::new(
            "loop_sum",
            vec![
                Parameter {
                    id: 0,
                    name: "n".to_string(),
                    ty: IRType::Int,
                },
                Parameter {
                    id: 1,
                    name: "acc".to_string(),
                    ty: IRType::Int,
                },
            ],
            IRType::Int,
        );
        let entry_block = function.add_block("entry");
        let n = IRValue { id: 0 };
        let acc = IRValue { id: 1 };

        // zero = 0; cond = n <= zero
        {
            let block = function.get_block_mut(entry_block).unwrap();
            block.add_instruction(InstructionKind::ConstInt {
                result: IRValue { id: 2 },
                value: 0,
            });
            block.add_instruction(InstructionKind::Le {
                result: IRValue { id: 3 },
                lhs: n,
                rhs: IRValue { id: 2 },
            });
        }

        let base = function.add_block("base");
        let step = function.add_block("step");
        function
            .get_block_mut(entry_block)
            .unwrap()
            .set_terminator(Terminator::CondBranch {
                condition: IRValue { id: 3 },
                true_block: base,
                false_block: step,
            });

        // base: return acc
        function
            .get_block_mut(base)
            .unwrap()
            .set_terminator(Terminator::Return { value: Some(acc) });

        // step: nm1 = n - 1; accn = acc + n; r = loop_sum(nm1, accn); return r
        {
            let block = function.get_block_mut(step).unwrap();
            block.add_instruction(InstructionKind::ConstInt {
                result: IRValue { id: 5 },
                value: 1,
            });
            block.add_instruction(InstructionKind::Sub {
                result: IRValue { id: 4 },
                lhs: n,
                rhs: IRValue { id: 5 },
            });
            block.add_instruction(InstructionKind::Add {
                result: IRValue { id: 6 },
                lhs: acc,
                rhs: n,
            });
            block.add_instruction(InstructionKind::Call {
                result: Some(IRValue { id: 7 }),
                function: "loop_sum".to_string(),
                args: vec![IRValue { id: 4 }, IRValue { id: 6 }],
                is_tail: true,
            });
            block.set_terminator(Terminator::Return {
                value: Some(IRValue { id: 7 }),
            });
        }
        function
    }

    #[test]
    fn tail_self_call_compiles_to_cranelift_return_call() {
        let mut codegen = CodeGenerator::new();
        let mut module = IRModule::new("tail_recursion");
        module.add_function(tail_recursion_loop_sum());
        codegen.pre_intern_host_names(&module);

        codegen.declare_function(&module.functions[0])
            .expect("declaration should succeed");
        let function_params: HashMap<String, Vec<IRType>> = HashMap::new();
        codegen
            .define_function(&module.functions[0], &function_params)
            .expect("definition should succeed");

        // Inspect the finalized Cranelift IR captured before the shared
        // context is cleared (see `last_finalized_func`).
        let func = codegen
            .last_finalized_func
            .as_ref()
            .expect("define_function should snapshot the finalized IR");
        let mut return_calls = 0;
        let mut plain_returns = 0;
        for block in func.layout.blocks() {
            for inst in func.layout.block_insts(block) {
                match func.dfg.insts[inst].opcode() {
                    cranelift_codegen::ir::Opcode::ReturnCall => return_calls += 1,
                    cranelift_codegen::ir::Opcode::Return => plain_returns += 1,
                    _ => {}
                }
            }
        }
        assert_eq!(return_calls, 1, "expected exactly one native return_call");
        assert!(
            plain_returns >= 1,
            "the base-case `ret` must survive as a normal return"
        );
        assert_eq!(func.signature.call_conv, isa::CallConv::Tail);
    }

    #[test]
    fn deep_tail_recursion_runs_without_stack_overflow() {
        let mut codegen = CodeGenerator::new();
        let mut module = IRModule::new("tail_recursion_run");

        // Recursive tail function.
        module.add_function(tail_recursion_loop_sum());

        // Platform-convention wrapper so the test can call through the normal
        // JIT entry ABI: wrapper(n, acc) = loop_sum(n, acc).
        let mut wrapper = IRFunction::new(
            "wrapper",
            vec![
                Parameter {
                    id: 0,
                    name: "n".to_string(),
                    ty: IRType::Int,
                },
                Parameter {
                    id: 1,
                    name: "acc".to_string(),
                    ty: IRType::Int,
                },
            ],
            IRType::Int,
        );
        let entry = wrapper.add_block("entry");
        wrapper
            .get_block_mut(entry)
            .unwrap()
            .add_instruction(InstructionKind::Call {
                result: Some(IRValue { id: 2 }),
                function: "loop_sum".to_string(),
                args: vec![IRValue { id: 0 }, IRValue { id: 1 }],
                is_tail: false,
            });
        wrapper
            .get_block_mut(entry)
            .unwrap()
            .set_terminator(Terminator::Return {
                value: Some(IRValue { id: 2 }),
            });
        module.add_function(wrapper);

        codegen.pre_intern_host_names(&module);
        for func in &module.functions {
            codegen.declare_function(func).expect("declare");
        }
        let function_params: HashMap<String, Vec<IRType>> = module
            .functions
            .iter()
            .map(|func| {
                (
                    func.name.clone(),
                    func.params.iter().map(|param| param.ty.clone()).collect(),
                )
            })
            .collect();
        for func in &module.functions {
            codegen.define_function(func, &function_params).expect("define");
        }
        codegen
            .module
            .finalize_definitions()
            .expect("finalize definitions");

        let wrapper_id = *codegen.function_map.get("wrapper").unwrap();
        let ptr = codegen.module.get_finalized_function(wrapper_id) as usize;
        let run: extern "C" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(ptr) };

        // ~100k recursion levels: without a native tail call this would blow
        // the stack long before finishing. Sum of 1..=100_000.
        const N: i64 = 100_000;
        let expected = N * (N + 1) / 2;
        assert_eq!(run(N, 0), expected);
    }
    #[test]
    fn aot_compiles_tail_self_recursion() {
        let mut module = IRModule::new("tail_recursion_aot");
        module.add_function(tail_recursion_loop_sum());
        let bytes = crate::AotCodeGenerator::new()
            .compile_to_object(&module, &crate::AotOptions::default())
            .expect("AOT compile of tail-recursive module");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn jit_sidecar_collected_and_written_when_env_requested() {
        use crate::debug::{JIT_DEBUG_ENV, write_jit_debug_sidecar};
        use spectra_midend::ir::{LocalDebugInfo, SourceSpan};

        // `main() -> int { let answer = 42; return answer; }`: the local is
        // returned, so Cranelift must prove at least one live range for its
        // labelled value.
        let mut function = IRFunction::new("main", vec![], IRType::Int);
        let entry = function.add_block("entry");
        function
            .get_block_mut(entry)
            .unwrap()
            .add_instruction(InstructionKind::ConstInt {
                result: IRValue { id: 1 },
                value: 42,
            });
        let block = function.get_block_mut(entry).unwrap();
        block.set_terminator(Terminator::Return {
            value: Some(IRValue { id: 1 }),
        });
        function.locals.push(LocalDebugInfo {
            name: "answer".to_string(),
            ty: IRType::Int,
            value_id: Some(1),
            declaration: Some(SourceSpan {
                file: "fixture.spectra".to_string(),
                start_line: 3,
                start_column: 9,
                end_line: 3,
                end_column: 24,
            }),
            scope_start: None,
            scope_end: None,
        });

        std::env::set_var(JIT_DEBUG_ENV, "1");
        let mut codegen = CodeGenerator::new();
        assert!(codegen.declare_function(&function).is_ok());
        assert!(
            codegen
                .define_function(&function, &std::collections::HashMap::new())
                .is_ok()
        );
        let collected = codegen.take_jit_debug_functions();

        assert_eq!(collected.len(), 1, "main must report sidecar variables");
        assert_eq!(collected[0].0, "main");
        let vars = &collected[0].1;
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].name, "answer");
        assert_eq!(vars[0].type_name, "int");
        assert_eq!(vars[0].line, Some(3));
        assert!(
            !vars[0].ranges.is_empty(),
            "Cranelift must prove at least one live range for the labelled local"
        );

        let source = std::env::temp_dir().join(format!(
            "spectra_jit_sidecar_test_{}.spectra",
            std::process::id()
        ));
        let source = source.to_str().unwrap();
        let written = write_jit_debug_sidecar(source, &collected, false)
            .expect("sidecar write must not fail")
            .expect("SPECTRA_JIT_DEBUG=1 must produce the sidecar");
        assert_eq!(
            written,
            crate::debug::jit_debug_sidecar_path(source),
            "sidecar is written next to the source"
        );
        let contents = std::fs::read_to_string(&written).unwrap();
        assert!(contents.contains("\"main\""), "{contents}");
        assert!(contents.contains("\"name\":\"answer\""), "{contents}");
        assert!(contents.contains("\"line\":3"), "{contents}");
        assert!(contents.contains("\"ranges\":["), "{contents}");
        std::fs::remove_file(&written).ok();
        std::env::remove_var(JIT_DEBUG_ENV);
    }
}
