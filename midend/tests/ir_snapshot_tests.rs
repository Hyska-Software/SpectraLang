use spectra_compiler::{Lexer, Parser};
use spectra_midend::ir::pretty::format_module;
use spectra_midend::ASTLowering;

use std::fs;
use std::path::Path;

fn assert_snapshot(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join(name);
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read snapshot {}: {err}", path.display()));
    assert_eq!(
        actual.trim_end(),
        expected.trim_end(),
        "snapshot {name} changed"
    );
}

#[test]
fn ir_snapshot_covers_lowering_stage() {
    let source = r#"
        module lowering_snapshot

        func add(lhs: int, rhs: int)  returns  int {
            let total = lhs + rhs
            return total
        }
    "#;

    let tokens = Lexer::new(source).tokenize().expect("lexing should pass");
    let ast = Parser::new(tokens)
        .parse()
        .expect("parsing should pass");
    let ir = ASTLowering::new()
        .lower_module(&ast)
        .expect("lowering should pass");

    assert_snapshot("lowering_ir.snap", &format_module(&ir));
}

#[test]
fn type_alias_lowers_to_the_target_aggregate_layout() {
    let source = r#"
        module alias_layout

        type Pair = (int, string)

        func make_pair() returns Pair {
            (42, "spectra")
        }

        public func main() returns int {
            let pair: Pair = make_pair()
            return pair.0
        }
    "#;

    let tokens = Lexer::new(source).tokenize().expect("lexing should pass");
    let ast = Parser::new(tokens)
        .parse()
        .expect("parsing should pass");
    let ir = ASTLowering::new()
        .lower_module(&ast)
        .expect("lowering should pass");

    let rendered = format_module(&ir);
    assert!(
        rendered.contains("fn make_pair() -> (int, string)"),
        "alias target should be visible in IR, got:\n{rendered}"
    );
}

#[test]
fn json_derived_static_error_field_lowers_as_string_comparison() {
    let source = r#"
        module json_lowering

        #[derive(Serialize, Deserialize)]
        record Profile {
            id: int,
            name: string,
        }

        public func main() returns int {
            let field = Profile::json_error_field("{}")
            if field != "" {
                return 1
            }
            return 0
        }
    "#;

    let tokens = Lexer::new(source).tokenize().expect("lexing should pass");
    let ast = Parser::new(tokens)
        .parse()
        .expect("parsing should pass");
    let ir = ASTLowering::new()
        .lower_module(&ast)
        .expect("lowering should pass");

    let rendered = format_module(&ir);
    assert!(
        rendered.contains("spectra.std.string.eq"),
        "derived error-field comparison should use string equality, got:\n{rendered}"
    );
    assert!(
        !rendered.contains("Profile_eq"),
        "derived error-field comparison must not request a struct equality helper"
    );
}
