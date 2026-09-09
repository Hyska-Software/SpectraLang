//! Native implementation boundary for the `spectra.api` package.
//!
//! This crate owns the Phase 22 API host-call namespace. Later Phase 22 items
//! add protocol-complete HTTP parsing, servers, clients, routing, JSON, and
//! TLS on top of this registration layer.

// Public C ABI entry points validate the raw context pointer and its
// length-delimited buffers inside the shared `args` helper. They intentionally
// expose a safe status-code boundary to the runtime instead of propagating
// Rust's `unsafe` marker through every registered host function.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use spectra_runtime::ffi::{
    register_host_function, HostFunction, SpectraHostCallContext, SpectraHostValue,
    HOST_STATUS_INVALID_ARGUMENT, HOST_STATUS_SUCCESS,
};

#[cfg(feature = "http3")]
pub mod http3;
#[cfg(feature = "http3")]
mod http3_host;
mod grpc_host;
mod graphql_host;
pub mod client;
pub mod conformance;
pub mod cors;
pub mod db;
pub mod errors;
pub mod form;
pub mod handler;
mod handles;
pub mod health;
pub mod grpc;
pub mod graphql;
pub mod http;
pub mod http2;
pub mod json;
pub mod jwt;
pub mod middleware;
pub mod multipart;
pub mod oauth;
pub mod query;
pub mod routing;
pub mod security;
pub mod server;
pub mod session;
pub mod sse;
pub mod tls;
pub mod trace;
pub mod validation;
pub mod websocket;

pub const HOST_PREFIX: &str = "spectra.api.";
pub const VERSION_MAJOR: SpectraHostValue = 0;
pub const VERSION_MINOR: SpectraHostValue = 1;
pub const VERSION_PATCH: SpectraHostValue = 0;

include!("host_calls.rs");

include!("api_registration.rs");

include!("api_tests.rs");

/// Process-wide serialiser for tests that mutate the global host-function
/// registry (`api_tests` and `db::tests`). The two suites historically used
/// separate locks, which raced `clear_host_functions`/`register` pairs and
/// flaked unrelated tests once enough registry-mutating tests overlapped.
#[cfg(test)]
pub(crate) static SHARED_REGISTRY_TEST_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(()));
