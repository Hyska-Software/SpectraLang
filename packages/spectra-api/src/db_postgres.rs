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
                let id = state.connections.insert(connection);
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
        let connection = store().lock().unwrap().connections.remove(&a[0]);
        let Some(connection) = connection else {
            return fail(r, spectra_db::sqlite::SqliteError::invalid_handle());
        };
        let span = operation_span("db.sqlite.close");
        let result = connection.close();
        finish_span(span, result.is_ok());
        match result {
            Ok(()) => bool_result(r, true),
            Err(error) => fail(r, error),
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
        let connection = store()
            .lock()
            .unwrap()
            .connections
            .get(&a[0])
            .cloned();
        let Some(connection) = connection else {
            return fail(r, spectra_db::sqlite::SqliteError::invalid_handle());
        };
        let span = operation_span("db.sqlite.prepare");
        match SqliteStatement::prepare(connection, sql) {
            Ok(statement) => {
                let mut state = store().lock().unwrap();
                let id = state.statements.insert(statement);
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
        let connection = store()
            .lock()
            .unwrap()
            .connections
            .get(&a[0])
            .cloned();
        let Some(connection) = connection else {
            return fail(r, spectra_db::sqlite::SqliteError::invalid_handle());
        };
        let task = spectra_runtime::stdlib::spawn_background_task(move || {
            let mut statement = SqliteStatement::prepare(connection, sql).map_err(|_| ())?;
            statement.step().map_err(|_| ())?;
            statement
                .affected_rows()
                .map_err(|_| ())
                .map(|rows| rows as i64)
        });
        match task {
            Ok(task_id) => value(r, task_id),
            Err(_) => value(r, 0),
        }
    }
}
fn with_statement<T>(
    id: i64,
    operation: impl FnOnce(&mut SqliteStatement) -> Result<T, spectra_db::sqlite::SqliteError>,
) -> Result<T, spectra_db::sqlite::SqliteError> {
    let mut state = store().lock().map_err(|_| {
        spectra_db::sqlite::SqliteError::new("DB2504_LOCK", "SQLite handle store lock poisoned")
    })?;
    let statement = state
        .statements
        .get_mut(&id)
        .ok_or_else(spectra_db::sqlite::SqliteError::invalid_handle)?;
    operation(statement)
}

fn with_postgres_statement<T>(
    id: i64,
    operation: impl FnOnce(&mut PostgresStatement) -> Result<T, spectra_db::postgres::PostgresError>,
) -> Result<T, spectra_db::postgres::PostgresError> {
    let statement = store()
        .lock()
        .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL handle store lock poisoned"))?
        .postgres_statements
        .get(&id)
        .cloned()
        .ok_or_else(spectra_db::postgres::PostgresError::invalid_handle)?;
    let mut statement = statement
        .lock()
        .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL statement lock poisoned"))?;
    operation(&mut statement)
}

pub extern "C" fn postgres_open(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(url) = string(a[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        let config = match PostgresConfig::from_url(&url) { Ok(config) => config, Err(error) => return fail_postgres(r, error) };
        match PostgresConnection::open(config) {
            Ok(connection) => { let mut state = store().lock().unwrap(); let id = state.postgres_connections.insert(connection); value(r, id) }
            Err(error) => fail_postgres(r, error),
        }
    }
}

pub extern "C" fn postgres_close(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let connection = store().lock().unwrap().postgres_connections.remove(&a[0]);
        match connection { Some(connection) => match connection.close() { Ok(()) => bool_result(r, true), Err(error) => fail_postgres(r, error) }, None => fail_postgres(r, spectra_db::postgres::PostgresError::invalid_handle()) }
    }
}

pub extern "C" fn postgres_prepare(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 2 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(sql) = string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        let connection = store().lock().unwrap().postgres_connections.get(&a[0]).cloned();
        let Some(connection) = connection else { return fail_postgres(r, spectra_db::postgres::PostgresError::invalid_handle()); };
        match connection.prepare(sql) {
            Ok(statement) => { let mut state = store().lock().unwrap(); let id = state.postgres_statements.insert(Arc::new(Mutex::new(statement))); value(r, id) }
            Err(error) => fail_postgres(r, error),
        }
    }
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
        let c = store().lock().unwrap().postgres_connections.get(&a[0]).cloned();
        match c {
            Some(c) => match operation(&c) {
                Ok(()) => bool_result(r, true),
                Err(error) => fail_postgres(r, error),
            },
            None => fail_postgres(r, spectra_db::postgres::PostgresError::invalid_handle()),
        }
    }
}

fn postgres_connection(id: i64) -> Result<PostgresConnection, spectra_db::postgres::PostgresError> {
    store()
        .lock()
        .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL handle store lock poisoned"))?
        .postgres_connections
        .get(&id)
        .cloned()
        .ok_or_else(spectra_db::postgres::PostgresError::invalid_handle)
}

fn spawn_postgres_task<F>(
    result: &mut [i64],
    work: F,
    cancellation: Option<PostgresOperationCancellation>,
    long_io: bool,
) -> i32
where
    F: FnOnce() -> Result<i64, spectra_db::postgres::PostgresError> + Send + 'static,
{
    let wrapped = move || match work() {
        Ok(value) => Ok(value),
        Err(error) => {
            record_postgres_error(&error);
            Err(())
        }
    };
    let task = if let Some(cancellation) = cancellation {
        if long_io {
            spectra_runtime::stdlib::spawn_cancellable_io_task(wrapped, move || {
                if let Err(error) = cancellation.request_cancel() {
                    record_postgres_error(&error);
                }
            })
        } else {
            spectra_runtime::stdlib::spawn_cancellable_background_task(wrapped, move || {
                if let Err(error) = cancellation.request_cancel() {
                    record_postgres_error(&error);
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
        let connection = match postgres_connection(a[0]) {
            Ok(connection) => connection,
            Err(error) => return fail_postgres(r, error),
        };
        let cancellation = PostgresOperationCancellation::new();
        let operation_cancellation = cancellation.clone();
        spawn_postgres_task(
            r,
            move || {
                connection
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
        let cancellation = PostgresOperationCancellation::new();
        let operation_cancellation = cancellation.clone();
        spawn_postgres_task(
            r,
            move || {
                with_postgres_statement(statement_id, |statement| {
                    statement
                        .step_cancellable(&operation_cancellation)
                        .map(i64::from)
                })
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
        let connection = match postgres_connection(a[0]) {
            Ok(connection) => connection,
            Err(error) => return fail_postgres(r, error),
        };
        match operation(&connection, &name) {
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
        let connection = match postgres_connection(a[0]) {
            Ok(connection) => connection,
            Err(error) => return fail_postgres(r, error),
        };
        let cancellation = PostgresOperationCancellation::new();
        let operation_cancellation = cancellation.clone();
        spawn_postgres_task(
            r,
            move || {
                connection
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
        let connection = match postgres_connection(a[0]) {
            Ok(connection) => connection,
            Err(error) => return fail_postgres(r, error),
        };
        let cancellation = PostgresOperationCancellation::new();
        let operation_cancellation = cancellation.clone();
        spawn_postgres_task(
            r,
            move || {
                let bytes = connection.copy_out_bytes_cancellable_limited(
                    &sql,
                    &operation_cancellation,
                    POSTGRES_COPY_OUT_TEXT_LIMIT,
                )?;
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
        let connection = match postgres_connection(a[0]) {
            Ok(connection) => connection,
            Err(error) => return fail_postgres(r, error),
        };
        match connection.listen(&channel) {
            Ok(listener) => {
                let mut state = store().lock().unwrap();
                let id = state
                    .postgres_channels
                    .insert(Arc::new(Mutex::new(listener)));
                value(r, id)
            }
            Err(error) => fail_postgres(r, error),
        }
    }
}

pub extern "C" fn postgres_notify_async(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT };
        if a.len() != 3 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(channel) = string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT };
        let Some(payload) = string(a[2]) else { return HOST_STATUS_INVALID_ARGUMENT };
        let connection = match postgres_connection(a[0]) {
            Ok(connection) => connection,
            Err(error) => return fail_postgres(r, error),
        };
        let cancellation = PostgresOperationCancellation::new();
        let operation_cancellation = cancellation.clone();
        spawn_postgres_task(
            r,
            move || {
                connection
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
        let listener = match store().lock().unwrap().postgres_channels.get(&a[0]).cloned() {
            Some(listener) => listener,
            None => return fail_postgres(r, spectra_db::postgres::PostgresError::invalid_handle()),
        };
        let timeout = Duration::from_millis(a[1] as u64);
        let cancellation = PostgresOperationCancellation::new();
        let operation_cancellation = cancellation.clone();
        spawn_postgres_task(
            r,
            move || {
                let notification = listener
                    .lock()
                    .map_err(|_| spectra_db::postgres::PostgresError::new("DB2505_LOCK", "PostgreSQL notification channel lock poisoned"))?
                    .next_timeout_cancellable(timeout, &operation_cancellation)?;
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
    ("spectra.api.db.postgres.last_error_code", sqlite_last_error_code),
    ("spectra.api.db.postgres.last_error_message", sqlite_last_error_message),
];

pub extern "C" fn redis_open(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let Some(url) = string(a[0]) else { return HOST_STATUS_INVALID_ARGUMENT; };
        let config = match RedisConfig::from_url(&url) { Ok(config) => config, Err(error) => return fail_redis(r, error) };
        let span = redis_operation_span("db.redis.connect");
        match RedisConnection::open(config) {
            Ok(connection) => { let mut state = store().lock().unwrap(); let id = state.redis_connections.insert(connection); finish_redis_span(span, true); value(r, id) }
            Err(error) => { finish_redis_span(span, false); fail_redis(r, error) }
        }
    }
}
pub extern "C" fn redis_close(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; };
        if a.len() != 1 { return HOST_STATUS_INVALID_ARGUMENT; }
        let connection = store().lock().unwrap().redis_connections.remove(&a[0]);
        let span = redis_operation_span("db.redis.close");
        match connection { Some(connection) => { let result = connection.close(); finish_redis_span(span, result.is_ok()); match result { Ok(()) => bool_result(r, true), Err(error) => fail_redis(r, error) } }, None => { finish_redis_span(span, false); fail_redis(r, RedisError::invalid_handle()) } }
    }
}
pub(crate) fn redis_connection(id: i64) -> Result<RedisConnection, RedisError> { store().lock().map_err(|_| RedisError::new("DB2507_LOCK", "Redis handle store lock poisoned"))?.redis_connections.get(&id).cloned().ok_or_else(RedisError::invalid_handle) }
pub extern "C" fn redis_get(ctx: *mut SpectraHostCallContext) -> i32 { redis_key_op(ctx, "db.redis.get", |c, key| c.get_blocking(key).map(|value| value.and_then(|v| v.into_bytes().ok()))) }
pub extern "C" fn redis_set(ctx: *mut SpectraHostCallContext) -> i32 { unsafe { let Some((a, r)) = args(ctx) else { return HOST_STATUS_INVALID_ARGUMENT; }; if a.len()!=3 { return HOST_STATUS_INVALID_ARGUMENT; } let Some(key)=string(a[1]) else { return HOST_STATUS_INVALID_ARGUMENT; }; let Some(value)=string(a[2]) else { return HOST_STATUS_INVALID_ARGUMENT; }; let connection=match redis_connection(a[0]){Ok(c)=>c,Err(e)=>return fail_redis(r,e)}; let span=redis_operation_span("db.redis.set"); let result=connection.set_blocking(&key,RedisValue::Text(value),None); finish_redis_span(span,result.is_ok()); match result {Ok(())=>bool_result(r,true),Err(e)=>fail_redis(r,e)} } }
pub extern "C" fn redis_delete(ctx: *mut SpectraHostCallContext) -> i32 { redis_key_op(ctx, "db.redis.delete", |c, key| c.delete_blocking(key)) }
pub extern "C" fn redis_expire(ctx: *mut SpectraHostCallContext) -> i32 { unsafe { let Some((a,r))=args(ctx) else{return HOST_STATUS_INVALID_ARGUMENT}; if a.len()!=3{return HOST_STATUS_INVALID_ARGUMENT}; let Some(key)=string(a[1]) else{return HOST_STATUS_INVALID_ARGUMENT}; let connection=match redis_connection(a[0]){Ok(c)=>c,Err(e)=>return fail_redis(r,e)}; let span=redis_operation_span("db.redis.expire"); let result=connection.expire_blocking(&key,Duration::from_secs(a[2].max(0) as u64)); finish_redis_span(span,result.is_ok()); match result{Ok(v)=>bool_result(r,v),Err(e)=>fail_redis(r,e)} } }
pub extern "C" fn redis_incr(ctx: *mut SpectraHostCallContext) -> i32 { unsafe { let Some((a,r))=args(ctx) else{return HOST_STATUS_INVALID_ARGUMENT}; if a.len()!=3{return HOST_STATUS_INVALID_ARGUMENT}; let Some(key)=string(a[1]) else{return HOST_STATUS_INVALID_ARGUMENT}; let connection=match redis_connection(a[0]){Ok(c)=>c,Err(e)=>return fail_redis(r,e)}; let span=redis_operation_span("db.redis.incr"); let result=connection.incr_blocking(&key,a[2]); finish_redis_span(span,result.is_ok()); match result{Ok(v)=>value(r,v),Err(e)=>fail_redis(r,e)} } }
pub extern "C" fn redis_exists(ctx: *mut SpectraHostCallContext) -> i32 { redis_key_op(ctx, "db.redis.exists", |c, key| c.exists_blocking(key)) }
fn redis_key_op<T: IntoRedisResult>(ctx: *mut SpectraHostCallContext, span_name: &str, operation: impl FnOnce(&RedisConnection, &str) -> Result<T, RedisError>) -> i32 { unsafe { let Some((a,r))=args(ctx) else{return HOST_STATUS_INVALID_ARGUMENT}; if a.len()!=2{return HOST_STATUS_INVALID_ARGUMENT}; let Some(key)=string(a[1]) else{return HOST_STATUS_INVALID_ARGUMENT}; let connection=match redis_connection(a[0]){Ok(c)=>c,Err(e)=>return fail_redis(r,e)}; let span=redis_operation_span(span_name); annotate_redis_span(span, &connection); let result=operation(&connection,&key); finish_redis_span(span,result.is_ok()); match result{Ok(v)=>v.into_result(r),Err(e)=>fail_redis(r,e)} } }
trait IntoRedisResult { fn into_result(self, result: &mut [i64]) -> i32; }
impl IntoRedisResult for bool { fn into_result(self,r:&mut[i64])->i32{bool_result(r,self)} }
impl IntoRedisResult for Option<Vec<u8>> { fn into_result(self,r:&mut[i64])->i32{ match self {Some(v)=>value(r,alloc(&String::from_utf8_lossy(&v))),None=>value(r,0)} } }
fn fail_redis(result: &mut [i64], error: RedisError) -> i32 { if let Ok(mut slot)=last_error().lock(){*slot=Some((error.code.to_string(),error.message.clone()));} value(result,0) }
pub const REDIS_HOST_CALLS: &[(&str, HostFunction)] = &[("spectra.api.db.redis.open",redis_open),("spectra.api.db.redis.close",redis_close),("spectra.api.db.redis.get",redis_get),("spectra.api.db.redis.set",redis_set),("spectra.api.db.redis.delete",redis_delete),("spectra.api.db.redis.expire",redis_expire),("spectra.api.db.redis.incr",redis_incr),("spectra.api.db.redis.exists",redis_exists)];
