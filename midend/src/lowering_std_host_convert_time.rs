use super::*;

pub(crate) fn lookup_std_host_group_convert_time(
    module: &str,
    function: &str,
) -> Option<HostFunctionDescriptor> {
    match (module, function) {
        ("convert", "int_to_string") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.convert.int_to_string",
            return_type: IRType::String,
            returns_value: true,
        }),
        ("convert", "float_to_string") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.convert.float_to_string",
            return_type: IRType::String,
            returns_value: true,
        }),
        ("convert", "bool_to_string") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.convert.bool_to_string",
            return_type: IRType::String,
            returns_value: true,
        }),
        ("convert", "string_to_int") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.convert.string_to_int",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("convert", "string_to_float") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.convert.string_to_float",
            return_type: IRType::Float,
            returns_value: true,
        }),
        ("convert", "int_to_float") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.convert.int_to_float",
            return_type: IRType::Float,
            returns_value: true,
        }),
        ("convert", "float_to_int") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.convert.float_to_int",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("convert", "string_to_int_or") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.convert.string_to_int_or",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("convert", "string_to_float_or") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.convert.string_to_float_or",
            return_type: IRType::Float,
            returns_value: true,
        }),
        ("convert", "string_to_bool") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.convert.string_to_bool",
            return_type: IRType::Bool,
            returns_value: true,
        }),
        ("convert", "bool_to_int") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.convert.bool_to_int",
            return_type: IRType::Int,
            returns_value: true,
        }),
        // ── std.char ──────────────────────────────────────────────────
        ("char", "is_alpha") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.char.is_alpha",
            return_type: IRType::Bool,
            returns_value: true,
        }),
        ("char", "is_digit_char") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.char.is_digit_char",
            return_type: IRType::Bool,
            returns_value: true,
        }),
        ("char", "is_whitespace_char") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.char.is_whitespace_char",
            return_type: IRType::Bool,
            returns_value: true,
        }),
        ("char", "is_upper_char") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.char.is_upper_char",
            return_type: IRType::Bool,
            returns_value: true,
        }),
        ("char", "is_lower_char") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.char.is_lower_char",
            return_type: IRType::Bool,
            returns_value: true,
        }),
        ("char", "to_upper_char") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.char.to_upper_char",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("char", "to_lower_char") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.char.to_lower_char",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("char", "is_alphanumeric") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.char.is_alphanumeric",
            return_type: IRType::Bool,
            returns_value: true,
        }),
        // ── std.range ─────────────────────────────────────────────────
        ("range", "create") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.range.create",
            return_type: IRType::Range,
            returns_value: true,
        }),
        ("range", "len") => Some(host_int("spectra.std.range.len")),
        ("range", "at") => Some(host_int("spectra.std.range.at")),
        ("range", "eq") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.range.eq",
            return_type: IRType::Bool,
            returns_value: true,
        }),
        ("range", "start") => Some(host_int("spectra.std.range.start")),
        ("range", "end") => Some(host_int("spectra.std.range.end")),
        ("range", "is_inclusive") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.range.is_inclusive",
            return_type: IRType::Bool,
            returns_value: true,
        }),
        ("range", "iter") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.range.iter",
            return_type: IRType::Struct {
                name: "Iterator_int".to_string(),
                fields: Vec::new(),
            },
            returns_value: true,
        }),
        // ── std.time ──────────────────────────────────────────────────
        ("time", "time_now_millis") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.time_now_millis",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "time_now_secs") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.time_now_secs",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "sleep_ms") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.sleep_ms",
            return_type: IRType::Void,
            returns_value: false,
        }),
        ("time", "monotonic_millis") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.monotonic_millis",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "monotonic_nanos") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.monotonic_nanos",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "duration_ms") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.duration_ms",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "duration_secs") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.duration_secs",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "duration_millis") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.duration_millis",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "duration_secs_value") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.duration_secs_value",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "duration_add") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.duration_add",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "duration_sub") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.duration_sub",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "instant_now") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.instant_now",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "instant_elapsed_ms") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.instant_elapsed_ms",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "instant_add") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.instant_add",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "instant_has_elapsed") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.instant_has_elapsed",
            return_type: IRType::Bool,
            returns_value: true,
        }),
        ("time", "sleep") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.sleep",
            return_type: IRType::Void,
            returns_value: false,
        }),
        ("time", "unix_to_utc") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.unix_to_utc",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "utc_year") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.utc_year",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "utc_month") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.utc_month",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "utc_day") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.utc_day",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "utc_hour") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.utc_hour",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "utc_minute") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.utc_minute",
            return_type: IRType::Int,
            returns_value: true,
        }),
        ("time", "utc_second") => Some(HostFunctionDescriptor {
            runtime_name: "spectra.std.time.utc_second",
            return_type: IRType::Int,
            returns_value: true,
        }),
        // ── std.random ────────────────────────────────────────────────
        _ => None,
    }
}
