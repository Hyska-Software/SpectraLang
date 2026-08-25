use super::*;

impl Parser {
    pub(crate) fn trait_method_signature_from_decl(method: &TraitMethod) -> TraitMethodSignature {
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

    pub(crate) fn parameter_signature_from_param(param: &Parameter) -> ParameterSignature {
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
}
