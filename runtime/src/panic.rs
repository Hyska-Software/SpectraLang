//! Unrecoverable runtime error reporting for generated Spectra code.
//!
//! Generated JIT/AOT code calls [`spectra_rt_panic`] instead of emitting raw
//! `trap` instructions, so recoverable runtime failures (integer division by
//! zero, failed host calls) surface as Spectra diagnostics on stderr with a
//! deterministic exit code instead of an opaque native crash.
//!
//! The function is a normal call (never a trap), so it composes with the
//! runtime's own error handling; it terminates the process itself and never
//! returns.

use std::io::Write;

/// Primary scalar type exchanged through host call contexts.
use crate::ffi::SpectraHostValue;

/// Conservative ceiling for the bounded message scan when the pointer is not
/// tracked by the manual AllocationTable (mirrors `ffi_fast_paths`).
const PANIC_MESSAGE_SCAN_LIMIT_SLOTS: usize = 16 * 1024 * 1024;

/// Deterministic exit code for unrecoverable Spectra runtime errors.
pub(crate) const SPECTRA_RUNTIME_PANIC_EXIT_CODE: i32 = 101;

/// Resolves a codegen-interned string literal to its text.
///
/// Codegen literals are null-terminated buffers with one byte per `i64` slot
/// (heap-allocated by the JIT, or placed in `.rodata` by AOT). They are not
/// registered in the host string-value registry — that registry only holds
/// strings materialized through Spectra host calls — so resolution uses the
/// same bounded scan as the string fast paths in `ffi_fast_paths`: the
/// AllocationTable bounds the read for tracked allocations, otherwise the
/// scan stops after [`PANIC_MESSAGE_SCAN_LIMIT_SLOTS`] slots so reads never
/// run past the documented limit.
fn resolve_panic_message(ptr_val: SpectraHostValue) -> String {
    if ptr_val == 0 {
        return "unknown".to_string();
    }
    let limit = match crate::ffi::manual_allocation_size(ptr_val) {
        Some(bytes) => bytes / std::mem::size_of::<i64>(),
        None => PANIC_MESSAGE_SCAN_LIMIT_SLOTS,
    };
    let raw = ptr_val as *const i64;
    let mut bytes: Vec<u8> = Vec::new();
    for offset in 0..limit {
        // SAFETY: offset stays within the tracked allocation (when known) or
        // within the documented scan limit (otherwise).
        let slot = unsafe { *raw.add(offset) };
        if slot == 0 {
            break;
        }
        bytes.push(slot as u8);
    }
    String::from_utf8(bytes).unwrap_or_else(|_| "invalid utf-8 panic message".to_string())
}

/// Reports a fatal runtime error from generated code and exits.
///
/// `message_ptr` must point to a null-terminated buffer of one byte per
/// `i64` slot, matching every other string import in the runtime ABI. The
/// message is written to stderr as `runtime error: <message>`, flushed, and
/// the process terminates with exit code 101 (deterministic for CI).
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn spectra_rt_panic(message_ptr: i64) {
    let message = resolve_panic_message(message_ptr);
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "runtime error: {message}");
    let _ = stderr.flush();
    std::process::exit(SPECTRA_RUNTIME_PANIC_EXIT_CODE);
}
