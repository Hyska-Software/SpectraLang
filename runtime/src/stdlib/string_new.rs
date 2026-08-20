// ── std.string new functions ─────────────────────────────────────────────────

/// Splits `s` by `sep` and returns a list handle (int) whose elements are
/// string pointers (i64) for each part. Returns -1 on allocation failure.
extern "C" fn std_string_split_by(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        let (s, sep) = match (read_spectra_string(args[0]), read_spectra_string(args[1])) {
            (Some(s), Some(sep)) => (s, sep),
            _ => {
                results[0] = -1;
                return HOST_STATUS_SUCCESS;
            }
        };
        let memory = crate::initialize().memory();
        let list = match memory.allocate_manual(StdList::default()) {
            Ok(l) => l,
            Err(_) => return HOST_STATUS_INTERNAL_ERROR,
        };
        let handle = with_list_registry(|reg| reg.insert(list));
        for part in s.split(sep.as_str()) {
            let ptr = alloc_spectra_string(part);
            let _ = with_list_registry(|reg| reg.push(handle, ptr));
        }
        results[0] = handle as SpectraHostValue;
    }
    HOST_STATUS_SUCCESS
}

/// Pads `s` on the left with `pad_char` (Unicode code point) until the result
/// has `width` bytes. If `s` is already at or longer than `width`, returns `s`
/// unchanged.
extern "C" fn std_string_pad_left(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 3 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let width = args[1].max(0) as usize;
        let pad_ch = char::from_u32(args[2] as u32).unwrap_or(' ');
        let ptr = match read_spectra_string(args[0]) {
            Some(s) => {
                if s.len() >= width {
                    alloc_spectra_string(&s)
                } else {
                    let padding: String = std::iter::repeat_n(pad_ch, width - s.len()).collect();
                    alloc_spectra_string(&(padding + &s))
                }
            }
            None => alloc_spectra_string(""),
        };
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = ptr;
    }
    HOST_STATUS_SUCCESS
}

/// Pads `s` on the right with `pad_char` (Unicode code point) until the result
/// has `width` bytes. If `s` is already at or longer than `width`, returns `s`
/// unchanged.
extern "C" fn std_string_pad_right(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 3 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let width = args[1].max(0) as usize;
        let pad_ch = char::from_u32(args[2] as u32).unwrap_or(' ');
        let ptr = match read_spectra_string(args[0]) {
            Some(s) => {
                if s.len() >= width {
                    alloc_spectra_string(&s)
                } else {
                    let padding: String = std::iter::repeat_n(pad_ch, width - s.len()).collect();
                    alloc_spectra_string(&(s + &padding))
                }
            }
            None => alloc_spectra_string(""),
        };
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = ptr;
    }
    HOST_STATUS_SUCCESS
}

/// Returns a new string with the characters of `s` in reverse order.
extern "C" fn std_string_reverse(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let ptr = match read_spectra_string(args[0]) {
            Some(s) => alloc_spectra_string(&s.chars().rev().collect::<String>()),
            None => alloc_spectra_string(""),
        };
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = ptr;
    }
    HOST_STATUS_SUCCESS
}

