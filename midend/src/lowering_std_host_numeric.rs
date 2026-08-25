use super::*;

pub(crate) fn lookup_std_host_module_function(module: &str, function: &str) -> Option<HostFunctionDescriptor> {
            if module == "numeric" {
                if function == "checked_f32" {
                    return Some(HostFunctionDescriptor {
                        runtime_name: "spectra.std.numeric.checked_f32",
                        return_type: IRType::ExactFloat { width: IRFloatWidth::F32 },
                        returns_value: true,
                    });
                }
                let (signed, width): (bool, IRIntWidth) = match function {
                    "wrapping_add_i8" | "wrapping_sub_i8" | "wrapping_mul_i8" => (true, IRIntWidth::I8),
                    "wrapping_add_i16" | "wrapping_sub_i16" | "wrapping_mul_i16" => (true, IRIntWidth::I16),
                    "wrapping_add_i32" | "wrapping_sub_i32" | "wrapping_mul_i32" => (true, IRIntWidth::I32),
                    "wrapping_add_i64" | "wrapping_sub_i64" | "wrapping_mul_i64" => (true, IRIntWidth::I64),
                    "wrapping_add_u8" | "wrapping_sub_u8" | "wrapping_mul_u8" => (false, IRIntWidth::I8),
                    "wrapping_add_u16" | "wrapping_sub_u16" | "wrapping_mul_u16" => (false, IRIntWidth::I16),
                    "wrapping_add_u32" | "wrapping_sub_u32" | "wrapping_mul_u32" => (false, IRIntWidth::I32),
                    "wrapping_add_u64" | "wrapping_sub_u64" | "wrapping_mul_u64" => (false, IRIntWidth::I64),
                    _ => return None,
                };
                return Some(HostFunctionDescriptor {
                    runtime_name: Box::leak(format!("spectra.std.numeric.{function}").into_boxed_str()),
                    return_type: IRType::ExactInt { signed, width },
                    returns_value: true,
                });
            }
    lookup_std_host_group_math_io_error(module, function)
        .or_else(|| lookup_std_host_group_tensor_ml(module, function))
        .or_else(|| lookup_std_host_group_collections_string(module, function))
        .or_else(|| lookup_std_host_group_convert_time(module, function))
        .or_else(|| lookup_std_host_group_legacy(module, function))
}
