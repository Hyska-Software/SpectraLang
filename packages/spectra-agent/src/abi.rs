//! Host-call ABI plumbing shared by the `std.agent` host functions.
//!
//! The host ABI carries scalars, handles and pointers only; records cross as
//! JSON documents (plan adaptation 11). This module owns the string codec and
//! the two-word tagged `Result` representation so every agent host function
//! writes its result through one path.

use spectra_runtime::ffi::{SpectraHostCallContext, SpectraHostValue};

/// Upper bound for scanning a NUL-terminated packed UTF-8 buffer, matching the
/// runtime's conservative `SPECTRA_STRING_SCAN_LIMIT` when the allocation is
/// not tracked by the runtime allocation table.
const STRING_SCAN_LIMIT: usize = 16 * 1024 * 1024;

/// Reads one Spectra string argument (packed UTF-8 bytes plus a single NUL).
pub(crate) fn read_string_arg(value: SpectraHostValue) -> Option<String> {
    if value == 0 {
        return None;
    }
    let ptr = value as *const u8;
    let mut bytes = Vec::new();
    let mut offset = 0usize;
    while offset < STRING_SCAN_LIMIT {
        let byte = unsafe { *ptr.add(offset) };
        if byte == 0 {
            break;
        }
        bytes.push(byte);
        offset += 1;
    }
    if offset >= STRING_SCAN_LIMIT {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Allocates a Spectra string in the runtime manual arena.
///
/// Mirrors the runtime's `alloc_spectra_string`: the packed representation and
/// the arena ownership are the ABI contract, so a host may hand the pointer to
/// compiled code.
pub(crate) unsafe fn alloc_string(value: &str) -> SpectraHostValue {
    spectra_runtime::tracing::alloc_string(value)
}

/// Allocates `words` zeroed arena words (used for the tagged `Result`).
pub(crate) unsafe fn alloc_words(words: usize) -> *mut i64 {
    let raw = spectra_runtime::ffi::spectra_rt_manual_alloc(words * 8);
    if raw.is_null() {
        return raw as *mut i64;
    }
    std::ptr::write_bytes(raw, 0, words * 8);
    raw as *mut i64
}

/// Writes `value` as the single host-call result.
pub(crate) fn write_value(ctx: *mut SpectraHostCallContext, value: SpectraHostValue) -> i32 {
    if ctx.is_null() {
        return spectra_runtime::ffi::HOST_STATUS_INVALID_ARGUMENT;
    }
    let ctx_ref = unsafe { &mut *ctx };
    let results = unsafe { ctx_ref.results_slice_mut() };
    if results.is_empty() {
        return spectra_runtime::ffi::HOST_STATUS_INVALID_ARGUMENT;
    }
    results[0] = value;
    spectra_runtime::ffi::HOST_STATUS_SUCCESS
}

/// Reads `expected` string/scalar arguments from a host-call context.
pub(crate) fn args<'a>(
    ctx: *mut SpectraHostCallContext,
) -> Option<&'a [SpectraHostValue]> {
    if ctx.is_null() {
        return None;
    }
    let ctx_ref = unsafe { &*ctx };
    if ctx_ref.arg_len == 0 || ctx_ref.args.is_null() {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len) })
}
