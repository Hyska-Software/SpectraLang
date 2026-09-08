//! Typed, parameterized SQL query construction.
//!
//! The query module owns SQL generation but never opens connections or executes
//! SQL. Values are collected as parameters and are bound by the selected driver.

mod ast;
mod compile;
mod dialect;
mod error;

pub use ast::{
    Aggregate, AggregateArgument, Blob, Boolean, Column, ColumnRef, Delete, Expr, Insert, Integer,
    JoinKind, Null, Order, Predicate, Query, Real, Select, SqlType, Text, Update, Value,
};
pub use compile::{CompiledQuery, QueryOutput};
pub use dialect::{Dialect, PostgresDialect, SqliteDialect};
pub use error::QueryError;
