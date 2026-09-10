use super::*;

impl SemanticAnalyzer {
    fn seed_builtin_module_namespaces(&mut self) {
        const BUILTIN_MODULES: &[&str] = &[
            "std",
            "std.io",
            "std.math",
            "std.collections",
            "std.string",
            "std.convert",
            "std.random",
            "std.fs",
            "std.env",
            "std.option",
            "std.result",
            "std.char",
            "std.time",
            "std.tensor",
            "std.ml",
            "std.concurrent",
            "std.serve",
            "std.api",
            "std.api.http",
            "std.api.server",
            "std.api.client",
            "std.api.json",
            "std.api.tls",
            "std.api.routing",
            "std.api.query",
            "std.api.form",
            "std.api.multipart",
            "std.api.handler",
            "std.api.cors",
            "std.api.middleware",
            "std.api.validation",
            "std.api.errors",
            "std.api.security",
        ];

        for module_path in BUILTIN_MODULES {
            let segments: Vec<&str> = module_path.split('.').collect();
            for i in 0..segments.len() {
                self.module_namespaces.insert(segments[..=i].join("."));
            }
        }
    }

    /// Create an analyzer with a private registry seeded with stdlib modules.
    /// Suitable for single-file use (e.g. tests, REPL).
    pub fn new() -> Self {
        let mut reg = ModuleRegistry::new();
        register_builtin_modules(&mut reg);
        Self::new_with_registry(Arc::new(RwLock::new(reg)), None)
    }

    /// Create an analyzer that shares a registry with other modules being compiled
    /// by the same pipeline run.  `package_name` is the value of the `name` field
    /// in `spectra.toml`; it is used for `internal` visibility enforcement.
    pub fn new_with_registry(
        registry: Arc<RwLock<ModuleRegistry>>,
        package_name: Option<String>,
    ) -> Self {
        let mut analyzer = Self {
            errors: Vec::new(),
            symbols: vec![HashMap::new()], // Start with global scope
            functions: HashMap::new(),
            function_type_params: HashMap::new(),
            enum_definitions: HashMap::new(),
            methods: HashMap::new(),
            method_definitions: HashMap::new(),
            method_visibility: HashMap::new(),
            drop_types: HashSet::new(),
            traits: HashMap::new(),
            trait_impls: HashMap::new(),
            trait_impl_type_args: HashMap::new(),
            trait_type_params: HashMap::new(),
            struct_infos: HashMap::new(),
            json_struct_derives: HashMap::new(),
            enum_infos: HashMap::new(),
            generic_structs: HashMap::new(),
            generic_enums: HashMap::new(),
            type_aliases: HashMap::new(),
            loop_depth: 0,
            current_function: None,
            trait_signatures: HashMap::new(),
            current_return_type: None,
            current_expected_type: None,
            async_context_depth: 0,
            generic_params: Vec::new(),
            generic_param_bounds: Vec::new(),
            registry,
            current_package: package_name,
            current_module_name: None,
            symbol_resolutions: HashMap::new(),
            module_namespaces: HashSet::new(),
            qualified_fn_types: Vec::new(),
            const_values: HashMap::new(),
            uaf_frame: None,
            uaf_suspend_use_checks: 0,
        };
        analyzer.register_builtin_generic_types();
        analyzer.register_builtin_async_traits();
        analyzer.seed_builtin_module_namespaces();
        analyzer
    }

    pub(crate) fn register_builtin_async_traits(&mut self) {
        let async_task = |output: Type| Type::Task {
            output: Box::new(output),
        };

        let future_methods = HashMap::from([
            (
                "poll".to_string(),
                TraitMethodInfo {
                    signature: FunctionSignature {
                        params: vec![Type::Unknown],
                        return_type: async_task(Type::Int),
                        self_kind: Some(SelfParamKind::Reference { mutable: false }),
                        is_async: true,
                    },
                    has_default: false,
                    default_body: None,
                },
            ),
            (
                "cancel".to_string(),
                TraitMethodInfo {
                    signature: FunctionSignature {
                        params: vec![Type::Unknown],
                        return_type: Type::Int,
                        self_kind: Some(SelfParamKind::Reference { mutable: false }),
                        is_async: false,
                    },
                    has_default: false,
                    default_body: None,
                },
            ),
        ]);

        let stream_methods = HashMap::from([
            (
                "next".to_string(),
                TraitMethodInfo {
                    signature: FunctionSignature {
                        params: vec![Type::Unknown],
                        return_type: async_task(Type::Int),
                        self_kind: Some(SelfParamKind::Reference { mutable: false }),
                        is_async: true,
                    },
                    has_default: false,
                    default_body: None,
                },
            ),
            (
                "cancel".to_string(),
                TraitMethodInfo {
                    signature: FunctionSignature {
                        params: vec![Type::Unknown],
                        return_type: Type::Int,
                        self_kind: Some(SelfParamKind::Reference { mutable: false }),
                        is_async: false,
                    },
                    has_default: false,
                    default_body: None,
                },
            ),
        ]);

        self.traits.insert("Future".to_string(), future_methods);
        self.traits.insert("Stream".to_string(), stream_methods);
        self.trait_signatures.insert(
            "Future".to_string(),
            HashMap::from([
                (
                    "poll".to_string(),
                    TraitMethodSignature {
                        params: vec![ParameterInfo {
                            is_self: true,
                            is_reference: true,
                            is_mutable: false,
                            ty: None,
                        }],
                        return_type: Some(TypeAnnotationPattern::Simple(vec!["int".to_string()])),
                        has_default_body: false,
                        is_async: true,
                    },
                ),
                (
                    "cancel".to_string(),
                    TraitMethodSignature {
                        params: vec![ParameterInfo {
                            is_self: true,
                            is_reference: true,
                            is_mutable: false,
                            ty: None,
                        }],
                        return_type: Some(TypeAnnotationPattern::Simple(vec!["int".to_string()])),
                        has_default_body: false,
                        is_async: false,
                    },
                ),
            ]),
        );
        self.trait_signatures.insert(
            "Stream".to_string(),
            HashMap::from([
                (
                    "next".to_string(),
                    TraitMethodSignature {
                        params: vec![ParameterInfo {
                            is_self: true,
                            is_reference: true,
                            is_mutable: false,
                            ty: None,
                        }],
                        return_type: Some(TypeAnnotationPattern::Simple(vec!["int".to_string()])),
                        has_default_body: false,
                        is_async: true,
                    },
                ),
                (
                    "cancel".to_string(),
                    TraitMethodSignature {
                        params: vec![ParameterInfo {
                            is_self: true,
                            is_reference: true,
                            is_mutable: false,
                            ty: None,
                        }],
                        return_type: Some(TypeAnnotationPattern::Simple(vec!["int".to_string()])),
                        has_default_body: false,
                        is_async: false,
                    },
                ),
            ]),
        );
    }

    /// Pre-register built-in generic types (`Option<T>`, `Result<T, E>`) so any
    /// module can use them without declaring them locally.
    fn register_builtin_generic_types(&mut self) {
        use crate::ast::{TypeAnnotation, TypeAnnotationKind, TypeParameter};

        let dummy = Span::dummy();

        let make_type_param = |name: &str| TypeParameter {
            name: name.to_string(),
            bounds: vec![],
            span: dummy,
        };

        let simple_type_ann = |name: &str| TypeAnnotation {
            kind: TypeAnnotationKind::Simple {
                segments: vec![name.to_string()],
            },
            span: dummy,
        };

        // ── Option<T> ──────────────────────────────────────────────────────────
        {
            let type_params = vec![make_type_param("T")];
            let variant_names = vec!["Some".to_string(), "None".to_string()];

            self.generic_enums
                .insert("Option".to_string(), (type_params, variant_names.clone()));
            self.enum_definitions
                .insert("Option".to_string(), variant_names);

            let mut variants_map = HashMap::new();
            variants_map.insert(
                "None".to_string(),
                EnumVariantInfo {
                    data: None,
                    struct_data: None,
                    span: dummy,
                },
            );
            variants_map.insert(
                "Some".to_string(),
                EnumVariantInfo {
                    data: Some(vec![simple_type_ann("T")]),
                    struct_data: None,
                    span: dummy,
                },
            );
            self.enum_infos.insert(
                "Option".to_string(),
                EnumInfo {
                    visibility: Visibility::Public,
                    type_params: vec!["T".to_string()],
                    variants: variants_map,
                },
            );
        }

        // ── Result<T, E> ───────────────────────────────────────────────────────
        {
            let type_params = vec![make_type_param("T"), make_type_param("E")];
            let variant_names = vec!["Ok".to_string(), "Err".to_string()];

            self.generic_enums
                .insert("Result".to_string(), (type_params, variant_names.clone()));
            self.enum_definitions
                .insert("Result".to_string(), variant_names);

            let mut variants_map = HashMap::new();
            variants_map.insert(
                "Ok".to_string(),
                EnumVariantInfo {
                    data: Some(vec![simple_type_ann("T")]),
                    struct_data: None,
                    span: dummy,
                },
            );
            variants_map.insert(
                "Err".to_string(),
                EnumVariantInfo {
                    data: Some(vec![simple_type_ann("E")]),
                    struct_data: None,
                    span: dummy,
                },
            );
            self.enum_infos.insert(
                "Result".to_string(),
                EnumInfo {
                    visibility: Visibility::Public,
                    type_params: vec!["T".to_string(), "E".to_string()],
                    variants: variants_map,
                },
            );
        }

        // `std.error.Error` is a runtime-owned record used by stable
        // `Result<_, Error>` signatures.  Registering its shape globally is
        // necessary even when a module imports only `std.fs`: generic Result
        // specialization must resolve the `Error` payload as a struct rather
        // than silently degrading it to an enum-shaped name.
        {
            let fields = [
                "code",
                "message",
                "operation",
                "context",
                "origin",
                "retryable",
            ]
            .into_iter()
            .map(|name| {
                (
                    name.to_string(),
                    StructFieldInfo {
                        ty: simple_type_ann(match name {
                            "code" => "int",
                            "message" | "operation" | "context" | "origin" => "string",
                            "retryable" => "bool",
                            _ => "unknown",
                        }),
                        span: dummy,
                        visibility: Visibility::Public,
                    },
                )
            })
            .collect();
            self.struct_infos.insert(
                "Error".to_string(),
                StructInfo {
                    visibility: Visibility::Public,
                    type_params: Vec::new(),
                    fields,
                },
            );
        }

        // Runtime collections currently have a concrete scalar ABI (int
        // elements and int keys/values), but their public type names are
        // generic applications rather than untyped integer handles. This
        // keeps the migration explicit while wider element ABIs are added.
        {
            let list_params = vec![make_type_param("T")];
            self.generic_structs
                .insert("List".to_string(), (list_params, Vec::new()));
            self.struct_infos.insert(
                "List".to_string(),
                StructInfo {
                    visibility: Visibility::Public,
                    type_params: vec!["T".to_string()],
                    fields: HashMap::new(),
                },
            );

            let map_params = vec![make_type_param("K"), make_type_param("V")];
            self.generic_structs
                .insert("Map".to_string(), (map_params, Vec::new()));
            self.struct_infos.insert(
                "Map".to_string(),
                StructInfo {
                    visibility: Visibility::Public,
                    type_params: vec!["K".to_string(), "V".to_string()],
                    fields: HashMap::new(),
                },
            );

            for name in ["Set", "Iterator"] {
                let params = vec![make_type_param("T")];
                self.generic_structs
                    .insert(name.to_string(), (params, Vec::new()));
                self.struct_infos.insert(
                    name.to_string(),
                    StructInfo {
                        visibility: Visibility::Public,
                        type_params: vec!["T".to_string()],
                        fields: HashMap::new(),
                    },
                );
            }
        }
    }
}

impl Default for SemanticAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
