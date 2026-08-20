// ── std.string string builder (R-3108) ──────────────────────────────────────
//
// Each builder function takes at least one argument (some a sentinel) so the
// parser produces a `Call` on a `FieldAccess` instead of a `MethodCall`. The
// midend currently has no representation for module/namespace types, and a
// `MethodCall` on a bare identifier falls through to `IRType::Int` and then
// fails to resolve a struct method. A `Call` on a qualified path routes
// through the existing qualified-call resolution path.

extern "C" fn std_string_builder_new(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let capacity_hint = unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        args[0].max(0) as usize
    };
    let memory = initialize().memory();
    let builder = match memory.allocate_manual(StringBuilder::new(capacity_hint)) {
        Ok(b) => b,
        Err(_) => return HOST_STATUS_INTERNAL_ERROR,
    };
    let handle = with_string_builder_registry(|reg| reg.insert(builder));
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = handle as SpectraHostValue;
        }
    }
    HOST_STATUS_SUCCESS
}

extern "C" fn std_string_builder_push(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let handle = args[0] as usize;
        let str_ptr = args[1];
        with_string_builder_registry(|reg| {
            let _ = reg.push_spectra_string(handle, str_ptr);
        });
    }
    HOST_STATUS_SUCCESS
}

extern "C" fn std_string_builder_len(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let result = unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let handle = args[0] as usize;
        with_string_builder_registry(|reg| match reg.len(handle) {
            Ok(n) => n as SpectraHostValue,
            Err(_) => -1,
        })
    };
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = result;
        }
    }
    HOST_STATUS_SUCCESS
}

extern "C" fn std_string_builder_finish(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let handle = args[0] as usize;
        let result = with_string_builder_registry(|reg| match reg.finish(handle) {
            Ok(s) => alloc_spectra_string(&s),
            Err(_) => 0,
        });
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = result;
        }
    }
    HOST_STATUS_SUCCESS
}

extern "C" fn std_string_builder_free(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let handle = args[0] as usize;
        with_string_builder_registry(|reg| {
            let _ = reg.discard(handle);
        });
    }
    HOST_STATUS_SUCCESS
}

extern "C" fn std_string_char_at(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let idx = args[1];
        let result = match read_spectra_string(args[0]) {
            Some(s) if idx >= 0 && (idx as usize) < s.len() => {
                s.as_bytes()[idx as usize] as SpectraHostValue
            }
            _ => -1,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = result;
        }
    }
    HOST_STATUS_SUCCESS
}

