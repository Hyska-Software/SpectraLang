/// Fast-path helper for `concurrent.task_spawn(value)` called from JIT code
/// via the `spectra_rt_concurrent_spawn_fast` fast ABI entry. Bypasses the
/// generic host-call dispatcher (no manual_alloc/free, no name lookup, no
/// catch_unwind, no host_registry lock). Returns the task_id, or 0 on
/// internal error (poisoned mutex).
pub fn concurrent_spawn_fast(value: SpectraHostValue) -> SpectraHostValue {
    if let Some(data) = concurrent_diagnostics() {
        data.spawn_fast_abi_calls.fetch_add(1, Ordering::Relaxed);
    }
    spawn_concurrent_task(value).unwrap_or(0)
}

/// Fast-path helper for `concurrent.task_join(task_id)`. Returns the value
/// written by the matching `task_spawn`, or 0 if the task_id is invalid
/// (out of range, recycled, or never existed).
pub fn concurrent_join_fast(task_id: SpectraHostValue) -> SpectraHostValue {
    if let Some(data) = concurrent_diagnostics() {
        data.join_fast_abi_calls.fetch_add(1, Ordering::Relaxed);
    }
    join_concurrent_task(task_id).unwrap_or_default()
}

/// Fast ABI for real-concurrency fan-out over a contiguous integer range.
pub fn concurrent_spawn_batch_fast(
    first_value: SpectraHostValue,
    count: SpectraHostValue,
) -> SpectraHostValue {
    if let Some(data) = concurrent_diagnostics() {
        data.batch_spawn_fast_abi_calls.fetch_add(1, Ordering::Relaxed);
    }
    spawn_concurrent_batch(first_value, count).unwrap_or(0)
}

/// Fast ABI for joining every task owned by a batch and summing results.
pub fn concurrent_join_batch_sum_fast(batch_id: SpectraHostValue) -> SpectraHostValue {
    if let Some(data) = concurrent_diagnostics() {
        data.batch_join_fast_abi_calls.fetch_add(1, Ordering::Relaxed);
    }
    join_concurrent_batch_sum(batch_id).unwrap_or(0)
}

/// Fast-path helper for an immediately paired spawn and join.
pub fn concurrent_spawn_join_fast(value: SpectraHostValue) -> SpectraHostValue {
    if let Some(data) = concurrent_diagnostics() {
        data.fused_fast_abi_calls.fetch_add(1, Ordering::Relaxed);
        data.tasks_counted.fetch_add(1, Ordering::Relaxed);
    }
    let mut registry = match lock_concurrent_registry() {
        Ok(r) => r,
        Err(_) => return 0,
    };
    registry.tasks_spawned += 1;
    value
}

/// Fast-path helper for `concurrent.reset()`.
pub fn concurrent_reset_fast() -> SpectraHostValue {
    if let Some(data) = concurrent_diagnostics() {
        data.reset_fast_abi_calls.fetch_add(1, Ordering::Relaxed);
    }
    let mut registry = match lock_concurrent_registry() {
        Ok(r) => r,
        Err(_) => return 0,
    };
    registry.clear();
    1
}

/// Fast-path helper for `concurrent.channel_new()`. Returns the new channel
/// id, or 0 if the registry mutex is poisoned.
pub fn concurrent_channel_new_fast() -> SpectraHostValue {
    let mut registry = match lock_concurrent_registry() {
        Ok(r) => r,
        Err(_) => return 0,
    };
    
    registry.channels.insert(Arc::new(Mutex::new(ConcurrentChannel {
        queue: VecDeque::with_capacity(CONCURRENT_CHANNEL_INITIAL_CAPACITY),
        closed: false,
    })))
}

/// Fast-path helper for `concurrent.channel_send(channel, value)`.
///
/// Returns 1 on success, 0 if the channel is closed, 0 if the channel id
/// is invalid (NOT_FOUND propagated as 0 for Fast ABI).
pub fn concurrent_channel_send_fast(channel: SpectraHostValue, value: SpectraHostValue) -> i32 {
    let arc = match lock_concurrent_registry() {
        Ok(r) => r.channels.get(channel).cloned(),
        Err(_) => return HOST_STATUS_INTERNAL_ERROR,
    };
    let Some(arc) = arc else {
        return HOST_STATUS_NOT_FOUND;
    };
    let mut ch = lock_unpoisoned(&arc);
    if ch.closed {
        return HOST_STATUS_SUCCESS;
    }
    ch.queue.push_back(value);
    1
}

/// Fast-path helper for `concurrent.channel_recv(channel)`. Returns the next
/// value in the channel, or -1 if the channel is empty / closed.
pub fn concurrent_channel_recv_fast(channel: SpectraHostValue) -> i64 {
    let arc = match lock_concurrent_registry() {
        Ok(r) => r.channels.get(channel).cloned(),
        Err(_) => return -1,
    };
    let Some(arc) = arc else {
        return -1;
    };
    let mut ch = lock_unpoisoned(&arc);
    ch.queue.pop_front().unwrap_or(-1)
}

/// Fast-path helper for `concurrent.channel_close(channel)`.
pub fn concurrent_channel_close_fast(channel: SpectraHostValue) -> i32 {
    let arc = match lock_concurrent_registry() {
        Ok(r) => r.channels.get(channel).cloned(),
        Err(_) => return HOST_STATUS_INTERNAL_ERROR,
    };
    let Some(arc) = arc else {
        return HOST_STATUS_NOT_FOUND;
    };
    lock_unpoisoned(&arc).closed = true;
    HOST_STATUS_SUCCESS
}

/// Fast-path helper for `concurrent.channel_len(channel)`.
pub fn concurrent_channel_len_fast(channel: SpectraHostValue) -> i64 {
    let arc = match lock_concurrent_registry() {
        Ok(r) => r.channels.get(channel).cloned(),
        Err(_) => return 0,
    };
    let Some(arc) = arc else {
        return 0;
    };
    let n = lock_unpoisoned(&arc).queue.len() as i64;
    n
}

extern "C" fn std_async_reactor_reset(ctx: *mut SpectraHostCallContext) -> i32 {
    let args = match host_call_void_args(ctx, 0) {
        Ok(args) => args,
        Err(status) => return status,
    };
    let _ = args;
    reactor::global().reset();
    if set_async_last_reactor_event(None).is_err() {
        return HOST_STATUS_INTERNAL_ERROR;
    }
    HOST_STATUS_SUCCESS
}
