use super::*;

// ── std.collections Stack and Queue ────────────────────────────────────────

pub(crate) const STACK_NEW: &str = "spectra.std.collections.stack_new";
pub(crate) const STACK_PUSH: &str = "spectra.std.collections.stack_push";
pub(crate) const STACK_POP: &str = "spectra.std.collections.stack_pop";
pub(crate) const STACK_PEEK: &str = "spectra.std.collections.stack_peek";
pub(crate) const STACK_LEN: &str = "spectra.std.collections.stack_len";
pub(crate) const STACK_IS_EMPTY: &str = "spectra.std.collections.stack_is_empty";
pub(crate) const STACK_CLEAR: &str = "spectra.std.collections.stack_clear";
pub(crate) const STACK_FREE: &str = "spectra.std.collections.stack_free";
pub(crate) const STACK_FREE_ALL: &str = "spectra.std.collections.stack_free_all";

pub(crate) const QUEUE_NEW: &str = "spectra.std.collections.queue_new";
pub(crate) const QUEUE_ENQUEUE: &str = "spectra.std.collections.queue_enqueue";
pub(crate) const QUEUE_DEQUEUE: &str = "spectra.std.collections.queue_dequeue";
pub(crate) const QUEUE_PEEK: &str = "spectra.std.collections.queue_peek";
pub(crate) const QUEUE_LEN: &str = "spectra.std.collections.queue_len";
pub(crate) const QUEUE_IS_EMPTY: &str = "spectra.std.collections.queue_is_empty";
pub(crate) const QUEUE_CLEAR: &str = "spectra.std.collections.queue_clear";
pub(crate) const QUEUE_FREE: &str = "spectra.std.collections.queue_free";
pub(crate) const QUEUE_FREE_ALL: &str = "spectra.std.collections.queue_free_all";

pub(crate) fn register_stack_queue() {
    register_host_function(STACK_NEW, std_stack_new);
    register_host_function(STACK_PUSH, std_stack_push);
    register_host_function(STACK_POP, std_stack_pop);
    register_host_function(STACK_PEEK, std_stack_peek);
    register_host_function(STACK_LEN, std_stack_len);
    register_host_function(STACK_IS_EMPTY, std_stack_is_empty);
    register_host_function(STACK_CLEAR, std_stack_clear);
    register_host_function(STACK_FREE, std_stack_free);
    register_host_function(STACK_FREE_ALL, std_stack_free_all);

    register_host_function(QUEUE_NEW, std_queue_new);
    register_host_function(QUEUE_ENQUEUE, std_queue_enqueue);
    register_host_function(QUEUE_DEQUEUE, std_queue_dequeue);
    register_host_function(QUEUE_PEEK, std_queue_peek);
    register_host_function(QUEUE_LEN, std_queue_len);
    register_host_function(QUEUE_IS_EMPTY, std_queue_is_empty);
    register_host_function(QUEUE_CLEAR, std_queue_clear);
    register_host_function(QUEUE_FREE, std_queue_free);
    register_host_function(QUEUE_FREE_ALL, std_queue_free_all);
}

#[derive(Default)]
pub(crate) struct StdStack {
    pub(crate) data: Vec<SpectraHostValue>,
}

pub(crate) struct StackRegistry {
    pub(crate) stacks: HandleTable<ManualBox<StdStack>>,
}

impl StackRegistry {
    pub(crate) fn new() -> Self {
        Self {
            stacks: HandleTable::new(HandleKind::Stack),
        }
    }

    pub(crate) fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    pub(crate) fn insert(&mut self, stack: ManualBox<StdStack>) -> usize {
        self.stacks.insert(stack).raw() as usize
    }

    pub(crate) fn push(&mut self, handle: usize, value: SpectraHostValue) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.stacks
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .push(value);
        Ok(())
    }

    pub(crate) fn pop(&mut self, handle: usize) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .stacks
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .pop())
    }

    pub(crate) fn peek(&self, handle: usize) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .stacks
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .last()
            .copied())
    }

    pub(crate) fn len(&self, handle: usize) -> Result<usize, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .stacks
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .len())
    }

    pub(crate) fn is_empty(&self, handle: usize) -> Result<bool, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .stacks
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .is_empty())
    }

    pub(crate) fn clear(&mut self, handle: usize) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.stacks
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .clear();
        Ok(())
    }

    pub(crate) fn remove(&mut self, handle: usize) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.stacks
            .remove(id)
            .map(|_| ())
            .map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    pub(crate) fn clear_all(&mut self) -> usize {
        self.stacks.clear()
    }

    pub(crate) fn snapshot(&self, handle: usize) -> Result<Vec<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .stacks
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .clone())
    }
}

pub(crate) fn stack_registry() -> &'static Mutex<StackRegistry> {
    static REGISTRY: OnceLock<Mutex<StackRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(StackRegistry::new()))
}

pub(crate) fn with_stack_registry<F, R>(action: F) -> R
where
    F: FnOnce(&mut StackRegistry) -> R,
{
    let mut guard = lock_unpoisoned(stack_registry());
    action(&mut guard)
}

#[derive(Default)]
pub(crate) struct StdQueue {
    pub(crate) data: VecDeque<SpectraHostValue>,
}

pub(crate) struct QueueRegistry {
    pub(crate) queues: HandleTable<ManualBox<StdQueue>>,
}

impl QueueRegistry {
    pub(crate) fn new() -> Self {
        Self {
            queues: HandleTable::new(HandleKind::Queue),
        }
    }

    pub(crate) fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    pub(crate) fn insert(&mut self, queue: ManualBox<StdQueue>) -> usize {
        self.queues.insert(queue).raw() as usize
    }

    pub(crate) fn enqueue(&mut self, handle: usize, value: SpectraHostValue) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.queues
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .push_back(value);
        Ok(())
    }

    pub(crate) fn dequeue(&mut self, handle: usize) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .queues
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .pop_front())
    }

    pub(crate) fn peek(&self, handle: usize) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .queues
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .front()
            .copied())
    }

    pub(crate) fn len(&self, handle: usize) -> Result<usize, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .queues
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .len())
    }

    pub(crate) fn is_empty(&self, handle: usize) -> Result<bool, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .queues
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .is_empty())
    }

    pub(crate) fn clear(&mut self, handle: usize) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.queues
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .clear();
        Ok(())
    }

    pub(crate) fn remove(&mut self, handle: usize) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.queues
            .remove(id)
            .map(|_| ())
            .map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    pub(crate) fn clear_all(&mut self) -> usize {
        self.queues.clear()
    }

    pub(crate) fn snapshot(&self, handle: usize) -> Result<Vec<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .queues
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .iter()
            .copied()
            .collect())
    }
}

pub(crate) fn queue_registry() -> &'static Mutex<QueueRegistry> {
    static REGISTRY: OnceLock<Mutex<QueueRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(QueueRegistry::new()))
}

pub(crate) fn with_queue_registry<F, R>(action: F) -> R
where
    F: FnOnce(&mut QueueRegistry) -> R,
{
    let mut guard = lock_unpoisoned(queue_registry());
    action(&mut guard)
}

fn allocate_stack() -> Result<usize, i32> {
    let stack = initialize()
        .memory()
        .allocate_manual(StdStack::default())
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    Ok(with_stack_registry(|registry| registry.insert(stack)))
}

fn allocate_queue() -> Result<usize, i32> {
    let queue = initialize()
        .memory()
        .allocate_manual(StdQueue::default())
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    Ok(with_queue_registry(|registry| registry.insert(queue)))
}

pub(crate) extern "C" fn std_stack_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((_, results)) = host_call_args(ctx, 0) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let handle = match allocate_stack() {
        Ok(handle) => handle,
        Err(code) => return code,
    };
    results[0] = handle as SpectraHostValue;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_stack_push(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_call_void_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    crate::ffi::escape_stored_value(args[1]);
    match with_stack_registry(|registry| registry.push(args[0] as usize, args[1])) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

fn stack_option(
    ctx: *mut SpectraHostCallContext,
    value: Result<Option<SpectraHostValue>, i32>,
) -> i32 {
    let value = match value {
        Ok(value) => value,
        Err(code) => return code,
    };
    unsafe { write_option_result(&mut *ctx, value) }
}

pub(crate) extern "C" fn std_stack_pop(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_call_void_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    stack_option(
        ctx,
        with_stack_registry(|registry| registry.pop(args[0] as usize)),
    )
}

pub(crate) extern "C" fn std_stack_peek(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_call_void_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    stack_option(
        ctx,
        with_stack_registry(|registry| registry.peek(args[0] as usize)),
    )
}

pub(crate) extern "C" fn std_stack_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_stack_registry(|registry| registry.len(args[0] as usize)) {
        Ok(len) => {
            results[0] = len as SpectraHostValue;
            HOST_STATUS_SUCCESS
        }
        Err(code) => code,
    }
}

pub(crate) extern "C" fn std_stack_is_empty(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_stack_registry(|registry| registry.is_empty(args[0] as usize)) {
        Ok(empty) => {
            results[0] = empty as SpectraHostValue;
            HOST_STATUS_SUCCESS
        }
        Err(code) => code,
    }
}

pub(crate) extern "C" fn std_stack_clear(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_call_void_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_stack_registry(|registry| registry.clear(args[0] as usize)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub(crate) extern "C" fn std_stack_free(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_call_void_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_stack_registry(|registry| registry.remove(args[0] as usize)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub(crate) extern "C" fn std_stack_free_all(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((_, results)) = host_call_args(ctx, 0) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    results[0] = with_stack_registry(|registry| registry.clear_all()) as SpectraHostValue;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_queue_new(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((_, results)) = host_call_args(ctx, 0) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let handle = match allocate_queue() {
        Ok(handle) => handle,
        Err(code) => return code,
    };
    results[0] = handle as SpectraHostValue;
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_queue_enqueue(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_call_void_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    crate::ffi::escape_stored_value(args[1]);
    match with_queue_registry(|registry| registry.enqueue(args[0] as usize, args[1])) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

fn queue_option(
    ctx: *mut SpectraHostCallContext,
    value: Result<Option<SpectraHostValue>, i32>,
) -> i32 {
    let value = match value {
        Ok(value) => value,
        Err(code) => return code,
    };
    unsafe { write_option_result(&mut *ctx, value) }
}

pub(crate) extern "C" fn std_queue_dequeue(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_call_void_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    queue_option(
        ctx,
        with_queue_registry(|registry| registry.dequeue(args[0] as usize)),
    )
}

pub(crate) extern "C" fn std_queue_peek(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_call_void_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    queue_option(
        ctx,
        with_queue_registry(|registry| registry.peek(args[0] as usize)),
    )
}

pub(crate) extern "C" fn std_queue_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_queue_registry(|registry| registry.len(args[0] as usize)) {
        Ok(len) => {
            results[0] = len as SpectraHostValue;
            HOST_STATUS_SUCCESS
        }
        Err(code) => code,
    }
}

pub(crate) extern "C" fn std_queue_is_empty(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_queue_registry(|registry| registry.is_empty(args[0] as usize)) {
        Ok(empty) => {
            results[0] = empty as SpectraHostValue;
            HOST_STATUS_SUCCESS
        }
        Err(code) => code,
    }
}

pub(crate) extern "C" fn std_queue_clear(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_call_void_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_queue_registry(|registry| registry.clear(args[0] as usize)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub(crate) extern "C" fn std_queue_free(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_call_void_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_queue_registry(|registry| registry.remove(args[0] as usize)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub(crate) extern "C" fn std_queue_free_all(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((_, results)) = host_call_args(ctx, 0) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    results[0] = with_queue_registry(|registry| registry.clear_all()) as SpectraHostValue;
    HOST_STATUS_SUCCESS
}
