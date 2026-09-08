// ============================================================================
// Session REPL core
//
// Model: an honest script-REPL. The session owns two accumulators:
//
//   * module-scope declarations (func/record/enum/import/type/const/static/
//     trait/impl) kept verbatim,
//   * session bindings introduced by top-level `let name = expr` statements.
//
// SpectraLang has no module-scope runtime bindings (`const`/`static` require
// compile-time initializers), so every candidate snapshot recompiles the
// declarations at module scope AND replays the accumulated `let` statements
// inside one generated entry function (`public func main`). Later expressions
// therefore resolve earlier bindings through ordinary body scoping, exactly
// what whole-snapshot validation guarantees.
//
// Every mutation is validated by compiling the WHOLE candidate snapshot
// through the normal front end (lex -> parse -> semantic -> lint) BEFORE the
// state is touched, so a failing line never corrupts the session. State
// persists by recompilation, not by incremental symbol tables — the same
// trade-off made by script-style REPLs sitting on top of a batch compiler.
//
// A bare expression is evaluated by appending `println(<expr>)` as the final
// statement of the replayed main body, compiling and JIT-running that
// snapshot through the normal pipeline, and discarding the generated entry
// afterwards — the persistent state never contains the wrapper. Because the
// JIT executes the function literally named `main`, a user-declared `main`
// collides with the replayed wrapper; that surfaces as a normal duplicate
// symbol error.
//
// Snapshots remember which *entered* line each generated line came from, so
// diagnostics are re-anchored onto the lines actually typed instead of the
// internal scaffold offsets.
//
// Everything here is TTY-free and side-effect-free so the loop can be unit
// tested: the interactive shell lives in cli_repl_project.rs.
// ============================================================================

const REPL_MODULE_NAME: &str = "__repl_session";
const REPL_PROBE_BINDING: &str = "__repl_value";
/// Indentation applied when embedding user statements inside the replayed
/// entry function. Reported columns are shifted back by this amount.
const REPLAYED_INDENT: usize = 4;

/// Keywords that introduce a top-level declaration accepted into the buffer.
const REPL_DECLARATION_KEYWORDS: &[&str] = &[
    "func", "fn", "record", "struct", "enum", "impl", "trait", "type", "const", "static",
    "import", "from", "export", "module",
];

/// Modifiers that may precede a declaration keyword and must be skipped when
/// classifying an input line.
const REPL_DECLARATION_MODIFIERS: &[&str] = &["public", "pub", "internal", "async"];

#[derive(Debug, Clone)]
struct ReplBinding {
    name: String,
    /// The statement exactly as entered (may span several lines).
    text: String,
}

#[derive(Debug, Default)]
struct ReplBuffer {
    declarations: Vec<String>,
    bindings: Vec<ReplBinding>,
}

impl ReplBuffer {
    fn new() -> Self {
        Self::default()
    }

    /// Module-scope view of the session (header plus declarations only).
    /// Compiles standalone; this is what `:save` writes and `:check` sees.
    fn source(&self) -> String {
        let mut source = repl_module_header();
        for declaration in &self.declarations {
            ensure_trailing_newline(&mut source);
            source.push_str("\n");
            source.push_str(declaration.trim_end());
            source.push('\n');
        }
        source
    }

    /// A printable view of the whole session state for `:buffer`.
    fn summary(&self) -> String {
        let mut rendered = self.source();
        if !self.bindings.is_empty() {
            rendered.push_str("\n// session bindings (replayed inside main)\n");
            for binding in &self.bindings {
                rendered.push_str(binding.text.trim_end());
                rendered.push('\n');
            }
        }
        rendered
    }

    /// Reset the session back to the bare module header.
    fn clear(&mut self) {
        self.declarations.clear();
        self.bindings.clear();
    }

    /// Append a classified input only when the WHOLE resulting snapshot
    /// parses, type-checks, and lints cleanly. On failure the state is
    /// untouched and the (user-line anchored) diagnostics are returned.
    ///
    /// Re-introducing an existing binding name replaces the previous value,
    /// matching ordinary REPL rebinding expectations.
    fn append_checked(
        &mut self,
        text: &str,
        options: &CompilationOptions,
    ) -> Result<AppendedEntry, Vec<String>> {
        let trimmed = text.trim_end();
        match repl_classify_input(trimmed) {
            ReplInput::Expression(_) => unreachable!(
                "callers must evaluate expressions; append_checked handles statements"
            ),
            ReplInput::Declaration(body) => {
                let trial = Self {
                    declarations: {
                        let mut copy = self.declarations.clone();
                        copy.push(body.to_string());
                        copy
                    },
                    bindings: self.bindings.clone(),
                };
                let snapshot = trial.build_snapshot(SnapshotTail::None);
                let diagnostics = snapshot.render_diagnostics(options);
                if diagnostics.is_empty() {
                    self.declarations.push(body.to_string());
                    Ok(AppendedEntry::Declaration)
                } else {
                    Err(diagnostics)
                }
            }
            ReplInput::Binding { name } => {
                let copy = self.bindings.clone();
                let text = match copy.iter().any(|binding| binding.name == name) {
                    // Re-declaration of a live binding: the language forbids
                    // same-scope shadowing but supports plain assignment, so
                    // the rebind replays as `name = <expr>` after the
                    // original declaration it initializes from.
                    true => match repl_rebind_assignment(trimmed) {
                        Some(assignment) => assignment,
                        None => {
                            return Err(vec![format!(
                                "cannot rebind '{}' in this session: expected 'let {} = <expr>'",
                                name, name
                            )])
                        }
                    },
                    false => trimmed.to_string(),
                };
                let mut next_bindings = copy;
                next_bindings.push(ReplBinding {
                    name: name.clone(),
                    text,
                });
                let next = Self {
                    declarations: self.declarations.clone(),
                    bindings: next_bindings,
                };
                let snapshot = next.build_snapshot(SnapshotTail::None);
                let diagnostics = snapshot.render_diagnostics(options);
                if diagnostics.is_empty() {
                    self.bindings = next.bindings;
                    Ok(AppendedEntry::Binding { name })
                } else {
                    Err(diagnostics)
                }
            }
        }
    }

    /// Merge the contents of a loaded file into the declarations. A leading
    /// `module <name>` header of the loaded file is stripped so it cannot
    /// collide with the session module.
    fn merge_checked(
        &mut self,
        contents: &str,
        options: &CompilationOptions,
    ) -> Result<(), Vec<String>> {
        match self.append_checked(&repl_strip_module_header(contents), options) {
            Ok(AppendedEntry::Declaration) | Ok(AppendedEntry::Binding { .. }) => Ok(()),
            Err(diagnostics) => Err(diagnostics),
        }
    }

    /// Full runnable snapshot: declarations, replayed bindings inside the
    /// generated entry point, and (for `SnapshotTail::Expression`) a printed
    /// value for `expression` as the last statement.
    fn build_snapshot(&self, tail: SnapshotTail<'_>) -> ReplSnapshot {
        let mut snapshot = ReplSnapshot::default();

        for line in repl_module_header().lines() {
            snapshot.push_scaffold(line);
        }
        for declaration in &self.declarations {
            snapshot.push_scaffold("");
            snapshot.push_user_text(declaration);
        }

        // Replay needs the entry function whenever session state exists.
        let replay = !self.bindings.is_empty() || tail.requires_entry_body();
        if replay {
            snapshot.push_scaffold("");
            snapshot.push_scaffold("public func main() returns int {");
            for binding in &self.bindings {
                snapshot.push_indented_user_text(&binding.text);
            }
            match tail {
                SnapshotTail::None => {}
                SnapshotTail::Expression(expression) => {
                    snapshot.push_indented_user_text(&format!("println({})", expression));
                }
                SnapshotTail::TypeProbe(expression) => {
                    snapshot.push_indented_user_text(&format!(
                        "let {} = ({})",
                        REPL_PROBE_BINDING, expression
                    ));
                }
            }
            snapshot.push_scaffold("    return 0");
            snapshot.push_scaffold("}");
        }

        snapshot
    }

    /// Inferred type of an existing session binding, by consulting the hints
    /// collected over the replayed entry body.
    fn infer_binding_type(
        &self,
        name: &str,
        options: &CompilationOptions,
    ) -> Result<String, Vec<String>> {
        let snapshot = self.build_snapshot(SnapshotTail::None);
        let analysis = analyze_document(&snapshot.source, "__repl_session.spectra", options, None);
        if !analysis.diagnostics.is_empty() {
            return Err(snapshot.render_diagnostics(options));
        }

        let mut inferred = None;
        for hint in collect_let_inlay_hints(&analysis) {
            if hint.name == name {
                inferred = Some(hint.ty);
            }
        }
        inferred.ok_or_else(|| {
            vec![format!("could not infer a type for binding '{}' yet", name)]
        })
    }
}

/// What a successful [`ReplBuffer::append_checked`] committed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AppendedEntry {
    Declaration,
    Binding { name: String },
}

/// How a generated snapshot ends.
enum SnapshotTail<'a> {
    /// Validate declarations plus replayed bindings only.
    None,
    /// Print the expression's value (normal evaluation path).
    Expression(&'a str),
    /// Bind the expression anonymously for type inference (`:type`).
    TypeProbe(&'a str),
}

impl SnapshotTail<'_> {
    fn requires_entry_body(&self) -> bool {
        matches!(self, SnapshotTail::Expression(_) | SnapshotTail::TypeProbe(_))
    }
}

/// One analyzed program image built from session state.
#[derive(Debug, Default)]
struct ReplSnapshot {
    source: String,
    /// Per snapshot line: its starting byte offset and, when the line comes
    /// from user input, how to re-anchor a location onto the entered text.
    line_table: Vec<(usize, Option<LineOrigin>)>,
}

#[derive(Debug, Clone, Copy)]
struct LineOrigin {
    /// 1-based line number within the user-entered text.
    entered_line: usize,
    /// Characters inserted before the original text (indentation).
    column_shift: usize,
}

impl ReplSnapshot {
    fn push_scaffold(&mut self, line: &str) {
        self.line_table.push((self.source.len(), None));
        self.source.push_str(line);
        self.source.push('\n');
    }

    fn push_indented_user_text(&mut self, text: &str) {
        self.push_user_text_indented(text, REPLAYED_INDENT);
    }

    /// Append user text verbatim except for a fixed left indent, recording
    /// per-line origins so diagnostics point back at the entered lines.
    fn push_user_text_indented(&mut self, text: &str, indent: usize) {
        let padding = " ".repeat(indent);
        let without_trailing = text.trim_end_matches(['\n', '\r']);
        for (index, line) in without_trailing.split(['\n']).enumerate() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            self.line_table.push((
                self.source.len(),
                Some(LineOrigin {
                    entered_line: index + 1,
                    column_shift: indent,
                }),
            ));
            if !line.is_empty() {
                self.source.push_str(&padding);
            }
            self.source.push_str(line);
            self.source.push('\n');
        }
    }

    fn push_user_text(&mut self, text: &str) {
        self.push_user_text_indented(text, 0);
    }

    /// Locate which table row contains `byte_offset`; returns the 1-based
    /// snapshot line number together with its (optional) user origin.
    fn locate(&self, byte_offset: usize) -> (usize, Option<LineOrigin>) {
        let count = self
            .line_table
            .partition_point(|&(start, _)| start <= byte_offset)
            .max(1);
        let index = (count - 1).min(self.line_table.len() - 1);
        let (start, origin) = self.line_table[index];
        (
            self.source[..start].matches('\n').count() + 1,
            origin,
        )
    }

    /// Analyze the snapshot and render its diagnostics with locations
    /// re-anchored onto the lines the user actually typed. Scaffold errors
    /// fall back to their internal position rather than being dropped.
    fn render_diagnostics(&self, options: &CompilationOptions) -> Vec<String> {
        let analysis = analyze_document(&self.source, "__repl_session.spectra", options, None);
        analysis
            .diagnostics
            .iter()
            .map(|error| self.render_error(error))
            .collect()
    }

    fn render_error(&self, error: &CompilerError) -> String {
        let (phase, message, span, context, hint) = match error {
            CompilerError::Lexical(e) => (
                "Lexical",
                e.message.as_str(),
                &e.span,
                e.context.as_deref(),
                e.hint.as_deref(),
            ),
            CompilerError::Parse(e) => (
                "Parse",
                e.message.as_str(),
                &e.span,
                e.context.as_deref(),
                e.hint.as_deref(),
            ),
            CompilerError::Semantic(e) => (
                "Semantic",
                e.message.as_str(),
                &e.span,
                e.context.as_deref(),
                e.hint.as_deref(),
            ),
            other => return other.to_string(),
        };

        let (snapshot_line, origin) = self.locate(span.start);
        let (mut line, mut column) =
            (span.start_location.line, span.start_location.column);
        if let Some(origin) = origin {
            line = origin.entered_line;
            column = column.saturating_sub(origin.column_shift).max(1);
        }
        let _ = snapshot_line;

        let mut rendered = format!(
            "{} error at line {}, column {}: {}",
            phase, line, column, message
        );
        if let Some(context) = context {
            rendered.push_str(&format!(" ({})", context));
        }
        if let Some(hint) = hint {
            rendered.push_str(&format!(" [hint: {}]", hint));
        }
        rendered
    }
}

fn ensure_trailing_newline(source: &mut String) {
    if !source.ends_with('\n') {
        source.push('\n');
    }
}

fn repl_module_header() -> String {
    // `import std.io` makes the stdlib console functions (println/print)
    // resolvable unqualified inside the session module, matching how normal
    // Spectra programs use them.
    format!("module {}\n\nimport std.io\n", REPL_MODULE_NAME)
}

/// Infer the type of `expression` in the context of the CURRENT session
/// (declarations plus replayed bindings) using the language service — the
/// same analyzer as `spectra check`. Returns the human-readable type (e.g.
/// `"int"`) or the diagnostics explaining why inference failed.
fn repl_infer_expression_type(
    buffer: &ReplBuffer,
    expression: &str,
    options: &CompilationOptions,
) -> Result<String, Vec<String>> {
    let snapshot = buffer.build_snapshot(SnapshotTail::TypeProbe(expression));
    let analysis = analyze_document(&snapshot.source, "__repl_session.spectra", options, None);
    if !analysis.diagnostics.is_empty() {
        return Err(snapshot.render_diagnostics(options));
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

/// How a prompt-sized input is processed.
#[derive(Debug)]
enum ReplInput<'a> {
    /// Top-level declaration text; appended verbatim to the module scope.
    Declaration(&'a str),
    /// A session binding introduced by a top-level `let` statement.
    Binding { name: String },
    /// Anything else; evaluated immediately.
    Expression(&'a str),
}

/// Classify an input: `let` statements become session bindings, keyword-led
/// lines become declarations, everything else is an expression.
fn repl_classify_input(input: &str) -> ReplInput<'_> {
    match repl_leading_significant_word(input) {
        Some(word) => {
            if word == "let" {
                if let Some(name) = repl_binding_name(input) {
                    return ReplInput::Binding { name };
                }
                // Unparseable let-shape (destructuring etc.): reject loudly
                // through the declaration path instead of wrapping it in a
                // bogus println evaluation.
                return ReplInput::Declaration(input);
            }
            if REPL_DECLARATION_KEYWORDS.contains(&word.as_str()) {
                return ReplInput::Declaration(input);
            }
            ReplInput::Expression(input)
        }
        None => ReplInput::Declaration(input),
    }
}

/// Extract the binding name from a top-level `let name = ...` /
/// `let name: T = ...` statement. Returns None for shapes we do not model
/// (multi-target destructuring etc.) so they take the honest rejection path.
fn repl_binding_name(input: &str) -> Option<String> {
    let rest = input.trim_start().strip_prefix("let")?;
    if !rest.starts_with(char::is_whitespace) {
        return None; // `letx` glued forms are not bindings.
    }
    let rest = rest.trim_start();
    let head_end = rest.find('=')?;
    let head = rest[..head_end].trim_end();
    let name = match head.find(':') {
        Some(colon) => head[..colon].trim_end(),
        None => head,
    };
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_alphabetic() || first == '_' => {}
        _ => return None,
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some(name.to_string())
}

/// Convert a re-entered `let name = <expr>` (or `let name: T = <expr>`)
/// statement into the assignment `name = <expr>` replayed after the original
/// declaration. Returns None when the head does not parse as a simple
/// single-target binding, leaving rejection to the caller.
fn repl_rebind_assignment(input: &str) -> Option<String> {
    let rest = input.trim_start().strip_prefix("let")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start();
    let head_end = rest.find('=')?;
    let head = rest[..head_end].trim_end();
    let name = match head.find(':') {
        Some(colon) => head[..colon].trim_end(),
        None => head,
    };
    // The initializer is everything after the head's '=' — but only a bare
    // assignment operator qualifies; `==`/`<=` etc. are malformed here.
    let value = rest[head_end + 1..].trim_start();
    if value.starts_with('=') {
        return None;
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') || name.is_empty()
        || name.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some(format!("{} = {}", name, value.trim_end()))
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
        let appended = buffer
            .append_checked(declaration, &options())
            .expect("declaration should compile cleanly");
        assert_eq!(appended, AppendedEntry::Declaration);
    }

    fn bind(buffer: &mut ReplBuffer, statement: &str) -> String {
        match buffer
            .append_checked(statement, &options())
            .expect("binding should compile cleanly")
        {
            AppendedEntry::Binding { name } => name,
            other => panic!("expected binding commit, got {:?}", other),
        }
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
        let called =
            repl_infer_expression_type(&buffer, "double(21)", &options())
                .expect("call to session function should infer");
        assert_eq!(called, "int");

        let composed =
            repl_infer_expression_type(&buffer, "double(3) + double(4)", &options())
                .expect("arithmetic over session function should infer");
        assert_eq!(composed, "int");

        assert!(buffer.source().contains("func double(n: int)"));
    }

    #[test]
    fn session_binding_persists_across_snapshot_generation() {
        let mut buffer = ReplBuffer::new();
        define(
            &mut buffer,
            "func double(n: int) returns int {\n    return n * 2\n}",
        );
        let bound = bind(&mut buffer, "let x = double(21)");
        assert_eq!(bound, "x");
        bind(&mut buffer, "let y = x * 10");

        // Whole-snapshot compilation must accept bindings referencing both
        // session functions and earlier bindings.
        let snapshot = buffer.build_snapshot(SnapshotTail::Expression("y + 1"));
        let diagnostics = snapshot.render_diagnostics(&options());
        assert!(
            diagnostics.is_empty(),
            "snapshot referencing accumulated bindings must compile, got: {:?}",
            diagnostics
        );
        assert!(snapshot.source.contains("let x = double(21)"));
        assert!(snapshot.source.contains("println(y + 1)"));

        // The persistent state must NOT contain the generated wrapper.
        assert!(!buffer.summary().contains("public func main"));
        assert!(!buffer.summary().contains("println(y + 1)"));
    }

    #[test]
    fn rebinding_replaces_earlier_value() {
        let mut buffer = ReplBuffer::new();
        bind(&mut buffer, "let x = 1");
        bind(&mut buffer, "let x = x + 1");

        let snapshot = buffer.build_snapshot(SnapshotTail::None);
        let diagnostics = snapshot.render_diagnostics(&options());
        assert!(
            diagnostics.is_empty(),
            "rebuilt snapshot after rebinding must compile, got: {:?}",
            diagnostics
        );
        let occurrences = snapshot.source.matches("let x = ").count();
        assert_eq!(occurrences, 1, "rebind must replace, not duplicate");

        let inferred =
            repl_infer_expression_type(&buffer, "x", &options()).unwrap();
        assert_eq!(inferred, "int");
    }

    #[test]
    fn binding_type_is_reported_for_echo() {
        let mut buffer = ReplBuffer::new();
        define(
            &mut buffer,
            "func greet(name: string) returns string {\n    return name\n}",
        );
        bind(&mut buffer, "let who = greet(\"ada\")");

        let ty = buffer.infer_binding_type("who", &options()).unwrap();
        assert_eq!(ty, "string");
    }

    #[test]
    fn failed_declaration_reverts_buffer() {
        let mut buffer = ReplBuffer::new();
        define(
            &mut buffer,
            "func double(n: int) returns int {\n    return n * 2\n}",
        );
        let before = buffer.source().to_string();
        let bindings_before = buffer.bindings.len();

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
        assert_eq!(buffer.bindings.len(), bindings_before);
    }

    #[test]
    fn failed_binding_reverts_state_and_reports_entered_line() {
        let mut buffer = ReplBuffer::new();
        bind(&mut buffer, "let x = 1");

        let failing = buffer.append_checked("let y = unknown_thing + 1", &options());
        match failing {
            Err(diagnostics) => {
                let rendered = diagnostics.join("\n");
                assert!(
                    rendered.contains("Undefined variable or function 'unknown_thing'"),
                    "diagnostics should mention the undefined name, got: {}",
                    rendered
                );
                assert!(
                    rendered.contains("line 1,"),
                    "diagnostic must anchor on entered line 1, got: {}",
                    rendered
                );
                assert!(!rendered.contains("line 6,"), "no stale scaffold lines: {}", rendered);
            }
            Ok(_) => panic!("undefined variable inside binding must be rejected"),
        }
        assert!(buffer.bindings.iter().all(|b| b.name != "y"));

        // State still usable afterwards.
        bind(&mut buffer, "let z = x * 2");
        let ty = buffer.infer_binding_type("z", &options()).unwrap();
        assert_eq!(ty, "int");
    }

    #[test]
    fn reset_clears_buffer_back_to_header() {
        let mut buffer = ReplBuffer::new();
        define(&mut buffer, "func answer() returns int {\n    return 42\n}");
        bind(&mut buffer, "let stored = answer()");

        buffer.clear();
        assert_eq!(buffer.source(), repl_module_header());
        let diagnostics =
            repl_infer_expression_type(&buffer, "answer()", &options()).unwrap_err();
        assert!(
            !diagnostics.is_empty(),
            "cleared session must have forgotten the function"
        );
        assert!(buffer.bindings.is_empty());
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

        let inferred = repl_infer_expression_type(&target, "saved_answer()", &options())
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
        let inferred = repl_infer_expression_type(&buffer, "helper()", &options())
            .expect("merged helper should be visible");
        assert_eq!(inferred, "int");
    }

    #[test]
    fn classification_matches_language_surface() {
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
                matches!(repl_classify_input(declaration), ReplInput::Declaration(_)),
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
                matches!(repl_classify_input(expression), ReplInput::Expression(_)),
                "'{}' should classify as an expression",
                expression
            );
        }

        // `let` statements classify as bindings with their names extracted.
        for (input, expected) in [
            ("let x = 1+2", "x"),
            ("let greeting: string = \"hi\"", "greeting"),
            ("let _hidden = 3", "_hidden"),
        ] {
            match repl_classify_input(input) {
                ReplInput::Binding { name } => assert_eq!(name, expected),
                other => panic!("'{}' should classify as binding, got {:?}", input, other),
            }
        }
    }

    #[test]
    fn type_probe_reports_inferred_types() {
        let mut buffer = ReplBuffer::new();
        define(
            &mut buffer,
            "func greet(name: string) returns string {\n    return name\n}",
        );

        let string_ty = repl_infer_expression_type(&buffer, "\"hi\"", &options()).unwrap();
        assert_eq!(string_ty, "string");

        let call_ty =
            repl_infer_expression_type(&buffer, "greet(\"hi\")", &options())
                .expect("call should infer");
        assert_eq!(call_ty, "string");

        // Expressions resolve bindings introduced earlier in the session.
        bind(&mut buffer, "let base = 40");
        let through_binding =
            repl_infer_expression_type(&buffer, "base + 2", &options())
                .expect("expression over binding should infer");
        assert_eq!(through_binding, "int");

        let error = repl_infer_expression_type(&buffer, "nope_unknown_ident", &options())
            .unwrap_err();
        assert!(!error.is_empty());
    }

    #[test]
    fn diagnostics_anchor_on_multi_line_block_inputs() {
        let mut buffer = ReplBuffer::new();
        bind(&mut buffer, "let flag = true");
        bind(&mut buffer, "let other = 2");

        // A later expression that breaks on its own single line reports 1.
        let snapshot = buffer.build_snapshot(SnapshotTail::Expression("flag + other_typo"));
        let diagnostics = snapshot.render_diagnostics(&options());
        let rendered = diagnostics.join("\n");
        assert!(
            rendered.contains("line 1,"),
            "single-line expression errors must map to entered line 1, got: {}",
            rendered
        );
    }
}
