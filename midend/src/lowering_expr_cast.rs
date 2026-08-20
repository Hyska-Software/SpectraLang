impl ASTLowering {
    fn lower_expression_cast(&mut self, expr: &Expression, ir_func: &mut IRFunction) -> Value {
        match &expr.kind {
            ExpressionKind::Cast {
                expr: inner,
                target_type,
                ..
            } => self.lower_cast_expression(inner, target_type, ir_func),
            _ => unreachable!("lowering expression category mismatch"),
        }
    }
}
