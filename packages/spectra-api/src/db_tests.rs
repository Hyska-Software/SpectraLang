#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{ClientConfig, HttpClient};
    use crate::server::{Handler, HttpServer, ServerConfig, ServerResponse};
    use spectra_runtime::tracing::{self, SpanKind, SpanStatus};
    use std::env;
    use std::time::Duration;

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
        let _guard = crate::SHARED_REGISTRY_TEST_LOCK.lock().unwrap();
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
        SpectraHostCallContext, SpectraHostValue,
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
        let _guard = crate::SHARED_REGISTRY_TEST_LOCK.lock().unwrap();
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
    fn pool_postgres_open_validates_and_checkout_fails_cleanly_without_daemon() {
        let _guard = crate::SHARED_REGISTRY_TEST_LOCK.lock().unwrap();
        spectra_runtime::ffi::clear_host_functions();
        crate::register();

        let refused = crate::alloc_spectra_string("postgres://spectra:pool@127.0.0.1:9/spectra_pool_test");
        // Bad size bounds fail fast without touching the network.
        assert_eq!(
            call_host("spectra.api.db.pool.postgres_open", &[refused, 0]),
            (HOST_STATUS_SUCCESS, 0)
        );
        assert_eq!(
            call_host("spectra.api.db.pool.postgres_open", &[refused, 1025]),
            (HOST_STATUS_SUCCESS, 0)
        );
        let garbage = crate::alloc_spectra_string("not a postgres url");
        assert_eq!(
            call_host("spectra.api.db.pool.postgres_open", &[garbage, 2]),
            (HOST_STATUS_SUCCESS, 0)
        );
        // Unknown pools never hand out connections.
        assert_eq!(
            call_host("spectra.api.db.pool.postgres_with_connection", &[123_456_789]),
            (HOST_STATUS_SUCCESS, 0)
        );
        assert_eq!(
            call_host("spectra.api.db.pool.close", &[123_456_789]),
            (HOST_STATUS_SUCCESS, 0)
        );

        // Creation is lazy (min_size zero): opening against a refused port
        // succeeds; only checkout reports the typed connection error, fast.
        let (status, pool) =
            call_host("spectra.api.db.pool.postgres_open", &[refused, 2]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_ne!(pool, 0);
        assert_eq!(
            call_host("spectra.api.db.pool.postgres_with_connection", &[pool]),
            (HOST_STATUS_SUCCESS, 0)
        );
        let (close_status, closed) = call_host("spectra.api.db.pool.close", &[pool]);
        assert_eq!(close_status, HOST_STATUS_SUCCESS);
        assert_eq!(closed, 1);

        spectra_runtime::ffi::spectra_rt_manual_clear();
        spectra_runtime::ffi::clear_host_functions();
    }

    #[test]
    fn pool_redis_open_validates_and_checkout_fails_cleanly_without_daemon() {
        let _guard = crate::SHARED_REGISTRY_TEST_LOCK.lock().unwrap();
        spectra_runtime::ffi::clear_host_functions();
        crate::register();

        let refused = crate::alloc_spectra_string("redis://127.0.0.1:9/0");
        assert_eq!(
            call_host("spectra.api.db.pool.redis_open", &[refused, 0]),
            (HOST_STATUS_SUCCESS, 0)
        );
        assert_eq!(
            call_host("spectra.api.db.pool.redis_open", &[refused, -3]),
            (HOST_STATUS_SUCCESS, 0)
        );
        let garbage = crate::alloc_spectra_string("not a redis url");
        assert_eq!(
            call_host("spectra.api.db.pool.redis_open", &[garbage, 2]),
            (HOST_STATUS_SUCCESS, 0)
        );
        assert_eq!(
            call_host("spectra.api.db.pool.redis_with_connection", &[123_456_789]),
            (HOST_STATUS_SUCCESS, 0)
        );

        let (status, pool) = call_host("spectra.api.db.pool.redis_open", &[refused, 2]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_ne!(pool, 0);
        assert_eq!(
            call_host("spectra.api.db.pool.redis_with_connection", &[pool]),
            (HOST_STATUS_SUCCESS, 0)
        );
        // A SQLite pool is the wrong driver for a Redis checkout.
        let database = unique_temp_path("wrong-driver");
        let database_string = database.to_string_lossy().into_owned();
        let path_arg = crate::alloc_spectra_string(&database_string);
        let (_, sqlite_pool) =
            call_host("spectra.api.db.pool.sqlite_open", &[path_arg, 1]);
        assert_ne!(sqlite_pool, 0);
        assert_eq!(
            call_host(
                "spectra.api.db.pool.redis_with_connection",
                &[sqlite_pool]
            ),
            (HOST_STATUS_SUCCESS, 0)
        );
        assert_eq!(
            call_host("spectra.api.db.pool.close", &[sqlite_pool]),
            (HOST_STATUS_SUCCESS, 1)
        );

        let (close_status, closed) = call_host("spectra.api.db.pool.close", &[pool]);
        assert_eq!(close_status, HOST_STATUS_SUCCESS);
        assert_eq!(closed, 1);

        let _ = std::fs::remove_file(&database_string);
        spectra_runtime::ffi::spectra_rt_manual_clear();
        spectra_runtime::ffi::clear_host_functions();
    }

    #[test]
    fn pool_redis_leases_against_fake_server() {
        let _guard = crate::SHARED_REGISTRY_TEST_LOCK.lock().unwrap();
        spectra_runtime::ffi::clear_host_functions();
        crate::register();

        let port = spawn_fake_redis(0);
        let url = crate::alloc_spectra_string(&format!("redis://127.0.0.1:{port}/0"));
        let (status, pool) = call_host("spectra.api.db.pool.redis_open", &[url, 1]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_ne!(pool, 0);

        let (lease_status, conn) =
            call_host("spectra.api.db.pool.redis_with_connection", &[pool]);
        assert_eq!(lease_status, HOST_STATUS_SUCCESS);
        assert_ne!(conn, 0);

        let key = crate::alloc_spectra_string("spectra:pool:key");
        let value = crate::alloc_spectra_string("pooled");
        assert_eq!(
            call_host("spectra.api.db.redis.set", &[conn, key, value]),
            (HOST_STATUS_SUCCESS, 1)
        );
        let (get_status, fetched_ptr) =
            call_host("spectra.api.db.redis.get", &[conn, key]);
        assert_eq!(get_status, HOST_STATUS_SUCCESS);
        assert_eq!(
            unsafe { string(fetched_ptr) }.as_deref(),
            Some("pooled")
        );

        // Driver close releases the lease instead of closing the socket.
        assert_eq!(
            call_host("spectra.api.db.redis.close", &[conn]),
            (HOST_STATUS_SUCCESS, 1)
        );
        // The released connection is reusable through the same pool.
        let (re_status, conn2) =
            call_host("spectra.api.db.pool.redis_with_connection", &[pool]);
        assert_eq!(re_status, HOST_STATUS_SUCCESS);
        assert_ne!(conn2, 0);
        assert_eq!(
            call_host("spectra.api.db.redis.close", &[conn2]),
            (HOST_STATUS_SUCCESS, 1)
        );
        assert_eq!(
            call_host("spectra.api.db.pool.close", &[pool]),
            (HOST_STATUS_SUCCESS, 1)
        );

        spectra_runtime::ffi::spectra_rt_manual_clear();
        spectra_runtime::ffi::clear_host_functions();
    }

    #[test]
    fn pool_postgres_lease_cycle_against_daemon() {
        let Ok(url) = env::var("SPECTRA_POSTGRES_URL") else {
            eprintln!("skipping postgres pool test: SPECTRA_POSTGRES_URL is not set");
            return;
        };
        let _guard = crate::SHARED_REGISTRY_TEST_LOCK.lock().unwrap();
        spectra_runtime::ffi::clear_host_functions();
        crate::register();

        let url_arg = crate::alloc_spectra_string(&url);
        let (status, pool) = call_host("spectra.api.db.pool.postgres_open", &[url_arg, 2]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        assert_ne!(pool, 0);
        let (lease_status, conn) =
            call_host("spectra.api.db.pool.postgres_with_connection", &[pool]);
        assert_eq!(lease_status, HOST_STATUS_SUCCESS);
        assert_ne!(conn, 0);

        let ddl = crate::alloc_spectra_string(
            "CREATE TEMP TABLE spectra_pool_fixture(id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
        );
        let (prepare_status, create) =
            call_host("spectra.api.db.postgres.prepare", &[conn, ddl]);
        assert_eq!(prepare_status, HOST_STATUS_SUCCESS);
        assert_eq!(
            call_host("spectra.api.db.postgres.step", &[create]),
            (HOST_STATUS_SUCCESS, 2)
        );
        assert_eq!(
            call_host("spectra.api.db.postgres.finalize", &[create]),
            (HOST_STATUS_SUCCESS, 1)
        );

        let insert_sql = crate::alloc_spectra_string(
            "INSERT INTO spectra_pool_fixture(id, name) VALUES($1, $2)",
        );
        let (prepare_status, insert) =
            call_host("spectra.api.db.postgres.prepare", &[conn, insert_sql]);
        assert_eq!(prepare_status, HOST_STATUS_SUCCESS);
        let name = crate::alloc_spectra_string("pooled");
        assert_eq!(
            call_host("spectra.api.db.postgres.bind_int", &[insert, 1, 1]),
            (HOST_STATUS_SUCCESS, 1)
        );
        assert_eq!(
            call_host("spectra.api.db.postgres.bind_text", &[insert, 2, name]),
            (HOST_STATUS_SUCCESS, 1)
        );
        assert_eq!(
            call_host("spectra.api.db.postgres.step", &[insert]),
            (HOST_STATUS_SUCCESS, 2)
        );
        assert_eq!(
            call_host("spectra.api.db.postgres.finalize", &[insert]),
            (HOST_STATUS_SUCCESS, 1)
        );

        let query_sql = crate::alloc_spectra_string(
            "SELECT id, name FROM spectra_pool_fixture ORDER BY id",
        );
        let (prepare_status, query) =
            call_host("spectra.api.db.postgres.prepare", &[conn, query_sql]);
        assert_eq!(prepare_status, HOST_STATUS_SUCCESS);
        assert_eq!(
            call_host("spectra.api.db.postgres.step", &[query]),
            (HOST_STATUS_SUCCESS, 1)
        );
        assert_eq!(
            call_host("spectra.api.db.postgres.column_int", &[query, 0]),
            (HOST_STATUS_SUCCESS, 1)
        );
        assert_eq!(
            call_host("spectra.api.db.postgres.step", &[query]),
            (HOST_STATUS_SUCCESS, 2)
        );
        assert_eq!(
            call_host("spectra.api.db.postgres.finalize", &[query]),
            (HOST_STATUS_SUCCESS, 1)
        );

        assert_eq!(
            call_host("spectra.api.db.postgres.close", &[conn]),
            (HOST_STATUS_SUCCESS, 1)
        );
        assert_eq!(
            call_host("spectra.api.db.pool.close", &[pool]),
            (HOST_STATUS_SUCCESS, 1)
        );

        spectra_runtime::ffi::spectra_rt_manual_clear();
        spectra_runtime::ffi::clear_host_functions();
    }

    #[test]
    fn migration_hosts_apply_up_set_report_status_and_roll_back_in_tmpdir() {
        let _guard = crate::SHARED_REGISTRY_TEST_LOCK.lock().unwrap();
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
            let cell = store()
                .lock()
                .unwrap()
                .connections
                .get(&connection)
                .cloned()
                .unwrap();
            let native = cell.value.lock().unwrap().clone();
            let migrator = spectra_db::migrations::SqliteMigrator::from_directory(
                native,
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
    #[test]
    fn sqlite_execute_async_returns_task_int_awaited_via_block_on() {
        let _guard = crate::SHARED_REGISTRY_TEST_LOCK.lock().unwrap();
        spectra_runtime::ffi::clear_host_functions();
        crate::register();

        let database = unique_temp_path("async-task");
        let path_arg = crate::alloc_spectra_string(&database.to_string_lossy());
        let (open_status, connection) =
            call_host("spectra.api.db.sqlite.open", &[path_arg]);
        assert_eq!(open_status, HOST_STATUS_SUCCESS);

        // CREATE TABLE affects zero rows and must round-trip through the
        // Task<int> protocol exactly like postgres.execute_async.
        let create_sql = crate::alloc_spectra_string("CREATE TABLE t(a INTEGER)");
        let (task_status, task) =
            call_host("spectra.api.db.sqlite.execute_async", &[connection, create_sql]);
        assert_eq!(task_status, HOST_STATUS_SUCCESS);
        assert_ne!(task, 0, "execute_async must return a real task handle");
        let created = spectra_runtime::stdlib::block_on_task_value(task);
        assert_eq!(created, Ok(0), "CREATE TABLE reports zero affected rows");

        let insert_sql = crate::alloc_spectra_string("INSERT INTO t VALUES (42)");
        let (insert_status, insert_task) =
            call_host("spectra.api.db.sqlite.execute_async", &[connection, insert_sql]);
        assert_eq!(insert_status, HOST_STATUS_SUCCESS);
        assert_eq!(
            spectra_runtime::stdlib::block_on_task_value(insert_task),
            Ok(1),
            "single INSERT must report one affected row through the task"
        );

        // A failing statement fails the task and records the error on the
        // connection handle's own slot.
        let bad_sql = crate::alloc_spectra_string("THIS IS NOT SQL");
        let (bad_status, bad_task) =
            call_host("spectra.api.db.sqlite.execute_async", &[connection, bad_sql]);
        assert_eq!(bad_status, HOST_STATUS_SUCCESS);
        assert_eq!(
            spectra_runtime::stdlib::wait_task_terminal_status(bad_task),
            Ok(2),
            "invalid SQL must fail the task (status 2)"
        );
        let (_, error_code_ptr) =
            call_host("spectra.api.db.sqlite.last_error_code", &[connection]);
        let error_code = unsafe { string(error_code_ptr) }.unwrap_or_default();
        assert!(
            error_code.starts_with("DB2504_"),
            "per-handle last_error_code must surface the async failure: {error_code}"
        );

        call_host("spectra.api.db.sqlite.close", &[connection]);
        let _ = std::fs::remove_file(&database);
        spectra_runtime::ffi::spectra_rt_manual_clear();
        spectra_runtime::ffi::clear_host_functions();
    }

    /// Deterministic CPU-bound query: sum of a recursive CTE. Runs entirely
    /// inside `step`, so overlapping wall-clock windows prove two statements
    /// executed simultaneously instead of queueing behind a global lock.
    fn heavy_query(rows: i64) -> String {
        format!(
            "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x < {rows}) \
             SELECT count(*) FROM c"
        )
    }

    #[test]
    fn concurrent_sqlite_statements_overlap_instead_of_serializing() {
        let _guard = crate::SHARED_REGISTRY_TEST_LOCK.lock().unwrap();
        spectra_runtime::ffi::clear_host_functions();
        crate::register();

        let sql_text = heavy_query(6_000_000);
        let mut connections = Vec::new();
        let mut statements = Vec::new();
        let mut files = Vec::new();
        for _index in 0..2 {
            let database = unique_temp_path("overlap");
            files.push(database.clone());
            let (open_status, connection) = call_host(
                "spectra.api.db.sqlite.open",
                &[crate::alloc_spectra_string(&database.to_string_lossy())],
            );
            assert_eq!(open_status, HOST_STATUS_SUCCESS);
            let (prepare_status, statement) = call_host(
                "spectra.api.db.sqlite.prepare",
                &[connection, crate::alloc_spectra_string(&sql_text)],
            );
            assert_eq!(prepare_status, HOST_STATUS_SUCCESS);
            connections.push(connection);
            statements.push(statement);
        }

        type Window = std::sync::Arc<Mutex<(std::time::Instant, std::time::Instant)>>;
        let run_step = |statement: i64, window: Window| {
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                let (row_status, row) = call_host("spectra.api.db.sqlite.step", &[statement]);
                assert_eq!(row_status, HOST_STATUS_SUCCESS);
                assert_eq!(row, 1, "aggregate query must produce a row");
                window.lock().unwrap().0 = started;
                let finished = std::time::Instant::now();
                let (done_status, done) = call_host("spectra.api.db.sqlite.step", &[statement]);
                assert_eq!(done_status, HOST_STATUS_SUCCESS);
                assert_eq!(done, 2);
                let mut guard = window.lock().unwrap();
                guard.1 = finished;
            })
        };

        let first: Window = std::sync::Arc::new(Mutex::new((
            std::time::Instant::now(),
            std::time::Instant::now(),
        )));
        let second: Window = std::sync::Arc::new(Mutex::new((
            std::time::Instant::now(),
            std::time::Instant::now(),
        )));
        let worker_a = run_step(statements[0], std::sync::Arc::clone(&first));
        let worker_b = run_step(statements[1], std::sync::Arc::clone(&second));
        worker_a.join().unwrap();
        worker_b.join().unwrap();

        let (a_start, a_end) = *first.lock().unwrap();
        let (b_start, b_end) = *second.lock().unwrap();
        assert!(
            a_start < b_end && b_start < a_end,
            "the two statement steps must overlap in time \
             (a={:?} b={:?}); serialization on the global store mutex leaked back in",
            (a_start, a_end),
            (b_start, b_end)
        );

        for statement in statements {
            call_host("spectra.api.db.sqlite.finalize", &[statement]);
        }
        for connection in connections {
            call_host("spectra.api.db.sqlite.close", &[connection]);
        }
        for file in files {
            let _ = std::fs::remove_file(file);
        }
        spectra_runtime::ffi::spectra_rt_manual_clear();
        spectra_runtime::ffi::clear_host_functions();
    }

    /// Minimal RESP2 server implementing exactly the commands the Redis host
    /// surface exercises, so the async roundtrip runs against real socket I/O
    /// without requiring an external Redis daemon.
    fn spawn_fake_redis(delay_millis: u64) -> u16 {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            while let Ok((stream, _)) = listener.accept() {
                std::thread::spawn(move || {
                    let mut writer = match stream.try_clone() {
                        Ok(writer) => writer,
                        Err(_) => return,
                    };
                    let mut reader = BufReader::new(stream);
                    let mut store: std::collections::HashMap<Vec<u8>, Vec<u8>> =
                        std::collections::HashMap::new();
                    loop {
                        let mut header = String::new();
                        if reader.read_line(&mut header).unwrap_or(0) == 0 {
                            return;
                        }
                        let Some(argc) = header.trim().strip_prefix('*').and_then(|n| n.parse().ok()) else {
                            continue;
                        };
                        let mut argv: Vec<Vec<u8>> = Vec::with_capacity(argc);
                        for _ in 0..argc {
                            let mut bulk = String::new();
                            if reader.read_line(&mut bulk).unwrap_or(0) == 0 {
                                return;
                            }
                            let len: usize = match bulk.trim().strip_prefix('$').and_then(|n| n.parse().ok()) {
                                Some(len) => len,
                                None => continue,
                            };
                            let mut payload = vec![0_u8; len];
                            if reader.read_exact(&mut payload).is_err() {
                                return;
                            }
                            let mut crlf = [0_u8; 2];
                            if reader.read_exact(&mut crlf).is_err() {
                                return;
                            }
                            argv.push(payload);
                        }
                        if argv.is_empty() {
                            continue;
                        }
                        let command = String::from_utf8_lossy(&argv[0]).to_ascii_uppercase();
                        if command == "GET" && delay_millis > 0 {
                            std::thread::sleep(std::time::Duration::from_millis(delay_millis));
                        }
                        let reply = match command.as_str() {
                            "PING" => b"+PONG\r\n".to_vec(),
                            "SELECT" | "AUTH" | "CLIENT" => b"+OK\r\n".to_vec(),
                            "SET" => {
                                store.insert(argv[1].clone(), argv[2].clone());
                                b"+OK\r\n".to_vec()
                            }
                            "GET" => match store.get(&argv[1]) {
                                Some(value) => {
                                    let mut frame = format!("${}\r\n", value.len()).into_bytes();
                                    frame.extend_from_slice(value);
                                    frame.extend_from_slice(b"\r\n");
                                    frame
                                }
                                None => b"$-1\r\n".to_vec(),
                            },
                            "DEL" => {
                                let removed = argv[1..]
                                    .iter()
                                    .filter(|key| store.remove(*key).is_some())
                                    .count();
                                format!(":{removed}\r\n").into_bytes()
                            }
                            "EXISTS" => {
                                let present = argv[1..]
                                    .iter()
                                    .filter(|key| store.contains_key(*key))
                                    .count();
                                format!(":{present}\r\n").into_bytes()
                            }
                            "INCR" | "INCRBY" => {
                                let delta: i64 = argv
                                    .get(2)
                                    .map(|raw| String::from_utf8_lossy(raw).parse().unwrap_or(1))
                                    .unwrap_or(1);
                                let entry = store
                                    .entry(argv[1].clone())
                                    .or_insert_with(|| b"0".to_vec());
                                let current: i64 =
                                    String::from_utf8_lossy(entry).parse().unwrap_or(0);
                                let next = current + delta;
                                *entry = next.to_string().into_bytes();
                                format!(":{next}\r\n").into_bytes()
                            }
                            "EXPIRE" => {
                                if store.contains_key(&argv[1]) {
                                    b":1\r\n".to_vec()
                                } else {
                                    b":0\r\n".to_vec()
                                }
                            }
                            _ => b"+OK\r\n".to_vec(),
                        };
                        if writer.write_all(&reply).is_err() {
                            return;
                        }
                    }
                });
            }
        });
        port
    }

    #[test]
    fn redis_async_hosts_roundtrip_over_real_socket_and_cancel() {
        let _guard = crate::SHARED_REGISTRY_TEST_LOCK.lock().unwrap();
        spectra_runtime::ffi::clear_host_functions();
        crate::register();

        let port = spawn_fake_redis(0);
        let url = crate::alloc_spectra_string(&format!("redis://127.0.0.1:{port}/0"));
        let (open_status, open_task) = call_host("spectra.api.db.redis.open_async", &[url]);
        assert_eq!(open_status, HOST_STATUS_SUCCESS);
        let connection = spectra_runtime::stdlib::block_on_task_value(open_task)
            .expect("open_async must resolve to a connection handle");
        assert_ne!(connection, 0);

        // SET -> GET roundtrip through Task<string>.
        let key = crate::alloc_spectra_string("spectra:async:key");
        let value = crate::alloc_spectra_string("roundtrip");
        let (set_status, set_task) = call_host(
            "spectra.api.db.redis.set_async",
            &[connection, key, value],
        );
        assert_eq!(set_status, HOST_STATUS_SUCCESS);
        assert_eq!(
            spectra_runtime::stdlib::block_on_task_value(set_task),
            Ok(1),
            "set_async must resolve true"
        );
        let (get_status, get_task) =
            call_host("spectra.api.db.redis.get_async", &[connection, key]);
        assert_eq!(get_status, HOST_STATUS_SUCCESS);
        let fetched_ptr = spectra_runtime::stdlib::block_on_task_value(get_task)
            .expect("get_async must resolve");
        assert_eq!(
            unsafe { string(fetched_ptr) }.as_deref(),
            Some("roundtrip"),
            "get_async must return the stored value"
        );

        // INCR twice, EXISTS, EXPIRE, DELETE through the task protocol.
        let counter = crate::alloc_spectra_string("spectra:async:counter");
        let five = 5_i64;
        let (_, incr_task) =
            call_host("spectra.api.db.redis.incr_async", &[connection, counter, five]);
        assert_eq!(
            spectra_runtime::stdlib::block_on_task_value(incr_task),
            Ok(5),
            "first incr_async applies the increment amount"
        );
        let one = 1_i64;
        let (_, incr_again) =
            call_host("spectra.api.db.redis.incr_async", &[connection, counter, one]);
        assert_eq!(spectra_runtime::stdlib::block_on_task_value(incr_again), Ok(6));
        let (_, exists_task) =
            call_host("spectra.api.db.redis.exists_async", &[connection, key]);
        assert_eq!(spectra_runtime::stdlib::block_on_task_value(exists_task), Ok(1));
        let (_, expire_task) = call_host(
            "spectra.api.db.redis.expire_async",
            &[connection, key, 60_i64],
        );
        assert_eq!(spectra_runtime::stdlib::block_on_task_value(expire_task), Ok(1));
        let (_, delete_task) =
            call_host("spectra.api.db.redis.delete_async", &[connection, key]);
        assert_eq!(spectra_runtime::stdlib::block_on_task_value(delete_task), Ok(1));
        let (_, missing_task) =
            call_host("spectra.api.db.redis.exists_async", &[connection, key]);
        assert_eq!(
            spectra_runtime::stdlib::block_on_task_value(missing_task),
            Ok(0),
            "deleted key must no longer exist"
        );

        // Blocking variants stay functional as the documented compat surface.
        let compat_key = crate::alloc_spectra_string("spectra:blocking:key");
        let compat_value = crate::alloc_spectra_string("compat");
        let (blocking_set_status, blocking_set) = call_host(
            "spectra.api.db.redis.set",
            &[connection, compat_key, compat_value],
        );
        assert_eq!(blocking_set_status, HOST_STATUS_SUCCESS);
        assert_eq!(blocking_set, 1);
        let (_, blocking_get) =
            call_host("spectra.api.db.redis.get", &[connection, compat_key]);
        assert_eq!(unsafe { string(blocking_get) }.as_deref(), Some("compat"));

        // Per-handle error slot: an invalid handle reports through the driver
        // alias, not the sqlite global.
        let (_, ghost_get) =
            call_host("spectra.api.db.redis.get_async", &[0x7FFF_FFFF, key]);
        assert_eq!(ghost_get, 0, "invalid handle must fail the call");

        let (close_status, close_task) =
            call_host("spectra.api.db.redis.close_async", &[connection]);
        assert_eq!(close_status, HOST_STATUS_SUCCESS);
        assert_eq!(
            spectra_runtime::stdlib::block_on_task_value(close_task),
            Ok(1),
            "close_async must resolve true"
        );

        // Cancel path: a GET delayed long enough that it cannot finish before
        // cancellation must report the cancelled terminal status (1).
        let slow_port = spawn_fake_redis(30_000);
        let slow_url =
            crate::alloc_spectra_string(&format!("redis://127.0.0.1:{slow_port}/0"));
        let (_, slow_open) = call_host("spectra.api.db.redis.open_async", &[slow_url]);
        let slow_connection = spectra_runtime::stdlib::block_on_task_value(slow_open)
            .expect("slow fake redis must accept the handshake");
        let slow_key = crate::alloc_spectra_string("spectra:cancel:key");
        let (_, slow_get) =
            call_host("spectra.api.db.redis.get_async", &[slow_connection, slow_key]);
        assert_ne!(slow_get, 0);
        assert!(
            spectra_runtime::stdlib::cancel_task_handle(slow_get),
            "cancelling a pending redis task must succeed"
        );
        assert_eq!(
            spectra_runtime::stdlib::wait_task_terminal_status(slow_get),
            Ok(1),
            "cancelled redis task must report terminal status 1"
        );

        spectra_runtime::ffi::spectra_rt_manual_clear();
        spectra_runtime::ffi::clear_host_functions();
    }
}
