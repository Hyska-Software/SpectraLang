impl SemanticAnalyzer {
    fn analyze_expression_match(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::Match { scrutinee, arms } => {
                self.analyze_expression(scrutinee);
                let scrutinee_type = self.infer_expression_type(scrutinee);

                // A bare variant name is the readable spelling for a unit
                // enum pattern (`when Pending then ...`).  Normalize a local
                // analysis copy before exhaustiveness and binding checks so
                // the existing qualified-pattern machinery remains the one
                // source of semantic truth.
                let mut normalized_arms = arms.clone();
                for arm in &mut normalized_arms {
                    self.normalize_bare_enum_pattern(&mut arm.pattern, &scrutinee_type);
                }

                // Verificar exhaustiveness
                self.check_match_exhaustiveness(&scrutinee_type, &normalized_arms, expr.span);

                let mut arm_result_types = Vec::new();
                for arm in &normalized_arms {
                    // Criar novo escopo para o arm
                    self.push_scope();

                    self.validate_pattern_against_type(&arm.pattern, &scrutinee_type, expr.span);
                    // Registrar variáveis do pattern
                    self.register_pattern_bindings(&arm.pattern);
                    self.bind_pattern_types(&arm.pattern, &scrutinee_type);

                    // Analisar corpo do arm
                    self.analyze_expression(&arm.body);
                    arm_result_types.push(self.infer_expression_type(&arm.body));

                    // Sair do escopo
                    self.pop_scope();
                }

                if let Some((expected, found)) = self.branch_type_mismatch(&arm_result_types) {
                    self.error(
                        format!(
                            "Match arms must return compatible types; expected {}, found {}",
                            type_name(&expected),
                            type_name(&found)
                        ),
                        expr.span,
                    );
                }
            }
            _ => unreachable!("expression category mismatch"),
        }
    }
}
