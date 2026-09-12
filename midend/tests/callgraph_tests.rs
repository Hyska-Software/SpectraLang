// Tests for the shared call graph and the R-3203 impact index.

use spectra_compiler::{analyze_modules, Lexer, Parser};
use spectra_midend::callgraph::{direct_calls, ImpactIndex};
use spectra_midend::ir::*;
use spectra_midend::passes::function_inlining::FunctionInlining;
use spectra_midend::passes::Pass;

fn lower_source(source: &str) -> Module {
    let tokens = Lexer::new(source).tokenize().expect("lexing should pass");
    let mut module = Parser::new(tokens).parse().expect("parsing should pass");
    analyze_modules(&mut [&mut module]).expect("semantic analysis should pass");
    spectra_midend::ASTLowering::new()
        .lower_module(&module)
        .expect("lowering should pass")
}

#[test]
fn direct_calls_maps_callers_to_callees_and_keeps_empty_entries() {
    let ir = lower_source(
        r#"
        module direct_calls_case

        func leaf() returns int {
            return 1
        }

        func middle() returns int {
            return leaf() + 1
        }

        public func main() returns int {
            return middle() + middle()
        }
        "#,
    );

    let graph = direct_calls(&ir);
    assert_eq!(
        graph.get("middle"),
        Some(&std::collections::HashSet::from(["leaf".to_string()])),
        "middle must list its cross-function callee"
    );
    assert_eq!(
        graph.get("main"),
        Some(&std::collections::HashSet::from(["middle".to_string()]))
    );
    assert!(
        graph.contains_key("leaf"),
        "every function gets an entry, even without calls"
    );
    assert!(graph.get("leaf").is_some_and(|callees| callees.is_empty()));
}

#[test]
fn direct_calls_agrees_with_function_inlining() {
    let mut ir = lower_source(
        r#"
        module inlining_agreement

        func helper(value: int) returns int {
            return value + 1
        }

        public func main() returns int {
            return helper(41)
        }
        "#,
    );

    let before = direct_calls(&ir);
    assert!(
        before
            .get("main")
            .is_some_and(|callees| callees.contains("helper")),
        "shared graph must see the call the pass is about to inline"
    );

    let changed = FunctionInlining::new().run(&mut ir);
    assert!(changed, "the small helper must be inlined");

    let after = direct_calls(&ir);
    assert!(
        after.get("main").is_some_and(|callees| callees.is_empty()),
        "after inlining the shared graph shows no remaining direct call"
    );
}

#[test]
fn impact_index_reports_cross_function_callers() {
    let ir = lower_source(
        r#"
        module impact_callers

        func leaf() returns int {
            return 1
        }

        func middle() returns int {
            return leaf()
        }

        public func main() returns int {
            return middle()
        }
        "#,
    );

    let index = ImpactIndex::build(&ir);
    assert_eq!(index.callers_of("leaf"), vec!["middle".to_string()]);
    assert_eq!(index.callers_of("middle"), vec!["main".to_string()]);
    assert!(index.callers_of("missing").is_empty());
}

#[test]
fn impact_index_reports_field_readers_writers_and_constructors() {
    let ir = lower_source(
        r#"
        module impact_fields

        record Point {
            x: int,
            y: int,
        }

        func make_point(px: int, py: int) returns Point {
            Point { x: px, y: py }
        }

        func read_point(p: Point) returns int {
            return p.x
        }

        func write_point(p: Point) returns int {
            p.x = 7
            return p.y
        }

        public func main() returns int {
            let p = make_point(1, 2)
            return read_point(p) + write_point(p)
        }
        "#,
    );

    let index = ImpactIndex::build(&ir);
    let users = index.field_users_of("Point.x");
    assert!(
        users.contains(&"make_point".to_string()),
        "struct-literal construction must count as a field user, got {users:?}"
    );
    assert!(
        users.contains(&"read_point".to_string()),
        "field read must count as a field user, got {users:?}"
    );
    assert!(
        users.contains(&"write_point".to_string()),
        "field write must count as a field user, got {users:?}"
    );

    let mut sorted = users.clone();
    sorted.sort();
    assert_eq!(users, sorted, "field users are sorted");

    assert!(index.field_users_of("Point.missing").is_empty());
    assert!(index.field_users_of("Missing.x").is_empty());
}

#[test]
fn impact_index_reports_unresolved_dynamic_calls_without_guessing() {
    let module = Module {
        name: "impact_dynamic".to_string(),
        functions: vec![Function {
            name: "dispatch".to_string(),
            params: vec![Parameter {
                id: 0,
                name: "target".to_string(),
                ty: Type::Int,
            }],
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
                            result: Value { id: 1 },
                            value: 5,
                        },
                        source_span: None,
                    },
                    Instruction {
                        id: 1,
                        kind: InstructionKind::CallIndirect {
                            result: Some(Value { id: 2 }),
                            fn_ptr: Value { id: 0 },
                            args: vec![Value { id: 1 }],
                            signature_params: vec![Type::Int],
                            signature_return: Box::new(Type::Int),
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

    let index = ImpactIndex::build(&module);
    assert_eq!(index.unresolved_dynamic(), vec!["dispatch".to_string()]);
    assert!(
        index.callers_of("dispatch").is_empty(),
        "an indirect site must not be attributed to a guessed target"
    );
}
