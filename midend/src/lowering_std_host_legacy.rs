fn lookup_std_host_group_legacy(module: &str, function: &str) -> Option<HostFunctionDescriptor> {
    match (module, function) {
            ("random", "random_seed") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.random.random_seed",
                return_type: IRType::Void,
                returns_value: false,
            }),
            ("random", "random_int") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.random.random_int",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("random", "random_float") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.random.random_float",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("random", "random_bool") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.random.random_bool",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            // ── std.collections extras ────────────────────────────────────
            ("collections", "list_pop") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_COLLECTIONS_LIST_POP_BINDING,
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "list_pop_front") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_COLLECTIONS_LIST_POP_FRONT_BINDING,
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "list_get_option") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_COLLECTIONS_LIST_GET_OPTION_BINDING,
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "list_pop_option") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_COLLECTIONS_LIST_POP_OPTION_BINDING,
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "list_pop_front_option") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_COLLECTIONS_LIST_POP_FRONT_OPTION_BINDING,
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "list_insert_at") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.list_insert_at",
                return_type: IRType::Void,
                returns_value: false,
            }),
            ("collections", "list_remove_at") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_COLLECTIONS_LIST_REMOVE_AT_BINDING,
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "list_remove_at_option") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_COLLECTIONS_LIST_REMOVE_AT_OPTION_BINDING,
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "list_index_of") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.list_index_of",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("collections", "list_sort") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.list_sort",
                return_type: IRType::Void,
                returns_value: false,
            }),
            ("collections", "list_map") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.list_map",
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "list_filter") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.list_filter",
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "list_reduce") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.list_reduce",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("collections", "list_sort_by") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.list_sort_by",
                return_type: IRType::Void,
                returns_value: false,
            }),
            // ── std.fs ────────────────────────────────────────────────────
            ("fs", "fs_read") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_FS_FS_READ_BINDING,
                return_type: builtin_result_ir_type(IRType::String),
                returns_value: true,
            }),
            ("fs", "fs_write") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_FS_FS_WRITE_BINDING,
                return_type: builtin_result_ir_type(IRType::Bool),
                returns_value: true,
            }),
            ("fs", "fs_append") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_FS_FS_APPEND_BINDING,
                return_type: builtin_result_ir_type(IRType::Bool),
                returns_value: true,
            }),
            ("fs", "fs_exists") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_FS_FS_EXISTS_BINDING,
                return_type: builtin_result_ir_type(IRType::Bool),
                returns_value: true,
            }),
            ("fs", "fs_remove") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_FS_FS_REMOVE_BINDING,
                return_type: builtin_result_ir_type(IRType::Bool),
                returns_value: true,
            }),
            ("fs", "create_dir_all") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_FS_CREATE_DIR_ALL_BINDING,
                return_type: builtin_result_ir_type(IRType::Bool),
                returns_value: true,
            }),
            ("fs", "remove_dir") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_FS_REMOVE_DIR_BINDING,
                return_type: builtin_result_ir_type(IRType::Bool),
                returns_value: true,
            }),
            ("fs", "rename") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_FS_RENAME_BINDING,
                return_type: builtin_result_ir_type(IRType::Bool),
                returns_value: true,
            }),
            ("fs", "copy") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_FS_COPY_BINDING,
                return_type: builtin_result_ir_type(IRType::Int),
                returns_value: true,
            }),
            ("fs", "read_dir") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_FS_READ_DIR_BINDING,
                return_type: builtin_result_ir_type(IRType::Struct {
                    name: "List_string".to_string(),
                    fields: Vec::new(),
                }),
                returns_value: true,
            }),
            // ── std.env ───────────────────────────────────────────────────
            ("env", "env_get") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_ENV_ENV_GET_BINDING,
                return_type: IRType::Enum {
                    name: "Option_string".to_string(),
                    variants: vec![
                        ("Some".to_string(), Some(vec![IRType::String])),
                        ("None".to_string(), None),
                    ],
                },
                returns_value: true,
            }),
            ("env", "env_get_option") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_ENV_ENV_GET_OPTION_BINDING,
                return_type: IRType::Enum {
                    name: "Option_string".to_string(),
                    variants: vec![
                        ("Some".to_string(), Some(vec![IRType::String])),
                        ("None".to_string(), None),
                    ],
                },
                returns_value: true,
            }),
            ("env", "env_set") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_ENV_ENV_SET_BINDING,
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("env", "env_args_count") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_ENV_ENV_ARGS_COUNT_BINDING,
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("env", "env_arg") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_ENV_ENV_ARG_BINDING,
                return_type: IRType::Enum {
                    name: "Option_string".to_string(),
                    variants: vec![
                        ("Some".to_string(), Some(vec![IRType::String])),
                        ("None".to_string(), None),
                    ],
                },
                returns_value: true,
            }),
            ("env", "env_arg_option") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_ENV_ENV_ARG_OPTION_BINDING,
                return_type: IRType::Enum {
                    name: "Option_string".to_string(),
                    variants: vec![
                        ("Some".to_string(), Some(vec![IRType::String])),
                        ("None".to_string(), None),
                    ],
                },
                returns_value: true,
            }),
            // ── std.option ────────────────────────────────────────────────
            ("option", "is_some") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.option.is_some",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("option", "is_none") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.option.is_none",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("option", "option_unwrap") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.option.option_unwrap",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("option", "option_unwrap_or") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.option.option_unwrap_or",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("option", "option_map") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.option.option_map",
                return_type: IRType::Int,
                returns_value: true,
            }),
            // ── std.result ────────────────────────────────────────────────
            ("result", "is_ok") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.result.is_ok",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("result", "is_err") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.result.is_err",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("result", "result_unwrap") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.result.result_unwrap",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("result", "result_unwrap_or") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.result.result_unwrap_or",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("result", "result_unwrap_err") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.result.result_unwrap_err",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("result", "result_map") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.result.result_map",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("result", "result_map_err") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.result.result_map_err",
                return_type: IRType::Int,
                returns_value: true,
            }),
            _ => None,
    }
}
