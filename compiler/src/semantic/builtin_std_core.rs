fn make_std_io() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "io".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    // print(value: any) -> unit
    // The runtime FFI accepts a single value and prints it.
    exports
        .functions
        .insert(
            "print".to_string(),
            pub_fn(
                vec![Type::TypeParameter { name: "T".to_string() }],
                Type::Unit,
            ),
        );
    // println(value: any) -> unit  (print + newline)
    exports.functions.insert(
        "println".to_string(),
        pub_fn(
            vec![Type::TypeParameter { name: "T".to_string() }],
            Type::Unit,
        ),
    );
    // eprint(value: any) -> unit  (stderr, no newline)
    exports.functions.insert(
        "eprint".to_string(),
        pub_fn(
            vec![Type::TypeParameter { name: "T".to_string() }],
            Type::Unit,
        ),
    );
    // eprintln(value: any) -> unit
    exports.functions.insert(
        "eprintln".to_string(),
        pub_fn(
            vec![Type::TypeParameter { name: "T".to_string() }],
            Type::Unit,
        ),
    );
    // flush() -> unit
    exports
        .functions
        .insert("flush".to_string(), pub_fn(vec![], Type::Unit));
    // read_line() -> string
    exports
        .functions
        .insert("read_line".to_string(), pub_fn(vec![], Type::String));
    // input(prompt: string) -> string  (prints prompt, flushes, reads line)
    exports.functions.insert(
        "input".to_string(),
        pub_fn(vec![Type::String], Type::String),
    );

    exports
}

fn make_std_math() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "math".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    exports
        .functions
        .insert("abs".to_string(), pub_fn(vec![Type::Int], Type::Int));
    exports.functions.insert(
        "min".to_string(),
        pub_fn(vec![Type::Int, Type::Int], Type::Int),
    );
    exports.functions.insert(
        "max".to_string(),
        pub_fn(vec![Type::Int, Type::Int], Type::Int),
    );
    exports.functions.insert(
        "clamp".to_string(),
        pub_fn(vec![Type::Int, Type::Int, Type::Int], Type::Int),
    );
    exports
        .functions
        .insert("sqrt_f".to_string(), pub_fn(vec![Type::Float], Type::Float));
    exports.functions.insert(
        "pow_f".to_string(),
        pub_fn(vec![Type::Float, Type::Float], Type::Float),
    );
    exports.functions.insert(
        "floor_f".to_string(),
        pub_fn(vec![Type::Float], Type::Float),
    );
    exports
        .functions
        .insert("ceil_f".to_string(), pub_fn(vec![Type::Float], Type::Float));
    exports.functions.insert(
        "round_f".to_string(),
        pub_fn(vec![Type::Float], Type::Float),
    );
    exports
        .functions
        .insert("sin_f".to_string(), pub_fn(vec![Type::Float], Type::Float));
    exports
        .functions
        .insert("cos_f".to_string(), pub_fn(vec![Type::Float], Type::Float));
    exports
        .functions
        .insert("tan_f".to_string(), pub_fn(vec![Type::Float], Type::Float));
    exports
        .functions
        .insert("log_f".to_string(), pub_fn(vec![Type::Float], Type::Float));
    exports
        .functions
        .insert("log2_f".to_string(), pub_fn(vec![Type::Float], Type::Float));
    exports.functions.insert(
        "log10_f".to_string(),
        pub_fn(vec![Type::Float], Type::Float),
    );
    exports.functions.insert(
        "atan2_f".to_string(),
        pub_fn(vec![Type::Float, Type::Float], Type::Float),
    );
    exports
        .functions
        .insert("pi".to_string(), pub_fn(vec![], Type::Float));
    exports
        .functions
        .insert("e_const".to_string(), pub_fn(vec![], Type::Float));
    // sign(n: int) -> int — returns -1, 0, or 1
    exports
        .functions
        .insert("sign".to_string(), pub_fn(vec![Type::Int], Type::Int));
    // gcd(a: int, b: int) -> int
    exports.functions.insert(
        "gcd".to_string(),
        pub_fn(vec![Type::Int, Type::Int], Type::Int),
    );
    // lcm(a: int, b: int) -> int
    exports.functions.insert(
        "lcm".to_string(),
        pub_fn(vec![Type::Int, Type::Int], Type::Int),
    );
    // is_nan_f(x: float) -> bool
    exports.functions.insert(
        "is_nan_f".to_string(),
        pub_fn(vec![Type::Float], Type::Bool),
    );
    // is_infinite_f(x: float) -> bool
    exports.functions.insert(
        "is_infinite_f".to_string(),
        pub_fn(vec![Type::Float], Type::Bool),
    );
    // abs_f(x: float) -> float
    exports
        .functions
        .insert("abs_f".to_string(), pub_fn(vec![Type::Float], Type::Float));

    exports
}

fn make_std_numeric() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "numeric".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };
    for (name, ty) in [
        ("i8", Type::ExactInt { signed: true, width: IntWidth::I8 }),
        ("i16", Type::ExactInt { signed: true, width: IntWidth::I16 }),
        ("i32", Type::ExactInt { signed: true, width: IntWidth::I32 }),
        ("i64", Type::ExactInt { signed: true, width: IntWidth::I64 }),
        ("u8", Type::ExactInt { signed: false, width: IntWidth::I8 }),
        ("u16", Type::ExactInt { signed: false, width: IntWidth::I16 }),
        ("u32", Type::ExactInt { signed: false, width: IntWidth::I32 }),
        ("u64", Type::ExactInt { signed: false, width: IntWidth::I64 }),
    ] {
        for op in ["add", "sub", "mul"] {
            exports.functions.insert(
                format!("wrapping_{op}_{name}"),
                pub_fn(vec![ty.clone(), ty.clone()], ty.clone()),
            );
        }
    }
    exports.functions.insert(
        "checked_f32".to_string(),
        pub_fn(
            vec![Type::ExactFloat { width: FloatWidth::F64 }],
            Type::ExactFloat { width: FloatWidth::F32 },
        ),
    );
    exports
}

/// Public collection surface.  Potentially empty reads are represented by
/// `Option<T>`; the sentinel ABI remains available only through the explicit
/// `std.compat.collections` namespace.
fn make_std_collections() -> ModuleExports {
    let mut exports = make_std_collections_legacy();
    exports.stdlib_path = Some(vec!["std".to_string(), "collections".to_string()]);

    let option = Type::Enum {
        name: "Option".to_string(),
    };
    let list = Type::Struct {
        name: "List".to_string(),
    };
    let map = Type::Struct {
        name: "Map".to_string(),
    };
    let set = Type::Struct {
        name: "Set".to_string(),
    };
    let iterator = Type::Struct {
        name: "Iterator".to_string(),
    };
    for name in ["list_get", "list_pop", "list_pop_front", "list_remove_at"] {
        let params = if name == "list_get" || name == "list_remove_at" {
            vec![list.clone(), Type::Int]
        } else {
            vec![list.clone()]
        };
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, option.clone()));
    }
    for name in ["map_get", "map_remove"] {
        exports.functions.insert(
            name.to_string(),
            pub_fn(
                vec![map.clone(), Type::TypeParameter { name: "K".to_string() }],
                option.clone(),
            ),
        );
    }
    exports.functions.insert(
        "set_get".to_string(),
        pub_fn(vec![set, Type::Int], option),
    );
    exports.functions.insert(
        "iterator_next".to_string(),
        pub_fn(vec![iterator], Type::Enum {
            name: "Option".to_string(),
        }),
    );

    exports
}

/// Compatibility-only collection surface retaining the historic sentinel
/// return values for callers that have not migrated yet.
fn make_std_compat_collections() -> ModuleExports {
    let mut exports = make_std_collections_legacy();
    exports.functions.retain(|name, _| {
        matches!(
            name.as_str(),
            "list_get"
                | "list_pop"
                | "list_pop_front"
                | "list_remove_at"
                | "map_get"
                | "map_remove"
        )
    });
    exports.stdlib_path = Some(vec![
        "std".to_string(),
        "compat".to_string(),
        "collections".to_string(),
    ]);
    exports
}

fn make_std_collections_legacy() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "collections".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    let list = Type::Struct {
        name: "List".to_string(),
    };
    let map = Type::Struct {
        name: "Map".to_string(),
    };
    let option = Type::Enum {
        name: "Option".to_string(),
    };
    let element = Type::TypeParameter {
        name: "T".to_string(),
    };
    let key = Type::TypeParameter {
        name: "K".to_string(),
    };
    let value = Type::TypeParameter {
        name: "V".to_string(),
    };
    let set = Type::Struct {
        name: "Set".to_string(),
    };
    let iterator = Type::Struct {
        name: "Iterator".to_string(),
    };

    exports
        .functions
        .insert("list_new".to_string(), pub_fn(vec![], list.clone()));
    exports.functions.insert(
        "list_push".to_string(),
        pub_fn(vec![list.clone(), element.clone()], Type::Unit),
    );
    exports
        .functions
        .insert("list_len".to_string(), pub_fn(vec![list.clone()], Type::Int));
    exports.functions.insert(
        "list_get".to_string(),
        pub_fn(vec![list.clone(), Type::Int], Type::Int),
    );
    // Typed accessors are the non-sentinel contract. The legacy accessors
    // above remain available only for compatibility while callers migrate.
    exports.functions.insert(
        "list_get_option".to_string(),
        pub_fn(vec![list.clone(), Type::Int], option.clone()),
    );
    exports.functions.insert(
        "list_set".to_string(),
        pub_fn(vec![list.clone(), Type::Int, element.clone()], Type::Unit),
    );
    exports.functions.insert(
        "list_contains".to_string(),
        pub_fn(vec![list.clone(), element.clone()], Type::Bool),
    );
    exports.functions.insert(
        "list_clear".to_string(),
        pub_fn(vec![list.clone()], Type::Unit),
    );
    exports
        .functions
        .insert("list_free".to_string(), pub_fn(vec![list.clone()], Type::Unit));
    // list_free_all() -> int
    exports
        .functions
        .insert("list_free_all".to_string(), pub_fn(vec![], Type::Int));
    // Compatibility reads keep the historical integer/sentinel contract.
    exports
        .functions
        .insert("list_pop".to_string(), pub_fn(vec![list.clone()], Type::Int));
    exports.functions.insert(
        "list_pop_front".to_string(),
        pub_fn(vec![list.clone()], Type::Int),
    );
    exports.functions.insert(
        "list_pop_option".to_string(),
        pub_fn(vec![list.clone()], option.clone()),
    );
    exports.functions.insert(
        "list_pop_front_option".to_string(),
        pub_fn(vec![list.clone()], option.clone()),
    );
    exports.functions.insert(
        "list_insert_at".to_string(),
        pub_fn(vec![list.clone(), Type::Int, element.clone()], Type::Unit),
    );
    exports.functions.insert(
        "list_remove_at".to_string(),
        pub_fn(vec![list.clone(), Type::Int], Type::Int),
    );
    exports.functions.insert(
        "list_remove_at_option".to_string(),
        pub_fn(vec![list.clone(), Type::Int], option.clone()),
    );
    exports.functions.insert(
        "list_index_of".to_string(),
        pub_fn(vec![list.clone(), element.clone()], Type::Int),
    );
    exports
        .functions
        .insert("list_sort".to_string(), pub_fn(vec![list.clone()], Type::Unit));
    let fn_int_to_int = Type::Fn {
        params: vec![Type::Int],
        return_type: Box::new(Type::Int),
    };
    let fn_int_to_bool = Type::Fn {
        params: vec![Type::Int],
        return_type: Box::new(Type::Bool),
    };
    let fn_int_int_to_int = Type::Fn {
        params: vec![Type::Int, Type::Int],
        return_type: Box::new(Type::Int),
    };
    exports.functions.insert(
        "list_map".to_string(),
        pub_fn(vec![list.clone(), fn_int_to_int.clone()], list.clone()),
    );
    exports.functions.insert(
        "list_filter".to_string(),
        pub_fn(vec![list.clone(), fn_int_to_bool], list.clone()),
    );
    exports.functions.insert(
        "list_reduce".to_string(),
        pub_fn(
            vec![list.clone(), Type::Int, fn_int_int_to_int.clone()],
            Type::Int,
        ),
    );
    exports.functions.insert(
        "list_sort_by".to_string(),
        pub_fn(vec![list.clone(), fn_int_int_to_int], Type::Unit),
    );

    // ── map API (R-3123: expose existing runtime HashMap<i64, i64>) ──────────
    exports
        .functions
        .insert("map_new".to_string(), pub_fn(vec![], map.clone()));
    exports.functions.insert(
        "map_set".to_string(),
        pub_fn(vec![map.clone(), key.clone(), value.clone()], Type::Unit),
    );
    exports.functions.insert(
        "map_get".to_string(),
        pub_fn(vec![map.clone(), key.clone()], Type::Int),
    );
    exports.functions.insert(
        "map_get_option".to_string(),
        pub_fn(vec![map.clone(), key.clone()], option.clone()),
    );
    exports.functions.insert(
        "map_contains".to_string(),
        pub_fn(vec![map.clone(), key.clone()], Type::Bool),
    );
    exports.functions.insert(
        "map_remove".to_string(),
        pub_fn(vec![map.clone(), key.clone()], Type::Int),
    );
    exports.functions.insert(
        "map_remove_option".to_string(),
        pub_fn(vec![map.clone(), key], option),
    );
    exports
        .functions
        .insert("map_len".to_string(), pub_fn(vec![map.clone()], Type::Int));
    exports
        .functions
        .insert("map_clear".to_string(), pub_fn(vec![map.clone()], Type::Unit));
    exports
        .functions
        .insert("map_free".to_string(), pub_fn(vec![map], Type::Unit));

    // ── set API ───────────────────────────────────────────────────────────
    exports
        .functions
        .insert("set_new".to_string(), pub_fn(vec![], set.clone()));
    exports.functions.insert(
        "set_insert".to_string(),
        pub_fn(vec![set.clone(), element.clone()], Type::Bool),
    );
    exports.functions.insert(
        "set_contains".to_string(),
        pub_fn(vec![set.clone(), element.clone()], Type::Bool),
    );
    exports.functions.insert(
        "set_remove".to_string(),
        pub_fn(vec![set.clone(), element.clone()], Type::Bool),
    );
    exports
        .functions
        .insert("set_len".to_string(), pub_fn(vec![set.clone()], Type::Int));
    exports.functions.insert(
        "set_get".to_string(),
        pub_fn(
            vec![set.clone(), Type::Int],
            Type::Enum {
                name: "Option".to_string(),
            },
        ),
    );
    exports
        .functions
        .insert("set_clear".to_string(), pub_fn(vec![set.clone()], Type::Unit));
    exports
        .functions
        .insert("set_free".to_string(), pub_fn(vec![set], Type::Unit));

    // ── Iterator protocol ─────────────────────────────────────────────────
    exports.functions.insert(
        "list_iter".to_string(),
        pub_fn(vec![list.clone()], iterator.clone()),
    );
    exports.functions.insert(
        "set_iter".to_string(),
        pub_fn(
            vec![Type::Struct {
                name: "Set".to_string(),
            }],
            iterator.clone(),
        ),
    );
    exports.functions.insert(
        "map_iter".to_string(),
        pub_fn(
            vec![Type::Struct {
                name: "Map".to_string(),
            }],
            iterator.clone(),
        ),
    );
    exports.functions.insert(
        "iterator_next".to_string(),
        pub_fn(
            vec![iterator.clone()],
            Type::Enum {
                name: "Option".to_string(),
            },
        ),
    );
    exports.functions.insert(
        "iterator_remaining".to_string(),
        pub_fn(vec![iterator.clone()], Type::Int),
    );
    exports
        .functions
        .insert("iterator_free".to_string(), pub_fn(vec![iterator], Type::Unit));

    // type aliases
    exports.types.insert(
        "List".to_string(),
        ExportedType {
            members: vec!["new".to_string(), "push".to_string(), "len".to_string()],
            visibility: ExportVisibility::Public,
            is_enum: false,
            struct_fields: None,
            enum_variants: None,
            enum_struct_variants: None,
        },
    );
    exports.types.insert(
        "Map".to_string(),
        ExportedType {
            members: vec!["new".to_string(), "set".to_string(), "get".to_string()],
            visibility: ExportVisibility::Public,
            is_enum: false,
            struct_fields: None,
            enum_variants: None,
            enum_struct_variants: None,
        },
    );
    for name in ["Set", "Iterator"] {
        exports.types.insert(
            name.to_string(),
            ExportedType {
                members: vec!["new".to_string(), "len".to_string()],
                visibility: ExportVisibility::Public,
                is_enum: false,
                struct_fields: None,
                enum_variants: None,
                enum_struct_variants: None,
            },
        );
    }

    exports
}

fn make_std_tensor() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "tensor".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    let int = Type::Int;
    let float = Type::Float;
    let tensor_float_rank1 = Type::Tensor {
        dtype: Box::new(Type::Float),
        rank: Some(1),
        dims: None,
        layout: None,
        device: None,
    };
    let tensor_float_rank2 = Type::Tensor {
        dtype: Box::new(Type::Float),
        rank: Some(2),
        dims: None,
        layout: None,
        device: None,
    };
    let tensor_float_rank0 = Type::Tensor {
        dtype: Box::new(Type::Float),
        rank: Some(0),
        dims: None,
        layout: None,
        device: None,
    };
    let tensor_float_dynamic = Type::Tensor {
        dtype: Box::new(Type::Float),
        rank: None,
        dims: None,
        layout: None,
        device: None,
    };
    let unit = Type::Unit;
    let bool_ty = Type::Bool;

    let functions = [
        (
            "vector_f",
            vec![int.clone(), float.clone()],
            tensor_float_rank1.clone(),
        ),
        (
            "matrix_f",
            vec![int.clone(), int.clone(), float.clone()],
            tensor_float_rank2.clone(),
        ),
        ("zeros", vec![int.clone()], int.clone()),
        ("ones", vec![int.clone()], int.clone()),
        ("full", vec![int.clone(), int.clone()], int.clone()),
        (
            "full_f",
            vec![int.clone(), float.clone()],
            tensor_float_rank1.clone(),
        ),
        (
            "arange",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        ("zeros2", vec![int.clone(), int.clone()], int.clone()),
        ("ones2", vec![int.clone(), int.clone()], int.clone()),
        (
            "full2",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "full2_f",
            vec![int.clone(), int.clone(), float.clone()],
            tensor_float_rank2.clone(),
        ),
        ("len", vec![int.clone()], int.clone()),
        ("rank", vec![int.clone()], int.clone()),
        ("dim", vec![int.clone(), int.clone()], int.clone()),
        ("rows", vec![int.clone()], int.clone()),
        ("cols", vec![int.clone()], int.clone()),
        ("is_valid", vec![int.clone()], bool_ty.clone()),
        ("get", vec![int.clone(), int.clone()], int.clone()),
        ("get_f", vec![int.clone(), int.clone()], float.clone()),
        (
            "set",
            vec![int.clone(), int.clone(), int.clone()],
            unit.clone(),
        ),
        (
            "set_f",
            vec![int.clone(), int.clone(), float.clone()],
            unit.clone(),
        ),
        (
            "get2",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "get2_f",
            vec![int.clone(), int.clone(), int.clone()],
            float.clone(),
        ),
        (
            "set2",
            vec![int.clone(), int.clone(), int.clone(), int.clone()],
            unit.clone(),
        ),
        (
            "set2_f",
            vec![int.clone(), int.clone(), int.clone(), float.clone()],
            unit.clone(),
        ),
        (
            "reshape",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        ("flatten", vec![int.clone()], int.clone()),
        (
            "permute",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "slice",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        ("concat", vec![int.clone(), int.clone()], int.clone()),
        ("stack", vec![int.clone(), int.clone()], int.clone()),
        (
            "add",
            vec![int.clone(), int.clone()],
            tensor_float_dynamic.clone(),
        ),
        (
            "sub",
            vec![int.clone(), int.clone()],
            tensor_float_dynamic.clone(),
        ),
        (
            "mul",
            vec![int.clone(), int.clone()],
            tensor_float_dynamic.clone(),
        ),
        (
            "div",
            vec![int.clone(), int.clone()],
            tensor_float_dynamic.clone(),
        ),
        ("sum", vec![int.clone()], int.clone()),
        ("sum_f", vec![int.clone()], float.clone()),
        ("sum_t", vec![int.clone()], tensor_float_rank0.clone()),
        ("mean_f", vec![int.clone()], float.clone()),
        ("mean_t", vec![int.clone()], tensor_float_rank0.clone()),
        ("max", vec![int.clone()], int.clone()),
        ("min", vec![int.clone()], int.clone()),
        ("argmax", vec![int.clone()], int.clone()),
        (
            "matmul",
            vec![int.clone(), int.clone()],
            tensor_float_rank2.clone(),
        ),
        (
            "matmul_batched",
            vec![int.clone(), int.clone()],
            int.clone(),
        ),
        ("transpose", vec![int.clone()], tensor_float_rank2.clone()),
        ("dot", vec![int.clone(), int.clone()], int.clone()),
        (
            "dot_t",
            vec![int.clone(), int.clone()],
            tensor_float_rank0.clone(),
        ),
        ("neg", vec![int.clone()], tensor_float_dynamic.clone()),
        ("exp_f", vec![int.clone()], tensor_float_dynamic.clone()),
        ("log_f", vec![int.clone()], tensor_float_dynamic.clone()),
        ("sqrt_f", vec![int.clone()], tensor_float_dynamic.clone()),
        ("relu", vec![int.clone()], tensor_float_dynamic.clone()),
        ("sigmoid_f", vec![int.clone()], tensor_float_dynamic.clone()),
        ("tanh_f", vec![int.clone()], tensor_float_dynamic.clone()),
        ("seed", vec![int.clone()], unit.clone()),
        (
            "uniform",
            vec![int.clone(), int.clone(), int.clone()],
            int.clone(),
        ),
        (
            "uniform_f",
            vec![int.clone(), float.clone(), float.clone()],
            tensor_float_rank1.clone(),
        ),
        (
            "normal_f",
            vec![int.clone(), float.clone(), float.clone()],
            tensor_float_rank1.clone(),
        ),
        (
            "bernoulli",
            vec![int.clone(), float.clone()],
            tensor_float_rank1.clone(),
        ),
        ("categorical", vec![int.clone(), int.clone()], int.clone()),
        ("set_deterministic_mode", vec![int.clone()], int.clone()),
        ("deterministic_mode", vec![], int.clone()),
        ("tolerance_abs", vec![], float.clone()),
        ("tolerance_rel", vec![], float.clone()),
        ("device", vec![int.clone()], int.clone()),
        ("device_available", vec![int.clone()], bool_ty.clone()),
        ("device_status", vec![int.clone()], int.clone()),
        ("to_device", vec![int.clone(), int.clone()], int.clone()),
        ("cpu", vec![int.clone()], int.clone()),
        ("sync", vec![int.clone()], unit.clone()),
        ("precision", vec![int.clone()], int.clone()),
        ("to_precision", vec![int.clone(), int.clone()], int.clone()),
        ("stats_allocations", vec![], int.clone()),
        ("stats_active", vec![], int.clone()),
        ("stats_peak_bytes", vec![], int.clone()),
        ("stats_reused_buffers", vec![], int.clone()),
        ("stats_pool_hits", vec![], int.clone()),
        ("stats_pool_misses", vec![], int.clone()),
        ("stats_active_bytes", vec![], int.clone()),
        ("stats_scratch_reuses", vec![], int.clone()),
        ("kernel_strategy", vec![], int.clone()),
        ("stats_kernel_ops", vec![], int.clone()),
        ("stats_kernel_elements", vec![], int.clone()),
        ("stats_device_transfers", vec![], int.clone()),
        ("stats_gpu_kernel_ops", vec![], int.clone()),
        ("stats_cpu_fallbacks", vec![], int.clone()),
        ("stats_gpu_errors", vec![int.clone()], int.clone()),
        ("stats_device_pool_hits", vec![], int.clone()),
        ("stats_device_pool_misses", vec![], int.clone()),
        ("stats_device_pool_bytes_resident", vec![], int.clone()),
        ("storage_device", vec![int.clone()], int.clone()),
        ("stats_device_resident_tensors", vec![], int.clone()),
        ("stats_gpu_backward_ops", vec![], int.clone()),
        ("stats_graph_nodes", vec![], int.clone()),
        ("stats_lifetime_records", vec![], int.clone()),
        ("stats_released_lifetimes", vec![], int.clone()),
        ("stats_allocation_sites", vec![], int.clone()),
        ("stats_reuse_rate_per_mille", vec![], int.clone()),
        ("memory_report", vec![], Type::String),
        ("reset_stats", vec![], unit.clone()),
        (
            "requires_grad",
            vec![int.clone(), bool_ty.clone()],
            tensor_float_dynamic.clone(),
        ),
        ("diff", vec![tensor_float_rank0.clone()], unit.clone()),
        ("backward", vec![tensor_float_rank0.clone()], unit.clone()),
        ("grad", vec![int.clone()], tensor_float_dynamic.clone()),
        ("zero_grad", vec![int.clone()], unit.clone()),
        ("set_grad_enabled", vec![bool_ty.clone()], unit.clone()),
        ("grad_enabled", vec![], bool_ty.clone()),
        ("free", vec![int.clone()], unit.clone()),
        ("free_all", vec![], int.clone()),
        ("refill", vec![int.clone(), float.clone()], unit.clone()),
    ];

    for (name, params, return_type) in functions {
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, return_type));
    }

    exports.types.insert(
        "Tensor".to_string(),
        ExportedType {
            members: vec![
                "shape".to_string(),
                "dtype".to_string(),
                "device".to_string(),
                "precision".to_string(),
                "layout".to_string(),
            ],
            visibility: ExportVisibility::Public,
            is_enum: false,
            struct_fields: None,
            enum_variants: None,
            enum_struct_variants: None,
        },
    );

    exports
}

