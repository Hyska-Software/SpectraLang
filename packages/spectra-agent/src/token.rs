//! `std.agent.token_count(text: string) returns int`.
//!
//! Tokenization is delegated to `spectra_runtime::stdlib::text_token_count`,
//! which shares the std.ml text-tokenizer definition (`ml_text_tokens`). This
//! module owns only the host-call ABI plumbing.

use spectra_runtime::ffi::{
    SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT, HOST_STATUS_SUCCESS,
};

/// Upper bound for scanning a NUL-terminated packed UTF-8 buffer, matching the
/// runtime's conservative `SPECTRA_STRING_SCAN_LIMIT` when the allocation is
/// not tracked by the runtime allocation table.
const STRING_SCAN_LIMIT: usize = 16 * 1024 * 1024;

/// Read a Spectra string argument (packed UTF-8 bytes plus a single NUL byte).
fn read_string_arg(value: SpectraHostValue) -> Option<String> {
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

/// Host entry for `spectra.std.agent.token_count`.
pub(crate) extern "C" fn token_count_host(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let ctx_ref = unsafe { &mut *ctx };
    let args = unsafe { ctx_ref.args_slice() };
    if args.len() != 1 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let Some(text) = read_string_arg(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let count = spectra_runtime::stdlib::text_token_count(&text);
    let results = unsafe { ctx_ref.results_slice_mut() };
    if results.is_empty() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    results[0] = count as SpectraHostValue;
    HOST_STATUS_SUCCESS
}
