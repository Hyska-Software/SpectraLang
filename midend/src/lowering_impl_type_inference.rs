use super::*;

impl ASTLowering {
    pub(crate) fn specialized_generic_annotation(&self, type_name: &str) -> Option<TypeAnnotation> {
        self.generic_enums
            .iter()
            .find_map(|(base_name, generic_enum)| {
                let prefix = format!("{}_", base_name);
                if !type_name.starts_with(&prefix) {
                    return None;
                }

                let suffix = &type_name[prefix.len()..];
                let parts: Vec<&str> = if generic_enum.type_params.len() == 1 {
                    vec![suffix]
                } else {
                    suffix.split('_').collect()
                };
                if parts.len() != generic_enum.type_params.len() {
                    return None;
                }

                let type_args = parts
                    .into_iter()
                    .map(Self::simple_type_annotation)
                    .collect();

                Some(TypeAnnotation {
                    kind: TypeAnnotationKind::Generic {
                        name: base_name.clone(),
                        type_args,
                    },
                    span: Span::dummy(),
                })
            })
            .or_else(|| {
                self.generic_structs
                    .iter()
                    .find_map(|(base_name, generic_struct)| {
                        let prefix = format!("{}_", base_name);
                        if !type_name.starts_with(&prefix) {
                            return None;
                        }

                        let suffix = &type_name[prefix.len()..];
                        let parts: Vec<&str> = if generic_struct.type_params.len() == 1 {
                            vec![suffix]
                        } else {
                            suffix.split('_').collect()
                        };
                        if parts.len() != generic_struct.type_params.len() {
                            return None;
                        }

                        let type_args = parts
                            .into_iter()
                            .map(Self::simple_type_annotation)
                            .collect();

                        Some(TypeAnnotation {
                            kind: TypeAnnotationKind::Generic {
                                name: base_name.clone(),
                                type_args,
                            },
                            span: Span::dummy(),
                        })
                    })
            })
    }

    pub(crate) fn specialized_generic_annotation_from_enum(
        &self,
        type_name: &str,
        variants: &[(String, Option<Vec<IRType>>)],
    ) -> Option<TypeAnnotation> {
        for (base_name, generic_enum) in &self.generic_enums {
            if generic_enum.variants.len() != variants.len() {
                continue;
            }

            let mut param_positions: HashMap<String, usize> = HashMap::new();
            for (idx, param) in generic_enum.type_params.iter().enumerate() {
                param_positions.insert(param.name.clone(), idx);
            }

            let mut inferred =
                vec![Self::unknown_type_annotation(); generic_enum.type_params.len()];
            let mut matches_shape = true;

            for (template_variant, actual_variant) in
                generic_enum.variants.iter().zip(variants.iter())
            {
                let (actual_name, actual_data) = actual_variant;
                if template_variant.name != *actual_name {
                    matches_shape = false;
                    break;
                }

                match (&template_variant.data, actual_data) {
                    (Some(template_types), Some(actual_types)) => {
                        if template_types.len() != actual_types.len() {
                            matches_shape = false;
                            break;
                        }

                        for (template, actual_type) in
                            template_types.iter().zip(actual_types.iter())
                        {
                            self.fill_type_args_from_annotation(
                                template,
                                actual_type,
                                &param_positions,
                                &mut inferred,
                            );
                        }
                    }
                    (None, None) => {}
                    _ => {
                        matches_shape = false;
                        break;
                    }
                }
            }

            if !matches_shape || inferred.iter().any(Self::is_unknown_annotation) {
                continue;
            }

            return Some(TypeAnnotation {
                kind: TypeAnnotationKind::Generic {
                    name: base_name.clone(),
                    type_args: inferred,
                },
                span: Span::dummy(),
            });
        }

        self.specialized_generic_annotation(type_name)
    }

    pub(crate) fn default_type_args_for_enum(&self, enum_name: &str) -> Option<Vec<TypeAnnotation>> {
        self.generic_enums.get(enum_name).map(|generic_enum| {
            generic_enum
                .type_params
                .iter()
                .map(|_| Self::unknown_type_annotation())
                .collect()
        })
    }

    pub(crate) fn fill_type_args_from_annotation(
        &self,
        template: &TypeAnnotation,
        actual_type: &IRType,
        param_positions: &HashMap<String, usize>,
        inferred: &mut [TypeAnnotation],
    ) {
        match &template.kind {
            TypeAnnotationKind::Simple { segments } if segments.len() == 1 => {
                if let Some(&index) = param_positions.get(&segments[0]) {
                    if Self::is_unknown_annotation(&inferred[index]) {
                        inferred[index] = self.ir_type_to_annotation(actual_type);
                    }
                }
            }
            TypeAnnotationKind::Tuple { elements } => {
                if let IRType::Tuple {
                    elements: actual_elements,
                } = actual_type
                {
                    for (sub_template, sub_type) in elements.iter().zip(actual_elements.iter()) {
                        self.fill_type_args_from_annotation(
                            sub_template,
                            sub_type,
                            param_positions,
                            inferred,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn type_annotation_needs_refinement(&self, ann: &TypeAnnotation) -> bool {
        match &ann.kind {
            TypeAnnotationKind::Simple { segments } if segments.len() == 1 => {
                let name = &segments[0];
                if name == "unknown" {
                    true
                } else if self.enum_definitions.contains_key(name)
                    || self.struct_definitions.contains_key(name)
                {
                    false
                } else { self.generic_enums.contains_key(name) }
            }
            _ => false,
        }
    }

    pub(crate) fn infer_enum_type_args_from_data(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        data_exprs: &[Expression],
    ) -> Option<Vec<TypeAnnotation>> {
        let (param_names, field_templates) = {
            let generic_enum = self.generic_enums.get(enum_name)?;
            let variant = generic_enum
                .variants
                .iter()
                .find(|v| v.name == variant_name)?;
            let data = variant.data.as_ref()?;
            let params = generic_enum
                .type_params
                .iter()
                .map(|param| param.name.clone())
                .collect::<Vec<_>>();
            (params, data.clone())
        };

        if field_templates.len() != data_exprs.len() {
            return None;
        }

        let mut param_positions: HashMap<String, usize> = HashMap::new();
        for (idx, param_name) in param_names.iter().enumerate() {
            param_positions.insert(param_name.clone(), idx);
        }

        let mut inferred = vec![Self::unknown_type_annotation(); param_names.len()];

        for (template, expr) in field_templates.iter().zip(data_exprs.iter()) {
            if let TypeAnnotationKind::Simple { segments } = &template.kind {
                if segments.len() == 1 {
                    if let Some(&index) = param_positions.get(&segments[0]) {
                        if Self::is_unknown_annotation(&inferred[index]) {
                            if let Some(annotation) = self.infer_expr_type_annotation(expr) {
                                inferred[index] = annotation;
                                continue;
                            }
                        }
                    }
                }
            }

            let actual_type = self.infer_expr_ir_type(expr);
            self.fill_type_args_from_annotation(
                template,
                &actual_type,
                &param_positions,
                &mut inferred,
            );
        }

        Some(inferred)
    }

    pub(crate) fn infer_expr_type_annotation(&mut self, expr: &Expression) -> Option<TypeAnnotation> {
        match &expr.kind {
            ExpressionKind::NumberLiteral(num) => {
                Some(Self::simple_type_annotation(if
                    spectra_compiler::numeric::number_literal_is_float(num)
                {
                    "float"
                } else {
                    "int"
                }))
            }
            ExpressionKind::StringLiteral(_) => Some(Self::simple_type_annotation("string")),
            ExpressionKind::BoolLiteral(_) => Some(Self::simple_type_annotation("bool")),
            ExpressionKind::StructLiteral {
                name, type_args, ..
            } => {
                if type_args.is_empty() {
                    Some(Self::simple_type_annotation(name))
                } else {
                    Some(TypeAnnotation {
                        kind: TypeAnnotationKind::Generic {
                            name: name.clone(),
                            type_args: type_args.clone(),
                        },
                        span: Span::dummy(),
                    })
                }
            }
            ExpressionKind::EnumVariant {
                enum_name,
                type_args,
                variant_name,
                data,
                struct_data,
                ..
            } => {
                let needs_refinement = type_args.is_empty()
                    || type_args
                        .iter()
                        .any(|ann| self.type_annotation_needs_refinement(ann));

                let mut final_args = if needs_refinement {
                    if let Some(data_exprs) = data {
                        self.infer_enum_type_args_from_data(enum_name, variant_name, data_exprs)
                            .or_else(|| self.default_type_args_for_enum(enum_name))
                            .unwrap_or_default()
                    } else if let Some(named_fields) = struct_data {
                        self.infer_enum_type_args_from_named_fields(
                            enum_name,
                            variant_name,
                            named_fields,
                        )
                        .or_else(|| self.default_type_args_for_enum(enum_name))
                        .unwrap_or_default()
                    } else {
                        self.default_type_args_for_enum(enum_name)
                            .unwrap_or_default()
                    }
                } else {
                    type_args.clone()
                };

                Self::fill_builtin_enum_defaults(enum_name, &mut final_args);

                if final_args.is_empty() {
                    Some(Self::simple_type_annotation(enum_name))
                } else {
                    Some(TypeAnnotation {
                        kind: TypeAnnotationKind::Generic {
                            name: enum_name.clone(),
                            type_args: final_args,
                        },
                        span: Span::dummy(),
                    })
                }
            }
            _ => None,
        }
    }

    pub(crate) fn infer_enum_type_args_from_named_fields(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        fields: &[(String, Expression)],
    ) -> Option<Vec<TypeAnnotation>> {
        let generic_enum = self.generic_enums.get(enum_name)?;
        let variant = generic_enum
            .variants
            .iter()
            .find(|v| v.name == variant_name)?;
        let field_templates = variant.struct_data.as_ref()?.clone();

        let ordered_exprs: Vec<&Expression> = field_templates
            .iter()
            .map(|(field_name, _)| {
                fields
                    .iter()
                    .find(|(name, _)| name == field_name)
                    .map(|(_, expr)| expr)
            })
            .collect::<Option<Vec<_>>>()?;

        let param_names = generic_enum
            .type_params
            .iter()
            .map(|param| param.name.clone())
            .collect::<Vec<_>>();

        let mut param_positions: HashMap<String, usize> = HashMap::new();
        for (idx, param_name) in param_names.iter().enumerate() {
            param_positions.insert(param_name.clone(), idx);
        }

        let mut inferred = vec![Self::unknown_type_annotation(); param_names.len()];

        for ((_, template), expr) in field_templates.iter().zip(ordered_exprs.iter()) {
            let actual_type = self.infer_expr_ir_type(expr);
            self.fill_type_args_from_annotation(
                template,
                &actual_type,
                &param_positions,
                &mut inferred,
            );
        }

        Some(inferred)
    }

    pub(crate) fn reorder_named_variant_exprs<'a>(
        &self,
        enum_name: &str,
        variant_name: &str,
        fields: &'a [(String, Expression)],
    ) -> Option<Vec<&'a Expression>> {
        let order = self
            .enum_variant_field_names
            .get(enum_name)?
            .get(variant_name)?;

        order
            .iter()
            .map(|field_name| {
                fields
                    .iter()
                    .find(|(name, _)| name == field_name)
                    .map(|(_, expr)| expr)
            })
            .collect()
    }

    pub(crate) fn reorder_named_variant_patterns<'a>(
        &self,
        enum_name: &str,
        variant_name: &str,
        fields: &'a [(String, spectra_compiler::ast::Pattern)],
    ) -> Option<Vec<&'a spectra_compiler::ast::Pattern>> {
        let order = self
            .enum_variant_field_names
            .get(enum_name)?
            .get(variant_name)?;

        order
            .iter()
            .map(|field_name| {
                fields
                    .iter()
                    .find(|(name, _)| name == field_name)
                    .map(|(_, pattern)| pattern)
            })
            .collect()
    }

    pub(crate) fn enum_variants_from_ir_type(
        &self,
        scrutinee_type: Option<&IRType>,
    ) -> Option<Vec<EnumVariantDefinition>> {
        if let Some(IRType::Enum { variants, .. }) = scrutinee_type
            .map(Self::ir_type_representation_static)
        {
            return Some(
                variants
                    .iter()
                    .enumerate()
                    .map(|(tag, (name, data))| (name.clone(), tag, data.clone()))
                    .collect(),
            );
        }
        None
    }

    pub(crate) fn merge_types(&self, left: &IRType, right: &IRType) -> Option<IRType> {
        if left == right {
            return Some(left.clone());
        }

        match (left, right) {
            (IRType::Int, IRType::Float) | (IRType::Float, IRType::Int) => Some(IRType::Float),
            (IRType::Void, other) => Some(other.clone()),
            (other, IRType::Void) => Some(other.clone()),
            _ => None,
        }
    }

    pub(crate) fn unify_types(&self, mut types: Vec<IRType>) -> IRType {
        if types.is_empty() {
            return IRType::Void;
        }

        let mut result = types.remove(0);
        for ty in types {
            if let Some(merged) = self.merge_types(&result, &ty) {
                result = merged;
            }
        }

        result
    }

}
