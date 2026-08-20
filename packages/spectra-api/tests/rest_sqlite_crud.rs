use serde_json::{json, Value};
use spectra_api::server::{Handler, HttpServer, ServerConfig, ServerResponse};
use spectra_db::migrations::SqliteMigrator;
use spectra_db::query::{
    Column, Delete, Insert, Integer, Order, Query, Select, SqliteDialect, Text, Update,
    Value as SqlValue,
};
use spectra_db::sqlite::{SqliteConnection, SqliteValue};
use spectra_runtime::health::{HealthCategory, HealthRegistry};
use spectra_runtime::metrics::MetricsRegistry;
use spectra_runtime::tracing::{self, SpanKind, SpanStatus};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn json_response(status: u16, value: Value) -> ServerResponse {
    ServerResponse {
        status_code: status,
        reason: if status == 200 { "OK" } else { "Error" }.into(),
        headers: vec![spectra_api::http::Header {
            name: "Content-Type".into(),
            value: "application/json".into(),
        }],
        body: spectra_api::http::HttpBody::from_bytes(value.to_string().into_bytes()),
        close: false,
    }
}

fn error_response(status: u16, message: &str) -> ServerResponse {
    json_response(status, json!({"error": message}))
}

fn query_span<T>(operation: &str, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let span = tracing::span_start("db.sqlite.query", SpanKind::Internal).ok();
    let result = f();
    if let Some(id) = span {
        let _ = tracing::span_set_attribute(id, "db.system", "sqlite");
        let _ = tracing::span_set_attribute(id, "db.operation", operation);
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

fn value_json(value: SqliteValue) -> Value {
    match value {
        SqliteValue::Null => Value::Null,
        SqliteValue::Integer(value) => json!(value),
        SqliteValue::Real(value) => json!(value),
        SqliteValue::Text(value) => json!(value),
        SqliteValue::Blob(value) => json!(value),
    }
}

fn request(addr: SocketAddr, method: &str, path: &str, body: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let request = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = response.split_whitespace().nth(1).unwrap().parse().unwrap();
    (
        status,
        response
            .split("\r\n\r\n")
            .nth(1)
            .unwrap_or_default()
            .to_string(),
    )
}

fn assert_transaction_rollback(connection: &SqliteConnection) {
    connection.begin().unwrap();
    connection
        .execute_query(spectra_db::CompiledQuery {
            sql: "INSERT INTO todos (id, title) VALUES (?1, ?2)".into(),
            params: vec![
                SqliteValue::Integer(1),
                SqliteValue::Text("original".into()),
            ],
        })
        .unwrap();
    let invalid = connection.execute_query(spectra_db::CompiledQuery {
        sql: "INSERT INTO todos (id, title) VALUES (?1, ?2)".into(),
        params: vec![
            SqliteValue::Integer(1),
            SqliteValue::Text("duplicate".into()),
        ],
    });
    assert!(invalid.is_err());
    connection.rollback().unwrap();
}

#[test]
fn real_http_sqlite_crud_uses_migrations_and_query_builder() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let configured_database = std::env::var("SPECTRA_R2511_DATABASE")
        .ok()
        .map(PathBuf::from);
    let temp = configured_database.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("spectralang-r2511-{}.sqlite", std::process::id()))
    });
    let _ = std::fs::remove_file(&temp);
    let connection = SqliteConnection::open(&temp, Duration::from_secs(2)).unwrap();
    let migrations = std::env::var("SPECTRA_R2511_MIGRATIONS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root.join("tests/fixtures/r2511/migrations"));
    let migrator = SqliteMigrator::from_directory(connection.clone(), migrations).unwrap();
    assert_eq!(migrator.migrate().unwrap().len(), 2);
    assert_transaction_rollback(&connection);

    let tracing_config = std::env::var("SPECTRA_R2511_OTLP_ENDPOINT")
        .ok()
        .and_then(|endpoint| {
            let id = tracing::config_new(&endpoint, "spectralang-r2511").ok()?;
            tracing::config_start(id).ok()?;
            Some(id)
        });

    let registry = MetricsRegistry::new();
    let health = HealthRegistry::new();
    let health_connection = connection.clone();
    health
        .register_check(
            "sqlite",
            HealthCategory::Database,
            Duration::from_secs(1),
            true,
            move || {
                health_connection
                    .execute_query(spectra_db::CompiledQuery {
                        sql: "SELECT 1".into(),
                        params: vec![],
                    })
                    .map(|_| ())
                    .map_err(|e| e.message)
            },
        )
        .unwrap();
    health.refresh().unwrap();
    health.set_startup_complete();

    let shared = Arc::new(Mutex::new(connection));
    let handler_connection = Arc::clone(&shared);
    let handler: Handler = Arc::new(move |request| {
        let connection = handler_connection.lock().unwrap().clone();
        let path = request.target.split('?').next().unwrap_or(&request.target);
        let result = query_span("crud", || {
            let todos_id = Column::<Integer>::new("id");
            let todos_title = Column::<Text>::new("title");
            match (request.method.as_str(), path) {
                ("GET", "/todos") => {
                    let query = Select::from("todos")
                        .columns_named(&[todos_id.reference(), todos_title.reference()])
                        .order_by(todos_id, Order::Asc);
                    let compiled = query.compile(&SqliteDialect).map_err(|e| e.to_string())?;
                    let rows = connection
                        .execute_query(compiled)
                        .map_err(|e| e.message)?
                        .rows;
                    Ok(json!(rows.into_iter().map(|row| json!({"id": value_json(row[0].clone()), "title": value_json(row[1].clone())})).collect::<Vec<_>>()))
                }
                ("POST", "/todos") => {
                    let body: Value = serde_json::from_slice(&request.body.bytes())
                        .map_err(|_| "invalid JSON".to_string())?;
                    let title = body
                        .get("title")
                        .and_then(Value::as_str)
                        .ok_or_else(|| "title is required".to_string())?;
                    let query = Insert::into("todos")
                        .set(
                            todos_id.clone(),
                            SqlValue::<Integer>::integer(
                                body.get("id").and_then(Value::as_i64).unwrap_or(1),
                            ),
                        )
                        .set(todos_title.clone(), SqlValue::<Text>::text(title));
                    let compiled = query.compile(&SqliteDialect).map_err(|e| e.to_string())?;
                    connection.execute_query(compiled).map_err(|e| e.message)?;
                    Ok(
                        json!({"id": body.get("id").and_then(Value::as_i64).unwrap_or(1), "title": title}),
                    )
                }
                ("GET", value) if value.starts_with("/todos/") => {
                    let id = value
                        .trim_start_matches("/todos/")
                        .parse::<i64>()
                        .map_err(|_| "invalid id".to_string())?;
                    let query = Select::from("todos")
                        .columns_named(&[todos_id.reference(), todos_title.reference()])
                        .where_(todos_id.equals(SqlValue::<Integer>::integer(id)));
                    let compiled = query.compile(&SqliteDialect).map_err(|e| e.to_string())?;
                    let rows = connection
                        .execute_query(compiled)
                        .map_err(|e| e.message)?
                        .rows;
                    rows.into_iter().next().map(|row| json!({"id": value_json(row[0].clone()), "title": value_json(row[1].clone())})).ok_or_else(|| "not found".into())
                }
                ("PUT", value) if value.starts_with("/todos/") => {
                    let id = value
                        .trim_start_matches("/todos/")
                        .parse::<i64>()
                        .map_err(|_| "invalid id".to_string())?;
                    let body: Value = serde_json::from_slice(&request.body.bytes())
                        .map_err(|_| "invalid JSON".to_string())?;
                    let title = body
                        .get("title")
                        .and_then(Value::as_str)
                        .ok_or_else(|| "title is required".to_string())?;
                    let query = Update::table("todos")
                        .set(todos_title.clone(), SqlValue::<Text>::text(title))
                        .where_(todos_id.equals(SqlValue::<Integer>::integer(id)));
                    let compiled = query.compile(&SqliteDialect).map_err(|e| e.to_string())?;
                    let result = connection.execute_query(compiled).map_err(|e| e.message)?;
                    if result.affected_rows == 0 {
                        return Err("not found".into());
                    }
                    Ok(json!({"id": id, "title": title}))
                }
                ("DELETE", value) if value.starts_with("/todos/") => {
                    let id = value
                        .trim_start_matches("/todos/")
                        .parse::<i64>()
                        .map_err(|_| "invalid id".to_string())?;
                    let query = Delete::from("todos")
                        .where_(todos_id.equals(SqlValue::<Integer>::integer(id)));
                    let compiled = query.compile(&SqliteDialect).map_err(|e| e.to_string())?;
                    let result = connection.execute_query(compiled).map_err(|e| e.message)?;
                    if result.affected_rows == 0 {
                        return Err("not found".into());
                    }
                    Ok(json!({"deleted": id}))
                }
                _ => Err("not found".into()),
            }
        });
        match result {
            Ok(value) => json_response(200, value),
            Err(error) if error == "not found" => error_response(404, &error),
            Err(error) if error == "invalid JSON" || error == "title is required" => {
                error_response(400, &error)
            }
            Err(error) => error_response(500, &error),
        }
    });
    let mut server = HttpServer::start(ServerConfig::default(), handler)
        .unwrap()
        .with_metrics_registry(registry)
        .with_health_registry(health.clone());
    let addr = server.local_addr();
    assert_eq!(request(addr, "GET", "/readyz", "").0, 200);
    assert_eq!(
        request(addr, "POST", "/todos", r#"{"id":1,"title":"first"}"#).0,
        200
    );
    assert_eq!(request(addr, "GET", "/todos/1", "").0, 200);
    assert_eq!(
        request(addr, "PUT", "/todos/1", r#"{"title":"updated"}"#).0,
        200
    );
    assert_eq!(request(addr, "GET", "/todos", "").0, 200);
    assert_eq!(request(addr, "POST", "/todos", "not-json").0, 400);
    assert_eq!(request(addr, "GET", "/todos/999", "").0, 404);
    let left =
        std::thread::spawn(move || request(addr, "POST", "/todos", r#"{"id":2,"title":"left"}"#).0);
    let right = std::thread::spawn(move || {
        request(addr, "POST", "/todos", r#"{"id":3,"title":"right"}"#).0
    });
    assert_eq!(left.join().unwrap(), 200);
    assert_eq!(right.join().unwrap(), 200);
    assert_eq!(request(addr, "DELETE", "/todos/1", "").0, 200);
    let (metrics_status, metrics_body) = request(addr, "GET", "/metrics", "");
    assert_eq!(metrics_status, 200);
    assert!(metrics_body.contains("spectra_http_requests_total"));
    assert!(metrics_body.contains("spectra_http_request_duration_seconds"));
    assert_eq!(request(addr, "GET", "/healthz", "").0, 200);
    assert_eq!(request(addr, "GET", "/startupz", "").0, 200);
    server.shutdown().unwrap();
    assert!(health.shutdown(Duration::from_secs(1)));
    if let Some(config) = tracing_config {
        tracing::flush().unwrap();
        tracing::config_shutdown(config).unwrap();
    }
    shared.lock().unwrap().close().unwrap();
    if configured_database.is_none() {
        let _ = std::fs::remove_file(temp);
    }
}
