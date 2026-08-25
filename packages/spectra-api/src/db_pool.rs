use spectra_db::migrations::{MigrationError, MigrationStatus, SqliteMigrator};
use spectra_db::sqlite::SqliteFactory;
use spectra_db::{ConnectionPool, PoolConfig, PooledConnection};

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
pub(crate) struct PoolLease {
    pub(crate) pool_raw: i64,
    pub(crate) pooled: Option<PooledConnection<SqliteFactory>>,
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
        let mut config = PoolConfig::default();
        config.max_size = a[1] as usize;
        let span = operation_span("db.pool.open");
        match ConnectionPool::new(SqliteFactory::new(&path), config) {
            Ok(pool) => {
                let mut state = store().lock().unwrap();
                let id = state.pools.insert(Arc::new(pool));
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
        let pool = store().lock().unwrap().pools.remove(&pool_raw);
        let Some(pool) = pool else {
            return fail(r, spectra_db::sqlite::SqliteError::invalid_handle());
        };
        let span = operation_span("db.pool.close");
        let result = pool.shutdown();
        finish_span(span, result.is_ok());
        match result {
            Ok(()) => bool_result(r, true),
            Err(error) => fail(r, pool_error(error)),
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
                let conn_raw = state.connections.insert(connection);
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

/// Resolve the first argument of a migrate host into a SQLite connection:
/// either a live `SqliteConnection` handle or a database path string.
unsafe fn resolve_migration_target(target: i64) -> Result<SqliteConnection, spectra_db::sqlite::SqliteError> {
    {
        let state = store().lock().unwrap_or_else(|error| error.into_inner());
        if let Some(connection) = state.connections.get(&target) {
            return Ok(connection.clone());
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

