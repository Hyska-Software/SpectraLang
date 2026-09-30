use super::*;

impl SemanticAnalyzer {
    pub(crate) fn analyze_expression_match(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::Match { scrutinee, arms } => {
                // The context belongs to the match result. It must not leak
                // into the scrutinee, whose type determines pattern checking.
                let saved_expected = self.current_expected_type.clone();
                self.current_expected_type = None;
                self.analyze_expression(scrutinee);
                let scrutinee_type = self.infer_expression_type(scrutinee);
                self.current_expected_type = saved_expected.clone();

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
                    // Isolated UAF snapshot per arm (E034): frees inside an
                    // arm are checked linearly but never leak past the match.
                    let saved_uaf = self.uaf_snapshot();
                    self.push_scope();

                    self.validate_pattern_against_type(&arm.pattern, &scrutinee_type, expr.span);
                    // Registrar variáveis do pattern
                    self.register_pattern_bindings(&arm.pattern);
                    self.bind_pattern_types(&arm.pattern, &scrutinee_type);

                    // Guards run in the same scope as their pattern bindings,
                    // so names such as `value` in `Some(value) if value > 0`
                    // resolve to the value captured by this arm.
                    if let Some(guard) = &arm.guard {
                        let saved_guard_expected = self.current_expected_type.clone();
                        self.current_expected_type = Some(Type::Bool);
                        self.analyze_expression(guard);
                        let guard_type = self.infer_expression_type(guard);
                        self.current_expected_type = saved_guard_expected;
                        if !matches!(guard_type, Type::Bool | Type::Unknown) {
                            self.error_coded(
                                "E040",
                                format!(
                                    "Match guard must be boolean, found {}",
                                    type_name(&guard_type)
                                ),
                                guard.span,
                            );
                        }
                    }

                    // Analisar corpo do arm
                    self.analyze_expression(&arm.body);
                    arm_result_types.push(self.infer_expression_type(&arm.body));

                    // Sair do escopo
                    self.pop_scope();
                    self.uaf_restore(saved_uaf);
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
