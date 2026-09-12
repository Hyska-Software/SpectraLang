//! Agent governance primitives (plan milestone M4).
//!
//! Phase 32 introduces a run identity the runtime can consult on every
//! dispatch. This module currently owns the active-run context and its
//! fail-closed detached-work policy; capability enforcement (`R-3214`) and the
//! remaining governance items build on it.

pub mod run_context;
