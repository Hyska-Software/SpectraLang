use spectra_db::{ConnectionFactory, ConnectionPool, PoolConfig, PoolError};
use std::future::Future;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

struct TestServer {
    address: String,
    stop: Arc<AtomicUsize>,
    join: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn start() -> Self {
        if let Ok(address) = std::env::var("SPECTRA_POOL_TEST_SERVER") {
            return Self {
                address,
                stop: Arc::new(AtomicUsize::new(1)),
                join: None,
            };
        }
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind real TCP server");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let address = listener.local_addr().unwrap().to_string();
        let stop = Arc::new(AtomicUsize::new(0));
        let signal = stop.clone();
        let join = thread::spawn(move || {
            while signal.load(Ordering::Acquire) == 0 {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        thread::spawn(move || {
                            let mut request = [0u8; 5];
                            if stream.read_exact(&mut request).is_ok() && &request == b"PING\n" {
                                let _ = stream.write_all(b"PONG\n");
                                let mut keep_alive = [0u8; 1];
                                let _ = stream.read(&mut keep_alive);
                            }
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            address,
            stop,
            join: Some(join),
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(1, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

struct TcpFactory {
    address: String,
    created: AtomicUsize,
    closed: AtomicUsize,
}

struct TcpConnection(TcpStream);

impl ConnectionFactory for TcpFactory {
    type Connection = TcpConnection;
    type Error = std::io::Error;

    fn connect(&self) -> Result<Self::Connection, Self::Error> {
        let mut stream = TcpStream::connect(&self.address)?;
        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
        stream.write_all(b"PING\n")?;
        let mut response = [0u8; 5];
        stream.read_exact(&mut response)?;
        assert_eq!(&response, b"PONG\n");
        self.created.fetch_add(1, Ordering::Relaxed);
        Ok(TcpConnection(stream))
    }

    fn is_valid(&self, connection: &Self::Connection) -> bool {
        connection.0.peer_addr().is_ok()
    }
    fn close(&self, connection: Self::Connection) {
        drop(connection);
        self.closed.fetch_add(1, Ordering::Relaxed);
    }
}

fn pool(server: &TestServer, min_size: usize, max_size: usize) -> Arc<ConnectionPool<TcpFactory>> {
    Arc::new(
        ConnectionPool::new(
            TcpFactory {
                address: server.address.clone(),
                created: AtomicUsize::new(0),
                closed: AtomicUsize::new(0),
            },
            PoolConfig {
                min_size,
                max_size,
                // Keep enough scheduling headroom for the real TCP server and
                // connection worker on loaded CI hosts.
                acquisition_timeout: Duration::from_millis(500),
                connection_timeout: Duration::from_secs(1),
                idle_timeout: Duration::from_millis(20),
                shutdown_timeout: Duration::from_millis(200),
            },
        )
        .unwrap(),
    )
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let signal = Arc::new((Mutex::new(false), Condvar::new()));
    let wake_signal = signal.clone();
    fn raw_waker(signal: Arc<(Mutex<bool>, Condvar)>) -> std::task::RawWaker {
        fn clone(data: *const ()) -> std::task::RawWaker {
            let arc = unsafe { Arc::<(Mutex<bool>, Condvar)>::from_raw(data as *const _) };
            let cloned = arc.clone();
            std::mem::forget(arc);
            raw_waker(cloned)
        }
        fn wake(data: *const ()) {
            let arc = unsafe { Arc::<(Mutex<bool>, Condvar)>::from_raw(data as *const _) };
            *arc.0.lock().unwrap() = true;
            arc.1.notify_one();
        }
        fn wake_by_ref(data: *const ()) {
            let arc = unsafe { Arc::<(Mutex<bool>, Condvar)>::from_raw(data as *const _) };
            *arc.0.lock().unwrap() = true;
            arc.1.notify_one();
            std::mem::forget(arc);
        }
        fn drop_waker(data: *const ()) {
            unsafe {
                drop(Arc::<(Mutex<bool>, Condvar)>::from_raw(data as *const _));
            }
        }
        static VTABLE: std::task::RawWakerVTable =
            std::task::RawWakerVTable::new(clone, wake, wake_by_ref, drop_waker);
        std::task::RawWaker::new(Arc::into_raw(signal) as *const (), &VTABLE)
    }
    let waker = unsafe { std::task::Waker::from_raw(raw_waker(wake_signal)) };
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        let mut ready = signal.0.lock().unwrap();
        while !*ready {
            ready = signal.1.wait(ready).unwrap();
        }
        *ready = false;
    }
}

#[test]
fn real_server_happy_path_and_min_max() {
    let server = TestServer::start();
    let pool = pool(&server, 1, 2);
    let first = pool.acquire_blocking().unwrap();
    assert_eq!(pool.metrics().active, 1);
    drop(first);
    assert_eq!(pool.metrics().idle, 1);
    let second = block_on(pool.acquire()).unwrap();
    assert_eq!(pool.metrics().active, 1);
    drop(second);
    assert!(pool.metrics().created <= 2);
    pool.shutdown().unwrap();
}

#[test]
fn exhaustion_timeout_and_recovery_are_typed() {
    let server = TestServer::start();
    let pool = pool(&server, 0, 1);
    let held = pool.acquire_blocking().unwrap();
    let error = match block_on(pool.acquire()) {
        Ok(_) => panic!("pool unexpectedly acquired beyond max_size"),
        Err(error) => error,
    };
    assert!(matches!(error, PoolError::AcquireTimeout(_)));
    drop(held);
    assert_eq!(pool.metrics().acquisition_timeouts, 1);
    let recovered = pool.acquire_blocking().unwrap();
    drop(recovered);
    pool.shutdown().unwrap();
}

#[test]
fn idle_connections_are_reaped_without_violating_minimum() {
    let server = TestServer::start();
    let pool = pool(&server, 0, 2);
    let connection = pool.acquire_blocking().unwrap();
    drop(connection);
    thread::sleep(Duration::from_millis(40));
    let next = pool.acquire_blocking().unwrap();
    drop(next);
    assert!(pool.metrics().discarded >= 1);
    pool.shutdown().unwrap();
}

#[test]
fn async_waiters_are_fifo_and_cancellation_does_not_leak() {
    let server = TestServer::start();
    let pool = pool(&server, 0, 1);
    let held = pool.acquire_blocking().unwrap();
    let cancelled = pool.acquire();
    let mut cancelled = Box::pin(cancelled);
    let noop = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(noop);
    assert!(matches!(
        cancelled.as_mut().poll(&mut context),
        std::task::Poll::Pending
    ));
    drop(cancelled);
    thread::sleep(Duration::from_millis(10));
    assert_eq!(pool.metrics().waiters, 0);

    let order = Arc::new(Mutex::new(Vec::new()));
    let first_pool = pool.clone();
    let first_order = order.clone();
    let first = thread::spawn(move || {
        let lease = block_on(first_pool.acquire()).unwrap();
        first_order.lock().unwrap().push(1);
        thread::sleep(Duration::from_millis(20));
        drop(lease);
    });
    wait_for_waiters(&pool, 1);
    let second_pool = pool.clone();
    let second_order = order.clone();
    let second = thread::spawn(move || {
        let lease = block_on(second_pool.acquire()).unwrap();
        second_order.lock().unwrap().push(2);
        drop(lease);
    });
    wait_for_waiters(&pool, 2);
    drop(held);
    first.join().unwrap();
    second.join().unwrap();
    assert_eq!(*order.lock().unwrap(), vec![1, 2]);
    assert_eq!(pool.metrics().waiters, 0);
    pool.shutdown().unwrap();
}

fn wait_for_waiters<F: ConnectionFactory>(pool: &ConnectionPool<F>, expected: usize) {
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while pool.metrics().waiters < expected {
        assert!(
            std::time::Instant::now() < deadline,
            "waiter did not register"
        );
        thread::yield_now();
    }
}

#[test]
fn shutdown_times_out_while_a_real_lease_is_active() {
    let server = TestServer::start();
    let pool = pool(&server, 0, 1);
    let held = pool.acquire_blocking().unwrap();
    assert!(matches!(
        pool.shutdown(),
        Err(PoolError::ShutdownTimeout(_))
    ));
    drop(held);
}
