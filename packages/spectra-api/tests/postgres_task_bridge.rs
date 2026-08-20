use spectra_runtime::ffi::{
    lookup_host_function, spectra_rt_manual_clear, SpectraHostCallContext, SpectraHostValue,
    HOST_STATUS_INVALID_ARGUMENT, HOST_STATUS_SUCCESS,
};
use spectra_runtime::tracing;
use std::env;
use std::time::{Duration, Instant};

fn call(name: &str, args: &[SpectraHostValue]) -> (i32, SpectraHostValue) {
    let function = lookup_host_function(name).unwrap_or_else(|| panic!("missing host call {name}"));
    let mut result = [0_i64];
    let mut context = SpectraHostCallContext {
        args: args.as_ptr(),
        arg_len: args.len(),
        results: result.as_mut_ptr(),
        result_len: result.len(),
        invoke_fn: None,
    };
    let status = function(&mut context);
    (status, result[0])
}

fn runtime_string(value: &str) -> SpectraHostValue {
    unsafe { tracing::alloc_string(value) }
}

#[test]
#[ignore = "requires PostgreSQL 16"]
fn public_postgres_task_cancel_is_non_blocking_and_connection_is_reusable() {
    let url = env::var("SPECTRA_POSTGRES_URL").expect("SPECTRA_POSTGRES_URL");
    spectra_api::register();

    let (status, connection) = call("spectra.api.db.postgres.open", &[runtime_string(&url)]);
    assert_eq!(status, HOST_STATUS_SUCCESS);
    let (status, unrelated_connection) =
        call("spectra.api.db.postgres.open", &[runtime_string(&url)]);
    assert_eq!(status, HOST_STATUS_SUCCESS);
    let (status, statement) = call(
        "spectra.api.db.postgres.prepare",
        &[connection, runtime_string("SELECT pg_sleep(10)::TEXT")],
    );
    assert_eq!(status, HOST_STATUS_SUCCESS);

    let dispatched_at = Instant::now();
    let (status, task) = call("spectra.api.db.postgres.step_async", &[statement]);
    assert_eq!(status, HOST_STATUS_SUCCESS);
    assert!(
        dispatched_at.elapsed() < Duration::from_millis(50),
        "public async dispatch blocked for {:?}",
        dispatched_at.elapsed()
    );
    std::thread::sleep(Duration::from_millis(150));

    let unrelated_at = Instant::now();
    let (status, unrelated_statement) = call(
        "spectra.api.db.postgres.prepare",
        &[unrelated_connection, runtime_string("SELECT 2::BIGINT")],
    );
    assert_eq!(status, HOST_STATUS_SUCCESS);
    assert!(
        unrelated_at.elapsed() < Duration::from_millis(250),
        "a running statement blocked unrelated handle dispatch for {:?}",
        unrelated_at.elapsed()
    );
    assert_eq!(
        call("spectra.api.db.postgres.step", &[unrelated_statement]),
        (HOST_STATUS_SUCCESS, 1)
    );
    assert_eq!(
        call("spectra.api.db.postgres.finalize", &[unrelated_statement]),
        (HOST_STATUS_SUCCESS, 1)
    );

    let cancel_at = Instant::now();
    assert_eq!(
        call("spectra.async.task.cancel", &[task]),
        (HOST_STATUS_SUCCESS, 1)
    );
    assert!(
        cancel_at.elapsed() < Duration::from_millis(50),
        "public cancellation blocked the task registry for {:?}",
        cancel_at.elapsed()
    );
    assert_eq!(
        call("spectra.async.task.block_on", &[task]).0,
        HOST_STATUS_INVALID_ARGUMENT
    );

    let reuse_at = Instant::now();
    let (status, reusable) = call(
        "spectra.api.db.postgres.prepare",
        &[connection, runtime_string("SELECT 1::BIGINT")],
    );
    assert_eq!(status, HOST_STATUS_SUCCESS);
    assert_eq!(
        call("spectra.api.db.postgres.step", &[reusable]),
        (HOST_STATUS_SUCCESS, 1)
    );
    assert!(
        reuse_at.elapsed() < Duration::from_secs(3),
        "connection did not become reusable after cancellation"
    );
    assert_eq!(
        call("spectra.api.db.postgres.finalize", &[reusable]),
        (HOST_STATUS_SUCCESS, 1)
    );

    let (status, channel) = call(
        "spectra.api.db.postgres.listen",
        &[connection, runtime_string("spectra_r2505_cancel_listener")],
    );
    assert_eq!(status, HOST_STATUS_SUCCESS);
    let (status, notification_task) = call(
        "spectra.api.db.postgres.notification_next_async",
        &[channel, 5_000],
    );
    assert_eq!(status, HOST_STATUS_SUCCESS);
    std::thread::sleep(Duration::from_millis(100));
    let notification_cancel_at = Instant::now();
    assert_eq!(
        call("spectra.async.task.cancel", &[notification_task]),
        (HOST_STATUS_SUCCESS, 1)
    );
    assert!(
        notification_cancel_at.elapsed() < Duration::from_millis(50),
        "notification cancellation blocked for {:?}",
        notification_cancel_at.elapsed()
    );
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        call("spectra.api.db.postgres.notification_close", &[channel]),
        (HOST_STATUS_SUCCESS, 1)
    );

    assert_eq!(
        call("spectra.api.db.postgres.finalize", &[statement]),
        (HOST_STATUS_SUCCESS, 1)
    );
    assert_eq!(
        call("spectra.api.db.postgres.close", &[connection]),
        (HOST_STATUS_SUCCESS, 1)
    );
    assert_eq!(
        call("spectra.api.db.postgres.close", &[unrelated_connection]),
        (HOST_STATUS_SUCCESS, 1)
    );
    spectra_rt_manual_clear();
}
