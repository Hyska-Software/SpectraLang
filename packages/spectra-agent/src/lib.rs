//! Native implementation boundary for the `std.agent` package.
//!
//! R-3209 lands the namespace seam with exactly one function
//! (`std.agent.token_count`) so the cost of adding the next function is
//! mechanical. This crate is an `rlib` aggregated by
//! `spectra_api::register` (see `packages/spectra-api/src/api_registration.rs`):
//! it declares no staticlib, no `#[no_mangle]` symbol, and no linker entry.

mod token;

use spectra_runtime::ffi::register_host_function;

/// Runtime host-call name the midend lowers `std.agent.token_count` to.
pub const TOKEN_COUNT_HOST_CALL: &str = "spectra.std.agent.token_count";

/// Register this crate's host functions into the process-wide runtime
/// registry and return the number of newly inserted entries.
pub fn register() -> usize {
    let mut inserted = 0;
    if register_host_function(TOKEN_COUNT_HOST_CALL, token::token_count_host) {
        inserted += 1;
    }
    inserted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_inserts_token_count_once() {
        // Registration is idempotent per process: the first call inserts the
        // entry, a second call observes it already present.
        let first = register();
        let second = register();
        assert_eq!(first, 1);
        assert_eq!(second, 0);
    }
}
