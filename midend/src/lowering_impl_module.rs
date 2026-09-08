use super::*;

impl ASTLowering {
    pub fn lower_module(&mut self, ast_module: &ASTModule) -> Result<IRModule, Vec<MidendError>> {
        let mut ir_module = IRModule::new(&ast_module.name);
        ir_module.source_file = Some(self.source_file.clone());

        // Populate stdlib alias map for unqualified call resolution.
        self.std_import_aliases = ast_module.std_import_aliases.iter().cloned().collect();
        self.type_aliases.clear();
        for item in &ast_module.items {
            if let Item::TypeAlias(alias) = item {
                self.type_aliases
                    .insert(alias.name.clone(), alias.ty.clone());
            }
        }
        self.const_values.clear();
        self.static_globals.clear();
        for item in &ast_module.items {
            if let Item::Const(decl) = item {
                if let Some(value) = self.eval_const_expression(&decl.value) {
                    self.const_values.insert(decl.name.clone(), value);
                }
            }
        }

        // Materialize statics as real module globals before lowering any
        // function body. Semantic analysis has already enforced that their
        // initializer is compile-time evaluable, so a missing value here is a
        // lowering error rather than a synthetic zero initializer.
        for item in &ast_module.items {
            if let Item::Static(decl) = item {
                let Some(value) = self.eval_const_expression(&decl.value) else {
                    self.error(format!(
                        "static '{}' has no valid compile-time initializer",
                        decl.name
                    ));
                    continue;
                };
                let ty = decl
                    .ty
                    .as_ref()
                    .map(|annotation| self.lower_type_annotation(annotation))
                    .unwrap_or_else(|| self.infer_expr_ir_type(&decl.value));
                let global_key = format!("{}::{}", ast_module.name, decl.name);
                let id = ir_module.globals.len();
                ir_module.globals.push(Global {
                    id,
                    name: global_key.clone(),
                    ty: ty.clone(),
                    is_mutable: true,
                    initializer: Some(Self::lowered_const_to_ir_constant(&value)),
                });
                self.static_globals
                    .insert(decl.name.clone(), (global_key, ty));
            }
        }

        for (local_name, global_key, ty) in &ast_module.imported_static_globals {
            let ir_ty = self.lower_type(ty);
            if !ir_module.globals.iter().any(|global| global.name == *global_key) {
                let id = ir_module.globals.len();
                ir_module.globals.push(Global {
                    id,
                    name: global_key.clone(),
                    ty: ir_ty.clone(),
                    is_mutable: true,
                    initializer: None,
                });
            }
            self.static_globals
                .insert(local_name.clone(), (global_key.clone(), ir_ty));
        }

        // Pre-register enum/struct definitions from imported user modules so
        // that cross-module type references resolve before the local first pass.
        for enum_def in &ast_module.imported_enum_defs {
            if self.enum_definitions.contains_key(&enum_def.name) {
                // Check whether the existing registration is identical (same variant names).
                // Identical → safe duplicate (same type imported via two paths), skip.
                // Different → two modules export different types with the same name → error.
                let existing_variants: Vec<String> = self
                    .enum_definitions
                    .get(&enum_def.name)
                    .map(|v| v.iter().map(|(name, _, _)| name.clone()).collect())
                    .unwrap_or_default();
                let incoming_variants: Vec<String> =
                    enum_def.variants.iter().map(|v| v.name.clone()).collect();
                if existing_variants != incoming_variants {
                    self.error(format!(
                        "Name collision: two imported modules export different enum types named '{}'. \
                         Rename one of them to resolve the conflict.",
                        enum_def.name
                    ));
                }
                continue;
            }
            if !enum_def.type_params.is_empty() {
                self.generic_enums
                    .entry(enum_def.name.clone())
                    .or_insert_with(|| enum_def.clone());
            } else {
                let mut field_names = HashMap::new();
                let variants: Vec<(String, usize, Option<Vec<IRType>>)> = enum_def
                    .variants
                    .iter()
                    .enumerate()
                    .map(|(tag, variant)| {
                        let data_types = if let Some(types) = variant.data.as_ref() {
                            Some(
                                types
                                    .iter()
                                    .map(|ty| self.lower_type_annotation(ty))
                                    .collect(),
                            )
                        } else if let Some(fields) = variant.struct_data.as_ref() {
                            field_names.insert(
                                variant.name.clone(),
                                fields.iter().map(|(name, _)| name.clone()).collect(),
                            );
                            Some(
                                fields
                                    .iter()
                                    .map(|(_, ty)| self.lower_type_annotation(ty))
                                    .collect(),
                            )
                        } else {
                            None
                        };
                        (variant.name.clone(), tag, data_types)
                    })
                    .collect();
                self.enum_definitions
                    .insert(enum_def.name.clone(), variants);
                if !field_names.is_empty() {
                    self.enum_variant_field_names
                        .insert(enum_def.name.clone(), field_names);
                }
            }
        }
        for struct_def in &ast_module.imported_struct_defs {
            if self.struct_definitions.contains_key(&struct_def.name) {
                // Check whether the existing registration is identical (same field names).
                let existing_fields: Vec<String> = self
                    .struct_definitions
                    .get(&struct_def.name)
                    .map(|v| v.iter().map(|(name, _)| name.clone()).collect())
                    .unwrap_or_default();
                let incoming_fields: Vec<String> =
                    struct_def.fields.iter().map(|f| f.name.clone()).collect();
                if existing_fields != incoming_fields {
                    self.error(format!(
                        "Name collision: two imported modules export different struct types named '{}'. \
                         Rename one of them to resolve the conflict.",
                        struct_def.name
                    ));
                }
                continue;
            }
            if !struct_def.type_params.is_empty() {
                self.generic_structs
                    .entry(struct_def.name.clone())
                    .or_insert_with(|| struct_def.clone());
            } else {
                let fields: Vec<(String, IRType)> = struct_def
                    .fields
                    .iter()
                    .map(|field| {
                        let field_type = self.lower_type_annotation(&field.ty);
                        (field.name.clone(), field_type)
                    })
                    .collect();
                self.struct_definitions
                    .insert(struct_def.name.clone(), fields);
            }
        }

        // Preserve imported user-function signatures in the IR so AOT object
        // emission can declare the corresponding native linker imports. This
        // must happen after imported aggregate layouts are registered; an
        // imported enum or struct otherwise lowers to `Unknown` even though
        // the semantic signature is valid. JIT compilation can resolve these
        // through its persistent function map, while each AOT object is
        // intentionally generated in isolation.
        for (name, params, return_type) in &ast_module.imported_function_signatures {
            let external = ExternalFunction {
                name: name.clone(),
                params: params
                    .iter()
                    .map(|param| self.lower_type(param))
                    .collect(),
                return_type: self.lower_type(return_type),
            };
            if !ir_module
                .external_functions
                .iter()
                .any(|existing| existing.name == external.name)
            {
                ir_module.external_functions.push(external);
            }
            let parameter_types = params
                .iter()
                .map(|param| self.lower_type(param))
                .collect();
            self.function_parameter_types
                .entry(name.clone())
                .or_insert(parameter_types);
        }

        // Pre-register return types of imported user functions and methods
        // after imported layouts exist. Otherwise aggregate returns degrade to
        // opaque Pointer(Void), and later method calls lose their receiver type.
        for (name, ty) in &ast_module.imported_function_return_types {
            let ir_ty = self.lower_type(ty);
            self.function_return_types
                .entry(name.clone())
                .or_insert(ir_ty);
        }

        // Preserve trait implementations discovered in imported modules so
        // generic-bound checks and dyn coercions use the same coherence facts
        // as the semantic pass.
        for (trait_name, type_name, _) in &ast_module.imported_trait_impls {
            self.trait_implementations
                .insert((type_name.clone(), trait_name.clone()), true);
        }
        for function in &ast_module.imported_generic_functions {
            self.generic_functions
                .entry(function.name.clone())
                .or_insert_with(|| function.clone());
        }
        for trait_decl in &ast_module.imported_trait_decls {
            self.register_trait_metadata(trait_decl);
        }

        // First pass: collect struct and enum definitions, and trait implementations
        for item in &ast_module.items {
            if let Item::Struct(struct_def) = item {
                // Check if this is a generic struct
                if !struct_def.type_params.is_empty() {
                    // Store generic struct for later monomorphization
                    self.generic_structs
                        .insert(struct_def.name.clone(), struct_def.clone());
                    // generic struct stored for monomorphization
                } else {
                    // Regular struct - process immediately
                    let fields: Vec<(String, IRType)> = struct_def
                        .fields
                        .iter()
                        .map(|field| {
                            let field_type = self.lower_type_annotation(&field.ty);
                            (field.name.clone(), field_type)
                        })
                        .collect();
                    self.struct_definitions
                        .insert(struct_def.name.clone(), fields);
                }
            } else if let Item::Enum(enum_def) = item {
                // Check if this is a generic enum
                if !enum_def.type_params.is_empty() {
                    let shadows_builtin_generic =
                        matches!(enum_def.name.as_str(), "Option" | "Result");
                    // Store generic enum for later monomorphization
                    self.generic_enums
                        .insert(enum_def.name.clone(), enum_def.clone());
                    if shadows_builtin_generic {
                        self.enum_definitions.remove(&enum_def.name);
                    }
                    // generic enum stored for monomorphization
                } else {
                    if matches!(enum_def.name.as_str(), "Option" | "Result") {
                        self.generic_enums.remove(&enum_def.name);
                    }
                    // Regular enum - process immediately
                    let mut field_names = HashMap::new();
                    let variants: Vec<(String, usize, Option<Vec<IRType>>)> = enum_def
                        .variants
                        .iter()
                        .enumerate()
                        .map(|(tag, variant)| {
                            let data_types = if let Some(types) = variant.data.as_ref() {
                                Some(
                                    types
                                        .iter()
                                        .map(|ty| self.lower_type_annotation(ty))
                                        .collect(),
                                )
                            } else if let Some(fields) = variant.struct_data.as_ref() {
                                field_names.insert(
                                    variant.name.clone(),
                                    fields.iter().map(|(name, _)| name.clone()).collect(),
                                );
                                Some(
                                    fields
                                        .iter()
                                        .map(|(_, ty)| self.lower_type_annotation(ty))
                                        .collect(),
                                )
                            } else {
                                None
                            };
                            (variant.name.clone(), tag, data_types)
                        })
                        .collect();
                    self.enum_definitions
                        .insert(enum_def.name.clone(), variants);
                    if !field_names.is_empty() {
                        self.enum_variant_field_names
                            .insert(enum_def.name.clone(), field_names);
                    }
                }
            } else if let Item::Impl(impl_block) = item {
                // `impl Type { ... }` never has a trait_name (that goes to Item::TraitImpl).
                // Nothing to do here for trait registration in this path.
                let _ = impl_block;
            } else if let Item::TraitImpl(trait_impl) = item {
                // Register that `type_name` implements `trait_name`
                let key = (trait_impl.type_name.clone(), trait_impl.trait_name.clone());
                self.trait_implementations.insert(key, true);
            } else if let Item::Trait(trait_decl) = item {
                self.trait_declarations
                    .insert(trait_decl.name.clone(), trait_decl.clone());
                // Record method declaration order for vtable slot lookup
                let methods: Vec<String> =
                    trait_decl.methods.iter().map(|m| m.name.clone()).collect();
                self.trait_method_order
                    .insert(trait_decl.name.clone(), methods);
                let signatures = trait_decl
                    .methods
                    .iter()
                    .map(|method| {
                        let params = method
                            .params
                            .iter()
                            .filter(|param| !param.is_self)
                            .filter_map(|param| param.type_annotation.as_ref())
                            .map(|ann| self.lower_type_annotation(ann))
                            .collect::<Vec<_>>();
                        let ret = method
                            .return_type
                            .as_ref()
                            .map(|ann| self.lower_type_annotation(ann))
                            .unwrap_or(IRType::Void);
                        let return_type = if method.is_async {
                            IRType::Task {
                                output: Box::new(ret),
                            }
                        } else {
                            ret
                        };
                        (method.name.clone(), (params, return_type))
                    })
                    .collect::<HashMap<_, _>>();
                self.trait_method_signatures
                    .insert(trait_decl.name.clone(), signatures);
            }
        }

        // Normalize trait metadata after every declaration has been seen so
        // child traits can be declared before their parents. Vtable slots are
        // parent-first, while a child declaration may override a parent's
        // signature under the same method name.
        let trait_names: Vec<String> = self.trait_declarations.keys().cloned().collect();
        for trait_name in trait_names {
            let mut order = Vec::new();
            let mut seen = HashSet::new();
            self.collect_trait_method_order_recursive(&trait_name, &mut seen, &mut order);
            self.trait_method_order.insert(trait_name.clone(), order);

            let mut methods = HashMap::new();
            self.collect_trait_methods_recursive(&trait_name, &mut methods);
            let signatures = methods
                .into_iter()
                .map(|(name, method)| {
                    let params = method
                        .params
                        .iter()
                        .filter(|param| !param.is_self)
                        .filter_map(|param| param.type_annotation.as_ref())
                        .map(|ann| self.lower_type_annotation(ann))
                        .collect::<Vec<_>>();
                    let ret = method
                        .return_type
                        .as_ref()
                        .map(|ann| self.lower_type_annotation(ann))
                        .unwrap_or(IRType::Void);
                    let return_type = if method.is_async {
                        IRType::Task {
                            output: Box::new(ret),
                        }
                    } else {
                        ret
                    };
                    (name, (params, return_type))
                })
                .collect::<HashMap<_, _>>();
            self.trait_method_signatures
                .insert(trait_name, signatures);
        }

        // Second pass: pre-register return types for regular functions and impl methods
        for item in &ast_module.items {
            if let Item::Function(func) = item {
                if func.type_params.is_empty() {
                    self.function_parameter_types.insert(
                        func.name.clone(),
                        func.params
                            .iter()
                            .map(|param| {
                                param
                                    .ty
                                    .as_ref()
                                    .map(|ty| self.lower_type_annotation(ty))
                                    .unwrap_or(IRType::Unknown)
                            })
                            .collect(),
                    );
                    let body_return_type = func
                        .return_type
                        .as_ref()
                        .map(|t| self.lower_type_annotation(t))
                        .unwrap_or(IRType::Void);
                    let return_type = if func.is_async {
                        IRType::Task {
                            output: Box::new(body_return_type),
                        }
                    } else {
                        body_return_type
                    };
                    self.function_return_types
                        .insert(func.name.clone(), return_type);
                }
            } else if let Item::Impl(impl_block) = item {
                for method in &impl_block.methods {
                    let mangled = format!("{}_{}", impl_block.type_name, method.name);
                    let body_return_type = method
                        .return_type
                        .as_ref()
                        .map(|t| self.lower_type_annotation(t))
                        .unwrap_or(IRType::Void);
                    let return_type = if method.is_async {
                        IRType::Task {
                            output: Box::new(body_return_type),
                        }
                    } else {
                        body_return_type
                    };
                    self.function_return_types
                        .entry(mangled)
                        .or_insert(return_type);
                }
            } else if let Item::TraitImpl(trait_impl) = item {
                for method in &trait_impl.methods {
                    let mangled = format!("{}_{}", trait_impl.type_name, method.name);
                    let body_return_type = method
                        .return_type
                        .as_ref()
                        .map(|t| self.lower_type_annotation(t))
                        .unwrap_or(IRType::Void);
                    let return_type = if method.is_async {
                        IRType::Task {
                            output: Box::new(body_return_type),
                        }
                    } else {
                        body_return_type
                    };
                    self.function_return_types
                        .entry(mangled)
                        .or_insert(return_type);
                }
                for method in
                    self.collect_default_trait_methods(&trait_impl.trait_name, &trait_impl.methods)
                {
                    let mangled = format!("{}_{}", trait_impl.type_name, method.name);
                    let return_type = method
                        .return_type
                        .as_ref()
                        .map(|t| self.lower_type_annotation(t))
                        .unwrap_or(IRType::Void);
                    self.function_return_types
                        .entry(mangled)
                        .or_insert(return_type);
                }
            }
        }

        // Third pass: lower regular functions and impl block methods to IR
        for item in &ast_module.items {
            if let Item::Function(func) = item {
                // Store generic functions for later monomorphization
                if !func.type_params.is_empty() {
                    self.generic_functions
                        .insert(func.name.clone(), func.clone());
                    // generic function stored for monomorphization
                    continue;
                }

                let ir_func = self.lower_function(func);
                ir_module.add_function(ir_func);
            } else if let Item::Impl(impl_block) = item {
                if impl_block.type_args.is_empty() {
                    for method in &impl_block.methods {
                        let ir_func = self.lower_method(method, &impl_block.type_name);
                        ir_module.add_function(ir_func);
                    }
                } else if let Some(struct_def) = self.generic_structs.get(&impl_block.type_name).cloned()
                {
                    // Template impl on a generic struct: register each method for
                    // per-instantiation specialization (R-211).
                    let type_params = struct_def.type_params.clone();
                    for method in &impl_block.methods {
                        let key = format!("{}_{}", impl_block.type_name, method.name);
                        self.generic_impl_methods
                            .insert(key, (method.clone(), type_params.clone()));
                    }
                } else if let Some(enum_def) = self.generic_enums.get(&impl_block.type_name).cloned()
                {
                    let type_params = enum_def.type_params.clone();
                    for method in &impl_block.methods {
                        let key = format!("{}_{}", impl_block.type_name, method.name);
                        self.generic_impl_methods
                            .insert(key, (method.clone(), type_params.clone()));
                    }
                } else {
                    self.error(format!(
                        "Impl with type arguments on non-generic type '{}'",
                        impl_block.type_name
                    ));
                }
            } else if let Item::TraitImpl(trait_impl) = item {
                for method in &trait_impl.methods {
                    let ir_func = self.lower_method(method, &trait_impl.type_name);
                    ir_module.add_function(ir_func);
                }
                for method in
                    self.collect_default_trait_methods(&trait_impl.trait_name, &trait_impl.methods)
                {
                    let ir_func = self.lower_method(&method, &trait_impl.type_name);
                    ir_module.add_function(ir_func);
                }
            }
        }

        // Process pending monomorphization requests
        self.process_monomorphization_requests(&mut ir_module);

        // Process pending generic impl method specializations (R-211). These may
        // nest (a specialized method can call another generic impl method), so
        // drain until empty.
        loop {
            if self.pending_method_specializations.is_empty() {
                break;
            }
            let requests = std::mem::take(&mut self.pending_method_specializations);
            for request in requests {
                self.process_method_specialization(&request, &mut ir_module);
            }
        }

        // Emit generated coroutine bodies after all public ramps. Their names
        // are now visible to FuncAddr/Call verification and backend linkage.
        let coroutines = std::mem::take(&mut self.pending_coroutines);
        for coroutine_func in coroutines {
            ir_module.add_function(coroutine_func);
        }

        // Emit any lambda functions collected during lowering.
        let lambdas = std::mem::take(&mut self.pending_lambdas);
        for lambda in lambdas {
            ir_module.add_function(lambda);
        }
        // Mark direct self-tail-recursion so the backend can emit native
        // Cranelift `return_call`s (see passes::tail_call_marking).
        crate::passes::tail_call_marking::mark_tail_self_recursion(&mut ir_module);

        if self.errors.is_empty() {
            Ok(ir_module)
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }

}
