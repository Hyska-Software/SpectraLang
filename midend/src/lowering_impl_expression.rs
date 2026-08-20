impl ASTLowering {
    fn lower_expression(&mut self, expr: &Expression, ir_func: &mut IRFunction) -> Value {
        self.builder.set_source_span(Some(self.source_span(expr.span)));
        match &expr.kind {
            ExpressionKind::NumberLiteral(_)
            | ExpressionKind::StringLiteral(_)
            | ExpressionKind::BoolLiteral(_)
            | ExpressionKind::Identifier(_) => self.lower_expression_literals(expr, ir_func),
            ExpressionKind::Binary { .. } | ExpressionKind::Unary { .. } => {
                self.lower_expression_binary(expr, ir_func)
            }
            ExpressionKind::Call { .. } => self.lower_expression_call(expr, ir_func),
            ExpressionKind::If { .. }
            | ExpressionKind::Unless { .. }
            | ExpressionKind::Grouping(_)
            | ExpressionKind::ArrayLiteral { .. }
            | ExpressionKind::IndexAccess { .. }
            | ExpressionKind::TupleLiteral { .. }
            | ExpressionKind::TupleAccess { .. } => {
                self.lower_expression_aggregates(expr, ir_func)
            }
            ExpressionKind::StructLiteral { .. } => self.lower_expression_struct(expr, ir_func),
            ExpressionKind::FieldAccess { .. } => self.lower_expression_field(expr, ir_func),
            ExpressionKind::EnumVariant { .. } => self.lower_expression_enum(expr, ir_func),
            ExpressionKind::Match { .. } => self.lower_expression_match(expr, ir_func),
            ExpressionKind::MethodCall { .. } => self.lower_expression_method(expr, ir_func),
            ExpressionKind::CharLiteral(_)
            | ExpressionKind::FString(_)
            | ExpressionKind::Try(_)
            | ExpressionKind::Await(_)
            | ExpressionKind::Range { .. }
            | ExpressionKind::Lambda { .. }
            | ExpressionKind::Block(_)
            | ExpressionKind::DifferentiableBlock(_)
            | ExpressionKind::AsyncBlock(_) => self.lower_expression_tail(expr, ir_func),
            ExpressionKind::Cast { .. } => self.lower_expression_cast(expr, ir_func),
        }
    }
}
