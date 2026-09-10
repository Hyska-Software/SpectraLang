use super::*;
// ── std.string host functions ────────────────────────────────────────────────

pub(crate) extern "C" fn std_string_len(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let len = match read_spectra_string(args[0]) {
            Some(s) => s.len() as SpectraHostValue,
            None => 0,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = len;
        }
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_string_contains(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let result = match (read_spectra_string(args[0]), read_spectra_string(args[1])) {
            (Some(s), Some(sub)) => s.contains(sub.as_str()) as SpectraHostValue,
            _ => 0,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = result;
        }
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_string_to_upper(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let ptr = match read_spectra_string(args[0]) {
            Some(s) => alloc_spectra_string(&s.to_uppercase()),
            None => 0,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = ptr;
        }
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_string_to_lower(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let ptr = match read_spectra_string(args[0]) {
            Some(s) => alloc_spectra_string(&s.to_lowercase()),
            None => 0,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = ptr;
        }
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_string_trim(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let ptr = match read_spectra_string(args[0]) {
            Some(s) => alloc_spectra_string(s.trim()),
            None => 0,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = ptr;
        }
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_string_starts_with(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let result = match (read_spectra_string(args[0]), read_spectra_string(args[1])) {
            (Some(s), Some(prefix)) => s.starts_with(prefix.as_str()) as SpectraHostValue,
            _ => 0,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = result;
        }
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_string_ends_with(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let result = match (read_spectra_string(args[0]), read_spectra_string(args[1])) {
            (Some(s), Some(suffix)) => s.ends_with(suffix.as_str()) as SpectraHostValue,
            _ => 0,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = result;
        }
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_string_eq(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let result = match (read_spectra_string(args[0]), read_spectra_string(args[1])) {
            (Some(left), Some(right)) => (left == right) as SpectraHostValue,
            _ => 0,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = result;
        }
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_string_concat(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let ptr = match (read_spectra_string(args[0]), read_spectra_string(args[1])) {
            (Some(a), Some(b)) => alloc_spectra_string(&(a + &b)),
            (Some(a), None) => alloc_spectra_string(&a),
            (None, Some(b)) => alloc_spectra_string(&b),
            (None, None) => alloc_spectra_string(""),
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = ptr;
        }
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_string_repeat(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let n = args[1].max(0) as usize;
        let ptr = match read_spectra_string(args[0]) {
            Some(s) => {
                // Guard against `capacity overflow` panics for absurd repeat
                // counts: fall back to the empty string when the requested
                // size is not even addressable.
                match s
                    .len()
                    .checked_mul(n)
                    .filter(|&total| total <= isize::MAX as usize)
                {
                    Some(_) => alloc_spectra_string(&s.repeat(n)),
                    None => alloc_spectra_string(""),
                }
            }
            None => 0,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = ptr;
        }
    }
    HOST_STATUS_SUCCESS
}
