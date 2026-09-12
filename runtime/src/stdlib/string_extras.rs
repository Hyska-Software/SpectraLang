use super::*;
// ── std.string extras ────────────────────────────────────────────────────────

/// Returns a substring from `start` (inclusive) to `end` (exclusive).
///
/// Indices are BYTE offsets into the UTF-8 encoding of `s`, not character
/// offsets. Indices are clamped to `[0, byte_len]`. Returns an empty string
/// on invalid input: when `start > end`, or when either clamped index falls
/// inside a multi-byte UTF-8 sequence (not on a char boundary).
pub(crate) extern "C" fn std_string_substring(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 3 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let start = args[1];
        let end = args[2];
        let ptr = match read_spectra_string(args[0]) {
            Some(s) => {
                let len = s.len() as i64;
                let s_start = start.clamp(0, len) as usize;
                let s_end = end.clamp(0, len) as usize;
                let slice =
                    if s_start <= s_end && s.is_char_boundary(s_start) && s.is_char_boundary(s_end)
                    {
                        &s[s_start..s_end]
                    } else {
                        ""
                    };
                alloc_spectra_string(slice)
            }
            None => alloc_spectra_string(""),
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = ptr;
        }
    }
    HOST_STATUS_SUCCESS
}

/// Replaces all occurrences of `from` with `to` in `s`.
pub(crate) extern "C" fn std_string_replace(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 3 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let ptr = match (
            read_spectra_string(args[0]),
            read_spectra_string(args[1]),
            read_spectra_string(args[2]),
        ) {
            (Some(s), Some(from), Some(to)) => alloc_spectra_string(&s.replace(from.as_str(), &to)),
            (Some(s), _, _) => alloc_spectra_string(&s),
            _ => alloc_spectra_string(""),
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = ptr;
        }
    }
    HOST_STATUS_SUCCESS
}

/// Returns the byte index of the first occurrence of `sub` in `s`, or -1 if not found.
pub(crate) extern "C" fn std_string_index_of(ctx: *mut SpectraHostCallContext) -> i32 {
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
            (Some(s), Some(sub)) => match s.find(sub.as_str()) {
                Some(idx) => idx as SpectraHostValue,
                None => -1,
            },
            _ => -1,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = result;
        }
    }
    HOST_STATUS_SUCCESS
}

/// Returns the part of `s` before the first occurrence of `sep`.
/// Returns `s` unchanged if `sep` is not found.
pub(crate) extern "C" fn std_string_split_first(ctx: *mut SpectraHostCallContext) -> i32 {
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
            (Some(s), Some(sep)) => {
                let part = s.split(sep.as_str()).next().unwrap_or("");
                alloc_spectra_string(part)
            }
            (Some(s), _) => alloc_spectra_string(&s),
            _ => alloc_spectra_string(""),
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = ptr;
        }
    }
    HOST_STATUS_SUCCESS
}

/// Returns the part of `s` after the last occurrence of `sep`.
/// Returns empty string if `sep` is not found.
pub(crate) extern "C" fn std_string_split_last(ctx: *mut SpectraHostCallContext) -> i32 {
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
            (Some(s), Some(sep)) => {
                let part = s.rsplit(sep.as_str()).next().unwrap_or("");
                alloc_spectra_string(part)
            }
            _ => alloc_spectra_string(""),
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = ptr;
        }
    }
    HOST_STATUS_SUCCESS
}

/// Returns 1 if the string is empty (or null), 0 otherwise.
pub(crate) extern "C" fn std_string_is_empty(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let result = match read_spectra_string(args[0]) {
            Some(s) => s.is_empty() as SpectraHostValue,
            None => 1,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = result;
        }
    }
    HOST_STATUS_SUCCESS
}

/// Returns the number of non-overlapping occurrences of `sub` in `s`.
pub(crate) extern "C" fn std_string_count_occurrences(ctx: *mut SpectraHostCallContext) -> i32 {
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
            (Some(s), Some(sub)) if !sub.is_empty() => {
                s.matches(sub.as_str()).count() as SpectraHostValue
            }
            _ => 0,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = result;
        }
    }
    HOST_STATUS_SUCCESS
}
