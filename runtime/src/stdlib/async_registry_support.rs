use super::*;
impl AsyncTaskRegistry {
    pub(crate) fn take_stream_chunk_sum(&mut self, stream_id: SpectraHostValue) -> Option<SpectraHostValue> {
        let stream = self.streams.get_mut(stream_id)?;
        let value = stream.chunk_items.iter().copied().sum();
        stream.chunk_items.clear();
        Some(value)
    }
}

pub(crate) fn map_stream_value(
    value: SpectraHostValue,
    op: SpectraHostValue,
    arg: SpectraHostValue,
) -> Option<SpectraHostValue> {
    match op {
        0 => Some(value),
        1 => Some(value.saturating_add(arg)),
        2 => Some(value.saturating_sub(arg)),
        3 => Some(value.saturating_mul(arg)),
        4 if arg != 0 => Some(value / arg),
        5 => Some(value.saturating_neg()),
        _ => None,
    }
}

pub(crate) fn filter_stream_value(
    value: SpectraHostValue,
    predicate: SpectraHostValue,
    arg: SpectraHostValue,
) -> Option<bool> {
    match predicate {
        0 => Some(value != 0),
        1 => Some(value == arg),
        2 => Some(value != arg),
        3 => Some(value > arg),
        4 => Some(value >= arg),
        5 => Some(value < arg),
        6 => Some(value <= arg),
        7 if arg != 0 => Some(value % arg == 0),
        _ => None,
    }
}

pub(crate) fn fold_stream_value(
    accumulator: SpectraHostValue,
    value: SpectraHostValue,
    op: SpectraHostValue,
) -> Option<SpectraHostValue> {
    match op {
        0 => Some(accumulator.saturating_add(value)),
        1 => Some(accumulator.saturating_mul(value)),
        2 => Some(accumulator.min(value)),
        3 => Some(accumulator.max(value)),
        _ => None,
    }
}

pub(crate) type BackgroundJob = Box<dyn FnOnce() + Send + 'static>;
pub(crate) type BackgroundCancelHook = Arc<dyn Fn() + Send + Sync + 'static>;

pub(crate) struct BackgroundWorkerQueue {
    pub(crate) sender: mpsc::SyncSender<BackgroundJob>,
}

pub(crate) fn background_worker_queue() -> &'static BackgroundWorkerQueue {
    static QUEUE: OnceLock<BackgroundWorkerQueue> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel::<BackgroundJob>(128);
        let receiver = Arc::new(Mutex::new(receiver));
        for index in 0..4 {
            let receiver = Arc::clone(&receiver);
            thread::Builder::new()
                .name(format!("spectra-background-{index}"))
                .spawn(move || loop {
                    let job = lock_unpoisoned(&receiver).recv();
                    match job {
                        Ok(job) => job(),
                        Err(_) => break,
                    }
                })
                .expect("background worker thread");
        }
        BackgroundWorkerQueue { sender }
    })
}

pub(crate) fn background_io_worker_queue() -> &'static BackgroundWorkerQueue {
    static QUEUE: OnceLock<BackgroundWorkerQueue> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel::<BackgroundJob>(128);
        let receiver = Arc::new(Mutex::new(receiver));
        for index in 0..4 {
            let receiver = Arc::clone(&receiver);
            thread::Builder::new()
                .name(format!("spectra-background-io-{index}"))
                .spawn(move || loop {
                    let job = lock_unpoisoned(&receiver).recv();
                    match job {
                        Ok(job) => job(),
                        Err(_) => break,
                    }
                })
                .expect("background I/O worker thread");
        }
        BackgroundWorkerQueue { sender }
    })
}

pub(crate) fn background_cancel_hooks(
) -> &'static Mutex<HashMap<SpectraHostValue, BackgroundCancelHook>> {
    static HOOKS: OnceLock<Mutex<HashMap<SpectraHostValue, BackgroundCancelHook>>> =
        OnceLock::new();
    HOOKS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn background_task_lifecycle() -> &'static Mutex<()> {
    static LIFECYCLE: OnceLock<Mutex<()>> = OnceLock::new();
    LIFECYCLE.get_or_init(|| Mutex::new(()))
}
