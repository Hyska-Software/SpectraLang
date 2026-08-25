use super::*;

impl SemanticAnalyzer {
    fn json_derive_set(&mut self, attributes: &[Attribute], owner_name: &str) -> JsonDeriveSet {
        let mut derives = JsonDeriveSet {
            serialize: false,
            deserialize: false,
        };

        for attribute in attributes {
            match attribute.name.as_str() {
                "derive" => {
                    if attribute.arguments.is_empty() {
                        self.error(
                            format!(
                                "derive attribute on '{}' must list at least one trait",
                                owner_name
                            ),
                            attribute.span,
                        );
                    }
                    for argument in &attribute.arguments {
                        match argument {
                            AttributeArgument::Name(name) if name == "Serialize" => {
                                derives.serialize = true;
                            }
                            AttributeArgument::Name(name) if name == "Deserialize" => {
                                derives.deserialize = true;
                            }
                            AttributeArgument::Name(name) => {
                                self.error(
                                    format!("Unsupported derive '{}' on '{}'", name, owner_name),
                                    attribute.span,
                                );
                            }
                            AttributeArgument::KeyValue { key, .. } => {
                                self.error(
                                    format!(
                                        "derive attribute on '{}' does not accept key-value argument '{}'",
                                        owner_name, key
                                    ),
                                    attribute.span,
                                );
                            }
                        }
                    }
                }
                "json" => {
                    self.error(
                        format!(
                            "json attribute is only valid on fields or enum variants, not on '{}'",
                            owner_name
                        ),
                        attribute.span,
                    );
                }
                _ => {
                    self.error(
                        format!(
                            "Unsupported attribute '{}' on '{}'",
                            attribute.name, owner_name
                        ),
                        attribute.span,
                    );
                }
            }
        }

        derives
    }

    fn json_field_options(
        &mut self,
        attributes: &[Attribute],
        owner_name: &str,
        field_name: &str,
    ) -> JsonFieldOptions {
        let mut options = JsonFieldOptions {
            json_name: field_name.to_string(),
            optional: false,
        };

        for attribute in attributes {
            if attribute.name != "json" {
                self.error(
                    format!(
                        "Unsupported attribute '{}' on field '{}.{}'",
                        attribute.name, owner_name, field_name
                    ),
                    attribute.span,
                );
                continue;
            }

            for argument in &attribute.arguments {
                match argument {
                    AttributeArgument::Name(name) if name == "optional" => {
                        options.optional = true;
                    }
                    AttributeArgument::Name(name) => {
                        self.error(
                            format!(
                                "Unsupported json option '{}' on field '{}.{}'",
                                name, owner_name, field_name
                            ),
                            attribute.span,
                        );
                    }
                    AttributeArgument::KeyValue { key, value } if key == "rename" => {
                        if value.is_empty() {
                            self.error(
                                format!(
                                    "json rename for field '{}.{}' cannot be empty",
                                    owner_name, field_name
                                ),
                                attribute.span,
                            );
                        } else {
                            options.json_name = value.clone();
                        }
                    }
                    AttributeArgument::KeyValue { key, .. } => {
                        self.error(
                            format!(
                                "Unsupported json key '{}' on field '{}.{}'",
                                key, owner_name, field_name
                            ),
                            attribute.span,
                        );
                    }
                }
            }
        }

        options
    }

    pub(crate) fn validate_json_struct_derives(&mut self, struct_def: &crate::ast::Struct) {
        let derives = self.json_derive_set(&struct_def.attributes, &struct_def.name);
        if !derives.serialize && !derives.deserialize {
            for field in &struct_def.fields {
                if !field.attributes.is_empty() {
                    self.error(
                        format!(
                            "json field attribute on '{}.{}' requires #[derive(Serialize)] or #[derive(Deserialize)] on '{}'",
                            struct_def.name, field.name, struct_def.name
                        ),
                        field.attributes[0].span,
                    );
                }
            }
            return;
        }

        let mut json_names = HashSet::new();
        let mut derived_fields = Vec::new();
        for field in &struct_def.fields {
            let options = self.json_field_options(&field.attributes, &struct_def.name, &field.name);
            if !json_names.insert(options.json_name.clone()) {
                self.error(
                    format!(
                        "Duplicate JSON field name '{}' in derived type '{}'",
                        options.json_name, struct_def.name
                    ),
                    field.span,
                );
            }
            derived_fields.push(JsonDerivedFieldInfo {
                source_name: field.name.clone(),
                json_name: options.json_name,
                optional: options.optional,
                ty: field.ty.clone(),
            });
        }

        self.json_struct_derives.insert(
            struct_def.name.clone(),
            JsonDerivedStructInfo {
                fields: derived_fields,
            },
        );

        self.register_json_derived_methods(
            &struct_def.name,
            Type::Struct {
                name: struct_def.name.clone(),
            },
            derives,
            struct_def.span,
        );
    }

    pub(crate) fn validate_json_enum_derives(&mut self, enum_def: &crate::ast::Enum) {
        let derives = self.json_derive_set(&enum_def.attributes, &enum_def.name);
        if !derives.serialize && !derives.deserialize {
            for variant in &enum_def.variants {
                if !variant.attributes.is_empty() {
                    self.error(
                        format!(
                            "json variant attribute on '{}::{}' requires #[derive(Serialize)] or #[derive(Deserialize)] on '{}'",
                            enum_def.name, variant.name, enum_def.name
                        ),
                        variant.attributes[0].span,
                    );
                }
            }
            return;
        }

        let mut json_names = HashSet::new();
        for variant in &enum_def.variants {
            let options =
                self.json_field_options(&variant.attributes, &enum_def.name, &variant.name);
            if options.optional {
                self.error(
                    format!(
                        "json optional is valid only on struct fields, not enum variant '{}::{}'",
                        enum_def.name, variant.name
                    ),
                    variant.span,
                );
            }
            if !json_names.insert(options.json_name.clone()) {
                self.error(
                    format!(
                        "Duplicate JSON variant name '{}' in derived enum '{}'",
                        options.json_name, enum_def.name
                    ),
                    variant.span,
                );
            }
        }

        self.register_json_derived_methods(
            &enum_def.name,
            Type::Enum {
                name: enum_def.name.clone(),
            },
            derives,
            enum_def.span,
        );
    }

    fn register_json_derived_methods(
        &mut self,
        type_name: &str,
        self_type: Type,
        derives: JsonDeriveSet,
        span: Span,
    ) {
        let type_methods = self.methods.entry(type_name.to_string()).or_default();
        let definitions = self
            .method_definitions
            .entry(type_name.to_string())
            .or_default();
        let visibilities = self
            .method_visibility
            .entry(type_name.to_string())
            .or_default();

        if derives.serialize {
            type_methods.insert(
                "to_json".to_string(),
                FunctionSignature {
                    params: vec![self_type.clone()],
                    return_type: Type::String,
                    self_kind: Some(SelfParamKind::Reference { mutable: false }),
                    is_async: false,
                },
            );
            definitions.insert("to_json".to_string(), span);
            visibilities.insert("to_json".to_string(), Visibility::Public);
        }

        if derives.deserialize {
            type_methods.insert(
                "from_json".to_string(),
                FunctionSignature {
                    params: vec![Type::String],
                    return_type: self_type.clone(),
                    self_kind: None,
                    is_async: false,
                },
            );
            definitions.insert("from_json".to_string(), span);
            visibilities.insert("from_json".to_string(), Visibility::Public);

            type_methods.insert(
                "json_error_field".to_string(),
                FunctionSignature {
                    params: vec![Type::String],
                    return_type: Type::String,
                    self_kind: None,
                    is_async: false,
                },
            );
            definitions.insert("json_error_field".to_string(), span);
            visibilities.insert("json_error_field".to_string(), Visibility::Public);
        }
    }

    pub(crate) fn validate_derived_from_json_literal(
        &mut self,
        derived_type_name: &str,
        method_name: &str,
        args: &[Expression],
        span: Span,
    ) {
        if method_name != "from_json" && method_name != "json_error_field" {
            return;
        }
        let Some(schema) = self.json_struct_derives.get(derived_type_name).cloned() else {
            return;
        };
        if args.len() != 1 {
            return;
        }
        let ExpressionKind::StringLiteral(json_text) = &args[0].kind else {
            return;
        };

        let value = match serde_json::from_str::<serde_json::Value>(json_text) {
            Ok(value) => value,
            Err(error) => {
                self.push_semantic_error_coded(
                    "EJSON001",
                    format!(
                        "Invalid JSON for derived type '{}': {}",
                        derived_type_name, error
                    ),
                    args[0].span,
                    Some("JSON derive validates string literals passed to from_json/json_error_field.".to_string()),
                    Some("Fix the JSON syntax before decoding this derived type.".to_string()),
                );
                return;
            }
        };

        let Some(object) = value.as_object() else {
            self.push_semantic_error_coded(
                "EJSON002",
                format!(
                    "Invalid JSON for derived type '{}': expected object at root",
                    derived_type_name
                ),
                span,
                Some("Derived struct deserialization expects a JSON object.".to_string()),
                Some(format!(
                    "Pass an object with fields for '{}'.",
                    derived_type_name
                )),
            );
            return;
        };

        for field in &schema.fields {
            let Some(field_value) = object.get(&field.json_name) else {
                if !field.optional {
                    self.push_semantic_error_coded(
                        "EJSON003",
                        format!(
                            "Invalid JSON for derived type '{}': missing required field '{}'",
                            derived_type_name, field.json_name
                        ),
                        span,
                        Some(format!(
                            "Field '{}.{}' is required and maps to JSON field '{}'.",
                            derived_type_name, field.source_name, field.json_name
                        )),
                        Some(
                            "Add the missing field or mark it with #[json(optional)].".to_string(),
                        ),
                    );
                }
                continue;
            };

            if field.optional && field_value.is_null() {
                continue;
            }

            let expected = self.type_annotation_to_type(&Some(field.ty.clone()));
            if !self.json_value_matches_type(field_value, &expected) {
                self.push_semantic_error_coded(
                    "EJSON004",
                    format!(
                        "Invalid JSON for derived type '{}': field '{}' has wrong type",
                        derived_type_name, field.json_name
                    ),
                    span,
                    Some(format!(
                        "Field '{}.{}' maps to JSON field '{}' and expects {}.",
                        derived_type_name,
                        field.source_name,
                        field.json_name,
                        type_name(&expected)
                    )),
                    Some(
                        "Change the JSON field value to match the derived field type.".to_string(),
                    ),
                );
            }
        }
    }

    fn json_value_matches_type(&self, value: &serde_json::Value, expected: &Type) -> bool {
        match expected {
            Type::Int | Type::ExactInt { .. } => value.as_i64().is_some() || value.as_u64().is_some(),
            Type::Float | Type::ExactFloat { .. } => value.as_f64().is_some(),
            Type::Bool => value.is_boolean(),
            Type::String | Type::Char => value.is_string(),
            Type::Array { element_type, .. } => value
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .all(|item| self.json_value_matches_type(item, element_type))
                })
                .unwrap_or(false),
            Type::Struct { name } => {
                if value.is_null() {
                    return false;
                }
                if let Some(schema) = self.json_struct_derives.get(name) {
                    let Some(object) = value.as_object() else {
                        return false;
                    };
                    schema.fields.iter().all(|field| {
                        object
                            .get(&field.json_name)
                            .map(|field_value| {
                                let expected =
                                    self.type_annotation_to_type(&Some(field.ty.clone()));
                                (field.optional && field_value.is_null())
                                    || self.json_value_matches_type(field_value, &expected)
                            })
                            .unwrap_or(field.optional)
                    })
                } else {
                    value.is_object()
                }
            }
            Type::Enum { .. } => value.is_string() || value.is_object(),
            Type::Applied { name, .. } => {
                if value.is_null() {
                    return false;
                }
                self.json_struct_derives
                    .get(name)
                    .map(|schema| value.is_object() && !schema.fields.is_empty())
                    .unwrap_or_else(|| value.is_object() || value.is_array())
            }
            Type::Unknown => false,
            Type::TypeParameter { .. } | Type::SelfType => true,
            Type::Unit => value.is_null(),
            Type::Tuple { elements } => value
                .as_array()
                .map(|items| {
                    items.len() == elements.len()
                        && items
                            .iter()
                            .zip(elements.iter())
                            .all(|(item, expected)| self.json_value_matches_type(item, expected))
                })
                .unwrap_or(false),
            Type::Fn { .. }
            | Type::Task { .. }
            | Type::Range
            | Type::Tensor { .. }
            | Type::DynTrait { .. } => false,
        }
    }

}
