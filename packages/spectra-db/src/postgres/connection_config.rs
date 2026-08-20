use super::async_ops::{
    PostgresExecuteFuture, PostgresFuture, PostgresPrepareFuture, PostgresQueryFuture,
};
use super::error::{PostgresError, PostgresResult};
use super::value::PostgresValue;
use crate::query::{CompiledQuery, PostgresDialect, Query, QueryError};
use crate::sqlite::SqliteValue;
use crate::{ConnectionFactory, ConnectionPool, PoolConfig};
use native_tls::TlsConnector;
use postgres::{CancelToken, Client, NoTls};
use postgres_native_tls::MakeTlsConnector;
use spectra_runtime::tracing::{self, SpanKind, SpanStatus};
use std::io::{Read, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslMode {
    Disable,
    Prefer,
    Require,
}

#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}
impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self(value)
    }
}
impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

#[derive(Debug, Clone)]
pub struct PostgresConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: SecretString,
    pub ssl_mode: SslMode,
    pub connect_timeout: Duration,
    pub statement_timeout: Option<Duration>,
}

impl Default for PostgresConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 5432,
            database: "postgres".into(),
            user: "postgres".into(),
            password: "".into(),
            ssl_mode: SslMode::Disable,
            connect_timeout: Duration::from_secs(5),
            statement_timeout: None,
        }
    }
}

impl PostgresConfig {
    pub fn from_url(url: &str) -> PostgresResult<Self> {
        let parsed = url::Url::parse(url)
            .map_err(|_| PostgresError::invalid_argument("invalid PostgreSQL URL"))?;
        if parsed.scheme() != "postgres" && parsed.scheme() != "postgresql" {
            return Err(PostgresError::invalid_argument(
                "PostgreSQL URL must use postgres://",
            ));
        }
        let user = percent_encoding::percent_decode_str(parsed.username())
            .decode_utf8()
            .map_err(|_| PostgresError::invalid_argument("invalid PostgreSQL username"))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| PostgresError::invalid_argument("PostgreSQL URL requires a host"))?;
        let database = parsed.path().trim_start_matches('/');
        if user.is_empty() || host.is_empty() || database.is_empty() {
            return Err(PostgresError::invalid_argument("invalid PostgreSQL URL"));
        }
        let mut config = Self {
            host: host.to_owned(),
            port: parsed.port().unwrap_or(5432),
            database: database.to_owned(),
            user: user.into_owned(),
            password: percent_encoding::percent_decode_str(parsed.password().unwrap_or_default())
                .decode_utf8()
                .map_err(|_| PostgresError::invalid_argument("invalid PostgreSQL password"))?
                .into_owned()
                .into(),
            ..Self::default()
        };
        for (key, value) in parsed.query_pairs() {
            match key.as_ref() {
                "sslmode" => {
                    config.ssl_mode = match value.as_ref() {
                        "disable" => SslMode::Disable,
                        "prefer" => SslMode::Prefer,
                        "require" => SslMode::Require,
                        _ => return Err(PostgresError::invalid_argument("invalid sslmode")),
                    }
                }
                "connect_timeout" => {
                    let seconds = value
                        .parse::<u64>()
                        .map_err(|_| PostgresError::invalid_argument("invalid connect_timeout"))?;
                    config.connect_timeout = Duration::from_secs(seconds);
                }
                "statement_timeout" => {
                    let millis = value
                        .parse::<u64>()
                        .map_err(|_| PostgresError::invalid_argument("invalid statement_timeout"))?;
                    config.statement_timeout = Some(Duration::from_millis(millis));
                }
                _ => {}
            }
        }
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> PostgresResult<()> {
        if self.host.is_empty()
            || self.database.is_empty()
            || self.user.is_empty()
            || self.port == 0
        {
            return Err(PostgresError::invalid_argument(
                "host, port, database and user are required",
            ));
        }
        Ok(())
    }

    fn connect(&self) -> PostgresResult<ClientKind> {
        self.validate()?;
        let build_config = || {
            let mut config = postgres::Config::new();
            config
                .host(&self.host)
                .port(self.port)
                .dbname(&self.database)
                .user(&self.user)
                .connect_timeout(self.connect_timeout);
            if !self.password.expose_secret().is_empty() {
                config.password(self.password.expose_secret());
            }
            config
        };
        match self.ssl_mode {
            SslMode::Disable => build_config()
                .connect(NoTls)
                .map(ClientKind::Plain)
                .map_err(PostgresError::from),
            SslMode::Require => {
                let tls = TlsConnector::new()
                    .map_err(|e| PostgresError::new("DB2505_TLS", e.to_string()))?;
                build_config()
                    .connect(MakeTlsConnector::new(tls))
                    .map(ClientKind::Tls)
                    .map_err(PostgresError::from)
            }
            SslMode::Prefer => {
                let tls = TlsConnector::new()
                    .map_err(|e| PostgresError::new("DB2505_TLS", e.to_string()))?;
                match build_config().connect(MakeTlsConnector::new(tls)) {
                    Ok(client) => Ok(ClientKind::Tls(client)),
                    Err(_) => build_config()
                        .connect(NoTls)
                        .map(ClientKind::Plain)
                        .map_err(PostgresError::from),
                }
            }
        }
    }
}

enum ClientKind {
    Plain(Client),
    Tls(Client),
}

impl ClientKind {
    fn client(&mut self) -> &mut Client {
        match self {
            Self::Plain(c) | Self::Tls(c) => c,
        }
    }

    fn cancellation(&mut self) -> PostgresCancellation {
        let tls = matches!(self, Self::Tls(_));
        let token = self.client().cancel_token();
        PostgresCancellation { token, tls }
    }
}

#[derive(Clone)]
pub struct PostgresConnection {
    state: Arc<Mutex<Option<ClientKind>>>,
    config: PostgresConfig,
}

#[derive(Clone)]
pub struct PostgresCancellation {
    token: CancelToken,
    tls: bool,
}

impl PostgresCancellation {
    pub fn cancel(&self) -> PostgresResult<()> {
        match self.tls {
            false => self
                .token
                .cancel_query(NoTls)
                .map_err(PostgresError::from),
            true => {
                let tls = TlsConnector::new()
                    .map_err(|error| PostgresError::new("DB2505_TLS", error.to_string()))?;
                self.token
                    .cancel_query(MakeTlsConnector::new(tls))
                    .map_err(PostgresError::from)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperationPhase {
    Queued,
    Running,
    CancelRequested,
    Cancelled,
    Done,
}

struct OperationCancellationState {
    phase: OperationPhase,
    token: Option<PostgresCancellation>,
    cancel_done: bool,
    cancel_error: Option<PostgresError>,
}

struct OperationCancellationInner {
    state: Mutex<OperationCancellationState>,
    ready: Condvar,
}

/// Per-operation cancellation state. A PostgreSQL cancel token is armed only
/// after the target operation owns its backend session, so cancelling a queued
/// task cannot affect another query on the same connection.
#[derive(Clone)]
pub struct PostgresOperationCancellation {
    inner: Arc<OperationCancellationInner>,
}

impl Default for PostgresOperationCancellation {
    fn default() -> Self {
        Self::new()
    }
}

impl PostgresOperationCancellation {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(OperationCancellationInner {
                state: Mutex::new(OperationCancellationState {
                    phase: OperationPhase::Queued,
                    token: None,
                    cancel_done: false,
                    cancel_error: None,
                }),
                ready: Condvar::new(),
            }),
        }
    }

    pub fn request_cancel(&self) -> PostgresResult<bool> {
        let token = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            match state.phase {
                OperationPhase::Queued => {
                    state.phase = OperationPhase::Cancelled;
                    self.inner.ready.notify_all();
                    return Ok(true);
                }
                OperationPhase::Running => {
                    state.phase = OperationPhase::CancelRequested;
                    state.cancel_done = false;
                    state.token.clone()
                }
                OperationPhase::CancelRequested
                | OperationPhase::Cancelled
                | OperationPhase::Done => return Ok(false),
            }
        };
        let Some(token) = token else {
            return Err(PostgresError::new(
                "DB2505_CANCEL_STATE",
                "running PostgreSQL operation has no armed cancellation token",
            ));
        };
        let inner = Arc::clone(&self.inner);
        if let Err(error) = super::async_ops::dispatch_cancellation(move || {
            let cancel_error = token.cancel().err();
            let mut state = inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.cancel_error = cancel_error;
            state.cancel_done = true;
            inner.ready.notify_all();
        }) {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.cancel_error = Some(error);
            state.cancel_done = true;
            self.inner.ready.notify_all();
        }
        Ok(true)
    }

    fn arm(&self, token: PostgresCancellation) -> bool {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match state.phase {
            OperationPhase::Queued => {
                state.phase = OperationPhase::Running;
                state.token = Some(token);
                true
            }
            OperationPhase::Cancelled => false,
            _ => false,
        }
    }

    fn finish(&self) -> PostgresResult<()> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while state.phase == OperationPhase::CancelRequested && !state.cancel_done {
            state = self
                .inner
                .ready
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
        state.phase = OperationPhase::Done;
        state.token = None;
        match state.cancel_error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn was_cancelled_before_start(&self) -> bool {
        matches!(
            self.inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .phase,
            OperationPhase::CancelRequested | OperationPhase::Cancelled
        )
    }
}

impl std::fmt::Debug for PostgresConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresConnection")
            .field("config", &self.config)
            .finish()
    }
}

