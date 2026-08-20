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
