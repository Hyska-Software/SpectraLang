use crate::error::{PoolError, PoolResult};
use crate::metrics::{MetricsState, PoolMetrics};
use spectra_runtime::tracing::{self, SpanKind, SpanStatus};
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::task::{Context, Poll, Waker};
use std::thread;
use std::time::{Duration, Instant};

pub trait ConnectionFactory: Send + Sync + 'static {
    type Connection: Send + 'static;
    type Error: std::fmt::Debug + Send + Sync + 'static;

    fn connect(&self) -> Result<Self::Connection, Self::Error>;
    fn is_valid(&self, connection: &Self::Connection) -> bool;
    fn close(&self, connection: Self::Connection);
}

#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub min_size: usize,
    pub max_size: usize,
    pub acquisition_timeout: Duration,
    pub connection_timeout: Duration,
    pub idle_timeout: Duration,
    pub shutdown_timeout: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_size: 0,
            max_size: 8,
            acquisition_timeout: Duration::from_secs(5),
            connection_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(300),
            shutdown_timeout: Duration::from_secs(5),
        }
    }
}

struct Idle<C> {
    connection: C,
    returned_at: Instant,
}
struct Waiter<F: ConnectionFactory> {
    id: usize,
    result: Mutex<Option<PoolResult<PooledConnection<F>>>>,
    waker: Mutex<Option<Waker>>,
    cancelled: AtomicBool,
}
struct State<F: ConnectionFactory> {
    idle: VecDeque<Idle<F::Connection>>,
    waiters: VecDeque<Arc<Waiter<F>>>,
    total: usize,
    checked_out: usize,
    closed: bool,
}

struct Inner<F: ConnectionFactory> {
    factory: Arc<F>,
    config: PoolConfig,
    state: Mutex<State<F>>,
    state_changed: Condvar,
    metrics: MetricsState,
    next_waiter: AtomicUsize,
}

pub struct ConnectionPool<F: ConnectionFactory> {
    inner: Arc<Inner<F>>,
}

pub struct PooledConnection<F: ConnectionFactory> {
    pool: Weak<Inner<F>>,
    factory: Arc<F>,
    connection: Option<F::Connection>,
    released: bool,
}

impl<F: ConnectionFactory> ConnectionPool<F> {
    pub fn new(factory: F, config: PoolConfig) -> PoolResult<Self> {
        validate_config(&config)?;
        let pool = Self {
            inner: Arc::new(Inner {
                factory: Arc::new(factory),
                config,
                state: Mutex::new(State {
                    idle: VecDeque::new(),
                    waiters: VecDeque::new(),
                    total: 0,
                    checked_out: 0,
                    closed: false,
                }),
                state_changed: Condvar::new(),
                metrics: MetricsState::default(),
                next_waiter: AtomicUsize::new(1),
            }),
        };
        for _ in 0..pool.inner.config.min_size {
            let connection = pool
                .inner
                .factory
                .connect()
                .map_err(|error| PoolError::Factory(format!("{error:?}")))?;
            let mut state = pool.inner.state.lock().unwrap_or_else(|e| e.into_inner());
            state.total += 1;
            state.idle.push_back(Idle {
                connection,
                returned_at: Instant::now(),
            });
            pool.inner.metrics.created.fetch_add(1, Ordering::Relaxed);
            pool.inner.metrics.idle.fetch_add(1, Ordering::Relaxed);
        }
        Ok(pool)
    }

    pub fn acquire(&self) -> AcquireFuture<F> {
        AcquireFuture {
            pool: self.inner.clone(),
            waiter: None,
            started: false,
        }
    }

    pub fn acquire_blocking(&self) -> PoolResult<PooledConnection<F>> {
        let deadline = Instant::now() + self.inner.config.acquisition_timeout;
        loop {
            match self.take_idle_or_reserve()? {
                TakeResult::Idle(connection) => {
                    return self.connect_reserved_blocking(Some(connection), deadline)
                }
                TakeResult::Reserved => return self.connect_reserved_blocking(None, deadline),
                TakeResult::Unavailable => {}
            }
            let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.closed {
                return Err(PoolError::Closed);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.inner
                    .metrics
                    .acquisition_timeouts
                    .fetch_add(1, Ordering::Relaxed);
                return Err(PoolError::AcquireTimeout(
                    self.inner.config.acquisition_timeout,
                ));
            }
            let (next, timeout) = self
                .inner
                .state_changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|e| e.into_inner());
            state = next;
            if timeout.timed_out() {
                // The OS wait may report a timeout slightly before the
                // deadline (millisecond truncation / early wakeup, notably on
                // Windows). Only fail once the deadline truly elapsed;
                // otherwise keep waiting for the remainder so the documented
                // `acquisition_timeout` contract holds exactly.
                if Instant::now() >= deadline {
                    self.inner
                        .metrics
                        .acquisition_timeouts
                        .fetch_add(1, Ordering::Relaxed);
                    return Err(PoolError::AcquireTimeout(
                        self.inner.config.acquisition_timeout,
                    ));
                }
            }
        }
    }

    fn take_idle_or_reserve(&self) -> PoolResult<TakeResult<F::Connection>> {
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed {
            return Err(PoolError::Closed);
        }
        reap_idle(&self.inner, &mut state);
        if let Some(idle) = state.idle.pop_front() {
            state.checked_out += 1;
            self.inner.metrics.idle.fetch_sub(1, Ordering::Relaxed);
            self.inner.metrics.active.fetch_add(1, Ordering::Relaxed);
            return Ok(TakeResult::Idle(idle.connection));
        }
        if state.total < self.inner.config.max_size {
            state.total += 1;
            state.checked_out += 1;
            self.inner.metrics.active.fetch_add(1, Ordering::Relaxed);
            return Ok(TakeResult::Reserved);
        }
        Ok(TakeResult::Unavailable)
    }

    fn connect_reserved_blocking(
        &self,
        existing: Option<F::Connection>,
        deadline: Instant,
    ) -> PoolResult<PooledConnection<F>> {
        let connection = match existing {
            Some(connection) => connection,
            None => {
                let started = Instant::now();
                let span = tracing::begin_external_span(SpanKind::Internal, "db.pool.create").ok();
                let result = self
                    .inner
                    .factory
                    .connect()
                    .map_err(|error| PoolError::Factory(format!("{error:?}")));
                finish_span(span, result.is_ok());
                match result {
                    Ok(connection) if started.elapsed() <= self.inner.config.connection_timeout => {
                        self.inner.metrics.created.fetch_add(1, Ordering::Relaxed);
                        connection
                    }
                    Ok(connection) => {
                        self.inner.factory.close(connection);
                        self.mark_creation_failed();
                        return Err(PoolError::Factory("connection timeout".into()));
                    }
                    Err(error) => {
                        self.mark_creation_failed();
                        return Err(error);
                    }
                }
            }
        };
        if Instant::now() > deadline {
            self.release_raw(connection, false);
            return Err(PoolError::AcquireTimeout(
                self.inner.config.acquisition_timeout,
            ));
        }
        let span = tracing::begin_external_span(SpanKind::Internal, "db.pool.acquire").ok();
        finish_span(span, true);
        Ok(PooledConnection {
            pool: Arc::downgrade(&self.inner),
            factory: self.inner.factory.clone(),
            connection: Some(connection),
            released: false,
        })
    }

    fn mark_creation_failed(&self) {
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        state.total = state.total.saturating_sub(1);
        state.checked_out = state.checked_out.saturating_sub(1);
        self.inner.metrics.active.fetch_sub(1, Ordering::Relaxed);
        self.inner.metrics.failures.fetch_add(1, Ordering::Relaxed);
        self.inner.state_changed.notify_all();
    }

    fn release_raw(&self, connection: F::Connection, valid: bool) {
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        state.checked_out = state.checked_out.saturating_sub(1);
        self.inner.metrics.active.fetch_sub(1, Ordering::Relaxed);
        if state.closed || !valid || !self.inner.factory.is_valid(&connection) {
            state.total = state.total.saturating_sub(1);
            self.inner.metrics.discarded.fetch_add(1, Ordering::Relaxed);
            let span = tracing::begin_external_span(SpanKind::Internal, "db.pool.discard").ok();
            finish_span(span, false);
            self.inner.factory.close(connection);
        } else if let Some(waiter) = pop_live_waiter(&mut state) {
            self.inner.metrics.waiters.fetch_sub(1, Ordering::Relaxed);
            self.inner.metrics.active.fetch_add(1, Ordering::Relaxed);
            let span = tracing::begin_external_span(SpanKind::Internal, "db.pool.release").ok();
            finish_span(span, true);
            let lease = PooledConnection {
                pool: Arc::downgrade(&self.inner),
                factory: self.inner.factory.clone(),
                connection: Some(connection),
                released: false,
            };
            deliver(waiter, Ok(lease));
        } else {
            let span = tracing::begin_external_span(SpanKind::Internal, "db.pool.release").ok();
            finish_span(span, true);
            state.idle.push_back(Idle {
                connection,
                returned_at: Instant::now(),
            });
            self.inner.metrics.idle.fetch_add(1, Ordering::Relaxed);
        }
        self.inner.state_changed.notify_all();
    }

    pub fn metrics(&self) -> PoolMetrics {
        self.inner.metrics.snapshot()
    }

    pub fn shutdown(&self) -> PoolResult<()> {
        let deadline = Instant::now() + self.inner.config.shutdown_timeout;
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        state.closed = true;
        while let Some(idle) = state.idle.pop_front() {
            state.total = state.total.saturating_sub(1);
            self.inner.metrics.idle.fetch_sub(1, Ordering::Relaxed);
            self.inner.factory.close(idle.connection);
        }
        for waiter in state.waiters.drain(..) {
            deliver(waiter, Err(PoolError::Closed));
        }
        self.inner.state_changed.notify_all();
        while state.checked_out != 0 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(PoolError::ShutdownTimeout(
                    self.inner.config.shutdown_timeout,
                ));
            }
            let (next, timeout) = self
                .inner
                .state_changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|e| e.into_inner());
            state = next;
            // Same early-wakeup guard as the acquire path: only report the
            // timeout once the deadline truly elapsed.
            if timeout.timed_out() && state.checked_out != 0 && Instant::now() >= deadline {
                return Err(PoolError::ShutdownTimeout(
                    self.inner.config.shutdown_timeout,
                ));
            }
        }
        Ok(())
    }
}

enum TakeResult<C> {
    Idle(C),
    Reserved,
    Unavailable,
}

pub struct AcquireFuture<F: ConnectionFactory> {
    pool: Arc<Inner<F>>,
    waiter: Option<Arc<Waiter<F>>>,
    started: bool,
}

impl<F: ConnectionFactory> Future for AcquireFuture<F> {
    type Output = PoolResult<PooledConnection<F>>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(waiter) = &this.waiter {
            *waiter.waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(cx.waker().clone());
            if let Some(result) = waiter
                .result
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                return Poll::Ready(result);
            }
            return Poll::Pending;
        }
        if !this.started {
            this.started = true;
            let deadline = Instant::now() + this.pool.config.acquisition_timeout;
            let waiter = Arc::new(Waiter {
                id: this.pool.next_waiter.fetch_add(1, Ordering::Relaxed),
                result: Mutex::new(None),
                waker: Mutex::new(Some(cx.waker().clone())),
                cancelled: AtomicBool::new(false),
            });
            let immediate = begin_async_acquire(&this.pool, waiter.clone());
            if let Some(result) = immediate {
                return Poll::Ready(result);
            }
            this.pool.metrics.waiters.fetch_add(1, Ordering::Relaxed);
            this.waiter = Some(waiter.clone());
            let weak = Arc::downgrade(&this.pool);
            let waiter_id = waiter.id;
            let timeout = this.pool.config.acquisition_timeout;
            thread::spawn(move || {
                thread::sleep(deadline.saturating_duration_since(Instant::now()));
                if !waiter.cancelled.swap(true, Ordering::AcqRel) {
                    if let Some(pool) = weak.upgrade() {
                        pool.metrics
                            .acquisition_timeouts
                            .fetch_add(1, Ordering::Relaxed);
                        remove_waiter(&pool, waiter_id);
                    }
                    deliver(waiter, Err(PoolError::AcquireTimeout(timeout)));
                }
            });
            return Poll::Pending;
        }
        Poll::Pending
    }
}

impl<F: ConnectionFactory> Drop for AcquireFuture<F> {
    fn drop(&mut self) {
        if let Some(waiter) = &self.waiter {
            if waiter
                .result
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_none()
            {
                waiter.cancelled.store(true, Ordering::Release);
                remove_waiter(&self.pool, waiter.id);
            }
        }
    }
}

fn begin_async_acquire<F: ConnectionFactory>(
    pool: &Arc<Inner<F>>,
    waiter: Arc<Waiter<F>>,
) -> Option<PoolResult<PooledConnection<F>>> {
    let mut state = pool.state.lock().unwrap_or_else(|e| e.into_inner());
    if state.closed {
        return Some(Err(PoolError::Closed));
    }
    reap_idle(pool, &mut state);
    if let Some(idle) = state.idle.pop_front() {
        state.checked_out += 1;
        pool.metrics.idle.fetch_sub(1, Ordering::Relaxed);
        pool.metrics.active.fetch_add(1, Ordering::Relaxed);
        let span = tracing::begin_external_span(SpanKind::Internal, "db.pool.acquire").ok();
        finish_span(span, true);
        return Some(Ok(PooledConnection {
            pool: Arc::downgrade(pool),
            factory: pool.factory.clone(),
            connection: Some(idle.connection),
            released: false,
        }));
    }
    if state.total < pool.config.max_size {
        state.total += 1;
        state.checked_out += 1;
        pool.metrics.active.fetch_add(1, Ordering::Relaxed);
        let factory = pool.factory.clone();
        let target = waiter.clone();
        let weak = Arc::downgrade(pool);
        let connection_timeout = pool.config.connection_timeout;
        thread::spawn(move || {
            let started = Instant::now();
            let span = tracing::begin_external_span(SpanKind::Internal, "db.pool.create").ok();
            let result = factory
                .connect()
                .map_err(|error| PoolError::Factory(format!("{error:?}")));
            finish_span(span, result.is_ok());
            if let Some(pool) = weak.upgrade() {
                match result {
                    Ok(connection) if started.elapsed() <= connection_timeout => {
                        pool.metrics.created.fetch_add(1, Ordering::Relaxed);
                        deliver_connection(&pool, target, connection);
                    }
                    Ok(connection) => {
                        pool.factory.close(connection);
                        pool.metrics.failures.fetch_add(1, Ordering::Relaxed);
                        let mut state = pool.state.lock().unwrap_or_else(|e| e.into_inner());
                        state.total = state.total.saturating_sub(1);
                        state.checked_out = state.checked_out.saturating_sub(1);
                        pool.metrics.active.fetch_sub(1, Ordering::Relaxed);
                        pool.metrics.waiters.fetch_sub(1, Ordering::Relaxed);
                        deliver(target, Err(PoolError::Factory("connection timeout".into())));
                        pool.state_changed.notify_all();
                    }
                    Err(error) => {
                        pool.metrics.failures.fetch_add(1, Ordering::Relaxed);
                        let mut state = pool.state.lock().unwrap_or_else(|e| e.into_inner());
                        state.total = state.total.saturating_sub(1);
                        state.checked_out = state.checked_out.saturating_sub(1);
                        pool.metrics.active.fetch_sub(1, Ordering::Relaxed);
                        pool.metrics.waiters.fetch_sub(1, Ordering::Relaxed);
                        deliver(target, Err(error));
                        pool.state_changed.notify_all();
                    }
                }
            }
        });
        return None;
    }
    state.waiters.push_back(waiter);
    None
}

fn deliver_connection<F: ConnectionFactory>(
    pool: &Arc<Inner<F>>,
    waiter: Arc<Waiter<F>>,
    connection: F::Connection,
) {
    if waiter.cancelled.load(Ordering::Acquire) {
        let mut state = pool.state.lock().unwrap_or_else(|e| e.into_inner());
        state.checked_out = state.checked_out.saturating_sub(1);
        state.idle.push_back(Idle {
            connection,
            returned_at: Instant::now(),
        });
        pool.metrics.active.fetch_sub(1, Ordering::Relaxed);
        pool.metrics.idle.fetch_add(1, Ordering::Relaxed);
        pool.metrics.waiters.fetch_sub(1, Ordering::Relaxed);
        pool.state_changed.notify_all();
        return;
    }
    pool.metrics.waiters.fetch_sub(1, Ordering::Relaxed);
    let span = tracing::begin_external_span(SpanKind::Internal, "db.pool.acquire").ok();
    finish_span(span, true);
    let lease = PooledConnection {
        pool: Arc::downgrade(pool),
        factory: pool.factory.clone(),
        connection: Some(connection),
        released: false,
    };
    deliver(waiter, Ok(lease));
}

fn deliver<F: ConnectionFactory>(waiter: Arc<Waiter<F>>, result: PoolResult<PooledConnection<F>>) {
    *waiter.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
    if let Some(waker) = waiter
        .waker
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    {
        waker.wake();
    }
}
fn pop_live_waiter<F: ConnectionFactory>(state: &mut State<F>) -> Option<Arc<Waiter<F>>> {
    while let Some(waiter) = state.waiters.pop_front() {
        if !waiter.cancelled.load(Ordering::Acquire) {
            return Some(waiter);
        }
    }
    None
}
fn remove_waiter<F: ConnectionFactory>(pool: &Arc<Inner<F>>, id: usize) {
    let mut state = pool.state.lock().unwrap_or_else(|e| e.into_inner());
    let before = state.waiters.len();
    state.waiters.retain(|waiter| waiter.id != id);
    if state.waiters.len() != before {
        pool.metrics.waiters.fetch_sub(1, Ordering::Relaxed);
    }
    pool.state_changed.notify_all();
}
fn reap_idle<F: ConnectionFactory>(pool: &Arc<Inner<F>>, state: &mut State<F>) {
    while let Some(front) = state.idle.front() {
        if front.returned_at.elapsed() < pool.config.idle_timeout
            || state.total <= pool.config.min_size
        {
            break;
        }
        let idle = state.idle.pop_front().unwrap();
        state.total -= 1;
        pool.metrics.idle.fetch_sub(1, Ordering::Relaxed);
        pool.metrics.discarded.fetch_add(1, Ordering::Relaxed);
        pool.factory.close(idle.connection);
    }
}
fn finish_span(span: Option<u64>, success: bool) {
    if let Some(id) = span {
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
}
fn validate_config(config: &PoolConfig) -> PoolResult<()> {
    if config.max_size == 0 {
        return Err(PoolError::InvalidConfig(
            "max_size must be greater than zero",
        ));
    }
    if config.min_size > config.max_size {
        return Err(PoolError::InvalidConfig("min_size cannot exceed max_size"));
    }
    if config.acquisition_timeout.is_zero()
        || config.connection_timeout.is_zero()
        || config.idle_timeout.is_zero()
        || config.shutdown_timeout.is_zero()
    {
        return Err(PoolError::InvalidConfig(
            "timeouts must be greater than zero",
        ));
    }
    Ok(())
}

impl<F: ConnectionFactory> PooledConnection<F> {
    pub fn connection(&self) -> PoolResult<&F::Connection> {
        self.connection.as_ref().ok_or(PoolError::Released)
    }
    pub fn connection_mut(&mut self) -> PoolResult<&mut F::Connection> {
        self.connection.as_mut().ok_or(PoolError::Released)
    }
    pub fn release(mut self) -> PoolResult<()> {
        let connection = self.connection.take().ok_or(PoolError::Released)?;
        self.released = true;
        if let Some(pool) = self.pool.upgrade() {
            let wrapper = ConnectionPool { inner: pool };
            wrapper.release_raw(connection, true);
        }
        Ok(())
    }
    pub fn invalidate(mut self) -> PoolResult<()> {
        let connection = self.connection.take().ok_or(PoolError::Released)?;
        self.released = true;
        if let Some(pool) = self.pool.upgrade() {
            let wrapper = ConnectionPool { inner: pool };
            wrapper.release_raw(connection, false);
        }
        Ok(())
    }
}

impl<F: ConnectionFactory> Drop for PooledConnection<F> {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            if let Some(pool) = self.pool.upgrade() {
                let wrapper = ConnectionPool { inner: pool };
                wrapper.release_raw(connection, true);
            } else {
                self.factory.close(connection);
            }
        }
    }
}
