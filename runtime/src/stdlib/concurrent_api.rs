use super::*;
pub(crate) extern "C" fn std_concurrent_task_spawn(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let value = args[0];
    match spawn_concurrent_task(value) {
        Ok(task_id) => {
            results[0] = task_id;
            HOST_STATUS_SUCCESS
        }
        Err(status) => status,
    }
}

pub(crate) extern "C" fn std_concurrent_task_spawn_fn(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    match spawn_concurrent_task_fn(args[0], args[1]) {
        Ok(task_id) => {
            results[0] = task_id;
            HOST_STATUS_SUCCESS
        }
        Err(status) => status,
    }
}

pub(crate) extern "C" fn std_concurrent_task_join(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    match join_concurrent_task(args[0]) {
        Ok(value) => {
            results[0] = value;
            HOST_STATUS_SUCCESS
        }
        Err(code) => code,
    }
}

pub(crate) extern "C" fn std_concurrent_task_spawn_join(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_concurrent_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    registry.tasks_spawned += 1;
    results[0] = args[0];
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_concurrent_task_spawn_batch(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    match spawn_concurrent_batch(args[0], args[1]) {
        Ok(batch_id) => {
            results[0] = batch_id;
            HOST_STATUS_SUCCESS
        }
        Err(status) => status,
    }
}

pub(crate) extern "C" fn std_concurrent_task_join_batch_sum(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    match join_concurrent_batch_sum(args[0]) {
        Ok(total) => {
            results[0] = total;
            HOST_STATUS_SUCCESS
        }
        Err(status) => status,
    }
}

pub(crate) extern "C" fn std_concurrent_task_is_done(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    match concurrent_task_done(args[0]) {
        Ok(done) => {
            results[0] = i64::from(done);
            HOST_STATUS_SUCCESS
        }
        Err(code) => code,
    }
}

pub(crate) extern "C" fn std_concurrent_channel_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let (_, results) = match host_call_args(ctx, 0) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let (channel_id, channel_arc) = match lock_concurrent_registry() {
        Ok(mut registry) => {
            let arc = Arc::new(Mutex::new(ConcurrentChannel {
                queue: VecDeque::with_capacity(CONCURRENT_CHANNEL_INITIAL_CAPACITY),
                closed: false,
            }));
            let id = registry.channels.insert(arc.clone());
            (id, arc)
        }
        Err(status) => return status,
    };
    let _ = channel_arc;
    results[0] = channel_id;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_concurrent_channel_send(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let channel_arc = match lock_concurrent_registry() {
        Ok(registry) => registry.channels.get(args[0]).cloned(),
        Err(status) => return status,
    };
    let Some(channel_arc) = channel_arc else {
        return HOST_STATUS_NOT_FOUND;
    };
    let mut channel = lock_unpoisoned(&channel_arc);
    if channel.closed {
        results[0] = 0;
        return HOST_STATUS_SUCCESS;
    }
    channel.queue.push_back(args[1]);
    results[0] = 1;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_concurrent_channel_recv(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let channel_arc = match lock_concurrent_registry() {
        Ok(registry) => registry.channels.get(args[0]).cloned(),
        Err(status) => return status,
    };
    let Some(channel_arc) = channel_arc else {
        return HOST_STATUS_NOT_FOUND;
    };
    let mut channel = lock_unpoisoned(&channel_arc);
    results[0] = channel.queue.pop_front().unwrap_or(-1);
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_concurrent_channel_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let channel_arc = match lock_concurrent_registry() {
        Ok(registry) => registry.channels.get(args[0]).cloned(),
        Err(status) => return status,
    };
    let Some(channel_arc) = channel_arc else {
        return HOST_STATUS_NOT_FOUND;
    };
    let channel = lock_unpoisoned(&channel_arc);
    let n = channel.queue.len() as i64;
    results[0] = n;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_concurrent_channel_close(ctx: *mut SpectraHostCallContext) -> i32 {
    let args = match host_call_void_args(ctx, 1) {
        Ok(args) => args,
        Err(status) => return status,
    };
    let channel_arc = match lock_concurrent_registry() {
        Ok(registry) => registry.channels.get(args[0]).cloned(),
        Err(status) => return status,
    };
    let Some(channel_arc) = channel_arc else {
        return HOST_STATUS_NOT_FOUND;
    };
    lock_unpoisoned(&channel_arc).closed = true;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_concurrent_counter_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_concurrent_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let counter_id = registry.counters.insert(args[0]);
    results[0] = counter_id;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_concurrent_counter_add(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 2) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let mut registry = match lock_concurrent_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(value) = registry.counters.get_mut(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    *value += args[1];
    results[0] = *value;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_concurrent_counter_get(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 1) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_concurrent_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    let Some(value) = registry.counters.get(args[0]) else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = *value;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_concurrent_pipeline_sum(ctx: *mut SpectraHostCallContext) -> i32 {
    let (args, results) = match host_call_args(ctx, 3) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let start = args[0];
    let count = args[1].max(0);
    let workers = args[2].max(1).min(count.max(1));
    if count == 0 {
        results[0] = 0;
        return HOST_STATUS_SUCCESS;
    }

    let chunk_size = (count + workers - 1) / workers;
    let mut handles = Vec::new();
    for worker in 0..workers {
        let chunk_start = start + worker * chunk_size;
        let chunk_end = (chunk_start + chunk_size).min(start + count);
        if chunk_start >= chunk_end {
            continue;
        }
        handles.push(thread::spawn(move || {
            let mut sum = 0;
            for value in chunk_start..chunk_end {
                sum += value;
            }
            sum
        }));
    }

    let mut total = 0;
    for handle in handles {
        match handle.join() {
            Ok(partial) => total += partial,
            Err(_) => return HOST_STATUS_INTERNAL_ERROR,
        }
    }
    results[0] = total;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_concurrent_stats_tasks_spawned(ctx: *mut SpectraHostCallContext) -> i32 {
    let (_, results) = match host_call_args(ctx, 0) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_concurrent_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    results[0] = registry.tasks_spawned;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_concurrent_stats_channels(ctx: *mut SpectraHostCallContext) -> i32 {
    let (_, results) = match host_call_args(ctx, 0) {
        Ok(parts) => parts,
        Err(status) => return status,
    };
    let registry = match lock_concurrent_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    results[0] = registry.channels.len() as i64;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_concurrent_reset(ctx: *mut SpectraHostCallContext) -> i32 {
    let _ = match host_call_void_args(ctx, 0) {
        Ok(args) => args,
        Err(status) => return status,
    };
    let mut registry = match lock_concurrent_registry() {
        Ok(registry) => registry,
        Err(status) => return status,
    };
    registry.clear();
    HOST_STATUS_SUCCESS
}
