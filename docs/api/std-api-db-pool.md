# Connection pools (`std.api.db.pool`, `std.api.db.migrate`)

The language exposes the Rust-only pooling and migration machinery of
`packages/spectra-db` through two builtin modules:

| Language function | Host call | Signature |
| --- | --- | --- |
| `pool.sqlite_open` | `spectra.api.db.pool.sqlite_open` | `fn(string path, int max_size) -> Pool` |
| `pool.close` | `spectra.api.db.pool.close` | `fn(Pool) -> bool` |
| `pool.with_connection` | `spectra.api.db.pool.with_connection` | `fn(Pool) -> SqliteConnection` |
| `pool.postgres_open` | `spectra.api.db.pool.postgres_open` | `fn(string url, int max_size) -> Pool` |
| `pool.redis_open` | `spectra.api.db.pool.redis_open` | `fn(string url, int max_size) -> Pool` |
| `pool.postgres_with_connection` | `spectra.api.db.pool.postgres_with_connection` | `fn(Pool) -> PostgresConnection` |
| `pool.redis_with_connection` | `spectra.api.db.pool.redis_with_connection` | `fn(Pool) -> RedisConnection` |
| `migrate.apply_sqlite` | `spectra.api.db.migrate.apply_sqlite` | `fn(SqliteConnection, string migrations_dir) -> int` |
| `migrate.status_sqlite` | `spectra.api.db.migrate.status_sqlite` | `fn(SqliteConnection, string migrations_dir) -> string` |

Both migrate hosts also accept a database **path string** instead of a
connection handle; a live handle always wins if both interpretations exist.

## Lease lifecycle

`with_connection` performs a short blocking rental from
`spectra_db::sqlite::SqlitePool` (acquisition timeout 5s):

1. `sqlite_open(path, max_size)` creates the pool and stores it under a `Pool`
   handle.
2. `with_connection(pool)` acquires blocking, registers the underlying
   `SqliteConnection` in the regular SQLite handle table and records an
   internal lease keyed by that connection's raw handle.
3. While leased, every existing `std.api.db.sqlite.*` function works on the
   connection unchanged (`prepare`, binds, `step`, transactions).
4. Closing the lease is done with the ordinary `db.sqlite.close(conn)` host:
   it detects the lease and calls `PooledConnection::release`, returning the
   physical connection to the idle queue instead of destroying it.
5. `pool.close(pool)` releases any leases the program forgot, shuts the pool
   down and drops it, so no pooled connection outlives its pool.

## Fixed pool tuning

The open hosts expose only `max_size` (range-checked fail-fast to 1–1024
before any network or filesystem touch); every other knob is fixed at the
`PoolConfig::default` values from `packages/spectra-db/src/pool.rs`:
acquisition timeout 5s, connection timeout 5s, idle timeout 300s, shutdown
timeout 5s, minimum size 0. There is no tuning surface beyond `max_size` by
design, not by omission.

## PostgreSQL and Redis pools

`postgres_open(url, max_size)` and `redis_open(url, max_size)` mirror
`sqlite_open`: `max_size` must be between 1 and 1024, creation never touches
the network (the pool starts empty and connects lazily), and checkout blocks
up to the pool acquisition timeout (5s default) before failing with a typed
driver error (`DB2505_POOL` / `DB2507_POOL`).

Leased handles live in the regular driver tables, so every existing
`std.api.db.postgres.*` / `std.api.db.redis.*` function works on them
unchanged. Returning a lease uses the ordinary driver close
(`db.postgres.close`, `db.redis.close`), which detects the lease and releases
it back to the idle queue instead of closing the physical connection.
`pool.close` is shared across drivers: it releases forgotten leases, shuts
the pool down, and drops it. Passing a pool of the wrong driver to a
`with_connection` variant fails with an invalid-handle error.

```spectra
from std.api.db.pool import sqlite_open as pool_open, close as pool_close, with_connection
from std.api.db.sqlite import prepare, bind_text, step, finalize

public func main() returns int {
    let pool = pool_open("app-data.sqlite", 4)
    let conn = with_connection(pool)
    let insert = prepare(conn, "INSERT INTO kv(key) VALUES(?1)")
    // ... bind/step/finalize ...
    // closing the plain connection returns it to the pool
    // pool_close(pool) then drains and destroys the pool
    0
}
```

## Migrations

`apply_sqlite` delegates to `spectra_db::migrations::SqliteMigrator`, which
discovers `<version>_<name>.up.sql` / `.down.sql` pairs in the given
directory, tracks them in `_spectra_migrations` with checksums and refuses to
apply on checksum drift. It returns the number of versions applied;
re-invocation is idempotent (returns `0`).

`status_sqlite` renders a report line:

```
applied=<n> pending=<n> drift=<n>
applied 1 create_notes <checksum>
pending 2 seed_note
```

Regression fixture: `tests/validation/349_api_db_pool_migrations.spectra`
(composes `std.fs` writes with pool leasing and migration application).
