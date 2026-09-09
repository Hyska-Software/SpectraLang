use super::*;

// ============================================================================
// deprecated-task-spawn lint — stable code E036 when escalated to error via
// --deny (DeprecateSpawn). This file is NOT a Rust module: it is textually
// included by `compiler/src/lint/mod.rs` so the rule can hook into the private
// LintRunner.
// Owner: DeprecateSpawn. Do not edit without coordinating.
// ============================================================================

impl<'a> crate::lint::LintRunner<'a> {
    /// BEGIN deprecated-task-spawn rule core (DeprecateSpawn)
    ///
    /// Warns on calls to the legacy `concurrent.task_spawn(value)` builtin,
    /// which only records an already-evaluated value instead of executing work
    /// concurrently. Real execution belongs in `concurrent.task_spawn_fn`,
    /// which takes a closure the runtime schedules on its worker pool.
    pub(crate) fn check_deprecated_task_spawn(&mut self, callee: &Expression) {
        if !self
            .options
            .is_enabled(crate::lint::LintRule::DeprecatedTaskSpawn)
        {
            return;
        }

        // `concurrent.task_spawn(...)` parses as a MethodCall on the module
        // identifier; a Call whose callee is the same FieldAccess shape is
        // accepted too so first-class references stay covered.
        let is_legacy_spawn = match &callee.kind {
            crate::ast::ExpressionKind::MethodCall {
                object,
                method_name,
                ..
            } => {
                matches!(object.kind, crate::ast::ExpressionKind::Identifier(ref name) if name == "concurrent")
                    && method_name == "task_spawn"
            }
            crate::ast::ExpressionKind::FieldAccess { object, field } => {
                matches!(object.kind, crate::ast::ExpressionKind::Identifier(ref name) if name == "concurrent")
                    && field == "task_spawn"
            }
            _ => false,
        };

        if !is_legacy_spawn {
            return;
        }

        self.diagnostics.push(crate::lint::LintDiagnostic {
            rule: crate::lint::LintRule::DeprecatedTaskSpawn,
            message: "call to legacy 'concurrent.task_spawn' records a pre-evaluated value; it does not execute work concurrently".to_string(),
            span: callee.span,
            note: Some(
                "use 'concurrent.task_spawn_fn(|value| ...)' to schedule real concurrent execution"
                    .to_string(),
            ),
            secondary_span: None,
        });
    }
    // END deprecated-task-spawn rule core (DeprecateSpawn)
}

impl<'a> crate::lint::LintRunner<'a> {
    /// BEGIN deprecated-text-embed rule core (ML)
    ///
    /// Warns on calls to `ml.text_embed(text, dim)`, which is a documented
    /// deterministic hashing baseline, not a model-backed embedding. Real
    /// embeddings belong in `ml.text_embed_model` / `ml.text_embed_model_session`.
    /// The `fn(string, int) -> int` contract is unchanged; this is a
    /// steering diagnostic only.
    pub(crate) fn check_deprecated_text_embed(&mut self, callee: &Expression) {
        if !self
            .options
            .is_enabled(crate::lint::LintRule::DeprecatedTextEmbed)
        {
            return;
        }

        // `ml.text_embed(...)` parses as a MethodCall on the module
        // identifier; a Call whose callee is the same FieldAccess shape is
        // accepted too so first-class references stay covered.
        let is_legacy_embed = match &callee.kind {
            crate::ast::ExpressionKind::MethodCall {
                object,
                method_name,
                ..
            } => {
                matches!(&object.kind, crate::ast::ExpressionKind::Identifier(name) if name == "ml")
                    && method_name == "text_embed"
            }
            crate::ast::ExpressionKind::FieldAccess { object, field } => {
                matches!(&object.kind, crate::ast::ExpressionKind::Identifier(name) if name == "ml")
                    && field == "text_embed"
            }
            _ => false,
        };
        if !is_legacy_embed {
            return;
        }

        self.diagnostics.push(crate::lint::LintDiagnostic {
            rule: crate::lint::LintRule::DeprecatedTextEmbed,
            message: "call to 'ml.text_embed' uses the deterministic hashing baseline; it is not a model-backed embedding".to_string(),
            span: callee.span,
            note: Some(
                "use 'ml.text_embed_model' / 'ml.text_embed_model_session' for model-backed embeddings"
                    .to_string(),
            ),
            secondary_span: None,
        });
    }
    // END deprecated-text-embed rule core (ML)
}

impl<'a> crate::lint::LintRunner<'a> {
    /// BEGIN deprecated-onnx-export rule core (ML)
    ///
    /// Warns on calls to `ml.onnx_export(path, kind)`, which emits a
    /// deterministic seeded fixture template, not a model export. Real
    /// exports belong in `ml.onnx_export_weights`, which fills initializers
    /// from caller-supplied live tensors. The `fn(string, string) -> string`
    /// contract is unchanged; this is a steering diagnostic only.
    pub(crate) fn check_deprecated_onnx_export(&mut self, callee: &Expression) {
        if !self
            .options
            .is_enabled(crate::lint::LintRule::DeprecatedOnnxExport)
        {
            return;
        }

        // `ml.onnx_export(...)` parses as a MethodCall on the module
        // identifier; a Call whose callee is the same FieldAccess shape is
        // accepted too so first-class references stay covered.
        let is_legacy_export = match &callee.kind {
            crate::ast::ExpressionKind::MethodCall {
                object,
                method_name,
                ..
            } => {
                matches!(&object.kind, crate::ast::ExpressionKind::Identifier(name) if name == "ml")
                    && method_name == "onnx_export"
            }
            crate::ast::ExpressionKind::FieldAccess { object, field } => {
                matches!(&object.kind, crate::ast::ExpressionKind::Identifier(name) if name == "ml")
                    && field == "onnx_export"
            }
            _ => false,
        };
        if !is_legacy_export {
            return;
        }

        self.diagnostics.push(crate::lint::LintDiagnostic {
            rule: crate::lint::LintRule::DeprecatedOnnxExport,
            message: "call to 'ml.onnx_export' emits a deterministic fixture template; it is not a trained-model export".to_string(),
            span: callee.span,
            note: Some(
                "use 'ml.onnx_export_weights' to export live training tensors"
                    .to_string(),
            ),
            secondary_span: None,
        });
    }
    // END deprecated-onnx-export rule core (ML)
}

#[cfg(test)]
mod deprecated_task_spawn_tests {
    use crate::lexer::Lexer;
    use crate::lint::{lint_module, LintOptions, LintRule};
    use crate::parser::Parser;
    use std::collections::HashSet;

    pub(crate) fn parse(source: &str) -> crate::ast::Module {
        let tokens = Lexer::new(source)
            .tokenize()
            .expect("lexer should not fail");
        Parser::new(tokens, HashSet::new()).parse().expect("parse")
    }

    fn spawn_diagnostics(source: &str) -> Vec<String> {
        lint_module(&parse(source), &LintOptions::default())
            .into_iter()
            .filter(|diagnostic| diagnostic.rule == LintRule::DeprecatedTaskSpawn)
            .map(|diagnostic| diagnostic.message)
            .collect()
    }

    #[test]
    fn warns_on_concurrent_task_spawn_call() {
        let diagnostics = spawn_diagnostics(
            r#"
            module demo
            func main() returns int {
                concurrent.reset()
                let task = concurrent.task_spawn(41)
                return concurrent.task_join(task)
            }
        "#,
        );
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("pre-evaluated value"));
    }

    #[test]
    fn task_spawn_fn_and_other_calls_stay_silent() {
        assert!(spawn_diagnostics(
            r#"
            module demo
            func main() returns int {
                concurrent.reset()
                let task = concurrent.task_spawn_fn(| value: int | { value + 1 }, 1)
                return concurrent.task_join(task)
            }
        "#
        )
        .is_empty());
        assert!(spawn_diagnostics(
            r#"
            module demo
            func main() returns int {
                return time.monotonic_millis() as int
            }
        "#
        )
        .is_empty());
    }

    #[test]
    fn same_named_method_on_other_objects_is_not_flagged() {
        assert!(spawn_diagnostics(
            r#"
            module demo
            func main() returns int {
                other.task_spawn(1)
                return 0
            }
        "#
        )
        .is_empty());
    }

    #[test]
    fn disabling_the_rule_suppresses_the_warning() {
        let mut options = LintOptions::disabled();
        options.enable_rule(LintRule::NarrowingCast);
        let diagnostics = lint_module(
            &parse(
                r#"
                module demo
                func main() returns int {
                    let task = concurrent.task_spawn(41)
                    return concurrent.task_join(task)
                }
            "#,
            ),
            &options,
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn rule_metadata_uses_reserved_code_e036() {
        assert_eq!(LintRule::DeprecatedTaskSpawn.code(), "deprecated-task-spawn");
        assert_eq!(
            LintRule::from_code("deprecated-task-spawn"),
            Some(LintRule::DeprecatedTaskSpawn)
        );
        assert_eq!(
            LintRule::DeprecatedTaskSpawn.stable_error_code(),
            Some("E036")
        );
    }
}

#[cfg(test)]
mod deprecated_text_embed_tests {
    use crate::lexer::Lexer;
    use crate::lint::{lint_module, LintOptions, LintRule};
    use crate::parser::Parser;
    use std::collections::HashSet;

    pub(crate) fn parse(source: &str) -> crate::ast::Module {
        let tokens = Lexer::new(source)
            .tokenize()
            .expect("lexer should not fail");
        Parser::new(tokens, HashSet::new()).parse().expect("parse")
    }

    fn embed_diagnostics(source: &str) -> Vec<String> {
        lint_module(&parse(source), &LintOptions::default())
            .into_iter()
            .filter(|diagnostic| diagnostic.rule == LintRule::DeprecatedTextEmbed)
            .map(|diagnostic| diagnostic.message)
            .collect()
    }

    #[test]
    fn warns_on_ml_text_embed_hashing_baseline() {
        let diagnostics = embed_diagnostics(
            r#"
            module demo
            func main() returns int {
                let vec = ml.text_embed("rag retrieval", 8)
                return 0
            }
        "#,
        );
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("hashing baseline"));
    }

    #[test]
    fn model_backed_variants_and_other_calls_stay_silent() {
        assert!(embed_diagnostics(
            r#"
            module demo
            func main() returns int {
                let session = ml.text_embed_model_session("model.spar")
                let vec = ml.text_embed_model(session, 1, "hello", 2)
                return 0
            }
        "#,
        )
        .is_empty());
        assert!(embed_diagnostics(
            r#"
            module demo
            func main() returns int {
                let vec = other.text_embed("x", 8)
                return 0
            }
        "#,
        )
        .is_empty());
    }

    #[test]
    fn rule_metadata_uses_reserved_code_e037() {
        assert_eq!(LintRule::DeprecatedTextEmbed.code(), "deprecated-text-embed");
        assert_eq!(
            LintRule::from_code("deprecated-text-embed"),
            Some(LintRule::DeprecatedTextEmbed)
        );
        assert_eq!(
            LintRule::DeprecatedTextEmbed.stable_error_code(),
            Some("E037")
        );
    }
}

#[cfg(test)]
mod deprecated_onnx_export_tests {
    use crate::lexer::Lexer;
    use crate::lint::{lint_module, LintOptions, LintRule};
    use crate::parser::Parser;
    use std::collections::HashSet;

    pub(crate) fn parse(source: &str) -> crate::ast::Module {
        let tokens = Lexer::new(source)
            .tokenize()
            .expect("lexer should not fail");
        Parser::new(tokens, HashSet::new()).parse().expect("parse")
    }

    fn export_diagnostics(source: &str) -> Vec<String> {
        lint_module(&parse(source), &LintOptions::default())
            .into_iter()
            .filter(|diagnostic| diagnostic.rule == LintRule::DeprecatedOnnxExport)
            .map(|diagnostic| diagnostic.message)
            .collect()
    }

    #[test]
    fn warns_on_ml_onnx_export_fixture_template() {
        let diagnostics = export_diagnostics(
            r#"
            module demo
            func main() returns int {
                let path = ml.onnx_export("model.onnx", "linear")
                return 0
            }
        "#,
        );
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("fixture template"));
    }

    #[test]
    fn weights_export_and_other_calls_stay_silent() {
        assert!(export_diagnostics(
            r#"
            module demo
            func main() returns int {
                let path = ml.onnx_export_weights("model.onnx", "linear", [])
                return 0
            }
        "#,
        )
        .is_empty());
        assert!(export_diagnostics(
            r#"
            module demo
            func main() returns int {
                let path = other.onnx_export("model.onnx", "linear")
                return 0
            }
        "#,
        )
        .is_empty());
    }

    #[test]
    fn rule_metadata_uses_reserved_code_e038() {
        assert_eq!(LintRule::DeprecatedOnnxExport.code(), "deprecated-onnx-export");
        assert_eq!(
            LintRule::from_code("deprecated-onnx-export"),
            Some(LintRule::DeprecatedOnnxExport)
        );
        assert_eq!(
            LintRule::DeprecatedOnnxExport.stable_error_code(),
            Some("E038")
        );
    }
}
