pub struct PostgresStatement {
    connection: PostgresConnection,
    sql: String,
    prepared: postgres::Statement,
    params: Vec<PostgresValue>,
    executed: bool,
    rows: Vec<Vec<PostgresValue>>,
    columns: Vec<PostgresColumn>,
    cursor: usize,
}

impl PostgresStatement {
    pub fn cancellation_handle(&self) -> PostgresResult<PostgresCancellation> {
        self.connection.cancellation_handle()
    }

    pub fn bind(&mut self, index: usize, value: PostgresValue) -> PostgresResult<()> {
        if index == 0 {
            return Err(PostgresError::invalid_argument(
                "parameter indexes are 1-based",
            ));
        }
        if self.executed {
            return Err(PostgresError::new(
                "DB2505_INVALID_STATE",
                "reset required before binding",
            ));
        }
        if self.params.len() < index {
            self.params.resize(index, PostgresValue::Null);
        }
        self.params[index - 1] = value;
        Ok(())
    }

    pub fn execute(&mut self) -> PostgresResult<PostgresExecutionResult> {
        self.execute_internal(None)
    }

    fn execute_internal(
        &mut self,
        cancellation: Option<&PostgresOperationCancellation>,
    ) -> PostgresResult<PostgresExecutionResult> {
        self.executed = true;
        self.cursor = 0;
        let params = self
            .params
            .iter()
            .map(PostgresValue::as_param)
            .collect::<Vec<_>>();
        let refs = params
            .iter()
            .map(|param| param.as_ref() as &(dyn postgres::types::ToSql + Sync))
            .collect::<Vec<_>>();
        let normalized = self.sql.trim_start().to_ascii_uppercase();
        let returns_rows = normalized.starts_with("SELECT ")
            || normalized.starts_with("SELECT\n")
            || normalized.starts_with("SHOW ")
            || normalized.starts_with("VALUES ")
            || normalized.contains(" RETURNING ");
        let result = with_postgres_span(
            &self.connection.config,
            "db.postgres.query",
            if returns_rows { "QUERY" } else { "EXECUTE" },
            || {
                if let Some(cancellation) = cancellation {
                    self.connection
                        .with_cancellable_client(cancellation, |client| {
                            execute_prepared(client, &self.prepared, &refs, returns_rows)
                        })
                } else {
                    let mut state = self.connection.lock()?;
                    let client = state
                        .as_mut()
                        .ok_or_else(PostgresError::invalid_handle)?
                        .client();
                    execute_prepared(client, &self.prepared, &refs, returns_rows)
                }
            },
        )?;
        self.rows = result.rows.clone();
        self.columns = result.columns.clone();
        Ok(result)
    }

    pub fn step(&mut self) -> PostgresResult<i32> {
        if !self.executed {
            self.execute()?;
        }
        if self.cursor < self.rows.len() {
            self.cursor += 1;
            Ok(1)
        } else {
            Ok(2)
        }
    }

    pub fn step_cancellable(
        &mut self,
        cancellation: &PostgresOperationCancellation,
    ) -> PostgresResult<i32> {
        if !self.executed {
            self.execute_internal(Some(cancellation))?;
        } else if cancellation.was_cancelled_before_start() {
            return Err(PostgresError::cancelled());
        }
        if self.cursor < self.rows.len() {
            self.cursor += 1;
            Ok(1)
        } else {
            Ok(2)
        }
    }
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }
    pub fn column_type(&self, index: usize) -> PostgresResult<super::value::PostgresType> {
        self.rows
            .first()
            .and_then(|r| r.get(index))
            .map(|v| v.ty())
            .ok_or_else(|| PostgresError::invalid_argument("column index out of range"))
    }
    pub fn column_value(&self, index: usize) -> PostgresResult<PostgresValue> {
        self.rows
            .get(self.cursor.saturating_sub(1))
            .and_then(|r| r.get(index))
            .cloned()
            .ok_or_else(|| PostgresError::invalid_argument("column index out of range"))
    }
    pub fn reset(&mut self) {
        self.executed = false;
        self.params.clear();
        self.rows.clear();
        self.columns.clear();
        self.cursor = 0;
    }
    pub fn finalize(self) {}
}

fn postgres_rows_to_result(rows: Vec<postgres::Row>) -> PostgresExecutionResult {
    let columns = rows
        .first()
        .map(|row| {
            row.columns()
                .iter()
                .map(|column| PostgresColumn {
                    name: column.name().to_owned(),
                    ty: column.type_().name().to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    let values = rows
        .iter()
        .map(|row| {
            (0..row.len())
                .map(|index| PostgresValue::from_cell(row, index))
                .collect()
        })
        .collect();
    PostgresExecutionResult {
        rows: values,
        affected_rows: rows.len(),
        columns,
    }
}

fn execute_prepared(
    client: &mut Client,
    prepared: &postgres::Statement,
    params: &[&(dyn postgres::types::ToSql + Sync)],
    returns_rows: bool,
) -> PostgresResult<PostgresExecutionResult> {
    if returns_rows {
        client
            .query(prepared, params)
            .map(postgres_rows_to_result)
            .map_err(PostgresError::from)
    } else {
        let affected = client
            .execute(prepared, params)
            .map_err(PostgresError::from)?;
        Ok(PostgresExecutionResult {
            rows: Vec::new(),
            affected_rows: affected as usize,
            columns: Vec::new(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct PostgresColumn {
    pub name: String,
    pub ty: String,
}

#[derive(Debug, Clone)]
pub struct PostgresExecutionResult {
    pub rows: Vec<Vec<PostgresValue>>,
    pub affected_rows: usize,
    pub columns: Vec<PostgresColumn>,
}

pub struct PostgresTransaction {
    connection: PostgresConnection,
    active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub channel: String,
    pub payload: String,
    pub process_id: i32,
}

pub struct NotificationListener {
    connection: PostgresConnection,
    channel: String,
}

impl NotificationListener {
    pub fn channel(&self) -> &str {
        &self.channel
    }
    pub fn next_timeout(&self, timeout: Duration) -> PostgresResult<Option<Notification>> {
        let mut state = self.connection.lock()?;
        let client = state
            .as_mut()
            .ok_or_else(PostgresError::invalid_handle)?
            .client();
        let mut notifications = client.notifications();
        let mut iterator = notifications.timeout_iter(timeout);
        use fallible_iterator::FallibleIterator;
        iterator.next().map_err(PostgresError::from).map(|item| {
            item.map(|n| Notification {
                channel: n.channel().to_owned(),
                payload: n.payload().to_owned(),
                process_id: n.process_id(),
            })
        })
    }

    pub fn next_timeout_cancellable(
        &self,
        timeout: Duration,
        cancellation: &PostgresOperationCancellation,
    ) -> PostgresResult<Option<Notification>> {
        let started = std::time::Instant::now();
        loop {
            if cancellation.was_cancelled_before_start() {
                return Err(PostgresError::cancelled());
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Ok(None);
            }
            let slice = remaining.min(Duration::from_millis(50));
            let mut state = self.connection.lock()?;
            let client = state
                .as_mut()
                .ok_or_else(PostgresError::invalid_handle)?
                .client();
            let mut notifications = client.notifications();
            let mut iterator = notifications.timeout_iter(slice);
            use fallible_iterator::FallibleIterator;
            let item = iterator.next().map_err(PostgresError::from)?.map(|notification| {
                Notification {
                    channel: notification.channel().to_owned(),
                    payload: notification.payload().to_owned(),
                    process_id: notification.process_id(),
                }
            });
            drop(iterator);
            drop(notifications);
            drop(state);
            if item.is_some() {
                return Ok(item);
            }
        }
    }
}

impl Drop for NotificationListener {
    fn drop(&mut self) {
        let _ = self
            .connection
            .execute(&format!("UNLISTEN \"{}\"", self.channel), &[]);
    }
}

impl PostgresTransaction {
    pub fn execute(&self, sql: &str) -> PostgresResult<PostgresExecutionResult> {
        if !self.active {
            return Err(PostgresError::new(
                "DB2505_TRANSACTION_CLOSED",
                "transaction is closed",
            ));
        }
        self.connection.execute(sql, &[])
    }
    pub fn savepoint(&self, name: &str) -> PostgresResult<()> {
        validate_identifier(name, "savepoint")?;
        self.execute(&format!("SAVEPOINT \"{name}\"")).map(|_| ())
    }
    pub fn rollback_to(&self, name: &str) -> PostgresResult<()> {
        validate_identifier(name, "savepoint")?;
        self.execute(&format!("ROLLBACK TO SAVEPOINT \"{name}\""))
            .map(|_| ())
    }
    pub fn release_savepoint(&self, name: &str) -> PostgresResult<()> {
        validate_identifier(name, "savepoint")?;
        self.execute(&format!("RELEASE SAVEPOINT \"{name}\""))
            .map(|_| ())
    }
    pub fn commit(mut self) -> PostgresResult<()> {
        self.connection.execute("COMMIT", &[])?;
        self.active = false;
        Ok(())
    }
    pub fn rollback(mut self) -> PostgresResult<()> {
        self.connection.execute("ROLLBACK", &[])?;
        self.active = false;
        Ok(())
    }
}

impl Drop for PostgresTransaction {
    fn drop(&mut self) {
        if self.active {
            let _ = self.connection.execute("ROLLBACK", &[]);
        }
    }
}

