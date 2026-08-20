impl ASTLowering {
    /// Substitute type parameters in a type annotation
    fn substitute_type(
        &self,
        ty: &TypeAnnotation,
        type_map: &HashMap<String, TypeAnnotation>,
    ) -> TypeAnnotation {
        let substituted_kind = match &ty.kind {
            TypeAnnotationKind::Simple { segments } => {
                // Check if this is a type parameter (single segment)
                if segments.len() == 1 {
                    if let Some(concrete) = type_map.get(&segments[0]) {
                        return concrete.clone();
                    }
                }
                TypeAnnotationKind::Simple {
                    segments: segments.clone(),
                }
            }
            TypeAnnotationKind::Tuple { elements } => {
                let subst_elements = elements
                    .iter()
                    .map(|el| self.substitute_type(el, type_map))
                    .collect();
                TypeAnnotationKind::Tuple {
                    elements: subst_elements,
                }
            }
            TypeAnnotationKind::Function {
                params,
                return_type,
            } => {
                let subst_params = params
                    .iter()
                    .map(|el| self.substitute_type(el, type_map))
                    .collect();
                let subst_ret = Box::new(self.substitute_type(return_type, type_map));
                TypeAnnotationKind::Function {
                    params: subst_params,
                    return_type: subst_ret,
                }
            }
            TypeAnnotationKind::Generic { name, type_args } => {
                let subst_args = type_args
                    .iter()
                    .map(|el| self.substitute_type(el, type_map))
                    .collect();
                TypeAnnotationKind::Generic {
                    name: name.clone(),
                    type_args: subst_args,
                }
            }
            TypeAnnotationKind::DynTrait {
                trait_name,
                auto_traits,
            } => TypeAnnotationKind::DynTrait {
                trait_name: trait_name.clone(),
                auto_traits: auto_traits.clone(),
            },
        };

        TypeAnnotation {
            kind: substituted_kind,
            span: ty.span,
        }
    }
}
