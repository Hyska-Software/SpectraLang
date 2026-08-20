fn lookup_std_host_group_collections_string(module: &str, function: &str) -> Option<HostFunctionDescriptor> {
    match (module, function) {
            // ── std.collections map ──────────────────────────────────────
            ("collections", "map_new") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.map_new",
                return_type: IRType::Struct {
                    name: "Map_int_int".to_string(),
                    fields: Vec::new(),
                },
                returns_value: true,
            }),
            ("collections", "map_set") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.map_set",
                return_type: IRType::Int,
                returns_value: false,
            }),
            ("collections", "map_get") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_COLLECTIONS_MAP_GET_BINDING,
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "map_get_option") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_COLLECTIONS_MAP_GET_OPTION_BINDING,
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "map_contains") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.map_contains",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("collections", "map_remove") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_COLLECTIONS_MAP_REMOVE_BINDING,
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "map_remove_option") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_COLLECTIONS_MAP_REMOVE_OPTION_BINDING,
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "map_len") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.map_len",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("collections", "map_clear") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.map_clear",
                return_type: IRType::Int,
                returns_value: false,
            }),
            ("collections", "map_free") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.map_free",
                return_type: IRType::Void,
                returns_value: false,
            }),
            // ── std.collections set/iterator ──────────────────────────────
            ("collections", "set_new") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.set_new",
                return_type: IRType::Struct {
                    name: "Set_int".to_string(),
                    fields: Vec::new(),
                },
                returns_value: true,
            }),
            ("collections", "set_insert") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.set_insert",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("collections", "set_contains") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.set_contains",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("collections", "set_remove") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.set_remove",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("collections", "set_len") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.set_len",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("collections", "set_get") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.set_get",
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "set_clear") => Some(host_void("spectra.std.collections.set_clear")),
            ("collections", "set_free") => Some(host_void("spectra.std.collections.set_free")),
            ("collections", "list_iter") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.list_iter",
                return_type: IRType::Struct {
                    name: "Iterator_int".to_string(),
                    fields: Vec::new(),
                },
                returns_value: true,
            }),
            ("collections", "set_iter") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.set_iter",
                return_type: IRType::Struct {
                    name: "Iterator_int".to_string(),
                    fields: Vec::new(),
                },
                returns_value: true,
            }),
            ("collections", "map_iter") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.map_iter",
                return_type: IRType::Struct {
                    name: "Iterator_int".to_string(),
                    fields: Vec::new(),
                },
                returns_value: true,
            }),
            ("collections", "iterator_next") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.iterator_next",
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "iterator_remaining") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.iterator_remaining",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("collections", "iterator_free") => Some(host_void("spectra.std.collections.iterator_free")),
            // ── std.string ────────────────────────────────────────────────
            ("string", "len") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.len",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("string", "contains") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.contains",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("string", "to_upper") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.to_upper",
                return_type: IRType::String,
                returns_value: true,
            }),
            ("string", "to_lower") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.to_lower",
                return_type: IRType::String,
                returns_value: true,
            }),
            ("string", "trim") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.trim",
                return_type: IRType::String,
                returns_value: true,
            }),
            ("string", "starts_with") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.starts_with",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("string", "ends_with") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.ends_with",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("string", "eq") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.eq",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("string", "concat") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.concat",
                return_type: IRType::String,
                returns_value: true,
            }),
            ("string", "repeat_str") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.repeat_str",
                return_type: IRType::String,
                returns_value: true,
            }),
            ("string", "char_at") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.char_at",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("string", "substring") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.substring",
                return_type: IRType::String,
                returns_value: true,
            }),
            ("string", "replace") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.replace",
                return_type: IRType::String,
                returns_value: true,
            }),
            ("string", "index_of") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.index_of",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("string", "split_first") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.split_first",
                return_type: IRType::String,
                returns_value: true,
            }),
            ("string", "split_last") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.split_last",
                return_type: IRType::String,
                returns_value: true,
            }),
            ("string", "is_empty") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.is_empty",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("string", "count_occurrences") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.count_occurrences",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("string", "reverse_str") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.reverse_str",
                return_type: IRType::String,
                returns_value: true,
            }),
            ("string", "pad_left") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.pad_left",
                return_type: IRType::String,
                returns_value: true,
            }),
            ("string", "pad_right") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.pad_right",
                return_type: IRType::String,
                returns_value: true,
            }),
            // ── std.string string builder (R-3108) ────────────────────────
            ("string", "builder_new") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.builder_new",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("string", "builder_push") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.builder_push",
                return_type: IRType::Void,
                returns_value: false,
            }),
            ("string", "builder_len") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.builder_len",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("string", "builder_finish") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.builder_finish",
                return_type: IRType::String,
                returns_value: true,
            }),
            ("string", "builder_free") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.builder_free",
                return_type: IRType::Void,
                returns_value: false,
            }),
            ("string", "split_by") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.string.split_by",
                return_type: IRType::Struct {
                    name: "List_string".to_string(),
                    fields: Vec::new(),
                },
                returns_value: true,
            }),
            // ── std.convert ───────────────────────────────────────────────
        _ => None,
    }
}
