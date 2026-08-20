pub extern "C" fn sqlite_bind_null(ctx: *mut SpectraHostCallContext) -> i32 {
    bind(ctx, SqliteValue::Null)
}
pub extern "C" fn sqlite_bind_int(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 3 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match with_statement(a[0], |statement| {
            statement.bind(a[1] as usize, SqliteValue::Integer(a[2]))
        }) {
            Ok(()) => bool_result(r, true),
            Err(error) => fail(r, error),
        }
    }
}
pub extern "C" fn sqlite_bind_float(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 3 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match with_statement(a[0], |statement| {
            statement.bind(
                a[1] as usize,
                SqliteValue::Real(f64::from_bits(a[2] as u64)),
            )
        }) {
            Ok(()) => bool_result(r, true),
            Err(error) => fail(r, error),
        }
    }
}
pub extern "C" fn sqlite_bind_text(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 3 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(text) = string(a[2]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match with_statement(a[0], |statement| {
            statement.bind(a[1] as usize, SqliteValue::Text(text))
        }) {
            Ok(()) => bool_result(r, true),
            Err(error) => fail(r, error),
        }
    }
}
pub extern "C" fn sqlite_bind_blob(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 3 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(bytes) = string(a[2]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match with_statement(a[0], |statement| {
            statement.bind(a[1] as usize, SqliteValue::Blob(bytes.into_bytes()))
        }) {
            Ok(()) => bool_result(r, true),
            Err(error) => fail(r, error),
        }
    }
}
fn bind(ctx: *mut SpectraHostCallContext, value: SqliteValue) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() < 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match with_statement(a[0], |statement| {
            statement.bind(a[1] as usize, value)
        }) {
            Ok(()) => bool_result(r, true),
            Err(error) => fail(r, error),
        }
    }
}
pub extern "C" fn sqlite_step(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let span = operation_span("db.sqlite.query");
        match with_statement(a[0], SqliteStatement::step) {
            Ok(StepResult::Row) => {
                finish_span(span, true);
                value(r, 1)
            }
            Ok(StepResult::Done) => {
                finish_span(span, true);
                value(r, 2)
            }
            Err(error) => {
                finish_span(span, false);
                fail(r, error)
            }
        }
    }
}
pub extern "C" fn sqlite_column_count(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match with_statement(a[0], |s| s.column_count()) {
            Ok(count) => value(r, count as i64),
            Err(error) => fail(r, error),
        }
    }
}
pub extern "C" fn sqlite_column_type(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match with_statement(a[0], |s| s.column_type(a[1] as usize)) {
            Ok(kind) => value(r, kind as i64),
            Err(error) => fail(r, error),
        }
    }
}
pub extern "C" fn sqlite_column_int(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match with_statement(a[0], |s| s.column_value(a[1] as usize)) {
            Ok(SqliteValue::Integer(v)) => value(r, v),
            Ok(_) => fail(
                r,
                spectra_db::sqlite::SqliteError::invalid_state("column is not integer"),
            ),
            Err(error) => fail(r, error),
        }
    }
}
pub extern "C" fn sqlite_column_float(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match with_statement(a[0], |s| s.column_value(a[1] as usize)) {
            Ok(SqliteValue::Real(v)) => value(r, v.to_bits() as i64),
            Ok(_) => fail(
                r,
                spectra_db::sqlite::SqliteError::invalid_state("column is not real"),
            ),
            Err(error) => fail(r, error),
        }
    }
}
pub extern "C" fn sqlite_column_text(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match with_statement(a[0], |s| s.column_value(a[1] as usize)) {
            Ok(SqliteValue::Text(v)) => value(r, alloc(&v)),
            Ok(_) => fail(
                r,
                spectra_db::sqlite::SqliteError::invalid_state("column is not text"),
            ),
            Err(error) => fail(r, error),
        }
    }
}
pub extern "C" fn sqlite_reset(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match with_statement(a[0], SqliteStatement::reset) {
            Ok(()) => bool_result(r, true),
            Err(error) => fail(r, error),
        }
    }
}
pub extern "C" fn sqlite_finalize(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let mut state = store().lock().unwrap();
        let Some(mut statement) = state.statements.remove(&a[0]) else {
            return fail(r, spectra_db::sqlite::SqliteError::invalid_handle());
        };
        match statement.finalize() {
            Ok(()) => bool_result(r, true),
            Err(error) => fail(r, error),
        }
    }
}
fn transaction(
    ctx: *mut SpectraHostCallContext,
    operation: &'static str,
    op: fn(&SqliteConnection) -> spectra_db::sqlite::SqliteResult<()>,
) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
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
        let span = operation_span(operation);
        match op(&connection) {
            Ok(()) => {
                finish_span(span, true);
                bool_result(r, true)
            }
            Err(error) => {
                finish_span(span, false);
                fail(r, error)
            }
        }
    }
}
pub extern "C" fn sqlite_begin(ctx: *mut SpectraHostCallContext) -> i32 {
    transaction(ctx, "db.sqlite.transaction", SqliteConnection::begin)
}
pub extern "C" fn sqlite_commit(ctx: *mut SpectraHostCallContext) -> i32 {
    transaction(ctx, "db.sqlite.commit", SqliteConnection::commit)
}
pub extern "C" fn sqlite_rollback(ctx: *mut SpectraHostCallContext) -> i32 {
    transaction(ctx, "db.sqlite.rollback", SqliteConnection::rollback)
}
pub extern "C" fn sqlite_last_error_code(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((_, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let code = last_error()
            .lock()
            .ok()
            .and_then(|v| v.as_ref().map(|x| x.0.clone()))
            .unwrap_or_default();
        value(r, alloc(&code))
    }
}
pub extern "C" fn sqlite_last_error_message(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((_, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let message = last_error()
            .lock()
            .ok()
            .and_then(|v| v.as_ref().map(|x| x.1.clone()))
            .unwrap_or_default();
        value(r, alloc(&message))
    }
}

pub const HOST_CALLS: &[(&str, HostFunction)] = &[
    ("spectra.api.db.sqlite.open", sqlite_open),
    ("spectra.api.db.sqlite.close", sqlite_close),
    ("spectra.api.db.sqlite.prepare", sqlite_prepare),
    ("spectra.api.db.sqlite.execute_async", sqlite_execute_async),
    ("spectra.api.db.sqlite.bind_null", sqlite_bind_null),
    ("spectra.api.db.sqlite.bind_int", sqlite_bind_int),
    ("spectra.api.db.sqlite.bind_float", sqlite_bind_float),
    ("spectra.api.db.sqlite.bind_text", sqlite_bind_text),
    ("spectra.api.db.sqlite.bind_blob", sqlite_bind_blob),
    ("spectra.api.db.sqlite.step", sqlite_step),
    ("spectra.api.db.sqlite.column_count", sqlite_column_count),
    ("spectra.api.db.sqlite.column_type", sqlite_column_type),
    ("spectra.api.db.sqlite.column_int", sqlite_column_int),
    ("spectra.api.db.sqlite.column_float", sqlite_column_float),
    ("spectra.api.db.sqlite.column_text", sqlite_column_text),
    ("spectra.api.db.sqlite.reset", sqlite_reset),
    ("spectra.api.db.sqlite.finalize", sqlite_finalize),
    ("spectra.api.db.sqlite.begin", sqlite_begin),
    ("spectra.api.db.sqlite.commit", sqlite_commit),
    ("spectra.api.db.sqlite.rollback", sqlite_rollback),
    (
        "spectra.api.db.sqlite.last_error_code",
        sqlite_last_error_code,
    ),
    (
        "spectra.api.db.sqlite.last_error_message",
        sqlite_last_error_message,
    ),
];
