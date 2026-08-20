#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{ClientConfig, HttpClient};
    use crate::server::{Handler, HttpServer, ServerConfig, ServerResponse};
    use spectra_runtime::tracing::{self, SpanKind, SpanStatus};
    use std::env;
    use std::sync::OnceLock;
    use std::time::Duration;

    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn traced_sqlite_operation(name: &str, operation: impl FnOnce() -> bool) -> bool {
        let span = tracing::begin_external_span(SpanKind::Internal, name).ok();
        let success = operation();
        if let Some(id) = span {
            let _ = tracing::span_set_attribute(id, "db.system", "sqlite");
            let _ = tracing::span_set_attribute(
                id,
                "db.operation",
                name.strip_prefix("db.sqlite.").unwrap_or(name),
            );
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
        success
    }

    #[test]
    #[ignore = "requires a real OTLP collector started by validate_r2504_sqlite.py"]
    fn sqlite_query_spans_preserve_http_parent() {
        let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let endpoint = env::var("SPECTRA_R2504_OTLP_ENDPOINT")
            .expect("validator must provide SPECTRA_R2504_OTLP_ENDPOINT");
        let config = tracing::config_new(&endpoint, "spectralang-r2504").unwrap();
        tracing::config_start(config).unwrap();
        let path = env::temp_dir().join(format!(
            "spectralang-r2504-http-{}.sqlite",
            std::process::id()
        ));
        let database = path.clone();
        let handler: Handler = std::sync::Arc::new(move |_request| {
            let mut connection = None;
            let opened =
                traced_sqlite_operation("db.sqlite.open", || {
                    match SqliteConnection::open(&database, Duration::from_secs(1)) {
                        Ok(value) => {
                            connection = Some(value);
                            true
                        }
                        Err(_) => false,
                    }
                });
            let prepared = connection
                .as_ref()
                .map(|connection| {
                    traced_sqlite_operation("db.sqlite.prepare", || {
                        connection
                            .execute_batch("CREATE TABLE IF NOT EXISTS items(value INTEGER)")
                            .is_ok()
                    })
                })
                .unwrap_or(false);
            let committed = connection
                .as_ref()
                .map(|connection| {
                    let began = traced_sqlite_operation("db.sqlite.transaction", || {
                        connection.begin().is_ok()
                    });
                    let inserted = began
                        && traced_sqlite_operation("db.sqlite.query", || {
                            connection
                                .execute_batch("INSERT INTO items(value) VALUES(1)")
                                .is_ok()
                        });
                    let committed = inserted
                        && traced_sqlite_operation("db.sqlite.commit", || {
                            connection.commit().is_ok()
                        });
                    let rollback_begin = traced_sqlite_operation("db.sqlite.transaction", || {
                        connection.begin().is_ok()
                    });
                    let rollback_insert = rollback_begin
                        && traced_sqlite_operation("db.sqlite.query", || {
                            connection
                                .execute_batch("INSERT INTO items(value) VALUES(2)")
                                .is_ok()
                        });
                    let rolled_back = rollback_insert
                        && traced_sqlite_operation("db.sqlite.rollback", || {
                            connection.rollback().is_ok()
                        });
                    committed && rolled_back
                })
                .unwrap_or(false);
            let queried = connection
                .as_ref()
                .map(|connection| {
                    traced_sqlite_operation("db.sqlite.query", || {
                        let Ok(mut statement) = SqliteStatement::prepare(
                            connection.clone(),
                            "SELECT COUNT(*) FROM items",
                        ) else {
                            return false;
                        };
                        statement.step().is_ok()
                    })
                })
                .unwrap_or(false);
            let closed = connection
                .map(|connection| {
                    traced_sqlite_operation("db.sqlite.close", || connection.close().is_ok())
                })
                .unwrap_or(false);
            let status = if opened && prepared && committed && queried && closed {
                200
            } else {
                500
            };
            ServerResponse::text(status, "sqlite")
        });
        let mut server = HttpServer::start(ServerConfig::default(), handler).unwrap();
        let client = HttpClient::new(ClientConfig::default().allow_private_networks(true));
        let response = client
            .get(&format!("http://{}/query", server.local_addr()))
            .unwrap();
        assert_eq!(response.status_code, 200);
        server.shutdown().unwrap();
        assert!(tracing::flush().is_ok());
        assert!(tracing::config_shutdown(config).is_ok());
        let _ = std::fs::remove_file(path);
    }
}
