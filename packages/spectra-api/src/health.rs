use spectra_runtime::ffi::{
    HostFunction, SpectraHostCallContext, HOST_STATUS_INVALID_ARGUMENT, HOST_STATUS_SUCCESS,
};
use spectra_runtime::health::{HealthCategory, HealthError, HealthRegistry};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

/// Registers a real file-backed SQLite `SELECT 1` probe.
pub fn register_sqlite_check(
    registry: &HealthRegistry,
    name: impl Into<String>,
    path: impl Into<PathBuf>,
    timeout: Duration,
    required: bool,
) -> Result<(), HealthError> {
    let path = path.into();
    registry.register_check(
        name,
        HealthCategory::Database,
        timeout,
        required,
        move || {
            let connection = spectra_db::sqlite::SqliteConnection::open(&path, timeout)
                .map_err(|e| e.to_string())?;
            connection
                .execute_batch("SELECT 1;")
                .map_err(|e| e.to_string())
        },
    )
}

/// Registers a real TCP connect probe with a bounded connection timeout.
pub fn register_tcp_check(
    registry: &HealthRegistry,
    name: impl Into<String>,
    address: String,
    timeout: Duration,
    required: bool,
) -> Result<(), HealthError> {
    registry.register_check(
        name,
        HealthCategory::ExternalService,
        timeout,
        required,
        move || {
            TcpStream::connect_timeout(
                &address
                    .parse()
                    .map_err(|_| "invalid TCP address".to_string())?,
                timeout,
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        },
    )
}

/// Registers a real Redis PING probe. It is intended to run in the health
/// evaluator worker, never directly from an HTTP request handler.
pub fn register_redis_check(
    registry: &HealthRegistry,
    name: impl Into<String>,
    config: spectra_db::redis::RedisConfig,
    timeout: Duration,
    required: bool,
) -> Result<(), HealthError> {
    registry.register_check(name, HealthCategory::Cache, timeout, required, move || {
        let connection =
            spectra_db::redis::RedisConnection::open(config.clone()).map_err(|e| e.to_string())?;
        connection.ping_blocking().map_err(|e| e.to_string())
    })
}

/// Registers a real PostgreSQL `SELECT 1` probe.
pub fn register_postgres_check(
    registry: &HealthRegistry,
    name: impl Into<String>,
    config: spectra_db::postgres::PostgresConfig,
    timeout: Duration,
    required: bool,
) -> Result<(), HealthError> {
    registry.register_check(
        name,
        HealthCategory::Database,
        timeout,
        required,
        move || {
            let connection = spectra_db::postgres::PostgresConnection::open(config.clone())
                .map_err(|e| e.to_string())?;
            connection.health_check().map_err(|e| e.to_string())
        },
    )
}

unsafe fn args(ctx: *mut SpectraHostCallContext) -> Option<(&'static [i64], &'static mut [i64])> {
    if ctx.is_null() {
        return None;
    }
    let c = &*ctx;
    let input = if c.args.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(c.args, c.arg_len)
    };
    let output = if c.results.is_null() {
        &mut []
    } else {
        std::slice::from_raw_parts_mut(c.results, c.result_len)
    };
    Some((input, output))
}

unsafe fn string(value: i64) -> Option<String> {
    if value == 0 {
        return None;
    }
    let ptr = value as *const u8;
    let mut bytes = Vec::new();
    for index in 0..4096 {
        let byte = *ptr.add(index) as u8;
        if byte == 0 {
            return String::from_utf8(bytes).ok();
        }
        bytes.push(byte);
    }
    None
}

fn write_bool(results: &mut [i64], value: bool) -> i32 {
    if results.is_empty() {
        HOST_STATUS_INVALID_ARGUMENT
    } else {
        results[0] = value as i64;
        HOST_STATUS_SUCCESS
    }
}

pub extern "C" fn startup_complete(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if !a.is_empty() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        spectra_runtime::health::global().set_startup_complete();
        write_bool(r, true)
    }
}

pub extern "C" fn startup_failed(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 || string(a[0]).is_none() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        spectra_runtime::health::global().set_startup_failed(string(a[0]).unwrap_or_default());
        write_bool(r, true)
    }
}

pub const HOST_FUNCTIONS: &[(&str, HostFunction)] = &[
    ("spectra.api.health.startup_complete", startup_complete),
    ("spectra.api.health.startup_failed", startup_failed),
];
