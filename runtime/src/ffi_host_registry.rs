struct HostRegistry {
    functions: HashMap<String, usize>,
}

impl HostRegistry {
    fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    fn insert(&mut self, name: &str, ptr: *const ()) -> bool {
        self.functions
            .insert(name.to_string(), ptr as usize)
            .is_none()
    }

    fn remove(&mut self, name: &str) -> bool {
        self.functions.remove(name).is_some()
    }

    fn lookup(&self, name: &str) -> *const () {
        self.functions
            .get(name)
            .copied()
            .and_then(|value| {
                if value == 0 {
                    None
                } else {
                    Some(value as *const ())
                }
            })
            .unwrap_or(ptr::null())
    }

    fn clear(&mut self) {
        self.functions.clear();
    }

    fn check_invariants(&self) -> bool {
        self.functions
            .iter()
            .all(|(name, ptr)| !name.is_empty() && *ptr != 0)
    }
}

fn host_registry() -> &'static Mutex<HostRegistry> {
    static REGISTRY: OnceLock<Mutex<HostRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HostRegistry::new()))
}

/// Monotonic invalidation token for all generated host-call cache slots.
///
/// The registry remains protected by its existing mutex. The generation is
/// deliberately separate so cache hits only perform atomic loads and never
/// need to acquire that mutex. It starts at one so zero can remain the
/// uninitialised generation in a cache slot.
fn host_registry_generation() -> &'static AtomicU64 {
    static GENERATION: AtomicU64 = AtomicU64::new(1);
    &GENERATION
}

fn advance_host_registry_generation() {
    // Registry mutations are rare compared with dispatch. A wrapping token is
    // practically unreachable, and keeping the operation lock-free avoids
    // introducing another synchronization primitive on the mutation path.
    host_registry_generation().fetch_add(1, Ordering::AcqRel);
}

fn publish_host_cache(cache: &SpectraHostCallCache, generation: u64, function: *const ()) {
    // Publish the pointer first and the generation second. Readers acquire the
    // generation before consuming the pointer, which makes a matching cache
    // entry observe the complete publication.
    cache.function.store(function as *mut (), Ordering::Release);
    cache.generation.store(generation, Ordering::Release);
}

fn resolve_cached_host(cache: &SpectraHostCallCache, name: &str) -> *const () {
    let generation = host_registry_generation().load(Ordering::Acquire);
    let cached_generation = cache.generation.load(Ordering::Acquire);
    if cached_generation == generation {
        let function = cache.function.load(Ordering::Acquire);
        // Recheck the global token after loading the pointer. A mutation that
        // completed while the two loads were in flight must force the slow
        // path rather than allowing a completed mutation to leave a stale hit.
        if host_registry_generation().load(Ordering::Acquire) == generation {
            return function as *const ();
        }
    }

    let registry = host_registry();
    let guard = registry.lock().unwrap_or_else(|e| e.into_inner());
    let function = guard.lookup(name);
    let generation = host_registry_generation().load(Ordering::Acquire);
    drop(guard);
    publish_host_cache(cache, generation, function);
    function
}

/// Resolves one descriptor against a generation already sampled for the
/// current batch. A batch is a single ordered dispatch operation, so sharing
/// that generation avoids rereading the global token for every descriptor
/// while still taking the normal locked miss path when a slot is stale.
fn resolve_cached_host_for_batch(
    cache: &SpectraHostCallCache,
    name: &str,
    generation: u64,
) -> *const () {
    if cache.generation.load(Ordering::Acquire) == generation {
        return cache.function.load(Ordering::Acquire) as *const ();
    }
    resolve_cached_host(cache, name)
}

/// Borrows a UTF-8 host name from generated code without materializing a
/// `String`. The caller must ensure the pointed-to bytes remain valid for the
/// duration of the returned borrow. JIT name storage is owned by the code
/// generator and AOT names live in `.rodata`, so both satisfy this contract.
unsafe fn read_host_name<'a>(name_ptr: *const u8, name_len: usize) -> Option<&'a str> {
    if name_ptr.is_null() {
        return None;
    }

    let bytes = slice::from_raw_parts(name_ptr, name_len);
    str::from_utf8(bytes).ok()
}

/// Descriptor consumed by the internal hostcall batch dispatcher.
///
/// This is an internal runtime/backend ABI only. It is not exposed as a
/// Spectra-language value and deliberately uses the same scalar buffers as
/// `spectra_rt_host_invoke` so every individual host function sees the same
/// `SpectraHostCallContext` contract.
#[repr(C)]
#[derive(Clone, Copy)]
#[doc(hidden)]
pub struct SpectraHostBatchCall {
    pub name_ptr: *const u8,
    pub name_len: usize,
    pub args_ptr: *const SpectraHostValue,
    pub arg_len: usize,
    pub results_ptr: *mut SpectraHostValue,
    pub result_len: usize,
}

/// Descriptor consumed by the cache-aware internal hostcall batch
/// dispatcher. The cache pointer is owned by generated JIT/AOT module data;
/// the remaining fields intentionally match [`SpectraHostBatchCall`].
#[repr(C)]
#[derive(Clone, Copy)]
#[doc(hidden)]
pub struct SpectraHostCachedBatchCall {
    pub cache_ptr: *const SpectraHostCallCache,
    pub name_ptr: *const u8,
    pub name_len: usize,
    pub args_ptr: *const SpectraHostValue,
    pub arg_len: usize,
    pub results_ptr: *mut SpectraHostValue,
    pub result_len: usize,
}

fn invoke_host_function(
    func_ptr: *const (),
    args_ptr: *const SpectraHostValue,
    arg_len: usize,
    results_ptr: *mut SpectraHostValue,
    result_len: usize,
) -> i32 {
    if func_ptr.is_null() {
        return HOST_STATUS_NOT_FOUND;
    }

    let func: HostFunction = unsafe { mem::transmute(func_ptr) };
    let mut ctx = SpectraHostCallContext {
        args: args_ptr,
        arg_len,
        results: results_ptr,
        result_len,
        invoke_fn: Some(spectra_rt_invoke_closure),
    };

    match catch_unwind(AssertUnwindSafe(|| func(&mut ctx as *mut _))) {
        Ok(status) => status,
        Err(_) => HOST_STATUS_INTERNAL_ERROR,
    }
}

fn invoke_host_function_unchecked(
    func_ptr: *const (),
    args_ptr: *const SpectraHostValue,
    arg_len: usize,
    results_ptr: *mut SpectraHostValue,
    result_len: usize,
) -> i32 {
    if func_ptr.is_null() {
        return HOST_STATUS_NOT_FOUND;
    }

    let func: HostFunction = unsafe { mem::transmute(func_ptr) };
    let mut ctx = SpectraHostCallContext {
        args: args_ptr,
        arg_len,
        results: results_ptr,
        result_len,
        invoke_fn: Some(spectra_rt_invoke_closure),
    };
    func(&mut ctx as *mut _)
}

fn invoke_registered_host(
    name: &str,
    args_ptr: *const SpectraHostValue,
    arg_len: usize,
    results_ptr: *mut SpectraHostValue,
    result_len: usize,
) -> i32 {
    let registry = host_registry();
    let guard = registry.lock().unwrap_or_else(|e| e.into_inner());
    let func_ptr = guard.lookup(name);
    drop(guard);

    invoke_host_function(func_ptr, args_ptr, arg_len, results_ptr, result_len)
}

/// Registers a host function accessible to JITed code.
pub fn register_host_function(name: &str, func: HostFunction) -> bool {
    let registry = host_registry();
    let mut guard = registry.lock().unwrap_or_else(|e| e.into_inner());
    let inserted = guard.insert(name, func as *const ());
    advance_host_registry_generation();
    inserted
}

/// Removes a previously registered host function.
pub fn unregister_host_function(name: &str) -> bool {
    let registry = host_registry();
    let mut guard = registry.lock().unwrap_or_else(|e| e.into_inner());
    let removed = guard.remove(name);
    if removed {
        advance_host_registry_generation();
    }
    removed
}

/// Returns the host function pointer associated with the provided name.
pub fn lookup_host_function(name: &str) -> Option<HostFunction> {
    let registry = host_registry();
    let guard = registry.lock().unwrap_or_else(|e| e.into_inner());
    let ptr = guard.lookup(name);
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { mem::transmute::<*const (), HostFunction>(ptr) })
    }
}

/// Clears all registered host functions.
pub fn clear_host_functions() {
    let registry = host_registry();
    let mut guard = registry.lock().unwrap_or_else(|e| e.into_inner());
    guard.clear();
    advance_host_registry_generation();
}
