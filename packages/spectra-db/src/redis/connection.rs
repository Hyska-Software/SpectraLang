use super::async_ops::RedisFuture;
use super::error::{RedisError, RedisResult};
use super::value::RedisValue;
use crate::{ConnectionFactory, ConnectionPool, PoolConfig};
use redis::Commands;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedisTlsMode {
    Disabled,
    NativeTls,
}

#[derive(Clone)]
pub struct RedisConfig {
    pub host: String,
    pub port: u16,
    pub database: u8,
    pub username: Option<String>,
    pub password: crate::postgres::SecretString,
    pub connect_timeout: Duration,
    pub command_timeout: Duration,
    pub tls: RedisTlsMode,
}

impl std::fmt::Debug for RedisConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("database", &self.database)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("tls", &self.tls)
            .finish()
    }
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 6379,
            database: 0,
            username: None,
            password: "".into(),
            connect_timeout: Duration::from_secs(5),
            command_timeout: Duration::from_secs(5),
            tls: RedisTlsMode::Disabled,
        }
    }
}

impl RedisConfig {
    pub fn from_url(value: &str) -> RedisResult<Self> {
        let url = Url::parse(value)
            .map_err(|e| RedisError::invalid_argument(format!("invalid Redis URL: {e}")))?;
        if url.scheme() != "redis" && url.scheme() != "rediss" {
            return Err(RedisError::invalid_argument(
                "Redis URL must use redis:// or rediss://",
            ));
        }
        let host = url
            .host_str()
            .ok_or_else(|| RedisError::invalid_argument("Redis URL requires a host"))?;
        let database = url.path().trim_matches('/').parse::<u8>().unwrap_or(0);
        if url.path().trim_matches('/').contains('/') || database > 15 {
            return Err(RedisError::invalid_argument(
                "Redis database must be between 0 and 15",
            ));
        }
        let password = url.password().unwrap_or_default().to_owned();
        Ok(Self {
            host: host.to_owned(),
            port: url
                .port()
                .unwrap_or(if url.scheme() == "rediss" { 6380 } else { 6379 }),
            database,
            username: (!url.username().is_empty()).then(|| url.username().to_owned()),
            password: password.into(),
            tls: if url.scheme() == "rediss" {
                RedisTlsMode::NativeTls
            } else {
                RedisTlsMode::Disabled
            },
            ..Self::default()
        })
    }
    fn connection_url(&self) -> String {
        let scheme = if self.tls == RedisTlsMode::NativeTls {
            "rediss"
        } else {
            "redis"
        };
        let auth = match (&self.username, self.password.expose_secret().is_empty()) {
            (Some(user), false) => format!("{}:{}@", user, self.password.expose_secret()),
            (Some(user), true) => format!("{}@", user),
            (None, false) => format!(":{}@", self.password.expose_secret()),
            (None, true) => String::new(),
        };
        format!(
            "{scheme}://{auth}{}:{}/{}",
            self.host, self.port, self.database
        )
    }
    fn validate(&self) -> RedisResult<()> {
        if self.host.is_empty()
            || self.port == 0
            || self.database > 15
            || self.command_timeout.is_zero()
            || self.connect_timeout.is_zero()
        {
            return Err(RedisError::invalid_argument("invalid Redis configuration"));
        }
        Ok(())
    }
}

struct Inner {
    connection: Mutex<Option<redis::Connection>>,
    config: RedisConfig,
}

#[derive(Clone)]
pub struct RedisConnection {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for RedisConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisConnection")
            .field("config", &self.inner.config)
            .finish()
    }
}

impl RedisConnection {
    pub fn open(config: RedisConfig) -> RedisResult<Self> {
        config.validate()?;
        let client = redis::Client::open(config.connection_url()).map_err(RedisError::from)?;
        let mut connection = client.get_connection().map_err(RedisError::from)?;
        connection
            .set_read_timeout(Some(config.command_timeout))
            .map_err(RedisError::from)?;
        connection
            .set_write_timeout(Some(config.command_timeout))
            .map_err(RedisError::from)?;
        let _: String = redis::cmd("PING")
            .query(&mut connection)
            .map_err(RedisError::from)?;
        let _: () = redis::cmd("SELECT")
            .arg(config.database)
            .query(&mut connection)
            .map_err(RedisError::from)?;
        Ok(Self {
            inner: Arc::new(Inner {
                connection: Mutex::new(Some(connection)),
                config,
            }),
        })
    }
    pub fn config(&self) -> &RedisConfig {
        &self.inner.config
    }
    fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut redis::Connection) -> RedisResult<T>,
    ) -> RedisResult<T> {
        let mut guard = self
            .inner
            .connection
            .lock()
            .map_err(|_| RedisError::new("DB2507_LOCK", "Redis connection lock poisoned"))?;
        let connection = guard.as_mut().ok_or_else(RedisError::closed)?;
        operation(connection)
    }
    pub fn ping(&self) -> RedisFuture<()> {
        let this = self.clone();
        RedisFuture::new(move || {
            this.with_connection(|c| {
                redis::cmd("PING")
                    .query::<String>(c)
                    .map(|_| ())
                    .map_err(RedisError::from)
            })
        })
    }
    pub fn ping_blocking(&self) -> RedisResult<()> {
        self.with_connection(|c| {
            redis::cmd("PING")
                .query::<String>(c)
                .map(|_| ())
                .map_err(RedisError::from)
        })
    }
    pub fn get(&self, key: impl Into<String>) -> RedisFuture<Option<RedisValue>> {
        let key = key.into();
        let this = self.clone();
        RedisFuture::new(move || {
            validate_key(&key)?;
            this.with_connection(|c| {
                c.get::<_, Option<Vec<u8>>>(&key)
                    .map(|v| v.map(RedisValue::from_bytes))
                    .map_err(RedisError::from)
            })
        })
    }
    pub fn get_blocking(&self, key: &str) -> RedisResult<Option<RedisValue>> {
        validate_key(key)?;
        self.with_connection(|c| {
            c.get::<_, Option<Vec<u8>>>(key)
                .map(|v| v.map(RedisValue::from_bytes))
                .map_err(RedisError::from)
        })
    }
    pub fn set(
        &self,
        key: impl Into<String>,
        value: RedisValue,
        expiration: Option<Duration>,
    ) -> RedisFuture<()> {
        let key = key.into();
        let this = self.clone();
        RedisFuture::new(move || {
            validate_key(&key)?;
            let bytes = value.into_bytes()?;
            this.with_connection(|c| {
                let mut command = redis::cmd("SET");
                command.arg(&key).arg(bytes);
                if let Some(ttl) = expiration {
                    if ttl.is_zero() {
                        return Err(RedisError::invalid_argument("TTL must be positive"));
                    }
                    command.arg("PX").arg(ttl.as_millis() as u64);
                }
                command
                    .query::<String>(c)
                    .map(|_| ())
                    .map_err(RedisError::from)
            })
        })
    }
    pub fn set_blocking(
        &self,
        key: &str,
        value: RedisValue,
        expiration: Option<Duration>,
    ) -> RedisResult<()> {
        validate_key(key)?;
        let bytes = value.into_bytes()?;
        self.with_connection(|c| {
            let mut command = redis::cmd("SET");
            command.arg(key).arg(bytes);
            if let Some(ttl) = expiration {
                if ttl.is_zero() {
                    return Err(RedisError::invalid_argument("TTL must be positive"));
                }
                command.arg("PX").arg(ttl.as_millis() as u64);
            }
            command
                .query::<String>(c)
                .map(|_| ())
                .map_err(RedisError::from)
        })
    }
    pub fn delete(&self, key: impl Into<String>) -> RedisFuture<bool> {
        let key = key.into();
        let this = self.clone();
        RedisFuture::new(move || {
            validate_key(&key)?;
            this.with_connection(|c| {
                c.del::<_, i64>(&key)
                    .map(|n| n > 0)
                    .map_err(RedisError::from)
            })
        })
    }
    pub fn delete_blocking(&self, key: &str) -> RedisResult<bool> {
        validate_key(key)?;
        self.with_connection(|c| {
            c.del::<_, i64>(key)
                .map(|n| n > 0)
                .map_err(RedisError::from)
        })
    }
    pub fn expire(&self, key: impl Into<String>, ttl: Duration) -> RedisFuture<bool> {
        let key = key.into();
        let this = self.clone();
        RedisFuture::new(move || {
            validate_key(&key)?;
            if ttl.is_zero() {
                return Err(RedisError::invalid_argument("TTL must be positive"));
            }
            this.with_connection(|c| {
                c.expire::<_, bool>(&key, ttl.as_secs() as i64)
                    .map_err(RedisError::from)
            })
        })
    }
    pub fn expire_blocking(&self, key: &str, ttl: Duration) -> RedisResult<bool> {
        validate_key(key)?;
        if ttl.is_zero() {
            return Err(RedisError::invalid_argument("TTL must be positive"));
        }
        self.with_connection(|c| {
            c.expire::<_, bool>(key, ttl.as_secs() as i64)
                .map_err(RedisError::from)
        })
    }
    pub fn incr(&self, key: impl Into<String>, amount: i64) -> RedisFuture<i64> {
        let key = key.into();
        let this = self.clone();
        RedisFuture::new(move || {
            validate_key(&key)?;
            this.with_connection(|c| c.incr::<_, _, i64>(&key, amount).map_err(RedisError::from))
        })
    }
    pub fn incr_blocking(&self, key: &str, amount: i64) -> RedisResult<i64> {
        validate_key(key)?;
        self.with_connection(|c| c.incr::<_, _, i64>(key, amount).map_err(RedisError::from))
    }
    pub fn exists(&self, key: impl Into<String>) -> RedisFuture<bool> {
        let key = key.into();
        let this = self.clone();
        RedisFuture::new(move || {
            validate_key(&key)?;
            this.with_connection(|c| c.exists::<_, bool>(&key).map_err(RedisError::from))
        })
    }
    pub fn exists_blocking(&self, key: &str) -> RedisResult<bool> {
        validate_key(key)?;
        self.with_connection(|c| c.exists::<_, bool>(key).map_err(RedisError::from))
    }
    pub fn close(&self) -> RedisResult<()> {
        let mut guard = self
            .inner
            .connection
            .lock()
            .map_err(|_| RedisError::new("DB2507_LOCK", "Redis connection lock poisoned"))?;
        guard.take();
        Ok(())
    }
    pub fn subscribe(&self, channel: impl Into<String>) -> RedisResult<RedisPubSub> {
        let channel = channel.into();
        validate_key(&channel)?;
        let client =
            redis::Client::open(self.inner.config.connection_url()).map_err(RedisError::from)?;
        let mut connection = client.get_connection().map_err(RedisError::from)?;
        connection
            .set_read_timeout(Some(self.inner.config.command_timeout))
            .map_err(RedisError::from)?;
        connection
            .set_write_timeout(Some(self.inner.config.command_timeout))
            .map_err(RedisError::from)?;
        // Do not use `Connection::as_pubsub()` here.  The redis-rs
        // `PubSub` guard unsubscribes from every channel when it is dropped;
        // keeping that guard in a short-lived block would therefore make the
        // returned handle silently unsubscribe before its first notification.
        // The owned `RedisPubSub` handle reads the protocol directly instead,
        // so its connection remains subscribed for its whole lifetime.
        let _: redis::Value = redis::cmd("SUBSCRIBE")
            .arg(&channel)
            .query(&mut connection)
            .map_err(RedisError::from)?;
        Ok(RedisPubSub {
            connection: Arc::new(Mutex::new(Some(connection))),
            channel,
        })
    }
}

#[derive(Clone)]
pub struct RedisPubSub {
    pub(crate) connection: Arc<Mutex<Option<redis::Connection>>>,
    channel: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisNotification {
    pub channel: String,
    pub payload: Vec<u8>,
}
impl RedisPubSub {
    pub fn channel(&self) -> &str {
        &self.channel
    }
    pub fn next_notification(&self) -> RedisFuture<Option<RedisNotification>> {
        let connection = Arc::clone(&self.connection);
        RedisFuture::new(move || {
            let mut guard = connection
                .lock()
                .map_err(|_| RedisError::new("DB2507_LOCK", "Redis pub/sub lock poisoned"))?;
            let connection = guard.as_mut().ok_or_else(RedisError::closed)?;
            let value = connection.recv_response().map_err(RedisError::from)?;
            let message = redis::Msg::from_value(&value).ok_or_else(|| {
                RedisError::new(
                    "DB2507_PROTOCOL",
                    "Redis returned a non-pub/sub notification response",
                )
            })?;
            Ok(Some(RedisNotification {
                channel: message.get_channel_name().to_owned(),
                payload: message.get_payload_bytes().to_vec(),
            }))
        })
    }
    pub fn unsubscribe(&self) -> RedisResult<()> {
        let mut guard = self
            .connection
            .lock()
            .map_err(|_| RedisError::new("DB2507_LOCK", "Redis pub/sub lock poisoned"))?;
        if let Some(connection) = guard.as_mut() {
            let _: redis::Value = redis::cmd("UNSUBSCRIBE")
                .arg(&self.channel)
                .query(connection)
                .map_err(RedisError::from)?;
        }
        guard.take();
        Ok(())
    }
}
impl Drop for RedisPubSub {
    fn drop(&mut self) {
        let _ = self.unsubscribe();
    }
}

#[derive(Clone)]
pub struct RedisFactory {
    pub config: RedisConfig,
}
impl ConnectionFactory for RedisFactory {
    type Connection = RedisConnection;
    type Error = RedisError;
    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        RedisConnection::open(self.config.clone())
    }
    fn is_valid(&self, connection: &Self::Connection) -> bool {
        connection
            .inner
            .connection
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }
    fn close(&self, connection: Self::Connection) {
        let _ = connection.close();
    }
}
pub type RedisPool = ConnectionPool<RedisFactory>;
pub fn open_pool(config: RedisConfig, pool_config: PoolConfig) -> RedisResult<RedisPool> {
    ConnectionPool::new(RedisFactory { config }, pool_config)
        .map_err(|e| RedisError::new("DB2507_POOL", e.to_string()))
}
pub trait RedisKeyValueStore {
    fn get(&self, key: String) -> RedisFuture<Option<RedisValue>>;
    fn set(&self, key: String, value: RedisValue, expiration: Option<Duration>) -> RedisFuture<()>;
    fn delete(&self, key: String) -> RedisFuture<bool>;
    fn expire(&self, key: String, ttl: Duration) -> RedisFuture<bool>;
}
impl RedisKeyValueStore for RedisConnection {
    fn get(&self, key: String) -> RedisFuture<Option<RedisValue>> {
        RedisConnection::get(self, key)
    }
    fn set(&self, key: String, value: RedisValue, expiration: Option<Duration>) -> RedisFuture<()> {
        RedisConnection::set(self, key, value, expiration)
    }
    fn delete(&self, key: String) -> RedisFuture<bool> {
        RedisConnection::delete(self, key)
    }
    fn expire(&self, key: String, ttl: Duration) -> RedisFuture<bool> {
        RedisConnection::expire(self, key, ttl)
    }
}
fn validate_key(key: &str) -> RedisResult<()> {
    if key.is_empty() || key.len() > 1024 {
        Err(RedisError::invalid_argument(
            "Redis key must contain 1..1024 bytes",
        ))
    } else {
        Ok(())
    }
}
