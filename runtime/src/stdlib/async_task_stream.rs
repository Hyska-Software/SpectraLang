use super::*;
pub(crate) fn async_task_registry() -> &'static Mutex<AsyncTaskRegistry> {
    static REGISTRY: OnceLock<Mutex<AsyncTaskRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(AsyncTaskRegistry::new()))
}

pub(crate) fn async_task_completion_signal() -> &'static (Mutex<u64>, Condvar) {
    static SIGNAL: OnceLock<(Mutex<u64>, Condvar)> = OnceLock::new();
    SIGNAL.get_or_init(|| (Mutex::new(0), Condvar::new()))
}

pub(crate) fn notify_async_task_completion() {
    let (epoch, ready) = async_task_completion_signal();
    let mut epoch = lock_unpoisoned(epoch);
    *epoch = epoch.wrapping_add(1);
    ready.notify_all();
}

pub(crate) fn lock_async_task_registry() -> Result<std::sync::MutexGuard<'static, AsyncTaskRegistry>, i32> {
    async_task_registry()
        .lock()
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)
}

use crate::async_frame::{
    invoke_frame_drop, AsyncAffinity, AsyncFrame, AsyncPollContext, AsyncPollOutcome,
    AsyncPollStatus, AsyncResultStorage,
};

/// Allocate an existing Async task handle and attach its private coroutine frame.
pub(crate) fn create_coroutine_task(
    frame: AsyncFrame,
    parent_scope: Option<SpectraHostValue>,
    affinity: AsyncAffinity,
) -> Result<SpectraHostValue, i32> {
    create_coroutine_task_boxed(Box::new(frame), parent_scope, affinity)
}

pub(crate) fn create_coroutine_task_boxed(
    frame: Box<AsyncFrame>,
    parent_scope: Option<SpectraHostValue>,
    affinity: AsyncAffinity,
) -> Result<SpectraHostValue, i32> {
    let mut registry = lock_async_task_registry()?;
    let task_id = registry.allocate_coroutine_task(parent_scope);
    if !registry.attach_coroutine_frame_boxed(task_id, frame, affinity) {
        let _ = registry.tasks.remove(task_id);
        return Err(HOST_STATUS_INTERNAL_ERROR);
    }
    Ok(task_id)
}

/// Poll one coroutine without holding the global task registry mutex.
pub(crate) fn poll_coroutine_task(task_id: SpectraHostValue) -> Result<AsyncPollOutcome, i32> {
    let (frames, invocation) = {
        let registry = lock_async_task_registry()?;
        if !registry.is_coroutine_task(task_id) { return Err(HOST_STATUS_NOT_FOUND); }
        let frames = registry.coroutine_frames.clone();
        let invocation = match frames.begin_poll(task_id) {
            Ok(invocation) => invocation,
            Err(AsyncPollOutcome::Ready) => return Ok(AsyncPollOutcome::Ready),
            Err(AsyncPollOutcome::Failed) => return Ok(AsyncPollOutcome::Failed),
            Err(AsyncPollOutcome::Cancelled) => return Ok(AsyncPollOutcome::Cancelled),
            Err(AsyncPollOutcome::AlreadyPolling) => return Ok(AsyncPollOutcome::AlreadyPolling),
            Err(AsyncPollOutcome::Stale) => return Err(HOST_STATUS_NOT_FOUND),
            Err(AsyncPollOutcome::AffinityRejected | AsyncPollOutcome::Pending) => return Err(HOST_STATUS_INVALID_ARGUMENT),
        };
        (frames, invocation)
    };
    let mut context = AsyncPollContext::new();
    let status = if invocation.cancel_before_poll {
        AsyncPollStatus::Cancelled
    } else {
        unsafe { AsyncPollStatus::from_abi((*invocation.frame).invoke_poll(task_id, &mut context)) }
    };
    let (outcome, drop_action) = {
        let mut registry = lock_async_task_registry()?;
        let result = frames.finish_poll(task_id, status, context)
            .map_err(|outcome| if matches!(outcome, AsyncPollOutcome::Stale) { HOST_STATUS_NOT_FOUND } else { HOST_STATUS_INTERNAL_ERROR })?;
        registry.apply_coroutine_outcome(task_id, result.0);
        result
    };
    if let Some(action) = drop_action {
        unsafe { invoke_frame_drop(action) };
        frames.complete_drop(task_id);
    }
    for wake in frames.take_wakes() { reactor::global().wake_task(wake); }
    notify_async_task_completion();
    Ok(outcome)
}

pub(crate) fn cancel_coroutine_task(task_id: SpectraHostValue) -> Result<bool, i32> {
    {
        let mut registry = lock_async_task_registry()?;
        if !registry.is_coroutine_task(task_id) { return Ok(false); }
        registry.cancel_task(task_id).ok_or(HOST_STATUS_NOT_FOUND)?;
    }
    let (frames, drop_action) = {
        let registry = lock_async_task_registry()?;
        let frames = registry.coroutine_frames.clone();
        let action = frames.take_cancel_drop(task_id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        (frames, action)
    };
    if let Some(action) = drop_action {
        unsafe { invoke_frame_drop(action) };
        frames.complete_drop(task_id);
    }
    for wake in frames.take_wakes() { reactor::global().wake_task(wake); }
    Ok(true)
}

pub(crate) fn take_coroutine_result(
    task_id: SpectraHostValue,
) -> Result<Result<AsyncResultStorage, AsyncResultStorage>, i32> {
    let registry = lock_async_task_registry()?;
    if !registry.is_coroutine_task(task_id) { return Err(HOST_STATUS_NOT_FOUND); }
    registry.take_coroutine_result(task_id).ok_or(HOST_STATUS_INVALID_ARGUMENT)
}

pub(crate) fn drop_coroutine_task(task_id: SpectraHostValue) -> Result<bool, i32> {
    let action = {
        let mut registry = lock_async_task_registry()?;
        if !registry.is_coroutine_task(task_id) { return Ok(false); }
        let action = registry.coroutine_frames.take_drop_task(task_id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        if action.is_none() { return Ok(false); }
        let cancel_handle = registry.tasks.get(task_id).map(|task| task.cancel_handle).unwrap_or(0);
        registry.tasks.remove(task_id).ok_or(HOST_STATUS_NOT_FOUND)?;
        registry.cancel_handles.remove(cancel_handle);
        action
    };
    if let Some(action) = action {
        unsafe { invoke_frame_drop(action) };
    }
    Ok(true)
}

pub(crate) fn subscribe_coroutine_child(
    parent: SpectraHostValue,
    child: SpectraHostValue,
) -> Result<bool, i32> {
    let (frames, subscribed) = {
        let registry = lock_async_task_registry()?;
        if !registry.is_coroutine_task(parent) || !registry.is_coroutine_task(child) { return Ok(false); }
        let frames = registry.coroutine_frames.clone();
        let subscribed = frames.subscribe_child(parent, child);
        (frames, subscribed)
    };
    for wake in frames.take_wakes() { reactor::global().wake_task(wake); }
    Ok(subscribed)
}

/// Subscribe a coroutine parent to either a coroutine or a scalar task.
pub(crate) fn subscribe_task_child(
    parent: SpectraHostValue,
    child: SpectraHostValue,
) -> Result<bool, i32> {
    let child_is_coroutine = {
        let registry = lock_async_task_registry()?;
        if !registry.is_coroutine_task(parent) || registry.tasks.get(child).is_none() {
            return Ok(false);
        }
        registry.is_coroutine_task(child)
    };
    if child_is_coroutine {
        subscribe_coroutine_child(parent, child)
    } else {
        let mut registry = lock_async_task_registry()?;
        Ok(registry.subscribe_scalar_child(parent, child))
    }
}

pub(crate) fn wake_coroutine_task(task_id: SpectraHostValue) -> Result<bool, i32> {
    let registry = lock_async_task_registry()?;
    if !registry.is_coroutine_task(task_id) { return Ok(false); }
    let frames = registry.coroutine_frames.clone();
    drop(registry);
    let woke = frames.wake(task_id);
    for wake in frames.take_wakes() { reactor::global().wake_task(wake); }
    Ok(woke)
}

pub(crate) fn set_coroutine_result(task_id: SpectraHostValue, value: AsyncResultStorage) -> Result<bool, i32> {
    let registry = lock_async_task_registry()?;
    if !registry.is_coroutine_task(task_id) { return Ok(false); }
    Ok(registry.coroutine_frames.set_result(task_id, value))
}

pub(crate) fn set_coroutine_error(task_id: SpectraHostValue, value: AsyncResultStorage) -> Result<bool, i32> {
    let registry = lock_async_task_registry()?;
    if !registry.is_coroutine_task(task_id) { return Ok(false); }
    Ok(registry.coroutine_frames.set_error(task_id, value))
}
/// Cooperative cancellation state shared by an external I/O task and its
/// cancellation hook. The worker must check this token at every potentially
/// blocking readiness boundary.
pub type CancellationToken = Arc<AtomicBool>;

/// Runs blocking external work off the reactor and exposes completion through
/// the existing `std.async.task.*` protocol.
pub fn spawn_background_task<F>(work: F) -> Result<SpectraHostValue, i32>
where
    F: FnOnce() -> Result<SpectraHostValue, ()> + Send + 'static,
{
    spawn_background_task_internal(work, None, background_worker_queue())
}

/// Runs blocking work on the bounded background executor and wires a
/// non-blocking cancellation hook into the normal `Task<T>` protocol.
pub fn spawn_cancellable_background_task<F, C>(
    work: F,
    cancel: C,
) -> Result<SpectraHostValue, i32>
where
    F: FnOnce() -> Result<SpectraHostValue, ()> + Send + 'static,
    C: Fn() + Send + Sync + 'static,
{
    spawn_background_task_internal(work, Some(Arc::new(cancel)), background_worker_queue())
}

/// Runs a cancellable long-lived I/O wait without consuming the general
/// blocking-work executor used by database queries and file operations.
pub fn spawn_cancellable_io_task<F, C>(
    work: F,
    cancel: C,
) -> Result<SpectraHostValue, i32>
where
    F: FnOnce() -> Result<SpectraHostValue, ()> + Send + 'static,
    C: Fn() + Send + Sync + 'static,
{
    spawn_background_task_internal(
        work,
        Some(Arc::new(cancel)),
        background_io_worker_queue(),
    )
}

/// Schedules a cancellable I/O task and gives the worker a token that is set
/// immediately when the corresponding Spectra task is cancelled.
pub fn spawn_cancellable_io_task_with_token<F>(
    work: F,
) -> Result<SpectraHostValue, i32>
where
    F: FnOnce(CancellationToken) -> Result<SpectraHostValue, ()> + Send + 'static,
{
    let token = Arc::new(AtomicBool::new(false));
    let worker_token = Arc::clone(&token);
    let cancel_token = Arc::clone(&token);
    spawn_cancellable_io_task(
        move || work(worker_token),
        move || cancel_token.store(true, Ordering::Release),
    )
}

pub(crate) fn spawn_background_task_internal<F>(
    work: F,
    cancel: Option<BackgroundCancelHook>,
    queue: &'static BackgroundWorkerQueue,
) -> Result<SpectraHostValue, i32>
where
    F: FnOnce() -> Result<SpectraHostValue, ()> + Send + 'static,
{
    let parent = tracing::current().and_then(|id| tracing::context(id).ok());
    let task_id = {
        let _lifecycle = lock_unpoisoned(background_task_lifecycle());
        let mut registry = lock_async_task_registry()?;
        let task_id =
            registry.allocate_task_with_completion(0, None, None, None, false, false);
        if let Some(cancel) = cancel {
            lock_unpoisoned(background_cancel_hooks()).insert(task_id, cancel);
        }
        task_id
    };
    let job: BackgroundJob = Box::new(move || {
        let cancelled = async_task_registry()
            .lock()
            .map(|registry| registry.task_is_cancelled(task_id))
            .unwrap_or(true);
        if !cancelled {
            let result = tracing::with_context(parent, work);
            if let Ok(mut registry) = async_task_registry().lock() {
                match result {
                    Ok(value) => {
                        let _ = registry.complete_task(task_id, value);
                    }
                    Err(()) => {
                        let _ = registry.fail_task(task_id);
                    }
                }
            }
        }
    });
    if queue.sender.try_send(job).is_err() {
        let _lifecycle = lock_unpoisoned(background_task_lifecycle());
        if let Ok(mut registry) = async_task_registry().lock() {
            lock_unpoisoned(background_cancel_hooks()).remove(&task_id);
            if let Some(task) = registry.tasks.remove(task_id) {
                registry.cancel_handles.remove(task.cancel_handle);
            }
        }
        return Err(HOST_STATUS_INTERNAL_ERROR);
    }
    Ok(task_id)
}

pub(crate) fn async_last_reactor_event() -> &'static Mutex<Option<ReactorEvent>> {
    static LAST: OnceLock<Mutex<Option<ReactorEvent>>> = OnceLock::new();
    LAST.get_or_init(|| Mutex::new(None))
}

pub(crate) fn set_async_last_reactor_event(event: Option<ReactorEvent>) -> Result<(), i32> {
    let mut last = async_last_reactor_event()
        .lock()
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    *last = event;
    Ok(())
}

pub(crate) extern "C" fn std_async_task_ready(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let task_id = registry.allocate_task(args[0], None, None, None);
    results[0] = task_id;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_task_ready_batch(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let count = args[0];
    if count <= 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let mut first_task = 0;
    for offset in 0..count {
        let task_id = registry.allocate_task_with_completion_fresh(
            args[1].saturating_add(offset),
            None,
            None,
            None,
            true,
            false,
        );
        if offset == 0 {
            first_task = task_id;
        }
    }
    reactor::global().wake_task(first_task);
    results[0] = first_task;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_task_batch_checksum(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let count = args[1];
    if count <= 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    registry.process_due_timeouts();
    let mut checksum = 0i64;
    for offset in 0..count {
        let Some(task) = registry.tasks.get(args[0].saturating_add(offset)) else {
            return HOST_STATUS_NOT_FOUND;
        };
        if task.cancelled || task.failed || !task.completed {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        checksum = checksum.wrapping_add(task.value);
    }
    results[0] = checksum;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_task_poll(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let task_id = args[0];
    let is_coroutine = match lock_async_task_registry() {
        Ok(registry) => registry.is_coroutine_task(task_id),
        Err(status) => return status,
    };
    if is_coroutine {
        return match poll_coroutine_task(task_id) {
            Ok(outcome) => {
                results[0] = match outcome {
                    AsyncPollOutcome::Ready => 1,
                    AsyncPollOutcome::Pending | AsyncPollOutcome::AlreadyPolling => 0,
                    AsyncPollOutcome::Failed | AsyncPollOutcome::Cancelled => -1,
                    AsyncPollOutcome::Stale | AsyncPollOutcome::AffinityRejected => 0,
                };
                HOST_STATUS_SUCCESS
            }
            Err(status) => status,
        };
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    registry.process_due_timeouts();
    registry.drive_pending_io_for_task(task_id);
    let Some(task) = registry.tasks.get(task_id) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = i64::from(task.completed && !task.cancelled && !task.failed);
    HOST_STATUS_SUCCESS
}

pub fn poll_task_once(task_id: SpectraHostValue) -> Result<bool, i32> {
    let is_coroutine = {
        let registry = lock_async_task_registry()?;
        registry.is_coroutine_task(task_id)
    };
    if is_coroutine {
        return Ok(matches!(poll_coroutine_task(task_id)?, AsyncPollOutcome::Ready));
    }
    let mut registry = lock_async_task_registry()?;
    registry.process_due_timeouts();
    registry.drive_pending_io_for_task(task_id);
    let Some(task) = registry.tasks.get(task_id) else {
        return Err(HOST_STATUS_NOT_FOUND);
    };
    Ok(task.completed && !task.cancelled && !task.failed)
}

/// Reads a completed task value without entering the host-call ABI.
pub fn task_result_value(task_id: SpectraHostValue) -> Result<SpectraHostValue, i32> {
    let is_coroutine = {
        let registry = lock_async_task_registry()?;
        registry.is_coroutine_task(task_id)
    };
    if is_coroutine {
        let _ = poll_coroutine_task(task_id)?;
        let registry = lock_async_task_registry()?;
        return registry
            .coroutine_frames
            .result_host_value(task_id)
            .ok_or(HOST_STATUS_INVALID_ARGUMENT);
    }
    let mut registry = lock_async_task_registry()?;
    registry.process_due_timeouts();
    registry.drive_pending_io_for_task(task_id);
    let Some(task) = registry.tasks.get(task_id) else {
        return Err(HOST_STATUS_NOT_FOUND);
    };
    if task.cancelled || task.failed || !task.completed {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    Ok(task.value)
}

/// Upper bound for one reactor park while waiting on a pending task.
///
/// Completion, failure, and cancellation all enqueue a reactor `TaskWake`
/// event (`complete_task` / `fail_task` / `cancel_task`), so the park is
/// normally interrupted immediately by the OS multiplexer waker or the
/// fallback condvar. The bound exists only as a safety net for events
/// consumed by another waiter thread and to keep driving IO/timer
/// readiness that only this thread can observe. This replaces the
/// historical fixed 10 ms spin: an idle waiter now blocks inside the
/// reactor instead of waking one hundred times per second.
pub(crate) const TASK_WAIT_PARK: Duration = Duration::from_millis(50);

/// Terminal join-status codes, mirroring `async_task_join_status`:
/// 0 = completed with a value, 1 = cancelled, 2 = failed.
pub fn wait_task_terminal_status(task_id: SpectraHostValue) -> Result<SpectraHostValue, i32> {
    let is_coroutine = {
        let registry = lock_async_task_registry()?;
        registry.is_coroutine_task(task_id)
    };
    if is_coroutine {
        loop {
            match poll_coroutine_task(task_id)? {
                AsyncPollOutcome::Ready => return Ok(0),
                AsyncPollOutcome::Failed => return Ok(2),
                AsyncPollOutcome::Cancelled => return Ok(1),
                AsyncPollOutcome::Pending | AsyncPollOutcome::AlreadyPolling => {}
                AsyncPollOutcome::Stale => return Err(HOST_STATUS_NOT_FOUND),
                AsyncPollOutcome::AffinityRejected => return Err(HOST_STATUS_INVALID_ARGUMENT),
            }
            if let Some(event) = reactor::global().poll(Some(TASK_WAIT_PARK)) {
                let mut registry = lock_async_task_registry()?;
                registry.process_reactor_event(event);
            }
        }
    }
    loop {
        {
            let mut registry = lock_async_task_registry()?;
            registry.process_due_timeouts();
            registry.drive_pending_io_for_task(task_id);
            let Some(task) = registry.tasks.get(task_id) else {
                return Err(HOST_STATUS_NOT_FOUND);
            };
            match async_task_join_status(task) {
                status @ (0 | 1 | 2) => return Ok(status),
                _ => {}
            }
        }
        if let Some(event) = reactor::global().poll(Some(TASK_WAIT_PARK)) {
            let mut registry = lock_async_task_registry()?;
            registry.process_reactor_event(event);
        }
    }
}

/// Blocks a non-event-loop caller until a task completes.
pub fn block_on_task_value(task_id: SpectraHostValue) -> Result<SpectraHostValue, i32> {
    if wait_task_terminal_status(task_id)? != 0 {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    task_result_value(task_id)
}

pub(crate) extern "C" fn std_async_task_result(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    match task_result_value(args[0]) {
        Ok(value) => { results[0] = value; HOST_STATUS_SUCCESS }
        Err(status) => status,
    }
}

pub(crate) extern "C" fn std_async_task_block_on(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    // Parks inside the reactor via `wait_task_terminal_status` instead of
    // spinning; see `TASK_WAIT_PARK` for the wakeup contract.
    match wait_task_terminal_status(args[0]) {
        Ok(0) => {}
        Ok(_) => return HOST_STATUS_INVALID_ARGUMENT,
        Err(status) => return status,
    }
    let registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(task) = registry.tasks.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = task.value;
    HOST_STATUS_SUCCESS
}
pub fn cancel_task_handle(task_id: SpectraHostValue) -> bool {
    let is_coroutine = async_task_registry()
        .lock()
        .ok()
        .map(|registry| registry.is_coroutine_task(task_id))
        .unwrap_or(false);
    if is_coroutine {
        return cancel_coroutine_task(task_id).unwrap_or(false);
    }
    async_task_registry()
        .lock()
        .ok()
        .and_then(|mut registry| registry.cancel_task(task_id))
        .is_some()
}

/// Blocks the caller until the task reaches a terminal state and reports
/// which one: 0 = completed with a value, 1 = cancelled, 2 = failed.
/// This is the efficient readiness wait used by the lowered `await`
/// expression (`spectra.async.task.wait`).
pub(crate) extern "C" fn std_async_task_wait(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    results[0] = match wait_task_terminal_status(args[0]) {
        Ok(status) => status,
        Err(status) => return status,
    };
    HOST_STATUS_SUCCESS
}

pub(crate) fn async_task_join_status(task: &AsyncTask) -> SpectraHostValue {
    if task.cancelled {
        1
    } else if task.failed {
        2
    } else if !task.completed {
        3
    } else {
        0
    }
}

pub(crate) extern "C" fn std_async_task_join(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let is_coroutine = match lock_async_task_registry() {
        Ok(registry) => registry.is_coroutine_task(args[0]),
        Err(status) => return status,
    };
    if is_coroutine {
        let outcome = match poll_coroutine_task(args[0]) {
            Ok(outcome) => outcome,
            Err(status) => return status,
        };
        results[0] = match outcome {
            AsyncPollOutcome::Ready => task_result_value(args[0]).unwrap_or(-3),
            AsyncPollOutcome::Cancelled => -1,
            AsyncPollOutcome::Failed => -2,
            AsyncPollOutcome::Pending | AsyncPollOutcome::AlreadyPolling => -3,
            AsyncPollOutcome::Stale | AsyncPollOutcome::AffinityRejected => return HOST_STATUS_NOT_FOUND,
        };
        return HOST_STATUS_SUCCESS;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    registry.process_due_timeouts();
    registry.drive_pending_io_for_task(args[0]);
    let Some(task) = registry.tasks.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = match async_task_join_status(task) {
        0 => task.value,
        1 => -1,
        2 => -2,
        _ => -3,
    };
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_task_join_status(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let is_coroutine = match lock_async_task_registry() {
        Ok(registry) => registry.is_coroutine_task(args[0]),
        Err(status) => return status,
    };
    if is_coroutine {
        results[0] = match poll_coroutine_task(args[0]) {
            Ok(AsyncPollOutcome::Ready) => 0,
            Ok(AsyncPollOutcome::Cancelled) => 1,
            Ok(AsyncPollOutcome::Failed) => 2,
            Ok(AsyncPollOutcome::Pending | AsyncPollOutcome::AlreadyPolling) => 3,
            Ok(AsyncPollOutcome::Stale) => return HOST_STATUS_NOT_FOUND,
            Ok(AsyncPollOutcome::AffinityRejected) => return HOST_STATUS_INVALID_ARGUMENT,
            Err(status) => return status,
        };
        return HOST_STATUS_SUCCESS;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    registry.process_due_timeouts();
    registry.drive_pending_io_for_task(args[0]);
    let Some(task) = registry.tasks.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = async_task_join_status(task);
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_task_cancel(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let is_coroutine = match lock_async_task_registry() {
        Ok(registry) => registry.is_coroutine_task(args[0]),
        Err(status) => return status,
    };
    if is_coroutine {
        return match cancel_coroutine_task(args[0]) {
            Ok(true) => { results[0] = 1; HOST_STATUS_SUCCESS }
            Ok(false) => HOST_STATUS_NOT_FOUND,
            Err(status) => status,
        };
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    if registry.cancel_task(args[0]).is_none() {
        return HOST_STATUS_NOT_FOUND;
    }
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_task_is_cancelled(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    registry.process_due_timeouts();
    let Some(task) = registry.tasks.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = i64::from(task.cancelled);
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_task_cancel_handle(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(task) = registry.tasks.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = task.cancel_handle;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_task_with_timeout(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if args[1] < 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(inner) = registry.tasks.get(args[0]).copied() else {
        return HOST_STATUS_NOT_FOUND;
    };
    let deadline = registry.now_ms.saturating_add(args[1]);
    let wrapper = registry.allocate_task(
        inner.value,
        inner.parent_scope,
        Some(args[0]),
        Some(deadline),
    );
    reactor::global().register_timer(wrapper, Duration::from_millis(args[1] as u64));
    registry.process_due_timeouts();
    results[0] = wrapper;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_task_fail(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(task) = registry.tasks.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    task.failed = true;
    reactor::global().wake_task(args[0]);
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_task_join_order(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(task) = registry.tasks.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = task.join_order.unwrap_or(0);
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_cancel_handle_cancel(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(task_id) = registry.cancel_handles.get(args[0]).copied() else {
        return HOST_STATUS_NOT_FOUND;
    };
    if registry.cancel_task(task_id).is_none() {
        return HOST_STATUS_NOT_FOUND;
    }
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_scheduler_advance_time(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if args[0] < 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    registry.now_ms = registry.now_ms.saturating_add(args[0]);
    registry.process_due_timeouts();
    results[0] = registry.now_ms;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_scope_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let (_, results) = match host_call_args(ctx, 0) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(scope) = registry.create_scope(None) else {
        return HOST_STATUS_INTERNAL_ERROR;
    };
    results[0] = scope;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_scope_child(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(scope) = registry.create_scope(Some(args[0])) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = scope;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_scope_attach(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    if registry.attach_task_to_scope(args[0], args[1]).is_none() {
        return HOST_STATUS_NOT_FOUND;
    }
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_scope_spawn_ready(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    if !registry.scopes.contains_key(args[0]) {
        return HOST_STATUS_NOT_FOUND;
    }
    results[0] = registry.allocate_task(args[1], Some(args[0]), None, None);
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_scope_cancel(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    if registry.cancel_scope(args[0]).is_none() {
        return HOST_STATUS_NOT_FOUND;
    }
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_scope_join(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(status) = registry.join_scope(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = status;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_scope_joined_count(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(scope) = registry.scopes.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = scope.joined_count;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_scope_failures(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(scope) = registry.scopes.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = scope.failures;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_stream_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if args[0] <= 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    results[0] = registry.create_stream(AsyncStreamKind::Source, args[0] as usize);
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_stream_push(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(status) = registry.push_stream_value(args[0], args[1]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = status;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_stream_done(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    if registry.mark_stream_done(args[0]).is_none() {
        return HOST_STATUS_NOT_FOUND;
    }
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_stream_next(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(pulled) = registry.pull_stream_value(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    let (task, status) = match pulled {
        AsyncStreamPull::Pending => {
            let task = registry.allocate_task_with_completion(0, None, None, None, false, true);
            if let Some(stream) = registry.streams.get_mut(args[0]) {
                stream.pending_next.push_back(task);
            }
            (task, 0)
        }
        AsyncStreamPull::Item(value) => (registry.allocate_task(value, None, None, None), 1),
        AsyncStreamPull::Done => (registry.allocate_task(-1, None, None, None), 2),
        AsyncStreamPull::Failed => {
            let task = registry.allocate_task(-2, None, None, None);
            if let Some(task_state) = registry.tasks.get_mut(task) {
                task_state.failed = true;
            }
            (task, 3)
        }
        AsyncStreamPull::Cancelled => {
            let task = registry.allocate_task(-3, None, None, None);
            let _ = registry.cancel_task(task);
            (task, 4)
        }
    };
    if let Some(stream) = registry.streams.get_mut(args[0]) {
        stream.last_next_status = status;
    }
    results[0] = task;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_stream_next_status(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(stream) = registry.streams.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = stream.last_next_status;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_stream_cancel(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    if registry.cancel_stream(args[0]).is_none() {
        return HOST_STATUS_NOT_FOUND;
    }
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_stream_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(stream) = registry.streams.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = stream.buffer.len() as SpectraHostValue;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_stream_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(stream) = registry.streams.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = stream.capacity as SpectraHostValue;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_stream_map(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 3) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if map_stream_value(1, args[1], args[2]).is_none() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    if !registry.streams.contains_key(args[0]) {
        return HOST_STATUS_NOT_FOUND;
    }
    results[0] = registry.create_stream(
        AsyncStreamKind::Map {
            upstream: args[0],
            op: args[1],
            arg: args[2],
        },
        1,
    );
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_stream_filter(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 3) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if filter_stream_value(1, args[1], args[2]).is_none() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    if !registry.streams.contains_key(args[0]) {
        return HOST_STATUS_NOT_FOUND;
    }
    results[0] = registry.create_stream(
        AsyncStreamKind::Filter {
            upstream: args[0],
            predicate: args[1],
            arg: args[2],
        },
        1,
    );
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_stream_fold(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 3) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if fold_stream_value(args[1], 1, args[2]).is_none() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    if !registry.streams.contains_key(args[0]) {
        return HOST_STATUS_NOT_FOUND;
    }
    let mut accumulator = args[1];
    loop {
        match registry.pull_stream_value(args[0]) {
            Some(AsyncStreamPull::Item(value)) => {
                let Some(next) = fold_stream_value(accumulator, value, args[2]) else {
                    return HOST_STATUS_INVALID_ARGUMENT;
                };
                accumulator = next;
            }
            Some(AsyncStreamPull::Done) => {
                results[0] = registry.allocate_task(accumulator, None, None, None);
                return HOST_STATUS_SUCCESS;
            }
            Some(AsyncStreamPull::Pending) => {
                results[0] = registry.allocate_task_with_completion(
                    accumulator,
                    None,
                    None,
                    None,
                    false,
                    true,
                );
                return HOST_STATUS_SUCCESS;
            }
            Some(AsyncStreamPull::Cancelled) => {
                let task = registry.allocate_task(-3, None, None, None);
                let _ = registry.cancel_task(task);
                results[0] = task;
                return HOST_STATUS_SUCCESS;
            }
            Some(AsyncStreamPull::Failed) => {
                let task = registry.allocate_task(-2, None, None, None);
                if let Some(task_state) = registry.tasks.get_mut(task) {
                    task_state.failed = true;
                }
                results[0] = task;
                return HOST_STATUS_SUCCESS;
            }
            None => return HOST_STATUS_NOT_FOUND,
        }
    }
}

pub(crate) extern "C" fn std_async_stream_take(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if args[1] < 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    if !registry.streams.contains_key(args[0]) {
        return HOST_STATUS_NOT_FOUND;
    }
    results[0] = registry.create_stream(
        AsyncStreamKind::Take {
            upstream: args[0],
            remaining: args[1],
        },
        1,
    );
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_stream_skip(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if args[1] < 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    if !registry.streams.contains_key(args[0]) {
        return HOST_STATUS_NOT_FOUND;
    }
    results[0] = registry.create_stream(
        AsyncStreamKind::Skip {
            upstream: args[0],
            remaining: args[1],
        },
        1,
    );
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_stream_chunks(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    if args[1] <= 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    if !registry.streams.contains_key(args[0]) {
        return HOST_STATUS_NOT_FOUND;
    }
    results[0] = registry.create_stream(
        AsyncStreamKind::Chunks {
            upstream: args[0],
            size: args[1],
        },
        1,
    );
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_async_stream_fuse(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_async_task_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    if !registry.streams.contains_key(args[0]) {
        return HOST_STATUS_NOT_FOUND;
    }
    results[0] = registry.create_stream(
        AsyncStreamKind::Fuse {
            upstream: args[0],
            fused_done: false,
        },
        1,
    );
    HOST_STATUS_SUCCESS
}

#[cfg(test)]
mod coroutine_tests {
    use crate::async_frame::{
        AsyncAffinity, AsyncFrame, AsyncOwnedValue, AsyncPollContext, AsyncPollOutcome,
        AsyncResultStorage, AsyncTaskState,
    };
    use super::{
        async_task_registry, cancel_coroutine_task, create_coroutine_task, drop_coroutine_task,
        lock_async_task_registry, poll_coroutine_task, take_coroutine_result, task_result_value,
    };
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    static REENTERED: AtomicBool = AtomicBool::new(false);
    static POLLS: AtomicUsize = AtomicUsize::new(0);
    static DROPS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn reenter_poll(
        _: i64,
        task: i64,
        context: i64,
    ) -> i64 {
        REENTERED.store(
            async_task_registry()
                .try_lock()
                .map(|registry| registry.coroutine_state(task) == Some(AsyncTaskState::Polling))
                .unwrap_or(false),
            Ordering::SeqCst,
        );
        if POLLS.fetch_add(1, Ordering::SeqCst) == 0 {
            (*(context as *mut AsyncPollContext)).wake_parent();
            0
        } else {
            (*(context as *mut AsyncPollContext)).set_result(AsyncResultStorage::scalar(task.saturating_add(1)));
            1
        }
    }

    unsafe extern "C" fn string_poll(
        _: i64,
        _: i64,
        context: i64,
    ) -> i64 {
        (*(context as *mut AsyncPollContext)).set_result(AsyncResultStorage::string("owned"));
        1
    }
    unsafe extern "C" fn aggregate_poll(
        _: i64,
        _: i64,
        context: i64,
    ) -> i64 {
        (*(context as *mut AsyncPollContext)).set_result(AsyncResultStorage::aggregate(vec![11, 22]));
        1
    }

    unsafe extern "C" fn cancel_during_poll(
        _: i64,
        task: i64,
        _: i64,
    ) -> i64 {
        let _ = cancel_coroutine_task(task);
        1
    }

    unsafe extern "C" fn count_drop(_: i64, _: i64, _: i64) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }

    fn reset() {
        let mut registry = lock_async_task_registry().expect("registry");
        registry.clear();
        POLLS.store(0, Ordering::SeqCst);
        REENTERED.store(false, Ordering::SeqCst);
        DROPS.store(0, Ordering::SeqCst);
    }

    #[test]
    fn generated_poll_reenters_registry_and_wakes_without_recursion() {
        let _guard = crate::runtime_test_guard();
        reset();
        let task = create_coroutine_task(
            AsyncFrame::new(Vec::new(), reenter_poll, count_drop),
            None,
            AsyncAffinity::Any,
        ).expect("task");
        assert_eq!(poll_coroutine_task(task), Ok(AsyncPollOutcome::Pending));
        assert!(REENTERED.load(Ordering::SeqCst));
        assert_eq!(poll_coroutine_task(task), Ok(AsyncPollOutcome::Ready));
        assert_eq!(task_result_value(task), Ok(task.saturating_add(1)));
        assert_eq!(DROPS.load(Ordering::SeqCst), 1);
        assert!(drop_coroutine_task(task).expect("drop"));
    }

    #[test]
    fn cancellation_during_poll_defers_drop_until_callback_returns() {
        let _guard = crate::runtime_test_guard();
        reset();
        let task = create_coroutine_task(
            AsyncFrame::new(Vec::new(), cancel_during_poll, count_drop),
            None,
            AsyncAffinity::Any,
        ).expect("task");
        assert_eq!(poll_coroutine_task(task), Ok(AsyncPollOutcome::Cancelled));
        assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn typed_coroutine_result_is_taken_as_owned_storage() {
        let _guard = crate::runtime_test_guard();
        reset();
        let task = create_coroutine_task(
            AsyncFrame::new(Vec::new(), string_poll, count_drop),
            None,
            AsyncAffinity::Any,
        ).expect("task");
        assert_eq!(poll_coroutine_task(task), Ok(AsyncPollOutcome::Ready));
        let result = take_coroutine_result(task).expect("result").expect("ready");
        assert!(matches!(result.as_value(), AsyncOwnedValue::String(value) if value == "owned"));
        assert!(drop_coroutine_task(task).expect("drop"));

        let aggregate_task = create_coroutine_task(
            AsyncFrame::new(Vec::new(), aggregate_poll, count_drop),
            None,
            AsyncAffinity::Any,
        ).expect("aggregate task");
        assert_eq!(poll_coroutine_task(aggregate_task), Ok(AsyncPollOutcome::Ready));
        let result = take_coroutine_result(aggregate_task).expect("result").expect("ready");
        assert!(matches!(result.as_value(), AsyncOwnedValue::Aggregate(value) if value == &vec![11, 22]));
        assert!(drop_coroutine_task(aggregate_task).expect("drop"));
    }
}
