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
    use spectra_runtime::ffi::{
        SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT,
        HOST_STATUS_SUCCESS,
    };

    fn call_host(name: &str, args: &[SpectraHostValue]) -> (i32, SpectraHostValue) {
        let func = spectra_runtime::ffi::lookup_host_function(name).expect("host registered");
        let mut result = [0_i64];
        let mut ctx = SpectraHostCallContext {
            args: args.as_ptr(),
            arg_len: args.len(),
            results: result.as_mut_ptr(),
            result_len: result.len(),
            invoke_fn: None,
        };
        let status = func(&mut ctx as *mut _);
        (status, result[0])
    }

    fn unique_temp_path(label: &str) -> std::path::PathBuf {
        let mut path = env::temp_dir();
        path.push(format!(
            "spectralang-pool-{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        path
    }

    #[test]
    fn pool_hosts_lease_connections_concurrently_and_close_cleanly() {
        let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        spectra_runtime::ffi::clear_host_functions();
        crate::register();

        let database = unique_temp_path("conc");
        let database_string = database.to_string_lossy().into_owned();
        let path_arg = crate::alloc_spectra_string(&database_string);
        let max_size = 2_i64;
        let (status, pool) = call_host(
            "spectra.api.db.pool.sqlite_open",
            &[path_arg, max_size],
        );
        assert_eq!(status, HOST_STATUS_SUCCESS, "pool open failed");
        assert_ne!(pool, 0);

        let errors = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut workers = Vec::new();
        for _ in 0..4 {
            let errors = errors.clone();
            workers.push(std::thread::spawn(move || {
                // Each worker leases the only two pooled connections in turn;
                // acquisition must block instead of failing.
                for _round in 0..4u32 {
                    let (lease_status, conn) =
                        call_host("spectra.api.db.pool.with_connection", &[pool]);
                    if lease_status != HOST_STATUS_SUCCESS {
                        errors.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        return;
                    }
                    let table_sql = "CREATE TABLE IF NOT EXISTS leases(w INTEGER)";
                    let sql_arg = crate::alloc_spectra_string(table_sql);
                    let (prepare_status, statement) =
                        call_host("spectra.api.db.sqlite.prepare", &[conn, sql_arg]);
                    if prepare_status != HOST_STATUS_SUCCESS {
                        errors.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        return;
                    }
                    let (step_status, stepped) =
                        call_host("spectra.api.db.sqlite.step", &[statement]);
                    if step_status != HOST_STATUS_SUCCESS || stepped != 2 {
                        errors.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        return;
                    }
                    call_host("spectra.api.db.sqlite.finalize", &[statement]);
                    // Returning the lease must succeed on every round.
                    let (close_status, closed) =
                        call_host("spectra.api.db.sqlite.close", &[conn]);
                    if close_status != HOST_STATUS_SUCCESS || closed != 1 {
                        errors.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        return;
                    }
                }
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(errors.load(std::sync::atomic::Ordering::SeqCst), 0);

        let (close_status, closed) = call_host("spectra.api.db.pool.close", &[pool]);
        assert_eq!(close_status, HOST_STATUS_SUCCESS);
        assert_eq!(closed, 1, "pool shutdown timed out; leases leaked");

        // After close the pool handle must be gone.
        let (reacquire_status, reacquired) =
            call_host("spectra.api.db.pool.with_connection", &[pool]);
        assert_eq!(reacquire_status, HOST_STATUS_SUCCESS);
        assert_eq!(reacquired, 0, "closed pool must not hand out connections");

        let _ = std::fs::remove_file(&database_string);
        spectra_runtime::ffi::spectra_rt_manual_clear();
        spectra_runtime::ffi::clear_host_functions();
    }

    #[test]
    fn migration_hosts_apply_up_set_report_status_and_roll_back_in_tmpdir() {
        let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        spectra_runtime::ffi::clear_host_functions();
        crate::register();

        let root = unique_temp_path("migrations");
        let migrations_dir = root.join("migrations");
        std::fs::create_dir_all(&migrations_dir).unwrap();
        std::fs::write(
            migrations_dir.join("0001_create_users.up.sql"),
            "CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
        )
        .unwrap();
        std::fs::write(
            migrations_dir.join("0001_create_users.down.sql"),
            "DROP TABLE users;",
        )
        .unwrap();
        std::fs::write(
            migrations_dir.join("0002_add_users_email.up.sql"),
            "ALTER TABLE users ADD COLUMN email TEXT NOT NULL DEFAULT '';",
        )
        .unwrap();
        std::fs::write(
            migrations_dir.join("0002_add_users_email.down.sql"),
            "ALTER TABLE users DROP COLUMN email;",
        )
        .unwrap();

        let database = root.join("state.sqlite");
        let path_arg = crate::alloc_spectra_string(&database.to_string_lossy());
        let dir_arg = crate::alloc_spectra_string(&migrations_dir.to_string_lossy());

        let (open_status, connection) =
            call_host("spectra.api.db.sqlite.open", &[path_arg]);
        assert_eq!(open_status, HOST_STATUS_SUCCESS);

        // Apply N_up = 2 versions.
        let (apply_status, applied) =
            call_host("spectra.api.db.migrate.apply_sqlite", &[connection, dir_arg]);
        assert_eq!(apply_status, HOST_STATUS_SUCCESS, "apply failed");
        assert_eq!(applied, 2, "expected both pending migrations to apply");

        // Re-applying is a no-op.
        let (reapply_status, reapplied) =
            call_host("spectra.api.db.migrate.apply_sqlite", &[connection, dir_arg]);
        assert_eq!(reapply_status, HOST_STATUS_SUCCESS);
        assert_eq!(reapplied, 0, "applied migrations must not re-apply");

        // Status reports the applied set with no drift or pending work.
        let (report_status, report) =
            call_host("spectra.api.db.migrate.status_sqlite", &[connection, dir_arg]);
        assert_eq!(report_status, HOST_STATUS_SUCCESS);
        let report_text = unsafe { string(report) }.expect("status report string");
        assert!(report_text.starts_with("applied=2 pending=0 drift=0"), "{report_text}");
        assert!(report_text.contains("applied 1 create_users"), "{report_text}");
        assert!(report_text.contains("applied 2 add_users_email"), "{report_text}");

        // Down set: roll back both versions through the migrator directly and
        // confirm status returns to an empty ledger.
        {
            let migrator = spectra_db::migrations::SqliteMigrator::from_directory(
                store()
                    .lock()
                    .unwrap()
                    .connections
                    .get(&connection)
                    .cloned()
                    .unwrap(),
                &migrations_dir,
            )
            .unwrap();
            let rolled_back = migrator.rollback(2).unwrap();
            assert_eq!(rolled_back.len(), 2);
            assert!(rolled_back.iter().all(|entry| entry.action == "rolled_back"));
            let status = migrator.status().unwrap();
            assert!(status.applied.is_empty() && status.pending.len() == 2);
        }

        call_host("spectra.api.db.sqlite.close", &[connection]);
        let _ = std::fs::remove_dir_all(&root);
        spectra_runtime::ffi::spectra_rt_manual_clear();
        spectra_runtime::ffi::clear_host_functions();
    }
}
