use super::*;
// ── std.char register & host functions ──────────────────────────────────────

pub(crate) fn register_char() {
    register_host_function(CHAR_IS_ALPHA, std_char_is_alpha);
    register_host_function(CHAR_IS_DIGIT, std_char_is_digit);
    register_host_function(CHAR_IS_WHITESPACE, std_char_is_whitespace);
    register_host_function(CHAR_IS_UPPER, std_char_is_upper);
    register_host_function(CHAR_IS_LOWER, std_char_is_lower);
    register_host_function(CHAR_TO_UPPER, std_char_to_upper);
    register_host_function(CHAR_TO_LOWER, std_char_to_lower);
    register_host_function(CHAR_IS_ALPHANUMERIC, std_char_is_alphanumeric);
}

pub(crate) extern "C" fn std_char_is_alpha(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        let v = char::from_u32(args[0] as u32)
            .map(|c| c.is_alphabetic())
            .unwrap_or(false);
        results[0] = v as i64;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_char_is_digit(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        let v = char::from_u32(args[0] as u32)
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false);
        results[0] = v as i64;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_char_is_whitespace(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        let v = char::from_u32(args[0] as u32)
            .map(|c| c.is_whitespace())
            .unwrap_or(false);
        results[0] = v as i64;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_char_is_upper(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        let v = char::from_u32(args[0] as u32)
            .map(|c| c.is_uppercase())
            .unwrap_or(false);
        results[0] = v as i64;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_char_is_lower(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        let v = char::from_u32(args[0] as u32)
            .map(|c| c.is_lowercase())
            .unwrap_or(false);
        results[0] = v as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Returns the uppercase version of the Unicode code point `c`.
pub(crate) extern "C" fn std_char_to_upper(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        let upper = char::from_u32(args[0] as u32)
            .and_then(|c| c.to_uppercase().next())
            .unwrap_or(char::from_u32(args[0] as u32).unwrap_or('\0'));
        results[0] = upper as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Returns the lowercase version of the Unicode code point `c`.
pub(crate) extern "C" fn std_char_to_lower(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        let lower = char::from_u32(args[0] as u32)
            .and_then(|c| c.to_lowercase().next())
            .unwrap_or(char::from_u32(args[0] as u32).unwrap_or('\0'));
        results[0] = lower as i64;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_char_is_alphanumeric(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        let v = char::from_u32(args[0] as u32)
            .map(|c| c.is_alphanumeric())
            .unwrap_or(false);
        results[0] = v as i64;
    }
    HOST_STATUS_SUCCESS
}
