// Builtin (virtual) module registrations — decomposed into real child modules.
// Maps well-known `std.*` module paths to their exported function signatures
// without requiring physical `.spectra` files.  The actual implementation of
// each function lives in the runtime FFI layer (runtime/src/stdlib/mod.rs).

#[path = "builtin_contract.rs"]
mod builtin_contract;
#[path = "builtin_api_core.rs"]
mod builtin_api_core;
#[path = "builtin_api_services.rs"]
mod builtin_api_services;
#[path = "builtin_std_core.rs"]
mod builtin_std_core;
#[path = "builtin_ml_async.rs"]
mod builtin_ml_async;
#[path = "builtin_text_system.rs"]
mod builtin_text_system;
#[path = "builtin_option_time.rs"]
mod builtin_option_time;

// Re-unite the textually-merged sibling scopes: every child sees every other
// child's items through these crate-visible re-exports.
pub(crate) use builtin_api_core::*;
pub(crate) use builtin_api_services::*;
pub(crate) use builtin_std_core::*;
pub(crate) use builtin_ml_async::*;
pub(crate) use builtin_text_system::*;
pub(crate) use builtin_option_time::*;

// Public API preserved for external consumers (`dump_stdlib_contract` binary,
// snapshot tests, language_service and pipeline).
pub use builtin_api_core::register_builtin_modules;
pub use builtin_contract::{
    BuiltinContractSymbol, STD_API_MODULE_PATHS, STD_API_PUBLIC_FUNCTIONS,
    STD_API_PUBLIC_TYPES, STD_RANGE_PUBLIC_FUNCTIONS, STD_RANGE_PUBLIC_TYPES,
    STD_TIME_PUBLIC_FUNCTIONS, STD_TIME_PUBLIC_TYPES, builtin_contract_symbols,
};
