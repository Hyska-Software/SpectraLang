//! Real PostgreSQL driver backed by the upstream `postgres` protocol client.
//!
//! The public async methods run protocol work on dedicated worker threads. They
//! never block the caller's executor and reuse the generic pool contract.

mod async_ops;
mod connection;
mod error;
mod value;

pub use async_ops::{PostgresExecuteFuture, PostgresPrepareFuture, PostgresQueryFuture};
pub use connection::{
    open_pool, Notification, NotificationListener, PostgresCancellation, PostgresColumn,
    PostgresConfig, PostgresConnection, PostgresFactory, PostgresOperationCancellation,
    PostgresPool, PostgresStatement, PostgresTransaction, SecretString, SslMode,
};
pub use error::{PostgresError, PostgresResult};
pub use value::{PostgresType, PostgresValue};

pub type PostgresExecutionResult = connection::PostgresExecutionResult;

#[cfg(test)]
mod tests {
    use super::PostgresConfig;

    #[test]
    fn config_debug_does_not_expose_password() {
        let config = PostgresConfig {
            password: "secret".into(),
            ..PostgresConfig::default()
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn config_parses_encoded_credentials_and_timeouts() {
        let config = PostgresConfig::from_url(
            "postgresql://alice:p%40ss@db.example:5433/app?sslmode=require&connect_timeout=7&statement_timeout=2500",
        )
        .unwrap();
        assert_eq!(config.user, "alice");
        assert_eq!(config.host, "db.example");
        assert_eq!(config.port, 5433);
        assert_eq!(config.database, "app");
        assert_eq!(config.password.expose_secret(), "p@ss");
        assert_eq!(config.ssl_mode, super::SslMode::Require);
        assert_eq!(config.connect_timeout, std::time::Duration::from_secs(7));
        assert_eq!(
            config.statement_timeout,
            Some(std::time::Duration::from_millis(2500))
        );
        assert!(!format!("{config:?}").contains("p@ss"));
    }
}
