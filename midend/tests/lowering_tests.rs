/// Tests for AST lowering to IR
use spectra_compiler::ast::{
    BinaryOperator, Block, Expression, ExpressionKind, ForLoop, Function, FunctionParam, Item,
    LetStatement, LoopStatement, Module, Pattern, ReturnStatement, Statement, StatementKind,
    Visibility, WhileLoop,
};
use spectra_compiler::span::Span;
use spectra_midend::ir::{InstructionKind, Type as IRType};
use spectra_midend::ASTLowering;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn s() -> Span {
    Span::dummy()
}

fn int_lit(n: i64) -> Expression {
    Expression {
        span: s(),
        kind: ExpressionKind::NumberLiteral(n.to_string()),
    }
}

fn bool_lit(b: bool) -> Expression {
    Expression {
        span: s(),
        kind: ExpressionKind::BoolLiteral(b),
    }
}

fn string_lit(value: &str) -> Expression {
    Expression {
        span: s(),
        kind: ExpressionKind::StringLiteral(value.to_string()),
    }
}

fn ident(name: &str) -> Expression {
    Expression {
        span: s(),
        kind: ExpressionKind::Identifier(name.to_string()),
    }
}

fn field_path(parts: &[&str]) -> Expression {
    let mut expression = ident(parts[0]);
    for field in &parts[1..] {
        expression = Expression {
            span: s(),
            kind: ExpressionKind::FieldAccess {
                object: Box::new(expression),
                field: (*field).to_string(),
            },
        };
    }
    expression
}

fn call(callee: Expression, arguments: Vec<Expression>) -> Expression {
    Expression {
        span: s(),
        kind: ExpressionKind::Call {
            callee: Box::new(callee),
            arguments,
        },
    }
}

fn enum_variant(enum_name: &str, variant_name: &str, data: Option<Vec<Expression>>) -> Expression {
    Expression {
        span: s(),
        kind: ExpressionKind::EnumVariant {
            module_path: None,
            enum_name: enum_name.to_string(),
            type_args: Vec::new(),
            variant_name: variant_name.to_string(),
            data,
            struct_data: None,
        },
    }
}

fn bin(left: Expression, op: BinaryOperator, right: Expression) -> Expression {
    Expression {
        span: s(),
        kind: ExpressionKind::Binary {
            left: Box::new(left),
            operator: op,
            right: Box::new(right),
        },
    }
}

fn range_expr(start: Expression, end: Expression, inclusive: bool) -> Expression {
    Expression {
        span: s(),
        kind: ExpressionKind::Range {
            start: Box::new(start),
            end: Box::new(end),
            inclusive,
        },
    }
}

fn let_stmt(name: &str, value: Expression) -> Statement {
    Statement {
        span: s(),
        kind: StatementKind::Let(LetStatement {
            pattern: Pattern::Identifier(name.to_string(), s()),
            span: s(),
            ty: None,
            value: Some(value),
        }),
    }
}

fn return_stmt(value: Expression) -> Statement {
    Statement {
        span: s(),
        kind: StatementKind::Return(ReturnStatement {
            span: s(),
            value: Some(value),
        }),
    }
}

fn make_function(name: &str, stmts: Vec<Statement>) -> Item {
    Item::Function(Function {
        name: name.to_string(),
        span: s(),
        visibility: Visibility::Public,
        attributes: Vec::new(),
        is_async: false,
        type_params: vec![],
        params: vec![],
        return_type: None,
        body: Block {
            span: s(),
            statements: stmts,
        },
    })
}

fn make_function_with_params(
    name: &str,
    params: Vec<(&str, spectra_compiler::ast::TypeAnnotation)>,
    stmts: Vec<Statement>,
    return_type: Option<spectra_compiler::ast::TypeAnnotation>,
) -> Item {
    Item::Function(Function {
        name: name.to_string(),
        span: s(),
        visibility: Visibility::Public,
        attributes: Vec::new(),
        is_async: false,
        type_params: vec![],
        params: params
            .into_iter()
            .map(|(n, ty)| FunctionParam {
                name: n.to_string(),
                span: s(),
                ty: Some(ty),
            })
            .collect(),
        return_type,
        body: Block {
            span: s(),
            statements: stmts,
        },
    })
}

fn int_type() -> spectra_compiler::ast::TypeAnnotation {
    use spectra_compiler::ast::{TypeAnnotation, TypeAnnotationKind};
    TypeAnnotation {
        kind: TypeAnnotationKind::Simple {
            segments: vec!["int".to_string()],
        },
        span: s(),
    }
}

fn make_module(name: &str, items: Vec<Item>) -> Module {
    Module {
        name: name.to_string(),
        span: s(),
        items,
        std_import_aliases: Vec::new(),
        imported_function_return_types: Vec::new(),
        imported_function_signatures: Vec::new(),
        imported_struct_defs: Vec::new(),
        imported_enum_defs: Vec::new(),
        imported_trait_impls: Vec::new(),
        imported_generic_functions: Vec::new(),
        imported_trait_decls: Vec::new(),
        imported_static_globals: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn list_contains_host_result_is_lowered_as_bool() {
    let contains = call(
        field_path(&["std", "collections", "list_contains"]),
        vec![int_lit(1), int_lit(1)],
    );
    let module = make_module(
        "collections_bool",
        vec![make_function("main", vec![let_stmt("present", contains)])],
    );

    let ir = ASTLowering::new()
        .lower_module(&module)
        .expect("lowering should pass");
    let contains_result_type = ir
        .functions
        .iter()
        .flat_map(|function| function.blocks.iter())
        .flat_map(|block| block.instructions.iter())
        .find_map(|instruction| match &instruction.kind {
            InstructionKind::HostCall {
                host,
                result_type: Some(result_type),
                ..
            } if host == "spectra.std.collections.list_contains" => Some(result_type),
            _ => None,
        });

    assert_eq!(contains_result_type, Some(&IRType::Bool));
}

#[test]
fn std_io_input_lowers_to_string_hostcall() {
    let input = call(
        field_path(&["std", "io", "input"]),
        vec![string_lit("prompt")],
    );
    let module = make_module(
        "io_input",
        vec![make_function("main", vec![let_stmt("line", input)])],
    );

    let ir = ASTLowering::new()
        .lower_module(&module)
        .expect("lowering should pass");
    let input_result_type = ir
        .functions
        .iter()
        .flat_map(|function| function.blocks.iter())
        .flat_map(|block| block.instructions.iter())
        .find_map(|instruction| match &instruction.kind {
            InstructionKind::HostCall {
                host,
                result_type: Some(result_type),
                ..
            } if host == "spectra.std.io.input" => Some(result_type),
            _ => None,
        });

    assert_eq!(input_result_type, Some(&IRType::String));
}

#[test]
fn stdlib_fs_result_and_env_boolean_host_types_are_preserved() {
    let fs_exists = call(
        field_path(&["std", "fs", "fs_exists"]),
        vec![string_lit("target/stdlib-bug-hunt-missing")],
    );
    let env_set = call(
        field_path(&["std", "env", "env_set"]),
        vec![string_lit("SPECTRA_TEST_KEY"), string_lit("value")],
    );
    let module = make_module(
        "stdlib_boolean_results",
        vec![make_function(
            "main",
            vec![let_stmt("exists", fs_exists), let_stmt("set", env_set)],
        )],
    );

    let ir = ASTLowering::new()
        .lower_module(&module)
        .expect("lowering should pass");
    let host_types: Vec<(&str, &IRType)> = ir
        .functions
        .iter()
        .flat_map(|function| function.blocks.iter())
        .flat_map(|block| block.instructions.iter())
        .filter_map(|instruction| match &instruction.kind {
            InstructionKind::HostCall {
                host,
                result_type: Some(result_type),
                ..
            } if host == "spectra.std.fs.fs_exists" || host == "spectra.std.env.env_set" => {
                Some((host.as_str(), result_type))
            }
            _ => None,
        })
        .collect();

    assert_eq!(host_types.len(), 2);
    let fs_result_type = host_types
        .iter()
        .find(|(host, _)| *host == "spectra.std.fs.fs_exists")
        .map(|(_, result_type)| *result_type);
    assert!(matches!(
        fs_result_type,
        Some(IRType::Enum { name, .. }) if name == "Result_bool_Error"
    ));
    let env_set_type = host_types
        .iter()
        .find(|(host, _)| *host == "spectra.std.env.env_set")
        .map(|(_, result_type)| *result_type);
    assert_eq!(env_set_type, Some(&IRType::Bool));
}

#[test]
fn result_unwrap_err_uses_the_enum_error_payload_type() {
    let error = enum_variant("Result", "Err", Some(vec![string_lit("failure")]));
    let unwrap = call(
        field_path(&["std", "result", "result_unwrap_err"]),
        vec![error],
    );
    let module = make_module(
        "result_error_payload",
        vec![make_function("main", vec![let_stmt("message", unwrap)])],
    );

    let ir = ASTLowering::new()
        .lower_module(&module)
        .expect("lowering should pass");
    let result_type = ir
        .functions
        .iter()
        .flat_map(|function| function.blocks.iter())
        .flat_map(|block| block.instructions.iter())
        .find_map(|instruction| match &instruction.kind {
            InstructionKind::HostCall {
                host,
                result_type: Some(result_type),
                ..
            } if host == "spectra.std.result.result_unwrap_err" => Some(result_type),
            _ => None,
        });

    assert_eq!(result_type, Some(&IRType::String));
}

#[test]
fn result_unwrap_err_keeps_inferred_binding_payload_type() {
    let error = enum_variant("Result", "Err", Some(vec![string_lit("failure")]));
    let unwrap = call(
        field_path(&["std", "result", "result_unwrap_err"]),
        vec![ident("error")],
    );
    let module = make_module(
        "result_error_binding",
        vec![make_function(
            "main",
            vec![let_stmt("error", error), let_stmt("message", unwrap)],
        )],
    );

    let ir = ASTLowering::new()
        .lower_module(&module)
        .expect("lowering should pass");
    let result_type = ir
        .functions
        .iter()
        .flat_map(|function| function.blocks.iter())
        .flat_map(|block| block.instructions.iter())
        .find_map(|instruction| match &instruction.kind {
            InstructionKind::HostCall {
                host,
                result_type: Some(result_type),
                ..
            } if host == "spectra.std.result.result_unwrap_err" => Some(result_type),
            _ => None,
        });

    assert_eq!(result_type, Some(&IRType::String));
}

#[test]
fn test_lower_simple_arithmetic() {
    // let x = 5 + 3;
    let module = make_module(
        "test",
        vec![make_function(
            "main",
            vec![let_stmt(
                "x",
                bin(int_lit(5), BinaryOperator::Add, int_lit(3)),
            )],
        )],
    );

    let mut lowering = ASTLowering::new();
    let ir_module = lowering
        .lower_module(&module)
        .expect("lowering should succeed");

    assert_eq!(ir_module.name, "test");
    assert_eq!(ir_module.functions.len(), 1);

    let func = &ir_module.functions[0];
    assert_eq!(func.name, "main");
    assert!(!func.blocks.is_empty());

    let entry_block = &func.blocks[0];
    // Should have at least: ConstInt(5), ConstInt(3), Add
    assert!(
        entry_block.instructions.len() >= 3,
        "Expected >= 3 instructions, got {}",
        entry_block.instructions.len()
    );
}

#[test]
fn test_lower_multiple_operations() {
    // let a = 10 - 4;
    // let b = a * 2;
    let module = make_module(
        "test",
        vec![make_function(
            "main",
            vec![
                let_stmt("a", bin(int_lit(10), BinaryOperator::Subtract, int_lit(4))),
                let_stmt("b", bin(ident("a"), BinaryOperator::Multiply, int_lit(2))),
            ],
        )],
    );

    let mut lowering = ASTLowering::new();
    let ir_module = lowering
        .lower_module(&module)
        .expect("lowering should succeed");

    let func = &ir_module.functions[0];
    assert!(!func.blocks.is_empty());

    let entry_block = &func.blocks[0];
    let has_sub = entry_block
        .instructions
        .iter()
        .any(|i| matches!(i.kind, InstructionKind::Sub { .. }));
    let has_mul = entry_block
        .instructions
        .iter()
        .any(|i| matches!(i.kind, InstructionKind::Mul { .. }));
    assert!(has_sub, "Should have Sub instruction");
    assert!(has_mul, "Should have Mul instruction");
}

#[test]
fn test_lower_while_loop() {
    // while true { }
    let module = make_module(
        "test",
        vec![make_function(
            "main",
            vec![Statement {
                span: s(),
                kind: StatementKind::While(WhileLoop {
                    condition: bool_lit(true),
                    body: Block {
                        span: s(),
                        statements: vec![Statement {
                            span: s(),
                            kind: StatementKind::Break,
                        }],
                    },
                    span: s(),
                }),
            }],
        )],
    );

    let mut lowering = ASTLowering::new();
    let ir_module = lowering
        .lower_module(&module)
        .expect("lowering should succeed");

    let func = &ir_module.functions[0];
    // while loop generates at least: header block + body block + exit block
    assert!(
        func.blocks.len() >= 2,
        "While loop should generate multiple blocks, got {}",
        func.blocks.len()
    );
}

#[test]
fn test_lower_loop_infinite() {
    // loop { break; }
    let module = make_module(
        "test",
        vec![make_function(
            "main",
            vec![Statement {
                span: s(),
                kind: StatementKind::Loop(LoopStatement {
                    body: Block {
                        span: s(),
                        statements: vec![Statement {
                            span: s(),
                            kind: StatementKind::Break,
                        }],
                    },
                    span: s(),
                }),
            }],
        )],
    );

    let mut lowering = ASTLowering::new();
    let ir_module = lowering
        .lower_module(&module)
        .expect("lowering should succeed");

    let func = &ir_module.functions[0];
    // loop produces at least a header block and a body block
    assert!(!func.blocks.is_empty());
}

#[test]
fn test_lower_stored_range_for_loop_uses_counted_loop() {
    let module = make_module(
        "test",
        vec![make_function(
            "main",
            vec![
                let_stmt("r", range_expr(int_lit(1), int_lit(4), false)),
                Statement {
                    span: s(),
                    kind: StatementKind::For(ForLoop {
                        iterator: "i".to_string(),
                        iterable: ident("r"),
                        body: Block {
                            span: s(),
                            statements: vec![Statement {
                                span: s(),
                                kind: StatementKind::Break,
                            }],
                        },
                        span: s(),
                    }),
                },
            ],
        )],
    );

    let mut lowering = ASTLowering::new();
    let ir_module = lowering
        .lower_module(&module)
        .expect("lowering should succeed");
    let func = &ir_module.functions[0];
    let hosts: Vec<&str> = func
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .filter_map(|instruction| match &instruction.kind {
            InstructionKind::HostCall { host, .. } => Some(host.as_str()),
            _ => None,
        })
        .collect();

    assert!(hosts.contains(&"spectra.std.range.create"));
    assert!(!hosts.contains(&"spectra.std.range.len"));
    assert!(!hosts.contains(&"spectra.std.range.at"));
}

#[test]
fn test_lower_literal_range_for_loop_uses_iterator_protocol() {
    let module = make_module(
        "test",
        vec![make_function(
            "main",
            vec![Statement {
                span: s(),
                kind: StatementKind::For(ForLoop {
                    iterator: "i".to_string(),
                    iterable: range_expr(int_lit(0), int_lit(100), false),
                    body: Block {
                        span: s(),
                        statements: vec![Statement {
                            span: s(),
                            kind: StatementKind::Break,
                        }],
                    },
                    span: s(),
                }),
            }],
        )],
    );

    let mut lowering = ASTLowering::new();
    let ir_module = lowering
        .lower_module(&module)
        .expect("lowering should succeed");
    let func = &ir_module.functions[0];
    let hosts: Vec<&str> = func
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .filter_map(|instruction| match &instruction.kind {
            InstructionKind::HostCall { host, .. } => Some(host.as_str()),
            _ => None,
        })
        .collect();

    assert!(hosts.contains(&"spectra.std.range.create"));
    assert!(hosts.contains(&"spectra.std.range.iter"));
    assert!(hosts.contains(&"spectra.std.collections.iterator_remaining"));
    assert!(hosts.contains(&"spectra.std.collections.iterator_next"));
    assert!(hosts.contains(&"spectra.std.option.option_unwrap"));
    assert!(!hosts.contains(&"spectra.std.range.len"));
    assert!(!hosts.contains(&"spectra.std.range.at"));
}

#[test]
fn test_lower_function_call() {
    // fn add(a: int, b: int) -> int { return a + b; }
    // fn main() { let r = add(5, 3); }
    let add_fn = make_function_with_params(
        "add",
        vec![("a", int_type()), ("b", int_type())],
        vec![return_stmt(bin(
            ident("a"),
            BinaryOperator::Add,
            ident("b"),
        ))],
        Some(int_type()),
    );

    let call_expr = Expression {
        span: s(),
        kind: ExpressionKind::Call {
            callee: Box::new(ident("add")),
            arguments: vec![int_lit(5), int_lit(3)],
        },
    };
    let main_fn = make_function("main", vec![let_stmt("r", call_expr)]);

    let module = make_module("test", vec![add_fn, main_fn]);

    let mut lowering = ASTLowering::new();
    let ir_module = lowering
        .lower_module(&module)
        .expect("lowering should succeed");

    assert_eq!(ir_module.functions.len(), 2);

    // Find main function and verify it has a Call instruction
    let main_ir = ir_module
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("main function should exist");

    let has_call = main_ir.blocks.iter().any(|b| {
        b.instructions
            .iter()
            .any(|i| matches!(i.kind, InstructionKind::Call { .. }))
    });
    assert!(has_call, "Should have Call instruction");
}

#[test]
fn test_lower_boolean_literals() {
    // let t = true;
    // let f = false;
    let module = make_module(
        "test",
        vec![make_function(
            "main",
            vec![
                let_stmt("t", bool_lit(true)),
                let_stmt("f", bool_lit(false)),
            ],
        )],
    );

    let mut lowering = ASTLowering::new();
    let ir_module = lowering
        .lower_module(&module)
        .expect("lowering should succeed");

    let func = &ir_module.functions[0];
    let entry = &func.blocks[0];

    let has_bool_true = entry
        .instructions
        .iter()
        .any(|i| matches!(i.kind, InstructionKind::ConstBool { value: true, .. }));
    let has_bool_false = entry
        .instructions
        .iter()
        .any(|i| matches!(i.kind, InstructionKind::ConstBool { value: false, .. }));

    assert!(has_bool_true, "Should have ConstBool(true)");
    assert!(has_bool_false, "Should have ConstBool(false)");
}

#[test]
fn test_lower_comparison() {
    // let x = 5 > 3;
    let module = make_module(
        "test",
        vec![make_function(
            "main",
            vec![let_stmt(
                "x",
                bin(int_lit(5), BinaryOperator::Greater, int_lit(3)),
            )],
        )],
    );

    let mut lowering = ASTLowering::new();
    let ir_module = lowering
        .lower_module(&module)
        .expect("lowering should succeed");

    let func = &ir_module.functions[0];
    let entry = &func.blocks[0];

    let has_gt = entry
        .instructions
        .iter()
        .any(|i| matches!(i.kind, InstructionKind::Gt { .. }));
    assert!(has_gt, "Should have Gt (greater-than) instruction");
}

#[test]
fn test_lower_empty_function() {
    // fn empty() { }
    let module = make_module("test", vec![make_function("empty", vec![])]);

    let mut lowering = ASTLowering::new();
    let ir_module = lowering
        .lower_module(&module)
        .expect("lowering should succeed");

    assert_eq!(ir_module.functions.len(), 1);
    let func = &ir_module.functions[0];
    assert_eq!(func.name, "empty");
    assert!(!func.blocks.is_empty());
}

#[test]
fn test_lower_multiple_functions() {
    let module = make_module(
        "test",
        vec![
            make_function("foo", vec![]),
            make_function("bar", vec![]),
            make_function("baz", vec![]),
        ],
    );

    let mut lowering = ASTLowering::new();
    let ir_module = lowering
        .lower_module(&module)
        .expect("lowering should succeed");

    assert_eq!(ir_module.functions.len(), 3);
    assert!(ir_module.functions.iter().any(|f| f.name == "foo"));
    assert!(ir_module.functions.iter().any(|f| f.name == "bar"));
    assert!(ir_module.functions.iter().any(|f| f.name == "baz"));
}

#[test]
fn test_tail_self_recursion_is_marked() {
    use spectra_compiler::ast::StatementKind;
    // fn countdown(n: int, acc: int) -> int {
    //     if n <= 0 { return acc; }
    //     return countdown(n - 1, acc + n);
    // }
    let if_expr = Expression {
        span: s(),
        kind: ExpressionKind::If {
            condition: Box::new(bin(ident("n"), BinaryOperator::LessEqual, int_lit(0))),
            then_block: Block {
                span: s(),
                statements: vec![Statement {
                    span: s(),
                    kind: StatementKind::Return(ReturnStatement {
                        span: s(),
                        value: Some(ident("acc")),
                    }),
                }],
            },
            elif_blocks: vec![],
            else_block: None,
        },
    };
    let recursive_call = Expression {
        span: s(),
        kind: ExpressionKind::Call {
            callee: Box::new(ident("countdown")),
            arguments: vec![
                bin(ident("n"), BinaryOperator::Subtract, int_lit(1)),
                bin(ident("acc"), BinaryOperator::Add, ident("n")),
            ],
        },
    };
    let countdown_fn = make_function_with_params(
        "countdown",
        vec![("n", int_type()), ("acc", int_type())],
        vec![
            Statement {
                span: s(),
                kind: StatementKind::Expression(if_expr),
            },
            return_stmt(recursive_call),
        ],
        Some(int_type()),
    );

    // fn main() { } — entry point must stay unmarked (external callers).
    let module = make_module("test", vec![countdown_fn, make_function("main", vec![])]);

    let mut lowering = ASTLowering::new();
    let ir_module = lowering
        .lower_module(&module)
        .expect("lowering should succeed");

    let countdown_ir = ir_module
        .functions
        .iter()
        .find(|f| f.name == "countdown")
        .expect("countdown function should exist");
    let marked = countdown_ir.blocks.iter().any(|block| {
        block.instructions.iter().any(|i| {
            matches!(
                &i.kind,
                InstructionKind::Call {
                    function,
                    is_tail: true,
                    ..
                } if function == "countdown"
            )
        })
    });
    assert!(
        marked,
        "the direct self-call in tail position should be marked is_tail"
    );

    let main_ir = ir_module
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("main function should exist");
    assert!(
        main_ir.blocks.iter().all(|block| {
            block
                .instructions
                .iter()
                .all(|i| !matches!(&i.kind, InstructionKind::Call { is_tail: true, .. }))
        }),
        "main is entered externally and must never be marked"
    );
}

#[test]
fn test_cross_function_tail_call_is_not_marked() {
    // fn helper(n: int) -> int { return n; }
    // fn caller() -> int { return helper(5); }  // cross-function: NOT a self call
    let helper_fn = make_function_with_params(
        "helper",
        vec![("n", int_type())],
        vec![return_stmt(ident("n"))],
        Some(int_type()),
    );
    let cross_call = Expression {
        span: s(),
        kind: ExpressionKind::Call {
            callee: Box::new(ident("helper")),
            arguments: vec![int_lit(5)],
        },
    };
    let caller_fn = make_function_with_params(
        "caller",
        vec![],
        vec![return_stmt(cross_call)],
        Some(int_type()),
    );
    let module = make_module("test", vec![helper_fn, caller_fn]);

    let mut lowering = ASTLowering::new();
    let ir_module = lowering
        .lower_module(&module)
        .expect("lowering should succeed");

    for func in &ir_module.functions {
        for block in &func.blocks {
            for instruction in &block.instructions {
                if let InstructionKind::Call { is_tail, .. } = &instruction.kind {
                    assert!(
                        !is_tail,
                        "cross-function calls must not be marked as tail calls"
                    );
                }
            }
        }
    }
}

fn named_type(name: &str) -> spectra_compiler::ast::TypeAnnotation {
    use spectra_compiler::ast::{TypeAnnotation, TypeAnnotationKind};
    TypeAnnotation {
        kind: TypeAnnotationKind::Simple {
            segments: vec![name.to_string()],
        },
        span: s(),
    }
}

#[test]
fn test_char_to_u8_cast_uses_checked_host() {
    // `c as u8` must validate the codepoint through the checked numeric
    // host instead of truncating it to a byte (D7). A raw Char->i8 cast
    // must never reach the backend from an `as` cast.
    use spectra_compiler::ast::CastMode;
    let cast = Expression {
        span: s(),
        kind: ExpressionKind::Cast {
            expr: Box::new(ident("c")),
            target_type: named_type("u8"),
            mode: CastMode::Checked,
        },
    };
    let narrow = make_function_with_params(
        "narrow",
        vec![("c", named_type("char"))],
        vec![return_stmt(cast)],
        Some(named_type("u8")),
    );
    let module = make_module("test", vec![narrow]);

    let mut lowering = ASTLowering::new();
    let ir_module = lowering
        .lower_module(&module)
        .expect("lowering should succeed");
    let func = &ir_module.functions[0];
    let mut saw_checked_host = false;
    for block in &func.blocks {
        for instruction in &block.instructions {
            match &instruction.kind {
                InstructionKind::HostCall { host, .. } => {
                    if host == "spectra.std.numeric.checked_u8" {
                        saw_checked_host = true;
                    }
                }
                InstructionKind::Cast { from_ty, to_ty, .. } => {
                    let narrows_char =
                        matches!(from_ty, IRType::Char) && matches!(to_ty, IRType::ExactInt { .. });
                    assert!(
                        !narrows_char,
                        "raw char narrowing must not reach the backend"
                    );
                }
                _ => {}
            }
        }
    }
    assert!(
        saw_checked_host,
        "char to u8 must go through the checked host"
    );
}
