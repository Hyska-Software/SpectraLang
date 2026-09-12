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

 /// Moves a manual allocation from the current function's frame to its parent frame,
 /// so that it survives the current function's `frame_exit` call.
 ///
 /// Only moves the allocation if it currently belongs to `current_frame_id` — this
 /// prevents accidentally re-parenting allocations that were passed in from the caller.
 /// If `ptr` is not a tracked allocation (e.g. a scalar value), this is a no-op.
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
/// Moves a manual allocation from the current function's frame to its parent frame,
/// so that it survives the current function's `frame_exit` call.
///
/// Only moves the allocation if it currently belongs to `current_frame_id` — this
/// prevents accidentally re-parenting allocations that were passed in from the caller.
/// If `ptr` is not a tracked allocation (e.g. a scalar value), this is a no-op.
#[no_mangle]
pub extern "C" fn spectra_rt_manual_escape(ptr: *mut u8, current_frame_id: usize) {
    if ptr.is_null() {
        return;
    }
    let ptr_value = ptr as usize;
    let table = allocation_table();
    let mut guard = table.lock().unwrap_or_else(|e| e.into_inner());

    // Only escape allocations that belong to the current frame.
    let old_frame_id = match guard.allocations.get(&ptr_value) {
        Some(entry) if entry.frame_id == current_frame_id => entry.frame_id,
        _ => return,
    };

    guard.remove_from_frame(old_frame_id, ptr_value);

    // Move the allocation to the base frame (0). Escaping only to the
    // immediate parent frame is unsafe: when the parent is itself a transient
    // callee frame (e.g. `Outer::create` calling `Inner::new`), the parent's
    // `frame_exit` would free the escaped allocation while the caller still
    // holds it. The base frame is never popped by `frame_exit`, so escaped
    // values remain valid for the lifetime of the program.
    let parent_frame_id = 0;

    if let Some(entry) = guard.allocations.get_mut(&ptr_value) {
        entry.frame_id = parent_frame_id;
    }

    if let Some(parent) = guard
        .frames
        .iter_mut()
        .rev()
        .find(|f| f.id == parent_frame_id)
    {
        parent.track(ptr_value);
    }
    let escaped_bytes = guard
        .allocations
        .get(&ptr_value)
        .map(|entry| entry.byte_len())
        .unwrap_or(0);
    drop(guard);

    // Monotonic accounting + budget enforcement. See the block comment above:
    // the counter never decrements; breaching it is fatal via spectra_rt_panic.
    let total =
        FRAME0_ESCAPED_BYTES.fetch_add(escaped_bytes, std::sync::atomic::Ordering::Relaxed)
            + escaped_bytes;
    if let Err(message) = frame0_budget_check(total, frame0_budget_mb()) {
        frame0_budget_abort(&message);
    }
}

/// Clears all outstanding manual allocations owned by the runtime.
#[no_mangle]
pub extern "C" fn spectra_rt_manual_clear() {
    let table = allocation_table();
    let mut guard = table.lock().unwrap_or_else(|e| e.into_inner());
    guard.clear_all();
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

    invoke_registered_host(name, args_ptr, arg_len, results_ptr, result_len)
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
    invoke_host_function(function, args_ptr, arg_len, results_ptr, result_len)
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
        let status = invoke_registered_host(
            name,
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
            let status = invoke_host_function_unchecked(
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
/// `HOST_STATUS_SUCCESS` on success, `HOST_STATUS_INVALID_ARGUMENT` if `fn_ptr == 0`,
/// or `HOST_STATUS_INTERNAL_ERROR` if `n_args` is outside the supported range.
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
    let closure_slots = fn_ptr as *const i64;
    if closure_slots.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let code_ptr = *closure_slots;
    if code_ptr == 0 {
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
    let _ = PROGRAM_ARGV.set(args);
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
