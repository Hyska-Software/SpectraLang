use super::*;

// ============================================================================
// narrowing-cast lint — stable code E035 when escalated to error via --deny
// (CastLint). This file is NOT a Rust module: it is textually included by
// `compiler/src/lint/mod.rs` so the rule can hook into the private LintRunner.
// Owner: CastLint. Do not edit without coordinating.
// ============================================================================

use crate::ast::{CastMode, TypeAnnotation, TypeAnnotationKind};

/// Exact-width numeric classification resolved syntactically from type
/// annotations (`i8`..`i64`, `u8`..`u64`, `isize`/`usize`, `f32`, `f64`).
/// The lint pass has no full type inference, so only explicitly annotated
/// widths participate in narrowing detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExactNum {
    Int { signed: bool, bits: u32 },
    Float { bits: u32 },
}

impl ExactNum {
    /// Resolve an exact numeric width from a type annotation, if it names one.
    pub(crate) fn from_annotation(annotation: Option<&TypeAnnotation>) -> Option<Self> {
        let annotation = annotation?;
        let TypeAnnotationKind::Simple { segments } = &annotation.kind else {
            return None;
        };
        if segments.len() != 1 {
            return None;
        }

        Some(match segments[0].as_str() {
            "i8" => Self::Int {
                signed: true,
                bits: 8,
            },
            "i16" => Self::Int {
                signed: true,
                bits: 16,
            },
            "i32" => Self::Int {
                signed: true,
                bits: 32,
            },
            "i64" | "isize" => Self::Int {
                signed: true,
                bits: 64,
            },
            "u8" => Self::Int {
                signed: false,
                bits: 8,
            },
            "u16" => Self::Int {
                signed: false,
                bits: 16,
            },
            "u32" => Self::Int {
                signed: false,
                bits: 32,
            },
            "u64" | "usize" => Self::Int {
                signed: false,
                bits: 64,
            },
            "f32" => Self::Float { bits: 32 },
            "f64" => Self::Float { bits: 64 },
            _ => return None,
        })
    }

    pub(crate) fn name(&self) -> String {
        match *self {
            Self::Int { signed, bits } => format!("{}{}", if signed { "i" } else { "u" }, bits),
            Self::Float { bits } => format!("f{}", bits),
        }
    }
}

impl<'a> crate::lint::LintRunner<'a> {
    /// BEGIN narrowing-cast rule core (CastLint)
    ///
    /// Warns on casts that silently narrow between exact widths:
    /// - int → int where the destination is strictly narrower than the source;
    /// - float → float where the destination is strictly narrower;
    /// - any float → any int (fractional truncation and range loss).
    ///   An explicit `as wrapping` cast records intent and suppresses the warning.
    pub(crate) fn check_narrowing_cast(
        &mut self,
        expression: &Expression,
        inner: &Expression,
        target_type: &TypeAnnotation,
        mode: CastMode,
    ) {
        if !self
            .options
            .is_enabled(crate::lint::LintRule::NarrowingCast)
        {
            return;
        }
        // Explicit wrapping casts are intentional truncations.
        if mode == CastMode::Wrapping {
            return;
        }

        let to = match ExactNum::from_annotation(Some(target_type)) {
            Some(to) => to,
            None => return,
        };
        let from = match self.exact_num_of_expression(inner) {
            Some(from) => from,
            None => return,
        };

        let narrowing = match (from, to) {
            // Any float → int loses the fractional part and may overflow.
            (ExactNum::Float { .. }, ExactNum::Int { .. }) => true,
            (
                ExactNum::Int {
                    bits: from_bits, ..
                },
                ExactNum::Int { bits: to_bits, .. },
            ) => to_bits < from_bits,
            (ExactNum::Float { bits: from_bits }, ExactNum::Float { bits: to_bits }) => {
                to_bits < from_bits
            }
            // Int → float never narrows under this policy.
            (ExactNum::Int { .. }, ExactNum::Float { .. }) => false,
        };
        if !narrowing {
            return;
        }

        let note = format!(
            "add an explicit 'as wrapping' cast to acknowledge the truncation, or keep the value in {}",
            from.name()
        );
        self.diagnostics.push(crate::lint::LintDiagnostic {
            rule: crate::lint::LintRule::NarrowingCast,
            message: format!(
                "narrowing cast from '{}' to '{}' silently truncates the value",
                from.name(),
                to.name()
            ),
            span: expression.span,
            note: Some(note),
            secondary_span: None,
        });
    }

    /// Best-effort exact-width resolution of a cast operand at lint time:
    /// annotated bindings, parenthesized expressions, and the result of a
    /// previous cast in a chain (`x as i16 as i8`). Untyped literals and
    /// inferred expressions yield `None` and are never flagged.
    fn exact_num_of_expression(&self, expression: &Expression) -> Option<ExactNum> {
        match &expression.kind {
            ExpressionKind::Identifier(name) => self
                .scope_stack
                .iter()
                .rev()
                .find_map(|scope| scope.bindings.get(name))
                .and_then(|binding| binding.exact_num),
            ExpressionKind::Grouping(inner) => self.exact_num_of_expression(inner),
            ExpressionKind::Cast { target_type, .. } => {
                ExactNum::from_annotation(Some(target_type))
            }
            _ => None,
        }
    }
    // END narrowing-cast rule core (CastLint)
}

#[cfg(test)]
mod narrowing_cast_tests {
    use crate::lexer::Lexer;
    use crate::lint::{lint_module, LintOptions, LintRule};
    use crate::parser::Parser;

    pub(crate) fn parse(source: &str) -> crate::ast::Module {
        let tokens = Lexer::new(source)
            .tokenize()
            .expect("lexer should not fail");
        Parser::new(tokens).parse().expect("parse")
    }

    fn narrowing_diagnostics(source: &str) -> Vec<String> {
        lint_module(&parse(source), &LintOptions::default())
            .into_iter()
            .filter(|diagnostic| diagnostic.rule == LintRule::NarrowingCast)
            .map(|diagnostic| diagnostic.message)
            .collect()
    }

    #[test]
    fn warns_on_exact_int_narrowing() {
        let diagnostics = narrowing_diagnostics(
            r#"
            module demo
            func main() returns int {
                let big: i64 = 300
                let small: i8 = big as i8
                return small as int
            }
        "#,
        );
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("'i64'"), "{}", diagnostics[0]);
        assert!(diagnostics[0].contains("'i8'"), "{}", diagnostics[0]);
    }

    #[test]
    fn warns_on_float_narrowing_and_float_to_int() {
        let diagnostics = narrowing_diagnostics(
            r#"
            module demo
            func main() returns int {
                let wide: f64 = 1.5
                let thin: f32 = wide as f32
                let tiny: i8 = wide as i8
                return thin as int + tiny as int
            }
        "#,
        );
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn widening_and_same_width_casts_stay_silent() {
        assert!(narrowing_diagnostics(
            r#"
            module demo
            func main() returns int {
                let small: i8 = 7
                let big: i64 = small as i64
                let same: i8 = small as i8
                let up: f64 = same as f64
                return big as int + same as int + up as int
            }
        "#,
        )
        .is_empty());
    }

    #[test]
    fn explicit_wrapping_mode_suppresses_warning() {
        assert!(narrowing_diagnostics(
            r#"
            module demo
            func main() returns int {
                let big: i64 = 300
                let small: i8 = big as wrapping i8
                return small as int
            }
        "#,
        )
        .is_empty());
    }

    #[test]
    fn untyped_operands_are_never_flagged() {
        assert!(narrowing_diagnostics(
            r#"
            module demo
            func main() returns int {
                let value = 300
                let narrowed: i8 = value as i8
                return narrowed as int
            }
        "#,
        )
        .is_empty());
    }

    #[test]
    fn chained_casts_narrow_from_inner_target() {
        let diagnostics = narrowing_diagnostics(
            r#"
            module demo
            func main() returns int {
                let value: i8 = 100
                let chained: i8 = ((value as i32)) as i8
                return chained as int
            }
        "#,
        );
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("'i32'"), "{}", diagnostics[0]);
    }

    #[test]
    fn rule_is_enumerated_and_resolvable_by_code() {
        assert!(LintRule::all().contains(&LintRule::NarrowingCast));
        assert_eq!(
            LintRule::from_code("narrowing-cast"),
            Some(LintRule::NarrowingCast)
        );
        assert_eq!(
            LintRule::from_code("narrowing_cast"),
            Some(LintRule::NarrowingCast)
        );
        assert_eq!(
            "narrowing-cast".parse::<LintRule>(),
            Ok(LintRule::NarrowingCast)
        );
        assert_eq!(LintRule::NarrowingCast.stable_error_code(), Some("E035"));
    }

    #[test]
    fn disabled_options_produce_no_narrowing_diagnostics() {
        let source = r#"
            module demo
            func main() returns int {
                let big: i64 = 300
                let small: i8 = big as i8
                return small as int
            }
        "#;
        let mut options = LintOptions::disabled();
        options.deny_rule(LintRule::Shadowing);
        assert!(lint_module(&parse(source), &options).is_empty());
    }

    #[test]
    fn deny_configuration_flags_the_rule_for_escalation() {
        let mut options = LintOptions::default();
        options.deny_rule(LintRule::NarrowingCast);
        assert!(options.is_denied(LintRule::NarrowingCast));
        assert!(options.is_enabled(LintRule::NarrowingCast));
    }
}
