impl Parser {
    #[allow(dead_code)]
    fn trait_method_signature_from_decl(method: &TraitMethod) -> TraitMethodSignature {
        TraitMethodSignature {
            params: method
                .params
                .iter()
                .map(Self::parameter_signature_from_param)
                .collect(),
            return_type: method
                .return_type
                .as_ref()
                .map(TypePattern::from_annotation),
            has_default_body: method.body.is_some(),
            is_async: method.is_async,
        }
    }

    #[allow(dead_code)]
    fn parameter_signature_from_param(param: &Parameter) -> ParameterSignature {
        ParameterSignature {
            is_self: param.is_self,
            is_reference: param.is_reference,
            is_mutable: param.is_mutable,
            ty: param
                .type_annotation
                .as_ref()
                .map(TypePattern::from_annotation),
        }
    }

    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    fn validate_trait_method_signature(
        &mut self,
        trait_name: &str,
        method_name: &str,
        expected: &TraitMethodSignature,
        actual_params: &[ParameterSignature],
        actual_return: Option<&TypePattern>,
        impl_type_name: &str,
        method_span: Span,
    ) {
        if expected.params.len() != actual_params.len() {
            let message = format!(
                "Method '{}' in impl for trait '{}' has {} parameter(s), but {} were expected",
                method_name,
                trait_name,
                actual_params.len(),
                expected.params.len()
            );
            self.error_at(&message, method_span);
            return;
        }

        for (index, (expected_param, actual_param)) in
            expected.params.iter().zip(actual_params.iter()).enumerate()
        {
            let position = index + 1;

            if expected_param.is_self != actual_param.is_self {
                let message = if expected_param.is_self {
                    format!(
                        "Method '{}' in impl for trait '{}' is missing self parameter at position {}",
                        method_name, trait_name, position
                    )
                } else {
                    format!(
                        "Method '{}' in impl for trait '{}' should not have self parameter at position {}",
                        method_name, trait_name, position
                    )
                };
                self.error_at(&message, method_span);
                continue;
            }

            if expected_param.is_self {
                if expected_param.is_reference != actual_param.is_reference {
                    let expected_kind = if expected_param.is_reference {
                        if expected_param.is_mutable {
                            "&mut self"
                        } else {
                            "&self"
                        }
                    } else if expected_param.is_mutable {
                        "mut self"
                    } else {
                        "self"
                    };

                    let actual_kind = if actual_param.is_reference {
                        if actual_param.is_mutable {
                            "&mut self"
                        } else {
                            "&self"
                        }
                    } else if actual_param.is_mutable {
                        "mut self"
                    } else {
                        "self"
                    };

                    let message = format!(
                        "Method '{}' in impl for trait '{}' has mismatched receiver: expected {}, found {}",
                        method_name, trait_name, expected_kind, actual_kind
                    );
                    self.error_at(&message, method_span);
                } else if expected_param.is_mutable != actual_param.is_mutable {
                    let expected_kind = if expected_param.is_reference {
                        if expected_param.is_mutable {
                            "&mut self"
                        } else {
                            "&self"
                        }
                    } else if expected_param.is_mutable {
                        "mut self"
                    } else {
                        "self"
                    };

                    let actual_kind = if actual_param.is_reference {
                        if actual_param.is_mutable {
                            "&mut self"
                        } else {
                            "&self"
                        }
                    } else if actual_param.is_mutable {
                        "mut self"
                    } else {
                        "self"
                    };

                    let message = format!(
                        "Method '{}' in impl for trait '{}' has mismatched receiver mutability: expected {}, found {}",
                        method_name, trait_name, expected_kind, actual_kind
                    );
                    self.error_at(&message, method_span);
                }

                continue;
            }

            match (&expected_param.ty, &actual_param.ty) {
                (Some(expected_ty), Some(actual_ty))
                    if Self::trait_types_compatible(expected_ty, actual_ty, impl_type_name) => {}
                (Some(expected_ty), Some(actual_ty)) => {
                    let message = format!(
                        "Method '{}' in impl for trait '{}' has mismatched type for parameter {}: expected {}, found {}",
                        method_name,
                        trait_name,
                        position,
                        expected_ty,
                        actual_ty
                    );
                    self.error_at(&message, method_span);
                }
                (Some(expected_ty), None) => {
                    let message = format!(
                        "Method '{}' in impl for trait '{}' is missing type annotation for parameter {} (expected {})",
                        method_name,
                        trait_name,
                        position,
                        expected_ty
                    );
                    self.error_at(&message, method_span);
                }
                (None, Some(actual_ty)) => {
                    let message = format!(
                        "Method '{}' in impl for trait '{}' should not specify a type for parameter {} (found {})",
                        method_name,
                        trait_name,
                        position,
                        actual_ty
                    );
                    self.error_at(&message, method_span);
                }
                (None, None) => {}
            }
        }

        match (&expected.return_type, actual_return) {
            (Some(expected_ty), Some(actual_ty))
                if Self::trait_types_compatible(expected_ty, actual_ty, impl_type_name) => {}
            (Some(expected_ty), Some(actual_ty)) => {
                let message = format!(
                    "Method '{}' in impl for trait '{}' has mismatched return type: expected {}, found {}",
                    method_name,
                    trait_name,
                    expected_ty,
                    actual_ty
                );
                self.error_at(&message, method_span);
            }
            (Some(expected_ty), None) => {
                let message = format!(
                    "Method '{}' in impl for trait '{}' is missing return type (expected {})",
                    method_name,
                    trait_name,
                    expected_ty
                );
                self.error_at(&message, method_span);
            }
            (None, Some(actual_ty)) => {
                let message = format!(
                    "Method '{}' in impl for trait '{}' should not declare a return type (found {})",
                    method_name,
                    trait_name,
                    actual_ty
                );
                self.error_at(&message, method_span);
            }
            (None, None) => {}
        }
    }

}
