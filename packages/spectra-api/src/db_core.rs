use spectra_db::sqlite::{SqliteConnection, SqliteStatement, SqliteValue, StepResult};
use spectra_db::postgres::{
    Notification, NotificationListener, PostgresConfig, PostgresConnection,
    PostgresOperationCancellation, PostgresStatement, PostgresType, PostgresValue,
};
use spectra_db::CompiledQuery;
use spectra_db::redis::{RedisConfig, RedisConnection, RedisError, RedisValue};
use spectra_runtime::ffi::{
    HostFunction, SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
    HOST_STATUS_SUCCESS,
};
use crate::handles::ApiHandleTable;
use spectra_runtime::handles::HandleKind;
use spectra_runtime::tracing::{self, SpanKind, SpanStatus};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const POSTGRES_COPY_OUT_TEXT_LIMIT: usize = 16 * 1024 * 1024;

/// Per-handle last-error storage. Each driver handle owns a slot, and a
/// statement shares the slot of the connection it was prepared from, so
/// concurrent drivers never clobber each other's reported errors.
#[derive(Default)]
pub(crate) struct LastError {
    slot: Mutex<Option<(String, String)>>,
}

impl LastError {
    fn record<E: DriverError>(&self, error: &E) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some((
                error.driver_code().to_string(),
                error.driver_message().clone(),
            ));
        }
    }
    fn record_raw(&self, code: &'static str, message: impl Into<String>) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some((code.to_string(), message.into()));
        }
    }
    fn snapshot(&self) -> Option<(String, String)> {
        self.slot.lock().ok().and_then(|slot| slot.clone())
    }
}

trait DriverError {
    fn driver_code(&self) -> &'static str;
    fn driver_message(&self) -> &String;
}
impl DriverError for spectra_db::sqlite::SqliteError {
    fn driver_code(&self) -> &'static str {
        self.code
    }
    fn driver_message(&self) -> &String {
        &self.message
    }
}
impl DriverError for spectra_db::postgres::PostgresError {
    fn driver_code(&self) -> &'static str {
        self.code
    }
    fn driver_message(&self) -> &String {
        &self.message
    }
}
impl DriverError for RedisError {
    fn driver_code(&self) -> &'static str {
        self.code
    }
    fn driver_message(&self) -> &String {
        &self.message
    }
}

/// A stored driver object plus its own error slot. The value mutex is
/// per-handle: running one statement never holds up unrelated handles.
pub(crate) struct DriverHandle<T> {
    pub(crate) value: Mutex<T>,
    pub(crate) last_error: Arc<LastError>,
}

impl<T> DriverHandle<T> {
    fn boxed(value: T) -> Arc<Self> {
        Arc::new(Self {
            value: Mutex::new(value),
            last_error: Arc::new(LastError::default()),
        })
    }
    /// Create a handle that reports errors into an existing slot (a statement
    /// inheriting its connection's slot).
    fn boxed_with(value: T, last_error: Arc<LastError>) -> Arc<Self> {
        Arc::new(Self {
            value: Mutex::new(value),
            last_error,
        })
    }
}

struct Store {
    connections: ApiHandleTable<Arc<DriverHandle<SqliteConnection>>>,
    statements: ApiHandleTable<Arc<DriverHandle<SqliteStatement>>>,
    postgres_connections: ApiHandleTable<Arc<DriverHandle<PostgresConnection>>>,
    postgres_statements: ApiHandleTable<Arc<DriverHandle<PostgresStatement>>>,
    postgres_channels: ApiHandleTable<Arc<DriverHandle<NotificationListener>>>,
    postgres_notifications: ApiHandleTable<Notification>,
    redis_connections: ApiHandleTable<Arc<DriverHandle<RedisConnection>>>,
    pools: ApiHandleTable<Arc<spectra_db::sqlite::SqlitePool>>,
    pool_leases: HashMap<i64, PoolLease>,
}
fn store() -> &'static Mutex<Store> {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
    STORE.get_or_init(|| {
        Mutex::new(Store {
            connections: ApiHandleTable::new(HandleKind::DatabaseSqliteConnection),
            statements: ApiHandleTable::new(HandleKind::DatabaseSqliteStatement),
            postgres_connections: ApiHandleTable::new(HandleKind::DatabasePostgresConnection),
            postgres_statements: ApiHandleTable::new(HandleKind::DatabasePostgresStatement),
            postgres_channels: ApiHandleTable::new(HandleKind::DatabasePostgresChannel),
            postgres_notifications: ApiHandleTable::new(HandleKind::DatabasePostgresNotification),
            redis_connections: ApiHandleTable::new(HandleKind::DatabaseRedisConnection),
            pools: ApiHandleTable::new(HandleKind::Database),
            pool_leases: HashMap::new(),
        })
    })
}
fn alloc(value: &str) -> i64 {
    unsafe { tracing::alloc_string(value) }
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
unsafe fn args<'a>(ctx: *mut SpectraHostCallContext) -> Option<(&'a [i64], &'a mut [i64])> {
    if ctx.is_null() {
        return None;
    }
    let context = &*ctx;
    let input = if context.args.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(context.args, context.arg_len)
    };
    let output = if context.results.is_null() {
        &mut []
    } else {
        std::slice::from_raw_parts_mut(context.results, context.result_len)
    };
    Some((input, output))
}
fn value(result: &mut [i64], value: i64) -> i32 {
    if result.is_empty() {
        HOST_STATUS_INVALID_ARGUMENT
    } else {
        result[0] = value;
        HOST_STATUS_SUCCESS
    }
}
fn bool_result(result: &mut [i64], flag: bool) -> i32 {
    value(result, flag as i64)
}
fn fail(result: &mut [i64], _error: spectra_db::sqlite::SqliteError) -> i32 {
    value(result, 0)
}
fn fail_postgres(result: &mut [i64], _error: spectra_db::postgres::PostgresError) -> i32 {
    value(result, 0)
}

fn sqlite_connection_cell(
    id: i64,
) -> Result<Arc<DriverHandle<SqliteConnection>>, spectra_db::sqlite::SqliteError> {
    store()
        .lock()
        .map_err(|_| spectra_db::sqlite::SqliteError::new("DB2504_LOCK", "SQLite handle store lock poisoned"))?
        .connections
        .get(&id)
        .cloned()
        .ok_or_else(spectra_db::sqlite::SqliteError::invalid_handle)
}

fn finish_span(span: Option<u64>, success: bool) {
    if let Some(id) = span {
        let _ = tracing::span_set_attribute_bool(id, "db.error", !success);
        let _ = tracing::span_set_status(
            id,
            if success {
                SpanStatus::Ok
            } else {
                SpanStatus::Error
            },
        );
        let _ = tracing::span_end(id);
    }
}
fn operation_span(name: &str) -> Option<u64> {
    let span = tracing::begin_external_span(SpanKind::Internal, name).ok()?;
    let operation = name.strip_prefix("db.sqlite.").unwrap_or(name);
    let _ = tracing::span_set_attribute(span, "db.system", "sqlite");
    let _ = tracing::span_set_attribute(span, "db.operation", operation);
    let _ = tracing::span_set_attribute(span, "db.connection.mode", "file");
    Some(span)
}
fn redis_operation_span(name: &str) -> Option<u64> {
    let span = tracing::begin_external_span(SpanKind::Internal, name).ok()?;
    let operation = name.strip_prefix("db.redis.").unwrap_or(name);
    let _ = tracing::span_set_attribute(span, "db.system", "redis");
    let _ = tracing::span_set_attribute(span, "db.operation", operation);
    Some(span)
}

fn finish_redis_span(span: Option<u64>, success: bool) { finish_span(span, success); }
fn annotate_redis_span(span: Option<u64>, connection: &RedisConnection) {
    if let Some(id) = span {
        let config = connection.config();
        let _ = tracing::span_set_attribute(id, "server.address", &config.host);
        let _ = tracing::span_set_attribute_int(id, "server.port", config.port as i64);
        let _ = tracing::span_set_attribute_int(id, "db.namespace", config.database as i64);
    }
}
