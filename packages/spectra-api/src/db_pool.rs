use spectra_db::migrations::{MigrationError, MigrationStatus, SqliteMigrator};
use spectra_db::sqlite::SqliteFactory;
use spectra_db::{ConnectionPool, PoolConfig, PooledConnection};
use spectra_db::ConnectionFactory;

/// A pool handle handed to the language plus the bookkeeping for one checked
/// out lease.
///
/// Lease cycle (no leak):
/// 1. `pool.sqlite_open(path, max_size)` stores an `Arc<SqlitePool>` in
///    `Store::pools`.
/// 2. `pool.with_connection(pool)` acquires blocking, registers a clone of the
///    underlying `SqliteConnection` in `Store::connections` under a fresh raw
///    handle and records a [`PoolLease`] keyed by that same raw value.
/// 3. While leased, every existing `spectra.api.db.sqlite.*` host works on the
///    connection unchanged (`prepare`, `step`, binds, transactions).
/// 4. The program returns the connection by calling the regular
///    `sqlite.close(conn)`: `sqlite_close` consults [`Store::pool_leases`],
///    finds the lease and calls [`PooledConnection::release`] so the physical
///    connection goes back to the idle queue instead of being closed.
/// 5. `pool.close(pool)` releases any leases the program forgot, shuts the
///    pool down and drops it, so no pooled connection can outlive the pool.
pub(crate) struct PoolLease<F: ConnectionFactory> {
    pub(crate) pool_raw: i64,
    pub(crate) pooled: Option<PooledConnection<F>>,
}

fn pool_error(error: impl std::fmt::Display) -> spectra_db::sqlite::SqliteError {
    spectra_db::sqlite::SqliteError::new("DB2504_POOL", error.to_string())
}

pub extern "C" fn pool_sqlite_open(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(path) = string(a[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if path.is_empty() {
            return fail(r, spectra_db::sqlite::SqliteError::new("DB2504_INVALID_PATH", "SQLite path is empty"));
        }
        if a[1] <= 0 || a[1] > 1024 {
            return fail(
                r,
                spectra_db::sqlite::SqliteError::new(
                    "DB2504_POOL",
                    "pool max_size must be between 1 and 1024",
                ),
            );
        }
        let config = PoolConfig {
            max_size: a[1] as usize,
            ..Default::default()
        };
        let span = operation_span("db.pool.open");
        match ConnectionPool::new(SqliteFactory::new(&path), config) {
            Ok(pool) => {
                let mut state = store().lock().unwrap();
                let id = state.pools.insert(Arc::new(AnyPool::Sqlite(Arc::new(pool))));
                finish_span(span, true);
                value(r, id)
            }
            Err(error) => {
                finish_span(span, false);
                fail(r, pool_error(error))
            }
        }
    }
}

pub extern "C" fn pool_close(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let pool_raw = a[0];
        // Release outstanding leases while the pool is still alive so every
        // checked out connection returns to the idle queue before shutdown.
        let forgotten: Vec<i64> = {
            let state = store().lock().unwrap();
            state
                .pool_leases
                .iter()
                .filter(|(_, lease)| lease.pool_raw == pool_raw)
                .map(|(raw, _)| *raw)
                .collect()
        };
        for raw in forgotten {
            release_lease(raw);
        }
        let forgotten_pg: Vec<i64> = {
            let state = store().lock().unwrap();
            state
                .postgres_pool_leases
                .iter()
                .filter(|(_, lease)| lease.pool_raw == pool_raw)
                .map(|(raw, _)| *raw)
                .collect()
        };
        for raw in forgotten_pg {
            release_postgres_lease(raw);
        }
        let forgotten_redis: Vec<i64> = {
            let state = store().lock().unwrap();
            state
                .redis_pool_leases
                .iter()
                .filter(|(_, lease)| lease.pool_raw == pool_raw)
                .map(|(raw, _)| *raw)
                .collect()
        };
        for raw in forgotten_redis {
            release_redis_lease(raw);
        }
        let pool = store().lock().unwrap().pools.remove(&pool_raw);
        let Some(pool) = pool else {
            return fail(r, spectra_db::sqlite::SqliteError::invalid_handle());
        };
        let span = operation_span("db.pool.close");
        match pool.as_ref() {
            AnyPool::Sqlite(pool) => {
                let result = pool.shutdown();
                finish_span(span, result.is_ok());
                match result {
                    Ok(()) => bool_result(r, true),
                    Err(error) => fail(r, pool_error(error)),
                }
            }
            AnyPool::Postgres(pool) => {
                let result = pool.shutdown();
                finish_span(span, result.is_ok());
                match result {
                    Ok(()) => bool_result(r, true),
                    Err(error) => fail_postgres(r, pool_postgres_error(error)),
                }
            }
            AnyPool::Redis(pool) => {
                let result = pool.shutdown();
                finish_span(span, result.is_ok());
                match result {
                    Ok(()) => bool_result(r, true),
                    Err(error) => fail_redis(r, pool_redis_error(error)),
                }
            }
        }
    }
}

pub extern "C" fn pool_with_connection(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let pool_raw = a[0];
        let pool = store().lock().unwrap().pools.get(&pool_raw).cloned();
        let Some(pool) = pool else {
            return fail(r, spectra_db::sqlite::SqliteError::invalid_handle());
        };
        let AnyPool::Sqlite(pool) = pool.as_ref() else {
            return fail(r, spectra_db::sqlite::SqliteError::invalid_handle());
        };
        let span = operation_span("db.pool.acquire");
        match pool.acquire_blocking() {
            Ok(pooled) => {
                let connection = match pooled.connection() {
                    Ok(connection) => connection.clone(),
                    Err(error) => {
                        finish_span(span, false);
                        return fail(r, pool_error(error));
                    }
                };
                let mut state = store().lock().unwrap();
                let conn_raw = state.connections.insert(DriverHandle::boxed(connection));
                state.pool_leases.insert(
                    conn_raw,
                    PoolLease {
                        pool_raw,
                        pooled: Some(pooled),
                    },
                );
                finish_span(span, true);
                value(r, conn_raw)
            }
            Err(error) => {
                finish_span(span, false);
                fail(r, pool_error(error))
            }
        }
    }
}

/// Return the lease registered under `conn_raw` to its pool. Used both by
/// `sqlite.close` on a leased connection and by `pool.close` cleanup.
pub(crate) fn release_lease(conn_raw: i64) -> bool {
    let lease = store().lock().unwrap().pool_leases.remove(&conn_raw);
    let Some(mut lease) = lease else {
        return false;
    };
    let Some(pooled) = lease.pooled.take() else {
        return true;
    };
    let span = operation_span("db.pool.release");
    let result = pooled.release();
    finish_span(span, result.is_ok());
    result.is_ok()
}

fn pool_postgres_error(error: impl std::fmt::Display) -> spectra_db::postgres::PostgresError {
    spectra_db::postgres::PostgresError::new("DB2505_POOL", error.to_string())
}

fn pool_redis_error(error: impl std::fmt::Display) -> RedisError {
    RedisError::new("DB2507_POOL", error.to_string())
}

/// Open a bounded PostgreSQL pool without connecting yet (`min_size` is
/// zero, so creation never touches the network). Checkout blocks up to the
/// pool's acquisition timeout, exactly like the SQLite pool.
pub extern "C" fn pool_postgres_open(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(url) = string(a[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a[1] <= 0 || a[1] > 1024 {
            return fail_postgres(
                r,
                spectra_db::postgres::PostgresError::new(
                    "DB2505_POOL",
                    "pool max_size must be between 1 and 1024",
                ),
            );
        }
        let config = match PostgresConfig::from_url(&url) {
            Ok(config) => config,
            Err(error) => return fail_postgres(r, error),
        };
        let pool_config = PoolConfig {
            max_size: a[1] as usize,
            ..Default::default()
        };
        let span = operation_span("db.pool.open");
        match ConnectionPool::new(PostgresFactory::new(config), pool_config) {
            Ok(pool) => {
                let mut state = store().lock().unwrap();
                let id = state.pools.insert(Arc::new(AnyPool::Postgres(Arc::new(pool))));
                finish_span(span, true);
                value(r, id)
            }
            Err(error) => {
                finish_span(span, false);
                fail_postgres(r, pool_postgres_error(error))
            }
        }
    }
}

/// Open a bounded Redis pool without connecting yet. Same laziness and
/// checkout-timeout contract as the PostgreSQL pool.
pub extern "C" fn pool_redis_open(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(url) = string(a[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a[1] <= 0 || a[1] > 1024 {
            return fail_redis(
                r,
                RedisError::new("DB2507_POOL", "pool max_size must be between 1 and 1024"),
            );
        }
        let config = match RedisConfig::from_url(&url) {
            Ok(config) => config,
            Err(error) => return fail_redis(r, error),
        };
        let pool_config = PoolConfig {
            max_size: a[1] as usize,
            ..Default::default()
        };
        let span = operation_span("db.pool.open");
        match ConnectionPool::new(RedisFactory { config }, pool_config) {
            Ok(pool) => {
                let mut state = store().lock().unwrap();
                let id = state.pools.insert(Arc::new(AnyPool::Redis(Arc::new(pool))));
                finish_span(span, true);
                value(r, id)
            }
            Err(error) => {
                finish_span(span, false);
                fail_redis(r, pool_redis_error(error))
            }
        }
    }
}

/// Check out a PostgreSQL connection from a pool opened with
/// `pool.postgres_open`. The leased handle lives in the regular PostgreSQL
/// connection table, so every existing `spectra.api.db.postgres.*` host
/// works on it unchanged; returning it via `postgres.close` releases the
/// lease back to the pool instead of closing the physical connection.
pub extern "C" fn pool_postgres_with_connection(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let pool_raw = a[0];
        let pool = store().lock().unwrap().pools.get(&pool_raw).cloned();
        let Some(pool) = pool else {
            return fail_postgres(r, spectra_db::postgres::PostgresError::invalid_handle());
        };
        let AnyPool::Postgres(pool) = pool.as_ref() else {
            return fail_postgres(r, spectra_db::postgres::PostgresError::invalid_handle());
        };
        let span = operation_span("db.pool.acquire");
        match pool.acquire_blocking() {
            Ok(pooled) => {
                let connection = match pooled.connection() {
                    Ok(connection) => connection.clone(),
                    Err(error) => {
                        finish_span(span, false);
                        return fail_postgres(r, pool_postgres_error(error));
                    }
                };
                let mut state = store().lock().unwrap();
                let conn_raw = state.postgres_connections.insert(DriverHandle::boxed(connection));
                state.postgres_pool_leases.insert(
                    conn_raw,
                    PoolLease {
                        pool_raw,
                        pooled: Some(pooled),
                    },
                );
                finish_span(span, true);
                value(r, conn_raw)
            }
            Err(error) => {
                finish_span(span, false);
                fail_postgres(r, pool_postgres_error(error))
            }
        }
    }
}

/// Check out a Redis connection from a pool opened with `pool.redis_open`.
/// Same lease contract as the PostgreSQL pool.
pub extern "C" fn pool_redis_with_connection(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let pool_raw = a[0];
        let pool = store().lock().unwrap().pools.get(&pool_raw).cloned();
        let Some(pool) = pool else {
            return fail_redis(r, RedisError::invalid_handle());
        };
        let AnyPool::Redis(pool) = pool.as_ref() else {
            return fail_redis(r, RedisError::invalid_handle());
        };
        let span = operation_span("db.pool.acquire");
        match pool.acquire_blocking() {
            Ok(pooled) => {
                let connection = match pooled.connection() {
                    Ok(connection) => connection.clone(),
                    Err(error) => {
                        finish_span(span, false);
                        return fail_redis(r, pool_redis_error(error));
                    }
                };
                let mut state = store().lock().unwrap();
                let conn_raw = state.redis_connections.insert(DriverHandle::boxed(connection));
                state.redis_pool_leases.insert(
                    conn_raw,
                    PoolLease {
                        pool_raw,
                        pooled: Some(pooled),
                    },
                );
                finish_span(span, true);
                value(r, conn_raw)
            }
            Err(error) => {
                finish_span(span, false);
                fail_redis(r, pool_redis_error(error))
            }
        }
    }
}

/// Return a PostgreSQL lease to its pool. Mirrors `release_lease`.
pub(crate) fn release_postgres_lease(conn_raw: i64) -> bool {
    let lease = store().lock().unwrap().postgres_pool_leases.remove(&conn_raw);
    let Some(mut lease) = lease else {
        return false;
    };
    let Some(pooled) = lease.pooled.take() else {
        return true;
    };
    let span = operation_span("db.pool.release");
    let result = pooled.release();
    finish_span(span, result.is_ok());
    result.is_ok()
}

/// Return a Redis lease to its pool. Mirrors `release_lease`.
pub(crate) fn release_redis_lease(conn_raw: i64) -> bool {
    let lease = store().lock().unwrap().redis_pool_leases.remove(&conn_raw);
    let Some(mut lease) = lease else {
        return false;
    };
    let Some(pooled) = lease.pooled.take() else {
        return true;
    };
    let span = operation_span("db.pool.release");
    let result = pooled.release();
    finish_span(span, result.is_ok());
    result.is_ok()
}

/// Resolve the first argument of a migrate host into a SQLite connection:
/// either a live `SqliteConnection` handle or a database path string.
unsafe fn resolve_migration_target(target: i64) -> Result<SqliteConnection, spectra_db::sqlite::SqliteError> {
    {
        let state = store().lock().unwrap_or_else(|error| error.into_inner());
        if let Some(cell) = state.connections.get(&target).cloned() {
            drop(state);
            let guard = cell
                .value
                .lock()
                .map_err(|_| spectra_db::sqlite::SqliteError::new("DB2504_LOCK", "SQLite connection lock poisoned"))?;
            return Ok(guard.clone());
        }
    }
    match string(target) {
        Some(path) => SqliteConnection::open(path, Duration::from_secs(5)),
        None => Err(spectra_db::sqlite::SqliteError::invalid_handle()),
    }
}

fn migration_error_to_sqlite(error: MigrationError) -> spectra_db::sqlite::SqliteError {
    spectra_db::sqlite::SqliteError::new(error.code, error.message)
}

pub extern "C" fn migrate_apply_sqlite(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(migrations_dir) = string(a[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let connection = match resolve_migration_target(a[0]) {
            Ok(connection) => connection,
            Err(error) => return fail(r, error),
        };
        let span = operation_span("db.migrate.apply");
        let migrator = match SqliteMigrator::from_directory(connection, &migrations_dir) {
            Ok(migrator) => migrator,
            Err(error) => {
                finish_span(span, false);
                return fail(r, migration_error_to_sqlite(error));
            }
        };
        match migrator.migrate() {
            Ok(entries) => {
                finish_span(span, true);
                let applied = entries
                    .iter()
                    .filter(|entry| entry.action == "applied")
                    .count() as i64;
                value(r, applied)
            }
            Err(error) => {
                finish_span(span, false);
                fail(r, migration_error_to_sqlite(error))
            }
        }
    }
}

fn migration_status_report(status: &MigrationStatus) -> String {
    let mut report = format!(
        "applied={} pending={} drift={}",
        status.applied.len(),
        status.pending.len(),
        status.drift.len()
    );
    for record in &status.applied {
        report.push_str(&format!(
            "\napplied {} {} {}",
            record.version, record.name, record.checksum
        ));
    }
    for migration in &status.pending {
        report.push_str(&format!(
            "\npending {} {}",
            migration.version, migration.name
        ));
    }
    for drift in &status.drift {
        report.push_str(&format!("\ndrift {} {}", drift.version, drift.reason));
    }
    report
}

pub extern "C" fn migrate_status_sqlite(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(migrations_dir) = string(a[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let connection = match resolve_migration_target(a[0]) {
            Ok(connection) => connection,
            Err(error) => return fail(r, error),
        };
        let span = operation_span("db.migrate.status");
        let migrator = match SqliteMigrator::from_directory(connection, &migrations_dir) {
            Ok(migrator) => migrator,
            Err(error) => {
                finish_span(span, false);
                return fail(r, migration_error_to_sqlite(error));
            }
        };
        match migrator.status() {
            Ok(status) => {
                finish_span(span, true);
                value(r, alloc(&migration_status_report(&status)))
            }
            Err(error) => {
                finish_span(span, false);
                fail(r, migration_error_to_sqlite(error))
            }
        }
    }
}

