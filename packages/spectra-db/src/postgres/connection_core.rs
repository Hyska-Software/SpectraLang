impl PostgresConnection {
    /// Sanitized connection metadata used for observability. Credentials and
    /// the original DSN are intentionally not exposed.
    pub fn server_address(&self) -> String {
        self.config.host.clone()
    }

    pub fn server_port(&self) -> u16 {
        self.config.port
    }

    pub fn database_name(&self) -> String {
        self.config.database.clone()
    }

    pub fn open(config: PostgresConfig) -> PostgresResult<Self> {
        with_postgres_span(&config, "db.postgres.connect", "CONNECT", || {
            let mut client = config.connect()?;
            if let Some(timeout) = config.statement_timeout {
                client
                    .client()
                    .batch_execute(&format!("SET statement_timeout = {}", timeout.as_millis()))
                    .map_err(PostgresError::from)?;
            }
            let connection = Self {
                state: Arc::new(Mutex::new(Some(client))),
                config: config.clone(),
            };
            connection.health_check()?;
            Ok(connection)
        })
    }

    pub fn health_check(&self) -> PostgresResult<()> {
        self.execute("SELECT 1", &[]).map(|_| ())
    }

    fn with_cancellable_client<T>(
        &self,
        cancellation: &PostgresOperationCancellation,
        work: impl FnOnce(&mut Client) -> PostgresResult<T>,
    ) -> PostgresResult<T> {
        let mut state = self.lock()?;
        let client_kind = state
            .as_mut()
            .ok_or_else(PostgresError::invalid_handle)?;
        if !cancellation.arm(client_kind.cancellation()) {
            return Err(PostgresError::cancelled());
        }
        let result = work(client_kind.client());
        let finish = cancellation.finish();
        match (result, finish) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(value), Ok(())) => Ok(value),
        }
    }

    pub fn prepare(&self, sql: impl Into<String>) -> PostgresResult<PostgresStatement> {
        let sql = sql.into();
        if sql.trim().is_empty() {
            return Err(PostgresError::invalid_argument("SQL cannot be empty"));
        }
        with_postgres_span(&self.config, "db.postgres.prepare", "PREPARE", || {
            let mut state = self.lock()?;
            let prepared = state
                .as_mut()
                .ok_or_else(PostgresError::invalid_handle)?
                .client()
                .prepare(&sql)
                .map_err(PostgresError::from)?;
            Ok(PostgresStatement {
                connection: self.clone(),
                sql,
                prepared,
                params: Vec::new(),
                executed: false,
                rows: Vec::new(),
                columns: Vec::new(),
                cursor: 0,
            })
        })
    }

    pub fn execute_query(
        &self,
        query: CompiledQuery<PostgresValue>,
    ) -> PostgresResult<PostgresExecutionResult> {
        let params = query
            .params
            .iter()
            .map(PostgresValue::as_param)
            .collect::<Vec<_>>();
        let refs = params
            .iter()
            .map(|p| p.as_ref() as &(dyn postgres::types::ToSql + Sync))
            .collect::<Vec<_>>();
        let normalized = query.sql.trim_start().to_ascii_uppercase();
        if normalized.starts_with("SELECT ")
            || normalized.starts_with("SELECT\n")
            || normalized.starts_with("SHOW ")
            || normalized.starts_with("VALUES ")
            || normalized.contains(" RETURNING ")
        {
            self.query(&query.sql, &refs)
        } else {
            self.execute(&query.sql, &refs)
        }
    }

    pub fn execute_query_cancellable(
        &self,
        query: CompiledQuery<PostgresValue>,
        cancellation: &PostgresOperationCancellation,
    ) -> PostgresResult<PostgresExecutionResult> {
        let params = query
            .params
            .iter()
            .map(PostgresValue::as_param)
            .collect::<Vec<_>>();
        let refs = params
            .iter()
            .map(|param| param.as_ref() as &(dyn postgres::types::ToSql + Sync))
            .collect::<Vec<_>>();
        let normalized = query.sql.trim_start().to_ascii_uppercase();
        let returns_rows = normalized.starts_with("SELECT ")
            || normalized.starts_with("SELECT\n")
            || normalized.starts_with("SHOW ")
            || normalized.starts_with("VALUES ")
            || normalized.contains(" RETURNING ");
        with_postgres_span(
            &self.config,
            "db.postgres.query",
            if returns_rows { "QUERY" } else { "EXECUTE" },
            || {
                self.with_cancellable_client(cancellation, |client| {
                    if returns_rows {
                        client
                            .query(&query.sql, &refs)
                            .map(postgres_rows_to_result)
                            .map_err(PostgresError::from)
                    } else {
                        let affected = client
                            .execute(&query.sql, &refs)
                            .map_err(PostgresError::from)?;
                        Ok(PostgresExecutionResult {
                            rows: Vec::new(),
                            affected_rows: affected as usize,
                            columns: Vec::new(),
                        })
                    }
                })
            },
        )
    }

    pub fn execute_builder<Q: Query>(&self, query: &Q) -> PostgresResult<PostgresExecutionResult> {
        let compiled = query.compile(&PostgresDialect).map_err(query_error)?;
        let params = compiled
            .params
            .into_iter()
            .map(sqlite_to_postgres)
            .collect();
        self.execute_query(CompiledQuery {
            sql: compiled.sql,
            params,
        })
    }

    pub fn query_async(&self, query: CompiledQuery<PostgresValue>) -> PostgresQueryFuture {
        let connection = self.clone();
        let cancellation = PostgresOperationCancellation::new();
        let operation_cancellation = cancellation.clone();
        let cancel = Arc::new(move || cancellation.request_cancel().map(|_| ()))
            as Arc<dyn Fn() -> PostgresResult<()> + Send + Sync + 'static>;
        PostgresFuture::new_cancellable(
            move || connection.execute_query_cancellable(query, &operation_cancellation),
            Some(cancel),
        )
    }

    pub fn prepare_async(&self, sql: impl Into<String>) -> PostgresPrepareFuture {
        let connection = self.clone();
        let sql = sql.into();
        PostgresFuture::new_cancellable(move || connection.prepare(sql), None)
    }

    pub fn execute_async(&self, query: CompiledQuery<PostgresValue>) -> PostgresExecuteFuture {
        let connection = self.clone();
        let cancellation = PostgresOperationCancellation::new();
        let operation_cancellation = cancellation.clone();
        let cancel = Arc::new(move || cancellation.request_cancel().map(|_| ()))
            as Arc<dyn Fn() -> PostgresResult<()> + Send + Sync + 'static>;
        PostgresFuture::new_cancellable(
            move || connection.execute_query_cancellable(query, &operation_cancellation),
            Some(cancel),
        )
    }

    pub fn cancellation_handle(&self) -> PostgresResult<PostgresCancellation> {
        let mut state = self.lock()?;
        let client = state
            .as_mut()
            .ok_or_else(PostgresError::invalid_handle)?;
        let tls = matches!(client, ClientKind::Tls(_));
        let token = client.client().cancel_token();
        Ok(PostgresCancellation {
            token,
            tls,
        })
    }

    pub fn begin(&self) -> PostgresResult<PostgresTransaction> {
        self.execute("BEGIN", &[])?;
        Ok(PostgresTransaction {
            connection: self.clone(),
            active: true,
        })
    }

    pub fn execute_batch(&self, sql: &str) -> PostgresResult<()> {
        with_postgres_span(&self.config, "db.postgres.query", "BATCH", || {
            let mut state = self.lock()?;
            state
                .as_mut()
                .ok_or_else(PostgresError::invalid_handle)?
                .client()
                .batch_execute(sql)
                .map_err(PostgresError::from)
        })
    }

    pub fn copy_in_rows(
        &self,
        sql: &str,
        rows: impl IntoIterator<Item = Vec<PostgresValue>>,
    ) -> PostgresResult<u64> {
        if !sql.trim_start().to_ascii_uppercase().starts_with("COPY ") {
            return Err(PostgresError::invalid_argument(
                "COPY IN requires a COPY statement",
            ));
        }
        with_postgres_span(&self.config, "db.postgres.copy", "COPY IN", || {
            let mut state = self.lock()?;
            let mut writer = state
                .as_mut()
                .ok_or_else(PostgresError::invalid_handle)?
                .client()
                .copy_in(sql)
                .map_err(PostgresError::from)?;
            let mut count = 0u64;
            for row in rows {
                let encoded = row
                    .iter()
                    .map(encode_copy_value)
                    .collect::<Vec<_>>()
                    .join("\t");
                writer
                    .write_all(encoded.as_bytes())
                    .map_err(|e| PostgresError::new("DB2505_COPY_IN", e.to_string()))?;
                writer
                    .write_all(b"\n")
                    .map_err(|e| PostgresError::new("DB2505_COPY_IN", e.to_string()))?;
                count += 1;
            }
            writer.finish().map_err(PostgresError::from)?;
            Ok(count)
        })
    }

    pub fn copy_in_text(&self, sql: &str, text: &str) -> PostgresResult<u64> {
        if !sql.trim_start().to_ascii_uppercase().starts_with("COPY ") {
            return Err(PostgresError::invalid_argument(
                "COPY IN requires a COPY statement",
            ));
        }
        with_postgres_span(&self.config, "db.postgres.copy", "COPY IN", || {
            let mut state = self.lock()?;
            let mut writer = state
                .as_mut()
                .ok_or_else(PostgresError::invalid_handle)?
                .client()
                .copy_in(sql)
                .map_err(PostgresError::from)?;
            writer
                .write_all(text.as_bytes())
                .map_err(|error| PostgresError::new("DB2505_COPY_IN", error.to_string()))?;
            writer.finish().map_err(PostgresError::from)?;
            Ok(text.lines().count() as u64)
        })
    }

    pub fn copy_in_text_cancellable(
        &self,
        sql: &str,
        text: &str,
        cancellation: &PostgresOperationCancellation,
    ) -> PostgresResult<u64> {
        if !sql.trim_start().to_ascii_uppercase().starts_with("COPY ") {
            return Err(PostgresError::invalid_argument(
                "COPY IN requires a COPY statement",
            ));
        }
        with_postgres_span(&self.config, "db.postgres.copy", "COPY IN", || {
            self.with_cancellable_client(cancellation, |client| {
                let mut writer = client.copy_in(sql).map_err(PostgresError::from)?;
                writer
                    .write_all(text.as_bytes())
                    .map_err(|error| PostgresError::new("DB2505_COPY_IN", error.to_string()))?;
                writer.finish().map_err(PostgresError::from)?;
                Ok(text.lines().count() as u64)
            })
        })
    }

    pub fn copy_out_bytes(&self, sql: &str) -> PostgresResult<Vec<u8>> {
        let mut output = Vec::new();
        self.copy_out_to(sql, &mut output)?;
        Ok(output)
    }

    pub fn copy_out_bytes_cancellable(
        &self,
        sql: &str,
        cancellation: &PostgresOperationCancellation,
    ) -> PostgresResult<Vec<u8>> {
        if !sql.trim_start().to_ascii_uppercase().starts_with("COPY ") {
            return Err(PostgresError::invalid_argument(
                "COPY OUT requires a COPY statement",
            ));
        }
        with_postgres_span(&self.config, "db.postgres.copy", "COPY OUT", || {
            self.with_cancellable_client(cancellation, |client| {
                let mut reader = client.copy_out(sql).map_err(PostgresError::from)?;
                let mut output = Vec::new();
                std::io::copy(&mut reader, &mut output)
                    .map_err(|error| PostgresError::new("DB2505_COPY_OUT", error.to_string()))?;
                Ok(output)
            })
        })
    }

    pub fn copy_out_bytes_cancellable_limited(
        &self,
        sql: &str,
        cancellation: &PostgresOperationCancellation,
        max_bytes: usize,
    ) -> PostgresResult<Vec<u8>> {
        if max_bytes == 0 {
            return Err(PostgresError::invalid_argument(
                "COPY OUT byte limit must be positive",
            ));
        }
        if !sql.trim_start().to_ascii_uppercase().starts_with("COPY ") {
            return Err(PostgresError::invalid_argument(
                "COPY OUT requires a COPY statement",
            ));
        }
        with_postgres_span(&self.config, "db.postgres.copy", "COPY OUT", || {
            self.with_cancellable_client(cancellation, |client| {
                let mut reader = client.copy_out(sql).map_err(PostgresError::from)?;
                let mut output = Vec::new();
                let mut chunk = [0_u8; 8192];
                loop {
                    let read = reader
                        .read(&mut chunk)
                        .map_err(|error| PostgresError::new("DB2505_COPY_OUT", error.to_string()))?;
                    if read == 0 {
                        break;
                    }
                    if output.len().saturating_add(read) > max_bytes {
                        return Err(PostgresError::new(
                            "DB2505_COPY_LIMIT",
                            format!("COPY OUT exceeds the {max_bytes}-byte text API limit"),
                        ));
                    }
                    output.extend_from_slice(&chunk[..read]);
                }
                Ok(output)
            })
        })
    }

    pub fn copy_out_to<W: Write>(&self, sql: &str, mut output: W) -> PostgresResult<u64> {
        if !sql.trim_start().to_ascii_uppercase().starts_with("COPY ") {
            return Err(PostgresError::invalid_argument(
                "COPY OUT requires a COPY statement",
            ));
        }
        with_postgres_span(&self.config, "db.postgres.copy", "COPY OUT", || {
            let mut state = self.lock()?;
            let mut reader = state
                .as_mut()
                .ok_or_else(PostgresError::invalid_handle)?
                .client()
                .copy_out(sql)
                .map_err(PostgresError::from)?;
            std::io::copy(&mut reader, &mut output)
                .map_err(|error| PostgresError::new("DB2505_COPY_OUT", error.to_string()))
        })
    }

    pub fn listen(&self, channel: &str) -> PostgresResult<NotificationListener> {
        validate_identifier(channel, "channel")?;
        with_postgres_span(&self.config, "db.postgres.listen", "LISTEN", || {
            self.execute(&format!("LISTEN \"{channel}\""), &[])?;
            Ok(NotificationListener {
                connection: self.clone(),
                channel: channel.to_owned(),
            })
        })
    }

    pub fn notify(&self, channel: &str, payload: &str) -> PostgresResult<()> {
        validate_identifier(channel, "channel")?;
        self.query("SELECT pg_notify($1, $2)", &[&channel, &payload])
            .map(|_| ())
    }

    pub fn notify_cancellable(
        &self,
        channel: &str,
        payload: &str,
        cancellation: &PostgresOperationCancellation,
    ) -> PostgresResult<()> {
        validate_identifier(channel, "channel")?;
        with_postgres_span(&self.config, "db.postgres.query", "NOTIFY", || {
            self.with_cancellable_client(cancellation, |client| {
                client
                    .query("SELECT pg_notify($1, $2)", &[&channel, &payload])
                    .map(|_| ())
                    .map_err(PostgresError::from)
            })
        })
    }

    pub fn savepoint(&self, name: &str) -> PostgresResult<()> {
        validate_identifier(name, "savepoint")?;
        self.execute_batch(&format!("SAVEPOINT \"{name}\""))
    }

    pub fn rollback_to(&self, name: &str) -> PostgresResult<()> {
        validate_identifier(name, "savepoint")?;
        self.execute_batch(&format!("ROLLBACK TO SAVEPOINT \"{name}\""))
    }

    pub fn release_savepoint(&self, name: &str) -> PostgresResult<()> {
        validate_identifier(name, "savepoint")?;
        self.execute_batch(&format!("RELEASE SAVEPOINT \"{name}\""))
    }

    pub fn close(&self) -> PostgresResult<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PostgresError::new("DB2505_LOCK", "connection lock poisoned"))?;
        state.take();
        Ok(())
    }

    pub(crate) fn execute(
        &self,
        sql: &str,
        params: &[&(dyn postgres::types::ToSql + Sync)],
    ) -> PostgresResult<PostgresExecutionResult> {
        with_postgres_span(&self.config, "db.postgres.query", "EXECUTE", || {
            let mut state = self.lock()?;
            let affected = state
                .as_mut()
                .ok_or_else(PostgresError::invalid_handle)?
                .client()
                .execute(sql, params)
                .map_err(PostgresError::from)?;
            Ok(PostgresExecutionResult {
                rows: Vec::new(),
                affected_rows: affected as usize,
                columns: Vec::new(),
            })
        })
    }

    pub(crate) fn query(
        &self,
        sql: &str,
        params: &[&(dyn postgres::types::ToSql + Sync)],
    ) -> PostgresResult<PostgresExecutionResult> {
        with_postgres_span(&self.config, "db.postgres.query", "QUERY", || {
            let mut state = self.lock()?;
            let rows = state
                .as_mut()
                .ok_or_else(PostgresError::invalid_handle)?
                .client()
                .query(sql, params)
                .map_err(PostgresError::from)?;
            let columns = rows
                .first()
                .map(|row| {
                    row.columns()
                        .iter()
                        .map(|c| PostgresColumn {
                            name: c.name().to_owned(),
                            ty: c.type_().name().to_owned(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let values = rows
                .iter()
                .map(|row| {
                    (0..row.len())
                        .map(|i| PostgresValue::from_cell(row, i))
                        .collect()
                })
                .collect();
            Ok(PostgresExecutionResult {
                rows: values,
                affected_rows: rows.len(),
                columns,
            })
        })
    }

    fn lock(&self) -> PostgresResult<std::sync::MutexGuard<'_, Option<ClientKind>>> {
        let guard = self
            .state
            .lock()
            .map_err(|_| PostgresError::new("DB2505_LOCK", "connection lock poisoned"))?;
        if guard.is_none() {
            return Err(PostgresError::invalid_handle());
        }
        Ok(guard)
    }
}

