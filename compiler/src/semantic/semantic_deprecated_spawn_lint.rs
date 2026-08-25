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
