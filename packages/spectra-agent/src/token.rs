//! `std.agent.token_count(text: string) returns int`.
//!
//! Tokenization is delegated to `spectra_runtime::stdlib::text_token_count`,
//! which shares the std.ml text-tokenizer definition (`ml_text_tokens`). This
//! module owns only the host-call ABI plumbing.

use spectra_runtime::ffi::{SpectraHostCallContext, HOST_STATUS_INVALID_ARGUMENT};

use crate::abi;

/// Host entry for `spectra.std.agent.token_count`.
pub(crate) extern "C" fn token_count_host(ctx: *mut SpectraHostCallContext) -> i32 {
    let Some(args) = abi::args(ctx) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if args.len() != 1 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let Some(text) = abi::read_string_arg(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let count = spectra_runtime::stdlib::text_token_count(&text);
    abi::write_value(ctx, count as i64)
}

/// Registers the `token_count` host function; returns the number of newly
/// inserted entries (0 when already registered).
pub(crate) fn register() -> usize {
    usize::from(spectra_runtime::ffi::register_host_function(
        crate::TOKEN_COUNT_HOST_CALL,
        token_count_host,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use spectra_runtime::ffi::{clear_host_functions, HOST_STATUS_SUCCESS};

    fn call(name: &str, args: &[i64]) -> (i32, i64) {
        let function = spectra_runtime::ffi::lookup_host_function(name).expect("registered");
        let mut results = [0 as i64; 1];
        let mut ctx = SpectraHostCallContext {
            args: args.as_ptr(),
            arg_len: args.len(),
            results: results.as_mut_ptr(),
            result_len: results.len(),
            invoke_fn: None,
        };
        (function(&mut ctx), results[0])
    }

    #[test]
    fn token_count_reports_required_and_invalid_arguments() {
        let _guard = crate::GLOBAL_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear_host_functions();
        assert_eq!(register(), 1);
        let text = unsafe { abi::alloc_string("alpha beta gamma") };
        let (status, count) = call(crate::TOKEN_COUNT_HOST_CALL, &[text]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_eq!(count, 3);
        // Wrong arity and a null string are rejected without panicking.
        assert_eq!(
            call(crate::TOKEN_COUNT_HOST_CALL, &[]).0,
            HOST_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            call(crate::TOKEN_COUNT_HOST_CALL, &[0]).0,
            HOST_STATUS_INVALID_ARGUMENT
        );
    }
}
