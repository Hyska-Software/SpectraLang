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
const PANIC_MESSAGE_SCAN_LIMIT_BYTES: usize = 16 * 1024 * 1024;

/// Deterministic exit code for unrecoverable Spectra runtime errors.
pub(crate) const SPECTRA_RUNTIME_PANIC_EXIT_CODE: i32 = 101;

/// Resolves a codegen-interned string literal to its text.
///
/// Codegen literals are packed UTF-8 byte buffers terminated by a single NUL
/// byte (heap-allocated by the JIT, or placed in `.rodata` by AOT). They are
/// not registered in the host string-value registry — that registry only
/// holds strings materialized through Spectra host calls — so resolution
/// uses the same bounded scan as the string fast paths in `ffi_fast_paths`:
/// the AllocationTable bounds the read for tracked allocations, otherwise
/// the scan stops after [`PANIC_MESSAGE_SCAN_LIMIT_BYTES`] bytes so reads
/// never run past the documented limit.
fn resolve_panic_message(ptr_val: SpectraHostValue) -> String {
    if ptr_val == 0 {
        return "unknown".to_string();
    }
    let limit = crate::ffi::manual_allocation_size(ptr_val)
        .unwrap_or(PANIC_MESSAGE_SCAN_LIMIT_BYTES);
    let raw = ptr_val as *const u8;
    let mut bytes: Vec<u8> = Vec::new();
    for offset in 0..limit {
        // SAFETY: offset stays within the tracked allocation (when known) or
        // within the documented scan limit (otherwise).
        let byte = unsafe { *raw.add(offset) };
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    String::from_utf8(bytes).unwrap_or_else(|_| "invalid utf-8 panic message".to_string())
}

/// Reports a fatal runtime error from generated code and exits.
///
/// `message_ptr` must point to a NUL-terminated buffer of packed UTF-8
/// bytes (`len + 1` bytes), matching every other string import in the
/// runtime ABI. The
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
