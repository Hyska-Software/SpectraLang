/// Tests for optimization passes on IR
use spectra_midend::ir::*;
use spectra_midend::passes::constant_folding;
use spectra_midend::passes::dead_code_elimination;
use spectra_midend::passes::function_inlining;

#[test]
fn test_constant_folding_add() {
    // Create a simple module with: x = 5 + 3
    let mut module = Module {
        name: "test".to_string(),
        functions: vec![Function {
            name: "test_func".to_string(),
            params: vec![],
            return_type: Type::Void,
            next_value_id: 0,
            next_block_id: 1,
            source_span: None,
            locals: vec![],
            async_layout: None,
            suspension_barrier: false,
            blocks: vec![BasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![
                    Instruction {
                        id: 0,
                        kind: InstructionKind::ConstInt {
                            result: Value { id: 0 },
                            value: 5,
                        },

                        source_span: None,
                    },
                    Instruction {
                        id: 1,
                        kind: InstructionKind::ConstInt {
                            result: Value { id: 1 },
                            value: 3,
                        },

                        source_span: None,
                    },
                    Instruction {
                        id: 2,
                        kind: InstructionKind::Add {
                            result: Value { id: 2 },
                            lhs: Value { id: 0 },
                            rhs: Value { id: 1 },
                        },

                        source_span: None,
                    },
                ],
                terminator: Some(Terminator::Return { value: None }),
            }],
        }],
        external_functions: vec![],
        globals: vec![],
        source_file: None,
    };

    // Apply constant folding
    let modified = constant_folding::run(&mut module);

    assert!(modified, "Constant folding should modify the module");

    // Check that Add was replaced with ConstInt(8)
    let func = &module.functions[0];
    let block = &func.blocks[0];

    let has_const_8 = block
        .instructions
        .iter()
        .any(|instr| matches!(instr.kind, InstructionKind::ConstInt { value: 8, .. }));

    assert!(has_const_8, "Should have ConstInt(8) after folding");
}

#[test]
fn test_constant_folding_mul() {
    // Create: x = 10 * 2
    let mut module = Module {
        name: "test".to_string(),
        functions: vec![Function {
            name: "test_func".to_string(),
            params: vec![],
            return_type: Type::Void,
            next_value_id: 0,
            next_block_id: 1,
            source_span: None,
            locals: vec![],
            async_layout: None,
            suspension_barrier: false,
            blocks: vec![BasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![
                    Instruction {
                        id: 0,
                        kind: InstructionKind::ConstInt {
                            result: Value { id: 0 },
                            value: 10,
                        },

                        source_span: None,
                    },
                    Instruction {
                        id: 1,
                        kind: InstructionKind::ConstInt {
                            result: Value { id: 1 },
                            value: 2,
                        },

                        source_span: None,
                    },
                    Instruction {
                        id: 2,
                        kind: InstructionKind::Mul {
                            result: Value { id: 2 },
                            lhs: Value { id: 0 },
                            rhs: Value { id: 1 },
                        },

                        source_span: None,
                    },
                ],
                terminator: Some(Terminator::Return { value: None }),
            }],
        }],
        external_functions: vec![],
        globals: vec![],
        source_file: None,
    };

    let modified = constant_folding::run(&mut module);
    assert!(modified, "Constant folding should modify the module");

    let func = &module.functions[0];
    let block = &func.blocks[0];

    let has_const_20 = block
        .instructions
        .iter()
        .any(|instr| matches!(instr.kind, InstructionKind::ConstInt { value: 20, .. }));

    assert!(has_const_20, "Should have ConstInt(20) after folding 10*2");
}

#[test]
fn test_constant_folding_reaches_a_fixed_point_for_chains() {
    let mut function = Function::new("constant_chain", Vec::new(), Type::Void);
    let entry = function.add_block("entry");
    // Keep normal source ordering and SSA IDs for the chain.
    let mut builder = spectra_midend::builder::IRBuilder::new();
    builder.set_current_block(entry);
    let five = builder.build_const_int(&mut function, 5);
    let three = builder.build_const_int(&mut function, 3);
    let sum = builder.build_add(&mut function, five, three);
    let two = builder.build_const_int(&mut function, 2);
    let product = builder.build_mul(&mut function, sum, two);
    builder.build_return(&mut function, None);

    let mut module = Module::new("constant_chain");
    module.add_function(function);
    assert!(constant_folding::run(&mut module));

    assert!(module.functions[0].blocks[0].instructions.iter().any(|instruction| {
        matches!(
            instruction.kind,
            InstructionKind::ConstInt {
                result: Value { id },
                value: 16
            } if id == product.id
        )
    }));
}

#[test]
fn test_dce_preserves_coroutine_payload_operands() {
    let mut function = Function::new("coroutine_payload", Vec::new(), Type::Void);
    let entry = function.add_block("entry");
    let mut builder = spectra_midend::builder::IRBuilder::new();
    builder.set_current_block(entry);
    let task = builder.build_const_int(&mut function, 7);
    let payload = builder.build_const_int(&mut function, 42);
    if let Some(block) = function.get_block_mut(entry) {
        block.add_instruction(InstructionKind::CoroutineComplete {
            task,
            value: Some(payload),
        });
        block.set_terminator(Terminator::Return { value: None });
    }

    let mut module = Module::new("coroutine_payload");
    module.add_function(function);
    dead_code_elimination::run(&mut module);

    let instructions = &module.functions[0].blocks[0].instructions;
    assert!(instructions.iter().any(|instruction| matches!(
        instruction.kind,
        InstructionKind::ConstInt {
            result: Value { id },
            value: 7
        } if id == task.id
    )));
    assert!(instructions.iter().any(|instruction| matches!(
        instruction.kind,
        InstructionKind::ConstInt {
            result: Value { id },
            value: 42
        } if id == payload.id
    )));
}

#[test]
fn test_dead_code_elimination_basic() {
    // Create module with unused computation
    let mut module = Module {
        name: "test".to_string(),
        functions: vec![Function {
            name: "test_func".to_string(),
            params: vec![],
            return_type: Type::Void,
            next_value_id: 0,
            next_block_id: 1,
            source_span: None,
            locals: vec![],
            async_layout: None,
            suspension_barrier: false,
            blocks: vec![BasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![
                    Instruction {
                        id: 0,
                        kind: InstructionKind::ConstInt {
                            result: Value { id: 0 },
                            value: 10,
                        },

                        source_span: None,
                    },
                    Instruction {
                        id: 1,
                        kind: InstructionKind::ConstInt {
                            result: Value { id: 1 },
                            value: 20,
                        },

                        source_span: None,
                    },
                    Instruction {
                        id: 2,
                        kind: InstructionKind::Add {
                            result: Value { id: 2 },
                            lhs: Value { id: 0 },
                            rhs: Value { id: 1 },
                        },

                        source_span: None,
                    },
                    // Result is never used - all code is dead
                ],
                terminator: Some(Terminator::Return { value: None }),
            }],
        }],
        external_functions: vec![],
        globals: vec![],
        source_file: None,
    };

    let initial_count = module.functions[0].blocks[0].instructions.len();

    let modified = dead_code_elimination::run(&mut module);
    assert!(modified, "DCE should modify the module");

    let final_count = module.functions[0].blocks[0].instructions.len();
    assert!(
        final_count < initial_count,
        "DCE should remove unused instructions"
    );
}

#[test]
fn test_dead_code_elimination_preserves_used() {
    // Create module with used value
    let mut module = Module {
        name: "test".to_string(),
        functions: vec![Function {
            name: "test_func".to_string(),
            params: vec![],
            return_type: Type::Int,
            next_value_id: 0,
            next_block_id: 1,
            source_span: None,
            locals: vec![],
            async_layout: None,
            suspension_barrier: false,
            blocks: vec![BasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![Instruction {
                    id: 0,
                    kind: InstructionKind::ConstInt {
                        result: Value { id: 0 },
                        value: 42,
                    },

                    source_span: None,
                }],
                terminator: Some(Terminator::Return {
                    value: Some(Value { id: 0 }),
                }),
            }],
        }],
        external_functions: vec![],
        globals: vec![],
        source_file: None,
    };

    let initial_count = module.functions[0].blocks[0].instructions.len();

    let _modified = dead_code_elimination::run(&mut module);

    let final_count = module.functions[0].blocks[0].instructions.len();
    assert_eq!(
        final_count, initial_count,
        "DCE should preserve used values"
    );
}

#[test]
fn test_combined_optimizations() {
    // Test constant folding followed by DCE
    let mut module = Module {
        name: "test".to_string(),
        functions: vec![Function {
            name: "test_func".to_string(),
            params: vec![],
            return_type: Type::Void,
            next_value_id: 0,
            next_block_id: 1,
            source_span: None,
            locals: vec![],
            async_layout: None,
            suspension_barrier: false,
            blocks: vec![BasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![
                    Instruction {
                        id: 0,
                        kind: InstructionKind::ConstInt {
                            result: Value { id: 0 },
                            value: 5,
                        },

                        source_span: None,
                    },
                    Instruction {
                        id: 1,
                        kind: InstructionKind::ConstInt {
                            result: Value { id: 1 },
                            value: 3,
                        },

                        source_span: None,
                    },
                    Instruction {
                        id: 2,
                        kind: InstructionKind::Add {
                            result: Value { id: 2 },
                            lhs: Value { id: 0 },
                            rhs: Value { id: 1 },
                        },

                        source_span: None,
                    },
                ],
                terminator: Some(Terminator::Return { value: None }),
            }],
        }],
        external_functions: vec![],
        globals: vec![],
        source_file: None,
    };

    // First pass: constant folding
    let cf_modified = constant_folding::run(&mut module);
    assert!(cf_modified, "Constant folding should apply");

    // Second pass: dead code elimination
    let dce_modified = dead_code_elimination::run(&mut module);
    assert!(dce_modified, "DCE should remove folded constants");

    // Result should have minimal instructions
    let final_count = module.functions[0].blocks[0].instructions.len();
    assert!(
        final_count == 0,
        "Combined optimizations should eliminate all dead code"
    );
}

#[test]
fn test_no_optimization_when_not_applicable() {
    // Test that passes don't modify code unnecessarily
    let mut module = Module {
        name: "test".to_string(),
        functions: vec![Function {
            name: "test_func".to_string(),
            params: vec![],
            return_type: Type::Int,
            next_value_id: 0,
            next_block_id: 1,
            source_span: None,
            locals: vec![],
            async_layout: None,
            suspension_barrier: false,
            blocks: vec![BasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![
                    // Non-constant operation
                    Instruction {
                        id: 0,
                        kind: InstructionKind::Add {
                            result: Value { id: 2 },
                            lhs: Value { id: 0 }, // Function parameter
                            rhs: Value { id: 1 }, // Function parameter
                        },

                        source_span: None,
                    },
                ],
                terminator: Some(Terminator::Return {
                    value: Some(Value { id: 2 }),
                }),
            }],
        }],
        external_functions: vec![],
        globals: vec![],
        source_file: None,
    };

    let cf_modified = constant_folding::run(&mut module);
    assert!(!cf_modified, "Constant folding should not apply");

    let dce_modified = dead_code_elimination::run(&mut module);
    assert!(!dce_modified, "DCE should not remove used value");
}

#[test]
fn test_dead_code_elimination_preserves_cast_operands() {
    let mut module = Module {
        name: "test".to_string(),
        functions: vec![Function {
            name: "test_func".to_string(),
            params: vec![],
            return_type: Type::Char,
            next_value_id: 0,
            next_block_id: 1,
            source_span: None,
            locals: vec![],
            async_layout: None,
            suspension_barrier: false,
            blocks: vec![BasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![
                    Instruction {
                        id: 0,
                        kind: InstructionKind::ConstInt {
                            result: Value { id: 0 },
                            value: 65,
                        },

                        source_span: None,
                    },
                    Instruction {
                        id: 1,
                        kind: InstructionKind::Cast {
                            result: Value { id: 1 },
                            operand: Value { id: 0 },
                            from_ty: Type::Int,
                            to_ty: Type::Char,
                        },

                        source_span: None,
                    },
                ],
                terminator: Some(Terminator::Return {
                    value: Some(Value { id: 1 }),
                }),
            }],
        }],
        external_functions: vec![],
        globals: vec![],
        source_file: None,
    };

    let modified = dead_code_elimination::run(&mut module);
    assert!(
        !modified,
        "DCE should preserve cast chains that feed a return"
    );

    let instructions = &module.functions[0].blocks[0].instructions;
    assert_eq!(instructions.len(), 2, "Cast operand and cast must remain");
    assert!(matches!(
        instructions[0].kind,
        InstructionKind::ConstInt { value: 65, .. }
    ));
    assert!(matches!(instructions[1].kind, InstructionKind::Cast { .. }));
}

#[test]
fn test_function_inlining_remaps_parameters() {
    let mut module = Module {
        name: "test".to_string(),
        functions: vec![
            Function {
                name: "add_pair".to_string(),
                params: vec![
                    Parameter {
                        id: 0,
                        name: "lhs".to_string(),
                        ty: Type::Int,
                    },
                    Parameter {
                        id: 1,
                        name: "rhs".to_string(),
                        ty: Type::Int,
                    },
                ],
                return_type: Type::Int,
                next_value_id: 3,
                next_block_id: 1,
                source_span: None,
                locals: vec![],
                async_layout: None,
                suspension_barrier: false,
                blocks: vec![BasicBlock {
                    id: 0,
                    label: "entry".to_string(),
                    instructions: vec![Instruction {
                        id: 0,
                        kind: InstructionKind::Add {
                            result: Value { id: 2 },
                            lhs: Value { id: 0 },
                            rhs: Value { id: 1 },
                        },

                        source_span: None,
                    }],
                    terminator: Some(Terminator::Return {
                        value: Some(Value { id: 2 }),
                    }),
                }],
            },
            Function {
                name: "main".to_string(),
                params: vec![],
                return_type: Type::Int,
                next_value_id: 3,
                next_block_id: 1,
                source_span: None,
                locals: vec![],
                async_layout: None,
                suspension_barrier: false,
                blocks: vec![BasicBlock {
                    id: 0,
                    label: "entry".to_string(),
                    instructions: vec![
                        Instruction {
                            id: 0,
                            kind: InstructionKind::ConstInt {
                                result: Value { id: 0 },
                                value: 20,
                            },

                            source_span: None,
                        },
                        Instruction {
                            id: 1,
                            kind: InstructionKind::ConstInt {
                                result: Value { id: 1 },
                                value: 22,
                            },

                            source_span: None,
                        },
                        Instruction {
                            id: 2,
                            kind: InstructionKind::Call {
                                result: Some(Value { id: 2 }),
                                function: "add_pair".to_string(),
                                args: vec![Value { id: 0 }, Value { id: 1 }],
                                is_tail: false,
                            },

                            source_span: None,
                        },
                    ],
                    terminator: Some(Terminator::Return {
                        value: Some(Value { id: 2 }),
                    }),
                }],
            },
        ],
        external_functions: vec![],
        globals: vec![],
        source_file: None,
    };

    let modified = function_inlining::run(&mut module);
    assert!(modified, "parameterized helper should inline");

    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main function");

    assert!(
        main.blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .all(|instruction| !matches!(instruction.kind, InstructionKind::Call { .. })),
        "inlined main must not retain the helper call"
    );

    assert!(
        main.blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .any(|instruction| matches!(
                instruction.kind,
                InstructionKind::Add {
                    lhs: Value { id: 0 },
                    rhs: Value { id: 1 },
                    ..
                }
            )),
        "callee parameters must be remapped to call-site arguments"
    );
}

#[test]
fn test_function_inlining_allows_stack_safe_alloca_helpers() {
    let mut module = Module {
        name: "test".to_string(),
        functions: vec![
            Function {
                name: "store_sum".to_string(),
                params: vec![
                    Parameter {
                        id: 0,
                        name: "lhs".to_string(),
                        ty: Type::Int,
                    },
                    Parameter {
                        id: 1,
                        name: "rhs".to_string(),
                        ty: Type::Int,
                    },
                ],
                return_type: Type::Int,
                next_value_id: 5,
                next_block_id: 1,
                source_span: None,
                locals: vec![],
                async_layout: None,
                suspension_barrier: false,
                blocks: vec![BasicBlock {
                    id: 0,
                    label: "entry".to_string(),
                    instructions: vec![
                        Instruction {
                            id: 0,
                            kind: InstructionKind::Alloca {
                                result: Value { id: 2 },
                                ty: Type::Int,
                            },

                            source_span: None,
                        },
                        Instruction {
                            id: 1,
                            kind: InstructionKind::Add {
                                result: Value { id: 3 },
                                lhs: Value { id: 0 },
                                rhs: Value { id: 1 },
                            },

                            source_span: None,
                        },
                        Instruction {
                            id: 2,
                            kind: InstructionKind::Store {
                                ptr: Value { id: 2 },
                                value: Value { id: 3 },
                            },

                            source_span: None,
                        },
                        Instruction {
                            id: 3,
                            kind: InstructionKind::Load {
                                result: Value { id: 4 },
                                ptr: Value { id: 2 },
                                ty: Type::Int,
                            },

                            source_span: None,
                        },
                    ],
                    terminator: Some(Terminator::Return {
                        value: Some(Value { id: 4 }),
                    }),
                }],
            },
            Function {
                name: "main".to_string(),
                params: vec![],
                return_type: Type::Int,
                next_value_id: 3,
                next_block_id: 1,
                source_span: None,
                locals: vec![],
                async_layout: None,
                suspension_barrier: false,
                blocks: vec![BasicBlock {
                    id: 0,
                    label: "entry".to_string(),
                    instructions: vec![
                        Instruction {
                            id: 0,
                            kind: InstructionKind::ConstInt {
                                result: Value { id: 0 },
                                value: 20,
                            },

                            source_span: None,
                        },
                        Instruction {
                            id: 1,
                            kind: InstructionKind::ConstInt {
                                result: Value { id: 1 },
                                value: 22,
                            },

                            source_span: None,
                        },
                        Instruction {
                            id: 2,
                            kind: InstructionKind::Call {
                                result: Some(Value { id: 2 }),
                                function: "store_sum".to_string(),
                                args: vec![Value { id: 0 }, Value { id: 1 }],
                                is_tail: false,
                            },

                            source_span: None,
                        },
                    ],
                    terminator: Some(Terminator::Return {
                        value: Some(Value { id: 2 }),
                    }),
                }],
            },
        ],
        external_functions: vec![],
        globals: vec![],
        source_file: None,
    };

    let modified = function_inlining::run(&mut module);
    assert!(modified, "stack-local helper should inline");

    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main function");
    let instructions: Vec<_> = main
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .collect();

    assert!(
        instructions
            .iter()
            .all(|instruction| !matches!(instruction.kind, InstructionKind::Call { .. })),
        "inlined main must not retain the helper call"
    );
    assert!(
        instructions
            .iter()
            .any(|instruction| matches!(instruction.kind, InstructionKind::Alloca { .. })),
        "stack-safe alloca should not block inlining"
    );
}

#[test]
fn test_function_inlining_remaps_typed_and_string_constants() {
    let mut helper = Function::new("typed_constants", Vec::new(), Type::Int);
    let helper_entry = helper.add_block("entry");
    if let Some(block) = helper.get_block_mut(helper_entry) {
        block.add_instruction(InstructionKind::ConstIntTyped {
            result: Value { id: 0 },
            value: 7,
            ty: Type::Int,
        });
        block.add_instruction(InstructionKind::ConstString {
            result: Value { id: 1 },
            value: "unused".into(),
        });
        block.set_terminator(Terminator::Return {
            value: Some(Value { id: 0 }),
        });
    }

    let mut caller = Function::new("main", Vec::new(), Type::Int);
    let caller_entry = caller.add_block("entry");
    if let Some(block) = caller.get_block_mut(caller_entry) {
        block.add_instruction(InstructionKind::Call {
            result: Some(Value { id: 0 }),
            function: "typed_constants".into(),
            args: Vec::new(),
            is_tail: false,
        });
        block.set_terminator(Terminator::Return {
            value: Some(Value { id: 0 }),
        });
    }
    caller.next_value_id = 1;

    let mut module = Module::new("typed_constants");
    module.add_function(helper);
    module.add_function(caller);

    assert!(function_inlining::run(&mut module));
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main function");
    let instructions: Vec<_> = main
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .collect();

    assert!(instructions
        .iter()
        .all(|instruction| !matches!(instruction.kind, InstructionKind::Call { .. })));
    assert!(instructions.iter().any(|instruction| matches!(
        instruction.kind,
        InstructionKind::ConstIntTyped {
            result: Value { id },
            value: 7,
            ..
        } if id != 0
    )));
    assert!(instructions.iter().any(|instruction| matches!(
        instruction.kind,
        InstructionKind::ConstString {
            result: Value { id },
            ref value,
        } if id != 0 && value == "unused"
    )));
}

#[test]
fn test_dce_preserves_autodiff_step_with_unused_result() {
    // AutodiffStep has side effects (accumulates gradients in the backward
    // pass), so it must survive DCE even when `result` is None/unused and the
    // Add result is only consumed by the AutodiffStep itself.
    let mut module = Module {
        name: "test".to_string(),
        functions: vec![Function {
            name: "test_func".to_string(),
            params: vec![],
            return_type: Type::Void,
            next_value_id: 0,
            next_block_id: 1,
            source_span: None,
            locals: vec![],
            async_layout: None,
            suspension_barrier: false,
            blocks: vec![BasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![
                    Instruction {
                        id: 0,
                        kind: InstructionKind::ConstInt {
                            result: Value { id: 0 },
                            value: 5,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 1,
                        kind: InstructionKind::ConstInt {
                            result: Value { id: 1 },
                            value: 3,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 2,
                        kind: InstructionKind::Add {
                            result: Value { id: 2 },
                            lhs: Value { id: 0 },
                            rhs: Value { id: 1 },
                        },
                        source_span: None,
                    },
                    // Reverse-mode autodiff step: result unused, but the step
                    // accumulates gradients — must never be eliminated.
                    Instruction {
                        id: 3,
                        kind: InstructionKind::AutodiffStep {
                            result: None,
                            operation: "add".to_string(),
                            output: Value { id: 2 },
                            upstream: None,
                            inputs: vec![Value { id: 0 }, Value { id: 1 }],
                            targets: vec![],
                        },
                        source_span: None,
                    },
                ],
                terminator: Some(Terminator::Return { value: None }),
            }],
        }],
        external_functions: vec![],
        globals: vec![],
        source_file: None,
    };

    let modified = dead_code_elimination::run(&mut module);
    assert!(!modified, "DCE must not remove side-effecting AutodiffStep");

    let instructions = &module.functions[0].blocks[0].instructions;
    assert_eq!(
        instructions.len(),
        4,
        "ConstInt, Add and AutodiffStep must all survive"
    );
    assert!(
        instructions
            .iter()
            .any(|i| matches!(i.kind, InstructionKind::AutodiffStep { .. })),
        "AutodiffStep must survive DCE"
    );
}

#[test]
fn test_constant_folding_typed_int_within_width() {
    let i8t = Type::ExactInt {
        signed: true,
        width: IntWidth::I8,
    };
    let u8t = Type::ExactInt {
        signed: false,
        width: IntWidth::I8,
    };

    let mut function = Function::new("typed_fold", Vec::new(), Type::Void);
    let entry = function.add_block("entry");
    if let Some(block) = function.get_block_mut(entry) {
        // i8: 100 + 100 = 200 does not fit in i8 -> must not be folded.
        block.add_instruction(InstructionKind::ConstIntTyped {
            result: Value { id: 0 },
            value: 100,
            ty: i8t.clone(),
        });
        block.add_instruction(InstructionKind::ConstIntTyped {
            result: Value { id: 1 },
            value: 100,
            ty: i8t.clone(),
        });
        block.add_instruction(InstructionKind::Add {
            result: Value { id: 2 },
            lhs: Value { id: 0 },
            rhs: Value { id: 1 },
        });
        // u8: 100 + 100 = 200 fits in u8 -> must fold to ConstIntTyped(200, u8).
        block.add_instruction(InstructionKind::ConstIntTyped {
            result: Value { id: 3 },
            value: 100,
            ty: u8t.clone(),
        });
        block.add_instruction(InstructionKind::ConstIntTyped {
            result: Value { id: 4 },
            value: 100,
            ty: u8t.clone(),
        });
        block.add_instruction(InstructionKind::Add {
            result: Value { id: 5 },
            lhs: Value { id: 3 },
            rhs: Value { id: 4 },
        });
        // u8: 200 * 200 = 40000 does not fit in u8 -> must not be folded.
        block.add_instruction(InstructionKind::ConstIntTyped {
            result: Value { id: 6 },
            value: 200,
            ty: u8t.clone(),
        });
        block.add_instruction(InstructionKind::ConstIntTyped {
            result: Value { id: 7 },
            value: 200,
            ty: u8t.clone(),
        });
        block.add_instruction(InstructionKind::Mul {
            result: Value { id: 8 },
            lhs: Value { id: 6 },
            rhs: Value { id: 7 },
        });
        block.set_terminator(Terminator::Return { value: None });
    }

    let mut module = Module::new("typed_fold");
    module.add_function(function);
    let modified = constant_folding::run(&mut module);
    assert!(modified, "the in-width u8 addition should fold");

    let instructions = &module.functions[0].blocks[0].instructions;
    assert!(
        matches!(&instructions[2].kind, InstructionKind::Add { .. }),
        "i8 100 + 100 would wrap and must not be folded"
    );
    assert!(
        matches!(
            &instructions[5].kind,
            InstructionKind::ConstIntTyped {
                value: 200,
                ty,
                ..
            } if *ty == u8t
        ),
        "u8 100 + 100 should fold to a typed constant"
    );
    assert!(
        matches!(&instructions[8].kind, InstructionKind::Mul { .. }),
        "u8 200 * 200 would wrap and must not be folded"
    );
}

#[test]
fn test_constant_folding_skips_mixed_typed_and_untyped_constants() {
    let mut function = Function::new("mixed_fold", Vec::new(), Type::Void);
    let entry = function.add_block("entry");
    if let Some(block) = function.get_block_mut(entry) {
        block.add_instruction(InstructionKind::ConstInt {
            result: Value { id: 0 },
            value: 5,
        });
        block.add_instruction(InstructionKind::ConstIntTyped {
            result: Value { id: 1 },
            value: 3,
            ty: Type::ExactInt {
                signed: true,
                width: IntWidth::I8,
            },
        });
        block.add_instruction(InstructionKind::Add {
            result: Value { id: 2 },
            lhs: Value { id: 0 },
            rhs: Value { id: 1 },
        });
        block.set_terminator(Terminator::Return { value: None });
    }

    let mut module = Module::new("mixed_fold");
    module.add_function(function);
    let modified = constant_folding::run(&mut module);
    assert!(
        !modified,
        "mixing an untyped constant with a typed one leaves the result type ambiguous"
    );
    assert!(matches!(
        module.functions[0].blocks[0].instructions[2].kind,
        InstructionKind::Add { .. }
    ));
}

#[test]
fn test_constant_folding_bool_logic() {
    let mut function = Function::new("bool_fold", Vec::new(), Type::Void);
    let entry = function.add_block("entry");
    if let Some(block) = function.get_block_mut(entry) {
        block.add_instruction(InstructionKind::ConstBool {
            result: Value { id: 0 },
            value: true,
        });
        block.add_instruction(InstructionKind::ConstBool {
            result: Value { id: 1 },
            value: false,
        });
        block.add_instruction(InstructionKind::And {
            result: Value { id: 2 },
            lhs: Value { id: 0 },
            rhs: Value { id: 1 },
        });
        block.add_instruction(InstructionKind::Or {
            result: Value { id: 3 },
            lhs: Value { id: 0 },
            rhs: Value { id: 1 },
        });
        block.add_instruction(InstructionKind::Not {
            result: Value { id: 4 },
            operand: Value { id: 1 },
        });
        block.set_terminator(Terminator::Return { value: None });
    }

    let mut module = Module::new("bool_fold");
    module.add_function(function);
    let modified = constant_folding::run(&mut module);
    assert!(modified, "trivial boolean logic should fold");

    let instructions = &module.functions[0].blocks[0].instructions;
    assert!(matches!(
        &instructions[2].kind,
        InstructionKind::ConstBool { value: false, .. }
    ));
    assert!(matches!(
        &instructions[3].kind,
        InstructionKind::ConstBool { value: true, .. }
    ));
    assert!(matches!(
        &instructions[4].kind,
        InstructionKind::ConstBool { value: true, .. }
    ));
}

#[test]
fn test_constant_folding_after_inlining() {
    // The helper computes `5 + 3` in its own body. Before inlining that add
    // is not visible from `main`; after inlining, the post-inlining fold
    // (which the CLI pipeline re-runs at -O2 and above) must fold it.
    let mut helper = Function::new("const_sum", Vec::new(), Type::Int);
    let helper_entry = helper.add_block("entry");
    if let Some(block) = helper.get_block_mut(helper_entry) {
        block.add_instruction(InstructionKind::ConstInt {
            result: Value { id: 0 },
            value: 5,
        });
        block.add_instruction(InstructionKind::ConstInt {
            result: Value { id: 1 },
            value: 3,
        });
        block.add_instruction(InstructionKind::Add {
            result: Value { id: 2 },
            lhs: Value { id: 0 },
            rhs: Value { id: 1 },
        });
        block.set_terminator(Terminator::Return {
            value: Some(Value { id: 2 }),
        });
    }

    let mut caller = Function::new("main", Vec::new(), Type::Int);
    let caller_entry = caller.add_block("entry");
    if let Some(block) = caller.get_block_mut(caller_entry) {
        block.add_instruction(InstructionKind::Call {
            result: Some(Value { id: 0 }),
            function: "const_sum".into(),
            args: Vec::new(),
            is_tail: false,
        });
        block.set_terminator(Terminator::Return {
            value: Some(Value { id: 0 }),
        });
    }
    caller.next_value_id = 1;

    let mut module = Module::new("post_inline_fold");
    module.add_function(helper);
    module.add_function(caller);

    assert!(function_inlining::run(&mut module), "helper should inline");
    let modified = constant_folding::run(&mut module);
    assert!(
        modified,
        "constants introduced by inlining must be folded by the post-inline pass"
    );

    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main function");
    let instructions: Vec<_> = main
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .collect();
    assert!(
        instructions
            .iter()
            .all(|instruction| !matches!(instruction.kind, InstructionKind::Add { .. })),
        "the inlined `5 + 3` must be folded away in main"
    );
    assert!(
        instructions.iter().any(|instruction| matches!(
            &instruction.kind,
            InstructionKind::ConstInt { value: 8, .. }
        )),
        "main should contain the folded constant 8"
    );
}

#[test]
fn test_dce_preserves_bounded_gep_and_removes_pure_add() {
    let mut function = Function::new("bounded_gep", Vec::new(), Type::Void);
    let entry = function.add_block("entry");
    let mut builder = spectra_midend::builder::IRBuilder::new();
    builder.set_current_block(entry);

    let slot = builder.build_alloca(&mut function, Type::Int);
    let zero = builder.build_const_int(&mut function, 0);
    // Bounded GEP with an unused result: the backend emits the array-bounds
    // panic branch from the bound, so removing the GEP would remove the trap.
    let bounded = builder.build_getelementptr_bounded(
        &mut function,
        slot,
        zero,
        Type::Int,
        Some(ArrayBound::Static(4)),
    );
    // Bound-less GEPs are pure address arithmetic and stay removable.
    let open = builder.build_getelementptr(&mut function, slot, zero, Type::Int);
    // Unused pure arithmetic must still be removed.
    let one = builder.build_const_int(&mut function, 1);
    let two = builder.build_const_int(&mut function, 2);
    builder.build_add(&mut function, one, two);
    builder.build_return(&mut function, None);

    let mut module = Module::new("bounded_gep");
    module.add_function(function);
    dead_code_elimination::run(&mut module);

    let instructions = &module.functions[0].blocks[0].instructions;
    assert!(
        instructions.iter().any(|instruction| matches!(
            &instruction.kind,
            InstructionKind::GetElementPtr { bound: Some(_), result, .. } if *result == bounded
        )),
        "an unused bounded GEP must survive DCE: it carries the bounds-check trap"
    );
    assert!(
        !instructions.iter().any(|instruction| matches!(
            &instruction.kind,
            InstructionKind::GetElementPtr { bound: None, result, .. } if *result == open
        )),
        "an unused bound-less GEP is pure address arithmetic and should be removed"
    );
    assert!(
        !instructions
            .iter()
            .any(|instruction| matches!(instruction.kind, InstructionKind::Add { .. })),
        "an unused pure Add must still be removed"
    );
}

#[test]
fn test_dce_preserves_unused_integer_division() {
    let mut function = Function::new("unused_division", Vec::new(), Type::Void);
    let entry = function.add_block("entry");
    let mut builder = spectra_midend::builder::IRBuilder::new();
    builder.set_current_block(entry);

    let ten = builder.build_const_int(&mut function, 10);
    let two = builder.build_const_int(&mut function, 2);
    // Integer division can trap (by zero, `MIN / -1`), and the backend emits
    // those panic branches: an unused Div must not be deleted.
    let quotient = builder.build_div(&mut function, ten, two);
    // Unused pure arithmetic in the same block must still be removed.
    let one = builder.build_const_int(&mut function, 1);
    let three = builder.build_const_int(&mut function, 3);
    builder.build_add(&mut function, one, three);
    builder.build_return(&mut function, None);

    let mut module = Module::new("unused_division");
    module.add_function(function);
    dead_code_elimination::run(&mut module);

    let instructions = &module.functions[0].blocks[0].instructions;
    assert!(
        instructions.iter().any(|instruction| matches!(
            &instruction.kind,
            InstructionKind::Div { result, .. } if *result == quotient
        )),
        "an unused Div must survive DCE: integer division can trap"
    );
    assert!(
        !instructions
            .iter()
            .any(|instruction| matches!(instruction.kind, InstructionKind::Add { .. })),
        "an unused pure Add must still be removed"
    );
}

#[test]
fn test_dce_iterates_to_fixpoint() {
    // First pass removes the unused Mul; only after that does the ConstInt
    // feeding it become unused. A single-pass DCE would leave it behind.
    let mut module = Module {
        name: "test".to_string(),
        functions: vec![Function {
            name: "test_func".to_string(),
            params: vec![],
            return_type: Type::Int,
            next_value_id: 0,
            next_block_id: 1,
            source_span: None,
            locals: vec![],
            async_layout: None,
            suspension_barrier: false,
            blocks: vec![BasicBlock {
                id: 0,
                label: "entry".to_string(),
                instructions: vec![
                    Instruction {
                        id: 0,
                        kind: InstructionKind::ConstInt {
                            result: Value { id: 0 },
                            value: 7,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 1,
                        kind: InstructionKind::Mul {
                            result: Value { id: 1 },
                            lhs: Value { id: 0 },
                            rhs: Value { id: 0 },
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 2,
                        kind: InstructionKind::ConstInt {
                            result: Value { id: 2 },
                            value: 9,
                        },
                        source_span: None,
                    },
                ],
                terminator: Some(Terminator::Return {
                    value: Some(Value { id: 2 }),
                }),
            }],
        }],
        external_functions: vec![],
        globals: vec![],
        source_file: None,
    };

    let modified = dead_code_elimination::run(&mut module);
    assert!(modified, "DCE should remove the dead chain");

    let instructions = &module.functions[0].blocks[0].instructions;
    assert_eq!(
        instructions.len(),
        1,
        "fixpoint iteration should remove both the Mul and its now-unused producer"
    );
    assert!(
        matches!(
            &instructions[0].kind,
            InstructionKind::ConstInt { value: 9, .. }
        ),
        "only the returned constant must remain"
    );
}
