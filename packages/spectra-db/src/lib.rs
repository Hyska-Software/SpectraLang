//! Shared database infrastructure for Spectra drivers.
//!
//! This crate intentionally contains no database protocol implementation. Drivers
//! such as SQLite and PostgreSQL provide a `ConnectionFactory` and consume the
//! pool without exposing a fake database surface.

mod error;
mod metrics;
pub mod migrations;
mod pool;
pub mod postgres;
pub mod query;
pub mod redis;
pub mod sqlite;

pub use error::{PoolError, PoolResult};
pub use metrics::PoolMetrics;
pub use pool::{ConnectionFactory, ConnectionPool, PoolConfig, PooledConnection};
pub use query::{CompiledQuery, Dialect, PostgresDialect, Query, QueryError, SqliteDialect};
