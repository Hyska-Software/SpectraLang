use super::*;

impl SemanticAnalyzer {
    pub(crate) fn block_guaranteed_return(&self, block: &Block) -> bool {
        if block.statements.is_empty() {
            return false;
        }

        for (index, statement) in block.statements.iter().enumerate() {
            let is_last = index + 1 == block.statements.len();
            if self.statement_guaranteed_return(statement, is_last) {
                return true;
            }
        }

        false
    }

    fn statement_guaranteed_return(&self, statement: &Statement, is_last: bool) -> bool {
        use crate::ast::StatementKind;

        match &statement.kind {
            StatementKind::Return(_) => true,
            StatementKind::Expression(expr) if is_last => self.expression_guaranteed_return(expr),
            StatementKind::Loop(_) if is_last => true,
            StatementKind::Switch(switch_stmt) if is_last => {
                switch_stmt.default.is_some()
                    && switch_stmt
                        .cases
                        .iter()
                        .all(|case| self.block_guaranteed_return(&case.body))
                    && switch_stmt
                        .default
                        .as_ref()
                        .map(|block| self.block_guaranteed_return(block))
                        .unwrap_or(false)
            }
            StatementKind::IfLet(if_let) if is_last => {
                self.block_guaranteed_return(&if_let.then_block)
                    && if_let
                        .else_block
                        .as_ref()
                        .map(|block| self.block_guaranteed_return(block))
                        .unwrap_or(false)
            }
            _ => false,
        }
    }

    fn expression_guaranteed_return(&self, expression: &Expression) -> bool {
        use crate::ast::ExpressionKind;

        match &expression.kind {
            ExpressionKind::If {
                then_block,
                elif_blocks,
                else_block,
                ..
            } => {
                let then_returns = self.block_guaranteed_return(then_block);
                let elif_returns = elif_blocks
                    .iter()
                    .all(|(_, block)| self.block_guaranteed_return(block));
                let else_returns = else_block
                    .as_ref()
                    .map(|block| self.block_guaranteed_return(block))
                    .unwrap_or(false);

                then_returns && elif_returns && else_returns
            }
            ExpressionKind::Unless {
                then_block,
                else_block,
                ..
            } => {
                let then_returns = self.block_guaranteed_return(then_block);
                let else_returns = else_block
                    .as_ref()
                    .map(|block| self.block_guaranteed_return(block))
                    .unwrap_or(false);

                then_returns && else_returns
            }
            ExpressionKind::Match { arms, .. } => {
                !arms.is_empty()
                    && arms
                        .iter()
                        .all(|arm| self.expression_guaranteed_return(&arm.body))
            }
            ExpressionKind::Block(block) | ExpressionKind::AsyncBlock(block) => {
                self.block_guaranteed_return(block)
            }
            _ => false,
        }
    }

    pub(crate) fn validate_function_block_return(
        &mut self,
        body: &Block,
        expected: &Type,
        span: Span,
    ) {
        match expected {
            Type::Unknown => (),
            Type::Unit => {
                let block_type = self.infer_block_type(body);
                if !matches!(block_type, Type::Unit | Type::Unknown) {
                    self.error(
                        format!(
                            "Function declared with no return type but final expression has type {:?}",
                            block_type
                        ),
                        body.span,
                    );
                }
            }
            expected_type => {
                let block_type = self.infer_block_type(body);

                match block_type {
                    Type::Unknown => {}
                    Type::Unit => {
                        self.error(
                            format!(
                                "Function must return value of type {}",
                                type_name(expected_type)
                            ),
                            span,
                        );
                    }
                    actual => {
                        if !self.return_types_match(&actual, expected_type) {
                            let hint = self.conversion_hint(&actual, expected_type);
                            let mut error = SemanticError::new(
                                format!(
                                    "Function final expression has type {}, expected {}",
                                    type_name(&actual),
                                    type_name(expected_type)
                                ),
                                body.span,
                            )
                            .with_code("E004")
                            .with_context(format!(
                                "function declared to return {}",
                                type_name(expected_type)
                            ))
                            .with_expected(type_name(expected_type))
                            .with_actual(type_name(&actual))
                            .with_fix(
                                "Align the returned value with the declared return type.",
                            );
                            if let Some(hint) = hint {
                                error = error.with_hint(hint);
                            }
                            self.push_semantic_error_built(error);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod final_expression_repair_field_tests {
    use crate::{CompilationOptions, CompilationPipeline, CompilerError, SemanticError};

    fn compile(source: &str) -> Result<(), Vec<CompilerError>> {
        let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
        pipeline
            .compile(source, "final_expression_repair.spectra")
            .map(|_| ())
    }

    fn semantic_error<'a>(errors: &'a [CompilerError], code: &str) -> &'a SemanticError {
        errors
            .iter()
            .find_map(|error| match error {
                CompilerError::Semantic(semantic) if semantic.code.as_deref() == Some(code) => {
                    Some(semantic)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected coded {code}: {errors:?}"))
    }

    #[test]
    fn repair_fields_populate_for_final_expression_return_mismatch() {
        let source = r#"
            module repair_e004_final

            func compute() returns int {
                "text"
            }

            func main() { }
        "#;
        let errors = compile(source).expect_err("final expression mismatch must be rejected");
        let error = semantic_error(&errors, "E004");
        assert_eq!(error.expected.as_deref(), Some("int"));
        assert_eq!(error.actual.as_deref(), Some("string"));
        assert_eq!(
            error.fix.as_deref(),
            Some("Align the returned value with the declared return type.")
        );
    }
}
