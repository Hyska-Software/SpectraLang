# `spectra.api.db.sqlite`

The SQLite surface is backed by the bundled SQLite engine and operates on
file-backed databases. It is not an in-memory mock and does not perform SQL
string interpolation.

Parameter and column indexes are zero-based. `step` returns `1` for a row and
`2` when the statement is complete. Invalid handles and state transitions
return `false`/`0`; the stable error code and message of the target handle are
available through `last_error_code(connection)` and
`last_error_message(connection)`. Errors are stored per connection handle
(statements report into their owning connection's slot), so concurrent
drivers never overwrite each other's reported error.

Prepared statements support null, integer, floating-point, text and blob
bindings. A statement must be reset before it can be bound again after a
`step`, and `finalize` permanently invalidates the statement.

Transactions are explicit through `begin`, `commit` and `rollback`. Dropping
the native transaction object rolls back an active transaction. The driver
uses the shared `spectra-db` connection pool for asynchronous consumers.

`execute_async(connection, sql)` runs the blocking SQLite operation on a
dedicated worker and returns a `Task<int>` (same semantics as
`postgres.execute_async`) whose value is the affected row count, consumed by
the existing `std.async.task.poll`, `std.async.task.result`, `await` and
`block_on` protocol. It does not run SQLite work on the reactor thread.

R-2504 is complete. The independent v2 gate validates file-backed CRUD,
prepared statements, transactions, pool consumption, cancellation, a real
SQLite lock wait off the reactor thread, and SQLite spans parented by a real
HTTP server span and decoded from OTLP protobuf. PostgreSQL and Redis remain
separate roadmap items. The type-safe query builder and SQLite migration
framework are available in the Rust `spectra-db` package; they do not add new
`spectra.api` host calls in this compatibility surface.
