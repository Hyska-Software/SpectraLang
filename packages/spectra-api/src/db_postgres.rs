pub extern "C" fn sqlite_open(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = string(a[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let span = operation_span("db.sqlite.open");
        match SqliteConnection::open(path, std::time::Duration::from_secs(5)) {
            Ok(connection) => {
                let mut state = store().lock().unwrap();
                let id = state.connections.insert(DriverHandle::boxed(connection));
                finish_span(span, true);
                value(r, id)
            }
            Err(error) => {
                finish_span(span, false);
                fail(r, error)
            }
        }
    }
}
pub extern "C" fn sqlite_close(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (cell, leased) = {
            let mut state = store().lock().unwrap();
            (
                state.connections.remove(&a[0]),
                state.pool_leases.contains_key(&a[0]),
            )
        };
        let Some(cell) = cell else {
            return fail(r, spectra_db::sqlite::SqliteError::invalid_handle());
        };
        // A pooled lease goes back to its pool instead of being closed; the
        // underlying physical connection stays alive inside the pool.
        if leased {
            drop(cell);
            if release_lease(a[0]) {
                bool_result(r, true)
            } else {
                fail(r, spectra_db::sqlite::SqliteError::new("DB2504_POOL", "pool lease was already released"))
            }
        } else {
            let span = operation_span("db.sqlite.close");
            let result = cell
                .value
                .lock()
                .map_err(|_| spectra_db::sqlite::SqliteError::new("DB2504_LOCK", "SQLite connection lock poisoned"))
                .and_then(|connection| connection.close());
            if let Err(error) = &result {
                cell.last_error.record(error);
            }
            finish_span(span, result.is_ok());
            match result {
                Ok(()) => bool_result(r, true),
                Err(error) => fail(r, error),
            }
        }
    }
}
pub extern "C" fn sqlite_prepare(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(sql) = string(a[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let cell = match sqlite_connection_cell(a[0]) {
            Ok(cell) => cell,
            Err(error) => return fail(r, error),
        };
        let span = operation_span("db.sqlite.prepare");
        let prepared = cell
            .value
            .lock()
            .map_err(|_| spectra_db::sqlite::SqliteError::new("DB2504_LOCK", "SQLite connection lock poisoned"))
            .and_then(|connection| SqliteStatement::prepare(connection.clone(), sql));
        match prepared {
            Ok(statement) => {
                let mut state = store().lock().unwrap();
                // Statements report errors into their owning connection's
                // slot so `last_error_code(connection)` sees step failures.
                let id = state
                    .statements
                    .insert(DriverHandle::boxed_with(statement, Arc::clone(&cell.last_error)));
                finish_span(span, true);
                value(r, id)
            }
            Err(error) => {
                cell.last_error.record(&error);
                finish_span(span, false);
                fail(r, error)
            }
        }
    }
}
pub extern "C" fn sqlite_execute_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(sql) = string(a[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let cell = match sqlite_connection_cell(a[0]) {
            Ok(cell) => cell,
            Err(error) => return fail(r, error),
        };
        // Same semantics as postgres.execute_async: the statement runs on the
        // background executor and the host returns a cancellable Task<int>
        // whose value is the affected row count.
        let task = spectra_runtime::stdlib::spawn_background_task(move || {
            let guard = match cell.value.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    cell.last_error.record(&spectra_db::sqlite::SqliteError::new(
                        "DB2504_LOCK",
                        "SQLite connection lock poisoned",
                    ));
                    return Err(());
                }
            };
            let result = SqliteStatement::prepare(guard.clone(), sql)
                .and_then(|mut statement| {
                    statement.step()?;
                    statement
                        .affected_rows()
                        .map(|rows| rows as i64)
                });
            match result {
                Ok(rows) => Ok(rows),
                Err(error) => {
                    cell.last_error.record(&error);
                    Err(())
                }
            }
        });
        match task {
            Ok(task_id) => value(r, task_id),
            Err(_) => {
                let error = spectra_db::sqlite::SqliteError::new(
                    "DB2504_ASYNC_QUEUE_FULL",
                    "SQLite background queue is full",
                );
                fail(r, error)
            }
        }
    }
}

/// Run `operation` against one SQLite statement while holding only that
/// statement's own mutex (never the global handle-store mutex), recording
/// failures into its shared per-connection error slot.
fn with_statement<T>(
    id: i64,
    operation: impl FnOnce(&mut SqliteStatement) -> Result<T, spectra_db::sqlite::SqliteError>,
) -> Result<T, spectra_db::sqlite::SqliteError> {
    let cell = store()
        .lock()
        .map_err(|_| {
            spectra_db::sqlite::SqliteError::new("DB2504_LOCK", "SQLite handle store lock poisoned")
        })?
        .statements
        .get(&id)
        .cloned()
        .ok_or_else(spectra_db::sqlite::SqliteError::invalid_handle)?;
    let mut statement = cell
        .value
        .lock()
        .map_err(|_| spectra_db::sqlite::SqliteError::new("DB2504_LOCK", "SQLite statement lock poisoned"))?;
    operation(&mut statement).inspect_err(|error| {
        cell.last_error.record(error);
    })
}

pub extern "C" fn postgres_open(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(url) = string(a[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        let config = match PostgresConfig::from_url(&url) { Ok(config) => config, Err(error) => return fail_postgres(r, error) };
        match PostgresConnection::open(config) {
            Ok(connection) => { let mut state = store().lock().unwrap(); let id = state.postgres_connections.insert(DriverHandle::boxed(connection)); value(r, id) }
            Err(error) => fail_postgres(r, error),
        }
    }
}

pub extern "C" fn postgres_close(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let (cell, leased) = {
            let mut state = store().lock().unwrap();
            (
                state.postgres_connections.remove(&a[0]),
                state.postgres_pool_leases.contains_key(&a[0]),
            )
        };
        let Some(cell) = cell else { return fail_postgres(r, spectra_db::postgres::PostgresError::invalid_handle()) };
        // A pooled lease goes back to its pool instead of being closed; the
        // underlying physical connection stays alive inside the pool.
        if leased {
            drop(cell);
            if release_postgres_lease(a[0]) {
                bool_result(r, true)
            } else {
                fail_postgres(r, spectra_db::postgres::PostgresError::new("DB2505_POOL", "pool lease was already released"))
            }
        } else {
        let result = cell
            .value
            .lock()
            .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL connection lock poisoned"))
            .and_then(|connection| connection.close());
        if let Err(error) = &result {
            cell.last_error.record(error);
        }
        match result { Ok(()) => bool_result(r, true), Err(error) => fail_postgres(r, error) }
        }
    }
}

/// Run `operation` against the PostgreSQL connection stored under `id`,
/// holding only that handle's own mutex and recording failures into its
/// per-handle error slot.
fn with_postgres_connection<T>(
    id: i64,
    operation: impl FnOnce(&PostgresConnection) -> Result<T, spectra_db::postgres::PostgresError>,
) -> Result<T, spectra_db::postgres::PostgresError> {
    let cell = postgres_connection_cell(id)?;
    let guard = cell
        .value
        .lock()
        .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL connection lock poisoned"))?;
    operation(&guard).inspect_err(|error| {
        cell.last_error.record(error);
    })
}

fn postgres_connection_cell(
    id: i64,
) -> Result<Arc<DriverHandle<PostgresConnection>>, spectra_db::postgres::PostgresError> {
    store()
        .lock()
        .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL handle store lock poisoned"))?
        .postgres_connections
        .get(&id)
        .cloned()
        .ok_or_else(spectra_db::postgres::PostgresError::invalid_handle)
}

pub extern "C" fn postgres_prepare(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 2 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(sql) = string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        let cell = match postgres_connection_cell(a[0]) {
            Ok(cell) => cell,
            Err(error) => return fail_postgres(r, error),
        };
        let prepared = cell
            .value
            .lock()
            .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL connection lock poisoned"))
            .and_then(|connection| connection.prepare(sql));
        match prepared {
            Ok(statement) => {
                let mut state = store().lock().unwrap();
                let id = state.postgres_statements.insert(DriverHandle::boxed_with(
                    statement,
                    Arc::clone(&cell.last_error),
                ));
                value(r, id)
            }
            Err(error) => {
                cell.last_error.record(&error);
                fail_postgres(r, error)
            }
        }
    }
}

/// Run `operation` against one PostgreSQL statement while holding only that
/// statement's own mutex, recording failures into its shared per-connection
/// error slot.
fn with_postgres_statement<T>(
    id: i64,
    operation: impl FnOnce(&mut PostgresStatement) -> Result<T, spectra_db::postgres::PostgresError>,
) -> Result<T, spectra_db::postgres::PostgresError> {
    let cell = store()
        .lock()
        .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL handle store lock poisoned"))?
        .postgres_statements
        .get(&id)
        .cloned()
        .ok_or_else(spectra_db::postgres::PostgresError::invalid_handle)?;
    let mut statement = cell
        .value
        .lock()
        .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL statement lock poisoned"))?;
    operation(&mut statement).inspect_err(|error| {
        cell.last_error.record(error);
    })
}

pub extern "C" fn postgres_bind_null(ctx: *mut SpectraHostCallContext) -> i32 { postgres_bind(ctx, PostgresValue::Null) }
pub extern "C" fn postgres_bind_int(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe { let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; }; if a.len()!=3 { return HOST_STATUS_INVALID_ARGUMENT; } match with_postgres_statement(a[0], |s| s.bind(a[1] as usize, PostgresValue::Int64(a[2])) ) { Ok(())=>bool_result(r,true), Err(e)=>fail_postgres(r,e) } }
}
pub extern "C" fn postgres_bind_float(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe { let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; }; if a.len()!=3 { return HOST_STATUS_INVALID_ARGUMENT; } match with_postgres_statement(a[0], |s| s.bind(a[1] as usize, PostgresValue::Float64(f64::from_bits(a[2] as u64))) ) { Ok(())=>bool_result(r,true), Err(e)=>fail_postgres(r,e) } }
}
pub extern "C" fn postgres_bind_text(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe { let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; }; if a.len()!=3 { return HOST_STATUS_INVALID_ARGUMENT; } let Some(text)=string(a[2]) else { return HOST_STATUS_INVALID_ARGUMENT; }; match with_postgres_statement(a[0], |s| s.bind(a[1] as usize, PostgresValue::Text(text)) ) { Ok(())=>bool_result(r,true), Err(e)=>fail_postgres(r,e) } }
}
fn postgres_bind(ctx: *mut SpectraHostCallContext, bound: PostgresValue) -> i32 {
    unsafe { let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; }; if a.len()!=2 { return HOST_STATUS_INVALID_ARGUMENT; }; match with_postgres_statement(a[0], |s| s.bind(a[1] as usize, bound) ) { Ok(())=>bool_result(r,true), Err(e)=>fail_postgres(r,e) } }
}
pub extern "C" fn postgres_step(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        match with_postgres_statement(a[0], |s| s.step()) {
            Ok(state) => value(r, state as i64),
            Err(error) => fail_postgres(r, error),
        }
    }
}
pub extern "C" fn postgres_column_count(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe { let Some((a,r))=args(ctx) else{return HOST_STATUS_INVALID_ARGUMENT}; if a.len()!=1{return HOST_STATUS_INVALID_ARGUMENT}; match with_postgres_statement(a[0], |s| Ok(s.column_count())) {Ok(count)=>value(r,count as i64),Err(e)=>fail_postgres(r,e)} }
}
pub extern "C" fn postgres_column_type(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe { let Some((a,r))=args(ctx) else{return HOST_STATUS_INVALID_ARGUMENT}; if a.len()!=2{return HOST_STATUS_INVALID_ARGUMENT}; match with_postgres_statement(a[0], |s| s.column_type(a[1] as usize).map(|t| match t {PostgresType::Null=>0,PostgresType::Bool=>1,PostgresType::Int16|PostgresType::Int32|PostgresType::Int64=>2,PostgresType::Float32|PostgresType::Float64=>3,PostgresType::Text=>4,PostgresType::Bytes=>5,PostgresType::Uuid|PostgresType::Timestamp=>6})) {Ok(kind)=>value(r,kind),Err(e)=>fail_postgres(r,e)} }
}
pub extern "C" fn postgres_column_int(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe { let Some((a,r))=args(ctx) else{return HOST_STATUS_INVALID_ARGUMENT}; if a.len()!=2{return HOST_STATUS_INVALID_ARGUMENT}; match with_postgres_statement(a[0], |s| s.column_value(a[1] as usize).map(|v| match v {PostgresValue::Int16(x)=>x as i64,PostgresValue::Int32(x)=>x as i64,PostgresValue::Int64(x)=>x,PostgresValue::Bool(x)=>x as i64,_=>0})) {Ok(value_)=>value(r,value_),Err(e)=>fail_postgres(r,e)} }
}
pub extern "C" fn postgres_column_text(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe { let Some((a,r))=args(ctx) else{return HOST_STATUS_INVALID_ARGUMENT}; if a.len()!=2{return HOST_STATUS_INVALID_ARGUMENT}; match with_postgres_statement(a[0], |s| s.column_value(a[1] as usize).map(|v| match v {PostgresValue::Text(x)=>x,PostgresValue::Uuid(x)=>x.to_string(),PostgresValue::Timestamp(x)=>x.to_rfc3339(),_=>String::new()})) {Ok(value_)=>value(r,alloc(&value_)),Err(e)=>fail_postgres(r,e)} }
}
pub extern "C" fn postgres_reset(ctx: *mut SpectraHostCallContext) -> i32 { unsafe { let Some((a,r))=args(ctx) else{return HOST_STATUS_INVALID_ARGUMENT}; if a.len()!=1{return HOST_STATUS_INVALID_ARGUMENT}; match with_postgres_statement(a[0], |s| {s.reset();Ok(())}) {Ok(())=>bool_result(r,true),Err(e)=>fail_postgres(r,e)} } }
pub extern "C" fn postgres_finalize(ctx: *mut SpectraHostCallContext) -> i32 { unsafe { let Some((a,r))=args(ctx) else{return HOST_STATUS_INVALID_ARGUMENT}; if a.len()!=1{return HOST_STATUS_INVALID_ARGUMENT}; let removed=store().lock().unwrap().postgres_statements.remove(&a[0]); bool_result(r,removed.is_some()) } }
pub extern "C" fn postgres_begin(ctx: *mut SpectraHostCallContext) -> i32 { postgres_transaction(ctx, "db.postgres.transaction", |c| c.execute_batch("BEGIN")) }
pub extern "C" fn postgres_commit(ctx: *mut SpectraHostCallContext) -> i32 { postgres_transaction(ctx, "db.postgres.commit", |c| c.execute_batch("COMMIT")) }
pub extern "C" fn postgres_rollback(ctx: *mut SpectraHostCallContext) -> i32 { postgres_transaction(ctx, "db.postgres.rollback", |c| c.execute_batch("ROLLBACK")) }
fn postgres_transaction(ctx: *mut SpectraHostCallContext, _span_name: &str, operation: impl FnOnce(&PostgresConnection)->Result<(), spectra_db::postgres::PostgresError>) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        match with_postgres_connection(a[0], operation) {
            Ok(()) => bool_result(r, true),
            Err(error) => fail_postgres(r, error),
        }
    }
}

fn spawn_postgres_task<F>(
    result: &mut [i64],
    last_error: Arc<LastError>,
    work: F,
    cancellation: Option<PostgresOperationCancellation>,
    long_io: bool,
) -> i32
where
    F: FnOnce() -> Result<i64, spectra_db::postgres::PostgresError> + Send + 'static,
{
    let hook_errors = Arc::clone(&last_error);
    let wrapped = move || match work() {
        Ok(value) => Ok(value),
        Err(error) => {
            last_error.record(&error);
            Err(())
        }
    };
    let task = if let Some(cancellation) = cancellation {
        if long_io {
            spectra_runtime::stdlib::spawn_cancellable_io_task(wrapped, move || {
                if let Err(error) = cancellation.request_cancel() {
                    hook_errors.record(&error);
                }
            })
        } else {
            spectra_runtime::stdlib::spawn_cancellable_background_task(wrapped, move || {
                if let Err(error) = cancellation.request_cancel() {
                    hook_errors.record(&error);
                }
            })
        }
    } else {
        spectra_runtime::stdlib::spawn_background_task(wrapped)
    };
    match task {
        Ok(task_id) => value(result, task_id),
        Err(_) => {
            let error = spectra_db::postgres::PostgresError::new(
                "DB2505_ASYNC_QUEUE_FULL",
                "PostgreSQL background queue is full",
            );
            fail_postgres(result, error)
        }
    }
}

pub extern "C" fn postgres_execute_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT };
        if a.len() != 2 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(sql) = string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT };
        let cell = match postgres_connection_cell(a[0]) {
            Ok(cell) => cell,
            Err(error) => return fail_postgres(r, error),
        };
        let cancellation = PostgresOperationCancellation::new();
        let operation_cancellation = cancellation.clone();
        spawn_postgres_task(
            r,
            Arc::clone(&cell.last_error),
            move || {
                let guard = cell
                    .value
                    .lock()
                    .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL connection lock poisoned"))?;
                guard
                    .execute_query_cancellable(
                        CompiledQuery { sql, params: vec![] },
                        &operation_cancellation,
                    )
                    .map(|result| result.affected_rows as i64)
            },
            Some(cancellation),
            false,
        )
    }
}

pub extern "C" fn postgres_step_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let statement_id = a[0];
        let resolved = store()
            .lock()
            .map(|state| state.postgres_statements.get(&statement_id).cloned())
            .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL handle store lock poisoned"));
        let cell = match resolved {
            Ok(Some(cell)) => cell,
            Ok(None) => return fail_postgres(r, spectra_db::postgres::PostgresError::invalid_handle()),
            Err(error) => return fail_postgres(r, error),
        };
        let cancellation = PostgresOperationCancellation::new();
        let operation_cancellation = cancellation.clone();
        spawn_postgres_task(
            r,
            Arc::clone(&cell.last_error),
            move || {
                let mut statement = cell
                    .value
                    .lock()
                    .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL statement lock poisoned"))?;
                statement
                    .step_cancellable(&operation_cancellation)
                    .map(i64::from)
            },
            Some(cancellation),
            false,
        )
    }
}

fn postgres_named_transaction(
    ctx: *mut SpectraHostCallContext,
    operation: impl FnOnce(&PostgresConnection, &str) -> Result<(), spectra_db::postgres::PostgresError>,
) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT };
        if a.len() != 2 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(name) = string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT };
        match with_postgres_connection(a[0], |connection| operation(connection, &name)) {
            Ok(()) => bool_result(r, true),
            Err(error) => fail_postgres(r, error),
        }
    }
}

pub extern "C" fn postgres_savepoint(ctx: *mut SpectraHostCallContext) -> i32 {
    postgres_named_transaction(ctx, PostgresConnection::savepoint)
}
pub extern "C" fn postgres_rollback_to(ctx: *mut SpectraHostCallContext) -> i32 {
    postgres_named_transaction(ctx, PostgresConnection::rollback_to)
}
pub extern "C" fn postgres_release_savepoint(ctx: *mut SpectraHostCallContext) -> i32 {
    postgres_named_transaction(ctx, PostgresConnection::release_savepoint)
}

pub extern "C" fn postgres_copy_in_text_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT };
        if a.len() != 3 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(sql) = string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT };
        let Some(text) = string(a[2]) else { return HOST_STATUS_INVALID_ARGUMENT };
        let cell = match postgres_connection_cell(a[0]) {
            Ok(cell) => cell,
            Err(error) => return fail_postgres(r, error),
        };
        let cancellation = PostgresOperationCancellation::new();
        let operation_cancellation = cancellation.clone();
        spawn_postgres_task(
            r,
            Arc::clone(&cell.last_error),
            move || {
                let guard = cell
                    .value
                    .lock()
                    .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL connection lock poisoned"))?;
                guard
                    .copy_in_text_cancellable(&sql, &text, &operation_cancellation)
                    .map(|rows| rows as i64)
            },
            Some(cancellation),
            false,
        )
    }
}

pub extern "C" fn postgres_copy_out_text_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT };
        if a.len() != 2 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(sql) = string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT };
        let cell = match postgres_connection_cell(a[0]) {
            Ok(cell) => cell,
            Err(error) => return fail_postgres(r, error),
        };
        let cancellation = PostgresOperationCancellation::new();
        let operation_cancellation = cancellation.clone();
        spawn_postgres_task(
            r,
            Arc::clone(&cell.last_error),
            move || {
                let guard = cell
                    .value
                    .lock()
                    .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL connection lock poisoned"))?;
                let bytes =
                    guard.copy_out_bytes_cancellable_limited(&sql, &operation_cancellation, POSTGRES_COPY_OUT_TEXT_LIMIT)?;
                let text = String::from_utf8(bytes).map_err(|_| {
                    spectra_db::postgres::PostgresError::new(
                        "DB2505_COPY_OUT",
                        "COPY OUT returned non-UTF-8 data",
                    )
                })?;
                Ok(alloc(&text))
            },
            Some(cancellation),
            false,
        )
    }
}

pub extern "C" fn postgres_listen(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT };
        if a.len() != 2 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(channel) = string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT };
        let cell = match postgres_connection_cell(a[0]) {
            Ok(cell) => cell,
            Err(error) => return fail_postgres(r, error),
        };
        let listened = cell
            .value
            .lock()
            .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL connection lock poisoned"))
            .and_then(|connection| connection.listen(&channel));
        match listened {
            Ok(listener) => {
                let mut state = store().lock().unwrap();
                let id = state.postgres_channels.insert(DriverHandle::boxed(listener));
                value(r, id)
            }
            Err(error) => {
                cell.last_error.record(&error);
                fail_postgres(r, error)
            }
        }
    }
}

pub extern "C" fn postgres_notify_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT };
        if a.len() != 3 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(channel) = string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT };
        let Some(payload) = string(a[2]) else { return HOST_STATUS_INVALID_ARGUMENT };
        let cell = match postgres_connection_cell(a[0]) {
            Ok(cell) => cell,
            Err(error) => return fail_postgres(r, error),
        };
        let cancellation = PostgresOperationCancellation::new();
        let operation_cancellation = cancellation.clone();
        spawn_postgres_task(
            r,
            Arc::clone(&cell.last_error),
            move || {
                let guard = cell
                    .value
                    .lock()
                    .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL connection lock poisoned"))?;
                guard
                    .notify_cancellable(&channel, &payload, &operation_cancellation)
                    .map(|_| 1)
            },
            Some(cancellation),
            false,
        )
    }
}

pub extern "C" fn postgres_notification_next_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT };
        if a.len() != 2 || a[1] < 0 { return HOST_STATUS_INVALID_ARGUMENT; }
        let resolved = store()
            .lock()
            .map(|state| state.postgres_channels.get(&a[0]).cloned())
            .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL handle store lock poisoned"));
        let resolved = match resolved {
            Ok(resolved) => resolved,
            Err(error) => return fail_postgres(r, error),
        };
        let cell = match resolved {
            Some(cell) => cell,
            None => return fail_postgres(r, spectra_db::postgres::PostgresError::invalid_handle()),
        };
        let timeout = Duration::from_millis(a[1] as u64);
        let cancellation = PostgresOperationCancellation::new();
        let operation_cancellation = cancellation.clone();
        spawn_postgres_task(
            r,
            Arc::clone(&cell.last_error),
            move || {
                let listener = cell
                    .value
                    .lock()
                    .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL notification channel lock poisoned"))?;
                let notification = listener.next_timeout_cancellable(timeout, &operation_cancellation)?;
                let Some(notification) = notification else { return Ok(0) };
                let mut state = store()
                    .lock()
                    .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL handle store lock poisoned"))?;
                let id = state.postgres_notifications.insert(notification);
                Ok(id)
            },
            Some(cancellation),
            true,
        )
    }
}

fn with_notification<T>(
    id: i64,
    operation: impl FnOnce(&Notification) -> T,
) -> Result<T, spectra_db::postgres::PostgresError> {
    let state = store()
        .lock()
        .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL handle store lock poisoned"))?;
    state
        .postgres_notifications
        .get(&id)
        .map(operation)
        .ok_or_else(spectra_db::postgres::PostgresError::invalid_handle)
}

pub extern "C" fn postgres_notification_channel(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe { let Some((a,r))=args(ctx) else{return HOST_STATUS_INVALID_ARGUMENT}; if a.len()!=1{return HOST_STATUS_INVALID_ARGUMENT}; match with_notification(a[0], |n| n.channel.clone()) {Ok(text)=>value(r,alloc(&text)),Err(error)=>fail_postgres(r,error)} }
}
pub extern "C" fn postgres_notification_payload(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe { let Some((a,r))=args(ctx) else{return HOST_STATUS_INVALID_ARGUMENT}; if a.len()!=1{return HOST_STATUS_INVALID_ARGUMENT}; match with_notification(a[0], |n| n.payload.clone()) {Ok(text)=>value(r,alloc(&text)),Err(error)=>fail_postgres(r,error)} }
}
pub extern "C" fn postgres_notification_process_id(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe { let Some((a,r))=args(ctx) else{return HOST_STATUS_INVALID_ARGUMENT}; if a.len()!=1{return HOST_STATUS_INVALID_ARGUMENT}; match with_notification(a[0], |n| n.process_id as i64) {Ok(pid)=>value(r,pid),Err(error)=>fail_postgres(r,error)} }
}
pub extern "C" fn postgres_notification_close(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let removed = store().lock().unwrap().postgres_channels.remove(&a[0]);
        bool_result(r, removed.is_some())
    }
}
pub extern "C" fn postgres_notification_free(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let removed = store().lock().unwrap().postgres_notifications.remove(&a[0]);
        bool_result(r, removed.is_some())
    }
}

pub extern "C" fn postgres_last_error_code(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let code = postgres_last_error_slot(a[0])
            .and_then(|slot| slot.snapshot())
            .map(|error| error.0)
            .unwrap_or_default();
        value(r, alloc(&code))
    }
}
pub extern "C" fn postgres_last_error_message(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let message = postgres_last_error_slot(a[0])
            .and_then(|slot| slot.snapshot())
            .map(|error| error.1)
            .unwrap_or_default();
        value(r, alloc(&message))
    }
}

/// Resolve the per-handle error slot for a PostgreSQL connection or statement
/// handle (both share the connection's slot).
fn postgres_last_error_slot(id: i64) -> Option<Arc<LastError>> {
    let state = store().lock().ok()?;
    state
        .postgres_connections
        .get(&id)
        .map(|cell| Arc::clone(&cell.last_error))
        .or_else(|| {
            state
                .postgres_statements
                .get(&id)
                .map(|cell| Arc::clone(&cell.last_error))
        })
}

pub const POSTGRES_HOST_CALLS: &[(&str, HostFunction)] = &[
    ("spectra.api.db.postgres.open", postgres_open), ("spectra.api.db.postgres.close", postgres_close),
    ("spectra.api.db.postgres.prepare", postgres_prepare), ("spectra.api.db.postgres.bind_null", postgres_bind_null),
    ("spectra.api.db.postgres.bind_int", postgres_bind_int), ("spectra.api.db.postgres.bind_float", postgres_bind_float),
    ("spectra.api.db.postgres.bind_text", postgres_bind_text), ("spectra.api.db.postgres.step", postgres_step),
    ("spectra.api.db.postgres.column_count", postgres_column_count), ("spectra.api.db.postgres.column_type", postgres_column_type),
    ("spectra.api.db.postgres.column_int", postgres_column_int), ("spectra.api.db.postgres.column_text", postgres_column_text),
    ("spectra.api.db.postgres.reset", postgres_reset), ("spectra.api.db.postgres.finalize", postgres_finalize),
    ("spectra.api.db.postgres.begin", postgres_begin), ("spectra.api.db.postgres.commit", postgres_commit),
    ("spectra.api.db.postgres.rollback", postgres_rollback),
    ("spectra.api.db.postgres.execute_async", postgres_execute_async),
    ("spectra.api.db.postgres.step_async", postgres_step_async),
    ("spectra.api.db.postgres.savepoint", postgres_savepoint),
    ("spectra.api.db.postgres.rollback_to", postgres_rollback_to),
    ("spectra.api.db.postgres.release_savepoint", postgres_release_savepoint),
    ("spectra.api.db.postgres.copy_in_text_async", postgres_copy_in_text_async),
    ("spectra.api.db.postgres.copy_out_text_async", postgres_copy_out_text_async),
    ("spectra.api.db.postgres.listen", postgres_listen),
    ("spectra.api.db.postgres.notify_async", postgres_notify_async),
    ("spectra.api.db.postgres.notification_next_async", postgres_notification_next_async),
    ("spectra.api.db.postgres.notification_channel", postgres_notification_channel),
    ("spectra.api.db.postgres.notification_payload", postgres_notification_payload),
    ("spectra.api.db.postgres.notification_process_id", postgres_notification_process_id),
    ("spectra.api.db.postgres.notification_close", postgres_notification_close),
    ("spectra.api.db.postgres.notification_free", postgres_notification_free),
    ("spectra.api.db.postgres.last_error_code", postgres_last_error_code),
    ("spectra.api.db.postgres.last_error_message", postgres_last_error_message),
];
pub extern "C" fn redis_open(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(url) = string(a[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        let config = match RedisConfig::from_url(&url) { Ok(config) => config, Err(error) => return fail_redis(r, error) };
        let span = redis_operation_span("db.redis.connect");
        match RedisConnection::open(config) {
            Ok(connection) => { let mut state = store().lock().unwrap(); let id = state.redis_connections.insert(DriverHandle::boxed(connection)); finish_redis_span(span, true); value(r, id) }
            Err(error) => { finish_redis_span(span, false); fail_redis(r, error) }
        }
    }
}
pub extern "C" fn redis_close(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let (cell, leased) = {
            let mut state = store().lock().unwrap();
            (
                state.redis_connections.remove(&a[0]),
                state.redis_pool_leases.contains_key(&a[0]),
            )
        };
        let span = redis_operation_span("db.redis.close");
        let Some(cell) = cell else { finish_redis_span(span, false); return fail_redis(r, RedisError::invalid_handle()) };
        // A pooled lease goes back to its pool instead of being closed; the
        // underlying physical connection stays alive inside the pool.
        if leased {
            drop(cell);
            if release_redis_lease(a[0]) {
                finish_redis_span(span, true);
                bool_result(r, true)
            } else {
                finish_redis_span(span, false);
                fail_redis(r, RedisError::new("DB2507_POOL", "pool lease was already released"))
            }
        } else {
        let result = cell
            .value
            .lock()
            .map_err(|_| RedisError::new("DB2507_LOCK", "Redis handle lock poisoned"))
            .and_then(|connection| connection.close());
        if let Err(error) = &result {
            cell.last_error.record(error);
        }
        finish_redis_span(span, result.is_ok());
        match result { Ok(()) => bool_result(r, true), Err(error) => fail_redis(r, error) }
        }
    }
}
fn redis_connection_cell(
    id: i64,
) -> Result<Arc<DriverHandle<RedisConnection>>, RedisError> {
    store()
        .lock()
        .map_err(|_| RedisError::new("DB2507_LOCK", "Redis handle store lock poisoned"))?
        .redis_connections
        .get(&id)
        .cloned()
        .ok_or_else(RedisError::invalid_handle)
}
/// Run a blocking Redis command against the stored connection while holding
/// only that handle's own mutex, recording failures into its error slot.
fn with_redis_connection<T>(
    id: i64,
    operation: impl FnOnce(&RedisConnection) -> Result<T, RedisError>,
) -> Result<T, RedisError> {
    let cell = redis_connection_cell(id)?;
    let guard = cell
        .value
        .lock()
        .map_err(|_| RedisError::new("DB2507_LOCK", "Redis handle lock poisoned"))?;
    operation(&guard).inspect_err(|error| {
        cell.last_error.record(error);
    })
}
pub extern "C" fn redis_get(ctx: *mut SpectraHostCallContext) -> i32 { redis_key_op(ctx, "db.redis.get", |c, key| c.get_blocking(key).map(|value| value.and_then(|v| v.into_bytes().ok()))) }
pub extern "C" fn redis_set(ctx: *mut SpectraHostCallContext) -> i32 { unsafe { let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; }; if a.len()!=3 { return HOST_STATUS_INVALID_ARGUMENT; } let Some(key)=string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT; }; let Some(value)=string(a[2]) else { return HOST_STATUS_INVALID_ARGUMENT; }; let span=redis_operation_span("db.redis.set"); let result=with_redis_connection(a[0], |c| c.set_blocking(&key,RedisValue::Text(value),None)); finish_redis_span(span,result.is_ok()); match result {Ok(())=>bool_result(r,true),Err(e)=>fail_redis(r,e)} } }
pub extern "C" fn redis_delete(ctx: *mut SpectraHostCallContext) -> i32 { redis_key_op(ctx, "db.redis.delete", |c, key| c.delete_blocking(key)) }
pub extern "C" fn redis_expire(ctx: *mut SpectraHostCallContext) -> i32 { unsafe { let Some((a,r))=args(ctx) else{return HOST_STATUS_INVALID_ARGUMENT}; if a.len()!=3{return HOST_STATUS_INVALID_ARGUMENT}; let Some(key)=string(a[1]) else{return HOST_STATUS_INVALID_ARGUMENT}; let span=redis_operation_span("db.redis.expire"); let result=with_redis_connection(a[0], |c| c.expire_blocking(&key,Duration::from_secs(a[2].max(0) as u64))); finish_redis_span(span,result.is_ok()); match result{Ok(v)=>bool_result(r,v),Err(e)=>fail_redis(r,e)} } }
pub extern "C" fn redis_incr(ctx: *mut SpectraHostCallContext) -> i32 { unsafe { let Some((a,r))=args(ctx) else{return HOST_STATUS_INVALID_ARGUMENT}; if a.len()!=3{return HOST_STATUS_INVALID_ARGUMENT}; let Some(key)=string(a[1]) else{return HOST_STATUS_INVALID_ARGUMENT}; let span=redis_operation_span("db.redis.incr"); let result=with_redis_connection(a[0], |c| c.incr_blocking(&key,a[2])); finish_redis_span(span,result.is_ok()); match result{Ok(v)=>value(r,v),Err(e)=>fail_redis(r,e)} } }
pub extern "C" fn redis_exists(ctx: *mut SpectraHostCallContext) -> i32 { redis_key_op(ctx, "db.redis.exists", |c, key| c.exists_blocking(key)) }
fn redis_key_op<T: IntoRedisResult>(ctx: *mut SpectraHostCallContext, span_name: &str, operation: impl FnOnce(&RedisConnection, &str) -> Result<T, RedisError>) -> i32 {
    unsafe {
        let Some((a,r))=args(ctx) else{return HOST_STATUS_INVALID_ARGUMENT};
        if a.len()!=2{return HOST_STATUS_INVALID_ARGUMENT};
        let Some(key)=string(a[1]) else{return HOST_STATUS_INVALID_ARGUMENT};
        let cell=match redis_connection_cell(a[0]){Ok(c)=>c,Err(e)=>return fail_redis(r,e)};
        let span=redis_operation_span(span_name);
        let result = cell
            .value
            .lock()
            .map_err(|_| RedisError::new("DB2507_LOCK", "Redis handle lock poisoned"))
            .and_then(|guard| {
                annotate_redis_span(span, &guard);
                operation(&guard, &key)
            });
        if let Err(error) = &result {
            cell.last_error.record(error);
        }
        finish_redis_span(span,result.is_ok());
        match result{Ok(v)=>v.into_result(r),Err(e)=>fail_redis(r,e)}
    }
}
trait IntoRedisResult { fn into_result(self, result: &mut [i64]) -> i32; }
impl IntoRedisResult for bool { fn into_result(self,r:&mut[i64])->i32{bool_result(r,self)} }
impl IntoRedisResult for Option<Vec<u8>> { fn into_result(self,r:&mut[i64])->i32{ match self {Some(v)=>value(r,alloc(&String::from_utf8_lossy(&v))),None=>value(r,0)} } }
fn fail_redis(result: &mut [i64], _error: RedisError) -> i32 { value(result, 0) }

/// Spawn a Redis operation on the dedicated I/O executor. The returned task
/// is cancellable: the token is checked before the command is dispatched, so
/// a pending-but-not-yet-sent command aborts immediately, and an in-flight
/// command is bounded by the connection's configured command timeout.
fn spawn_redis_task<F>(
    result: &mut [i64],
    cell: &Arc<DriverHandle<RedisConnection>>,
    work: F,
) -> i32
where
    F: FnOnce(&RedisConnection) -> Result<i64, RedisError> + Send + 'static,
{
    let owned_cell = Arc::clone(cell);
    let errors = Arc::clone(&cell.last_error);
    let task = spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |token| {
        use std::sync::atomic::Ordering;
        let cancelled = || {
            errors.record_raw("DB2507_CANCELLED", "Redis operation was cancelled before dispatch");
        };
        if token.load(Ordering::Acquire) {
            cancelled();
            return Err(());
        }
        let guard = owned_cell
            .value
            .lock()
            .map_err(|_| RedisError::new("DB2507_LOCK", "Redis handle lock poisoned"));
        let guard = match guard {
            Ok(guard) => guard,
            Err(error) => {
                errors.record(&error);
                return Err(());
            }
        };
        if token.load(Ordering::Acquire) {
            cancelled();
            return Err(());
        }
        work(&guard).map_err(|error| {
            errors.record(&error);
        })
    });
    finish_spawned_redis_task(result, cell, task)
}

fn finish_spawned_redis_task(
    result: &mut [i64],
    cell: &Arc<DriverHandle<RedisConnection>>,
    task: Result<SpectraHostValue, i32>,
) -> i32 {
    match task {
        Ok(task_id) => value(result, task_id),
        Err(_) => {
            cell.last_error.record_raw(
                "DB2507_ASYNC_QUEUE_FULL",
                "Redis background queue is full",
            );
            fail_redis(result, RedisError::new("DB2507_ASYNC_QUEUE_FULL", "Redis background queue is full"))
        }
    }
}

pub extern "C" fn redis_open_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(url) = string(a[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        let config = match RedisConfig::from_url(&url) { Ok(config) => config, Err(error) => return fail_redis(r, error) };
        let span = redis_operation_span("db.redis.connect_async");
        let task = spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |token| {
            use std::sync::atomic::Ordering;
            if token.load(Ordering::Acquire) {
                return Err(());
            }
            let connection = RedisConnection::open(config).map_err(|_: RedisError| ())?;
            let mut state = store().lock().map_err(|_| ())?;
            Ok(state.redis_connections.insert(DriverHandle::boxed(connection)))
        });
        finish_redis_span(span, task.is_ok());
        match task {
            Ok(task_id) => value(r, task_id),
            Err(_) => fail_redis(r, RedisError::new("DB2507_ASYNC_QUEUE_FULL", "Redis background queue is full")),
        }
    }
}

/// Clone the stored Redis connection out of its handle cell for callers that
/// operate on the raw driver value (e.g. the session store).
pub(crate) fn redis_connection(id: i64) -> Result<RedisConnection, RedisError> {
    let cell = redis_connection_cell(id)?;
    cell.value
        .lock()
        .map(|guard| guard.clone())
        .map_err(|_| RedisError::new("DB2507_LOCK", "Redis handle lock poisoned"))
}

pub extern "C" fn redis_close_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let cell = match store().lock().unwrap().redis_connections.remove(&a[0]) {
            Some(cell) => cell,
            None => return fail_redis(r, RedisError::invalid_handle()),
        };
        let span = redis_operation_span("db.redis.close_async");
        let errors = Arc::clone(&cell.last_error);
        let task = spectra_runtime::stdlib::spawn_cancellable_io_task_with_token(move |token| {
            use std::sync::atomic::Ordering;
            if token.load(Ordering::Acquire) {
                errors.record_raw("DB2507_CANCELLED", "Redis operation was cancelled before dispatch");
                return Err(());
            }
            let guard = match cell.value.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    errors.record_raw("DB2507_LOCK", "Redis handle lock poisoned");
                    return Err(());
                }
            };
            guard.close().map(|_| 1).map_err(|error| {
                errors.record(&error);
            })
        });
        finish_redis_span(span, task.is_ok());
        match task {
            Ok(task_id) => value(r, task_id),
            Err(_) => fail_redis(r, RedisError::new("DB2507_ASYNC_QUEUE_FULL", "Redis background queue is full")),
        }
    }
}

pub extern "C" fn redis_get_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 2 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(key) = string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        let cell = match redis_connection_cell(a[0]) { Ok(cell) => cell, Err(error) => return fail_redis(r, error) };
        spawn_redis_task(r, &cell, move |connection| {
            connection
                .get_blocking(&key)
                .map(|value| {
                    let text = value
                        .and_then(|decoded| decoded.into_bytes().ok())
                        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                        .unwrap_or_default();
                    alloc(&text)
                })
        })
    }
}

pub extern "C" fn redis_set_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 3 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(key) = string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        let Some(payload) = string(a[2]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        let cell = match redis_connection_cell(a[0]) { Ok(cell) => cell, Err(error) => return fail_redis(r, error) };
        spawn_redis_task(r, &cell, move |connection| {
            connection.set_blocking(&key, RedisValue::Text(payload), None).map(|_| 1)
        })
    }
}

pub extern "C" fn redis_delete_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 2 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(key) = string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        let cell = match redis_connection_cell(a[0]) { Ok(cell) => cell, Err(error) => return fail_redis(r, error) };
        spawn_redis_task(r, &cell, move |connection| {
            connection.delete_blocking(&key).map(|removed| removed as i64)
        })
    }
}

pub extern "C" fn redis_exists_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 2 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(key) = string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        let cell = match redis_connection_cell(a[0]) { Ok(cell) => cell, Err(error) => return fail_redis(r, error) };
        spawn_redis_task(r, &cell, move |connection| {
            connection.exists_blocking(&key).map(|present| present as i64)
        })
    }
}

pub extern "C" fn redis_incr_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 3 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(key) = string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        let amount = a[2];
        let cell = match redis_connection_cell(a[0]) { Ok(cell) => cell, Err(error) => return fail_redis(r, error) };
        spawn_redis_task(r, &cell, move |connection| connection.incr_blocking(&key, amount))
    }
}

pub extern "C" fn redis_expire_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 3 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(key) = string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        let ttl = Duration::from_secs(a[2].max(0) as u64);
        let cell = match redis_connection_cell(a[0]) { Ok(cell) => cell, Err(error) => return fail_redis(r, error) };
        spawn_redis_task(r, &cell, move |connection| {
            connection.expire_blocking(&key, ttl).map(|expired| expired as i64)
        })
    }
}

fn redis_last_error_slot(id: i64) -> Option<Arc<LastError>> {
    store()
        .lock()
        .ok()?
        .redis_connections
        .get(&id)
        .map(|cell| Arc::clone(&cell.last_error))
}

pub extern "C" fn redis_last_error_code(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let code = redis_last_error_slot(a[0])
            .and_then(|slot| slot.snapshot())
            .map(|error| error.0)
            .unwrap_or_default();
        value(r, alloc(&code))
    }
}
pub extern "C" fn redis_last_error_message(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let message = redis_last_error_slot(a[0])
            .and_then(|slot| slot.snapshot())
            .map(|error| error.1)
            .unwrap_or_default();
        value(r, alloc(&message))
    }
}

pub const REDIS_HOST_CALLS: &[(&str, HostFunction)] = &[
    ("spectra.api.db.redis.open", redis_open),
    ("spectra.api.db.redis.open_async", redis_open_async),
    ("spectra.api.db.redis.close", redis_close),
    ("spectra.api.db.redis.close_async", redis_close_async),
    ("spectra.api.db.redis.get", redis_get),
    ("spectra.api.db.redis.get_async", redis_get_async),
    ("spectra.api.db.redis.set", redis_set),
    ("spectra.api.db.redis.set_async", redis_set_async),
    ("spectra.api.db.redis.delete", redis_delete),
    ("spectra.api.db.redis.delete_async", redis_delete_async),
    ("spectra.api.db.redis.exists", redis_exists),
    ("spectra.api.db.redis.exists_async", redis_exists_async),
    ("spectra.api.db.redis.incr", redis_incr),
    ("spectra.api.db.redis.incr_async", redis_incr_async),
    ("spectra.api.db.redis.expire", redis_expire),
    ("spectra.api.db.redis.expire_async", redis_expire_async),
    ("spectra.api.db.redis.last_error_code", redis_last_error_code),
    ("spectra.api.db.redis.last_error_message", redis_last_error_message),
];
