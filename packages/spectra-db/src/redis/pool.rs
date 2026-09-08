//! Real connection pooling for the Redis driver.
//!
//! [`super::RedisConnection`] wraps a single socket, so concurrent tasks that
//! share one instance serialize on its command mutex. [`RedisConnectionPool`]
//! fixes this by leasing distinct [`super::RedisConnection`] handles from the
//! crate-wide [`crate::ConnectionPool`] machinery and layering the Redis
//! specific pieces on top:
//!
//! * **Sizing** — `PoolConfig::min_size` connections are created eagerly and
//!   the pool refuses to grow past `PoolConfig::max_size`; acquisition beyond
//!   capacity queues until a lease is returned or
//!   `PoolConfig::acquisition_timeout` elapses.
//! * **Checkout health check** — every lease answers a protocol-level `PING`
//!   before it leaves the pool ([`RedisHealthCheck`]). Unhealthy leases are
//!   discarded (`metrics.discarded`) and replaced with fresh connections up to
//!   a bounded number of attempts.
//! * **Command timeouts respected** — `PING` traffic travels over the socket
//!   read/write timeouts installed by `RedisConnection::open` from
//!   `RedisConfig::command_timeout`, so checkout can never stall longer than
//!   roughly one command round trip per attempt.
//! * **Idle reaping** — idle leases older than `PoolConfig::idle_timeout` are
//!   closed down to `min_size` during the next acquisition attempt, matching
//!   the driver-agnostic pool behavior.
//! * **Graceful shutdown** — [`RedisConnectionPool::shutdown`] marks the pool
//!   closed, releases idle connections, drains pending waiters, and waits up to
//!   `PoolConfig::shutdown_timeout` for outstanding leases to come back.
//! * **Metrics** — [`RedisConnectionPool::metrics`] exposes the shared
//!   `PoolMetrics` snapshot (created, active, idle, discarded, failures,
//!   acquisition_timeouts, waiters).
//!
//! Asynchronous operations run on short-lived worker threads and are connected
//! to the caller through [`RedisFuture`], matching the per-operation
//! cancellation style used throughout this crate: dropping the future flags
//! cancellation and discards the eventual result (the lease, if any, is
//! released back to the pool instead of leaking).
//!
//! # Live integration
//!
//! Unit tests below drive a mock connection factory and never touch the
//! network. End-to-end behavior against a real Redis server is exercised by
//! the environment-gated integration harness in `tests/redis_integration.rs`,
//! enabled by setting `SPECTRA_REDIS_URL` (same convention as
//! `SPECTRA_POSTGRES_URL` in `tests/postgres_integration.rs`); those tests
//! self-skip when the variable is absent, keeping `cargo test` offline-safe.

use super::async_ops::RedisFuture;
use super::connection::{
    validate_key, RedisConfig, RedisConnection, RedisFactory, RedisKeyValueStore,
};
use super::error::{RedisError, RedisResult};
use super::value::RedisValue;
use crate::{ConnectionFactory, ConnectionPool, PoolConfig, PoolError, PoolMetrics};
use std::sync::Arc;
use std::time::Duration;

/// Health probe executed on checkout before a lease reaches the caller.
///
/// Implementations must be cheap and non-blocking beyond one bounded round
/// trip; for the real driver the probe is a `PING` whose traffic respects the
/// command timeout configured on the underlying socket.
pub trait RedisHealthCheck {
    /// Returns `true` when the connection still answers a `PING`.
    fn is_healthy(&self) -> bool;
}

/// Blocking key-value commands every pooled connection must support so the
/// pool can dispatch them on its worker threads. The real RESP connection
/// delegates to its inherent `*_blocking` methods; test doubles provide
/// in-memory stand-ins.
pub trait RedisPoolCommands: RedisHealthCheck {
    fn pool_get(&self, key: &str) -> RedisResult<Option<RedisValue>>;
    fn pool_set(
        &self,
        key: &str,
        value: RedisValue,
        expiration: Option<Duration>,
    ) -> RedisResult<()>;
    fn pool_delete(&self, key: &str) -> RedisResult<bool>;
    fn pool_expire(&self, key: &str, ttl: Duration) -> RedisResult<bool>;
    fn pool_incr(&self, key: &str, amount: i64) -> RedisResult<i64>;
    fn pool_exists(&self, key: &str) -> RedisResult<bool>;
}

impl RedisHealthCheck for RedisConnection {
    fn is_healthy(&self) -> bool {
        self.ping_blocking().is_ok()
    }
}

impl RedisPoolCommands for RedisConnection {
    fn pool_get(&self, key: &str) -> RedisResult<Option<RedisValue>> {
        self.get_blocking(key)
    }
    fn pool_set(
        &self,
        key: &str,
        value: RedisValue,
        expiration: Option<Duration>,
    ) -> RedisResult<()> {
        self.set_blocking(key, value, expiration)
    }
    fn pool_delete(&self, key: &str) -> RedisResult<bool> {
        self.delete_blocking(key)
    }
    fn pool_expire(&self, key: &str, ttl: Duration) -> RedisResult<bool> {
        self.expire_blocking(key, ttl)
    }
    fn pool_incr(&self, key: &str, amount: i64) -> RedisResult<i64> {
        self.incr_blocking(key, amount)
    }
    fn pool_exists(&self, key: &str) -> RedisResult<bool> {
        self.exists_blocking(key)
    }
}

const POOL_ERROR_CODE: &str = "DB2507_POOL";

fn map_pool_error(error: PoolError) -> RedisError {
    RedisError::new(POOL_ERROR_CODE, error.to_string())
}

/// Upper bound on checkout attempts before giving up on finding a healthy
/// lease. Prevents spinning forever when every pooled socket died underneath
/// us.
const MAX_CHECKOUT_ATTEMPTS: usize = 8;

fn checkout<F>(pool: &Arc<ConnectionPool<F>>) -> RedisResult<RedisPooledConnection<F>>
where
    F: ConnectionFactory,
    F::Connection: RedisHealthCheck,
{
    for _ in 0..MAX_CHECKOUT_ATTEMPTS {
        let leased = pool.acquire_blocking().map_err(map_pool_error)?;
        let healthy = match leased.connection() {
            Ok(connection) => connection.is_healthy(),
            Err(_) => false,
        };
        if healthy {
            return Ok(RedisPooledConnection {
                inner: Some(leased),
            });
        }
        // Handing the lease back as invalid closes the socket and frees pool
        // capacity; the next attempt either finds another idle lease or grows
        // the pool again.
        let _ = leased.invalidate();
    }
    Err(RedisError::new(
        POOL_ERROR_CODE,
        format!("no healthy Redis connection after {MAX_CHECKOUT_ATTEMPTS} checkout attempts"),
    ))
}

/// Real Redis connection pool.
///
/// Wraps the generic [`crate::ConnectionPool`] with Redis checkout-time health
/// checking and a [`RedisFuture`] based async bridge.
pub struct RedisConnectionPool<F: ConnectionFactory> {
    pool: Arc<ConnectionPool<F>>,
}

/// A leased Redis connection checked out from [`RedisConnectionPool`].
///

pub struct RedisPooledConnection<F: ConnectionFactory> {
    inner: Option<crate::PooledConnection<F>>,
}

impl<F: ConnectionFactory> std::fmt::Debug for RedisPooledConnection<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisPooledConnection")
            .field("leased", &self.inner.is_some())
            .finish()
    }
}

impl<F> RedisPooledConnection<F>
where
    F: ConnectionFactory,
{
    /// Borrows the wrapped connection while it is still leased.
    pub fn get(&self) -> Option<&F::Connection> {
        self.inner
            .as_ref()
            .and_then(|pooled| pooled.connection().ok())
    }

    /// Runs `operation` against the leased connection.
    pub fn with<T>(
        &self,
        operation: impl FnOnce(&F::Connection) -> RedisResult<T>,
    ) -> RedisResult<T> {
        let connection = self.get().ok_or_else(|| {
            RedisError::new(
                POOL_ERROR_CODE,
                "pooled Redis connection has already been released",
            )
        })?;
        operation(connection)
    }

    /// Returns the connection to the pool early (same effect as dropping it).
    pub fn release(mut self) -> RedisResult<()> {
        let pooled = self
            .inner
            .take()
            .ok_or(map_pool_error(PoolError::Released))?;
        pooled.release().map_err(map_pool_error)
    }

    /// Discards the connection instead of returning it to the pool. Call this
    /// after protocol-breaking failures so subsequent checkouts cannot observe
    /// a broken lease.
    pub fn invalidate(mut self) -> RedisResult<()> {
        let pooled = self
            .inner
            .take()
            .ok_or(map_pool_error(PoolError::Released))?;
        pooled.invalidate().map_err(map_pool_error)
    }
}

impl<F> RedisConnectionPool<F>
where
    F: ConnectionFactory,
    F::Connection: RedisPoolCommands,
{
    /// Creates a pool from a connection factory and configuration.
    ///
    /// The `min_size` initial connections are opened synchronously here; an
    /// unreachable server surfaces immediately as a `DB2507_POOL` error.
    pub fn new(factory: F, config: PoolConfig) -> RedisResult<Self> {
        let pool = ConnectionPool::new(factory, config).map_err(map_pool_error)?;
        Ok(Self {
            pool: Arc::new(pool),
        })
    }

    /// Checks out a healthy lease, blocking until one becomes available or
    /// `PoolConfig::acquisition_timeout` elapses.
    pub fn acquire_blocking(&self) -> RedisResult<RedisPooledConnection<F>> {
        checkout(&self.pool)
    }

    /// Checks out a healthy lease asynchronously on a worker thread.
    ///
    /// Bounded by `PoolConfig::acquisition_timeout`; dropping the future
    /// cancels delivery and any eventually produced lease returns to the pool.
    pub fn acquire(&self) -> RedisFuture<RedisPooledConnection<F>> {
        let pool = Arc::clone(&self.pool);
        RedisFuture::new(move || checkout(&pool))
    }

    /// Snapshot of the shared pool metrics.
    pub fn metrics(&self) -> PoolMetrics {
        self.pool.metrics()
    }

    /// Gracefully shuts the pool down: idles are closed immediately, queued
    /// waiters fail with a closed-pool error, and outstanding leases get
    /// `shutdown_timeout` to return before the call reports a timeout.
    pub fn shutdown(&self) -> RedisResult<()> {
        self.pool.shutdown().map_err(map_pool_error)
    }

    /// Same as [`RedisConnectionPool::shutdown`] but dispatched on a worker
    /// thread for async contexts, mirroring the per-operation bridge style.
    pub fn shutdown_async(&self) -> RedisFuture<()> {
        let pool = Arc::clone(&self.pool);
        RedisFuture::new(move || pool.shutdown().map_err(map_pool_error))
    }

    pub fn get(&self, key: impl Into<String>) -> RedisFuture<Option<RedisValue>> {
        let pool = Arc::clone(&self.pool);
        let key = key.into();
        RedisFuture::new(move || {
            validate_key(&key)?;
            checkout(&pool)?.with(|connection| connection.pool_get(&key))
        })
    }

    pub fn set(
        &self,
        key: impl Into<String>,
        value: RedisValue,
        expiration: Option<Duration>,
    ) -> RedisFuture<()> {
        let pool = Arc::clone(&self.pool);
        let key = key.into();
        RedisFuture::new(move || {
            validate_key(&key)?;
            checkout(&pool)?.with(|connection| connection.pool_set(&key, value, expiration))
        })
    }

    pub fn delete(&self, key: impl Into<String>) -> RedisFuture<bool> {
        let pool = Arc::clone(&self.pool);
        let key = key.into();
        RedisFuture::new(move || {
            validate_key(&key)?;
            checkout(&pool)?.with(|connection| connection.pool_delete(&key))
        })
    }

    pub fn expire(&self, key: impl Into<String>, ttl: Duration) -> RedisFuture<bool> {
        let pool = Arc::clone(&self.pool);
        let key = key.into();
        RedisFuture::new(move || {
            validate_key(&key)?;
            checkout(&pool)?.with(|connection| connection.pool_expire(&key, ttl))
        })
    }

    pub fn incr(&self, key: impl Into<String>, amount: i64) -> RedisFuture<i64> {
        let pool = Arc::clone(&self.pool);
        let key = key.into();
        RedisFuture::new(move || {
            validate_key(&key)?;
            checkout(&pool)?.with(|connection| connection.pool_incr(&key, amount))
        })
    }

    pub fn exists(&self, key: impl Into<String>) -> RedisFuture<bool> {
        let pool = Arc::clone(&self.pool);
        let key = key.into();
        RedisFuture::new(move || {
            validate_key(&key)?;
            checkout(&pool)?.with(|connection| connection.pool_exists(&key))
        })
    }
}

impl<F> RedisKeyValueStore for RedisConnectionPool<F>
where
    F: ConnectionFactory,
    F::Connection: RedisPoolCommands,
{
    fn get(&self, key: String) -> RedisFuture<Option<RedisValue>> {
        RedisConnectionPool::get(self, key)
    }
    fn set(&self, key: String, value: RedisValue, expiration: Option<Duration>) -> RedisFuture<()> {
        RedisConnectionPool::set(self, key, value, expiration)
    }
    fn delete(&self, key: String) -> RedisFuture<bool> {
        RedisConnectionPool::delete(self, key)
    }
    fn expire(&self, key: String, ttl: Duration) -> RedisFuture<bool> {
        RedisConnectionPool::expire(self, key, ttl)
    }
}

/// Convenience alias wiring the real RESP driver factory into the pool.
pub type RedisPool = RedisConnectionPool<RedisFactory>;

/// Opens a real Redis connection pool using [`RedisConfig`].
pub fn open_pool(config: RedisConfig, pool_config: PoolConfig) -> RedisResult<RedisPool> {
    RedisConnectionPool::new(RedisFactory { config }, pool_config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Condvar, Mutex};
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    use std::time::Instant;

    type Registry = Arc<Mutex<HashMap<usize, Arc<AtomicBool>>>>;

    #[derive(Clone, Default)]
    struct MockFactory {
        registry: Registry,
        seq: Arc<AtomicUsize>,
        connect_count: Arc<AtomicUsize>,
    }

    struct MockConnection {
        id: usize,
        registry: Registry,
    }

    impl MockFactory {
        fn connect_count(&self) -> usize {
            self.connect_count.load(Ordering::SeqCst)
        }
        fn alive_count(&self) -> usize {
            self.registry
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .filter(|flag| flag.load(Ordering::SeqCst))
                .count()
        }
        fn kill(&self, id: usize) {
            if let Some(flag) = self
                .registry
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&id)
            {
                flag.store(false, Ordering::SeqCst);
            }
        }
        fn alive(&self, id: usize) -> bool {
            self.registry
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&id)
                .is_some_and(|flag| flag.load(Ordering::SeqCst))
        }
    }

    impl ConnectionFactory for MockFactory {
        type Connection = MockConnection;
        type Error = std::io::Error;

        fn connect(&self) -> Result<Self::Connection, Self::Error> {
            let id = self.seq.fetch_add(1, Ordering::SeqCst);
            self.connect_count.fetch_add(1, Ordering::SeqCst);
            self.registry
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(id)
                .or_insert_with(|| Arc::new(AtomicBool::new(true)));
            Ok(MockConnection {
                id,
                registry: Arc::clone(&self.registry),
            })
        }

        fn is_valid(&self, connection: &Self::Connection) -> bool {
            self.alive(connection.id)
        }

        fn close(&self, connection: Self::Connection) {
            self.registry
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&connection.id);
        }
    }

    impl RedisHealthCheck for MockConnection {
        fn is_healthy(&self) -> bool {
            self.registry
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&self.id)
                .is_some_and(|flag| flag.load(Ordering::SeqCst))
        }
    }

    impl RedisPoolCommands for MockConnection {
        fn pool_get(&self, _key: &str) -> RedisResult<Option<RedisValue>> {
            Ok(None)
        }
        fn pool_set(
            &self,
            _key: &str,
            _value: RedisValue,
            _expiration: Option<Duration>,
        ) -> RedisResult<()> {
            Ok(())
        }
        fn pool_delete(&self, _key: &str) -> RedisResult<bool> {
            Ok(false)
        }
        fn pool_expire(&self, _key: &str, _ttl: Duration) -> RedisResult<bool> {
            Ok(false)
        }
        fn pool_incr(&self, _key: &str, _amount: i64) -> RedisResult<i64> {
            Ok(0)
        }
        fn pool_exists(&self, _key: &str) -> RedisResult<bool> {
            Ok(false)
        }
    }

    type TestPool = RedisConnectionPool<MockFactory>;

    fn pool_config(min_size: usize, max_size: usize) -> PoolConfig {
        PoolConfig {
            min_size,
            max_size,
            ..PoolConfig::default()
        }
    }

    fn lease_id(lease: &RedisPooledConnection<MockFactory>) -> usize {
        lease.get().expect("lease holds a connection").id
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        type Signal = (Mutex<bool>, Condvar);
        unsafe fn clone(data: *const ()) -> RawWaker {
            let arc = Arc::<Signal>::from_raw(data.cast());
            std::mem::forget(Arc::clone(&arc));
            RawWaker::new(data.cast(), &VTABLE)
        }
        unsafe fn wake(data: *const ()) {
            let arc = Arc::<Signal>::from_raw(data.cast());
            *arc.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
            arc.1.notify_all();
        }
        unsafe fn wake_by_ref(data: *const ()) {
            let arc = Arc::<Signal>::from_raw(data.cast());
            std::mem::forget(Arc::clone(&arc));
            *arc.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
            arc.1.notify_all();
        }
        unsafe fn drop_waker(data: *const ()) {
            drop(Arc::<Signal>::from_raw(data.cast()));
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop_waker);

        let signal = Arc::new((Mutex::new(false), Condvar::new()));
        let waiter = Arc::clone(&signal);
        let raw = RawWaker::new(Arc::into_raw(signal).cast(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw) };
        let mut cx = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            match Pin::as_mut(&mut future).poll(&mut cx) {
                Poll::Ready(value) => return value,
                Poll::Pending => {}
            }
            let (done, changed) = &*waiter;
            let mut ready = done.lock().unwrap_or_else(|e| e.into_inner());
            while !*ready {
                ready = changed.wait(ready).unwrap_or_else(|e| e.into_inner());
            }
            *ready = false;
        }
    }

    #[test]
    fn pool_min_max_and_acquire_timeout_are_enforced() {
        let factory = MockFactory::default();
        let config = PoolConfig {
            min_size: 2,
            max_size: 3,
            acquisition_timeout: Duration::from_millis(80),
            ..PoolConfig::default()
        };
        let pool = TestPool::new(factory.clone(), config).expect("pool opens");
        assert_eq!(factory.connect_count(), 2, "min_size prewarmed eagerly");
        assert_eq!(pool.metrics().idle, 2);

        let lease_a = pool.acquire_blocking().expect("first lease");
        let lease_b = pool.acquire_blocking().expect("second lease");
        let lease_c = pool.acquire_blocking().expect("third lease grows the pool");
        assert_eq!(factory.connect_count(), 3, "max_size honored");
        assert_ne!(lease_id(&lease_a), lease_id(&lease_b));
        assert_ne!(lease_id(&lease_b), lease_id(&lease_c));

        assert_eq!(pool.metrics().active, 3);
        let started = Instant::now();
        let error = pool
            .acquire_blocking()
            .expect_err("exhausted pool times out");
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(80),
            "waited the acquisition timeout, got {elapsed:?}"
        );
        assert!(
            error.message.contains("timed out"),
            "unexpected error: {error}"
        );
        drop((lease_a, lease_b, lease_c));
    }

    #[test]
    fn released_connection_is_reused_before_growth() {
        let factory = MockFactory::default();
        let pool = TestPool::new(factory.clone(), pool_config(0, 2)).expect("pool opens");
        let lease = pool.acquire_blocking().expect("first lease");
        let first_id = lease_id(&lease);
        drop(lease);

        let again = pool.acquire_blocking().expect("second lease");
        assert_eq!(lease_id(&again), first_id, "idle lease reused");
        assert_eq!(factory.connect_count(), 1, "no extra connection opened");
    }

    #[test]
    fn idle_connections_past_idle_timeout_are_reaped() {
        let factory = MockFactory::default();
        let config = PoolConfig {
            min_size: 0,
            max_size: 2,
            idle_timeout: Duration::from_millis(40),
            ..PoolConfig::default()
        };
        let pool = TestPool::new(factory.clone(), config).expect("pool opens");

        let lease = pool.acquire_blocking().expect("lease");
        let first_id = lease_id(&lease);
        drop(lease);
        assert_eq!(pool.metrics().idle, 1);

        std::thread::sleep(Duration::from_millis(110));
        let fresh = pool.acquire_blocking().expect("post-reap lease");
        assert_ne!(
            lease_id(&fresh),
            first_id,
            "reaped idle replaced by new connection"
        );
        assert_eq!(factory.connect_count(), 2);
        assert_eq!(
            pool.metrics().discarded,
            1,
            "idle reaping discarded the stale lease"
        );
        assert_eq!(pool.metrics().created, 2);
    }

    #[test]
    fn unhealthy_idle_connection_is_discarded_on_checkout() {
        let factory = MockFactory::default();
        // Long idle timeout so age-based reaping cannot interfere: this test
        // isolates the PING-on-checkout path.
        let config = PoolConfig {
            min_size: 0,
            max_size: 1,
            idle_timeout: Duration::from_secs(300),
            ..PoolConfig::default()
        };
        let pool = TestPool::new(factory.clone(), config).expect("pool opens");

        let lease = pool.acquire_blocking().expect("first lease");
        let dead_id = lease_id(&lease);
        drop(lease);
        factory.kill(dead_id);

        let next = pool.acquire_blocking().expect("replacement lease");
        assert_ne!(lease_id(&next), dead_id, "unhealthy lease not handed out");
        assert_eq!(
            factory.connect_count(),
            2,
            "fresh connection replaced dead lease"
        );
        assert_eq!(
            pool.metrics().discarded,
            1,
            "checkout PING discarded dead lease"
        );
    }

    #[test]
    fn shutdown_closes_idle_connections_and_rejects_acquire() {
        let factory = MockFactory::default();
        let pool = TestPool::new(factory.clone(), pool_config(1, 2)).expect("pool opens");
        assert_eq!(factory.alive_count(), 1);

        pool.shutdown().expect("graceful shutdown");
        assert_eq!(factory.alive_count(), 0, "idle sockets closed");
        assert_eq!(pool.metrics().idle, 0);

        let error = pool
            .acquire_blocking()
            .expect_err("closed pool rejects acquire");
        assert!(
            error.message.contains("closed"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn async_bridge_checks_out_and_returns_leases() {
        let factory = MockFactory::default();
        let pool = TestPool::new(factory.clone(), pool_config(1, 1)).expect("pool opens");

        let lease = block_on(pool.acquire()).expect("async lease");
        assert_eq!(pool.metrics().active, 1);
        drop(lease);
        assert_eq!(pool.metrics().active, 0);
        assert_eq!(pool.metrics().idle, 1);
    }
}
