use super::*;
// ── std.collections extras ──────────────────────────────────────────────────

pub(crate) fn write_option_result(
    ctx: &mut SpectraHostCallContext,
    value: Option<SpectraHostValue>,
) -> i32 {
    if ctx.result_len == 0 || ctx.results.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let (tag, payload) = match value {
        Some(value) => (0, value),
        None => (1, 0),
    };
    let handle = unsafe { alloc_tagged_payload(tag, payload) };
    if handle == 0 {
        return HOST_STATUS_INTERNAL_ERROR;
    }
    unsafe {
        let results = slice::from_raw_parts_mut(ctx.results, ctx.result_len);
        results[0] = handle;
    }
    HOST_STATUS_SUCCESS
}


pub(crate) extern "C" fn std_list_get_option(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let value = with_list_registry(|registry| registry.get_option(args[0] as usize, args[1]))
            .unwrap_or(None);
        write_option_result(ctx_ref, value)
    }
}

pub(crate) extern "C" fn std_list_set(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 3 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let handle = args[0] as usize;
        let index = args[1];
        let value = args[2];
        let _ = with_list_registry(|registry| registry.set(handle, index, value));
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = 0;
        }
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_list_contains(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let handle = args[0] as usize;
        let value = args[1];
        let found = with_list_registry(|registry| registry.contains(handle, value)).unwrap_or(false);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = found as SpectraHostValue;
    }
    HOST_STATUS_SUCCESS
}



pub(crate) extern "C" fn std_list_pop_option(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let value = with_list_registry(|registry| registry.pop_option(args[0] as usize))
            .unwrap_or(None);
        write_option_result(ctx_ref, value)
    }
}

pub(crate) extern "C" fn std_list_pop_front_option(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let value = with_list_registry(|registry| registry.pop_front_option(args[0] as usize))
            .unwrap_or(None);
        write_option_result(ctx_ref, value)
    }
}

pub(crate) extern "C" fn std_list_insert_at(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 3 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let handle = args[0] as usize;
        let index = args[1];
        let value = args[2];
        let _ = with_list_registry(|registry| registry.insert_at(handle, index, value));
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = 0;
        }
    }
    HOST_STATUS_SUCCESS
}


pub(crate) extern "C" fn std_list_remove_at_option(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let value = with_list_registry(|registry| {
            registry.remove_at_option(args[0] as usize, args[1])
        })
        .unwrap_or(None);
        write_option_result(ctx_ref, value)
    }
}

pub(crate) extern "C" fn std_list_index_of(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let handle = args[0] as usize;
        let value = args[1];
        let idx = with_list_registry(|registry| registry.index_of(handle, value)).unwrap_or(-1);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = idx;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_list_sort(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let handle = args[0] as usize;
        let _ = with_list_registry(|registry| registry.sort_asc(handle));
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = 0;
        }
    }
    HOST_STATUS_SUCCESS
}

