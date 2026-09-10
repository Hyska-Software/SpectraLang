use super::*;

impl ASTLowering {
    pub(crate) fn lower_type_annotation(&self, type_ann: &TypeAnnotation) -> IRType {
        self.lower_type_annotation_with_map(type_ann, &self.type_substitution_map)
    }

    pub(crate) fn lower_generic_application(
        &self,
        name: &str,
        type_args: &[TypeAnnotation],
        representation: IRType,
        substitutions: &HashMap<String, IRType>,
    ) -> IRType {
        IRType::Generic {
            name: name.to_string(),
            args: type_args
                .iter()
                .map(|arg| self.lower_type_annotation_with_map(arg, substitutions))
                .collect(),
            representation: Box::new(representation),
        }
    }

    /// Return the ABI/layout carrier for a structural generic application.
    /// Generic arguments remain available to type inference while existing
    /// aggregate lowering can continue to operate on the monomorphized
    /// representation.
    pub(crate) fn ir_type_representation<'a>(&self, ty: &'a IRType) -> &'a IRType {
        match ty {
            IRType::Generic { representation, .. } => self.ir_type_representation(representation),
            other => other,
        }
    }

    pub(crate) fn ir_type_representation_static(ty: &IRType) -> &IRType {
        match ty {
            IRType::Generic { representation, .. } => {
                Self::ir_type_representation_static(representation)
            }
            other => other,
        }
    }

    pub(crate) fn ir_generic_args_static<'a>(ty: &'a IRType, name: &str) -> Option<&'a [IRType]> {
        match ty {
            IRType::Generic {
                name: actual, args, ..
            } if actual == name => Some(args.as_slice()),
            _ => None,
        }
    }

    pub(crate) fn ir_nominal_name<'a>(&self, ty: &'a IRType) -> Option<&'a str> {
        match self.ir_type_representation(ty) {
            IRType::Struct { name, .. } | IRType::Enum { name, .. } => Some(name.as_str()),
            IRType::Generic { name, .. } => Some(name.as_str()),
            _ => None,
        }
    }

    pub(crate) fn lower_type(&self, ast_type: &ASTType) -> IRType {
        match ast_type {
            ASTType::Int => IRType::Int,
            ASTType::Float => IRType::Float,
            ASTType::ExactInt { signed, width } => IRType::ExactInt {
                signed: *signed,
                width: match width {
                    spectra_compiler::ast::IntWidth::I8 => IRIntWidth::I8,
                    spectra_compiler::ast::IntWidth::I16 => IRIntWidth::I16,
                    spectra_compiler::ast::IntWidth::I32 => IRIntWidth::I32,
                    spectra_compiler::ast::IntWidth::I64 => IRIntWidth::I64,
                    spectra_compiler::ast::IntWidth::Isize => IRIntWidth::Isize,
                    spectra_compiler::ast::IntWidth::Usize => IRIntWidth::Usize,
                },
            },
            ASTType::ExactFloat { width } => IRType::ExactFloat {
                width: match width {
                    spectra_compiler::ast::FloatWidth::F32 => IRFloatWidth::F32,
                    spectra_compiler::ast::FloatWidth::F64 => IRFloatWidth::F64,
                },
            },
            ASTType::Bool => IRType::Bool,
            ASTType::String => IRType::String,
            ASTType::Char => IRType::Char,
            ASTType::Unit => IRType::Void,
            ASTType::Unknown => IRType::Unknown,
            ASTType::Array { element_type, .. } => IRType::Array {
                element_type: Box::new(self.lower_type(element_type)),
                size: 0,
            },
            ASTType::Tuple { elements } => {
                // Converter cada tipo do elemento
                let ir_elements: Vec<IRType> = elements
                    .iter()
                    .map(|elem_type| self.lower_type(elem_type))
                    .collect();
                IRType::Tuple {
                    elements: ir_elements,
                }
            }
            ASTType::Struct { name } => {
                if is_std_api_handle_type_name(name) {
                    IRType::Int
                } else if let Some(fields) = self.struct_definitions.get(name) {
                    IRType::Struct {
                        name: name.clone(),
                        fields: fields.clone(),
                    }
                } else if let Some(specialized) = self.specialized_generic_annotation(name) {
                    // Exported semantic types are intentionally represented by
                    // their stable mangled name (`Box_int`, `Pair_string_int`)
                    // because the public export table stores `Type`, not the
                    // original annotation tree.  Reconstruct the annotation
                    // and send it through the same generic resolver used by
                    // local declarations instead of degrading the signature
                    // to `Unknown`.
                    self.lower_type_annotation(&specialized)
                } else {
                    IRType::Unknown
                }
            }
            ASTType::Applied { name, args } => {
                let arg_types: Vec<IRType> = args.iter().map(|arg| self.lower_type(arg)).collect();
                let arg_annotations: Vec<TypeAnnotation> = arg_types
                    .iter()
                    .map(|arg| self.ir_type_to_annotation(arg))
                    .collect();
                let application = TypeAnnotation {
                    kind: TypeAnnotationKind::Generic {
                        name: name.clone(),
                        type_args: arg_annotations,
                    },
                    span: Span::dummy(),
                };
                let representation = self.lower_type_annotation(&application);
                IRType::Generic {
                    name: name.clone(),
                    args: arg_types,
                    representation: Box::new(representation),
                }
            }
            ASTType::Enum { name } => {
                // Enums são representados como tagged unions
                // Para simplificar, vamos representar como uma tupla ou int
                // dependendo se tem dados ou não
                if let Some(variants) = self.enum_definitions.get(name) {
                    // Se todos os variants são unit, usar int
                    let all_unit = variants.iter().all(|(_, _, data)| data.is_none());
                    if all_unit {
                        IRType::Int
                    } else {
                        let simplified = variants
                            .iter()
                            .map(|(variant_name, _, data)| (variant_name.clone(), data.clone()))
                            .collect();
                        IRType::Enum {
                            name: name.clone(),
                            variants: simplified,
                        }
                    }
                } else if let Some(specialized) = self.specialized_generic_annotation(name) {
                    self.lower_type_annotation(&specialized)
                } else {
                    IRType::Unknown
                }
            }
            ASTType::TypeParameter { name: _ } | ASTType::SelfType => IRType::Unknown,
            ASTType::Fn {
                params,
                return_type,
            } => {
                let ir_params = params.iter().map(|t| self.lower_type(t)).collect();
                let ir_return = Box::new(self.lower_type(return_type));
                IRType::Function {
                    params: ir_params,
                    return_type: ir_return,
                }
            }
            ASTType::Task { output } => IRType::Task {
                output: Box::new(self.lower_type(output)),
            },
            ASTType::Range => IRType::Range,
            ASTType::Tensor {
                dtype,
                rank,
                dims,
                layout,
                device,
            } => IRType::Tensor {
                dtype: Box::new(self.lower_type(dtype)),
                rank: *rank,
                dims: dims.clone(),
                layout: layout.clone(),
                device: device.clone(),
            },
            ASTType::DynTrait {
                trait_name,
                auto_traits,
            } => IRType::DynTrait {
                trait_name: trait_name.clone(),
                auto_traits: auto_traits.clone(),
            },
        }
    }

    /// Convert TypeAnnotation to string for name mangling
    pub(crate) fn type_annotation_to_string(&self, ty: &TypeAnnotation) -> String {
        match &ty.kind {
            TypeAnnotationKind::Simple { segments } => segments.join("::"),
            TypeAnnotationKind::Tuple { elements } => {
                let element_strs: Vec<String> = elements
                    .iter()
                    .map(|el| self.type_annotation_to_string(el))
                    .collect();
                format!("tuple_{}", element_strs.join("_"))
            }
            TypeAnnotationKind::Function { .. } => "function".to_string(),
            TypeAnnotationKind::Generic { name, type_args } => {
                let arg_strs: Vec<String> = type_args
                    .iter()
                    .map(|a| self.type_annotation_to_string(a))
                    .collect();
                format!("{}_{}", name, arg_strs.join("_"))
            }
            TypeAnnotationKind::DynTrait {
                trait_name,
                auto_traits,
            } => format!(
                "dyn_{}{}",
                trait_name,
                auto_traits
                    .iter()
                    .map(|bound| format!("_{}", bound))
                    .collect::<String>()
            ),
        }
    }

    /// Specialize a generic struct with concrete type arguments
    pub(crate) fn specialize_struct(
        &mut self,
        generic: &ASTStruct,
        type_args: &[TypeAnnotation],
        mangled_name: &str,
    ) {
        // Create type substitution map: T -> int, U -> float, etc.
        let mut type_map: HashMap<String, TypeAnnotation> = HashMap::new();

        if generic.type_params.len() != type_args.len() {
            self.error(format!(
                "Type argument count mismatch for struct '{}': expected {}, got {}",
                generic.name,
                generic.type_params.len(),
                type_args.len()
            ));
            return;
        }

        for (param, arg) in generic.type_params.iter().zip(type_args.iter()) {
            type_map.insert(param.name.clone(), arg.clone());
        }

        // Substitute types in fields
        let specialized_fields: Vec<(String, IRType)> = generic
            .fields
            .iter()
            .map(|field| {
                let substituted_type = self.substitute_type(&field.ty, &type_map);
                let ir_type = self.lower_type_annotation(&substituted_type);
                (field.name.clone(), ir_type)
            })
            .collect();

        // Store specialized struct definition
        self.struct_definitions
            .insert(mangled_name.to_string(), specialized_fields);

        // specialized struct
    }

    /// Specialize a generic enum with concrete type arguments
    pub(crate) fn specialize_enum(
        &mut self,
        generic: &ASTEnum,
        type_args: &[TypeAnnotation],
        mangled_name: &str,
    ) {
        // Create type substitution map: T -> int, U -> float, etc.
        let mut type_map: HashMap<String, TypeAnnotation> = HashMap::new();

        if generic.type_params.len() != type_args.len() {
            self.error(format!(
                "Type argument count mismatch for enum '{}': expected {}, got {}",
                generic.name,
                generic.type_params.len(),
                type_args.len()
            ));
            return;
        }

        for (param, arg) in generic.type_params.iter().zip(type_args.iter()) {
            type_map.insert(param.name.clone(), arg.clone());
        }

        // Substitute types in variants
        let mut field_names = HashMap::new();
        let specialized_variants: Vec<(String, usize, Option<Vec<IRType>>)> = generic
            .variants
            .iter()
            .enumerate()
            .map(|(tag, variant)| {
                let variant_name = variant.name.clone();

                // Substitute types in variant data if present
                let variant_types = if let Some(ref data_types) = variant.data {
                    let substituted: Vec<IRType> = data_types
                        .iter()
                        .map(|ty| {
                            let substituted_type = self.substitute_type(ty, &type_map);
                            self.lower_type_annotation(&substituted_type)
                        })
                        .collect();
                    Some(substituted)
                } else if let Some(ref fields) = variant.struct_data {
                    field_names.insert(
                        variant_name.clone(),
                        fields.iter().map(|(name, _)| name.clone()).collect(),
                    );
                    let substituted: Vec<IRType> = fields
                        .iter()
                        .map(|(_, ty)| {
                            let substituted_type = self.substitute_type(ty, &type_map);
                            self.lower_type_annotation(&substituted_type)
                        })
                        .collect();
                    Some(substituted)
                } else {
                    None
                };

                (variant_name, tag, variant_types)
            })
            .collect();

        // Store specialized enum definition
        self.enum_definitions
            .insert(mangled_name.to_string(), specialized_variants);
        if !field_names.is_empty() {
            self.enum_variant_field_names
                .insert(mangled_name.to_string(), field_names);
        }

        // specialized enum
    }
}
