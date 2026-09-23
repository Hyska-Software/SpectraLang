use spectra_compiler::{CompilationOptions, CompilationPipeline, Lexer, Parser};

fn parse(source: &str) -> spectra_compiler::Module {
    let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");
    Parser::new(tokens)
        .parse()
        .expect("canonical source should parse")
}

#[test]
fn canonical_surface_parses_without_semicolons() {
    let source = r#"
        module readable_surface

        from std.io import println

        public record Box {
            value: int
        }

        public func main() returns int {
            let total = 0
            for item in [1, 2, 3] {
                total = total + item
            }
            if not total == 0 {
                println("readable")
            } else if total == 0 {
                println("empty")
            } else {
                println("other")
            }
            return total
        }
    "#;

    let module = parse(source);
    assert!(module.items.iter().any(|item| matches!(
        item,
        spectra_compiler::ast::Item::Function(function) if function.name == "main"
    )));
}

#[test]
fn trailing_commas_are_accepted_in_arrays_and_call_arguments() {
    let source = r#"
        module trailing_comma_surface

        func add_all(a: int, b: int, c: int) returns int {
            return a + b + c
        }

        public func main() returns int {
            let values = [1, 2, 3,]
            let single = [7,]
            let empty: array<int> = []
            return add_all(1, 2, 3,) + values[0] + single[0] + empty[0]
        }
    "#;

    let module = parse(source);
    assert!(module.items.iter().any(|item| matches!(
        item,
        spectra_compiler::ast::Item::Function(function) if function.name == "main"
    )));
}

#[test]
fn generic_annotations_parse_in_record_fields_and_array_elements() {
    use spectra_compiler::ast::{Item, StatementKind, TypeAnnotationKind};

    // The generic-argument heuristic must accept a record's last field (`}`),
    // a comma-less next field (identifier), and an array element type (`]`).
    let source = r#"
        module generic_annotation_surface

        record Inventory {
            items: Stack<int>
            labels: Map<string, int>,
            pending: Queue<string>
        }

        public func main() returns int {
            let slots: [Stack<int>] = []
            return 0
        }
    "#;

    let module = parse(source);
    let fields = module
        .items
        .iter()
        .find_map(|item| match item {
            Item::Struct(record) if record.name == "Inventory" => Some(record.fields.clone()),
            _ => None,
        })
        .expect("Inventory record");
    assert_eq!(fields.len(), 3);
    for (field, expected) in fields.iter().zip(["Stack", "Map", "Queue"]) {
        assert!(
            matches!(
                &field.ty.kind,
                TypeAnnotationKind::Generic { name, .. } if name == expected
            ),
            "field {} should keep its generic annotation, got {:?}",
            field.name,
            field.ty.kind
        );
    }

    let main = module
        .items
        .iter()
        .find_map(|item| match item {
            Item::Function(function) if function.name == "main" => Some(function),
            _ => None,
        })
        .expect("main function");
    let statement = main
        .body
        .statements
        .iter()
        .find_map(|statement| match &statement.kind {
            StatementKind::Let(let_statement) => Some(let_statement),
            _ => None,
        })
        .expect("let statement");
    let annotation = statement.ty.as_ref().expect("let annotation");
    let TypeAnnotationKind::Generic { name, type_args } = &annotation.kind else {
        panic!("expected an array annotation, got {:?}", annotation.kind);
    };
    assert_eq!(name, "array");
    assert!(matches!(
        &type_args[0].kind,
        TypeAnnotationKind::Generic { name, .. } if name == "Stack"
    ));
}

#[test]
fn doubled_comma_in_array_literal_remains_a_parse_error() {
    let source = r#"
        module doubled_comma_array

        public func main() returns int {
            let broken = [1,, 2]
            return broken[0]
        }
    "#;

    let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");
    let errors = Parser::new(tokens)
        .parse()
        .expect_err("a doubled comma inside an array must remain rejected");

    assert!(
        !errors.is_empty(),
        "expected at least one parser diagnostic"
    );
}

#[test]
fn legacy_surface_is_rejected_with_migration_diagnostics() {
    // The `->` arrow is no longer a token: the lexer rejects it with L010 and
    // points at the canonical `returns` keyword.
    let source = "module legacy;\nfn main() -> int { return 0; }\n";
    let lex_errors = Lexer::new(source)
        .tokenize()
        .expect_err("legacy arrow syntax must fail lexing");

    assert!(lex_errors.iter().any(|error| {
        error.code.as_deref() == Some("L010")
            && error
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("returns"))
    }));

    // Semicolons remain rejected by the parser (P012).
    let source = "module legacy\nfn main() returns int { return 0; }\n";
    let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");
    let errors = Parser::new(tokens)
        .parse()
        .expect_err("legacy syntax must not remain accepted");

    assert!(errors
        .iter()
        .any(|error| { matches!(error.code.as_deref(), Some("P001" | "P012")) }));
}

#[test]
fn bare_enum_variants_participate_in_exhaustiveness() {
    let source = r#"
        module readable_match

        enum State {
            Pending,
            Paid
        }

        public func main() returns int {
            let state = State::Pending
            return match state {
                when Pending then 1
            }
        }
    "#;

    let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
    let errors = pipeline
        .compile(source, "readable_match.spectra")
        .expect_err("a missing enum arm must remain a semantic error");
    let rendered = format!("{errors:?}");
    assert!(rendered.contains("not exhaustive"));
    assert!(rendered.contains("State::Paid"));
}
