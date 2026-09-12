use spectra_db::sqlite::{
    ColumnType, SqliteConnection, SqliteExecuteFuture, SqliteFactory, SqliteStatement,
    SqliteTransaction, SqliteValue, StepResult,
};
use spectra_db::{ConnectionPool, PoolConfig};
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Same coarse-tick collision-proofing as migrations_integration.rs.
static DATABASE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn database_path(name: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = DATABASE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "spectralang-{name}-{nonce}-{}-{sequence}.sqlite",
        std::process::id()
    ))
}

fn connection(name: &str) -> (SqliteConnection, std::path::PathBuf) {
    let path = database_path(name);
    (
        SqliteConnection::open(&path, Duration::from_secs(1)).unwrap(),
        path,
    )
}

fn block_on<F: Future>(future: F) -> F::Output {
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
fn file_backed_crud_and_typed_prepared_values() {
    let (connection, path) = connection("crud");
    connection.execute_batch("CREATE TABLE items(id INTEGER PRIMARY KEY, name TEXT NOT NULL, score REAL, payload BLOB, note TEXT)").unwrap();
    let mut insert = SqliteStatement::prepare(
        connection.clone(),
        "INSERT INTO items(id,name,score,payload,note) VALUES(?1,?2,?3,?4,?5)",
    )
    .unwrap();
    insert.bind(0, SqliteValue::Integer(7)).unwrap();
    insert.bind(1, SqliteValue::Text("spectra".into())).unwrap();
    insert.bind(2, SqliteValue::Real(1.25)).unwrap();
    insert.bind(3, SqliteValue::Blob(vec![1, 2, 3])).unwrap();
    insert.bind(4, SqliteValue::Null).unwrap();
    assert_eq!(insert.step().unwrap(), StepResult::Done);
    assert_eq!(insert.affected_rows().unwrap(), 1);
    let mut query = SqliteStatement::prepare(
        connection.clone(),
        "SELECT id,name,score,payload,note FROM items WHERE id=?1",
    )
    .unwrap();
    query.bind(0, SqliteValue::Integer(7)).unwrap();
    assert_eq!(query.step().unwrap(), StepResult::Row);
    assert_eq!(query.column_type(0).unwrap(), ColumnType::Integer);
    assert_eq!(
        query.column_value(1).unwrap(),
        SqliteValue::Text("spectra".into())
    );
    assert_eq!(query.column_value(2).unwrap(), SqliteValue::Real(1.25));
    assert_eq!(
        query.column_value(3).unwrap(),
        SqliteValue::Blob(vec![1, 2, 3])
    );
    assert_eq!(query.column_type(4).unwrap(), ColumnType::Null);
    assert_eq!(query.step().unwrap(), StepResult::Done);
    let _ = std::fs::remove_file(path);
}

#[test]
fn transaction_commit_rollback_and_drop_rollback() {
    let (connection, path) = connection("transactions");
    connection
        .execute_batch("CREATE TABLE values_table(value INTEGER)")
        .unwrap();
    let transaction = SqliteTransaction::begin(connection.clone()).unwrap();
    let mut insert = SqliteStatement::prepare(
        connection.clone(),
        "INSERT INTO values_table(value) VALUES(?1)",
    )
    .unwrap();
    insert.bind(0, SqliteValue::Integer(1)).unwrap();
    insert.step().unwrap();
    transaction.rollback().unwrap();
    let transaction = SqliteTransaction::begin(connection.clone()).unwrap();
    let mut insert = SqliteStatement::prepare(
        connection.clone(),
        "INSERT INTO values_table(value) VALUES(?1)",
    )
    .unwrap();
    insert.bind(0, SqliteValue::Integer(2)).unwrap();
    insert.step().unwrap();
    transaction.commit().unwrap();
    let transaction = SqliteTransaction::begin(connection.clone()).unwrap();
    drop(transaction);
    let mut count =
        SqliteStatement::prepare(connection, "SELECT COUNT(*) FROM values_table").unwrap();
    assert_eq!(count.step().unwrap(), StepResult::Row);
    assert_eq!(count.column_value(0).unwrap(), SqliteValue::Integer(1));
    let _ = std::fs::remove_file(path);
}

#[test]
fn reset_is_required_before_rebinding_and_finalize_is_terminal() {
    let (connection, path) = connection("state");
    let mut statement = SqliteStatement::prepare(connection, "SELECT ?1").unwrap();
    statement.bind(0, SqliteValue::Integer(1)).unwrap();
    statement.step().unwrap();
    assert!(statement.bind(0, SqliteValue::Integer(2)).is_err());
    statement.reset().unwrap();
    statement.bind(0, SqliteValue::Integer(2)).unwrap();
    statement.finalize().unwrap();
    assert!(statement.step().is_err());
    let _ = std::fs::remove_file(path);
}

#[test]
fn sqlite_factory_consumes_connection_pool() {
    let path = database_path("pool");
    let factory = SqliteFactory::new(&path);
    let pool = Arc::new(
        ConnectionPool::new(
            factory,
            PoolConfig {
                min_size: 1,
                max_size: 2,
                ..PoolConfig::default()
            },
        )
        .unwrap(),
    );
    let connection = pool.acquire_blocking().unwrap();
    connection
        .connection()
        .unwrap()
        .execute_batch("CREATE TABLE pool_table(value INTEGER)")
        .unwrap();
    drop(connection);
    let async_connection = block_on(pool.acquire()).unwrap();
    async_connection
        .connection()
        .unwrap()
        .execute_batch("INSERT INTO pool_table(value) VALUES(1)")
        .unwrap();
    drop(async_connection);
    let affected = block_on(SqliteExecuteFuture::new(
        pool.clone(),
        "INSERT INTO pool_table(value) VALUES(2)",
    ))
    .unwrap();
    assert_eq!(affected, 1);
    pool.shutdown().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn concurrent_file_backed_reads_are_isolated() {
    let path = database_path("concurrent");
    let seed = SqliteConnection::open(&path, Duration::from_secs(1)).unwrap();
    seed.execute_batch(
        "CREATE TABLE values_table(value INTEGER); INSERT INTO values_table VALUES(1),(2),(3)",
    )
    .unwrap();
    let factory = SqliteFactory::new(&path);
    let pool = Arc::new(
        ConnectionPool::new(
            factory,
            PoolConfig {
                min_size: 2,
                max_size: 4,
                ..PoolConfig::default()
            },
        )
        .unwrap(),
    );
    let handles = (0..4)
        .map(|_| {
            let pool = pool.clone();
            thread::spawn(move || {
                let connection = pool.acquire_blocking().unwrap();
                let mut query = SqliteStatement::prepare(
                    connection.connection().unwrap().clone(),
                    "SELECT COUNT(*) FROM values_table",
                )
                .unwrap();
                query.step().unwrap();
                assert_eq!(query.column_value(0).unwrap(), SqliteValue::Integer(3));
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.join().unwrap();
    }
    pool.shutdown().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn cancelled_async_operation_does_not_execute_after_waiter_cancellation() {
    let path = database_path("cancel");
    let seed = SqliteConnection::open(&path, Duration::from_secs(1)).unwrap();
    seed.execute_batch("CREATE TABLE values_table(value INTEGER)")
        .unwrap();
    let pool = Arc::new(
        ConnectionPool::new(
            SqliteFactory::new(&path),
            PoolConfig {
                min_size: 0,
                max_size: 1,
                acquisition_timeout: Duration::from_millis(500),
                ..PoolConfig::default()
            },
        )
        .unwrap(),
    );
    let held = pool.acquire_blocking().unwrap();
    let mut future = Box::pin(SqliteExecuteFuture::new(
        pool.clone(),
        "INSERT INTO values_table(value) VALUES(99)",
    ));
    fn noop(_: *const ()) {}
    fn clone(_: *const ()) -> std::task::RawWaker {
        std::task::RawWaker::new(std::ptr::null(), &VTABLE)
    }
    static VTABLE: std::task::RawWakerVTable =
        std::task::RawWakerVTable::new(clone, noop, noop, noop);
    let waker =
        unsafe { std::task::Waker::from_raw(std::task::RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut context = std::task::Context::from_waker(&waker);
    assert!(matches!(
        future.as_mut().poll(&mut context),
        std::task::Poll::Pending
    ));
    drop(future);
    drop(held);
    thread::sleep(Duration::from_millis(100));
    let mut query =
        SqliteStatement::prepare(seed.clone(), "SELECT COUNT(*) FROM values_table").unwrap();
    assert_eq!(query.step().unwrap(), StepResult::Row);
    assert_eq!(query.column_value(0).unwrap(), SqliteValue::Integer(0));
    pool.shutdown().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn async_sqlite_lock_wait_runs_off_reactor_thread() {
    let path = database_path("async-lock");
    let blocker = SqliteConnection::open(&path, Duration::from_secs(1)).unwrap();
    blocker
        .execute_batch("CREATE TABLE values_table(value INTEGER)")
        .unwrap();
    blocker.begin().unwrap();
    let mut held_insert =
        SqliteStatement::prepare(blocker.clone(), "INSERT INTO values_table(value) VALUES(1)")
            .unwrap();
    held_insert.step().unwrap();

    let pool = Arc::new(
        ConnectionPool::new(
            SqliteFactory::new(&path).with_busy_timeout(Duration::from_millis(250)),
            PoolConfig {
                max_size: 1,
                acquisition_timeout: Duration::from_secs(1),
                connection_timeout: Duration::from_secs(1),
                ..PoolConfig::default()
            },
        )
        .unwrap(),
    );
    let ticks = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let tick_count = ticks.clone();
    let stop_flag = stop.clone();
    let timer = thread::spawn(move || {
        while !stop_flag.load(Ordering::Acquire) {
            tick_count.fetch_add(1, Ordering::Relaxed);
            thread::sleep(Duration::from_millis(5));
        }
    });
    let result = block_on(SqliteExecuteFuture::new(
        pool.clone(),
        "INSERT INTO values_table(value) VALUES(2)",
    ));
    stop.store(true, Ordering::Release);
    timer.join().unwrap();
    assert!(result.is_err());
    assert!(
        ticks.load(Ordering::Acquire) >= 5,
        "timer did not progress while SQLite was locked"
    );
    blocker.rollback().unwrap();
    pool.shutdown().unwrap();
    let _ = std::fs::remove_file(path);
}
