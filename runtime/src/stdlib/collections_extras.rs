use super::*;
// ── std.collections extras ──────────────────────────────────────────────────

pub(crate) fn write_option_result(
    ctx: &mut SpectraHostCallContext,
    value: Option<SpectraHostValue>,
) -> i32 {
    if ctx.result_len == 0 || ctx.results.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let handle = collection_option_handle(value);
    if handle == 0 {
        return HOST_STATUS_INTERNAL_ERROR;
    }
    unsafe {
        let results = slice::from_raw_parts_mut(ctx.results, ctx.result_len);
        results[0] = handle;
    }
    HOST_STATUS_SUCCESS
}

/// A `None` value has no payload and is immutable after construction. Keep a
/// process-owned representation for it so empty collection reads do not spend
/// an allocation and an allocation-table entry on every miss.
static NONE_OPTION: [SpectraHostValue; 2] = [1, 0];

pub(crate) fn none_option_handle() -> SpectraHostValue {
    NONE_OPTION.as_ptr() as SpectraHostValue
}

/// Materializes the tagged option representation used by the compiler.
/// `None` is borrowed from the process-wide immutable singleton; `Some` still
/// uses a tracked allocation because its payload can be an owned value.
pub(crate) fn collection_option_handle(value: Option<SpectraHostValue>) -> SpectraHostValue {
    match value {
        Some(payload) => {
            let handle = unsafe { alloc_tagged_payload(0, payload) };
            if handle == 0 {
                // Preserve a valid tagged representation under allocation
                // pressure. The generic ABI reports OOM; fast scalar paths
                // have no status/result pair, so a borrowed None is the safe
                // failure value instead of a null pointer that pattern
                // lowering cannot dereference.
                none_option_handle()
            } else {
                handle
            }
        }
        None => none_option_handle(),
    }
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
        crate::ffi::escape_stored_value(value);
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
        let found =
            with_list_registry(|registry| registry.contains(handle, value)).unwrap_or(false);
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
        let value =
            with_list_registry(|registry| registry.pop_option(args[0] as usize)).unwrap_or(None);
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
        crate::ffi::escape_stored_value(value);
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
        let value =
            with_list_registry(|registry| registry.remove_at_option(args[0] as usize, args[1]))
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
