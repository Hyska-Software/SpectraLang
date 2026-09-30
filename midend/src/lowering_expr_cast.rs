use super::*;

impl ASTLowering {
    pub(crate) fn lower_expression_cast(
        &mut self,
        expr: &Expression,
        ir_func: &mut IRFunction,
    ) -> Value {
        match &expr.kind {
            ExpressionKind::Cast {
                expr: inner,
                target_type,
                mode,
            } => self.lower_cast_expression(inner, target_type, *mode, ir_func),
            _ => self.invalid_value("lower_expression_cast called with a non-cast expression"),
        }
    }
}
