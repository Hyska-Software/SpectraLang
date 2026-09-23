use super::*;

impl SemanticAnalyzer {
    pub(crate) fn analyze_expression(&mut self, expr: &Expression) {
        if self.enter_analysis_depth(expr.span).is_err() {
            return;
        }
        self.analyze_expression_dispatch(expr);
        self.exit_analysis_depth();
        // Type recording runs outside this frame's depth accounting; the
        // inference walk carries its own guard.
        self.record_expression_type(expr);
    }

    fn analyze_expression_dispatch(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::Identifier(_)
            | ExpressionKind::NumberLiteral(_)
            | ExpressionKind::StringLiteral(_)
            | ExpressionKind::BoolLiteral(_) => self.analyze_expression_literals(expr),
            ExpressionKind::Binary { .. } | ExpressionKind::Unary { .. } => {
                self.analyze_expression_binary(expr)
            }
            ExpressionKind::Call { .. } => self.analyze_expression_call(expr),
            ExpressionKind::If { .. }
            | ExpressionKind::Unless { .. }
            | ExpressionKind::Grouping(_)
            | ExpressionKind::ArrayLiteral { .. }
            | ExpressionKind::IndexAccess { .. }
            | ExpressionKind::TupleLiteral { .. }
            | ExpressionKind::TupleAccess { .. } => self.analyze_expression_aggregates(expr),
            ExpressionKind::StructLiteral { .. } => self.analyze_expression_struct(expr),
            ExpressionKind::FieldAccess { .. } => self.analyze_expression_field(expr),
            ExpressionKind::EnumVariant { .. } => self.analyze_expression_enum(expr),
            ExpressionKind::Match { .. } => self.analyze_expression_match(expr),
            ExpressionKind::MethodCall { .. } => self.analyze_expression_method(expr),
            ExpressionKind::CharLiteral(_)
            | ExpressionKind::FString(_)
            | ExpressionKind::Lambda { .. }
            | ExpressionKind::Try(_)
            | ExpressionKind::Await(_)
            | ExpressionKind::Cast { .. }
            | ExpressionKind::Range { .. }
            | ExpressionKind::Block(_)
            | ExpressionKind::DifferentiableBlock(_)
            | ExpressionKind::AsyncBlock(_) => self.analyze_expression_tail(expr),
        }
    }
}

#[cfg(test)]
mod depth_guard_tests {
    use super::*;
    use crate::ast::{Expression, ExpressionKind};
    use crate::span::{Location, Span};

    fn deep_grouping_expression(depth: usize) -> Expression {
        let mut expression = Expression {
            span: Span::new(0, 1, Location::new(1, 1), Location::new(1, 2)),
            kind: ExpressionKind::NumberLiteral("1".to_string()),
        };
        for _ in 0..depth {
            expression = Expression {
                span: expression.span,
                kind: ExpressionKind::Grouping(Box::new(expression)),
            };
        }
        expression
    }

    #[test]
    fn deeply_nested_ast_reports_coded_guard_instead_of_overflowing() {
        let mut analyzer = SemanticAnalyzer::new();
        let expression = deep_grouping_expression(50_000);
        analyzer.analyze_expression(&expression);

        let guard_errors: Vec<_> = analyzer
            .errors
            .iter()
            .filter(|error| error.code.as_deref() == Some("P013"))
            .collect();
        assert_eq!(
            guard_errors.len(),
            1,
            "the nesting guard must fire exactly once: {:?}",
            analyzer.errors
        );
        assert!(
            analyzer.errors.len() <= 2,
            "only the guard (plus at most one tolerated sibling) may be reported: {:?}",
            analyzer.errors
        );
        // The 50k-deep Box chain recurses just as deeply in `Drop`, which is
        // unrelated to the walk under test; leaking it isolates the guard.
        std::mem::forget(expression);
    }
}
