use super::*;
// ── std.collections higher-order functions ──────────────────────────────────

/// `list_map(handle, fn_ptr) -> new_handle`
///
/// Creates a new list by applying the Spectra closure `fn_ptr(elem: int) -> int`
/// to every element of the source list.
pub(crate) extern "C" fn std_list_map(ctx: *mut SpectraHostCallContext) -> i32 {
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
            Some(f) => f,
            None => return HOST_STATUS_INTERNAL_ERROR,
        };
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let src_handle = args[0] as usize;
        let fn_ptr = args[1];

        // Snapshot source data in a single lock acquisition so the lock is not
        // held while calling back into JIT code.
        let src_data = match with_list_registry(|reg| reg.snapshot(src_handle)) {
            Ok(d) => d,
            Err(code) => return code,
        };

        // Allocate the destination list.
        let memory = initialize().memory();
        let dest_list = match memory.allocate_manual(StdList::default()) {
            Ok(l) => l,
            Err(_) => return HOST_STATUS_INTERNAL_ERROR,
        };
        let dest_handle = with_list_registry(|reg| reg.insert(dest_list));

        for &elem in &src_data {
            let arg_buf = [elem];
            let mut out = 0i64;
            let status = invoke(fn_ptr, arg_buf.as_ptr(), 1, &mut out);
            if status != HOST_STATUS_SUCCESS {
                let _ = with_list_registry(|reg| reg.remove(dest_handle));
                return status;
            }
            if let Err(code) = with_list_registry(|reg| reg.push(dest_handle, out)) {
                let _ = with_list_registry(|reg| reg.remove(dest_handle));
                return code;
            }
        }

        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = dest_handle as SpectraHostValue;
    }
    HOST_STATUS_SUCCESS
}

/// `list_filter(handle, fn_ptr) -> new_handle`
///
/// Creates a new list containing only the elements for which the Spectra closure
/// `fn_ptr(elem: int) -> int` returns a non-zero (truthy) value.
pub(crate) extern "C" fn std_list_filter(ctx: *mut SpectraHostCallContext) -> i32 {
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
            Some(f) => f,
            None => return HOST_STATUS_INTERNAL_ERROR,
        };
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let src_handle = args[0] as usize;
        let fn_ptr = args[1];

        let src_data = match with_list_registry(|reg| reg.snapshot(src_handle)) {
            Ok(d) => d,
            Err(code) => return code,
        };

        let memory = initialize().memory();
        let dest_list = match memory.allocate_manual(StdList::default()) {
            Ok(l) => l,
            Err(_) => return HOST_STATUS_INTERNAL_ERROR,
        };
        let dest_handle = with_list_registry(|reg| reg.insert(dest_list));

        for &elem in &src_data {
            let arg_buf = [elem];
            let mut out = 0i64;
            let status = invoke(fn_ptr, arg_buf.as_ptr(), 1, &mut out);
            if status != HOST_STATUS_SUCCESS {
                let _ = with_list_registry(|reg| reg.remove(dest_handle));
                return status;
            }
            // Bool closures use the narrow language ABI while the generic
            // callback slot is an i64. Consume only the canonical low byte so
            // an unspecified upper part of a widened bool cannot turn false
            // into a truthy predicate.
            if (out & 0xff) != 0 {
                if let Err(code) = with_list_registry(|reg| reg.push(dest_handle, elem)) {
                    let _ = with_list_registry(|reg| reg.remove(dest_handle));
                    return code;
                }
            }
        }

        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = dest_handle as SpectraHostValue;
    }
    HOST_STATUS_SUCCESS
}

/// `list_reduce(handle, initial, fn_ptr) -> int`
///
/// Folds the list left-to-right using `fn_ptr(accumulator: int, elem: int) -> int`,
/// starting with `initial` as the accumulator. Returns the final accumulator value.
pub(crate) extern "C" fn std_list_reduce(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 3 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let invoke = match ctx_ref.invoke_fn {
            Some(f) => f,
            None => return HOST_STATUS_INTERNAL_ERROR,
        };
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let src_handle = args[0] as usize;
        let mut accumulator = args[1];
        let fn_ptr = args[2];

        let src_data = match with_list_registry(|reg| reg.snapshot(src_handle)) {
            Ok(d) => d,
            Err(code) => return code,
        };

        for &elem in &src_data {
            let arg_buf = [accumulator, elem];
            let mut out = 0i64;
            let status = invoke(fn_ptr, arg_buf.as_ptr(), 2, &mut out);
            if status != HOST_STATUS_SUCCESS {
                return status;
            }
            accumulator = out;
        }

        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = accumulator;
    }
    HOST_STATUS_SUCCESS
}

/// `list_sort_by(handle, fn_ptr) -> unit`
///
/// Sorts the list in-place using the Spectra comparator closure
/// `fn_ptr(a: int, b: int) -> int` (negative ⇒ a < b, 0 ⇒ equal, positive ⇒ a > b).
pub(crate) extern "C" fn std_list_sort_by(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let invoke = match ctx_ref.invoke_fn {
            Some(f) => f,
            None => return HOST_STATUS_INTERNAL_ERROR,
        };
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let handle = args[0] as usize;
        let fn_ptr = args[1];

        // Snapshot, sort outside the lock, then restore.
        let mut data = match with_list_registry(|reg| reg.snapshot(handle)) {
            Ok(d) => d,
            Err(code) => return code,
        };

        // Use a cell to propagate callback errors out of the sort closure.
        let mut callback_err: i32 = HOST_STATUS_SUCCESS;
        data.sort_by(|&a, &b| {
            if callback_err != HOST_STATUS_SUCCESS {
                return std::cmp::Ordering::Equal;
            }
            let arg_buf = [a, b];
            let mut out = 0i64;
            let status = invoke(fn_ptr, arg_buf.as_ptr(), 2, &mut out);
            if status != HOST_STATUS_SUCCESS {
                callback_err = status;
                return std::cmp::Ordering::Equal;
            }
            out.cmp(&0)
        });
        if callback_err != HOST_STATUS_SUCCESS {
            return callback_err;
        }

        if let Err(code) = with_list_registry(|reg| reg.restore(handle, data)) {
            return code;
        }
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = 0;
        }
    }
    HOST_STATUS_SUCCESS
}
