use super::error::{PostgresError, PostgresResult};
use spectra_runtime::tracing::{self, TraceContext};
use std::future::Future;
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, TrySendError},
    Arc, Mutex, OnceLock,
};
use std::task::{Context, Poll, Waker};
use std::thread;

type Job = Box<dyn FnOnce() + Send + 'static>;
type CancelJob = Box<dyn FnOnce() + Send + 'static>;

struct WorkerQueue {
    sender: mpsc::SyncSender<Job>,
}

static WORKERS: OnceLock<WorkerQueue> = OnceLock::new();
static CANCELLERS: OnceLock<mpsc::SyncSender<CancelJob>> = OnceLock::new();

fn workers() -> &'static WorkerQueue {
    WORKERS.get_or_init(|| {
        // PostgreSQL calls are blocking at the crate boundary. Keep them off the
        // language reactor, but use a bounded, shared worker queue instead of
        // creating an unbounded OS thread per future.
        let (sender, receiver) = mpsc::sync_channel::<Job>(64);
        let receiver = Arc::new(Mutex::new(receiver));
        for index in 0..4 {
            let receiver = Arc::clone(&receiver);
            thread::Builder::new()
                .name(format!("spectra-postgres-io-{index}"))
                .spawn(move || loop {
                    let job = receiver
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .recv();
                    match job {
                        Ok(job) => job(),
                        Err(_) => break,
                    }
                })
                .expect("postgres worker thread");
        }
        WorkerQueue { sender }
    })
}

fn cancellers() -> &'static mpsc::SyncSender<CancelJob> {
    CANCELLERS.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel::<CancelJob>(64);
        let receiver = Arc::new(Mutex::new(receiver));
        for index in 0..2 {
            let receiver = Arc::clone(&receiver);
            thread::Builder::new()
                .name(format!("spectra-postgres-cancel-{index}"))
                .spawn(move || loop {
                    let job = receiver
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .recv();
                    match job {
                        Ok(job) => job(),
                        Err(_) => break,
                    }
                })
                .expect("postgres cancellation worker");
        }
        sender
    })
}

pub(super) fn dispatch_cancellation(work: impl FnOnce() + Send + 'static) -> PostgresResult<()> {
    cancellers().try_send(Box::new(work)).map_err(|error| {
        let code = match error {
            TrySendError::Full(_) => "DB2505_ASYNC_QUEUE_FULL",
            TrySendError::Disconnected(_) => "DB2505_ASYNC_WORKER_UNAVAILABLE",
        };
        PostgresError::new(code, "PostgreSQL cancellation queue is unavailable")
    })
}

struct Shared<T> {
    result: Mutex<Option<PostgresResult<T>>>,
    waker: Mutex<Option<Waker>>,
    cancelled: AtomicBool,
    finished: AtomicBool,
}

pub struct PostgresFuture<T> {
    shared: Arc<Shared<T>>,
    started: bool,
    operation: Option<Box<dyn FnOnce() -> PostgresResult<T> + Send + 'static>>,
    cancel: Option<Arc<dyn Fn() -> PostgresResult<()> + Send + Sync + 'static>>,
    parent: Option<TraceContext>,
}

impl<T: Send + 'static> PostgresFuture<T> {
    pub(crate) fn new_cancellable(
        operation: impl FnOnce() -> PostgresResult<T> + Send + 'static,
        cancel: Option<Arc<dyn Fn() -> PostgresResult<()> + Send + Sync + 'static>>,
    ) -> Self {
        let parent = tracing::current().and_then(|id| tracing::context(id).ok());
        Self {
            shared: Arc::new(Shared {
                result: Mutex::new(None),
                waker: Mutex::new(None),
                cancelled: AtomicBool::new(false),
                finished: AtomicBool::new(false),
            }),
            started: false,
            operation: Some(Box::new(operation)),
            cancel,
            parent,
        }
    }

    /// Requests cancellation without blocking the polling thread.
    ///
    /// If the operation is still queued it will never reach PostgreSQL. Once
    /// running, the captured PostgreSQL cancellation token is dispatched on a
    /// separate bounded worker.
    pub fn cancel(&self) -> bool {
        if self.shared.finished.load(Ordering::Acquire)
            || self.shared.cancelled.swap(true, Ordering::AcqRel)
        {
            return false;
        }
        if let Some(cancel) = self.cancel.clone() {
            if let Err(error) = cancel() {
                let mut result = self
                    .shared
                    .result
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *result = Some(Err(error));
                self.shared.finished.store(true, Ordering::Release);
                if let Some(waker) = self
                    .shared
                    .waker
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take()
                {
                    waker.wake();
                }
            }
        }
        true
    }
}

impl<T: Send + 'static> Future for PostgresFuture<T> {
    type Output = PostgresResult<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(result) = this
            .shared
            .result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            return Poll::Ready(result);
        }

        *this
            .shared
            .waker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(cx.waker().clone());
        if let Some(result) = this
            .shared
            .result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = this
                .shared
                .waker
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            return Poll::Ready(result);
        }

        if !this.started {
            this.started = true;
            let shared = Arc::clone(&this.shared);
            let operation = this.operation.take().expect("postgres future operation");
            let parent = this.parent.clone();
            let job: Job = Box::new(move || {
                let result = if shared.cancelled.load(Ordering::Acquire) {
                    Err(PostgresError::cancelled())
                } else {
                    tracing::with_context(parent, operation)
                };
                if !shared.finished.swap(true, Ordering::AcqRel) {
                    *shared
                        .result
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(result);
                }
                if let Some(waker) = shared
                    .waker
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take()
                {
                    waker.wake();
                }
            });
            if let Err(error) = workers().sender.try_send(job) {
                let code = match error {
                    TrySendError::Full(_) => "DB2505_ASYNC_QUEUE_FULL",
                    TrySendError::Disconnected(_) => "DB2505_ASYNC_WORKER_UNAVAILABLE",
                };
                *this
                    .shared
                    .result
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Err(
                    PostgresError::new(code, "PostgreSQL worker queue unavailable"),
                ));
                this.shared.finished.store(true, Ordering::Release);
                if let Some(waker) = this
                    .shared
                    .waker
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take()
                {
                    waker.wake();
                }
            }
        }
        Poll::Pending
    }
}

pub type PostgresPrepareFuture = PostgresFuture<super::connection::PostgresStatement>;
pub type PostgresExecuteFuture = PostgresFuture<super::connection::PostgresExecutionResult>;
pub type PostgresQueryFuture = PostgresFuture<super::connection::PostgresExecutionResult>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn queue_reuses_bounded_worker_threads() {
        let (tx, rx) = mpsc::sync_channel(1);
        let _ = tx.send(Box::new(|| {}));
        drop(rx);
        let _ = workers();
        thread::sleep(Duration::from_millis(5));
        assert!(WORKERS.get().is_some());
    }

    #[test]
    fn cancellation_before_poll_prevents_dispatch() {
        let future = PostgresFuture::new_cancellable(|| Ok::<_, PostgresError>(7), None);
        assert!(future.cancel());
        assert!(!future.cancel());
    }
}
