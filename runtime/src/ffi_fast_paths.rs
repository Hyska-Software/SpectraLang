// Spectra string representation (packed bytes).
//
// Every string crossing the runtime ABI — host-call arguments/results,
// JIT/AOT `ConstString` literals, panic messages, tracing attributes — is a
// **packed UTF-8 buffer**: one byte per byte, terminated by a single NUL
// byte (`len + 1` bytes total). Pointers still travel as `i64`, so external
// signatures are unchanged; only the layout under the pointer is packed.
//
// Scans are bounded by the manual AllocationTable when the pointer is
// tracked, otherwise by `SPECTRA_STRING_SCAN_LIMIT` bytes.

/// Registers the built-in standard library host calls.
#[no_mangle]
pub extern "C" fn spectra_rt_std_register() {
    crate::stdlib::register();
}

/// Begins a manual allocation frame and returns its identifier.
///
/// The id is allocated in the global frame registry (under the table lock)
/// *and* pushed onto the **calling thread's** frame stack: a frame id is owned
/// by the thread that entered it, and only that thread may exit it. See
/// [`spectra_rt_manual_frame_exit`].
#[no_mangle]
pub extern "C" fn spectra_rt_manual_frame_enter() -> usize {
    let table = allocation_table();
    let mut guard = table.lock().unwrap_or_else(|e| e.into_inner());
    let id = guard.push_frame();
    with_thread_frames(&guard, |frames| frames.push(id));
    id
}

/// Ends a manual allocation frame, freeing all allocations created since it began.
///
/// # Frame ownership
///
/// Only frames on the **calling thread's** stack are popped: `frame_id` plus
/// every frame this thread opened above it. A `frame_id` that this thread does
/// not own (unknown, stale after `spectra_rt_manual_clear`, or owned by
/// another thread) frees **nothing** and is reported through
/// `spectra_rt_manual_frame_exit_last_status` as
/// [`HOST_STATUS_INVALID_ARGUMENT`] — mirroring the `spectra_rt_manual_free`
/// convention. This is what keeps thread B's `frame_exit` from draining and
/// quarantining thread A's live frames (a use-after-free once the quarantine
/// evicts the tombstones).
#[no_mangle]
pub extern "C" fn spectra_rt_manual_frame_exit(frame_id: usize) {
    let table = allocation_table();
    let mut guard = table.lock().unwrap_or_else(|e| e.into_inner());

    // Split the calling thread's own stack at `frame_id`: everything from that
    // position upward (inclusive) belongs to this exit. Not found → not ours.
    let own_frames = with_thread_frames(&guard, |frames| {
        frames
            .iter()
            .rposition(|&id| id == frame_id)
            .map(|position| frames.split_off(position))
            .unwrap_or_default()
    });
    if own_frames.is_empty() {
        // Base frame (0), an unknown id, a stale id, or another thread's
        // frame: free nothing.
        crate::ffi::record_manual_frame_exit_status(crate::ffi::HOST_STATUS_INVALID_ARGUMENT);
        return;
    }
    crate::ffi::record_manual_frame_exit_status(crate::ffi::HOST_STATUS_SUCCESS);

    // Drain exactly these frames from the global registry and free their
    // pointers through the quarantine-aware path.
    for id in own_frames {
        for ptr in guard.take_frame(id) {
            // Quarantine-aware free: leaves a tombstone so a stale pointer to
            // frame-local memory cannot silently free an unrelated object
            // after address reuse.
            guard.free_tracked(ptr);
        }
    }
}

/// Allocates zero-initialised manual memory tracked by the runtime.
#[no_mangle]
pub extern "C" fn spectra_rt_manual_alloc(size: usize) -> *mut u8 {
    if size == 0 {
        return ptr::null_mut();
    }

    let state = initialize();
    let memory = state.memory();
    let mut allocation = match memory.allocate_manual_bytes(size) {
        Ok(allocation) => allocation,
        Err(_) => return ptr::null_mut(),
    };

    let ptr = allocation.as_mut().as_mut_ptr();

    let ptr_value = ptr as usize;

    let table = allocation_table();
    let mut guard = table.lock().unwrap_or_else(|e| e.into_inner());

    // Attach to the top of the *calling thread's* frame stack (base frame 0
    // when the thread has no open frame) — never to the globally newest frame,
    // which may belong to a different thread. Re-validated against the global
    // registry so a stale stack entry cannot orphan the allocation.
    let frame_id = current_thread_frame_id(&guard).unwrap_or(0);

    guard.allocations.insert(
        ptr_value,
        ManualAllocation {
            frame_id,
            _storage: allocation,
        },
    );

    if let Some(frame) = guard.frames.iter_mut().find(|frame| frame.id == frame_id) {
        frame.track(ptr_value);
    }

    ptr
}

const SPECTRA_STRING_SCAN_LIMIT: usize = 16 * 1024 * 1024;

/// Returns the size in bytes recorded by the manual AllocationTable for the
/// allocation starting at `ptr_val`, or `None` when the pointer is unknown
/// (e.g. memory not allocated through `spectra_rt_manual_alloc`).
pub(crate) fn manual_allocation_size(ptr_val: SpectraHostValue) -> Option<usize> {
    let table = allocation_table();
    let guard = table.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(allocation) = guard.allocations.get(&(ptr_val as usize)) {
        return Some(allocation._storage.len());
    }
    guard.literal_len(ptr_val as usize)
}

/// Registers an image-owned string literal (`.rodata` in an AOT build) so the
/// runtime reads it as a string while still owning nothing: registered
/// literals are never freed and never counted as leaks.
#[no_mangle]
pub extern "C" fn spectra_rt_register_literal(ptr: i64, len: i64) {
    let (Ok(ptr), Ok(len)) = (usize::try_from(ptr), usize::try_from(len)) else {
        return;
    };
    let table = allocation_table();
    let mut guard = table.lock().unwrap_or_else(|e| e.into_inner());
    guard.register_literal(ptr, len);
}

/// Returns the scan ceiling in BYTES for a string pointer: the exact size
/// of the tracked allocation when known, otherwise the conservative global
/// scan limit.
///
/// The untracked fallback (`SPECTRA_STRING_SCAN_LIMIT`, 16 MiB) is a **policy
/// limit, not a validity guarantee**: it only bounds how far a scan may run,
/// it does not prove the memory is mapped or that the object really is that
/// large. For an untracked pointer the scan may still read past the true
/// object (residual risk documented in `spectra_rt_string_len`); callers must
/// only ever pass Spectra strings (NUL-terminated inside their allocation).
/// JIT string literals are allocated through `spectra_rt_manual_alloc` and
/// AOT images register theirs via `spectra_rt_register_literal`, so in
/// practice every runtime-produced string is tracked and scanned exactly.
pub(crate) fn string_scan_limit_bytes(ptr_val: SpectraHostValue) -> usize {
    match manual_allocation_size(ptr_val) {
        Some(bytes) => bytes,
        None => SPECTRA_STRING_SCAN_LIMIT,
    }
}

/// Fast ABI entry for `std.string.len`.
///
/// Spectra strings are packed UTF-8 byte buffers terminated by a single NUL
/// byte. This keeps the hot path out of the generic host-call dispatcher
/// while preserving the same null handling as the stdlib host function.
///
/// Returns:
/// - `0` for a null pointer (the empty string), matching the stdlib contract;
/// - the byte length when a NUL terminator is found inside the scan bound;
/// - `-1` when **no terminator exists within the scan bound** — the pointer
///   does not describe a valid Spectra string. This follows the fast-ABI
///   convention of negative sentinels for error statuses (like
///   `spectra_rt_string_char_at`'s `-1`); the historical `0` conflated
///   "empty" with "unterminated".
///
/// # Bound semantics (residual risk)
///
/// When the pointer is tracked by the AllocationTable, only bytes inside that
/// allocation are read. Otherwise the scan is capped at
/// [`SPECTRA_STRING_SCAN_LIMIT`] bytes — a **policy limit, not a validity
/// guarantee**: the bound stops runaway scans but cannot prove the untracked
/// memory is mapped for its whole extent, so a garbage pointer can still fault
/// inside the window (residual risk; production strings are tracked — see
/// [`string_scan_limit_bytes`]).
#[no_mangle]
pub extern "C" fn spectra_rt_string_len(ptr_val: SpectraHostValue) -> SpectraHostValue {
    if ptr_val == 0 {
        return 0;
    }

    let raw = ptr_val as *const u8;
    let limit = string_scan_limit_bytes(ptr_val);
    for offset in 0..limit {
        // SAFETY: `offset` stays within the tracked allocation when the
        // pointer is tracked. For an untracked pointer `limit` is a policy
        // cap, not proof of validity — see the residual-risk note above.
        let byte = unsafe { *raw.add(offset) };
        if byte == 0 {
            return offset as SpectraHostValue;
        }
    }

    // No terminator inside the scan bound: report the unterminated pointer
    // as an error instead of the historical `0` (which said "empty").
    -1
}

/// Returns -1 for null strings, negative indexes, and indexes at or after the
/// null terminator, matching the public stdlib contract.
///
/// The string length is derived with a bounded scan (see
/// [`spectra_rt_string_len`], including its residual-risk note: the
/// untracked-pointer cap is a policy limit, not a validity guarantee)
/// BEFORE the indexed byte is dereferenced, so an out-of-range index never
/// touches memory beyond the terminator.
#[no_mangle]
pub extern "C" fn spectra_rt_string_char_at(
    ptr_val: SpectraHostValue,
    index: SpectraHostValue,
) -> SpectraHostValue {
    if ptr_val == 0 || index < 0 {
        return -1;
    }

    let target = index as usize;
    let raw = ptr_val as *const u8;
    let limit = string_scan_limit_bytes(ptr_val);

    // Derive the length first without ever reading past the scan limit.
    let mut len = 0usize;
    while len < limit {
        // SAFETY: tracked pointers stay inside their allocation; untracked
        // pointers are bounded by a policy limit only — see the residual-risk
        // note on `spectra_rt_string_len`.
        let byte = unsafe { *raw.add(len) };
        if byte == 0 {
            break;
        }
        len += 1;
    }

    if target >= len {
        return -1;
    }
    // SAFETY: target < len <= limit, inside the region the scan already read.
    let byte = unsafe { *raw.add(target) };
    byte as SpectraHostValue
}



/// Fast ABI entry for `concurrent.task_spawn_fn(closure, arg)`.
///
/// Skips the generic host-call dispatch. Called directly from JIT code when
/// the backend inlines the `task_spawn_fn` call.
///
/// Returns the new task_id (>0 on success) or 0 on internal error (poisoned
/// registry mutex or null closure pointer). Task 0 is the invalid sentinel.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_concurrent_spawn_fn_fast(
    fn_ptr: SpectraHostValue,
    arg: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::concurrent_spawn_fn_fast(fn_ptr, arg)
}

/// Fast ABI entry for `concurrent.task_join(task_id)`.
///
/// Skips the generic host-call dispatch. Called directly from JIT code when
/// the backend inlines the `task_join` call.
///
/// Returns the value written by the matching `task_spawn`, or 0 if the
/// task_id is invalid (out of range, recycled, or never existed).
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_concurrent_join_fast(task_id: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::concurrent_join_fast(task_id)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_concurrent_spawn_batch_fast(
    first_value: SpectraHostValue,
    count: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::concurrent_spawn_batch_fast(first_value, count)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_concurrent_join_batch_sum_fast(
    batch_id: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::concurrent_join_batch_sum_fast(batch_id)
}


/// Fast ABI entry for `concurrent.reset()`.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_concurrent_reset_fast() -> SpectraHostValue {
    crate::stdlib::concurrent_reset_fast()
}

/// Fast ABI entry for `str.builder_new(capacity)`.
///
/// Skips the generic host-call dispatch. Called directly from JIT code
/// when the backend inlines the `builder_new` call.
///
/// Returns the new builder handle (>0 on success) or 0 on internal error.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_builder_new(capacity: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::string_builder_new_fast(capacity as usize)
}

/// Fast ABI entry for `str.builder_push(handle, str_ptr)`.
///
/// Skips the generic host-call dispatch and the intermediate `String`
/// allocation in `read_spectra_string`. Reads the Spectra string bytes
/// directly into the builder buffer. Called directly from JIT code when
/// the backend inlines the `builder_push` call.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_builder_push(handle: SpectraHostValue, str_ptr: SpectraHostValue) {
    crate::stdlib::string_builder_push_fast(handle as usize, str_ptr)
}

/// Fast ABI entry for `str.builder_len(handle)`.
///
/// Skips the generic host-call dispatch. Returns the current byte count
/// of the builder, or 0 for an invalid handle.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_builder_len(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::string_builder_len_fast(handle as usize)
}

/// Fast ABI entry for `str.builder_finish(handle)`.
///
/// Skips the generic host-call dispatch. Returns a Spectra string handle
/// for the accumulated bytes, or 0 for an invalid handle.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_builder_finish(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::string_builder_finish_fast(handle as usize)
}

/// Fast ABI entry for `str.builder_free(handle)`.
///
/// Skips the generic host-call dispatch. Frees the builder resources.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_builder_free(handle: SpectraHostValue) {
    crate::stdlib::string_builder_free_fast(handle as usize)
}

/// Fast ABI entry for `col.map_set(handle, key, value)`.
///
/// Skips the generic host-call dispatch. Called directly from JIT code
/// when the backend inlines the `map_set` call. Returns `HOST_STATUS_SUCCESS`
/// (0) on success or `HOST_STATUS_NOT_FOUND` if the handle is invalid.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_set_fast(
    handle: SpectraHostValue,
    key: SpectraHostValue,
    value: SpectraHostValue,
) -> i32 {
    crate::stdlib::map_set_fast(handle as usize, key, value)
}

/// Scalar-only map insertion used by typed collection lowering. It skips the
/// allocation-table probe required by the polymorphic entry point.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_set_scalar_fast(
    handle: SpectraHostValue,
    key: SpectraHostValue,
    value: SpectraHostValue,
) -> i32 {
    crate::stdlib::map_set_scalar_fast(handle as usize, key, value)
}


/// Fast ABI entry for `col.map_contains(handle, key)`.
///
/// Skips the generic host-call dispatch. Returns 1 if the key is present
/// in the map, 0 otherwise (including invalid handle).
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_contains_fast(
    handle: SpectraHostValue,
    key: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::map_contains_fast(handle as usize, key)
}

/// Scalar-only map membership check used by typed collection lowering.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_contains_scalar_fast(
    handle: SpectraHostValue,
    key: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::map_contains_scalar_fast(handle as usize, key)
}

/// Fast ABI entry for `col.map_new()`.
///
/// Creates a new empty map and returns its handle (>0) or 0 on internal
/// error (poisoned registry mutex).
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_new_fast() -> SpectraHostValue {
    crate::stdlib::map_new_fast()
}


/// Fast ABI entry for `col.map_len(handle)`.
///
/// Skips the generic host-call dispatch. Returns the number of entries
/// in the map, or 0 for an invalid handle.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_len_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::map_len_fast(handle as usize)
}

/// Fast ABI entry for `col.map_clear(handle)`.
///
/// Skips the generic host-call dispatch. Removes all entries from the
/// map. No-op for an invalid handle.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_clear_fast(handle: SpectraHostValue) {
    crate::stdlib::map_clear_fast(handle as usize)
}

/// Fast ABI entry for `col.map_free(handle)`.
///
/// Skips the generic host-call dispatch. Removes the map from the
/// registry. No-op for an invalid handle.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_free_fast(handle: SpectraHostValue) {
    crate::stdlib::map_free_fast(handle as usize)
}

// ── Collection fast ABI -----------------------------------------------------

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_new_fast() -> SpectraHostValue {
    crate::stdlib::list_new_fast()
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_push_fast(
    handle: SpectraHostValue,
    value: SpectraHostValue,
) -> i32 {
    crate::stdlib::list_push_fast(handle as usize, value)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_push_value_fast(
    handle: SpectraHostValue,
    value: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::list_push_value_fast(handle as usize, value)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_len_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::list_len_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_get_fast(
    handle: SpectraHostValue,
    index: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::list_get_fast(handle as usize, index)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_get_option_fast(
    handle: SpectraHostValue,
    index: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::list_get_fast(handle as usize, index)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_set_fast(
    handle: SpectraHostValue,
    index: SpectraHostValue,
    value: SpectraHostValue,
) -> i32 {
    crate::stdlib::list_set_fast(handle as usize, index, value)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_contains_fast(
    handle: SpectraHostValue,
    value: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::list_contains_fast(handle as usize, value)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_clear_fast(handle: SpectraHostValue) -> i32 {
    crate::stdlib::list_clear_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_free_fast(handle: SpectraHostValue) -> i32 {
    crate::stdlib::list_free_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_free_all_fast() -> SpectraHostValue {
    crate::stdlib::list_free_all_fast()
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_pop_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::list_pop_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_pop_front_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::list_pop_front_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_pop_option_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::list_pop_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_pop_front_option_fast(
    handle: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::list_pop_front_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_insert_at_fast(
    handle: SpectraHostValue,
    index: SpectraHostValue,
    value: SpectraHostValue,
) -> i32 {
    crate::stdlib::list_insert_at_fast(handle as usize, index, value)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_remove_at_fast(
    handle: SpectraHostValue,
    index: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::list_remove_at_fast(handle as usize, index)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_remove_at_option_fast(
    handle: SpectraHostValue,
    index: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::list_remove_at_fast(handle as usize, index)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_index_of_fast(
    handle: SpectraHostValue,
    value: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::list_index_of_fast(handle as usize, value)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_sort_fast(handle: SpectraHostValue) -> i32 {
    crate::stdlib::list_sort_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_get_fast(
    handle: SpectraHostValue,
    key: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::map_get_fast(handle as usize, key)
}

/// Scalar-only map lookup with the regular tagged `Option` result ABI.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_get_scalar_fast(
    handle: SpectraHostValue,
    key: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::map_get_scalar_fast(handle as usize, key)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_get_option_fast(
    handle: SpectraHostValue,
    key: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::map_get_fast(handle as usize, key)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_remove_fast(
    handle: SpectraHostValue,
    key: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::map_remove_fast(handle as usize, key)
}

/// Scalar-only map removal with the regular tagged `Option` result ABI.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_remove_scalar_fast(
    handle: SpectraHostValue,
    key: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::map_remove_scalar_fast(handle as usize, key)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_remove_option_fast(
    handle: SpectraHostValue,
    key: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::map_remove_fast(handle as usize, key)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_is_empty_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::map_is_empty_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_free_all_fast() -> SpectraHostValue {
    crate::stdlib::map_free_all_fast()
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_clear_status_fast(handle: SpectraHostValue) -> i32 {
    crate::stdlib::map_clear_fast_collection(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_free_status_fast(handle: SpectraHostValue) -> i32 {
    crate::stdlib::map_free_fast_collection(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_stack_new_fast() -> SpectraHostValue {
    crate::stdlib::stack_new_fast()
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_stack_push_fast(
    handle: SpectraHostValue,
    value: SpectraHostValue,
) -> i32 {
    crate::stdlib::stack_push_fast(handle as usize, value)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_stack_pop_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::stack_pop_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_stack_peek_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::stack_peek_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_stack_len_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::stack_len_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_stack_is_empty_fast(
    handle: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::stack_is_empty_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_stack_clear_fast(handle: SpectraHostValue) -> i32 {
    crate::stdlib::stack_clear_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_stack_free_fast(handle: SpectraHostValue) -> i32 {
    crate::stdlib::stack_free_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_stack_free_all_fast() -> SpectraHostValue {
    crate::stdlib::stack_free_all_fast()
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_queue_new_fast() -> SpectraHostValue {
    crate::stdlib::queue_new_fast()
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_queue_enqueue_fast(
    handle: SpectraHostValue,
    value: SpectraHostValue,
) -> i32 {
    crate::stdlib::queue_enqueue_fast(handle as usize, value)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_queue_dequeue_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::queue_dequeue_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_queue_peek_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::queue_peek_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_queue_len_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::queue_len_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_queue_is_empty_fast(
    handle: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::queue_is_empty_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_queue_clear_fast(handle: SpectraHostValue) -> i32 {
    crate::stdlib::queue_clear_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_queue_free_fast(handle: SpectraHostValue) -> i32 {
    crate::stdlib::queue_free_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_queue_free_all_fast() -> SpectraHostValue {
    crate::stdlib::queue_free_all_fast()
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_list_iter_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::list_iter_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_set_iter_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::set_iter_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_iter_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::map_iter_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_map_values_iter_fast(
    handle: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::map_values_iter_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_stack_iter_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::stack_iter_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_queue_iter_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::queue_iter_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_iterator_next_fast(handle: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::iterator_next_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_iterator_next_unchecked_fast(
    handle: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::iterator_next_unchecked_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_iterator_remaining_fast(
    handle: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::iterator_remaining_fast(handle as usize)
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_iterator_free_fast(handle: SpectraHostValue) -> i32 {
    crate::stdlib::iterator_free_fast(handle as usize)
}

/// Fast ABI entry for `concurrent.channel_new()`.
///
/// Skips the generic host-call dispatch. Returns the new channel id
/// (>0 on success) or 0 on internal error.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_channel_new_fast() -> SpectraHostValue {
    crate::stdlib::concurrent_channel_new_fast()
}

/// Fast ABI entry for `concurrent.channel_send(channel, value)`.
///
/// Skips the generic host-call dispatch. Returns 1 on success, 0 if the
/// channel is closed. Returns `HOST_STATUS_NOT_FOUND` (-2) if the channel
/// id is invalid.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_channel_send_fast(
    channel: SpectraHostValue,
    value: SpectraHostValue,
) -> i32 {
    crate::stdlib::concurrent_channel_send_fast(channel, value)
}

/// Fast ABI entry for `concurrent.channel_recv(channel)`.
///
/// Skips the generic host-call dispatch. Returns the next value in the
/// channel, or -1 if the channel is empty / closed / invalid id.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_channel_recv_fast(channel: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::concurrent_channel_recv_fast(channel)
}

/// Fast ABI entry for `concurrent.channel_close(channel)`.
///
/// Skips the generic host-call dispatch. Returns `HOST_STATUS_SUCCESS` (0)
/// on success or `HOST_STATUS_NOT_FOUND` (-2) if the channel id is invalid.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_channel_close_fast(channel: SpectraHostValue) -> i32 {
    crate::stdlib::concurrent_channel_close_fast(channel)
}

/// Fast ABI entry for `concurrent.channel_len(channel)`.
///
/// Skips the generic host-call dispatch. Returns the queue length, or 0
/// for an invalid channel id.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_channel_len_fast(channel: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::concurrent_channel_len_fast(channel)
}

/// Fast ABI entry for `ml.linear(input, weight, bias)`.
///
/// Skips the generic host-call dispatch. Returns the new tensor handle
/// (>0) on success or 0 on error.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_ml_linear_fast(
    input_h: SpectraHostValue,
    weight_h: SpectraHostValue,
    bias_h: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::ml_linear_fast(input_h as usize, weight_h as usize, bias_h as usize)
}

/// Fast ABI entry for `ml.mse_loss(prediction, target)`.
///
/// Skips the generic host-call dispatch. Returns the new loss tensor
/// handle (>0) on success or 0 on error.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_ml_mse_loss_fast(
    prediction_h: SpectraHostValue,
    target_h: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::ml_mse_loss_fast(prediction_h as usize, target_h as usize)
}

/// Fast ABI entry for `tensor.backward(loss)`.
///
/// Skips the generic host-call dispatch. Returns `HOST_STATUS_SUCCESS` (0)
/// on success or the error code on failure.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_tensor_backward_fast(loss_h: SpectraHostValue) -> i32 {
    crate::stdlib::tensor_backward_fast(loss_h as usize)
}

#[no_mangle]
pub extern "C" fn spectra_rt_tensor_grad_handle_fast(
    input_h: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::tensor_grad_handle_fast(input_h as usize) as SpectraHostValue
}

#[no_mangle]
pub extern "C" fn spectra_rt_tensor_autodiff_apply_fast(
    operation: SpectraHostValue,
    output_h: SpectraHostValue,
    upstream_h: SpectraHostValue,
    target0: SpectraHostValue,
    target1: SpectraHostValue,
    target2: SpectraHostValue,
) -> i32 {
    crate::stdlib::tensor_autodiff_apply_fast(
        operation,
        output_h as usize,
        upstream_h as usize,
        target0 as usize,
        target1 as usize,
        target2 as usize,
    )
}

/// Fast ABI entry for `ml.sgd_step(param, lr)`.
///
/// Skips the generic host-call dispatch. `lr` is the raw `f64` learning
/// rate passed directly across the FFI boundary. Returns `HOST_STATUS_SUCCESS` (0)
/// on success, `HOST_STATUS_INVALID_ARGUMENT` if the LR is invalid, or
/// `HOST_STATUS_NOT_FOUND` if the handle is invalid.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_ml_sgd_step_fast(param_h: SpectraHostValue, lr: f64) -> i32 {
    crate::stdlib::ml_sgd_step_fast(param_h as usize, lr)
}

/// Fast ABI entry for `tensor.full_f(n, value)`.
///
/// Skips the generic host-call dispatch. `value` is the raw `f64` fill value.
/// Returns the new tensor handle (>0) on success or 0 on error.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_tensor_full_f_fast(
    n: SpectraHostValue,
    value: f64,
) -> SpectraHostValue {
    crate::stdlib::tensor_full_f_fast(n as usize, value)
}

/// Fast ABI entry for `str.len(s)`.
///
/// Skips the generic host-call dispatch AND the intermediate `String`
/// allocation in `read_spectra_string`. Walks the null-terminated `i64`
/// array directly to count bytes. Called from JIT/AOT code when the inline
/// path is not available (e.g., AOT path with non-statically-known length).
///
/// Returns the string length (>0) or `0` for an invalid handle.
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_string_len_fast(s: SpectraHostValue) -> SpectraHostValue {
    crate::stdlib::string_len_fast(s)
}

/// Fast ABI entry for `str.char_at(s, index)`.
///
/// Skips the generic host-call dispatch AND the intermediate `String`
/// allocation in `read_spectra_string`. Reads the byte at the given index
/// directly from the null-terminated `i64` array. Called from JIT/AOT code
/// when the inline path is not available.
///
/// Returns the byte value (0-255) on success or `-1` for an out-of-bounds
/// access (null handle, negative index, or index past the null terminator).
#[no_mangle]
#[inline(never)]
pub extern "C" fn spectra_rt_string_char_at_fast(
    s: SpectraHostValue,
    index: SpectraHostValue,
) -> SpectraHostValue {
    crate::stdlib::string_char_at_fast(s, index)
}
