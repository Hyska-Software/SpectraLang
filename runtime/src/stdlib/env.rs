use super::*;
// ── std.env host functions ───────────────────────────────────────────────────


pub(crate) extern "C" fn std_env_get_option(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let key = match read_spectra_string(args[0]) {
            Some(key) => key,
            None => return HOST_STATUS_INVALID_ARGUMENT,
        };
        let value = std::env::var(&key)
            .ok()
            .map(|value| alloc_spectra_string(&value));
        if value == Some(0) {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        write_option_result(ctx_ref, value)
    }
}

pub(crate) extern "C" fn std_env_set(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let key = match read_spectra_string(args[0]) {
            Some(k) => k,
            None => return HOST_STATUS_INVALID_ARGUMENT,
        };
        let value = read_spectra_string(args[1]).unwrap_or_default();
        std::env::set_var(&key, &value);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = 1;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_env_args_count(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        // Use explicitly forwarded program args when available (JIT runner sets
        // these via spectra_runtime::set_program_args; AOT executables use
        // spectra_rt_startup_with_args). Fall back to std::env::args otherwise.
        let count = if let Some(args) = crate::ffi::get_program_args() {
            args.len() as i64
        } else {
            std::env::args().count() as i64
        };
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = count;
    }
    HOST_STATUS_SUCCESS
}


pub(crate) extern "C" fn std_env_arg_option(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let index = args[0] as usize;
        let value = if let Some(program_args) = crate::ffi::get_program_args() {
            program_args
                .get(index)
                .map(|value| alloc_spectra_string(value))
        } else {
            std::env::args()
                .nth(index)
                .map(|value| alloc_spectra_string(&value))
        };
        if value == Some(0) {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        write_option_result(ctx_ref, value)
    }
}

