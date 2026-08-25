use super::*;

impl SemanticAnalyzer {
    pub fn analyze_module(&mut self, module: &mut Module) -> Vec<SemanticError> {
        // Pass 0: resolve imports — inject symbols from imported modules into scope
        // so they are visible during all subsequent analysis passes.
        // Collect imports first to avoid a borrow conflict when we later write
        // into `module.std_import_aliases`.
        let imports: Vec<crate::ast::Import> = module
            .items
            .iter()
            .filter_map(|item| {
                if let Item::Import(imp) = item {
                    Some(imp.clone())
                } else {
                    None
                }
            })
            .collect();

        let mut new_aliases: Vec<(String, Vec<String>)> = Vec::new();
        let mut new_user_fn_types: Vec<(String, crate::ast::Type)> = Vec::new();
        let mut new_user_fn_signatures: Vec<(
            String,
            Vec<crate::ast::Type>,
            crate::ast::Type,
        )> = Vec::new();
        let mut new_static_globals: Vec<(String, String, crate::ast::Type)> = Vec::new();
        let mut new_enum_defs: Vec<crate::ast::Enum> = Vec::new();
        let mut new_struct_defs: Vec<crate::ast::Struct> = Vec::new();
        let mut new_trait_impls: Vec<(String, String, Vec<crate::ast::Type>)> = Vec::new();
        let mut new_generic_functions: Vec<crate::ast::Function> = Vec::new();
        let mut new_trait_decls: Vec<crate::ast::TraitDeclaration> = Vec::new();
        for import in &imports {
            let aliases = self.analyze_import(
                import,
                &mut new_user_fn_types,
                &mut new_user_fn_signatures,
                &mut new_static_globals,
                &mut new_enum_defs,
                &mut new_struct_defs,
                &mut new_trait_impls,
                &mut new_generic_functions,
                &mut new_trait_decls,
            );
            new_aliases.extend(aliases);
        }
        module.std_import_aliases.extend(new_aliases);
        module
            .imported_function_return_types
            .extend(new_user_fn_types);
        module
            .imported_function_signatures
            .extend(new_user_fn_signatures);
        module.imported_static_globals.extend(new_static_globals);
        module.imported_enum_defs.extend(new_enum_defs);
        module.imported_struct_defs.extend(new_struct_defs);
        module.imported_trait_impls.extend(new_trait_impls);
        module
            .imported_generic_functions
            .extend(new_generic_functions);
        module.imported_trait_decls.extend(new_trait_decls);

        // First pass: collect all declarations (functions, generic structs, generic enums)
        for item in &module.items {
            if let Item::Trait(trait_decl) = item {
                self.analyze_trait_declaration(trait_decl);
            }
        }

        for item in &module.items {
            match item {
                Item::Function(func) => {
                    if self.functions.contains_key(&func.name) {
                        self.error_coded(
                            "E006",
                            format!("Function '{}' is already defined", func.name),
                            func.span,
                        );
                    } else {
                        let pushed_generics = self.push_generic_params(&func.type_params);

                        // Extract parameter types
                        let params: Vec<Type> = func
                            .params
                            .iter()
                            .map(|p| self.type_annotation_to_type(&p.ty))
                            .collect();

                        // Extract return type
                        let return_type = Self::async_task_type(
                            func.is_async,
                            self.type_annotation_to_type(&func.return_type),
                        );

                        if pushed_generics {
                            self.pop_generic_params();
                        }

                        let signature = FunctionSignature {
                            params,
                            return_type: return_type.clone(),
                            self_kind: None,
                            is_async: func.is_async,
                        };

                        self.functions.insert(func.name.clone(), signature.clone());
                        self.function_type_params
                            .insert(func.name.clone(), func.type_params.clone());
                        // Store the full function type so that passing the
                        // function as a first-class value gives the correct
                        // `Fn(params) -> return_type` type instead of just the
                        // bare return type.
                        let fn_type = Type::Fn {
                            params: signature.params.clone(),
                            return_type: Box::new(signature.return_type.clone()),
                        };
                        self.declare_symbol(func.name.clone(), func.span, fn_type);
                    }
                }
                Item::Struct(struct_def) => {
                    // Build struct metadata and validate duplicate fields
                    let mut fields_map = HashMap::new();
                    for field in &struct_def.fields {
                        if fields_map.contains_key(&field.name) {
                            self.error_coded(
                                "E030",
                                format!(
                                    "Field '{}' is duplicated in struct '{}'",
                                    field.name, struct_def.name
                                ),
                                field.span,
                            );
                            continue;
                        }

                        fields_map.insert(
                            field.name.clone(),
                            StructFieldInfo {
                                ty: field.ty.clone(),
                                span: field.span,
                                visibility: field.visibility,
                            },
                        );
                    }

                    let struct_info = StructInfo {
                        visibility: struct_def.visibility,
                        type_params: struct_def
                            .type_params
                            .iter()
                            .map(|tp| tp.name.clone())
                            .collect(),
                        fields: fields_map,
                    };

                    if self
                        .struct_infos
                        .insert(struct_def.name.clone(), struct_info)
                        .is_some()
                    {
                        self.error_coded(
                            "E030",
                            format!("Struct '{}' is already defined", struct_def.name),
                            struct_def.span,
                        );
                    }

                    // Collect generic structs for type inference
                    if !struct_def.type_params.is_empty() {
                        let type_params = struct_def.type_params.clone();
                        let fields: Vec<(String, crate::ast::TypeAnnotation)> = struct_def
                            .fields
                            .iter()
                            .map(|f| (f.name.clone(), f.ty.clone()))
                            .collect();
                        self.generic_structs
                            .insert(struct_def.name.clone(), (type_params, fields));
                    }

                    self.validate_json_struct_derives(struct_def);
                }
                Item::Enum(enum_def) => {
                    let shadows_builtin_generic =
                        matches!(enum_def.name.as_str(), "Option" | "Result");

                    let variant_type_params: Vec<String> = enum_def
                        .type_params
                        .iter()
                        .map(|tp| tp.name.clone())
                        .collect();

                    let mut variants_map = HashMap::new();
                    let mut variant_names = Vec::new();

                    for variant in &enum_def.variants {
                        if variants_map.contains_key(&variant.name) {
                            self.error_coded(
                                "E030",
                                format!(
                                    "Variant '{}' is duplicated in enum '{}'",
                                    variant.name, enum_def.name
                                ),
                                variant.span,
                            );
                            continue;
                        }

                        variants_map.insert(
                            variant.name.clone(),
                            EnumVariantInfo {
                                data: variant.data.clone(),
                                struct_data: variant.struct_data.clone(),
                                span: variant.span,
                            },
                        );
                        variant_names.push(variant.name.clone());
                    }

                    let enum_info = EnumInfo {
                        visibility: enum_def.visibility,
                        type_params: variant_type_params.clone(),
                        variants: variants_map,
                    };

                    if self
                        .enum_infos
                        .insert(enum_def.name.clone(), enum_info)
                        .is_some()
                        && !shadows_builtin_generic
                    {
                        self.error_coded(
                            "E030",
                            format!("Enum '{}' is already defined", enum_def.name),
                            enum_def.span,
                        );
                    }

                    // Store variant names for exhaustiveness checking
                    self.enum_definitions
                        .insert(enum_def.name.clone(), variant_names);

                    // Collect generic enums for type inference
                    if !enum_def.type_params.is_empty() {
                        let type_params = enum_def.type_params.clone();
                        let variant_names: Vec<String> =
                            enum_def.variants.iter().map(|v| v.name.clone()).collect();
                        self.generic_enums
                            .insert(enum_def.name.clone(), (type_params, variant_names));
                    } else if shadows_builtin_generic {
                        self.generic_enums.remove(&enum_def.name);
                    }

                    self.validate_json_enum_derives(enum_def);
                }
                _ => {}
            }
        }

        // Register aliases after structs/enums are known, but before function
        // signatures and bodies are analysed.  Keeping every alias in the map
        // first also makes declaration order irrelevant for alias chains.
        for item in &module.items {
            if let Item::TypeAlias(alias) = item {
                if self.type_aliases.contains_key(&alias.name)
                    || self.struct_infos.contains_key(&alias.name)
                    || self.enum_infos.contains_key(&alias.name)
                {
                    self.error_coded(
                        "E006",
                        format!("Type '{}' is already defined", alias.name),
                        alias.span,
                    );
                } else {
                    self.type_aliases
                        .insert(alias.name.clone(), alias.ty.clone());
                }
            }
        }

        // The declaration pass runs before aliases are registered so that
        // names are available independent of item order.  Refresh function
        // signatures now that aliases and aggregate definitions are known;
        // otherwise a return type such as `Pair = (int, string)` would remain
        // `Unknown` and only fail later when a caller binds the result.
        for item in &module.items {
            let Item::Function(func) = item else {
                continue;
            };
            let pushed_generics = self.push_generic_params(&func.type_params);
            let params: Vec<Type> = func
                .params
                .iter()
                .map(|param| self.type_annotation_to_type(&param.ty))
                .collect();
            let return_type = Self::async_task_type(
                func.is_async,
                self.type_annotation_to_type(&func.return_type),
            );
            if pushed_generics {
                self.pop_generic_params();
            }
            let signature = FunctionSignature {
                params,
                return_type: return_type.clone(),
                self_kind: None,
                is_async: func.is_async,
            };
            self.functions.insert(func.name.clone(), signature.clone());
            let fn_type = Type::Fn {
                params: signature.params,
                return_type: Box::new(signature.return_type),
            };
            if let Some(info) = self
                .symbols
                .first_mut()
                .and_then(|scope| scope.get_mut(&func.name))
            {
                info.ty = fn_type.clone();
                self.symbol_resolutions.insert(func.span, info.clone());
            }
        }

        // Constants/statics are registered before body analysis so functions can
        // reference declarations regardless of item order.
        for item in &module.items {
            match item {
                Item::Const(decl) => self.analyze_const_decl(decl),
                Item::Static(decl) => self.analyze_static_decl(decl),
                _ => {}
            }
        }

        for item in &module.items {
            self.enforce_visibility_rules(item);
        }

        // Trait implementations must be visible before function body analysis so
        // generic bound checks do not depend on textual item order. Full impl
        // validation and method-body analysis still run in the normal item pass.
        for item in &module.items {
            match item {
                Item::TraitImpl(trait_impl) => {
                    self.predeclare_trait_impl(&trait_impl.trait_name, &trait_impl.type_name);
                }
                Item::Impl(impl_block) => {
                    if let Some(trait_name) = &impl_block.trait_name {
                        self.predeclare_trait_impl(trait_name, &impl_block.type_name);
                    }
                }
                _ => {}
            }
        }

        // Second pass: analyze function bodies
        for item in &module.items {
            self.analyze_item(item);
        }

        // Third pass: infer generic type arguments
        for item in &mut module.items {
            self.infer_generic_types_in_item(item);
        }

        // Fourth pass: fill type information in method calls
        for item in &mut module.items {
            self.fill_method_call_types_in_item(item);
        }

        // Flush qualified-path function discoveries so the midend knows
        // about cross-module calls that weren't brought in via `import`.
        module
            .imported_function_return_types.append(&mut self.qualified_fn_types);

        // Return collected errors
        std::mem::take(&mut self.errors)
    }

}
