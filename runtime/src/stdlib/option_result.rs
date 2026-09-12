//! Typed `Option<T>` and `Result<T, E>` host operations.
//!
//! The runtime representation is intentionally small and explicit: a tagged
//! two-word allocation with the tag in slot zero and the payload in slot one.
//! The parent `stdlib` module owns allocation and host ABI helpers; this
//! module owns only the ADT operations and their registrations.

use super::*;

pub(crate) const OPTION_IS_SOME: &str = "spectra.std.option.is_some";
pub(crate) const OPTION_IS_NONE: &str = "spectra.std.option.is_none";
pub(super) const OPTION_UNWRAP: &str = "spectra.std.option.option_unwrap";
pub(crate) const OPTION_UNWRAP_OR: &str = "spectra.std.option.option_unwrap_or";
pub(crate) const OPTION_MAP: &str = "spectra.std.option.option_map";

pub(crate) const RESULT_IS_OK: &str = "spectra.std.result.is_ok";
pub(crate) const RESULT_IS_ERR: &str = "spectra.std.result.is_err";
pub(super) const RESULT_UNWRAP: &str = "spectra.std.result.result_unwrap";
pub(crate) const RESULT_UNWRAP_OR: &str = "spectra.std.result.result_unwrap_or";
pub(super) const RESULT_UNWRAP_ERR: &str = "spectra.std.result.result_unwrap_err";
pub(crate) const RESULT_MAP: &str = "spectra.std.result.result_map";
pub(crate) const RESULT_MAP_ERR: &str = "spectra.std.result.result_map_err";

pub(super) fn register() {
    register_host_function(OPTION_IS_SOME, std_option_is_some);
    register_host_function(OPTION_IS_NONE, std_option_is_none);
    register_host_function(OPTION_UNWRAP, std_option_unwrap);
    register_host_function(OPTION_UNWRAP_OR, std_option_unwrap_or);
    register_host_function(OPTION_MAP, std_option_map);
    register_host_function(RESULT_IS_OK, std_result_is_ok);
    register_host_function(RESULT_IS_ERR, std_result_is_err);
    register_host_function(RESULT_UNWRAP, std_result_unwrap);
    register_host_function(RESULT_UNWRAP_OR, std_result_unwrap_or);
    register_host_function(RESULT_UNWRAP_ERR, std_result_unwrap_err);
    register_host_function(RESULT_MAP, std_result_map);
    register_host_function(RESULT_MAP_ERR, std_result_map_err);
}

pub(crate) extern "C" fn std_option_is_some(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let ptr = args[0] as *const i64;
        let tag = if ptr.is_null() { 1i64 } else { *ptr };
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = (tag == 0) as SpectraHostValue;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_option_is_none(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let ptr = args[0] as *const i64;
        let tag = if ptr.is_null() { 1i64 } else { *ptr };
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = (tag != 0) as SpectraHostValue;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_option_unwrap(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let ptr = args[0] as *const i64;
        if ptr.is_null() || *ptr != 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = *ptr.add(1);
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_option_unwrap_or(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let ptr = args[0] as *const i64;
        let default_val = args[1];
        let tag = if ptr.is_null() { 1i64 } else { *ptr };
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = if tag == 0 { *ptr.add(1) } else { default_val };
    }
    HOST_STATUS_SUCCESS
}

/// Maps the payload of `Option<T>` without turning `None` into a sentinel.
pub(crate) extern "C" fn std_option_map(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let invoke = match ctx_ref.invoke_fn {
            Some(invoke) => invoke,
            None => return HOST_STATUS_INTERNAL_ERROR,
        };
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let option_ptr = args[0] as *const i64;
        let fn_ptr = args[1];
        let mut mapped = 0i64;
        let tag = if option_ptr.is_null() { 1 } else { *option_ptr };
        if tag == 0 {
            let input = [*option_ptr.add(1)];
            let status = invoke(fn_ptr, input.as_ptr(), input.len(), &mut mapped);
            if status != HOST_STATUS_SUCCESS {
                return status;
            }
        }
        let value = alloc_tagged_payload(tag, mapped);
        if value == 0 {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = value;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_result_is_ok(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let ptr = args[0] as *const i64;
        let tag = if ptr.is_null() { 1i64 } else { *ptr };
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = (tag == 0) as SpectraHostValue;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_result_is_err(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let ptr = args[0] as *const i64;
        let tag = if ptr.is_null() { 1i64 } else { *ptr };
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = (tag != 0) as SpectraHostValue;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_result_unwrap(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let ptr = args[0] as *const i64;
        if ptr.is_null() || *ptr != 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = *ptr.add(1);
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_result_unwrap_or(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let ptr = args[0] as *const i64;
        let default_val = args[1];
        let tag = if ptr.is_null() { 1i64 } else { *ptr };
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = if tag == 0 { *ptr.add(1) } else { default_val };
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_result_unwrap_err(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let ptr = args[0] as *const i64;
        if ptr.is_null() || *ptr == 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = *ptr.add(1);
    }
    HOST_STATUS_SUCCESS
}

/// Maps only the `Ok` payload of `Result<T, E>` and preserves `Err(E)`.
pub(crate) extern "C" fn std_result_map(ctx: *mut SpectraHostCallContext) -> i32 {
    map_result_payload(ctx, true)
}

/// Maps only the `Err` payload of `Result<T, E>` and preserves `Ok(T)`.
pub(crate) extern "C" fn std_result_map_err(ctx: *mut SpectraHostCallContext) -> i32 {
    map_result_payload(ctx, false)
}

pub(crate) fn map_result_payload(ctx: *mut SpectraHostCallContext, map_ok: bool) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let invoke = match ctx_ref.invoke_fn {
            Some(invoke) => invoke,
            None => return HOST_STATUS_INTERNAL_ERROR,
        };
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let result_ptr = args[0] as *const i64;
        let fn_ptr = args[1];
        let tag = if result_ptr.is_null() { 1 } else { *result_ptr };
        let mut payload = if result_ptr.is_null() {
            0
        } else {
            *result_ptr.add(1)
        };
        if (map_ok && tag == 0) || (!map_ok && tag != 0) {
            let input = [payload];
            let status = invoke(fn_ptr, input.as_ptr(), input.len(), &mut payload);
            if status != HOST_STATUS_SUCCESS {
                return status;
            }
        }
        let value = alloc_tagged_payload(tag, payload);
        if value == 0 {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = value;
    }
    HOST_STATUS_SUCCESS
}
