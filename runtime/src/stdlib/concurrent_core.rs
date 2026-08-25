const CONCURRENT_CHANNEL_INITIAL_CAPACITY: usize = 8;

struct ConcurrentChannel {
    queue: VecDeque<SpectraHostValue>,
    closed: bool,
}

struct ConcurrentTask {
    state: AtomicU8,
    value: AtomicI64,
}

impl ConcurrentTask {
    const PENDING: u8 = 0;
    const READY: u8 = 1;
    const FAILED: u8 = 2;
    const CANCELLED: u8 = 3;

    fn pending() -> Self {
        Self {
            state: AtomicU8::new(Self::PENDING),
            value: AtomicI64::new(0),
        }
    }

    fn prepare(&self) {
        self.value.store(0, Ordering::Relaxed);
        self.state.store(Self::PENDING, Ordering::Release);
    }

    fn complete(&self, value: SpectraHostValue) -> bool {
        self.value.store(value, Ordering::Relaxed);
        self.state
            .compare_exchange(
                Self::PENDING,
                Self::READY,
                Ordering::Release,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    fn fail(&self) -> bool {
        self.state
            .compare_exchange(
                Self::PENDING,
                Self::FAILED,
                Ordering::Release,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    fn cancel(&self) -> bool {
        self.state
            .compare_exchange(
                Self::PENDING,
                Self::CANCELLED,
                Ordering::Release,
                Ordering::Relaxed,
            )
            .is_ok()
    }

    fn is_done(&self) -> bool {
        self.state.load(Ordering::Acquire) != Self::PENDING
    }

    fn join(&self) -> Result<SpectraHostValue, i32> {
        loop {
            match self.state.load(Ordering::Acquire) {
                Self::READY => return Ok(self.value.load(Ordering::Relaxed)),
                Self::FAILED => return Err(HOST_STATUS_INTERNAL_ERROR),
                Self::CANCELLED => return Err(HOST_STATUS_NOT_FOUND),
                Self::PENDING => {
                    let (epoch, ready) = concurrent_completion_signal();
                    let guard = lock_unpoisoned(epoch);
                    if self.state.load(Ordering::Acquire) == Self::PENDING {
                        drop(ready.wait(guard).unwrap_or_else(|error| error.into_inner()));
                    }
                }
                _ => return Err(HOST_STATUS_INTERNAL_ERROR),
            }
        }
    }
}

fn concurrent_completion_signal() -> &'static (Mutex<u64>, Condvar) {
    static SIGNAL: OnceLock<(Mutex<u64>, Condvar)> = OnceLock::new();
    SIGNAL.get_or_init(|| (Mutex::new(0), Condvar::new()))
}

fn notify_concurrent_completion() {
    let (epoch, ready) = concurrent_completion_signal();
    let mut epoch = lock_unpoisoned(epoch);
    *epoch = epoch.wrapping_add(1);
    ready.notify_all();
}

struct ConcurrentBatch {
    count: usize,
    remaining: AtomicUsize,
    total: AtomicI64,
    failed: AtomicBool,
    cancelled: AtomicBool,
}

impl ConcurrentBatch {
    fn new(count: usize) -> Self {
        Self {
            count,
            remaining: AtomicUsize::new(count),
            total: AtomicI64::new(0),
            failed: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
        }
    }

    fn finish_lane(&self, lane_total: SpectraHostValue, completed: usize) -> bool {
        if completed == 0 {
            return false;
        }
        self.total.fetch_add(lane_total, Ordering::Relaxed);
        self.remaining.fetch_sub(completed, Ordering::AcqRel) == completed
    }

    fn join_sum(&self) -> Result<SpectraHostValue, i32> {
        let mut spins = 0usize;
        while self.remaining.load(Ordering::Acquire) != 0 {
            if self.cancelled.load(Ordering::Acquire) {
                return Err(HOST_STATUS_NOT_FOUND);
            }
            if self.failed.load(Ordering::Acquire) {
                return Err(HOST_STATUS_INTERNAL_ERROR);
            }
            if spins < 128 {
                std::hint::spin_loop();
                spins += 1;
            } else {
                thread::yield_now();
            }
        }
        if self.failed.load(Ordering::Acquire) {
            return Err(HOST_STATUS_INTERNAL_ERROR);
        }
        Ok(self.total.load(Ordering::Relaxed))
    }
}

enum ConcurrentJob {
    Single {
        task: Arc<ConcurrentTask>,
        value: SpectraHostValue,
        queued_at: StdInstant,
    },
    Closure {
        task: Arc<ConcurrentTask>,
        fn_ptr: SpectraHostValue,
        arg: SpectraHostValue,
        queued_at: StdInstant,
    },
    BatchLane {
        batch: Arc<ConcurrentBatch>,
        first_value: SpectraHostValue,
        count: usize,
        lane: usize,
        lanes: usize,
        queued_at: StdInstant,
    },
}

struct ConcurrentExecutor {
    sender: mpsc::Sender<ConcurrentJob>,
    workers: usize,
}

impl ConcurrentExecutor {
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel::<ConcurrentJob>();
        let receiver = Arc::new(Mutex::new(receiver));
        let workers = thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(2)
            .clamp(2, 2);
        for worker_index in 0..workers {
            let receiver = Arc::clone(&receiver);
            thread::Builder::new()
                .name(format!("spectra-concurrent-{worker_index}"))
                .spawn(move || loop {
                    let job = {
                        let receiver = lock_unpoisoned(&receiver);
                        receiver.recv()
                    };
                    let Ok(job) = job else {
                        break;
                    };
                    match job {
                        ConcurrentJob::Single {
                            task,
                            value,
                            queued_at,
                        } => {
                            let execution_started = StdInstant::now();
                            if let Some(data) = concurrent_diagnostics() {
                                data.tasks_executed.fetch_add(1, Ordering::Relaxed);
                            }
                            if task.complete(value) {
                                if let Some(data) = concurrent_diagnostics() {
                                    data.task_wakeups.fetch_add(1, Ordering::Relaxed);
                                    data.pending_tasks.fetch_sub(1, Ordering::Relaxed);
                                    data.scheduler_ns.fetch_add(
                                        queued_at.elapsed().as_nanos().min(u64::MAX as u128) as u64,
                                        Ordering::Relaxed,
                                    );
                                    data.execution_ns.fetch_add(
                                        execution_started.elapsed().as_nanos().min(u64::MAX as u128)
                                            as u64,
                                        Ordering::Relaxed,
                                    );
                                }
                                notify_concurrent_completion();
                            }
                        }
                        ConcurrentJob::Closure {
                            task,
                            fn_ptr,
                            arg,
                            queued_at,
                        } => {
                            let execution_started = StdInstant::now();
                            if let Some(data) = concurrent_diagnostics() {
                                data.tasks_executed.fetch_add(1, Ordering::Relaxed);
                            }
                            // Real user code runs here: the worker thread calls
                            // back into the JIT-compiled closure through the
                            // same boundary HOFs use (spectra_rt_invoke_closure,
                            // catch_unwind inside). A panicking closure marks
                            // the task FAILED; it never aborts the process.
                            let outcome = invoke_concurrent_closure(fn_ptr, arg);
                            let completed = match outcome {
                                Ok(value) => task.complete(value),
                                Err(()) => task.fail(),
                            };
                            if completed {
                                if let Some(data) = concurrent_diagnostics() {
                                    data.task_wakeups.fetch_add(1, Ordering::Relaxed);
                                    data.pending_tasks.fetch_sub(1, Ordering::Relaxed);
                                    data.scheduler_ns.fetch_add(
                                        queued_at.elapsed().as_nanos().min(u64::MAX as u128) as u64,
                                        Ordering::Relaxed,
                                    );
                                    data.execution_ns.fetch_add(
                                        execution_started.elapsed().as_nanos().min(u64::MAX as u128)
                                            as u64,
                                        Ordering::Relaxed,
                                    );
                                }
                                notify_concurrent_completion();
                            }
                        }
                        ConcurrentJob::BatchLane {
                            batch,
                            first_value,
                            count,
                            lane,
                            lanes,
                            queued_at,
                        } => {
                            let execution_started = StdInstant::now();
                            let mut lane_total: SpectraHostValue = 0;
                            let mut completed = 0usize;
                            for offset in (lane..count).step_by(lanes) {
                                if batch.cancelled.load(Ordering::Acquire) {
                                    break;
                                }
                                let value = first_value.saturating_add(offset as SpectraHostValue);
                                lane_total = lane_total.wrapping_add(value);
                                completed += 1;
                                if let Some(data) = concurrent_diagnostics() {
                                    data.tasks_executed.fetch_add(1, Ordering::Relaxed);
                                    data.pending_tasks.fetch_sub(1, Ordering::Relaxed);
                                }
                            }

                            let completed_last = batch.finish_lane(lane_total, completed);
                            if let Some(data) = concurrent_diagnostics() {
                                data.scheduler_ns.fetch_add(
                                    queued_at.elapsed().as_nanos().min(u64::MAX as u128) as u64,
                                    Ordering::Relaxed,
                                );
                                data.execution_ns.fetch_add(
                                    execution_started.elapsed().as_nanos().min(u64::MAX as u128)
                                        as u64,
                                    Ordering::Relaxed,
                                );
                            }
                            if completed_last {
                                if let Some(data) = concurrent_diagnostics() {
                                    data.task_wakeups.fetch_add(1, Ordering::Relaxed);
                                }
                                notify_concurrent_completion();
                            }
                        }
                    }
                })
                .expect("failed to create Spectra concurrent worker");
        }
        Self { sender, workers }
    }

    fn submit(&self, task: Arc<ConcurrentTask>, value: SpectraHostValue) -> Result<(), ()> {
        self.sender
            .send(ConcurrentJob::Single {
                task,
                value,
                queued_at: StdInstant::now(),
            })
            .map_err(|_| ())
    }
    fn submit_closure(
        &self,
        task: Arc<ConcurrentTask>,
        fn_ptr: SpectraHostValue,
        arg: SpectraHostValue,
    ) -> Result<(), ()> {
        self.sender
            .send(ConcurrentJob::Closure {
                task,
                fn_ptr,
                arg,
                queued_at: StdInstant::now(),
            })
            .map_err(|_| ())
    }

    fn submit_batch_lane(
        &self,
        batch: Arc<ConcurrentBatch>,
        first_value: SpectraHostValue,
        count: usize,
        lane: usize,
        lanes: usize,
    ) -> Result<(), ()> {
        self.sender
            .send(ConcurrentJob::BatchLane {
                batch,
                first_value,
                count,
                lane,
                lanes,
                queued_at: StdInstant::now(),
            })
            .map_err(|_| ())
    }
}

fn concurrent_executor() -> &'static ConcurrentExecutor {
    static EXECUTOR: OnceLock<ConcurrentExecutor> = OnceLock::new();
    EXECUTOR.get_or_init(ConcurrentExecutor::new)
}

struct ConcurrentHandleTable<T> {
    table: HandleTable<T>,
}

impl<T> ConcurrentHandleTable<T> {
    fn new(kind: HandleKind) -> Self {
        Self {
            table: HandleTable::new(kind),
        }
    }

    fn insert(&mut self, value: T) -> SpectraHostValue {
        self.table.insert(value).raw()
    }

    fn get(&self, raw: SpectraHostValue) -> Option<&T> {
        let handle = HandleId::from_raw(raw).ok()?;
        self.table.get(handle).ok()
    }

    fn get_mut(&mut self, raw: SpectraHostValue) -> Option<&mut T> {
        let handle = HandleId::from_raw(raw).ok()?;
        self.table.get_mut(handle).ok()
    }

    fn remove(&mut self, raw: SpectraHostValue) -> Option<T> {
        let handle = HandleId::from_raw(raw).ok()?;
        self.table.remove(handle).ok()
    }

    fn clear(&mut self) {
        self.table.clear();
    }

    fn len(&self) -> usize {
        self.table.len()
    }

    fn slot_count(&self) -> usize {
        self.table.slot_count()
    }

    fn iter(&self) -> impl Iterator<Item = (SpectraHostValue, &T)> + '_ {
        self.table
            .iter()
            .map(|(handle, value)| (handle.raw(), value))
    }
}

struct ConcurrentRegistry {
    // task_spawn receives an already-evaluated Spectra value; task_spawn_fn
    // instead dispatches a real JIT closure onto the worker pool. Both
    // schedule completion on the persistent executor, preserving the public
    // API while making fan-out/fan-in observable without one OS thread per
    // task.
    tasks: ConcurrentHandleTable<Arc<ConcurrentTask>>,
    batches: ConcurrentHandleTable<Arc<ConcurrentBatch>>,
    channels: ConcurrentHandleTable<Arc<Mutex<ConcurrentChannel>>>,
    counters: ConcurrentHandleTable<SpectraHostValue>,
    tasks_spawned: SpectraHostValue,
}

impl ConcurrentRegistry {
    fn new() -> Self {
        Self {
            tasks: ConcurrentHandleTable::new(HandleKind::ConcurrentTask),
            batches: ConcurrentHandleTable::new(HandleKind::ConcurrentBatch),
            channels: ConcurrentHandleTable::new(HandleKind::ConcurrentChannel),
            counters: ConcurrentHandleTable::new(HandleKind::ConcurrentCounter),
            tasks_spawned: 0,
        }
    }

    fn clear(&mut self) {
        for (_, batch) in self.batches.iter() {
            batch.cancelled.store(true, Ordering::Release);
        }
        for (_, task) in self.tasks.iter() {
            if task.cancel() {
                if let Some(data) = concurrent_diagnostics() {
                    data.tasks_cancelled.fetch_add(1, Ordering::Relaxed);
                    data.pending_tasks.fetch_sub(1, Ordering::Relaxed);
                }
            }
        }
        notify_concurrent_completion();
        self.tasks.clear();
        self.batches.clear();
        self.channels.clear();
        self.counters.clear();
        self.tasks_spawned = 0;
    }

    fn allocate_task(&mut self) -> (SpectraHostValue, Arc<ConcurrentTask>) {
        self.tasks_spawned += 1;
        let slots_before = self.tasks.slot_count();
        let task = Arc::new(ConcurrentTask::pending());
        let task_id = self.tasks.insert(Arc::clone(&task));
        if self.tasks.slot_count() > slots_before {
            if let Some(data) = concurrent_diagnostics() {
                data.slots_created.fetch_add(1, Ordering::Relaxed);
            }
        }
        task.prepare();
        (task_id, task)
    }

    fn task(&self, task_id: SpectraHostValue) -> Result<Arc<ConcurrentTask>, i32> {
        self.tasks
            .get(task_id)
            .cloned()
            .ok_or(HOST_STATUS_NOT_FOUND)
    }

    fn is_done(&self, task_id: SpectraHostValue) -> Result<bool, i32> {
        self.tasks
            .get(task_id)
            .map(|task| task.is_done())
            .ok_or(HOST_STATUS_NOT_FOUND)
    }

    fn release(
        &mut self,
        task_id: SpectraHostValue,
        task: &Arc<ConcurrentTask>,
    ) -> Result<(), i32> {
        let current = self.tasks.get(task_id).ok_or(HOST_STATUS_NOT_FOUND)?;
        if !Arc::ptr_eq(current, task) {
            return Err(HOST_STATUS_NOT_FOUND);
        }
        self.tasks.remove(task_id).ok_or(HOST_STATUS_NOT_FOUND)?;
        Ok(())
    }

    fn allocate_batch(&mut self, count: usize) -> (SpectraHostValue, Arc<ConcurrentBatch>) {
        self.tasks_spawned = self.tasks_spawned.saturating_add(count as SpectraHostValue);
        let batch = Arc::new(ConcurrentBatch::new(count));
        let batch_id = self.batches.insert(Arc::clone(&batch));
        if let Some(data) = concurrent_diagnostics() {
            data.batches_created.fetch_add(1, Ordering::Relaxed);
        }
        (batch_id, batch)
    }

    fn batch(&self, batch_id: SpectraHostValue) -> Result<Arc<ConcurrentBatch>, i32> {
        self.batches
            .get(batch_id)
            .cloned()
            .ok_or(HOST_STATUS_NOT_FOUND)
    }

    fn release_batch(&mut self, batch_id: SpectraHostValue) -> Result<(), i32> {
        self.batches
            .remove(batch_id)
            .map(|_| {
                if let Some(data) = concurrent_diagnostics() {
                    data.batches_joined.fetch_add(1, Ordering::Relaxed);
                }
            })
            .ok_or(HOST_STATUS_NOT_FOUND)
    }
}

fn concurrent_registry() -> &'static Mutex<ConcurrentRegistry> {
    static REGISTRY: OnceLock<Mutex<ConcurrentRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(ConcurrentRegistry::new()))
}

fn lock_concurrent_registry() -> Result<std::sync::MutexGuard<'static, ConcurrentRegistry>, i32> {
    let result = concurrent_registry()
        .lock()
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR);
    if result.is_ok() {
        if let Some(data) = concurrent_diagnostics() {
            data.locks_acquired.fetch_add(1, Ordering::Relaxed);
        }
    }
    result
}

fn record_concurrent_task_created() {
    if let Some(data) = concurrent_diagnostics() {
        data.tasks_created.fetch_add(1, Ordering::Relaxed);
        data.tasks_counted.fetch_add(1, Ordering::Relaxed);
        let pending = data.pending_tasks.fetch_add(1, Ordering::Relaxed) + 1;
        let mut maximum = data.max_pending_tasks.load(Ordering::Relaxed);
        while pending > maximum {
            match data.max_pending_tasks.compare_exchange_weak(
                maximum,
                pending,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => maximum = observed,
            }
        }
    }
}

fn spawn_concurrent_task(value: SpectraHostValue) -> Result<SpectraHostValue, i32> {
    let (task_id, task) = {
        let mut registry = lock_concurrent_registry()?;
        registry.allocate_task()
    };
    record_concurrent_task_created();
    if concurrent_executor()
        .submit(Arc::clone(&task), value)
        .is_err()
    {
        if task.fail() {
            if let Some(data) = concurrent_diagnostics() {
                data.tasks_failed.fetch_add(1, Ordering::Relaxed);
                data.pending_tasks.fetch_sub(1, Ordering::Relaxed);
            }
        }
        return Err(HOST_STATUS_INTERNAL_ERROR);
    }
    Ok(task_id)
}

/// Invokes a JIT-compiled Spectra closure on a worker thread through the
/// same boundary the higher-order stdlib functions use
/// (`spectra_rt_invoke_closure`, which carries its own catch_unwind).
///
/// Returns `Ok(value)` with the closure result, or `Err(())` when the
/// invocation fails (null fn pointer, bad arity, panicking closure). The
/// error never propagates as a Rust panic: a failing user closure must turn
/// into a FAILED task, not abort the worker.
fn invoke_concurrent_closure(
    fn_ptr: SpectraHostValue,
    arg: SpectraHostValue,
) -> Result<SpectraHostValue, ()> {
    if fn_ptr == 0 {
        return Err(());
    }
    let args = [arg];
    let mut out: SpectraHostValue = 0;
    // SAFETY: fn_ptr is the closure handle produced by the backend (slot 0 =
    // code pointer) and `args` lives for the duration of the call.
    let status = unsafe {
        crate::ffi::spectra_rt_invoke_closure(fn_ptr, args.as_ptr(), 1, &mut out)
    };
    if status == HOST_STATUS_SUCCESS {
        Ok(out)
    } else {
        Err(())
    }
}

/// `concurrent.task_spawn_fn(closure_handle, arg) -> task_id`.
///
/// The task handle is allocated and registered BEFORE the job is dispatched,
/// so `task_is_done` / `task_join` observe it immediately. The closure runs
/// on a persistent executor worker; its return value becomes the task's
/// value (join returns it), and a panic inside the closure marks the task
/// FAILED (join reports an internal-error status) instead of aborting.
fn spawn_concurrent_task_fn(
    fn_ptr: SpectraHostValue,
    arg: SpectraHostValue,
) -> Result<SpectraHostValue, i32> {
    let (task_id, task) = {
        let mut registry = lock_concurrent_registry()?;
        registry.allocate_task()
    };
    record_concurrent_task_created();
    if concurrent_executor()
        .submit_closure(Arc::clone(&task), fn_ptr, arg)
        .is_err()
    {
        if task.fail() {
            if let Some(data) = concurrent_diagnostics() {
                data.tasks_failed.fetch_add(1, Ordering::Relaxed);
                data.pending_tasks.fetch_sub(1, Ordering::Relaxed);
            }
        }
        return Err(HOST_STATUS_INTERNAL_ERROR);
    }
    Ok(task_id)
}

fn join_concurrent_task(task_id: SpectraHostValue) -> Result<SpectraHostValue, i32> {
    let task = {
        let registry = lock_concurrent_registry()?;
        registry.task(task_id)?
    };
    if let Some(data) = concurrent_diagnostics() {
        data.task_joins.fetch_add(1, Ordering::Relaxed);
    }
    let value = task.join()?;
    let mut registry = lock_concurrent_registry()?;
    registry.release(task_id, &task)?;
    Ok(value)
}

fn concurrent_task_done(task_id: SpectraHostValue) -> Result<bool, i32> {
    if let Some(data) = concurrent_diagnostics() {
        data.task_polls.fetch_add(1, Ordering::Relaxed);
    }
    let registry = lock_concurrent_registry()?;
    registry.is_done(task_id)
}

fn spawn_concurrent_batch(
    first_value: SpectraHostValue,
    count: SpectraHostValue,
) -> Result<SpectraHostValue, i32> {
    let count = usize::try_from(count).map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    if count == 0 || count > 4096 {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let (batch_id, batch) = {
        let mut registry = lock_concurrent_registry()?;
        registry.allocate_batch(count)
    };
    for _ in 0..count {
        record_concurrent_task_created();
    }
    let lanes = concurrent_executor().workers.min(count);
    for lane in 0..lanes {
        if concurrent_executor()
            .submit_batch_lane(Arc::clone(&batch), first_value, count, lane, lanes)
            .is_err()
        {
            batch.failed.store(true, Ordering::Release);
            if let Some(data) = concurrent_diagnostics() {
                data.tasks_failed.fetch_add(count as u64, Ordering::Relaxed);
            }
            return Err(HOST_STATUS_INTERNAL_ERROR);
        }
    }
    Ok(batch_id)
}

fn join_concurrent_batch_sum(batch_id: SpectraHostValue) -> Result<SpectraHostValue, i32> {
    let batch = {
        let registry = lock_concurrent_registry()?;
        registry.batch(batch_id)?
    };
    if let Some(data) = concurrent_diagnostics() {
        data.task_joins
            .fetch_add(batch.count as u64, Ordering::Relaxed);
    }
    let total = batch.join_sum()?;
    let mut registry = lock_concurrent_registry()?;
    registry.release_batch(batch_id)?;
    Ok(total)
}

