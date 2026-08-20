use spectra_compiler::{CompilationOptions, CompilationPipeline, Lexer, Parser};
use std::collections::HashSet;

fn parse(source: &str) -> spectra_compiler::Module {
    let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");
    Parser::new(tokens, HashSet::new())
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
fn legacy_surface_is_rejected_with_migration_diagnostics() {
    let source = "module legacy;\nfn main() -> int { return 0; }\n";
    let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");
    let errors = Parser::new(tokens, HashSet::new())
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
