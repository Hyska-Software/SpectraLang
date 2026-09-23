/// Releases a manual allocation previously returned by `spectra_rt_manual_alloc`.
///
/// The freed address is NOT immediately reusable: it becomes a quarantine
/// tombstone (bounded FIFO window, see `QUARANTINE_CAPACITY`) whose backing
/// block stays pinned so `spectra_rt_manual_alloc` cannot hand the same
/// address out while the tombstone is retained. This turns stale/double
/// frees into detectable errors instead of silent wrong-frees.
///
/// Invalid frees (unknown address, or an address still inside the
/// quarantine window) do not release anything; they are reported through
/// [`spectra_rt_manual_free_last_status`] with
/// [`HOST_STATUS_INVALID_ARGUMENT`](crate::ffi::HOST_STATUS_INVALID_ARGUMENT).
/// This entry keeps its historical `void` signature — JIT import ABI is
/// unchanged.
#[no_mangle]
pub extern "C" fn spectra_rt_manual_free(ptr: *mut u8) {
    if ptr.is_null() {
        // Freeing null is a legal no-op (C convention), not an error.
        crate::ffi::record_manual_free_status(HOST_STATUS_SUCCESS);
        return;
    }

    let ptr_value = ptr as usize;
    let table = allocation_table();
    let mut guard = table.lock().unwrap_or_else(|e| e.into_inner());
    let status = guard.free_tracked(ptr_value);
    crate::ffi::record_manual_free_status(status);
}

/// Returns the host status code recorded by the most recent
/// [`spectra_rt_manual_free`] call: `HOST_STATUS_SUCCESS` when the pointer
/// was a live tracked allocation, `HOST_STATUS_INVALID_ARGUMENT` when it was
/// null-free-adjacent garbage — unknown to the table or still quarantined
/// from a previous free (double free / stale pointer).
///
/// Process-wide, like the allocation table itself. Intended for debug
/// tooling and validation harnesses; generated code does not branch on it.
#[no_mangle]
pub extern "C" fn spectra_rt_manual_free_last_status() -> i32 {
    crate::ffi::last_manual_free_status()
}

/// Returns the number of tombstones currently held in the free quarantine
/// (bounded by `QUARANTINE_CAPACITY`). Introspection for tests and soak
/// tooling.
#[no_mangle]
pub extern "C" fn spectra_rt_manual_quarantine_len() -> usize {
    let table = allocation_table();
    let guard = table.lock().unwrap_or_else(|e| e.into_inner());
    guard.quarantine_len()
}

// --- Frame-0 escape budget --------------------------------------------------
//
// `spectra_rt_manual_escape` re-parents allocations onto the base frame (0),
// which is never popped by `frame_exit`. Escaped bytes therefore stay
// resident for the whole process lifetime: a loop that escapes per iteration
// grows without bound and eventually dies under the operating system's OOM
// killer — a silent, undiagnosable failure.
//
// The counter below is deliberately MONOTONIC. It never decrements — not
// even when an escaped value is later released through
// `spectra_rt_manual_free` — because escaped values typically back returned
// results whose lifetime spans frames: cumulative volume ("peak") is what
// predicts memory exhaustion, so the budget is a hard ceiling on total
// re-parented bytes, not a live-bytes gauge.
//
// Configure with `SPECTRA_FRAME0_BUDGET_MB` (default 512). `0` disables the
// ceiling entirely for short-lived scripts where process exit frees
// everything. On breach the runtime takes the existing `spectra_rt_panic`
// path: a loud `runtime error:` diagnostic on stderr and exit code 101,
// instead of an opaque OS OOM kill.

/// Environment variable overriding the frame-0 escape budget, in MiB.
const FRAME0_BUDGET_ENV: &str = "SPECTRA_FRAME0_BUDGET_MB";

/// Default frame-0 escape budget in MiB.
const FRAME0_BUDGET_DEFAULT_MB: u64 = 512;

const FRAME0_BYTES_PER_MB: usize = 1024 * 1024;

/// Cumulative bytes moved into frame 0 by [`spectra_rt_manual_escape`].
/// Monotonic by design — see the block comment above.
static FRAME0_ESCAPED_BYTES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Parses the frame-0 budget override. `None` or an unparseable value falls
/// back to [`FRAME0_BUDGET_DEFAULT_MB`]; `0` means unlimited.
pub(crate) fn parse_frame0_budget_mb(raw: Option<&str>) -> u64 {
    match raw {
        Some(raw) => raw.trim().parse::<u64>().unwrap_or(FRAME0_BUDGET_DEFAULT_MB),
        None => FRAME0_BUDGET_DEFAULT_MB,
    }
}

fn frame0_budget_mb() -> u64 {
    parse_frame0_budget_mb(std::env::var(FRAME0_BUDGET_ENV).ok().as_deref())
}

/// Pure budget predicate: `Ok(())` while cumulative escaped `bytes` stay
/// within `budget_mb`, `Err` carrying the full diagnostic otherwise.
/// `budget_mb == 0` disables the ceiling. Kept side-effect-free so tests can
/// exercise boundary behaviour without touching process state.
pub(crate) fn frame0_budget_check(bytes: usize, budget_mb: u64) -> Result<(), String> {
    if budget_mb == 0 {
        return Ok(());
    }
    let limit = usize::try_from(budget_mb.saturating_mul(1024 * 1024)).unwrap_or(usize::MAX);
    if bytes <= limit {
        return Ok(());
    }
    Err(format!(
        "frame-0 budget exceeded (escaped {} MB > {} MB). Long-running programs must free or stream; raise SPECTRA_FRAME0_BUDGET_MB if intentional.",
        bytes.saturating_add(FRAME0_BYTES_PER_MB - 1) / FRAME0_BYTES_PER_MB,
        budget_mb,
    ))
}

/// Reports a frame-0 budget breach through the standard panic path
/// ([`crate::panic::spectra_rt_panic`]): stderr diagnostic + exit code 101.
fn frame0_budget_abort(message: &str) -> ! {
    let mut buf = message.as_bytes().to_vec();
    buf.push(0); // NUL terminator expected by the panic message scan.
    crate::panic::spectra_rt_panic(buf.as_ptr() as i64);
    unreachable!("spectra_rt_panic terminates the process")
}

/// Moves a manual allocation from its current frame to the process-wide base
/// frame (id `0`), so that it survives every `frame_exit`.
///
/// Only moves the allocation if it currently belongs to `current_frame_id` — this
/// prevents accidentally re-parenting allocations that were passed in from the caller.
/// If `ptr` is not a tracked allocation (e.g. a scalar value), this is a no-op.
/// The destination is always the **base frame (0)**, never the lexical parent:
/// `frame_exit` pops whole per-thread frame stacks down to the requested id,
/// and frame 0 is the one frame no exit can ever free, so an escaped value
/// outlives its caller's frames as well.
///
/// # Frame-0 budget
///
/// Frame 0 is never popped by `frame_exit`, so every escaped byte stays
/// resident until process exit. Each successful escape adds the allocation
/// size to a process-wide monotonic counter and checks it against
/// `SPECTRA_FRAME0_BUDGET_MB` (MiB; default 512; `0` = unlimited for short
/// scripts). The counter does NOT rewind when an escaped value is later
/// freed via [`spectra_rt_manual_free`] — the budget is a hard cumulative
/// ceiling on re-parented volume, because escaped values sustain returned
/// results and peak volume is what exhausts memory. Breaching the ceiling
/// aborts through `spectra_rt_panic` with
/// `runtime error: frame-0 budget exceeded (...)` and exit code 101,
/// replacing an eventual silent OS OOM kill with an actionable diagnostic.
fn escape_allocation_locked(
    table: &mut AllocationTable,
    ptr_value: usize,
    current_frame_id: usize,
) -> usize {
    // Frame 0 is already immortal. Avoid charging an allocation that is
    // already there a second time when a caller stores it again.
    if current_frame_id == 0 {
        return 0;
    }

    let belongs_to_current = table
        .allocations
        .get(&ptr_value)
        .is_some_and(|entry| entry.frame_id == current_frame_id);
    if !belongs_to_current {
        return 0;
    }

    table.remove_from_frame(current_frame_id, ptr_value);
    if let Some(entry) = table.allocations.get_mut(&ptr_value) {
        // Escaping directly to frame 0 keeps values alive across nested
        // transient frames.
        entry.frame_id = 0;
    }
    if let Some(parent) = table.frames.iter_mut().rev().find(|frame| frame.id == 0) {
        parent.track(ptr_value);
    }
    table
        .allocations
        .get(&ptr_value)
        .map(ManualAllocation::byte_len)
        .unwrap_or(0)
}

/// Re-parents a tracked allocation to frame 0 regardless of which active
/// stack frame created it. This is reserved for values handed to durable
/// runtime-owned storage (coroutine frames and containers).
fn escape_stored_allocation_locked(table: &mut AllocationTable, ptr_value: usize) -> usize {
    let Some(owner_frame_id) = table.allocations.get(&ptr_value).map(|entry| entry.frame_id) else {
        return 0;
    };
    if owner_frame_id == 0 {
        return 0;
    }

    table.remove_from_frame(owner_frame_id, ptr_value);
    if let Some(entry) = table.allocations.get_mut(&ptr_value) {
        entry.frame_id = 0;
    }
    if let Some(base) = table.frames.iter_mut().find(|frame| frame.id == 0) {
        base.track(ptr_value);
    }
    table
        .allocations
        .get(&ptr_value)
        .map(ManualAllocation::byte_len)
        .unwrap_or(0)
}

fn account_frame0_escape(escaped_bytes: usize) {
    if escaped_bytes == 0 {
        return;
    }
    let total =
        FRAME0_ESCAPED_BYTES.fetch_add(escaped_bytes, std::sync::atomic::Ordering::Relaxed)
            + escaped_bytes;
    if let Err(message) = frame0_budget_check(total, frame0_budget_mb()) {
        frame0_budget_abort(&message);
    }
}

#[no_mangle]
pub extern "C" fn spectra_rt_manual_escape(ptr: *mut u8, current_frame_id: usize) {
    if ptr.is_null() {
        return;
    }
    let ptr_value = ptr as usize;
    let table = allocation_table();
    let mut guard = table.lock().unwrap_or_else(|e| e.into_inner());

    let escaped_bytes = escape_allocation_locked(&mut guard, ptr_value, current_frame_id);
    drop(guard);
    account_frame0_escape(escaped_bytes);
}

/// Moves a pointer placed in runtime-owned storage to frame 0 so it survives
/// the producer frame's exit. Unknown pointers and scalar words are no-ops.
#[no_mangle]
pub extern "C" fn spectra_rt_manual_escape_stored(value: i64) {
    if value == 0 {
        return;
    }
    let table = allocation_table();
    let mut guard = table.lock().unwrap_or_else(|e| e.into_inner());
    let escaped_bytes = escape_stored_allocation_locked(&mut guard, value as usize);
    drop(guard);
    account_frame0_escape(escaped_bytes);
}

/// Re-parents a value that a runtime-owned container is about to store.
///
/// A runtime-owned container or coroutine frame can outlive the frame that
/// supplied a value, including when the allocation belongs to an outer frame.
/// Re-parent tracked allocations before storing them so a later frame exit
/// cannot leave the durable runtime object with a dangling pointer. Values
/// that are not tracked allocations (scalars and borrowed string literals)
/// remain unchanged.
pub(crate) fn escape_stored_value(value: SpectraHostValue) {
    if value == 0 {
        return;
    }
    let table = allocation_table();
    let mut guard = table.lock().unwrap_or_else(|error| error.into_inner());
    let escaped_bytes = escape_stored_allocation_locked(&mut guard, value as usize);
    drop(guard);
    account_frame0_escape(escaped_bytes);
}

/// Clears all outstanding manual allocations owned by the runtime.
///
/// # Why this also resets the container registries
///
/// `clear_all` frees every tracked manual buffer, but the runtime's container
/// registries (lists, maps, sets, stacks, queues, iterators, string builders,
/// tensors, concurrent tasks/counters) and the async task/coroutine tables can
/// still hold raw pointers into that freed heap. The CLI calls this after
/// every JIT run, and a REPL/package-test loop runs many programs per
/// process, so leaving those registries populated would hand the next program
/// handles whose backing storage points into freed memory. Resetting them here
/// keeps that fix inside the runtime — no CLI change is required.
///
/// The async coroutine frames are drained through the **polling-safe** clear:
/// a frame another thread is currently inside (`polling` / `drop_in_progress`)
/// is retained instead of freed (see `AsyncFrameRegistry::clear`).
#[no_mangle]
pub extern "C" fn spectra_rt_manual_clear() {
    {
        let table = allocation_table();
        let mut guard = table.lock().unwrap_or_else(|e| e.into_inner());
        guard.clear_all();
    }
    reset_registries_after_manual_clear();
}

/// Drops every runtime registry that can hold raw pointers into the manual
/// heap freed by [`spectra_rt_manual_clear`]. Extracted so tests (and any
/// future internal caller of `AllocationTable::clear_all`) share one path.
pub(crate) fn reset_registries_after_manual_clear() {
    // Collections whose elements are raw `SpectraHostValue` pointers.
    crate::stdlib::with_list_registry(|registry| registry.lists.clear());
    // `clear_all` also bumps the map fast-cache epoch, invalidating the
    // thread-local weak caches that would otherwise pin stale maps.
    crate::stdlib::with_map_registry(|registry| {
        registry.clear_all();
    });
    crate::stdlib::with_set_registry(|registry| registry.sets.clear());
    crate::stdlib::with_stack_registry(|registry| registry.clear_all());
    crate::stdlib::with_queue_registry(|registry| registry.clear_all());
    // Iterators snapshot container elements, so they alias the same pointers.
    crate::stdlib::with_iterator_registry(|registry| registry.iterators.clear());
    crate::stdlib::lock_unpoisoned(crate::stdlib::string_builder_registry()).builders.clear();
    // Tensors store scalar host values that may be raw pointers.
    crate::stdlib::with_tensor_registry(|registry| registry.tensors.clear());
    // Concurrent tasks/counters store scalar host values too.
    crate::stdlib::lock_unpoisoned(crate::stdlib::concurrent_registry()).clear();
    // Async tasks and coroutine frame slots: polling-safe (frames currently
    // being polled are retained, not dropped).
    let _ = crate::stdlib::reset_async_state();
}

/// Registers a host function that JITed code can invoke by name.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn spectra_rt_host_register(
    name_ptr: *const u8,
    name_len: usize,
    fn_ptr: *const (),
) -> bool {
    if fn_ptr.is_null() || name_len == 0 {
        return false;
    }

    let Some(name) = (unsafe { read_host_name(name_ptr, name_len) }) else {
        return false;
    };

    let registry = host_registry();
    let mut guard = registry.lock().unwrap_or_else(|e| e.into_inner());
    let inserted = guard.insert(name, fn_ptr);
    advance_host_registry_generation();
    inserted
}

/// Unregisters a previously registered host function.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn spectra_rt_host_unregister(name_ptr: *const u8, name_len: usize) -> bool {
    if name_len == 0 {
        return false;
    }

    let Some(name) = (unsafe { read_host_name(name_ptr, name_len) }) else {
        return false;
    };

    let registry = host_registry();
    let mut guard = registry.lock().unwrap_or_else(|e| e.into_inner());
    let removed = guard.remove(name);
    if removed {
        advance_host_registry_generation();
    }
    removed
}

/// Looks up a host function by name, returning `NULL` if not found or invalid.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn spectra_rt_host_lookup(name_ptr: *const u8, name_len: usize) -> *const () {
    if name_len == 0 {
        return ptr::null();
    }

    let Some(name) = (unsafe { read_host_name(name_ptr, name_len) }) else {
        return ptr::null();
    };

    let registry = host_registry();
    let guard = registry.lock().unwrap_or_else(|e| e.into_inner());
    guard.lookup(name)
}

/// Looks up a host function and invokes it with the provided context buffers.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn spectra_rt_host_invoke(
    name_ptr: *const u8,
    name_len: usize,
    args_ptr: *const SpectraHostValue,
    arg_len: usize,
    results_ptr: *mut SpectraHostValue,
    result_len: usize,
) -> i32 {
    if name_len == 0 || name_ptr.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }

    if (arg_len > 0 && args_ptr.is_null()) || (result_len > 0 && results_ptr.is_null()) {
        return HOST_STATUS_INVALID_ARGUMENT;
    }

    let Some(name) = (unsafe { read_host_name(name_ptr, name_len) }) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };

    dispatch_generic(
        name,
        resolve_registered_host(name),
        args_ptr,
        arg_len,
        results_ptr,
        result_len,
    )
}

/// Looks up a host function through a module-owned cache slot and invokes it.
///
/// Cache hits validate the registry generation and load the function pointer
/// atomically. A miss falls back to the existing registry lock and publishes
/// the result for subsequent calls. The individual call retains its own panic
/// boundary and the public context layout is unchanged.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn spectra_rt_host_invoke_cached(
    cache_ptr: *const SpectraHostCallCache,
    name_ptr: *const u8,
    name_len: usize,
    args_ptr: *const SpectraHostValue,
    arg_len: usize,
    results_ptr: *mut SpectraHostValue,
    result_len: usize,
) -> i32 {
    if cache_ptr.is_null() || name_len == 0 || name_ptr.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }

    if (arg_len > 0 && args_ptr.is_null()) || (result_len > 0 && results_ptr.is_null()) {
        return HOST_STATUS_INVALID_ARGUMENT;
    }

    let Some(name) = (unsafe { read_host_name(name_ptr, name_len) }) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };

    let function = resolve_cached_host(unsafe { &*cache_ptr }, name);
    dispatch_generic(name, function, args_ptr, arg_len, results_ptr, result_len)
}

/// Invokes a bounded sequence of generic hostcalls in source order.
///
/// The dispatcher intentionally stops at the first non-success status. The
/// backend treats any non-success exactly like the individual dispatcher and
/// traps after its stack-owned arenas become unreachable. No hostcall after a
/// failure is observed by the runtime.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn spectra_rt_host_invoke_batch(
    calls_ptr: *const SpectraHostBatchCall,
    call_len: usize,
) -> i32 {
    if call_len == 0 || calls_ptr.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }

    let calls = unsafe { slice::from_raw_parts(calls_ptr, call_len) };
    for call in calls {
        if call.name_len == 0 || call.name_ptr.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if (call.arg_len > 0 && call.args_ptr.is_null())
            || (call.result_len > 0 && call.results_ptr.is_null())
        {
            return HOST_STATUS_INVALID_ARGUMENT;
        }

        let Some(name) = (unsafe { read_host_name(call.name_ptr, call.name_len) }) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let status = dispatch_generic(
            name,
            resolve_registered_host(name),
            call.args_ptr,
            call.arg_len,
            call.results_ptr,
            call.result_len,
        );
        if status != HOST_STATUS_SUCCESS {
            return status;
        }
    }

    HOST_STATUS_SUCCESS
}

/// Invokes a bounded sequence of cache-aware generic hostcalls in source
/// order. This path wraps the whole sequence in one panic boundary. It stops
/// at the first non-success status or panic and leaves all following
/// descriptors unobserved.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn spectra_rt_host_invoke_cached_batch(
    calls_ptr: *const SpectraHostCachedBatchCall,
    call_len: usize,
) -> i32 {
    if call_len == 0 || calls_ptr.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }

    let calls = unsafe { slice::from_raw_parts(calls_ptr, call_len) };
    let batch_generation = host_registry_generation().load(Ordering::Acquire);
    match catch_unwind(AssertUnwindSafe(|| {
        for call in calls {
            if call.cache_ptr.is_null() || call.name_len == 0 || call.name_ptr.is_null() {
                return HOST_STATUS_INVALID_ARGUMENT;
            }
            if (call.arg_len > 0 && call.args_ptr.is_null())
                || (call.result_len > 0 && call.results_ptr.is_null())
            {
                return HOST_STATUS_INVALID_ARGUMENT;
            }

            let Some(name) = (unsafe { read_host_name(call.name_ptr, call.name_len) }) else {
                return HOST_STATUS_INVALID_ARGUMENT;
            };
            let function =
                resolve_cached_host_for_batch(unsafe { &*call.cache_ptr }, name, batch_generation);
            let status = dispatch_generic(
                name,
                function,
                call.args_ptr,
                call.arg_len,
                call.results_ptr,
                call.result_len,
            );
            if status != HOST_STATUS_SUCCESS {
                return status;
            }
        }
        HOST_STATUS_SUCCESS
    })) {
        Ok(status) => status,
        Err(_) => HOST_STATUS_INTERNAL_ERROR,
    }
}

/// Checks runtime debug invariants for host registry and manual allocation state.
///
/// This function is cheap enough to use in stress/soak validation and returns
/// false instead of panicking so automation can report a normal failure.
#[no_mangle]
pub extern "C" fn spectra_rt_debug_invariants_check() -> bool {
    let host_ok = host_registry()
        .lock()
        .map(|registry| registry.check_invariants())
        .unwrap_or(false);
    let allocation_ok = allocation_table()
        .lock()
        .map(|table| table.check_invariants())
        .unwrap_or(false);
    host_ok && allocation_ok
}

/// Returns the host status code recorded by the most recent
/// [`spectra_rt_manual_frame_exit`] call: `HOST_STATUS_SUCCESS` when the
/// exited frame (and the frames above it) belonged to the calling thread,
/// `HOST_STATUS_INVALID_ARGUMENT` when the id was unknown, stale, owned by
/// another thread, or the base frame `0` — in every one of those cases the
/// exit freed **nothing**.
///
/// Process-wide, like the allocation table itself. Intended for debug
/// tooling and validation harnesses; generated code does not branch on it.
#[no_mangle]
pub extern "C" fn spectra_rt_manual_frame_exit_last_status() -> i32 {
    crate::ffi::last_manual_frame_exit_status()
}

// ── Closure-object and code-range validation ────────────────────────────────
//
// `spectra_rt_invoke_closure` used to transmute and call an arbitrary i64
// after only a null check. Two runtime-only checks now gate the call:
//
//  (a) the closure object must be a tracked live manual allocation of at
//      least the closure header size, or — when it is not tracked — memory
//      the OS reports as committed and readable;
//  (b) the code pointer must fall inside a registered executable code range
//      (explicitly registered through `spectra_rt_register_code_range`, or
//      discovered by the first-use executable-region scan on platforms that
//      support it). Unregistered code pointers are rejected without being
//      called.
//
// Known deviation from a strict "must be a tracked manual allocation" rule
// for (a): the midend allocates closure objects with `build_alloca`
// (`lowering_impl_methods.rs::build_closure_object`), i.e. on the generated
// function's stack, not through `spectra_rt_manual_alloc`. Requiring manual
// tracking would reject every production closure, so the OS readability
// probe is the fallback for untracked objects. Making closure objects
// heap-tracked needs a midend change (out of the runtime's scope).

/// Minimum readable size of a closure object: slot 0 holds the code pointer.
/// Capture slots beyond slot 0 are read by the callee itself.
const CLOSURE_OBJECT_MIN_BYTES: usize = mem::size_of::<i64>();

/// Best-effort OS view of a memory span.
#[derive(Clone, Copy, Debug, Default)]
struct MemoryProbe {
    /// The span is committed and readable. `true` when the platform has no
    /// probe (fail-open).
    readable: bool,
    /// A probe was actually performed on this platform.
    probed: bool,
}

/// Whether range enforcement (probe + scan) is available on this platform.
/// Windows uses `VirtualQuery`; Linux/Android parse `/proc/self/maps`.
/// Elsewhere both checks fail open (documented in the report): the closure
/// object degrades to a null check and code pointers are not range-gated.
fn range_enforcement_available() -> bool {
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "android"))]
    {
        true
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "android")))]
    {
        false
    }
}

#[cfg(target_os = "windows")]
fn probe_memory(addr: usize, len: usize) -> MemoryProbe {
    #[repr(C)]
    struct MemoryBasicInformation {
        base_address: usize,
        allocation_base: usize,
        allocation_protect: u32,
        // PartitionId (WORD) + padding on modern SDKs; plain padding on
        // older ones — the offsets of the following fields are identical.
        _partition_or_pad: u32,
        region_size: usize,
        state: u32,
        protect: u32,
        _type: u32,
    }
    extern "system" {
        fn VirtualQuery(
            lp_address: *const core::ffi::c_void,
            lp_buffer: *mut MemoryBasicInformation,
            dw_length: usize,
        ) -> usize;
    }
    const MEM_COMMIT: u32 = 0x1000;
    const PAGE_NOACCESS: u32 = 0x01;
    const PAGE_GUARD: u32 = 0x100;

    let mut info: MemoryBasicInformation = unsafe { mem::zeroed() };
    let result = unsafe {
        VirtualQuery(
            addr as *const core::ffi::c_void,
            &mut info,
            mem::size_of::<MemoryBasicInformation>(),
        )
    };
    if result == 0 {
        return MemoryProbe::default();
    }
    let committed = info.state == MEM_COMMIT;
    let guarded = info.protect & (PAGE_GUARD | PAGE_NOACCESS) != 0;
    // The span must fit inside the region that contains its start; crossing
    // into a differently-protected region is treated as not readable.
    let covers_span =
        addr.saturating_add(len) <= info.base_address.saturating_add(info.region_size);
    MemoryProbe {
        readable: committed && !guarded && covers_span,
        probed: true,
    }
}

#[cfg(target_os = "windows")]
fn scan_executable_ranges() -> Vec<(usize, usize)> {
    #[repr(C)]
    struct MemoryBasicInformation {
        base_address: usize,
        allocation_base: usize,
        allocation_protect: u32,
        _partition_or_pad: u32,
        region_size: usize,
        state: u32,
        protect: u32,
        _type: u32,
    }
    extern "system" {
        fn VirtualQuery(
            lp_address: *const core::ffi::c_void,
            lp_buffer: *mut MemoryBasicInformation,
            dw_length: usize,
        ) -> usize;
    }
    const MEM_COMMIT: u32 = 0x1000;
    const PAGE_NOACCESS: u32 = 0x01;
    const PAGE_GUARD: u32 = 0x100;
    const EXECUTE_FLAGS: u32 = 0x10 | 0x20 | 0x40 | 0x80;

    let mut ranges = Vec::new();
    let mut addr: usize = 0;
    loop {
        let mut info: MemoryBasicInformation = unsafe { mem::zeroed() };
        let result = unsafe {
            VirtualQuery(
                addr as *const core::ffi::c_void,
                &mut info,
                mem::size_of::<MemoryBasicInformation>(),
            )
        };
        if result == 0 {
            break;
        }
        if info.state == MEM_COMMIT
            && info.protect & (PAGE_GUARD | PAGE_NOACCESS) == 0
            && info.protect & EXECUTE_FLAGS != 0
        {
            ranges.push((info.base_address, info.region_size));
        }
        let next = info.base_address.saturating_add(info.region_size);
        if next <= addr {
            break;
        }
        addr = next;
    }
    ranges
}

#[cfg(all(not(target_os = "windows"), any(target_os = "linux", target_os = "android")))]
fn probe_memory(addr: usize, len: usize) -> MemoryProbe {
    match maps_region(addr) {
        Some((_start, end, perms)) => MemoryProbe {
            readable: perms.as_bytes().first() == Some(&b'r') && addr.saturating_add(len) <= end,
            probed: true,
        },
        None => MemoryProbe::default(),
    }
}

#[cfg(all(not(target_os = "windows"), any(target_os = "linux", target_os = "android")))]
fn scan_executable_ranges() -> Vec<(usize, usize)> {
    read_maps()
        .filter(|(_, _, perms)| perms.as_bytes().get(2) == Some(&b'x'))
        .map(|(start, end, _)| (start, end - start))
        .collect()
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "android")))]
fn probe_memory(_addr: usize, _len: usize) -> MemoryProbe {
    MemoryProbe {
        readable: true,
        probed: false,
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "android")))]
fn scan_executable_ranges() -> Vec<(usize, usize)> {
    Vec::new()
}

#[cfg(all(not(target_os = "windows"), any(target_os = "linux", target_os = "android")))]
fn read_maps() -> impl Iterator<Item = (usize, usize, String)> {
    let text = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    text.lines().filter_map(|line| {
        let mut parts = line.split_whitespace();
        let range = parts.next()?;
        let perms = parts.next()?.to_string();
        let (start, end) = range.split_once('-')?;
        let start = usize::from_str_radix(start, 16).ok()?;
        let end = usize::from_str_radix(end, 16).ok()?;
        Some((start, end, perms))
    })
}

#[cfg(all(not(target_os = "windows"), any(target_os = "linux", target_os = "android")))]
fn maps_region(addr: usize) -> Option<(usize, usize, String)> {
    read_maps().find(|(start, end, _)| *start <= addr && addr < *end)
}

/// Registered executable code ranges. Windows' main image is covered by the
/// first-use `VirtualQuery` scan (which subsumes `GetModuleHandleW` +
/// `SizeOfImage`), Linux by the `/proc/self/maps` scan; JIT/AOT loaders and
/// embedders can add ranges explicitly via
/// [`spectra_rt_register_code_range`].
static CODE_RANGES: OnceLock<Mutex<Vec<(usize, usize)>>> = OnceLock::new();

fn code_ranges() -> &'static Mutex<Vec<(usize, usize)>> {
    CODE_RANGES.get_or_init(|| Mutex::new(Vec::new()))
}

/// Registers an executable code range that [`spectra_rt_invoke_closure`] may
/// call into. `ptr`/`len` describe `[ptr, ptr + len)` in bytes; invalid
/// arguments are rejected (`false`). JIT-compiled code lives in loader-owned
/// memory: either call this after JITing, or rely on the platform's
/// executable-region scan (available on Windows and Linux, absent on macOS).
#[no_mangle]
pub extern "C" fn spectra_rt_register_code_range(ptr: i64, len: i64) -> bool {
    let (Ok(start), Ok(len)) = (usize::try_from(ptr), usize::try_from(len)) else {
        return false;
    };
    if start == 0 || len == 0 {
        return false;
    }
    let mut guard = code_ranges().lock().unwrap_or_else(|poison| poison.into_inner());
    if guard.iter().any(|(known, size)| *known == start && *size == len) {
        return true;
    }
    guard.push((start, len));
    true
}

fn code_in_registered_range(addr: usize) -> bool {
    code_ranges()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .iter()
        .any(|(start, len)| addr.wrapping_sub(*start) < *len)
}

/// True when `addr` may be called: inside a registered range, or discoverable
/// by (re)scanning executable regions — JIT images map new pages after the
/// last scan, so a miss triggers one refresh before deciding.
fn code_ptr_permitted(addr: usize) -> bool {
    if code_in_registered_range(addr) {
        return true;
    }
    if !range_enforcement_available() {
        return true; // documented fail-open on unsupported platforms
    }
    for (start, len) in scan_executable_ranges() {
        let mut guard = code_ranges().lock().unwrap_or_else(|poison| poison.into_inner());
        if !guard.iter().any(|(known, size)| *known == start && *size == len) {
            guard.push((start, len));
        }
    }
    code_in_registered_range(addr)
}

/// Validates the closure *object* (item (a) above): a tracked live manual
/// allocation of at least the closure header, or — untracked — OS-verified
/// readable memory covering slot 0. Returns `false` for garbage pointers.
fn closure_object_valid(fn_ptr: i64) -> bool {
    if fn_ptr <= 0 {
        return false;
    }
    if let Some(size) = crate::ffi::manual_allocation_size(fn_ptr) {
        return size >= CLOSURE_OBJECT_MIN_BYTES;
    }
    if !range_enforcement_available() {
        return true; // documented fail-open on unsupported platforms
    }
    let probe = probe_memory(fn_ptr as usize, CLOSURE_OBJECT_MIN_BYTES);
    probe.probed && probe.readable
}

/// Invokes a JIT-compiled Spectra closure by its runtime closure handle.
///
/// # Parameters
/// - `fn_ptr`: raw i64 holding the closure object pointer. Slot 0 stores the
///   native code pointer; the closure handle itself is passed as hidden env.
/// - `args`: pointer to an array of `n_args` i64 argument values (may be null when
///   `n_args == 0`)
/// - `n_args`: number of arguments — currently 0, 1, or 2 are supported
/// - `result`: output slot written with the returned i64 value; may be null for
///   unit-returning functions
///
/// # Returns
/// `HOST_STATUS_SUCCESS` on success, `HOST_STATUS_INVALID_ARGUMENT` if
/// `fn_ptr == 0`, the closure object does not validate (untracked and not
/// readable memory), or the code pointer lies outside every registered
/// executable code range (rejected *without* being called), or
/// `HOST_STATUS_INTERNAL_ERROR` if `n_args` is outside the supported range.
///
/// # Safety
/// `fn_ptr` must be a valid closure handle whose code pointer calling convention
/// matches `fn(env, args...) -> i64` for the given `n_args`.
#[no_mangle]
pub unsafe extern "C" fn spectra_rt_invoke_closure(
    fn_ptr: i64,
    args: *const i64,
    n_args: usize,
    result: *mut i64,
) -> i32 {
    if fn_ptr == 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    // (a) Validate the closure object BEFORE dereferencing slot 0.
    if !closure_object_valid(fn_ptr) {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let closure_slots = fn_ptr as *const i64;
    if closure_slots.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let code_ptr = *closure_slots;
    if code_ptr == 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    // (b) The code pointer must sit inside a registered executable range;
    // otherwise reject WITHOUT transmuting or calling anything.
    if !code_ptr_permitted(code_ptr as usize) {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let invoke_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match n_args {
            0 => {
                let f: unsafe extern "C" fn(i64) -> i64 = mem::transmute(code_ptr as usize);
                Ok(f(fn_ptr))
            }
            1 => {
                let f: unsafe extern "C" fn(i64, i64) -> i64 = mem::transmute(code_ptr as usize);
                Ok(f(fn_ptr, if args.is_null() { 0 } else { *args }))
            }
            2 => {
                let f: unsafe extern "C" fn(i64, i64, i64) -> i64 = mem::transmute(code_ptr as usize);
                let a0 = if args.is_null() { 0 } else { *args };
                let a1 = if args.is_null() { 0 } else { *args.add(1) };
                Ok(f(fn_ptr, a0, a1))
            }
            _ => Err(HOST_STATUS_INTERNAL_ERROR),
        }
    }));
    // Boundary catch_unwind: a panicking JIT closure (e.g. one dispatched on
    // a concurrent worker thread) becomes a host status instead of unwinding
    // through foreign frames or aborting the process.
    let returned: i64 = match invoke_result {
        Ok(Ok(returned)) => returned,
        Ok(Err(status)) => return status,
        Err(_) => return HOST_STATUS_INTERNAL_ERROR,
    };
    if !result.is_null() {
        *result = returned;
    }
    HOST_STATUS_SUCCESS
}

/// Clears all registered host functions.
#[no_mangle]
pub extern "C" fn spectra_rt_host_clear() {
    let registry = host_registry();
    let mut guard = registry.lock().unwrap_or_else(|e| e.into_inner());
    guard.clear();
    advance_host_registry_generation();
}

/// One-shot startup for AOT executables: initialises the runtime and registers
/// all built-in stdlib host functions.  Must be called before any Spectra code runs.
#[no_mangle]
pub extern "C" fn spectra_rt_startup() {
    initialize();
    crate::register();
}

/// Startup for AOT executables with argument forwarding.
/// Initialises the runtime, registers the standard library, and stores the
/// process arguments so that `std.env.env_args_count` and `std.env.env_arg`
/// return the program's own arguments rather than the CLI's arguments.
///
/// # Safety
/// `argv` must be a valid C-style array of `argc` null-terminated UTF-8
/// strings, as provided by the OS through the C `main(argc, argv)` signature.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn spectra_rt_startup_with_args(argc: i32, argv: *const *const u8) {
    initialize();
    crate::register();
    if argv.is_null() || argc <= 0 {
        return;
    }
    let args: Vec<String> = (0..argc as usize)
        .filter_map(|i| unsafe {
            let ptr = *argv.add(i);
            if ptr.is_null() {
                return None;
            }
            let len = (0..).take_while(|&j| *ptr.add(j) != 0).count();
            str::from_utf8(slice::from_raw_parts(ptr, len))
                .ok()
                .map(str::to_owned)
        })
        .collect();
    crate::ffi::set_program_args(args);
}

/// Called at the end of every AOT executable's native `main` shim.
///
/// No-op by default. Set `SPECTRA_PAUSE_ON_EXIT=1` to opt in: on Windows, if
/// the process owns its console (i.e. it was launched by double-clicking in
/// Explorer rather than from a terminal), prints a "press any key" prompt and
/// waits so that the output window stays open long enough for the user to read
/// the output.
///
/// On all other platforms this is always a no-op.
#[no_mangle]
pub extern "C" fn spectra_rt_maybe_pause() {
    if std::env::var("SPECTRA_PAUSE_ON_EXIT").as_deref() != Ok("1") {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        // GetConsoleProcessList returns the number of processes attached to the
        // current console.  When the value is <= 1 this process is the sole
        // owner, which happens when the user double-clicks the .exe.
        extern "system" {
            fn GetConsoleProcessList(lpdwProcessList: *mut u32, dwProcessCount: u32) -> u32;
        }
        let standalone = unsafe {
            let mut pids = [0u32; 2];
            GetConsoleProcessList(pids.as_mut_ptr(), 2) <= 1
        };
        if standalone {
            use std::io::{Read, Write};
            let _ = std::io::stdout().flush();
            let _ = write!(
                std::io::stderr(),
                "\nAperte qualquer tecla para continuar..."
            );
            let _ = std::io::stderr().flush();
            // Read one byte — waits until the user presses Enter (or any key
            // that produces input on the console's stdin stream).
            let _ = std::io::stdin().read(&mut [0u8; 1]);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // On non-Windows platforms terminals remain open on exit — nothing to do.
    }
}
