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
                            bound: None,
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
    fn returning_a_field_pointer_escapes_the_stack_alloca() {
        // The address of a field of a stack aggregate is as escape-worthy as
        // the aggregate itself: without `FieldPtr` propagation the root alloca
        // stayed in `stack_allocas` while the returned pointer outlived the
        // frame (dangling).
        let function = IRFunction {
            name: "escaping_field".to_string(),
            params: vec![],
            return_type: IRType::Array {
                element_type: Box::new(IRType::Int),
                size: 4,
            },
            source_span: None,
            locals: vec![],
            async_layout: None,
            suspension_barrier: false,
            next_value_id: 2,
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
                        kind: InstructionKind::FieldPtr {
                            result: IRValue { id: 1 },
                            ptr: IRValue { id: 0 },
                            offset: 0,
                        },
                        source_span: None,
                    },
                ],
                terminator: Some(Terminator::Return {
                    value: Some(IRValue { id: 1 }),
                }),
            }],
        };

        let stack_allocas = CodeGenerator::collect_stack_allocas(&function);
        assert!(
            !stack_allocas.contains(&0),
            "returning a field address must demote the root alloca off the stack frame"
        );
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
                            bound: None,
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
                            bound: None,
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
    fn cyclic_aggregate_construction_is_not_stack_promoted() {
        // A struct literal built inside a loop body must get fresh storage per
        // iteration: promoted to one fixed stack slot it would make every
        // stored pointer alias the latest construction.
        let element_type = IRType::Struct {
            name: "Handle".to_string(),
            fields: vec![],
        };
        let array_type = IRType::Array {
            element_type: Box::new(element_type.clone()),
            size: 2,
        };
        let function = IRFunction {
            name: "cyclic_construction".to_string(),
            params: vec![],
            return_type: IRType::Void,
            source_span: None,
            locals: vec![],
            async_layout: None,
            suspension_barrier: false,
            next_value_id: 4,
            next_block_id: 2,
            blocks: vec![
                IRBasicBlock {
                    id: 0,
                    label: "entry".to_string(),
                    instructions: vec![
                        Instruction {
                            id: 0,
                            kind: InstructionKind::Alloca {
                                result: IRValue { id: 0 },
                                ty: array_type,
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
                                element_type: element_type.clone(),
                                bound: None,
                            },
                            source_span: None,
                        },
                    ],
                    terminator: Some(Terminator::Branch { target: 1 }),
                },
                IRBasicBlock {
                    id: 1,
                    label: "loop.body".to_string(),
                    instructions: vec![
                        Instruction {
                            id: 3,
                            kind: InstructionKind::Alloca {
                                result: IRValue { id: 3 },
                                ty: element_type,
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
                    terminator: Some(Terminator::Branch { target: 1 }),
                },
            ],
        };

        let stack_allocas = CodeGenerator::collect_stack_allocas(&function);
        assert!(
            stack_allocas.contains(&0),
            "the array container itself stays stack-promoted"
        );
        assert!(
            !stack_allocas.contains(&3),
            "per-iteration construction must be demoted to the manual heap"
        );
    }

    #[test]
    fn straight_line_aggregate_construction_stays_stack_promoted() {
        // The same construction outside a cycle executes once, so its stack
        // slot cannot be reused by a later iteration and stays promoted.
        let element_type = IRType::Struct {
            name: "Handle".to_string(),
            fields: vec![],
        };
        let array_type = IRType::Array {
            element_type: Box::new(element_type.clone()),
            size: 2,
        };
        let function = IRFunction {
            name: "straight_line_construction".to_string(),
            params: vec![],
            return_type: IRType::Void,
            source_span: None,
            locals: vec![],
            async_layout: None,
            suspension_barrier: false,
            next_value_id: 4,
            next_block_id: 2,
            blocks: vec![
                IRBasicBlock {
                    id: 0,
                    label: "entry".to_string(),
                    instructions: vec![
                        Instruction {
                            id: 0,
                            kind: InstructionKind::Alloca {
                                result: IRValue { id: 0 },
                                ty: array_type,
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
                                element_type: element_type.clone(),
                                bound: None,
                            },
                            source_span: None,
                        },
                        Instruction {
                            id: 3,
                            kind: InstructionKind::Alloca {
                                result: IRValue { id: 3 },
                                ty: element_type,
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
                    terminator: Some(Terminator::Return { value: None }),
                },
            ],
        };

        let stack_allocas = CodeGenerator::collect_stack_allocas(&function);
        assert!(stack_allocas.contains(&0));
        assert!(stack_allocas.contains(&3));
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
            unsigned: false,
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
            unsigned: false,
        });

        // ge = a >= b
        let ge_value = Value { id: 3 };
        entry_block.add_instruction(InstructionKind::Ge {
            result: ge_value,
            lhs: Value { id: 0 },
            rhs: Value { id: 1 },
            unsigned: false,
        });

        // rem = a % b (F32 must lower to frem, not srem)
        let rem_value = Value { id: 4 };
        entry_block.add_instruction(InstructionKind::Rem {
            result: rem_value,
            lhs: Value { id: 0 },
            rhs: Value { id: 1 },
            unsigned: false,
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
                unsigned: false,
            });
        } else {
            entry_block.add_instruction(InstructionKind::Div {
                result: quotient,
                lhs: one,
                rhs: zero,
                unsigned: false,
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

    /// Builds `fn name() -> int { alloca [4 x int]; return base[1]; }` with a
    /// static bound so the backend must emit the OOB panic path.
    fn bounded_gep_function(name: &str) -> IRFunction {
        use spectra_midend::ir::{ArrayBound, InstructionKind, Terminator, Value};

        let mut func = IRFunction::new(name, vec![], IRType::Int);
        let entry_block_id = func.add_block("entry");
        let entry_block = func.get_block_mut(entry_block_id).unwrap();
        let base = Value { id: 0 };
        entry_block.add_instruction(InstructionKind::Alloca {
            result: base,
            ty: IRType::Array {
                element_type: Box::new(IRType::Int),
                size: 4,
            },
        });
        let index = Value { id: 1 };
        entry_block.add_instruction(InstructionKind::ConstInt {
            result: index,
            value: 1,
        });
        let elem_ptr = Value { id: 2 };
        entry_block.add_instruction(InstructionKind::GetElementPtr {
            result: elem_ptr,
            ptr: base,
            index,
            element_type: IRType::Int,
            bound: Some(ArrayBound::Static(4)),
        });
        let loaded = Value { id: 3 };
        entry_block.add_instruction(InstructionKind::Load {
            result: loaded,
            ptr: elem_ptr,
            ty: IRType::Int,
        });
        entry_block.set_terminator(Terminator::Return {
            value: Some(loaded),
        });
        func
    }

    #[test]
    fn bounded_gep_lowering_passes_verifier() {
        // The JIT lowering must finalize (Cranelift verifier) with the
        // bounds-check branch and panic block in place.
        let mut codegen = CodeGenerator::new();
        let func = bounded_gep_function("bounded_gep");
        assert!(codegen.declare_function(&func).is_ok());
        assert!(codegen.define_function(&func, &std::collections::HashMap::new()).is_ok());
    }

    #[test]
    fn aot_bounded_gep_references_spectra_rt_panic() {
        let name = "aot_bounded_gep";
        let mut module = IRModule::new(name);
        module.add_function(bounded_gep_function(name));

        let bytes = crate::AotCodeGenerator::new()
            .compile_to_object(&module, &crate::AotOptions::default())
            .expect("AOT compile of bounded-gep module");
        let haystack = String::from_utf8_lossy(&bytes);
        assert!(
            haystack.contains("spectra_rt_panic"),
            "AOT object does not reference spectra_rt_panic"
        );
        // Panic literals are embedded as packed UTF-8 bytes with a
        // single-byte NUL terminator, so search for the raw byte pattern.
        let expected: Vec<u8> = "array index out of bounds"
            .bytes()
            .chain(std::iter::once(0))
            .collect();
        assert!(
            bytes.windows(expected.len()).any(|window| window == expected),
            "AOT object does not embed the panic message"
        );
    }

    /// Builds `fn name(len: int) -> int { alloca [4 x int]; return base[len]; }`
    /// with a dynamic bound (the hidden-size parameter path), so the backend
    /// must compare against a runtime length instead of a constant.
    fn dynamic_bounded_gep_function(name: &str) -> IRFunction {
        use spectra_midend::ir::{ArrayBound, InstructionKind, Parameter, Terminator, Value};

        let mut func = IRFunction::new(
            name,
            vec![Parameter {
                id: 0,
                name: "len".to_string(),
                ty: IRType::Int,
            }],
            IRType::Int,
        );
        let entry_block_id = func.add_block("entry");
        let entry_block = func.get_block_mut(entry_block_id).unwrap();
        let base = Value { id: 1 };
        entry_block.add_instruction(InstructionKind::Alloca {
            result: base,
            ty: IRType::Array {
                element_type: Box::new(IRType::Int),
                size: 4,
            },
        });
        let elem_ptr = Value { id: 2 };
        entry_block.add_instruction(InstructionKind::GetElementPtr {
            result: elem_ptr,
            ptr: base,
            index: Value { id: 0 },
            element_type: IRType::Int,
            bound: Some(ArrayBound::Dynamic(Value { id: 0 })),
        });
        let loaded = Value { id: 3 };
        entry_block.add_instruction(InstructionKind::Load {
            result: loaded,
            ptr: elem_ptr,
            ty: IRType::Int,
        });
        entry_block.set_terminator(Terminator::Return {
            value: Some(loaded),
        });
        func
    }

    #[test]
    fn dynamic_bounded_gep_lowering_passes_verifier() {
        // The hidden-length bound must lower through the same checked path as
        // the static bound: verifier-clean JIT output with the panic block.
        let mut codegen = CodeGenerator::new();
        let func = dynamic_bounded_gep_function("dynamic_bounded_gep");
        assert!(codegen.declare_function(&func).is_ok());
        assert!(codegen.define_function(&func, &std::collections::HashMap::new()).is_ok());
    }

    #[test]
    fn aot_dynamic_bounded_gep_references_spectra_rt_panic() {
        let name = "aot_dynamic_bounded_gep";
        let mut module = IRModule::new(name);
        module.add_function(dynamic_bounded_gep_function(name));

        let bytes = crate::AotCodeGenerator::new()
            .compile_to_object(&module, &crate::AotOptions::default())
            .expect("AOT compile of dynamic bounded-gep module");
        let haystack = String::from_utf8_lossy(&bytes);
        assert!(
            haystack.contains("spectra_rt_panic"),
            "AOT object does not reference spectra_rt_panic"
        );
        let expected: Vec<u8> = "array index out of bounds"
            .bytes()
            .chain(std::iter::once(0))
            .collect();
        assert!(
            bytes.windows(expected.len()).any(|window| window == expected),
            "AOT object does not embed the panic message"
        );
    }

    #[test]
    fn backend_error_does_not_poison_builder_context() {
        use spectra_midend::ir::{InstructionKind, Terminator, Value};

        let mut codegen = CodeGenerator::new();
        // Broken on purpose: the return references a value no instruction
        // defines, so lowering fails *after* its FunctionBuilder exists.
        let mut broken = IRFunction::new("broken_fn", vec![], IRType::Int);
        let entry = broken.add_block("entry");
        broken
            .get_block_mut(entry)
            .unwrap()
            .set_terminator(Terminator::Return {
                value: Some(Value { id: 99 }),
            });
        assert!(codegen.declare_function(&broken).is_ok());
        assert!(codegen
            .define_function(&broken, &std::collections::HashMap::new())
            .is_err());
        // The next definition reuses the same generator: without a context
        // reset this panics inside `FunctionBuilder::new` (debug_assert).
        let mut good = IRFunction::new("good_fn", vec![], IRType::Int);
        let g_entry = good.add_block("entry");
        let g_block = good.get_block_mut(g_entry).unwrap();
        g_block.add_instruction(InstructionKind::ConstInt {
            result: Value { id: 0 },
            value: 7,
        });
        g_block.set_terminator(Terminator::Return {
            value: Some(Value { id: 0 }),
        });
        assert!(codegen.declare_function(&good).is_ok());
        assert!(codegen
            .define_function(&good, &std::collections::HashMap::new())
            .is_ok());
    }

    /// Builds `fn name() -> int { return host(-7) }` with a generic host call
    /// so the lowering emits both the generic failure panic and the
    /// capability-denial branch.
    fn generic_host_call_function(name: &str) -> IRFunction {
        use spectra_midend::ir::{InstructionKind, Terminator, Value};

        let mut func = IRFunction::new(name, vec![], IRType::Int);
        let entry_block_id = func.add_block("entry");
        let entry_block = func.get_block_mut(entry_block_id).unwrap();
        let arg = Value { id: 0 };
        entry_block.add_instruction(InstructionKind::ConstInt {
            result: arg,
            value: -7,
        });
        let result = Value { id: 1 };
        entry_block.add_instruction(InstructionKind::HostCall {
            result: Some(result),
            host: "spectra.std.math.abs".to_string(),
            args: vec![arg],
            result_type: Some(IRType::Int),
        });
        entry_block.set_terminator(Terminator::Return {
            value: Some(result),
        });
        func
    }

    #[test]
    fn aot_generic_host_call_references_capability_denial_symbol() {
        let name = "aot_capability_denied";
        let mut module = IRModule::new(name);
        module.add_function(generic_host_call_function(name));

        let bytes = crate::AotCodeGenerator::new()
            .compile_to_object(&module, &crate::AotOptions::default())
            .expect("AOT compile of generic host-call module");
        let haystack = String::from_utf8_lossy(&bytes);
        // HOST_STATUS_DENIED must branch to the dedicated fatal symbol rather
        // than reusing the generic `spectra_rt_panic` failure path.
        assert!(
            haystack.contains("spectra_rt_capability_denied"),
            "AOT object does not reference spectra_rt_capability_denied"
        );
        assert!(
            haystack.contains("spectra_rt_panic"),
            "AOT object does not reference spectra_rt_panic"
        );
        // The denial message is the call-site text without the failure suffix.
        let expected: Vec<u8> = "host call 'spectra.std.math.abs'"
            .bytes()
            .chain(std::iter::once(0))
            .collect();
        assert!(
            bytes.windows(expected.len()).any(|window| window == expected),
            "AOT object does not embed the denial message"
        );
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
                unsigned: false,
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

    /// Midend block order regression: the tail-call block (`step`) precedes
    /// the plain-return block (`base`) in the block list, exactly like a
    /// source-level `if cond { return f(...) } return acc` lowers. Before the
    /// per-block reset of `emitted_tail_call`, the tail call in `step` leaked
    /// into `base` and suppressed its `return`, leaving the block unfilled.
    fn tail_recursion_loop_sum_tail_block_first() -> IRFunction {
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
                unsigned: false,
            });
        }

        // The recursive step is registered before the base case, which is the
        // order the midend emits for a trailing return after the `if`.
        let step = function.add_block("step");
        let base = function.add_block("base");
        function
            .get_block_mut(entry_block)
            .unwrap()
            .set_terminator(Terminator::CondBranch {
                condition: IRValue { id: 3 },
                true_block: base,
                false_block: step,
            });

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
            .get_block_mut(base)
            .unwrap()
            .set_terminator(Terminator::Return { value: Some(acc) });
        function
    }

    #[test]
    fn tail_call_block_does_not_suppress_later_block_return() {
        let mut codegen = CodeGenerator::new();
        let mut module = IRModule::new("tail_recursion_ordered");

        // Recursive function plus platform-ABI wrapper, mirroring the deep
        // recursion test so the compiled result can be executed.
        module.add_function(tail_recursion_loop_sum_tail_block_first());
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
        {
            let block = wrapper.get_block_mut(entry).unwrap();
            block.add_instruction(InstructionKind::Call {
                result: Some(IRValue { id: 2 }),
                function: "loop_sum".to_string(),
                args: vec![IRValue { id: 0 }, IRValue { id: 1 }],
                is_tail: false,
            });
            block.set_terminator(Terminator::Return {
                value: Some(IRValue { id: 2 }),
            });
        }
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
        let loop_sum = module
            .functions
            .iter()
            .find(|func| func.name == "loop_sum")
            .expect("loop_sum")
            .clone();
        let wrapper = module
            .functions
            .iter()
            .find(|func| func.name == "wrapper")
            .expect("wrapper")
            .clone();

        // Defining `loop_sum` used to panic in `FunctionBuilder::finalize`
        // ("block is not filled") for the base block.
        codegen
            .define_function(&loop_sum, &function_params)
            .expect("define loop_sum");

        // The base block's plain return must survive next to the native tail
        // call emitted for the recursive step, even though the step block is
        // generated first.
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

        codegen
            .define_function(&wrapper, &function_params)
            .expect("define wrapper");
        codegen
            .module
            .finalize_definitions()
            .expect("finalize definitions");

        let wrapper_id = *codegen.function_map.get("wrapper").unwrap();
        let ptr = codegen.module.get_finalized_function(wrapper_id) as usize;
        let run: extern "C" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(ptr) };

        // Small case: sum of 0..=5.
        assert_eq!(run(5, 0), 15);
        // Deep case: the recursive call must still be a native tail call, so
        // 100k levels must not overflow the stack.
        const N: i64 = 100_000;
        assert_eq!(run(N, 0), N * (N + 1) / 2);
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

    // -----------------------------------------------------------------------
    // Signedness-sensitive arithmetic (exact-width unsigned integers)
    // -----------------------------------------------------------------------

    fn finalized_icc_conditions(func: &cranelift_codegen::ir::Function) -> Vec<IntCC> {
        let mut conditions = Vec::new();
        for block in func.layout.blocks() {
            for inst in func.layout.block_insts(block) {
                if let Some(cond) = func.dfg.insts[inst].cond_code() {
                    conditions.push(cond);
                }
            }
        }
        conditions
    }

    fn finalized_opcodes(func: &cranelift_codegen::ir::Function) -> Vec<cranelift_codegen::ir::Opcode>
    {
        let mut opcodes = Vec::new();
        for block in func.layout.blocks() {
            for inst in func.layout.block_insts(block) {
                opcodes.push(func.dfg.insts[inst].opcode());
            }
        }
        opcodes
    }

    /// Whether the function compares a value against `expected` with an
    /// integer `icmp` — matching both the immediate form (`icmp_imm`) and
    /// the canonicalized register form (`iconst` + `icmp`), since Cranelift's
    /// preopt rewrites one into the other.
    fn has_icmp_imm(func: &cranelift_codegen::ir::Function, expected: i64) -> bool {
        use cranelift_codegen::ir::{InstructionData, Opcode};
        let mut iconsts: HashMap<cranelift_codegen::ir::Value, i64> = HashMap::new();
        for block in func.layout.blocks() {
            for inst in func.layout.block_insts(block) {
                if let InstructionData::UnaryImm { opcode, imm } = &func.dfg.insts[inst] {
                    if *opcode == Opcode::Iconst {
                        if let Some(&result) = func.dfg.inst_results(inst).first() {
                            iconsts.insert(result, imm.bits());
                        }
                    }
                }
            }
        }
        for block in func.layout.blocks() {
            for inst in func.layout.block_insts(block) {
                match &func.dfg.insts[inst] {
                    InstructionData::IntCompareImm { imm, .. } if imm.bits() == expected => {
                        return true;
                    }
                    InstructionData::IntCompare { args, .. } => {
                        for &arg in args.iter() {
                            if iconsts.get(&arg) == Some(&expected) {
                                return true;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        false
    }

    /// `Lt { unsigned: true }` on exact-width unsigned operands must lower to
    /// an unsigned `icmp` condition; `unsigned: false` must keep the signed
    /// one. Before this, both emitted `SignedLessThan` unconditionally and
    /// misordered values above the signed maximum.
    #[test]
    fn unsigned_exact_int_comparison_selects_the_icc_signedness() {
        use spectra_midend::ir::IntWidth;

        let build = |name: &str, unsigned: bool| {
            let mut func = IRFunction::new(
                name,
                vec![
                    Parameter {
                        id: 0,
                        name: "a".to_string(),
                        ty: IRType::ExactInt {
                            signed: false,
                            width: IntWidth::I8,
                        },
                    },
                    Parameter {
                        id: 1,
                        name: "b".to_string(),
                        ty: IRType::ExactInt {
                            signed: false,
                            width: IntWidth::I8,
                        },
                    },
                ],
                IRType::Bool,
            );
            let entry = func.add_block("entry");
            func.get_block_mut(entry)
                .unwrap()
                .add_instruction(InstructionKind::Lt {
                    result: IRValue { id: 2 },
                    lhs: IRValue { id: 0 },
                    rhs: IRValue { id: 1 },
                    unsigned,
                });
            func.get_block_mut(entry)
                .unwrap()
                .set_terminator(Terminator::Return {
                    value: Some(IRValue { id: 2 }),
                });
            func
        };

        let mut codegen = CodeGenerator::new();
        let func = build("cmp_unsigned", true);
        codegen.declare_function(&func).expect("declare unsigned");
        codegen
            .define_function(&func, &HashMap::new())
            .expect("define unsigned");
        let finalized = codegen
            .last_finalized_func
            .as_ref()
            .expect("finalized IR snapshot");
        let conditions = finalized_icc_conditions(finalized);
        assert!(
            conditions.contains(&IntCC::UnsignedLessThan),
            "unsigned Lt must emit UnsignedLessThan, got {conditions:?}"
        );
        assert!(
            !conditions.contains(&IntCC::SignedLessThan),
            "unsigned Lt must not emit SignedLessThan, got {conditions:?}"
        );

        let mut codegen = CodeGenerator::new();
        let func = build("cmp_signed", false);
        codegen.declare_function(&func).expect("declare signed");
        codegen
            .define_function(&func, &HashMap::new())
            .expect("define signed");
        let finalized = codegen
            .last_finalized_func
            .as_ref()
            .expect("finalized IR snapshot");
        let conditions = finalized_icc_conditions(finalized);
        assert!(
            conditions.contains(&IntCC::SignedLessThan),
            "signed Lt must emit SignedLessThan, got {conditions:?}"
        );
    }

    /// Unsigned `Div`/`Rem` must lower to `udiv`/`urem` (and keep the
    /// zero-divisor panic), while signed division must emit the `MIN / -1`
    /// overflow guard alongside the zero check: `sdiv(MIN, -1)` raises a
    /// native `#DE`/SIGFPE instead of the runtime's exit-101 panic without
    /// it.
    #[test]
    fn unsigned_division_emits_udiv_and_signed_division_guards_min_overflow() {
        use spectra_midend::ir::IntWidth;

        let build = |name: &str, width: IntWidth, signed: bool, unsigned_flag: bool| {
            let mut func = IRFunction::new(
                name,
                vec![
                    Parameter {
                        id: 0,
                        name: "a".to_string(),
                        ty: IRType::ExactInt { signed, width },
                    },
                    Parameter {
                        id: 1,
                        name: "b".to_string(),
                        ty: IRType::ExactInt { signed, width },
                    },
                ],
                IRType::ExactInt { signed, width },
            );
            let entry = func.add_block("entry");
            func.get_block_mut(entry)
                .unwrap()
                .add_instruction(InstructionKind::Div {
                    result: IRValue { id: 2 },
                    lhs: IRValue { id: 0 },
                    rhs: IRValue { id: 1 },
                    unsigned: unsigned_flag,
                });
            func.get_block_mut(entry)
                .unwrap()
                .set_terminator(Terminator::Return {
                    value: Some(IRValue { id: 2 }),
                });
            func
        };

        // Unsigned 64-bit division -> udiv, no signed-overflow guard.
        let mut codegen = CodeGenerator::new();
        let func = build(
            "div_u64",
            IntWidth::I64,
            false,
            true,
        );
        codegen.declare_function(&func).expect("declare u64");
        codegen
            .define_function(&func, &HashMap::new())
            .expect("define u64");
        codegen
            .module
            .finalize_definitions()
            .expect("udiv must legalize");
        let finalized = codegen.last_finalized_func.as_ref().unwrap();
        let opcodes = finalized_opcodes(finalized);
        assert!(
            opcodes.contains(&cranelift_codegen::ir::Opcode::Udiv),
            "unsigned Div must emit udiv, got {opcodes:?}"
        );
        assert!(
            !opcodes.contains(&cranelift_codegen::ir::Opcode::Sdiv),
            "unsigned Div must not emit sdiv, got {opcodes:?}"
        );

        // Narrow (u8) unsigned division: same contract at I8 width.
        let mut codegen = CodeGenerator::new();
        let func = build("div_u8", IntWidth::I8, false, true);
        codegen.declare_function(&func).expect("declare u8");
        codegen
            .define_function(&func, &HashMap::new())
            .expect("define u8");
        codegen
            .module
            .finalize_definitions()
            .expect("narrow udiv must legalize");
        let finalized = codegen.last_finalized_func.as_ref().unwrap();
        assert!(finalized_opcodes(finalized).contains(
            &cranelift_codegen::ir::Opcode::Udiv
        ));

        // Signed division keeps sdiv and gains the MIN / -1 guard
        // (icmp rhs == -1, icmp lhs == i64::MIN) plus the zero check.
        let mut codegen = CodeGenerator::new();
        let func = build("div_i64", IntWidth::I64, true, false);
        codegen.declare_function(&func).expect("declare i64");
        codegen
            .define_function(&func, &HashMap::new())
            .expect("define i64");
        let finalized = codegen.last_finalized_func.as_ref().unwrap();
        let opcodes = finalized_opcodes(finalized);
        assert!(
            opcodes.contains(&cranelift_codegen::ir::Opcode::Sdiv),
            "signed Div must emit sdiv, got {opcodes:?}"
        );
        assert!(
            has_icmp_imm(finalized, -1),
            "signed Div must guard rhs == -1 (the MIN / -1 overflow divisor)"
        );
        assert!(
            has_icmp_imm(finalized, i64::MIN),
            "signed Div must guard lhs == signed MIN"
        );
    }

    /// The signed overflow case must reach the *runtime* panic, so the AOT
    /// object embeds the "integer division overflow" literal next to the
    /// zero-divisor ones.
    #[test]
    fn aot_signed_division_embeds_the_overflow_panic_literal() {
        let mut module = IRModule::new("aot_div_overflow_literal");
        let mut func = IRFunction::new("divide", vec![], IRType::Int);
        let entry = func.add_block("entry");
        func.get_block_mut(entry)
            .unwrap()
            .add_instruction(InstructionKind::Div {
                result: IRValue { id: 2 },
                lhs: IRValue { id: 0 },
                rhs: IRValue { id: 1 },
                unsigned: false,
            });
        // Feed the operands from parameters so nothing constant-folds.
        func.params = vec![
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
        ];
        func.next_value_id = 3;
        func.get_block_mut(entry)
            .unwrap()
            .set_terminator(Terminator::Return {
                value: Some(IRValue { id: 2 }),
            });
        module.add_function(func);

        let bytes = crate::AotCodeGenerator::new()
            .compile_to_object(&module, &crate::AotOptions::default())
            .expect("AOT compile of signed division module");
        for expected in [
            "integer division overflow",
            "integer division by zero",
        ] {
            let needle: Vec<u8> = expected.bytes().chain(std::iter::once(0)).collect();
            assert!(
                bytes.windows(needle.len()).any(|w| w == needle),
                "AOT object does not embed the {expected:?} panic literal"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Tail-call convention eligibility (CallConv::Tail ABI safety)
    // -----------------------------------------------------------------------

    fn trivial_main() -> IRFunction {
        let mut func = IRFunction::new("main", vec![], IRType::Int);
        let entry = func.add_block("entry");
        func.get_block_mut(entry)
            .unwrap()
            .add_instruction(InstructionKind::ConstInt {
                result: IRValue { id: 0 },
                value: 0,
            });
        func.get_block_mut(entry)
            .unwrap()
            .set_terminator(Terminator::Return {
                value: Some(IRValue { id: 0 }),
            });
        func
    }

    /// The entry module keeps `CallConv::Tail` for a clean self-tail-recursive
    /// function: deep recursion must still compile to a native `return_call`.
    #[test]
    fn entry_module_keeps_tail_convention_for_self_tail_recursion() {
        let mut codegen = CodeGenerator::new();
        let mut module = IRModule::new("entry_tail");
        module.add_function(tail_recursion_loop_sum());
        module.add_function(trivial_main());
        codegen
            .generate_module(&module)
            .expect("entry module must generate");

        let func_id = *codegen
            .function_map
            .get("entry_tail::loop_sum")
            .expect("loop_sum declared");
        let signature = &codegen
            .module
            .declarations()
            .get_function_decl(func_id)
            .signature;
        assert_eq!(
            signature.call_conv,
            isa::CallConv::Tail,
            "clean self-tail-recursion in the entry module keeps Tail"
        );
    }

    /// A function whose address is taken anywhere in the module must keep the
    /// platform default: `CallIndirect` rebuilds its signature with
    /// `module.make_signature()`, so a Tail callee would be entered with the
    /// wrong argument-register placement (the `let g = fib; g(10)` /
    /// vtable shapes).
    #[test]
    fn address_taken_function_is_not_tail_eligible() {
        let mut codegen = CodeGenerator::new();
        let default_conv = codegen.module.make_signature().call_conv;

        let mut module = IRModule::new("closure_tail");
        module.add_function(tail_recursion_loop_sum());

        // main: `let g = loop_sum; return g(5, 0)` as FuncAddr + CallIndirect.
        let mut main = IRFunction::new("main", vec![], IRType::Int);
        let entry = main.add_block("entry");
        {
            let block = main.get_block_mut(entry).unwrap();
            block.add_instruction(InstructionKind::ConstInt {
                result: IRValue { id: 0 },
                value: 5,
            });
            block.add_instruction(InstructionKind::ConstInt {
                result: IRValue { id: 1 },
                value: 0,
            });
            block.add_instruction(InstructionKind::FuncAddr {
                result: IRValue { id: 2 },
                function: "loop_sum".to_string(),
            });
            block.add_instruction(InstructionKind::CallIndirect {
                result: Some(IRValue { id: 3 }),
                fn_ptr: IRValue { id: 2 },
                args: vec![IRValue { id: 0 }, IRValue { id: 1 }],
                signature_params: vec![IRType::Int, IRType::Int],
                signature_return: Box::new(IRType::Int),
            });
            block.set_terminator(Terminator::Return {
                value: Some(IRValue { id: 3 }),
            });
        }
        main.next_value_id = 4;
        module.add_function(main);

        codegen
            .generate_module(&module)
            .expect("address-taken module must generate");

        let func_id = *codegen
            .function_map
            .get("closure_tail::loop_sum")
            .expect("loop_sum declared");
        let signature = &codegen
            .module
            .declarations()
            .get_function_decl(func_id)
            .signature;
        assert_eq!(
            signature.call_conv, default_conv,
            "an address-taken function must keep the default convention so \
             CallIndirect's default signature matches"
        );
    }

    /// Library modules (no `main`) are referenced by later modules through
    /// names this module cannot see (external lists, cross-module vtables), so
    /// they never use Tail in the JIT either.
    #[test]
    fn library_module_without_main_disables_tail_convention() {
        let mut codegen = CodeGenerator::new();
        let default_conv = codegen.module.make_signature().call_conv;

        let mut module = IRModule::new("library_tail");
        module.add_function(tail_recursion_loop_sum());
        codegen
            .generate_module(&module)
            .expect("library module must generate");

        let func_id = *codegen
            .function_map
            .get("library_tail::loop_sum")
            .expect("loop_sum declared");
        let signature = &codegen
            .module
            .declarations()
            .get_function_decl(func_id)
            .signature;
        assert_eq!(
            signature.call_conv, default_conv,
            "library modules fall back to plain calls instead of Tail"
        );
    }

    // -----------------------------------------------------------------------
    // Async f32 code generation (frame store/load + AsyncReady)
    // -----------------------------------------------------------------------

    /// `FrameStore` used to emit `uextend(I64, f32)` (illegal on floats) and
    /// `AsyncReady` used to emit `bitcast(F64, f32)` (width mismatch). The
    /// round trip must lower, verify, execute, and preserve the f32 value.
    #[test]
    fn f32_frame_roundtrip_and_async_ready_execute() {
        use spectra_midend::ir::FloatWidth;

        let f32_ty = IRType::ExactFloat {
            width: FloatWidth::F32,
        };
        let mut func = IRFunction::new("async_f32_roundtrip", vec![], f32_ty.clone());
        let entry = func.add_block("entry");
        {
            let block = func.get_block_mut(entry).unwrap();
            block.add_instruction(InstructionKind::FrameAlloc {
                result: IRValue { id: 0 },
                layout: "test_f32_frame".to_string(),
                slot_count: 1,
            });
            block.add_instruction(InstructionKind::ConstFloatTyped {
                result: IRValue { id: 1 },
                value: 1.5,
                ty: f32_ty.clone(),
            });
            block.add_instruction(InstructionKind::FrameStore {
                frame: IRValue { id: 0 },
                slot: 0,
                value: IRValue { id: 1 },
                escape_value: false,
            });
            block.add_instruction(InstructionKind::FrameLoad {
                result: IRValue { id: 2 },
                frame: IRValue { id: 0 },
                slot: 0,
                ty: f32_ty.clone(),
            });
            // Exercise the AsyncReady float conversion (fpromote F32 -> F64).
            block.add_instruction(InstructionKind::AsyncReady {
                result: IRValue { id: 3 },
                value: Some(IRValue { id: 2 }),
                output_type: f32_ty.clone(),
            });
            block.set_terminator(Terminator::Return {
                value: Some(IRValue { id: 2 }),
            });
        }
        func.next_value_id = 4;

        let mut codegen = CodeGenerator::new();
        codegen.pre_intern_host_names_for_test(&func);
        codegen.declare_function(&func).expect("declare");
        codegen
            .define_function(&func, &HashMap::new())
            .expect("f32 frame codegen must verify");
        codegen
            .module
            .finalize_definitions()
            .expect("f32 frame codegen must compile");
        let func_id = *codegen.function_map.get("async_f32_roundtrip").unwrap();
        let ptr = codegen.module.get_finalized_function(func_id) as usize;
        let run: extern "C" fn() -> f32 = unsafe { std::mem::transmute(ptr) };
        assert_eq!(run(), 1.5, "the f32 payload must survive the frame round trip");
    }

    // -----------------------------------------------------------------------
    // String/array logical length maps (StringLen / char_at)
    // -----------------------------------------------------------------------

    fn run_i64_function(codegen: &mut CodeGenerator, name: &str) -> i64 {
        codegen
            .module
            .finalize_definitions()
            .unwrap_or_else(|e| panic!("{name}: finalize failed: {e}"));
        let func_id = *codegen
            .function_map
            .get(name)
            .unwrap_or_else(|| panic!("{name} declared"));
        let ptr = codegen.module.get_finalized_function(func_id) as usize;
        let run: extern "C" fn() -> i64 = unsafe { std::mem::transmute(ptr) };
        run()
    }

    fn push_char_stores(
        func: &mut IRFunction,
        base_id: usize,
        chars: &[u8],
        first_value_id: &mut usize,
    ) {
        use spectra_midend::ir::IntWidth;
        let packed_byte = IRType::ExactInt {
            signed: false,
            width: IntWidth::I8,
        };
        let entry = func.blocks[0].id;
        for (slot, byte) in chars.iter().enumerate() {
            let index_id = *first_value_id;
            *first_value_id += 1;
            let gep_id = *first_value_id;
            *first_value_id += 1;
            let const_id = *first_value_id;
            *first_value_id += 1;
            let block = func.get_block_mut(entry).unwrap();
            block.add_instruction(InstructionKind::ConstInt {
                result: IRValue { id: index_id },
                value: slot as i64,
            });
            block.add_instruction(InstructionKind::GetElementPtr {
                result: IRValue { id: gep_id },
                ptr: IRValue { id: base_id },
                index: IRValue { id: index_id },
                // `char_at` addresses buffers as packed bytes (stride 1), so
                // the test array uses the language's packed byte layout
                // (`array<u8>`), whose elements record into `array_lengths`
                // like `Char` arrays do.
                element_type: packed_byte.clone(),
                bound: None,
            });
            block.add_instruction(InstructionKind::ConstIntTyped {
                result: IRValue { id: const_id },
                value: *byte as i64,
                ty: packed_byte.clone(),
            });
            block.add_instruction(InstructionKind::Store {
                ptr: IRValue { id: gep_id },
                value: IRValue { id: const_id },
            });
        }
    }

    /// Emit `result = len * 1_000_000 + first * 10_000 + last * 100 +
    /// (out_of_bounds + 1)` so one i64 return carries every assertion:
    /// `out_of_bounds` must be -1 (its `+1` term then vanishes).
    fn emit_packed_len_char_at_assertions(
        func: &mut IRFunction,
        ptr_id: usize,
        len_id: usize,
        first_id: usize,
        last_id: usize,
        oob_id: usize,
        next_value_id: &mut usize,
    ) -> usize {
        let entry = func.blocks[0].id;
        let emit_const = |func: &mut IRFunction, value: i64, next: &mut usize| {
            let id = *next;
            *next += 1;
            func.get_block_mut(entry)
                .unwrap()
                .add_instruction(InstructionKind::ConstInt {
                    result: IRValue { id },
                    value,
                });
            id
        };
        let binop = |func: &mut IRFunction,
                     make: fn(IRValue, IRValue, IRValue) -> InstructionKind,
                     lhs: usize,
                     rhs: usize,
                     next: &mut usize| {
            let id = *next;
            *next += 1;
            func.get_block_mut(entry).unwrap().add_instruction(make(
                IRValue { id },
                IRValue { id: lhs },
                IRValue { id: rhs },
            ));
            id
        };

        let million = emit_const(func, 1_000_000, next_value_id);
        let len_scaled = binop(
            func,
            |result, lhs, rhs| InstructionKind::Mul { result, lhs, rhs },
            len_id,
            million,
            next_value_id,
        );
        let ten_thousand = emit_const(func, 10_000, next_value_id);
        let first_scaled = binop(
            func,
            |result, lhs, rhs| InstructionKind::Mul { result, lhs, rhs },
            first_id,
            ten_thousand,
            next_value_id,
        );
        let partial = binop(
            func,
            |result, lhs, rhs| InstructionKind::Add { result, lhs, rhs },
            len_scaled,
            first_scaled,
            next_value_id,
        );
        let hundred = emit_const(func, 100, next_value_id);
        let last_scaled = binop(
            func,
            |result, lhs, rhs| InstructionKind::Mul { result, lhs, rhs },
            last_id,
            hundred,
            next_value_id,
        );
        let with_last = binop(
            func,
            |result, lhs, rhs| InstructionKind::Add { result, lhs, rhs },
            partial,
            last_scaled,
            next_value_id,
        );
        let one = emit_const(func, 1, next_value_id);
        let oob_adjusted = binop(
            func,
            |result, lhs, rhs| InstructionKind::Add { result, lhs, rhs },
            oob_id,
            one,
            next_value_id,
        );
        let packed = binop(
            func,
            |result, lhs, rhs| InstructionKind::Add { result, lhs, rhs },
            with_last,
            oob_adjusted,
            next_value_id,
        );
        let _ = ptr_id;
        packed
    }

    fn char_array_len_char_at_function(name: &str, chars: &[u8], escape_via_container: bool) -> IRFunction {
        let mut func = IRFunction::new(name, vec![], IRType::Int);
        let base_id = 0usize;
        let mut next = 1usize;
        {
            let entry_id = func.add_block("entry");
            let block = func.get_block_mut(entry_id).unwrap();
            block.add_instruction(InstructionKind::Alloca {
                result: IRValue { id: base_id },
                ty: IRType::Array {
                    element_type: Box::new(IRType::ExactInt {
                        signed: false,
                        width: spectra_midend::ir::IntWidth::I8,
                    }),
                    size: chars.len(),
                },
            });
            next += 1;
            if escape_via_container {
                // Store the array pointer into a manual-heap container so the
                // array itself must use the manual-heap alloca path even
                // before the host-call arguments below escape it.
                let container_id = next;
                next += 1;
                block.add_instruction(InstructionKind::ManualAlloc {
                    result: IRValue { id: container_id },
                    size: 8,
                });
                block.add_instruction(InstructionKind::Store {
                    ptr: IRValue { id: container_id },
                    value: IRValue { id: base_id },
                });
            }
        }
        push_char_stores(&mut func, base_id, chars, &mut next);

        let entry = func.blocks[0].id;
        let len_id = next;
        next += 1;
        func.get_block_mut(entry)
            .unwrap()
            .add_instruction(InstructionKind::HostCall {
                result: Some(IRValue { id: len_id }),
                host: "spectra.std.string.len".to_string(),
                args: vec![IRValue { id: base_id }],
                result_type: Some(IRType::Int),
            });

        let char_at = |func: &mut IRFunction, index: i64, next: &mut usize| -> (usize, usize) {
            let index_id = *next;
            *next += 1;
            let result_id = *next;
            *next += 1;
            let entry = func.blocks[0].id;
            func.get_block_mut(entry).unwrap().add_instruction(
                InstructionKind::ConstInt {
                    result: IRValue { id: index_id },
                    value: index,
                },
            );
            func.get_block_mut(entry).unwrap().add_instruction(
                InstructionKind::HostCall {
                    result: Some(IRValue { id: result_id }),
                    host: "spectra.std.string.char_at".to_string(),
                    args: vec![IRValue { id: base_id }, IRValue { id: index_id }],
                    result_type: Some(IRType::Int),
                },
            );
            (index_id, result_id)
        };
        let (_, first_id) = char_at(&mut func, 0, &mut next);
        let last_index = chars.len() as i64 - 1;
        let (_, last_id) = char_at(&mut func, last_index, &mut next);
        let (_, oob_id) = char_at(&mut func, chars.len() as i64, &mut next);

        let packed = emit_packed_len_char_at_assertions(
            &mut func,
            base_id,
            len_id,
            first_id,
            last_id,
            oob_id,
            &mut next,
        );
        func.get_block_mut(entry)
            .unwrap()
            .set_terminator(Terminator::Return {
                value: Some(IRValue { id: packed }),
            });
        func.next_value_id = next + 1;
        func
    }

    /// Char array length + `char_at` including the LAST valid index
    /// (`N - 1`) and one-past-the-end returning -1. Both the stack-style and
    /// the explicitly-escaping shapes must report the logical element count
    /// (no off-by-one): before this, `StringLen` returned `N - 1` and
    /// `char_at(N - 1)` returned -1.
    #[test]
    fn char_array_len_and_char_at_use_the_logical_element_count() {
        let chars = [b'a', b'b', b'c', b'd'];

        for (name, escape) in [
            ("stack_initialized_char_array", false),
            ("heap_escaping_char_array", true),
        ] {
            let func = char_array_len_char_at_function(name, &chars, escape);
            let mut codegen = CodeGenerator::new();
            codegen.pre_intern_host_names_for_test(&func);
            codegen.declare_function(&func).expect("declare");
            codegen
                .define_function(&func, &HashMap::new())
                .expect("define");
            let packed = run_i64_function(&mut codegen, name);
            // len=4, 'a'=97, 'd'=100, oob=-1 -> 4*1e6 + 97*1e4 + 100*100 + 0
            let expected = 4 * 1_000_000 + 97 * 10_000 + 100 * 100;
            assert_eq!(
                packed, expected,
                "{name}: wrong len/char_at result (last index must be valid, \
                 one-past must be -1)"
            );
        }
    }

    /// String literals store the logical byte length (excluding the NUL);
    /// `StringLen` returns it directly and `char_at` accepts the last byte
    /// index (`len - 1`) while rejecting `len`.
    #[test]
    fn string_literal_len_and_char_at_use_the_logical_byte_length() {
        let mut func = IRFunction::new("string_literal_measures", vec![], IRType::Int);
        let entry = func.add_block("entry");
        let mut next = 1usize;
        {
            let block = func.get_block_mut(entry).unwrap();
            block.add_instruction(InstructionKind::ConstString {
                result: IRValue { id: 0 },
                value: "hello".to_string(),
            });
            let len_id = next;
            next += 1;
            block.add_instruction(InstructionKind::HostCall {
                result: Some(IRValue { id: len_id }),
                host: "spectra.std.string.len".to_string(),
                args: vec![IRValue { id: 0 }],
                result_type: Some(IRType::Int),
            });
        }

        let char_at = |func: &mut IRFunction, index: i64, next: &mut usize| -> usize {
            let index_id = *next;
            *next += 1;
            let result_id = *next;
            *next += 1;
            let block = func.get_block_mut(entry).unwrap();
            block.add_instruction(InstructionKind::ConstInt {
                result: IRValue { id: index_id },
                value: index,
            });
            block.add_instruction(InstructionKind::HostCall {
                result: Some(IRValue { id: result_id }),
                host: "spectra.std.string.char_at".to_string(),
                args: vec![IRValue { id: 0 }, IRValue { id: index_id }],
                result_type: Some(IRType::Int),
            });
            result_id
        };
        let len_id = 1usize;
        let first_id = char_at(&mut func, 0, &mut next);
        let last_id = char_at(&mut func, 4, &mut next);
        let oob_id = char_at(&mut func, 5, &mut next);

        let packed = emit_packed_len_char_at_assertions(
            &mut func,
            0,
            len_id,
            first_id,
            last_id,
            oob_id,
            &mut next,
        );
        func.get_block_mut(entry)
            .unwrap()
            .set_terminator(Terminator::Return {
                value: Some(IRValue { id: packed }),
            });
        func.next_value_id = next + 1;

        let mut codegen = CodeGenerator::new();
        codegen.pre_intern_host_names_for_test(&func);
        codegen.declare_function(&func).expect("declare");
        codegen
            .define_function(&func, &HashMap::new())
            .expect("define");
        let packed = run_i64_function(&mut codegen, "string_literal_measures");
        // len=5, 'h'=104, 'o'=111, oob=-1 -> 5*1e6 + 104*1e4 + 111*100 + 0
        let expected = 5 * 1_000_000 + 104 * 10_000 + 111 * 100;
        assert_eq!(
            packed, expected,
            "string literal len/char_at must use the logical byte length"
        );
    }

    // -----------------------------------------------------------------------
    // PHI block parameters generated in reverse-postorder (item 10)
    // -----------------------------------------------------------------------

    /// The IR lists the merge block *before* the block that jumps into it.
    /// With reverse-postorder emission the jump is generated first and its
    /// bool argument types the phi parameter; previously the merge padded an
    /// I64 placeholder and the later `jump i8 -> i64` failed the Cranelift
    /// verifier.
    #[test]
    fn phi_merge_listed_before_its_predecessor_types_params_from_the_jump() {
        let mut func = IRFunction::new("merge_before_pred", vec![], IRType::Bool);
        func.next_value_id = 3;
        func.next_block_id = 3;

        let entry = IRBasicBlock {
            id: 0,
            label: "entry".to_string(),
            instructions: vec![],
            terminator: Some(Terminator::Branch { target: 2 }),
        };
        let merge = IRBasicBlock {
            id: 1,
            label: "merge".to_string(),
            instructions: vec![Instruction {
                id: 0,
                kind: InstructionKind::Phi {
                    result: IRValue { id: 1 },
                    incoming: vec![(IRValue { id: 0 }, 2)],
                },
                source_span: None,
            }],
            terminator: Some(Terminator::Return {
                value: Some(IRValue { id: 1 }),
            }),
        };
        let pred = IRBasicBlock {
            id: 2,
            label: "pred".to_string(),
            instructions: vec![Instruction {
                id: 0,
                kind: InstructionKind::ConstBool {
                    result: IRValue { id: 0 },
                    value: true,
                },
                source_span: None,
            }],
            terminator: Some(Terminator::Branch { target: 1 }),
        };
        func.blocks = vec![entry, merge, pred];

        let mut codegen = CodeGenerator::new();
        codegen.declare_function(&func).expect("declare");
        codegen
            .define_function(&func, &HashMap::new())
            .expect("define: the bool phi must be typed from the first jump");
        codegen
            .module
            .finalize_definitions()
            .expect("compile");
        let func_id = *codegen.function_map.get("merge_before_pred").unwrap();
        let ptr = codegen.module.get_finalized_function(func_id) as usize;
        let run: extern "C" fn() -> i8 = unsafe { std::mem::transmute(ptr) };
        assert_ne!(run(), 0, "the phi must carry the incoming bool value");
    }
}
