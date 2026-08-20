fn lookup_std_host_group_math_io_error(module: &str, function: &str) -> Option<HostFunctionDescriptor> {
    match (module, function) {
            ("math", "abs") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.abs",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("math", "min") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.min",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("math", "max") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.max",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("math", "clamp") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.clamp",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("math", "sqrt_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.sqrt_f",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("math", "pow_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.pow_f",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("math", "floor_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.floor_f",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("math", "ceil_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.ceil_f",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("math", "round_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.round_f",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("math", "sin_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.sin_f",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("math", "cos_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.cos_f",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("math", "tan_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.tan_f",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("math", "log_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.log_f",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("math", "log2_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.log2_f",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("math", "log10_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.log10_f",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("math", "atan2_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.atan2_f",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("math", "pi") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.pi",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("math", "e_const") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.e_const",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("math", "sign") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.sign",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("math", "abs_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.abs_f",
                return_type: IRType::Float,
                returns_value: true,
            }),
            ("math", "gcd") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.gcd",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("math", "lcm") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.lcm",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("math", "is_nan_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.is_nan_f",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("math", "is_infinite_f") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.math.is_infinite_f",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("io", "print") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.io.print",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("io", "println") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.io.println",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("io", "eprint") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.io.eprint",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("io", "eprintln") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.io.eprintln",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("io", "flush") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.io.flush",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("io", "read_line") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.io.read_line",
                return_type: IRType::String,
                returns_value: true,
            }),
            ("io", "input") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.io.input",
                return_type: IRType::String,
                returns_value: true,
            }),
            ("error", "new") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_ERROR_NEW_BINDING,
                return_type: builtin_error_ir_type(),
                returns_value: true,
            }),
            ("error", "code") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_ERROR_CODE_BINDING,
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("error", "message") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_ERROR_MESSAGE_BINDING,
                return_type: IRType::String,
                returns_value: true,
            }),
            ("error", "operation") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_ERROR_OPERATION_BINDING,
                return_type: IRType::String,
                returns_value: true,
            }),
            ("error", "context") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_ERROR_CONTEXT_BINDING,
                return_type: IRType::String,
                returns_value: true,
            }),
            ("error", "origin") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_ERROR_ORIGIN_BINDING,
                return_type: IRType::String,
                returns_value: true,
            }),
            ("error", "retryable") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_ERROR_RETRYABLE_BINDING,
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("collections", "list_new") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_COLLECTIONS_LIST_NEW_BINDING,
                return_type: IRType::Struct {
                    name: "List_int".to_string(),
                    fields: Vec::new(),
                },
                returns_value: true,
            }),
            ("collections", "list_push") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.list_push",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("collections", "list_len") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.list_len",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("collections", "list_get") => Some(HostFunctionDescriptor {
                runtime_name: spectra_contract::STD_COLLECTIONS_LIST_GET_BINDING,
                return_type: IRType::Unknown,
                returns_value: true,
            }),
            ("collections", "list_set") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.list_set",
                return_type: IRType::Void,
                returns_value: false,
            }),
            ("collections", "list_contains") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.list_contains",
                return_type: IRType::Bool,
                returns_value: true,
            }),
            ("collections", "list_clear") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.list_clear",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("collections", "list_free") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.list_free",
                return_type: IRType::Int,
                returns_value: true,
            }),
            ("collections", "list_free_all") => Some(HostFunctionDescriptor {
                runtime_name: "spectra.std.collections.list_free_all",
                return_type: IRType::Int,
                returns_value: true,
            }),
        _ => None,
    }
}
