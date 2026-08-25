fn make_std_string() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "string".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    // len(s: string) -> int — number of characters (bytes for ASCII content)
    exports
        .functions
        .insert("len".to_string(), pub_fn(vec![Type::String], Type::Int));
    // contains(s: string, sub: string) -> bool
    exports.functions.insert(
        "contains".to_string(),
        pub_fn(vec![Type::String, Type::String], Type::Bool),
    );
    // to_upper(s: string) -> string
    exports.functions.insert(
        "to_upper".to_string(),
        pub_fn(vec![Type::String], Type::String),
    );
    // to_lower(s: string) -> string
    exports.functions.insert(
        "to_lower".to_string(),
        pub_fn(vec![Type::String], Type::String),
    );
    // trim(s: string) -> string
    exports
        .functions
        .insert("trim".to_string(), pub_fn(vec![Type::String], Type::String));
    // starts_with(s: string, prefix: string) -> bool
    exports.functions.insert(
        "starts_with".to_string(),
        pub_fn(vec![Type::String, Type::String], Type::Bool),
    );
    // ends_with(s: string, suffix: string) -> bool
    exports.functions.insert(
        "ends_with".to_string(),
        pub_fn(vec![Type::String, Type::String], Type::Bool),
    );
    // eq(a: string, b: string) -> bool
    exports.functions.insert(
        "eq".to_string(),
        pub_fn(vec![Type::String, Type::String], Type::Bool),
    );
    // concat(a: string, b: string) -> string
    exports.functions.insert(
        "concat".to_string(),
        pub_fn(vec![Type::String, Type::String], Type::String),
    );
    // repeat_str(s: string, n: int) -> string
    exports.functions.insert(
        "repeat_str".to_string(),
        pub_fn(vec![Type::String, Type::Int], Type::String),
    );
    // String builder API (R-3108). Avoids the per-call allocation cost of
    // `concat` by accumulating parts in a handle and joining them in a
    // single allocation on `builder_finish`. The constructor takes a
    // capacity hint (in bytes) so it is not parsed as a no-arg method
    // call by the parser.
    exports.functions.insert(
        "builder_new".to_string(),
        pub_fn(vec![Type::Int], Type::Int),
    );
    exports.functions.insert(
        "builder_push".to_string(),
        pub_fn(vec![Type::Int, Type::String], Type::Unit),
    );
    exports.functions.insert(
        "builder_len".to_string(),
        pub_fn(vec![Type::Int], Type::Int),
    );
    exports.functions.insert(
        "builder_finish".to_string(),
        pub_fn(vec![Type::Int], Type::String),
    );
    exports.functions.insert(
        "builder_free".to_string(),
        pub_fn(vec![Type::Int], Type::Unit),
    );
    // concat_n(list: int, count: int) -> string
    // Concatenates the first `count` string elements of a std.collections
    // list (each stored as a string handle) into a single fresh allocation.
    // Low-level building block for R-3108; the user-facing string builder
    // API is added by a follow-up.
    // Currently not exposed at the language level because list_push takes
    // int and the encoding of a string handle is not user-facing. This entry
    // is left commented until the builder API lands.
    // exports.functions.insert(
    //     "concat_n".to_string(),
    //     pub_fn(vec![Type::Int, Type::Int], Type::String),
    // );
    // char_at(s: string, index: int) -> int  (returns char code; -1 if out of bounds)
    exports.functions.insert(
        "char_at".to_string(),
        pub_fn(vec![Type::String, Type::Int], Type::Int),
    );
    // substring(s: string, start: int, end: int) -> string
    exports.functions.insert(
        "substring".to_string(),
        pub_fn(vec![Type::String, Type::Int, Type::Int], Type::String),
    );
    // replace(s: string, from: string, to: string) -> string
    exports.functions.insert(
        "replace".to_string(),
        pub_fn(vec![Type::String, Type::String, Type::String], Type::String),
    );
    // index_of(s: string, sub: string) -> int  (-1 if not found)
    exports.functions.insert(
        "index_of".to_string(),
        pub_fn(vec![Type::String, Type::String], Type::Int),
    );
    // split_first(s: string, sep: string) -> string
    exports.functions.insert(
        "split_first".to_string(),
        pub_fn(vec![Type::String, Type::String], Type::String),
    );
    // split_last(s: string, sep: string) -> string
    exports.functions.insert(
        "split_last".to_string(),
        pub_fn(vec![Type::String, Type::String], Type::String),
    );
    // is_empty(s: string) -> bool
    exports.functions.insert(
        "is_empty".to_string(),
        pub_fn(vec![Type::String], Type::Bool),
    );
    // count_occurrences(s: string, sub: string) -> int
    exports.functions.insert(
        "count_occurrences".to_string(),
        pub_fn(vec![Type::String, Type::String], Type::Int),
    );
    // split_by(s: string, sep: string) -> List<string>
    exports.functions.insert(
        "split_by".to_string(),
        pub_fn(
            vec![Type::String, Type::String],
            Type::Struct {
                name: "List_string".to_string(),
            },
        ),
    );
    // pad_left(s: string, width: int, pad_char: int) -> string
    exports.functions.insert(
        "pad_left".to_string(),
        pub_fn(vec![Type::String, Type::Int, Type::Int], Type::String),
    );
    // pad_right(s: string, width: int, pad_char: int) -> string
    exports.functions.insert(
        "pad_right".to_string(),
        pub_fn(vec![Type::String, Type::Int, Type::Int], Type::String),
    );
    // reverse_str(s: string) -> string
    exports.functions.insert(
        "reverse_str".to_string(),
        pub_fn(vec![Type::String], Type::String),
    );

    exports
}

fn make_std_convert() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "convert".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    // to_string(val: int) -> string  (also accepts float via Unknown type)
    exports.functions.insert(
        "int_to_string".to_string(),
        pub_fn(vec![Type::Int], Type::String),
    );
    // float_to_string(val: float) -> string
    exports.functions.insert(
        "float_to_string".to_string(),
        pub_fn(vec![Type::Float], Type::String),
    );
    // bool_to_string(val: bool) -> string
    exports.functions.insert(
        "bool_to_string".to_string(),
        pub_fn(vec![Type::Bool], Type::String),
    );
    // string_to_int(s: string) -> int  (returns 0 on parse error)
    exports.functions.insert(
        "string_to_int".to_string(),
        pub_fn(vec![Type::String], Type::Int),
    );
    // string_to_float(s: string) -> float  (returns 0.0 on parse error)
    exports.functions.insert(
        "string_to_float".to_string(),
        pub_fn(vec![Type::String], Type::Float),
    );
    // int_to_float(val: int) -> float
    exports.functions.insert(
        "int_to_float".to_string(),
        pub_fn(vec![Type::Int], Type::Float),
    );
    // float_to_int(val: float) -> int  (truncates)
    exports.functions.insert(
        "float_to_int".to_string(),
        pub_fn(vec![Type::Float], Type::Int),
    );
    // string_to_int_or(s: string, default: int) -> int
    exports.functions.insert(
        "string_to_int_or".to_string(),
        pub_fn(vec![Type::String, Type::Int], Type::Int),
    );
    // string_to_float_or(s: string, default: float) -> float
    exports.functions.insert(
        "string_to_float_or".to_string(),
        pub_fn(vec![Type::String, Type::Float], Type::Float),
    );
    // string_to_bool(s: string) -> bool
    exports.functions.insert(
        "string_to_bool".to_string(),
        pub_fn(vec![Type::String], Type::Bool),
    );
    // bool_to_int(b: bool) -> int
    exports.functions.insert(
        "bool_to_int".to_string(),
        pub_fn(vec![Type::Bool], Type::Int),
    );

    exports
}

fn make_std_random() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "random".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    // random_seed(seed: int) -> unit
    exports.functions.insert(
        "random_seed".to_string(),
        pub_fn(vec![Type::Int], Type::Unit),
    );
    // random_int(min: int, max: int) -> int
    exports.functions.insert(
        "random_int".to_string(),
        pub_fn(vec![Type::Int, Type::Int], Type::Int),
    );
    // random_float() -> float  ([0.0, 1.0))
    exports
        .functions
        .insert("random_float".to_string(), pub_fn(vec![], Type::Float));
    // random_bool() -> bool
    exports
        .functions
        .insert("random_bool".to_string(), pub_fn(vec![], Type::Bool));

    exports
}

fn make_std_fs() -> ModuleExports {
    let mut exports = make_std_fs_legacy();
    exports.stdlib_path = Some(vec!["std".to_string(), "fs".to_string()]);

    let result_string_error = Type::Enum {
        name: "Result_string_Error".to_string(),
    };
    let result_bool_error = Type::Enum {
        name: "Result_bool_Error".to_string(),
    };
    let result_int_error = Type::Enum {
        name: "Result_int_Error".to_string(),
    };
    exports
        .functions
        .insert("fs_read".to_string(), pub_fn(vec![Type::String], result_string_error));
    for name in ["fs_write", "fs_append", "fs_exists", "fs_remove"] {
        let params = if matches!(name, "fs_write" | "fs_append") {
            vec![Type::String, Type::String]
        } else {
            vec![Type::String]
        };
        exports
            .functions
            .insert(name.to_string(), pub_fn(params, result_bool_error.clone()));
    }
    // Directory-management surface mirroring the fs_read Result contract.
    exports.functions.insert(
        "create_dir_all".to_string(),
        pub_fn(vec![Type::String], result_bool_error.clone()),
    );
    exports.functions.insert(
        "remove_dir".to_string(),
        pub_fn(vec![Type::String], result_bool_error.clone()),
    );
    exports.functions.insert(
        "rename".to_string(),
        pub_fn(
            vec![Type::String, Type::String],
            result_bool_error.clone(),
        ),
    );
    // copy(from, to) -> Result<int, Error> (bytes copied)
    exports.functions.insert(
        "copy".to_string(),
        pub_fn(vec![Type::String, Type::String], result_int_error),
    );
    // read_dir(path) -> Result<List<string>, Error> (entry names, sorted).
    // The structural `Result<...>` application keeps the `Ok` payload a real
    // `List<string>` instead of an unparseable mangled enum name.
    let list_of_string = Type::Applied {
        name: "List".to_string(),
        args: vec![Type::String],
    };
    let std_error = Type::Struct {
        name: "Error".to_string(),
    };
    exports.functions.insert(
        "read_dir".to_string(),
        pub_fn(
            vec![Type::String],
            Type::Applied {
                name: "Result".to_string(),
                args: vec![list_of_string, std_error],
            },
        ),
    );

    exports
}

/// Compatibility-only filesystem surface retaining the historic string and
/// boolean return values.  New code must import `std.fs` and handle
/// `Result<_, Error>` explicitly.
fn make_std_compat_fs() -> ModuleExports {
    let mut exports = make_std_fs_legacy();
    exports.stdlib_path = Some(vec![
        "std".to_string(),
        "compat".to_string(),
        "fs".to_string(),
    ]);
    exports
}

fn make_std_fs_legacy() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "fs".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    // fs_read(path: string) returns string  (reads entire file; returns "" on error)
    exports.functions.insert(
        "fs_read".to_string(),
        pub_fn(vec![Type::String], Type::String),
    );
    // fs_write(path: string, content: string) -> bool
    // Creates missing parent directories when possible; returns false on controlled filesystem failures.
    exports.functions.insert(
        "fs_write".to_string(),
        pub_fn(vec![Type::String, Type::String], Type::Bool),
    );
    // fs_append(path: string, content: string) -> bool
    // Creates missing parent directories when possible; returns false on controlled filesystem failures.
    exports.functions.insert(
        "fs_append".to_string(),
        pub_fn(vec![Type::String, Type::String], Type::Bool),
    );
    // fs_exists(path: string) -> bool
    exports.functions.insert(
        "fs_exists".to_string(),
        pub_fn(vec![Type::String], Type::Bool),
    );
    // fs_remove(path: string) -> bool
    exports.functions.insert(
        "fs_remove".to_string(),
        pub_fn(vec![Type::String], Type::Bool),
    );

    exports
}

/// Structured runtime failure values shared by the stable I/O surface.
///
/// `ErrorCode` is a closed unit enum so callers cannot silently invent an
/// incompatible numeric code.  `Error` is a concrete record because the
/// runtime materializes it as an opaque pointer with these six word-sized
/// fields; accessors are also exported for code that prefers not to use field
/// syntax.
fn make_std_error() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "error".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    let error_codes = [
        "InvalidArgument",
        "NotFound",
        "PermissionDenied",
        "Io",
        "Internal",
        "Unsupported",
    ];
    let enum_variants = error_codes
        .iter()
        .map(|name| ((*name).to_string(), None))
        .collect::<HashMap<_, _>>();
    exports.types.insert(
        "ErrorCode".to_string(),
        ExportedType {
            members: error_codes.iter().map(|name| (*name).to_string()).collect(),
            visibility: ExportVisibility::Public,
            is_enum: true,
            struct_fields: None,
            enum_variants: Some(enum_variants),
            enum_struct_variants: None,
        },
    );

    let field_types = [
        ("code", "int"),
        ("message", "string"),
        ("operation", "string"),
        ("context", "string"),
        ("origin", "string"),
        ("retryable", "bool"),
    ];
    let struct_fields = field_types
        .iter()
        .map(|(name, ty)| ((*name).to_string(), builtin_type_annotation(ty)))
        .collect::<HashMap<_, _>>();
    exports.types.insert(
        "Error".to_string(),
        ExportedType {
            members: field_types
                .iter()
                .map(|(name, _)| (*name).to_string())
                .collect(),
            visibility: ExportVisibility::Public,
            is_enum: false,
            struct_fields: Some(struct_fields),
            enum_variants: None,
            enum_struct_variants: None,
        },
    );

    let error_code = Type::Enum {
        name: "ErrorCode".to_string(),
    };
    let error = Type::Struct {
        name: "Error".to_string(),
    };
    exports.functions.insert(
        "new".to_string(),
        pub_fn(
            vec![
                error_code,
                Type::String,
                Type::String,
                Type::String,
                Type::String,
                Type::Bool,
            ],
            error.clone(),
        ),
    );
    for (name, return_type) in [
        ("code", Type::Int),
        ("message", Type::String),
        ("operation", Type::String),
        ("context", Type::String),
        ("origin", Type::String),
        ("retryable", Type::Bool),
    ] {
        exports
            .functions
            .insert(name.to_string(), pub_fn(vec![error.clone()], return_type));
    }

    exports
}

/// Public environment surface. Missing variables and out-of-range arguments
/// are represented by `Option<string>`; sentinel strings remain available only
/// through the explicit `std.compat.env` namespace.
fn make_std_env() -> ModuleExports {
    let mut exports = make_std_env_legacy();
    exports.stdlib_path = Some(vec!["std".to_string(), "env".to_string()]);

    let option_string = Type::Enum {
        name: "Option_string".to_string(),
    };
    exports
        .functions
        .insert("env_get".to_string(), pub_fn(vec![Type::String], option_string.clone()));
    exports.functions.insert(
        "env_get_option".to_string(),
        pub_fn(vec![Type::String], option_string.clone()),
    );
    exports
        .functions
        .insert("env_arg".to_string(), pub_fn(vec![Type::Int], option_string.clone()));
    exports.functions.insert(
        "env_arg_option".to_string(),
        pub_fn(vec![Type::Int], option_string),
    );

    exports
}

/// Compatibility-only environment surface retaining the historic empty-string
/// sentinel behavior for callers that have not migrated yet.
fn make_std_compat_env() -> ModuleExports {
    let mut exports = make_std_env_legacy();
    exports
        .functions
        .retain(|name, _| matches!(name.as_str(), "env_get" | "env_arg"));
    exports.stdlib_path = Some(vec![
        "std".to_string(),
        "compat".to_string(),
        "env".to_string(),
    ]);
    exports
}

fn make_std_env_legacy() -> ModuleExports {
    let mut exports = ModuleExports {
        stdlib_path: Some(vec!["std".to_string(), "env".to_string()]),
        package_name: Some("std".to_string()),
        ..Default::default()
    };

    // env_get(key: string) returns string  (returns "" if not set)
    exports.functions.insert(
        "env_get".to_string(),
        pub_fn(vec![Type::String], Type::String),
    );
    exports.functions.insert(
        "env_get_option".to_string(),
        pub_fn(
            vec![Type::String],
            Type::Enum {
                name: "Option".to_string(),
            },
        ),
    );
    // env_set(key: string, value: string) -> bool
    exports.functions.insert(
        "env_set".to_string(),
        pub_fn(vec![Type::String, Type::String], Type::Bool),
    );
    // env_args_count() -> int
    exports
        .functions
        .insert("env_args_count".to_string(), pub_fn(vec![], Type::Int));
    // env_arg(index: int) returns string  (returns "" if out of bounds)
    exports
        .functions
        .insert("env_arg".to_string(), pub_fn(vec![Type::Int], Type::String));
    exports.functions.insert(
        "env_arg_option".to_string(),
        pub_fn(
            vec![Type::Int],
            Type::Enum {
                name: "Option".to_string(),
            },
        ),
    );

    exports
}

