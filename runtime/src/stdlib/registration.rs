/// Registers the standard library host functions.
pub fn register() {
    register_math();
    register_numeric();
    register_io();
    register_collections();
    register_set();
    register_iterator();
    register_map();
    register_string();
    register_convert();
    register_random();
    register_fs();
    register_env();
    error::register();
    option_result::register();
    register_char();
    register_time();
    register_range();
    register_tensor();
    register_ml();
    register_concurrent();
    register_async();
    register_serve();
    register_serve_real(); // ── ServeReal ──
    register_ml_text_embedding_model(); // StatsEmbed
    // Anchor the fast-path extern "C" symbols so the JIT symbol resolver
    // can find them at runtime. See `ffi::keep_fast_symbols` for details.
    crate::ffi::keep_fast_symbols();
}

fn numeric_binary_args(ctx: *mut SpectraHostCallContext) -> Option<([i64; 2], *mut i64)> {
    if ctx.is_null() {
        return None;
    }
    unsafe {
        let c = &mut *ctx;
        if c.arg_len != 2 || c.result_len == 0 || c.args.is_null() || c.results.is_null() {
            return None;
        }
        let args = slice::from_raw_parts(c.args, 2);
        Some(([args[0], args[1]], c.results))
    }
}

fn numeric_unary_arg(ctx: *mut SpectraHostCallContext) -> Option<(i64, *mut i64)> {
    if ctx.is_null() {
        return None;
    }
    unsafe {
        let c = &mut *ctx;
        if c.arg_len != 1 || c.result_len == 0 || c.args.is_null() || c.results.is_null() {
            return None;
        }
        Some((*c.args, c.results))
    }
}

macro_rules! define_numeric_host {
    ($fn_name:ident, $helper:path, $bits:expr) => {
        extern "C" fn $fn_name(ctx: *mut SpectraHostCallContext) -> i32 {
            let Some((args, results_ptr)) = numeric_binary_args(ctx) else {
                return HOST_STATUS_INVALID_ARGUMENT;
            };
            let result = $helper(args[0], args[1], $bits);
            unsafe {
                *results_ptr = result;
            }
            HOST_STATUS_SUCCESS
        }
    };
}

macro_rules! define_checked_binary_host {
    ($fn_name:ident, $path:literal, $signed:expr, $bits:expr, $op:tt) => {
        extern "C" fn $fn_name(ctx: *mut SpectraHostCallContext) -> i32 {
            let Some((args, results_ptr)) = numeric_binary_args(ctx) else {
                return HOST_STATUS_INVALID_ARGUMENT;
            };
            let value = if $signed {
                let a = args[0] as i128;
                let b = args[1] as i128;
                let value = match stringify!($op) {
                    "+" => a.checked_add(b),
                    "-" => a.checked_sub(b),
                    "*" => a.checked_mul(b),
                    _ => None,
                };
                let Some(value) = value else {
                    return numeric_checked_error("E2902", concat!($path, " arithmetic overflow"));
                };
                let min = -(1_i128 << ($bits - 1));
                let max = (1_i128 << ($bits - 1)) - 1;
                if value < min || value > max {
                    return numeric_checked_error("E2902", concat!($path, " arithmetic overflow"));
                }
                value as i64
            } else {
                let a = args[0] as u128;
                let b = args[1] as u128;
                let value = match stringify!($op) {
                    "+" => a.checked_add(b),
                    "-" => a.checked_sub(b),
                    "*" => a.checked_mul(b),
                    _ => None,
                };
                let Some(value) = value else {
                    return numeric_checked_error("E2902", concat!($path, " arithmetic overflow"));
                };
                if value > ((1_u128 << $bits) - 1) {
                    return numeric_checked_error("E2902", concat!($path, " arithmetic overflow"));
                }
                value as i64
            };
            unsafe {
                *results_ptr = value;
            }
            HOST_STATUS_SUCCESS
        }
    };
}

define_numeric_host!(std_numeric_add_i8, crate::numeric::wrapping_add_signed, 8);
define_numeric_host!(std_numeric_sub_i8, crate::numeric::wrapping_sub_signed, 8);
define_numeric_host!(std_numeric_mul_i8, crate::numeric::wrapping_mul_signed, 8);
define_numeric_host!(std_numeric_add_i16, crate::numeric::wrapping_add_signed, 16);
define_numeric_host!(std_numeric_sub_i16, crate::numeric::wrapping_sub_signed, 16);
define_numeric_host!(std_numeric_mul_i16, crate::numeric::wrapping_mul_signed, 16);
define_numeric_host!(std_numeric_add_i32, crate::numeric::wrapping_add_signed, 32);
define_numeric_host!(std_numeric_sub_i32, crate::numeric::wrapping_sub_signed, 32);
define_numeric_host!(std_numeric_mul_i32, crate::numeric::wrapping_mul_signed, 32);
define_numeric_host!(std_numeric_add_i64, crate::numeric::wrapping_add_signed, 64);
define_numeric_host!(std_numeric_sub_i64, crate::numeric::wrapping_sub_signed, 64);
define_numeric_host!(std_numeric_mul_i64, crate::numeric::wrapping_mul_signed, 64);
define_numeric_host!(std_numeric_add_u8, crate::numeric::wrapping_add_unsigned, 8);
define_numeric_host!(std_numeric_sub_u8, crate::numeric::wrapping_sub_unsigned, 8);
define_numeric_host!(std_numeric_mul_u8, crate::numeric::wrapping_mul_unsigned, 8);
define_numeric_host!(
    std_numeric_add_u16,
    crate::numeric::wrapping_add_unsigned,
    16
);
define_numeric_host!(
    std_numeric_sub_u16,
    crate::numeric::wrapping_sub_unsigned,
    16
);
define_numeric_host!(
    std_numeric_mul_u16,
    crate::numeric::wrapping_mul_unsigned,
    16
);
define_numeric_host!(
    std_numeric_add_u32,
    crate::numeric::wrapping_add_unsigned,
    32
);
define_numeric_host!(
    std_numeric_sub_u32,
    crate::numeric::wrapping_sub_unsigned,
    32
);
define_numeric_host!(
    std_numeric_mul_u32,
    crate::numeric::wrapping_mul_unsigned,
    32
);
define_numeric_host!(
    std_numeric_add_u64,
    crate::numeric::wrapping_add_unsigned,
    64
);
define_numeric_host!(
    std_numeric_sub_u64,
    crate::numeric::wrapping_sub_unsigned,
    64
);
define_numeric_host!(
    std_numeric_mul_u64,
    crate::numeric::wrapping_mul_unsigned,
    64
);
define_checked_binary_host!(std_numeric_checked_add_i8, "i8", true, 8, +);
define_checked_binary_host!(std_numeric_checked_sub_i8, "i8", true, 8, -);
define_checked_binary_host!(std_numeric_checked_mul_i8, "i8", true, 8, *);
define_checked_binary_host!(std_numeric_checked_add_i16, "i16", true, 16, +);
define_checked_binary_host!(std_numeric_checked_sub_i16, "i16", true, 16, -);
define_checked_binary_host!(std_numeric_checked_mul_i16, "i16", true, 16, *);
define_checked_binary_host!(std_numeric_checked_add_i32, "i32", true, 32, +);
define_checked_binary_host!(std_numeric_checked_sub_i32, "i32", true, 32, -);
define_checked_binary_host!(std_numeric_checked_mul_i32, "i32", true, 32, *);
define_checked_binary_host!(std_numeric_checked_add_i64, "i64", true, 64, +);
define_checked_binary_host!(std_numeric_checked_sub_i64, "i64", true, 64, -);
define_checked_binary_host!(std_numeric_checked_mul_i64, "i64", true, 64, *);
define_checked_binary_host!(std_numeric_checked_add_u8, "u8", false, 8, +);
define_checked_binary_host!(std_numeric_checked_sub_u8, "u8", false, 8, -);
define_checked_binary_host!(std_numeric_checked_mul_u8, "u8", false, 8, *);
define_checked_binary_host!(std_numeric_checked_add_u16, "u16", false, 16, +);
define_checked_binary_host!(std_numeric_checked_sub_u16, "u16", false, 16, -);
define_checked_binary_host!(std_numeric_checked_mul_u16, "u16", false, 16, *);
define_checked_binary_host!(std_numeric_checked_add_u32, "u32", false, 32, +);
define_checked_binary_host!(std_numeric_checked_sub_u32, "u32", false, 32, -);
define_checked_binary_host!(std_numeric_checked_mul_u32, "u32", false, 32, *);
define_checked_binary_host!(std_numeric_checked_add_u64, "u64", false, 64, +);
define_checked_binary_host!(std_numeric_checked_sub_u64, "u64", false, 64, -);
define_checked_binary_host!(std_numeric_checked_mul_u64, "u64", false, 64, *);

fn register_numeric() {
    register_host_function("spectra.std.numeric.wrapping_add_i8", std_numeric_add_i8);
    register_host_function("spectra.std.numeric.wrapping_sub_i8", std_numeric_sub_i8);
    register_host_function("spectra.std.numeric.wrapping_mul_i8", std_numeric_mul_i8);
    register_host_function("spectra.std.numeric.wrapping_add_i16", std_numeric_add_i16);
    register_host_function("spectra.std.numeric.wrapping_sub_i16", std_numeric_sub_i16);
    register_host_function("spectra.std.numeric.wrapping_mul_i16", std_numeric_mul_i16);
    register_host_function("spectra.std.numeric.wrapping_add_i32", std_numeric_add_i32);
    register_host_function("spectra.std.numeric.wrapping_sub_i32", std_numeric_sub_i32);
    register_host_function("spectra.std.numeric.wrapping_mul_i32", std_numeric_mul_i32);
    register_host_function("spectra.std.numeric.wrapping_add_i64", std_numeric_add_i64);
    register_host_function("spectra.std.numeric.wrapping_sub_i64", std_numeric_sub_i64);
    register_host_function("spectra.std.numeric.wrapping_mul_i64", std_numeric_mul_i64);
    register_host_function("spectra.std.numeric.wrapping_add_u8", std_numeric_add_u8);
    register_host_function("spectra.std.numeric.wrapping_sub_u8", std_numeric_sub_u8);
    register_host_function("spectra.std.numeric.wrapping_mul_u8", std_numeric_mul_u8);
    register_host_function("spectra.std.numeric.wrapping_add_u16", std_numeric_add_u16);
    register_host_function("spectra.std.numeric.wrapping_sub_u16", std_numeric_sub_u16);
    register_host_function("spectra.std.numeric.wrapping_mul_u16", std_numeric_mul_u16);
    register_host_function("spectra.std.numeric.wrapping_add_u32", std_numeric_add_u32);
    register_host_function("spectra.std.numeric.wrapping_sub_u32", std_numeric_sub_u32);
    register_host_function("spectra.std.numeric.wrapping_mul_u32", std_numeric_mul_u32);
    register_host_function("spectra.std.numeric.wrapping_add_u64", std_numeric_add_u64);
    register_host_function("spectra.std.numeric.wrapping_sub_u64", std_numeric_sub_u64);
    register_host_function("spectra.std.numeric.wrapping_mul_u64", std_numeric_mul_u64);
    register_host_function("spectra.std.numeric.checked_i8", std_numeric_checked_i8);
    register_host_function("spectra.std.numeric.checked_i16", std_numeric_checked_i16);
    register_host_function("spectra.std.numeric.checked_i32", std_numeric_checked_i32);
    register_host_function("spectra.std.numeric.checked_i64", std_numeric_checked_i64);
    register_host_function("spectra.std.numeric.checked_u8", std_numeric_checked_u8);
    register_host_function("spectra.std.numeric.checked_u16", std_numeric_checked_u16);
    register_host_function("spectra.std.numeric.checked_u32", std_numeric_checked_u32);
    register_host_function("spectra.std.numeric.checked_u64", std_numeric_checked_u64);
    register_host_function(NUMERIC_CHECKED_F32, std_numeric_checked_f32);
    register_host_function(
        "spectra.std.numeric.checked_float_i8",
        std_numeric_checked_float_i8,
    );
    register_host_function(
        "spectra.std.numeric.checked_float_i16",
        std_numeric_checked_float_i16,
    );
    register_host_function(
        "spectra.std.numeric.checked_float_i32",
        std_numeric_checked_float_i32,
    );
    register_host_function(
        "spectra.std.numeric.checked_float_i64",
        std_numeric_checked_float_i64,
    );
    register_host_function(
        "spectra.std.numeric.checked_float_u8",
        std_numeric_checked_float_u8,
    );
    register_host_function(
        "spectra.std.numeric.checked_float_u16",
        std_numeric_checked_float_u16,
    );
    register_host_function(
        "spectra.std.numeric.checked_float_u32",
        std_numeric_checked_float_u32,
    );
    register_host_function(
        "spectra.std.numeric.checked_float_u64",
        std_numeric_checked_float_u64,
    );
    register_host_function(
        "spectra.std.numeric.checked_add_i8",
        std_numeric_checked_add_i8,
    );
    register_host_function(
        "spectra.std.numeric.checked_sub_i8",
        std_numeric_checked_sub_i8,
    );
    register_host_function(
        "spectra.std.numeric.checked_mul_i8",
        std_numeric_checked_mul_i8,
    );
    register_host_function(
        "spectra.std.numeric.checked_add_i16",
        std_numeric_checked_add_i16,
    );
    register_host_function(
        "spectra.std.numeric.checked_sub_i16",
        std_numeric_checked_sub_i16,
    );
    register_host_function(
        "spectra.std.numeric.checked_mul_i16",
        std_numeric_checked_mul_i16,
    );
    register_host_function(
        "spectra.std.numeric.checked_add_i32",
        std_numeric_checked_add_i32,
    );
    register_host_function(
        "spectra.std.numeric.checked_sub_i32",
        std_numeric_checked_sub_i32,
    );
    register_host_function(
        "spectra.std.numeric.checked_mul_i32",
        std_numeric_checked_mul_i32,
    );
    register_host_function(
        "spectra.std.numeric.checked_add_i64",
        std_numeric_checked_add_i64,
    );
    register_host_function(
        "spectra.std.numeric.checked_sub_i64",
        std_numeric_checked_sub_i64,
    );
    register_host_function(
        "spectra.std.numeric.checked_mul_i64",
        std_numeric_checked_mul_i64,
    );
    register_host_function(
        "spectra.std.numeric.checked_add_u8",
        std_numeric_checked_add_u8,
    );
    register_host_function(
        "spectra.std.numeric.checked_sub_u8",
        std_numeric_checked_sub_u8,
    );
    register_host_function(
        "spectra.std.numeric.checked_mul_u8",
        std_numeric_checked_mul_u8,
    );
    register_host_function(
        "spectra.std.numeric.checked_add_u16",
        std_numeric_checked_add_u16,
    );
    register_host_function(
        "spectra.std.numeric.checked_sub_u16",
        std_numeric_checked_sub_u16,
    );
    register_host_function(
        "spectra.std.numeric.checked_mul_u16",
        std_numeric_checked_mul_u16,
    );
    register_host_function(
        "spectra.std.numeric.checked_add_u32",
        std_numeric_checked_add_u32,
    );
    register_host_function(
        "spectra.std.numeric.checked_sub_u32",
        std_numeric_checked_sub_u32,
    );
    register_host_function(
        "spectra.std.numeric.checked_mul_u32",
        std_numeric_checked_mul_u32,
    );
    register_host_function(
        "spectra.std.numeric.checked_add_u64",
        std_numeric_checked_add_u64,
    );
    register_host_function(
        "spectra.std.numeric.checked_sub_u64",
        std_numeric_checked_sub_u64,
    );
    register_host_function(
        "spectra.std.numeric.checked_mul_u64",
        std_numeric_checked_mul_u64,
    );
}

fn numeric_checked_error(code: &str, message: &str) -> i32 {
    eprintln!("{code}: {message}");
    HOST_STATUS_INVALID_ARGUMENT
}

extern "C" fn std_numeric_checked_f32(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some((raw, results_ptr)) = numeric_unary_arg(ctx) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let value = f64::from_bits(raw as u64);
    let narrowed = value as f32;
    if !value.is_finite() || !narrowed.is_finite() || narrowed as f64 != value {
        return numeric_checked_error("E2904", "value is not exactly representable as f32");
    }
    unsafe {
        *results_ptr = (narrowed as f64).to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

macro_rules! define_checked_int_cast {
    ($fn_name:ident, $path:literal, $signed:expr, $bits:expr) => {
        extern "C" fn $fn_name(ctx: *mut SpectraHostCallContext) -> i32 {
            let Some((value, results_ptr)) = numeric_unary_arg(ctx) else {
                return HOST_STATUS_INVALID_ARGUMENT;
            };
            let value = value as i128;
            let valid = if $signed {
                let min = -(1_i128 << ($bits - 1));
                let max = (1_i128 << ($bits - 1)) - 1;
                value >= min && value <= max
            } else {
                value >= 0 && (value as u128) <= ((1_u128 << $bits) - 1)
            };
            if !valid {
                return numeric_checked_error(
                    "E2903",
                    concat!("value is outside ", $path, " range"),
                );
            }
            unsafe {
                *results_ptr = value as i64;
            }
            HOST_STATUS_SUCCESS
        }
    };
}

define_checked_int_cast!(std_numeric_checked_i8, "i8", true, 8);
define_checked_int_cast!(std_numeric_checked_i16, "i16", true, 16);
define_checked_int_cast!(std_numeric_checked_i32, "i32", true, 32);
define_checked_int_cast!(std_numeric_checked_i64, "i64", true, 64);
define_checked_int_cast!(std_numeric_checked_u8, "u8", false, 8);
define_checked_int_cast!(std_numeric_checked_u16, "u16", false, 16);
define_checked_int_cast!(std_numeric_checked_u32, "u32", false, 32);
define_checked_int_cast!(std_numeric_checked_u64, "u64", false, 64);

macro_rules! define_checked_float_int_cast {
    ($fn_name:ident, $path:literal, $signed:expr, $bits:expr) => {
        extern "C" fn $fn_name(ctx: *mut SpectraHostCallContext) -> i32 {
            let Some((raw, results_ptr)) = numeric_unary_arg(ctx) else {
                return HOST_STATUS_INVALID_ARGUMENT;
            };
            let value = f64::from_bits(raw as u64);
            let valid = value.is_finite()
                && value.fract() == 0.0
                && if $signed {
                    let min = -(2_f64).powi(($bits - 1) as i32);
                    let max = (2_f64).powi(($bits - 1) as i32) - 1.0;
                    value >= min && value <= max
                } else {
                    value >= 0.0 && value <= (2_f64).powi($bits as i32) - 1.0
                };
            if !valid {
                return numeric_checked_error(
                    "E2904",
                    concat!("value is outside ", $path, " range or is non-finite"),
                );
            }
            unsafe {
                *results_ptr = value as i128 as i64;
            }
            HOST_STATUS_SUCCESS
        }
    };
}

define_checked_float_int_cast!(std_numeric_checked_float_i8, "i8", true, 8);
define_checked_float_int_cast!(std_numeric_checked_float_i16, "i16", true, 16);
define_checked_float_int_cast!(std_numeric_checked_float_i32, "i32", true, 32);
define_checked_float_int_cast!(std_numeric_checked_float_i64, "i64", true, 64);
define_checked_float_int_cast!(std_numeric_checked_float_u8, "u8", false, 8);
define_checked_float_int_cast!(std_numeric_checked_float_u16, "u16", false, 16);
define_checked_float_int_cast!(std_numeric_checked_float_u32, "u32", false, 32);
define_checked_float_int_cast!(std_numeric_checked_float_u64, "u64", false, 64);

fn register_math() {
    register_host_function(MATH_ABS, std_math_abs);
    register_host_function(MATH_MIN, std_math_min);
    register_host_function(MATH_MAX, std_math_max);
    register_host_function(MATH_CLAMP, std_math_clamp);
    register_host_function(MATH_SQRT_F, std_math_sqrt_f);
    register_host_function(MATH_POW_F, std_math_pow_f);
    register_host_function(MATH_FLOOR_F, std_math_floor_f);
    register_host_function(MATH_CEIL_F, std_math_ceil_f);
    register_host_function(MATH_ROUND_F, std_math_round_f);
    register_host_function(MATH_SIN_F, std_math_sin_f);
    register_host_function(MATH_COS_F, std_math_cos_f);
    register_host_function(MATH_TAN_F, std_math_tan_f);
    register_host_function(MATH_LOG_F, std_math_log_f);
    register_host_function(MATH_LOG2_F, std_math_log2_f);
    register_host_function(MATH_LOG10_F, std_math_log10_f);
    register_host_function(MATH_ATAN2_F, std_math_atan2_f);
    register_host_function(MATH_PI, std_math_pi);
    register_host_function(MATH_E_CONST, std_math_e_const);
    register_host_function(MATH_SIGN, std_math_sign);
    register_host_function(MATH_GCD, std_math_gcd);
    register_host_function(MATH_LCM, std_math_lcm);
    register_host_function(MATH_IS_NAN_F, std_math_is_nan_f);
    register_host_function(MATH_IS_INFINITE_F, std_math_is_infinite_f);
    register_host_function(MATH_ABS_F, std_math_abs_f);
}

fn register_io() {
    register_host_function(IO_PRINT, std_io_print);
    register_host_function(IO_PRINTLN, std_io_println);
    register_host_function(IO_FLUSH, std_io_flush);
    register_host_function(IO_EPRINT, std_io_eprint);
    register_host_function(IO_EPRINTLN, std_io_eprintln);
    register_host_function(IO_READ_LINE, std_io_read_line);
    register_host_function(IO_INPUT, std_io_input);
}

fn register_collections() {
    register_host_function(LIST_NEW, std_list_new);
    register_host_function(LIST_PUSH, std_list_push);
    register_host_function(LIST_LEN, std_list_len);
    register_host_function(LIST_GET, std_list_get_option);
    register_host_function(LIST_GET_OPTION, std_list_get_option);
    register_host_function(LIST_GET_COMPAT, std_list_get);
    register_host_function(LIST_SET, std_list_set);
    register_host_function(LIST_CONTAINS, std_list_contains);
    register_host_function(LIST_CLEAR, std_list_clear);
    register_host_function(LIST_FREE, std_list_free);
    register_host_function(LIST_FREE_ALL, std_list_free_all);
    register_host_function(LIST_POP, std_list_pop_option);
    register_host_function(LIST_POP_FRONT, std_list_pop_front_option);
    register_host_function(LIST_POP_OPTION, std_list_pop_option);
    register_host_function(LIST_POP_FRONT_OPTION, std_list_pop_front_option);
    register_host_function(LIST_POP_COMPAT, std_list_pop);
    register_host_function(LIST_POP_FRONT_COMPAT, std_list_pop_front);
    register_host_function(LIST_INSERT_AT, std_list_insert_at);
    register_host_function(LIST_REMOVE_AT, std_list_remove_at_option);
    register_host_function(LIST_REMOVE_AT_OPTION, std_list_remove_at_option);
    register_host_function(LIST_REMOVE_AT_COMPAT, std_list_remove_at);
    register_host_function(LIST_INDEX_OF, std_list_index_of);
    register_host_function(LIST_SORT, std_list_sort);
    register_host_function(LIST_MAP, std_list_map);
    register_host_function(LIST_FILTER, std_list_filter);
    register_host_function(LIST_REDUCE, std_list_reduce);
    register_host_function(LIST_SORT_BY, std_list_sort_by);
}

fn register_tensor() {
    register_host_function(TENSOR_ZEROS, std_tensor_zeros);
    register_host_function(TENSOR_ONES, std_tensor_ones);
    register_host_function(TENSOR_FULL, std_tensor_full);
    register_host_function(TENSOR_FULL_F, std_tensor_full_f);
    register_host_function(TENSOR_LITERAL, std_tensor_literal);
    register_host_function(TENSOR_LITERAL_F, std_tensor_literal_f);
    register_host_function(TENSOR_LITERAL2, std_tensor_literal2);
    register_host_function(TENSOR_LITERAL2_F, std_tensor_literal2_f);
    register_host_function(TENSOR_ARANGE, std_tensor_arange);
    register_host_function(TENSOR_ZEROS2, std_tensor_zeros2);
    register_host_function(TENSOR_ONES2, std_tensor_ones2);
    register_host_function(TENSOR_FULL2, std_tensor_full2);
    register_host_function(TENSOR_FULL2_F, std_tensor_full2_f);
    register_host_function(TENSOR_LEN, std_tensor_len);
    register_host_function(TENSOR_RANK, std_tensor_rank);
    register_host_function(TENSOR_DIM, std_tensor_dim);
    register_host_function(TENSOR_ROWS, std_tensor_rows);
    register_host_function(TENSOR_COLS, std_tensor_cols);
    register_host_function(TENSOR_IS_VALID, std_tensor_is_valid);
    register_host_function(TENSOR_GET, std_tensor_get);
    register_host_function(TENSOR_GET_F, std_tensor_get_f);
    register_host_function(TENSOR_SET, std_tensor_set);
    register_host_function(TENSOR_SET_F, std_tensor_set_f);
    register_host_function(TENSOR_GET2, std_tensor_get2);
    register_host_function(TENSOR_GET2_F, std_tensor_get2_f);
    register_host_function(TENSOR_SET2, std_tensor_set2);
    register_host_function(TENSOR_SET2_F, std_tensor_set2_f);
    register_host_function(TENSOR_RESHAPE, std_tensor_reshape);
    register_host_function(TENSOR_FLATTEN, std_tensor_flatten);
    register_host_function(TENSOR_PERMUTE, std_tensor_permute);
    register_host_function(TENSOR_SLICE, std_tensor_slice);
    register_host_function(TENSOR_CONCAT, std_tensor_concat);
    register_host_function(TENSOR_STACK, std_tensor_stack);
    register_host_function(TENSOR_ADD, std_tensor_add);
    register_host_function(TENSOR_SUB, std_tensor_sub);
    register_host_function(TENSOR_MUL, std_tensor_mul);
    register_host_function(TENSOR_DIV, std_tensor_div);
    register_host_function(TENSOR_SUM, std_tensor_sum);
    register_host_function(TENSOR_SUM_F, std_tensor_sum_f);
    register_host_function(TENSOR_SUM_T, std_tensor_sum_t);
    register_host_function(TENSOR_MEAN_F, std_tensor_mean_f);
    register_host_function(TENSOR_MEAN_T, std_tensor_mean_t);
    register_host_function(TENSOR_MAX, std_tensor_max);
    register_host_function(TENSOR_MIN, std_tensor_min);
    register_host_function(TENSOR_ARGMAX, std_tensor_argmax);
    register_host_function(TENSOR_MATMUL, std_tensor_matmul);
    register_host_function(TENSOR_MATMUL_BATCHED, std_tensor_matmul_batched);
    register_host_function(TENSOR_TRANSPOSE, std_tensor_transpose);
    register_host_function(TENSOR_DOT, std_tensor_dot);
    register_host_function(TENSOR_DOT_T, std_tensor_dot_t);
    register_host_function(TENSOR_NEG, std_tensor_neg);
    register_host_function(TENSOR_EXP_F, std_tensor_exp_f);
    register_host_function(TENSOR_LOG_F, std_tensor_log_f);
    register_host_function(TENSOR_SQRT_F, std_tensor_sqrt_f);
    register_host_function(TENSOR_RELU, std_tensor_relu);
    register_host_function(TENSOR_SIGMOID_F, std_tensor_sigmoid_f);
    register_host_function(TENSOR_TANH_F, std_tensor_tanh_f);
    register_host_function(TENSOR_SEED, std_tensor_seed);
    register_host_function(TENSOR_UNIFORM, std_tensor_uniform);
    register_host_function(TENSOR_UNIFORM_F, std_tensor_uniform_f);
    register_host_function(TENSOR_NORMAL_F, std_tensor_normal_f);
    register_host_function(TENSOR_BERNOULLI, std_tensor_bernoulli);
    register_host_function(TENSOR_CATEGORICAL, std_tensor_categorical);
    register_host_function(
        TENSOR_SET_DETERMINISTIC_MODE,
        std_tensor_set_deterministic_mode,
    );
    register_host_function(TENSOR_DETERMINISTIC_MODE, std_tensor_deterministic_mode);
    register_host_function(TENSOR_TOLERANCE_ABS, std_tensor_tolerance_abs);
    register_host_function(TENSOR_TOLERANCE_REL, std_tensor_tolerance_rel);
    register_host_function(TENSOR_DEVICE, std_tensor_device);
    register_host_function(TENSOR_DEVICE_AVAILABLE, std_tensor_device_available);
    register_host_function(TENSOR_DEVICE_STATUS, std_tensor_device_status);
    register_host_function(TENSOR_TO_DEVICE, std_tensor_to_device);
    register_host_function(TENSOR_CPU, std_tensor_cpu);
    register_host_function(TENSOR_SYNC, std_tensor_sync);
    register_host_function(TENSOR_PRECISION, std_tensor_precision);
    register_host_function(TENSOR_TO_PRECISION, std_tensor_to_precision);
    register_host_function(TENSOR_STATS_ALLOCATIONS, std_tensor_stats_allocations);
    register_host_function(TENSOR_STATS_ACTIVE, std_tensor_stats_active);
    register_host_function(TENSOR_STATS_PEAK_BYTES, std_tensor_stats_peak_bytes);
    register_host_function(TENSOR_STATS_REUSED_BUFFERS, std_tensor_stats_reused_buffers);
    register_host_function(TENSOR_STATS_POOL_HITS, std_tensor_stats_pool_hits);
    register_host_function(TENSOR_STATS_POOL_MISSES, std_tensor_stats_pool_misses);
    register_host_function(TENSOR_STATS_ACTIVE_BYTES, std_tensor_stats_active_bytes);
    register_host_function(TENSOR_STATS_SCRATCH_REUSES, std_tensor_stats_scratch_reuses);
    register_host_function(TENSOR_KERNEL_STRATEGY, std_tensor_kernel_strategy);
    register_host_function(TENSOR_STATS_KERNEL_OPS, std_tensor_stats_kernel_ops);
    register_host_function(
        TENSOR_STATS_KERNEL_ELEMENTS,
        std_tensor_stats_kernel_elements,
    );
    register_host_function(
        TENSOR_STATS_DEVICE_TRANSFERS,
        std_tensor_stats_device_transfers,
    );
    register_host_function(TENSOR_STATS_GPU_KERNEL_OPS, std_tensor_stats_gpu_kernel_ops);
    register_host_function(TENSOR_STATS_CPU_FALLBACKS, std_tensor_stats_cpu_fallbacks);
    register_host_function(TENSOR_STATS_GPU_ERRORS, std_tensor_stats_gpu_errors);
    register_host_function(
        TENSOR_STATS_DEVICE_RESIDENT,
        std_tensor_stats_device_resident_tensors,
    );
    register_host_function(
        TENSOR_STATS_GPU_BACKWARD_OPS,
        std_tensor_stats_gpu_backward_ops,
    );
    #[cfg(feature = "gpu")]
    {
        register_host_function(
            TENSOR_STATS_DEVICE_POOL_HITS,
            std_tensor_stats_device_pool_hits,
        );
        register_host_function(
            TENSOR_STATS_DEVICE_POOL_MISSES,
            std_tensor_stats_device_pool_misses,
        );
        register_host_function(
            TENSOR_STATS_DEVICE_POOL_BYTES_RESIDENT,
            std_tensor_stats_device_pool_bytes_resident,
        );
        register_host_function(TENSOR_STORAGE_DEVICE, std_tensor_storage_device);
    }
    register_host_function(TENSOR_STATS_GRAPH_NODES, std_tensor_stats_graph_nodes);
    register_host_function(
        TENSOR_STATS_LIFETIME_RECORDS,
        std_tensor_stats_lifetime_records,
    );
    register_host_function(
        TENSOR_STATS_RELEASED_LIFETIMES,
        std_tensor_stats_released_lifetimes,
    );
    register_host_function(
        TENSOR_STATS_ALLOCATION_SITES,
        std_tensor_stats_allocation_sites,
    );
    register_host_function(
        TENSOR_STATS_REUSE_RATE_PER_MILLE,
        std_tensor_stats_reuse_rate_per_mille,
    );
    register_host_function(TENSOR_MEMORY_REPORT, std_tensor_memory_report);
    register_host_function(TENSOR_RESET_STATS, std_tensor_reset_stats);
    register_host_function(TENSOR_REQUIRES_GRAD, std_tensor_requires_grad);
    register_host_function(TENSOR_BACKWARD, std_tensor_backward);
    register_host_function(TENSOR_GRAD, std_tensor_grad);
    register_host_function(TENSOR_ZERO_GRAD, std_tensor_zero_grad);
    register_host_function(TENSOR_SET_GRAD_ENABLED, std_tensor_set_grad_enabled);
    register_host_function(TENSOR_GRAD_ENABLED, std_tensor_grad_enabled);
    register_host_function(TENSOR_FREE, std_tensor_free);
    register_host_function(TENSOR_FREE_ALL, std_tensor_free_all);
    register_host_function(TENSOR_REFILL, std_tensor_refill);
}

fn register_ml() {
    register_host_function(ML_MODULE_NEW, std_ml_module_new);
    register_host_function(ML_MODULE_ADD_PARAMETER, std_ml_module_add_parameter);
    register_host_function(ML_MODULE_PARAMETER_COUNT, std_ml_module_parameter_count);
    register_host_function(ML_MODULE_PARAMETER, std_ml_module_parameter);
    register_host_function(ML_MODULE_SET_TRAINING, std_ml_module_set_training);
    register_host_function(ML_MODULE_IS_TRAINING, std_ml_module_is_training);
    register_host_function(ML_LINEAR, std_ml_linear);
    register_host_function(ML_CONV2D, std_ml_conv2d);
    register_host_function(ML_DROPOUT, std_ml_dropout);
    register_host_function(ML_MAX_POOL2D, std_ml_max_pool2d);
    register_host_function(ML_MSE_LOSS, std_ml_mse_loss);
    register_host_function(ML_BCE_LOSS, std_ml_bce_loss);
    register_host_function(ML_CROSS_ENTROPY_LOSS, std_ml_cross_entropy_loss);
    register_host_function(ML_NLL_LOSS, std_ml_nll_loss);
    register_host_function(ML_SGD_STEP, std_ml_sgd_step);
    register_host_function(ML_SGD_MOMENTUM_STEP, std_ml_sgd_momentum_step);
    register_host_function(ML_ADAM_STEP, std_ml_adam_step);
    register_host_function(ML_ADAMW_STEP, std_ml_adamw_step);
    register_host_function(ML_EXP_LR, std_ml_exp_lr);
    register_host_function(ML_UNSCALE_GRAD, std_ml_unscale_grad);
    register_host_function(ML_DATASET_FROM_TENSORS, std_ml_dataset_from_tensors);
    register_host_function(ML_DATASET_FROM_CSV, std_ml_dataset_from_csv);
    register_host_function(ML_DATASET_FROM_JSONL, std_ml_dataset_from_jsonl);
    register_host_function(ML_DATASET_FROM_NPY, std_ml_dataset_from_npy);
    register_host_function(ML_DATASET_FROM_DIRECTORY, std_ml_dataset_from_directory);
    register_host_function(ML_DATASET_LEN, std_ml_dataset_len);
    register_host_function(ML_DATASET_MAP_FEATURES, std_ml_dataset_map_features);
    register_host_function(ML_DATASET_FILTER_LABEL_MIN, std_ml_dataset_filter_label_min);
    register_host_function(ML_DATASET_TRAIN_SPLIT, std_ml_dataset_train_split);
    register_host_function(ML_DATASET_TEST_SPLIT, std_ml_dataset_test_split);
    register_host_function(ML_DATALOADER_NEW, std_ml_dataloader_new);
    register_host_function(ML_DATALOADER_BATCH_COUNT, std_ml_dataloader_batch_count);
    register_host_function(
        ML_DATALOADER_BATCH_FEATURES,
        std_ml_dataloader_batch_features,
    );
    register_host_function(ML_DATALOADER_BATCH_LABELS, std_ml_dataloader_batch_labels);
    register_host_function(ML_DATAFRAME_FROM_CSV, std_ml_dataframe_from_csv);
    register_host_function(ML_DATAFRAME_ROWS, std_ml_dataframe_rows);
    register_host_function(ML_DATAFRAME_COLS, std_ml_dataframe_cols);
    register_host_function(ML_DATAFRAME_COLUMN, std_ml_dataframe_column);
    register_host_function(ML_EXPERIMENT_START, std_ml_experiment_start);
    register_host_function(ML_EXPERIMENT_SET_CONFIG, std_ml_experiment_set_config);
    register_host_function(ML_EXPERIMENT_LOG_METRIC, std_ml_experiment_log_metric);
    register_host_function(ML_EXPERIMENT_LOG_ARTIFACT, std_ml_experiment_log_artifact);
    register_host_function(ML_EXPERIMENT_SET_LOCKFILE, std_ml_experiment_set_lockfile);
    register_host_function(
        ML_EXPERIMENT_SET_MODEL_OUTPUT,
        std_ml_experiment_set_model_output,
    );
    register_host_function(ML_EXPERIMENT_FINISH, std_ml_experiment_finish);
    register_host_function(ML_EXPERIMENT_MANIFEST_PATH, std_ml_experiment_manifest_path);
    register_host_function(ML_EXPERIMENT_REPRO_COMMAND, std_ml_experiment_repro_command);
    register_host_function(
        ML_EXPERIMENT_COMPARE_MANIFESTS,
        std_ml_experiment_compare_manifests,
    );
    register_host_function(
        ML_DISTRIBUTED_SESSION_START,
        std_ml_distributed_session_start,
    );
    register_host_function(ML_DISTRIBUTED_GLOBAL_STEP, std_ml_distributed_global_step);
    register_host_function(
        ML_DISTRIBUTED_WORKER_STEP_COUNT,
        std_ml_distributed_worker_step_count,
    );
    register_host_function(
        ML_DISTRIBUTED_CHECKPOINT_SAVE,
        std_ml_distributed_checkpoint_save,
    );
    register_host_function(ML_DISTRIBUTED_RESUME, std_ml_distributed_resume);
    register_host_function(ML_DISTRIBUTED_SUMMARY, std_ml_distributed_summary);
    // ── DistTCP ──
    register_host_function(
        ML_DISTRIBUTED_TRAIN_MULTITHREAD,
        std_ml_distributed_train_multithread,
    );
    register_host_function(ML_DISTRIBUTED_TRAIN_TCP, std_ml_distributed_train_tcp);
    register_host_function(ML_ONNX_EXPORT, std_ml_onnx_export);
    register_host_function(ML_ONNX_IMPORT_SUMMARY, std_ml_onnx_import_summary);
    register_host_function(ML_ONNX_VALIDATE, std_ml_onnx_validate);
    register_host_function(ML_ONNX_ROUNDTRIP, std_ml_onnx_roundtrip);
    register_host_function(ML_ONNX_SESSION_FROM_BYTES, std_ml_onnx_session_from_bytes);
    register_host_function(ML_ONNX_RUN, std_ml_onnx_run);
    register_host_function(ML_ONNX_SESSION_FREE, std_ml_onnx_session_free);
    register_host_function(ML_EMBEDDING_LOOKUP, std_ml_embedding_lookup);
    register_host_function(ML_POSITIONAL_ENCODING, std_ml_positional_encoding);
    register_host_function(ML_LAYER_NORM, std_ml_layer_norm);
    register_host_function(ML_GELU, std_ml_gelu);
    register_host_function(ML_SWIGLU, std_ml_swiglu);
    register_host_function(ML_ATTENTION, std_ml_attention);
    register_host_function(ML_KV_CACHE_NEW, std_ml_kv_cache_new);
    register_host_function(ML_KV_CACHE_APPEND, std_ml_kv_cache_append);
    register_host_function(ML_KV_CACHE_KEYS, std_ml_kv_cache_keys);
    register_host_function(ML_KV_CACHE_VALUES, std_ml_kv_cache_values);
    register_host_function(ML_KV_CACHE_LEN, std_ml_kv_cache_len);
    register_host_function(ML_LOGITS_SAMPLE, std_ml_logits_sample);
    register_host_function(ML_TOKENIZER_WORDPIECE, std_ml_tokenizer_wordpiece);
    register_host_function(ML_TOKENIZER_LOAD, std_ml_tokenizer_load);
    register_host_function(ML_TOKENIZER_ENCODE, std_ml_tokenizer_encode);
    register_host_function(ML_TOKENIZER_DECODE, std_ml_tokenizer_decode);
    register_host_function(ML_TEXT_EMBED, std_ml_text_embed);
    register_host_function(ML_EMBEDDING_LOAD, std_ml_embedding_load);
    register_host_function(ML_VECTOR_INDEX_NEW, std_ml_vector_index_new);
    register_host_function(ML_VECTOR_INDEX_INSERT, std_ml_vector_index_insert);
    register_host_function(ML_VECTOR_INDEX_QUERY, std_ml_vector_index_query);
    register_host_function(ML_VECTOR_INDEX_PERSIST, std_ml_vector_index_persist);
    register_host_function(ML_VECTOR_INDEX_LOAD, std_ml_vector_index_load);
    register_host_function(
        ML_VECTOR_INDEX_SET_METADATA,
        std_ml_vector_index_set_metadata,
    );
    register_host_function(ML_VECTOR_INDEX_METRICS, std_ml_vector_index_metrics);
    register_host_function(ML_RAG_CHUNK_TEXT, std_ml_rag_chunk_text);
    register_host_function(ML_RAG_BUILD_PROMPT, std_ml_rag_build_prompt);
    register_host_function(ML_RAG_EVALUATE_ANSWER, std_ml_rag_evaluate_answer);
    register_host_function(ML_METRICS_CLASSIFICATION, std_ml_metrics_classification);
    register_host_function(ML_METRICS_REGRESSION, std_ml_metrics_regression);
    register_host_function(ML_METRICS_RANKING, std_ml_metrics_ranking);
    register_host_function(ML_METRICS_GENERATION, std_ml_metrics_generation);
    register_host_function(ML_SERVING_METRICS, std_ml_serving_metrics);
    register_host_function(ML_EVALUATION_REPORT, std_ml_evaluation_report);
    register_host_function(ML_ARTIFACT_NEW, std_ml_artifact_new);
    register_host_function(ML_ARTIFACT_SET_METADATA, std_ml_artifact_set_metadata);
    register_host_function(ML_ARTIFACT_ADD_TENSOR, std_ml_artifact_add_tensor);
    register_host_function(ML_ARTIFACT_SAVE, std_ml_artifact_save);
    register_host_function(ML_ARTIFACT_LOAD, std_ml_artifact_load);
    register_host_function(ML_ARTIFACT_TENSOR, std_ml_artifact_tensor);
    register_host_function(ML_ARTIFACT_METADATA, std_ml_artifact_metadata);
    register_host_function(ML_ARTIFACT_VALIDATE, std_ml_artifact_validate);
    register_host_function(ML_ARTIFACT_FREE, std_ml_artifact_free);

    // ── TokenizerTrainer ──
    register_host_function(ML_TOKENIZER_TRAIN_BPE, std_ml_train_bpe);
    register_host_function(ML_TOKENIZER_TRAIN_WORDPIECE, std_ml_train_wordpiece);
    register_host_function(ML_TOKENIZER_VOCAB, std_ml_tokenizer_vocab);
}

fn register_concurrent() {
    register_host_function(CONCURRENT_TASK_SPAWN, std_concurrent_task_spawn);
    register_host_function(CONCURRENT_TASK_SPAWN_FN, std_concurrent_task_spawn_fn);
    register_host_function(CONCURRENT_TASK_JOIN, std_concurrent_task_join);
    register_host_function(CONCURRENT_TASK_SPAWN_JOIN, std_concurrent_task_spawn_join);
    register_host_function(CONCURRENT_TASK_SPAWN_BATCH, std_concurrent_task_spawn_batch);
    register_host_function(
        CONCURRENT_TASK_JOIN_BATCH_SUM,
        std_concurrent_task_join_batch_sum,
    );
    register_host_function(CONCURRENT_TASK_IS_DONE, std_concurrent_task_is_done);
    register_host_function(CONCURRENT_CHANNEL_NEW, std_concurrent_channel_new);
    register_host_function(CONCURRENT_CHANNEL_SEND, std_concurrent_channel_send);
    register_host_function(CONCURRENT_CHANNEL_RECV, std_concurrent_channel_recv);
    register_host_function(CONCURRENT_CHANNEL_LEN, std_concurrent_channel_len);
    register_host_function(CONCURRENT_CHANNEL_CLOSE, std_concurrent_channel_close);
    register_host_function(CONCURRENT_COUNTER_NEW, std_concurrent_counter_new);
    register_host_function(CONCURRENT_COUNTER_ADD, std_concurrent_counter_add);
    register_host_function(CONCURRENT_COUNTER_GET, std_concurrent_counter_get);
    register_host_function(CONCURRENT_PIPELINE_SUM, std_concurrent_pipeline_sum);
    register_host_function(
        CONCURRENT_STATS_TASKS_SPAWNED,
        std_concurrent_stats_tasks_spawned,
    );
    register_host_function(CONCURRENT_STATS_CHANNELS, std_concurrent_stats_channels);
    register_host_function(CONCURRENT_RESET, std_concurrent_reset);
}

fn register_async() {
    register_host_function(ASYNC_TASK_READY, std_async_task_ready);
    register_host_function(ASYNC_TASK_READY_BATCH, std_async_task_ready_batch);
    register_host_function(ASYNC_TASK_BATCH_CHECKSUM, std_async_task_batch_checksum);
    register_host_function(ASYNC_TASK_POLL, std_async_task_poll);
    register_host_function(ASYNC_TASK_RESULT, std_async_task_result);
    register_host_function(ASYNC_TASK_WAIT, std_async_task_wait);
    register_host_function(ASYNC_TASK_BLOCK_ON, std_async_task_block_on);
    register_host_function(ASYNC_TASK_JOIN, std_async_task_join);
    register_host_function(ASYNC_TASK_JOIN_STATUS, std_async_task_join_status);
    register_host_function(ASYNC_TASK_CANCEL, std_async_task_cancel);
    register_host_function(ASYNC_TASK_IS_CANCELLED, std_async_task_is_cancelled);
    register_host_function(ASYNC_TASK_CANCEL_HANDLE, std_async_task_cancel_handle);
    register_host_function(ASYNC_TASK_WITH_TIMEOUT, std_async_task_with_timeout);
    register_host_function(ASYNC_TASK_FAIL, std_async_task_fail);
    register_host_function(ASYNC_TASK_JOIN_ORDER, std_async_task_join_order);
    register_host_function(ASYNC_TASK_RESET, std_async_task_reset);
    register_host_function(ASYNC_CANCEL_HANDLE_CANCEL, std_async_cancel_handle_cancel);
    register_host_function(
        ASYNC_SCHEDULER_ADVANCE_TIME,
        std_async_scheduler_advance_time,
    );
    register_host_function(ASYNC_SCOPE_NEW, std_async_scope_new);
    register_host_function(ASYNC_SCOPE_CHILD, std_async_scope_child);
    register_host_function(ASYNC_SCOPE_ATTACH, std_async_scope_attach);
    register_host_function(ASYNC_SCOPE_SPAWN_READY, std_async_scope_spawn_ready);
    register_host_function(ASYNC_SCOPE_CANCEL, std_async_scope_cancel);
    register_host_function(ASYNC_SCOPE_JOIN, std_async_scope_join);
    register_host_function(ASYNC_SCOPE_JOINED_COUNT, std_async_scope_joined_count);
    register_host_function(ASYNC_SCOPE_FAILURES, std_async_scope_failures);
    register_host_function(ASYNC_STREAM_NEW, std_async_stream_new);
    register_host_function(ASYNC_STREAM_PUSH, std_async_stream_push);
    register_host_function(ASYNC_STREAM_DONE, std_async_stream_done);
    register_host_function(ASYNC_STREAM_NEXT, std_async_stream_next);
    register_host_function(ASYNC_STREAM_NEXT_STATUS, std_async_stream_next_status);
    register_host_function(ASYNC_STREAM_CANCEL, std_async_stream_cancel);
    register_host_function(ASYNC_STREAM_LEN, std_async_stream_len);
    register_host_function(ASYNC_STREAM_CAPACITY, std_async_stream_capacity);
    register_host_function(ASYNC_STREAM_MAP, std_async_stream_map);
    register_host_function(ASYNC_STREAM_FILTER, std_async_stream_filter);
    register_host_function(ASYNC_STREAM_FOLD, std_async_stream_fold);
    register_host_function(ASYNC_STREAM_TAKE, std_async_stream_take);
    register_host_function(ASYNC_STREAM_SKIP, std_async_stream_skip);
    register_host_function(ASYNC_STREAM_CHUNKS, std_async_stream_chunks);
    register_host_function(ASYNC_STREAM_FUSE, std_async_stream_fuse);
    register_host_function(ASYNC_FS_READ, std_async_fs_read);
    register_host_function(ASYNC_FS_WRITE, std_async_fs_write);
    register_host_function(ASYNC_TCP_LISTEN, std_async_tcp_listen);
    register_host_function(ASYNC_TCP_LISTENER_PORT, std_async_tcp_listener_port);
    register_host_function(ASYNC_TCP_CONNECT, std_async_tcp_connect);
    register_host_function(ASYNC_TCP_ACCEPT, std_async_tcp_accept);
    register_host_function(ASYNC_TCP_READ, std_async_tcp_read);
    register_host_function(ASYNC_TCP_WRITE, std_async_tcp_write);
    register_host_function(ASYNC_TCP_CLOSE, std_async_tcp_close);
    register_host_function(ASYNC_UDP_BIND, std_async_udp_bind);
    register_host_function(ASYNC_UDP_PORT, std_async_udp_port);
    register_host_function(ASYNC_UDP_SEND_TO, std_async_udp_send_to);
    register_host_function(ASYNC_UDP_RECV, std_async_udp_recv);
    register_host_function(ASYNC_UDP_CLOSE, std_async_udp_close);
    register_host_function(ASYNC_CHANNEL_NEW, std_async_channel_new);
    register_host_function(ASYNC_CHANNEL_SEND, std_async_channel_send);
    register_host_function(ASYNC_CHANNEL_RECV, std_async_channel_recv);
    register_host_function(ASYNC_CHANNEL_CLOSE, std_async_channel_close);
    register_host_function(ASYNC_CHANNEL_LEN, std_async_channel_len);
    register_host_function(ASYNC_REACTOR_BACKEND, std_async_reactor_backend);
    register_host_function(ASYNC_REACTOR_WAKE, std_async_reactor_wake);
    register_host_function(ASYNC_REACTOR_TIMER, std_async_reactor_timer);
    register_host_function(ASYNC_REACTOR_IO_REGISTER, std_async_reactor_io_register);
    register_host_function(ASYNC_REACTOR_IO_NOTIFY, std_async_reactor_io_notify);
    register_host_function(ASYNC_REACTOR_POLL, std_async_reactor_poll);
    register_host_function(ASYNC_REACTOR_LAST_KIND, std_async_reactor_last_kind);
    register_host_function(
        ASYNC_REACTOR_LAST_READINESS,
        std_async_reactor_last_readiness,
    );
    register_host_function(ASYNC_REACTOR_STATS_QUEUED, std_async_reactor_stats_queued);
    register_host_function(
        ASYNC_REACTOR_STATS_TASK_WAKEUPS,
        std_async_reactor_stats_task_wakeups,
    );
    register_host_function(
        ASYNC_REACTOR_STATS_TIMER_EVENTS,
        std_async_reactor_stats_timer_events,
    );
    register_host_function(
        ASYNC_REACTOR_STATS_IO_EVENTS,
        std_async_reactor_stats_io_events,
    );
    register_host_function(
        ASYNC_REACTOR_STATS_IO_REGISTRATIONS,
        std_async_reactor_stats_io_registrations,
    );
    register_host_function(ASYNC_REACTOR_RESET, std_async_reactor_reset);
}

fn register_serve() {
    register_host_function(SERVE_SERVER_NEW, std_serve_server_new);
    register_host_function(SERVE_SERVER_WARMUP, std_serve_server_warmup);
    register_host_function(SERVE_SERVER_IS_WARM, std_serve_server_is_warm);
    register_host_function(SERVE_SERVER_ENQUEUE, std_serve_server_enqueue);
    register_host_function(SERVE_SERVER_CANCEL, std_serve_server_cancel);
    register_host_function(SERVE_SERVER_PROCESS_BATCH, std_serve_server_process_batch);
    register_host_function(SERVE_SERVER_RESULT, std_serve_server_result);
    register_host_function(SERVE_SERVER_PENDING, std_serve_server_pending);
    register_host_function(SERVE_SERVER_SET_TIMEOUT, std_serve_server_set_timeout);
    register_host_function(SERVE_SERVER_RESIDENT_MODEL, std_serve_server_resident_model);
    register_host_function(SERVE_SERVER_BENCHMARK, std_serve_server_benchmark);
    register_host_function(
        SERVE_SERVER_SET_INPUT_POLICY,
        std_serve_server_set_input_policy,
    );
    register_host_function(
        SERVE_SERVER_SET_OUTPUT_POLICY,
        std_serve_server_set_output_policy,
    );
    register_host_function(SERVE_SERVER_SET_RATE_LIMIT, std_serve_server_set_rate_limit);
    register_host_function(SERVE_SERVER_SET_FALLBACK, std_serve_server_set_fallback);
    register_host_function(
        SERVE_SERVER_LAST_DIAGNOSTIC,
        std_serve_server_last_diagnostic,
    );
    register_host_function(SERVE_SERVER_AUDIT_LOG, std_serve_server_audit_log);
    register_host_function(
        SERVE_SERVER_SET_MODEL_VERSION,
        std_serve_server_set_model_version,
    );
    register_host_function(
        SERVE_SERVER_MONITORING_SNAPSHOT,
        std_serve_server_monitoring_snapshot,
    );
    register_host_function(
        SERVE_SERVER_DISTRIBUTION_SUMMARY,
        std_serve_server_distribution_summary,
    );
    register_host_function(SERVE_DRIFT_CHECK, std_serve_drift_check);
    register_host_function(SERVE_EXPORT_MONITORING, std_serve_export_monitoring);
    register_host_function(SERVE_RESET, std_serve_reset);
}

fn register_fs() {
    register_host_function(FS_READ, std_fs_read);
    register_host_function(FS_WRITE, std_fs_write);
    register_host_function(FS_APPEND, std_fs_append);
    register_host_function(FS_EXISTS, std_fs_exists);
    register_host_function(FS_REMOVE, std_fs_remove);
    register_host_function(FS_CREATE_DIR_ALL, std_fs_create_dir_all);
    register_host_function(FS_REMOVE_DIR, std_fs_remove_dir);
    register_host_function(FS_RENAME, std_fs_rename);
    register_host_function(FS_COPY, std_fs_copy);
    register_host_function(FS_READ_DIR, std_fs_read_dir);
    register_host_function(FS_READ_COMPAT, std_fs_compat_read);
    register_host_function(FS_WRITE_COMPAT, std_fs_compat_write);
    register_host_function(FS_APPEND_COMPAT, std_fs_compat_append);
    register_host_function(FS_EXISTS_COMPAT, std_fs_compat_exists);
    register_host_function(FS_REMOVE_COMPAT, std_fs_compat_remove);
}

fn register_env() {
    register_host_function(ENV_GET, std_env_get_option);
    register_host_function(ENV_GET_OPTION, std_env_get_option);
    register_host_function(ENV_SET, std_env_set);
    register_host_function(ENV_ARGS_COUNT, std_env_args_count);
    register_host_function(ENV_ARG, std_env_arg_option);
    register_host_function(ENV_ARG_OPTION, std_env_arg_option);
    register_host_function(ENV_GET_COMPAT, std_env_get);
    register_host_function(ENV_ARG_COMPAT, std_env_arg);
}

// ── StatsEmbed ───────────────────────────────────────────────────────────────
fn register_ml_text_embedding_model() {
    register_host_function(ML_TEXT_EMBED_MODEL_SESSION, std_ml_text_embed_model_session);
    register_host_function(ML_TEXT_EMBED_MODEL, std_ml_text_embed_model);
}

// ── ServeReal ────────────────────────────────────────────────────────────────
fn register_serve_real() {
    register_host_function(
        SERVE_SERVER_REGISTER_MODEL_LINEAR,
        std_serve_server_register_model_linear,
    );
    register_host_function(
        SERVE_SERVER_REGISTER_MODEL_ONNX,
        std_serve_server_register_model_onnx,
    );
    register_host_function(SERVE_SERVER_RESULT_VECTOR, std_serve_server_result_vector);
}
