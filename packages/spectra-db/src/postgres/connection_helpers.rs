fn validate_identifier(name: &str, kind: &str) -> PostgresResult<()> {
    if name.is_empty()
        || name.len() > 63
        || name
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || c == '_'))
    {
        return Err(PostgresError::invalid_argument(format!(
            "invalid PostgreSQL {kind} name"
        )));
    }
    Ok(())
}

fn encode_copy_value(value: &PostgresValue) -> String {
    match value {
        PostgresValue::Null => "\\N".to_owned(),
        PostgresValue::Bool(value) => value.to_string(),
        PostgresValue::Int16(value) => value.to_string(),
        PostgresValue::Int32(value) => value.to_string(),
        PostgresValue::Int64(value) => value.to_string(),
        PostgresValue::Float32(value) => value.to_string(),
        PostgresValue::Float64(value) => value.to_string(),
        PostgresValue::Text(value) => value
            .replace('\\', "\\\\")
            .replace('\t', "\\t")
            .replace('\n', "\\n")
            .replace('\r', "\\r"),
        PostgresValue::Bytes(value) => format!(
            "\\\\x{}",
            value.iter().map(|b| format!("{b:02x}")).collect::<String>()
        ),
        PostgresValue::Uuid(value) => value.to_string(),
        PostgresValue::Timestamp(value) => value.to_rfc3339(),
    }
}

fn query_error(error: QueryError) -> PostgresError {
    PostgresError::new("DB2505_QUERY", error.to_string())
}

fn with_postgres_span<T>(
    config: &PostgresConfig,
    name: &str,
    operation: &str,
    work: impl FnOnce() -> PostgresResult<T>,
) -> PostgresResult<T> {
    let span = tracing::begin_external_span(SpanKind::Client, name).ok();
    if let Some(id) = span {
        let _ = tracing::span_set_attribute(id, "db.system", "postgresql");
        let _ = tracing::span_set_attribute(id, "db.operation", operation);
        let _ = tracing::span_set_attribute(id, "server.address", &config.host);
        let _ = tracing::span_set_attribute_int(id, "server.port", config.port as i64);
        let _ = tracing::span_set_attribute(id, "db.namespace", &config.database);
    }
    let result = work();
    if let Some(id) = span {
        let _ = tracing::span_set_attribute_bool(id, "db.error", result.is_err());
        let _ = tracing::span_set_status(
            id,
            if result.is_ok() {
                SpanStatus::Ok
            } else {
                SpanStatus::Error
            },
        );
        let _ = tracing::span_end(id);
    }
    result
}

fn sqlite_to_postgres(value: SqliteValue) -> PostgresValue {
    match value {
        SqliteValue::Null => PostgresValue::Null,
        SqliteValue::Integer(value) => PostgresValue::Int64(value),
        SqliteValue::Real(value) => PostgresValue::Float64(value),
        SqliteValue::Text(value) => PostgresValue::Text(value),
        SqliteValue::Blob(value) => PostgresValue::Bytes(value),
    }
}

#[derive(Clone)]
pub struct PostgresFactory {
    pub config: PostgresConfig,
}
impl PostgresFactory {
    pub fn new(config: PostgresConfig) -> Self {
        Self { config }
    }
}
impl ConnectionFactory for PostgresFactory {
    type Connection = PostgresConnection;
    type Error = PostgresError;
    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        PostgresConnection::open(self.config.clone())
    }
    fn is_valid(&self, connection: &Self::Connection) -> bool {
        connection.health_check().is_ok()
    }
    fn close(&self, connection: Self::Connection) {
        let _ = connection.close();
    }
}
pub type PostgresPool = ConnectionPool<PostgresFactory>;
pub fn open_pool(config: PostgresConfig, pool: PoolConfig) -> PostgresResult<PostgresPool> {
    ConnectionPool::new(PostgresFactory::new(config), pool)
        .map_err(|e| PostgresError::new("DB2505_POOL", e.to_string()))
}
