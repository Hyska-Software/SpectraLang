// ============================================================================
// Session REPL core
//
// Model: an honest script-REPL. The session owns a persistent *module buffer*
// (`module __repl_session`) that accumulates top-level declarations
// (func/record/enum/import/type/const/static/trait/impl). Every mutation is
// validated by compiling the WHOLE candidate buffer through the normal front
// end (lex -> parse -> semantic -> lint) BEFORE the buffer is touched, so a
// failing line never corrupts the session. State therefore persists by
// recompilation, not by incremental symbol tables — the same trade-off made by
// script-style REPLs sitting on top of a batch compiler.
//
// A bare expression is evaluated by generating a temporary entry point
// (`public func main() returns int { println(<expr>) return 0 }`) appended to
// the CURRENT buffer, compiling and JIT-running that snapshot through the
// normal pipeline, and discarding the generated function afterwards — the
// persistent buffer never contains the wrapper. Because the JIT executes the
// function literally named `main`, a user-declared `main` in the session
// collides with the expression wrapper; that surfaces as a normal duplicate
// symbol error.
//
// Everything here is TTY-free and side-effect-free so the loop can be unit
// tested: the interactive shell lives in cli_repl_project.rs.
// ============================================================================

const REPL_MODULE_NAME: &str = "__repl_session";
const REPL_PROBE_BINDING: &str = "__repl_value";
const REPL_PROBE_FUNCTION: &str = "__repl_type_probe";
const REPL_EXPRESSION_ENTRY: &str = "main";

/// Keywords that introduce a top-level declaration accepted into the buffer.
const REPL_DECLARATION_KEYWORDS: &[&str] = &[
    "func", "fn", "record", "struct", "enum", "impl", "trait", "type", "const", "static",
    "import", "from", "export", "module",
];

/// Modifiers that may precede a declaration keyword and must be skipped when
/// classifying an input line.
const REPL_DECLARATION_MODIFIERS: &[&str] = &["public", "pub", "internal", "async"];

#[derive(Debug)]
struct ReplBuffer {
    source: String,
}

impl ReplBuffer {
    fn new() -> Self {
        Self {
            source: repl_module_header(),
        }
    }

    fn source(&self) -> &str {
        &self.source
    }

    /// Reset the session back to the bare module header.
    fn clear(&mut self) {
        self.source = repl_module_header();
    }

    /// The buffer that WOULD result from appending `declaration`.
    fn candidate(&self, declaration: &str) -> String {
        let mut candidate = ensure_trailing_newline(self.source.clone());
        candidate.push_str(declaration.trim_end());
        candidate.push('\n');
        candidate.push('\n');
        candidate
    }

    /// Append `declaration` only when the whole candidate buffer parses,
    /// type-checks, and lints cleanly. On failure the buffer is untouched and
    /// the diagnostics are returned.
    fn append_checked(
        &mut self,
        declaration: &str,
        options: &CompilationOptions,
    ) -> Result<(), Vec<String>> {
        let candidate = self.candidate(declaration);
        let diagnostics = repl_session_diagnostics(&candidate, options);
        if diagnostics.is_empty() {
            self.source = candidate;
            Ok(())
        } else {
            Err(diagnostics)
        }
    }

    /// Merge the contents of a loaded file into the buffer. A leading
    /// `module <name>` header of the loaded file is stripped so it cannot
    /// collide with the session module.
    fn merge_checked(
        &mut self,
        contents: &str,
        options: &CompilationOptions,
    ) -> Result<(), Vec<String>> {
        self.append_checked(&repl_strip_module_header(contents), options)
    }

    /// Full compilable snapshot: buffer plus a temporary entry point that
    /// prints the value of `expression`.
    fn expression_snapshot(&self, expression: &str) -> String {
        let mut snapshot = ensure_trailing_newline(self.source.clone());
        snapshot.push_str("\npublic func ");
        snapshot.push_str(REPL_EXPRESSION_ENTRY);
        snapshot.push_str("() returns int {\n");
        snapshot.push_str(&format!("    println({})\n", expression));
        snapshot.push_str("    return 0\n}\n");
        snapshot
    }

    /// Snapshot used by `:type`: binds the expression to an un-annotated
    /// `let` so the language service can report its inferred type.
    fn type_probe_snapshot(&self, expression: &str) -> String {
        let mut snapshot = ensure_trailing_newline(self.source.clone());
        snapshot.push_str("\nfunc ");
        snapshot.push_str(REPL_PROBE_FUNCTION);
        snapshot.push_str("() {\n");
        snapshot.push_str(&format!(
            "    let {} = ({})\n",
            REPL_PROBE_BINDING, expression
        ));
        snapshot.push_str("}\n");
        snapshot
    }
}

fn ensure_trailing_newline(mut source: String) -> String {
    if !source.ends_with('\n') {
        source.push('\n');
    }
    source
}

fn repl_module_header() -> String {
    // `import std.io` makes the stdlib console functions (println/print)
    // resolvable unqualified inside the session module, matching how normal
    // Spectra programs use them.
    format!("module {}\n\nimport std.io\n", REPL_MODULE_NAME)
}

/// Parse + semantic + lint diagnostics for a full session source. Empty means
/// the snapshot is safe to adopt.
fn repl_session_diagnostics(source: &str, options: &CompilationOptions) -> Vec<String> {
    let analysis = analyze_document(source, "__repl_session.spectra", options, None);
    analysis
        .diagnostics
        .iter()
        .map(|error| error.to_string())
        .collect()
}

/// Infer the type of `expression` in the context of `buffer_source` using the
/// language service (same analyzer as `spectra check`). Returns the
/// human-readable type (e.g. `"int"`) or the diagnostics explaining why
/// inference failed.
fn repl_infer_expression_type(
    buffer_source: &str,
    expression: &str,
    options: &CompilationOptions,
) -> Result<String, Vec<String>> {
    let probe = ReplBuffer {
        source: buffer_source.to_string(),
    }
    .type_probe_snapshot(expression);

    let analysis = analyze_document(&probe, "__repl_session.spectra", options, None);
    if !analysis.diagnostics.is_empty() {
        return Err(analysis
            .diagnostics
            .iter()
            .map(|error| error.to_string())
            .collect());
    }

    for hint in collect_let_inlay_hints(&analysis) {
        if hint.name == REPL_PROBE_BINDING {
            return Ok(hint.ty);
        }
    }

    Err(vec![format!(
        "could not infer a type for '{}' (the expression may be unit-typed)",
        expression
    )])
}

/// Classify an input line: declarations go into the persistent buffer,
/// anything else is treated as an expression to evaluate.
fn repl_is_declaration(input: &str) -> bool {
    match repl_leading_significant_word(input) {
        Some(word) => REPL_DECLARATION_KEYWORDS.contains(&word.as_str()),
        None => false,
    }
}

/// First meaningful token of the input, skipping visibility/async modifiers
/// and attribute tokens, lowercased.
fn repl_leading_significant_word(input: &str) -> Option<String> {
    for word in input.split_whitespace() {
        if word.starts_with('@') || word.starts_with("#[") {
            continue;
        }
        let lower = word.trim_start_matches('(').to_ascii_lowercase();
        if REPL_DECLARATION_MODIFIERS.contains(&lower.as_str()) {
            continue;
        }
        return Some(lower);
    }
    None
}

/// Remove a leading `module <name>` line from loaded file contents so the
/// text can be merged into the session module, and drop bare `import std.io`
/// lines (the session header already provides that import; re-importing it
/// verbatim would only accumulate duplicates across :save/:load round-trips).
/// Comments and blank lines before the header are preserved.
fn repl_strip_module_header(contents: &str) -> String {
    let mut result = String::new();
    let mut header_handled = false;
    for line in contents.lines() {
        if !header_handled {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                result.push_str(line);
                result.push('\n');
                continue;
            }
            header_handled = true;
            if trimmed.split_whitespace().next() == Some("module") {
                // Skip the foreign module declaration itself.
                continue;
            }
        }
        let trimmed = line.trim();
        if trimmed == "import std.io" {
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }
    result
}
#[cfg(test)]
mod repl_session_tests {
    use super::*;

    fn options() -> CompilationOptions {
        CompilationOptions::default()
    }

    fn define(buffer: &mut ReplBuffer, declaration: &str) {
        buffer
            .append_checked(declaration, &options())
            .expect("declaration should compile cleanly");
    }

    #[test]
    fn session_defines_func_then_call_persists_value() {
        let mut buffer = ReplBuffer::new();
        define(
            &mut buffer,
            "func double(n: int) returns int {\n    return n * 2\n}",
        );

        // The earlier definition is visible to later inputs: inference of a
        // call resolves against everything accumulated so far.
        let called = repl_infer_expression_type(buffer.source(), "double(21)", &options())
            .expect("call to session function should infer");
        assert_eq!(called, "int");

        let composed =
            repl_infer_expression_type(buffer.source(), "double(3) + double(4)", &options())
                .expect("arithmetic over session function should infer");
        assert_eq!(composed, "int");

        assert!(buffer.source().contains("func double(n: int)"));
    }

    #[test]
    fn expression_snapshot_prints_value_through_entry_point() {
        let mut buffer = ReplBuffer::new();
        define(
            &mut buffer,
            "func double(n: int) returns int {\n    return n * 2\n}",
        );

        let snapshot = buffer.expression_snapshot("double(21)");
        assert!(snapshot.contains("println(double(21))"));
        assert!(snapshot.contains("public func main() returns int"));
        let diagnostics = repl_session_diagnostics(&snapshot, &options());
        assert!(
            diagnostics.is_empty(),
            "expression snapshot must compile, got: {:?}",
            diagnostics
        );

        // The persistent buffer itself must NOT contain the generated wrapper.
        assert!(!buffer.source().contains("public func main"));
        assert!(!buffer.source().contains("println(double(21))"));
    }

    #[test]
    fn failed_expression_reports_diagnostics_instead_of_running() {
        let mut buffer = ReplBuffer::new();
        define(
            &mut buffer,
            "func double(n: int) returns int {\n    return n * 2\n}",
        );

        let snapshot = buffer.expression_snapshot("double(\"nope\")");
        let diagnostics = repl_session_diagnostics(&snapshot, &options());
        assert!(
            !diagnostics.is_empty(),
            "type error in expression must produce diagnostics"
        );
    }

    #[test]
    fn failed_declaration_reverts_buffer() {
        let mut buffer = ReplBuffer::new();
        define(
            &mut buffer,
            "func double(n: int) returns int {\n    return n * 2\n}",
        );
        let before = buffer.source().to_string();

        let broken = buffer.append_checked("func broken( {", &options());
        assert!(broken.is_err(), "malformed declaration must be rejected");
        assert_eq!(buffer.source(), before, "buffer must be unchanged");

        let mismatched = buffer.append_checked(
            "func mismatched(lhs: int, rhs: string) returns int {\n    return lhs + rhs\n}",
            &options(),
        );
        assert!(mismatched.is_err(), "type error must be rejected");
        assert_eq!(buffer.source(), before, "buffer must be unchanged");

        // Buffer still usable afterwards.
        define(
            &mut buffer,
            "func triple(n: int) returns int {\n    return n * 3\n}",
        );
        assert!(buffer.source().contains("func triple"));
    }

    #[test]
    fn reset_clears_buffer_back_to_header() {
        let mut buffer = ReplBuffer::new();
        define(&mut buffer, "func answer() returns int {\n    return 42\n}");

        buffer.clear();
        assert_eq!(buffer.source(), repl_module_header());
        let diagnostics =
            repl_infer_expression_type(buffer.source(), "answer()", &options()).unwrap_err();
        assert!(
            !diagnostics.is_empty(),
            "cleared session must have forgotten the function"
        );
    }

    #[test]
    fn save_and_load_roundtrip_merges_state() {
        let saved = {
            let mut buffer = ReplBuffer::new();
            define(
                &mut buffer,
                "func saved_answer() returns int {\n    return 42\n}",
            );
            buffer.source().to_string()
        };

        let path = std::env::temp_dir().join(format!(
            "spectra-repl-save-test-{}.spectra",
            std::process::id()
        ));
        std::fs::write(&path, &saved).expect("write saved session");

        let mut target = ReplBuffer::new();
        let contents = std::fs::read_to_string(&path).expect("read saved session");
        target
            .merge_checked(&contents, &options())
            .expect("merge of valid session dump must succeed");

        let inferred = repl_infer_expression_type(target.source(), "saved_answer()", &options())
            .expect("merged definition should be callable");
        assert_eq!(inferred, "int");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn load_strips_foreign_module_header() {
        let foreign = "module some_app\n\nimport std.io\n\nfunc helper() returns int {\n    return 7\n}\n";
        let stripped = repl_strip_module_header(foreign);
        assert!(!stripped.contains("module"));
        // The session header already carries `import std.io`; a saved dump
        // must not re-add it as a duplicate on :load.
        assert_eq!(stripped.matches("import std.io").count(), 0);

        let mut buffer = ReplBuffer::new();
        buffer
            .merge_checked(foreign, &options())
            .expect("merge must succeed after stripping the header");
        assert_eq!(buffer.source().matches("import std.io").count(), 1);
        let inferred = repl_infer_expression_type(buffer.source(), "helper()", &options())
            .expect("merged helper should be visible");
        assert_eq!(inferred, "int");
    }

    #[test]
    fn declaration_classification_matches_language_surface() {
        for declaration in [
            "func f() {}",
            "fn g() {}",
            "public func visible() returns int { return 1 }",
            "record Point { x: int, y: int }",
            "enum Color { Red, Green }",
            "impl Point { func sum(&self) returns int { return self.x + self.y } }",
            "trait Shape { func area(&self) returns float }",
            "type Meters = int",
            "const LIMIT: int = 10",
            "static COUNTER: int = 0",
            "import std.io",
            "from std.io import println",
        ] {
            assert!(
                repl_is_declaration(declaration),
                "'{}' should classify as a declaration",
                declaration
            );
        }

        for expression in [
            "1 + 2",
            "double(21)",
            "\"hello\"",
            "(1, 2)",
            "[1, 2, 3]",
            "-x",
            "point.x",
        ] {
            assert!(
                !repl_is_declaration(expression),
                "'{}' should classify as an expression",
                expression
            );
        }
    }

    #[test]
    fn type_probe_reports_inferred_types() {
        let mut buffer = ReplBuffer::new();
        define(
            &mut buffer,
            "func greet(name: string) returns string {\n    return name\n}",
        );

        let string_ty = repl_infer_expression_type(buffer.source(), "\"hi\"", &options()).unwrap();
        assert_eq!(string_ty, "string");

        let call_ty = repl_infer_expression_type(buffer.source(), "greet(\"hi\")", &options())
            .expect("call should infer");
        assert_eq!(call_ty, "string");

        let error =
            repl_infer_expression_type(buffer.source(), "nope_unknown_ident", &options())
                .unwrap_err();
        assert!(!error.is_empty());
    }
}
