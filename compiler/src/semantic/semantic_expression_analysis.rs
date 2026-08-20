impl SemanticAnalyzer {
    fn analyze_expression(&mut self, expr: &Expression) {
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
        self.record_expression_type(expr);
    }
}
