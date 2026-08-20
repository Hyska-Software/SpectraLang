use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::mem::size_of;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Configures the runtime memory manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryConfig {
    /// Soft threshold for triggering a garbage collection cycle when the traced heap
    /// retains at least this many bytes. A value of `0` disables automatic collections.
    pub traced_collection_threshold_bytes: usize,
    /// Maximum number of bytes that may be tracked on the manual heap before allocations
    /// start failing. A value of `0` disables the limit and allows unbounded manual usage.
    pub manual_soft_limit_bytes: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            traced_collection_threshold_bytes: 4 * 1024 * 1024,
            manual_soft_limit_bytes: 32 * 1024 * 1024,
        }
    }
}

/// Aggregated memory usage information for the hybrid allocator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryStats {
    pub traced: TracedStats,
    pub manual: ManualStats,
}

/// Statistics for the traced heap segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TracedStats {
    pub allocations: usize,
    pub bytes: usize,
}

/// Statistics for the manual heap segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ManualStats {
    pub allocations: usize,
    pub bytes: usize,
}

/// Result of a garbage collection cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollectionOutcome {
    pub freed_allocations: usize,
    pub freed_bytes: usize,
    pub remaining_allocations: usize,
    pub remaining_bytes: usize,
    pub triggered_automatically: bool,
}

/// Error emitted when a memory allocation cannot be satisfied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocationError {
    kind: AllocationErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AllocationErrorKind {
    ManualLimitExceeded { requested: usize, limit: usize },
}

impl AllocationError {
    /// Returns details about a manual-allocation soft limit overflow, if applicable.
    pub fn manual_limit_exceeded(&self) -> Option<(usize, usize)> {
        match self.kind {
            AllocationErrorKind::ManualLimitExceeded { requested, limit } => {
                Some((requested, limit))
            }
        }
    }
}

impl fmt::Display for AllocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            AllocationErrorKind::ManualLimitExceeded { requested, limit } => {
                write!(
                    f,
                    "manual heap soft limit of {} bytes exceeded by {} bytes allocation",
                    limit, requested
                )
            }
        }
    }
}

impl std::error::Error for AllocationError {}

/// Primary entry point for Spectra's hybrid memory manager.
#[derive(Clone)]
pub struct HybridMemory {
    collector: Arc<Collector>,
    manual: ManualHeap,
    config: MemoryConfig,
}

impl fmt::Debug for HybridMemory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HybridMemory")
            .field("config", &self.config)
            .field("stats", &self.stats())
            .finish()
    }
}

impl HybridMemory {
    /// Creates a memory manager using the provided configuration.
    pub fn with_config(config: MemoryConfig) -> Self {
        Self {
            collector: Arc::new(Collector::new(config)),
            manual: ManualHeap::new(),
            config,
        }
    }

    /// Allocates a traced value on the managed heap. The returned handle does not
    /// keep the object alive; create a `GcRoot` to prevent collection when required.
    pub fn allocate_traced<T>(&self, value: T) -> Gc<T>
    where
        T: Trace + 'static,
    {
        let mut inner = self
            .collector
            .inner
            .lock()
            .expect("collector mutex poisoned");
        let id = inner.allocate(value);
        let should_collect = inner.should_collect();
        drop(inner);

        let handle = Gc::new(Arc::clone(&self.collector), id);

        if should_collect {
            self.collector.collect(true);
        }

        handle
    }

    /// Allocates a manually managed value. The caller assumes responsibility for
    /// holding on to the returned box and dropping or extracting it when finished.
    pub fn allocate_manual<T>(&self, value: T) -> Result<ManualBox<T>, AllocationError> {
        self.manual.allocate(value, &self.config)
    }

    /// Forces a full collection cycle and returns the outcome statistics.
    pub fn collect_garbage(&self) -> CollectionOutcome {
        self.collector.collect(false)
    }

    /// Retrieves the current memory usage statistics.
    pub fn stats(&self) -> MemoryStats {
        MemoryStats {
            traced: self.collector.stats(),
            manual: self.manual.stats(),
        }
    }

    /// Returns the configuration that initialised this memory manager.
    pub fn config(&self) -> MemoryConfig {
        self.config
    }
}

impl Default for HybridMemory {
    fn default() -> Self {
        Self::with_config(MemoryConfig::default())
    }
}

struct Collector {
    inner: Mutex<CollectorInner>,
}

impl Collector {
    fn new(config: MemoryConfig) -> Self {
        Self {
            inner: Mutex::new(CollectorInner {
                slots: Vec::new(),
                free_indices: Vec::new(),
                roots: HashMap::new(),
                stats: TracedStats::default(),
                next_root_id: 0,
                collection_threshold: config.traced_collection_threshold_bytes,
            }),
        }
    }

    fn register_root(&self, allocation: AllocationId) -> RootId {
        let mut inner = self.inner.lock().expect("collector mutex poisoned");
        if inner.slot(allocation).is_none() {
            panic!("attempted to root a collected or unknown allocation");
        }
        inner.register_root(allocation)
    }

    fn unregister_root(&self, root: RootId) {
        let mut inner = self.inner.lock().expect("collector mutex poisoned");
        inner.unregister_root(root);
    }

    fn stats(&self) -> TracedStats {
        let inner = self.inner.lock().expect("collector mutex poisoned");
        inner.stats
    }

    fn has_allocation(&self, allocation: AllocationId) -> bool {
        let inner = self.inner.lock().expect("collector mutex poisoned");
        inner.slot(allocation).is_some()
    }

    fn collect(&self, triggered_automatically: bool) -> CollectionOutcome {
        // SAFETY: The mutex is held for the entire mark-and-sweep cycle.
        // This is safe because Spectra is a single-threaded language — no
        // user code can run concurrently with a collection cycle, so no new
        // roots or inter-heap pointers can be created while the lock is held.
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.collect(triggered_automatically)
    }
}

struct CollectorInner {
    slots: Vec<Option<GcSlot>>,
    free_indices: Vec<usize>,
    roots: HashMap<RootId, AllocationId>,
    stats: TracedStats,
    next_root_id: u64,
    collection_threshold: usize,
}

impl CollectorInner {
    fn allocate<T>(&mut self, value: T) -> AllocationId
    where
        T: Trace + 'static,
    {
        let boxed = Box::new(GcBox { value });
        let size = boxed.size();
        let slot = GcSlot::new(boxed, size);

        let index = match self.free_indices.pop() {
            Some(index) => {
                self.slots[index] = Some(slot);
                index
            }
            None => {
                self.slots.push(Some(slot));
                self.slots.len() - 1
            }
        };

        self.stats.allocations = self.stats.allocations.saturating_add(1);
        self.stats.bytes = self.stats.bytes.saturating_add(size);
        AllocationId(index)
    }

    fn should_collect(&self) -> bool {
        self.collection_threshold != 0 && self.stats.bytes >= self.collection_threshold
    }

    fn register_root(&mut self, allocation: AllocationId) -> RootId {
        let root = RootId(self.next_root_id);
        self.next_root_id = self.next_root_id.wrapping_add(1);
        self.roots.insert(root, allocation);
        root
    }

    fn unregister_root(&mut self, root: RootId) {
        self.roots.remove(&root);
    }

    fn slot(&self, id: AllocationId) -> Option<&GcSlot> {
        self.slots.get(id.index()).and_then(|entry| entry.as_ref())
    }

    fn slot_mut(&mut self, id: AllocationId) -> Option<&mut GcSlot> {
        self.slots
            .get_mut(id.index())
            .and_then(|entry| entry.as_mut())
    }

    fn collect(&mut self, triggered_automatically: bool) -> CollectionOutcome {
        self.mark_from_roots();
        let (freed_allocations, freed_bytes) = self.sweep();

        CollectionOutcome {
            freed_allocations,
            freed_bytes,
            remaining_allocations: self.stats.allocations,
            remaining_bytes: self.stats.bytes,
            triggered_automatically,
        }
    }

    fn mark_from_roots(&mut self) {
        // We snapshot the root IDs before walking to avoid borrowing `self.roots`
        // while also calling `self.mark_allocation` through `&mut self`.
        // This is safe because:
        //   1. The caller (`collect`) holds the `CollectorInner` exclusively
        //      via `&mut self`, so no concurrent modification is possible.
        //   2. Spectra is a single-threaded runtime — no new GC roots are
        //      registered between the snapshot and the traversal.
        let mut stack = Vec::new();

        let roots: Vec<AllocationId> = self.roots.values().copied().collect();
        for root in roots {
            if self.mark_allocation(root) {
                stack.push(root);
            }
        }

        while let Some(current) = stack.pop() {
            let Some(slot) = self.slot(current) else {
                continue;
            };

            let mut edges = Vec::new();
            {
                let mut ctx = EdgeCollector { edges: &mut edges };
                slot.trace(&mut ctx);
            }

            for child in edges {
                if self.mark_allocation(child) {
                    stack.push(child);
                }
            }
        }
    }

    fn mark_allocation(&mut self, id: AllocationId) -> bool {
        match self.slot_mut(id) {
            Some(slot) if !slot.marked => {
                slot.marked = true;
                true
            }
            _ => false,
        }
    }

    fn sweep(&mut self) -> (usize, usize) {
        let mut freed_allocations: usize = 0;
        let mut freed_bytes: usize = 0;

        for (index, slot) in self.slots.iter_mut().enumerate() {
            if let Some(entry) = slot {
                if entry.marked {
                    entry.marked = false;
                } else {
                    freed_allocations += 1;
                    freed_bytes = freed_bytes.saturating_add(entry.size);
                    *slot = None;
                    self.free_indices.push(index);
                }
            }
        }

        if freed_allocations > 0 {
            self.stats.allocations = self.stats.allocations.saturating_sub(freed_allocations);
            self.stats.bytes = self.stats.bytes.saturating_sub(freed_bytes);
        }

        (freed_allocations, freed_bytes)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AllocationId(usize);

impl AllocationId {
    fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct RootId(u64);

struct GcSlot {
    payload: Box<dyn TraceObject>,
    marked: bool,
    size: usize,
}

impl GcSlot {
    fn new(payload: Box<dyn TraceObject>, size: usize) -> Self {
        Self {
            payload,
            marked: false,
            size,
        }
    }

    fn trace(&self, ctx: &mut dyn TraceContext) {
        self.payload.trace(ctx);
    }
}

trait TraceObject: Send + Sync {
    fn trace(&self, ctx: &mut dyn TraceContext);
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

struct GcBox<T>
where
    T: Trace + 'static,
{
    value: T,
}

impl<T> GcBox<T>
where
    T: Trace + 'static,
{
    fn size(&self) -> usize {
        size_of::<Self>()
    }
}

impl<T> TraceObject for GcBox<T>
where
    T: Trace + 'static,
{
    fn trace(&self, ctx: &mut dyn TraceContext) {
        self.value.trace(ctx);
    }

    fn as_any(&self) -> &dyn Any {
        &self.value
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.value
    }
}

/// Trait implemented by types that participate in tracing GC scans.
pub trait Trace: Send + Sync {
    fn trace(&self, ctx: &mut dyn TraceContext);
}

/// Collector callback interface used while traversing reachable allocations.
pub trait TraceContext {
    fn mark(&mut self, handle: &GcErased);
}

struct EdgeCollector<'a> {
    edges: &'a mut Vec<AllocationId>,
}

impl TraceContext for EdgeCollector<'_> {
    fn mark(&mut self, handle: &GcErased) {
        self.edges.push(handle.allocation_id());
    }
}

/// A dynamically typed GC handle used internally while tracing.
#[derive(Clone)]
pub struct GcErased {
    collector: Arc<Collector>,
    id: AllocationId,
}

impl GcErased {
    fn new(collector: Arc<Collector>, id: AllocationId) -> Self {
        Self { collector, id }
    }

    pub fn is_alive(&self) -> bool {
        self.collector.has_allocation(self.id)
    }

    pub(crate) fn allocation_id(&self) -> AllocationId {
        self.id
    }
}

pub struct Gc<T>
where
    T: Trace + 'static,
{
    collector: Arc<Collector>,
    id: AllocationId,
    marker: PhantomData<T>,
}

impl<T> Gc<T>
where
    T: Trace + 'static,
{
    fn new(collector: Arc<Collector>, id: AllocationId) -> Self {
        Self {
            collector,
            id,
            marker: PhantomData,
        }
    }

    pub fn erase(&self) -> GcErased {
        GcErased::new(Arc::clone(&self.collector), self.id)
    }

    pub fn try_borrow(&self) -> Option<GcBorrow<'_, T>> {
        let guard = self
            .collector
            .inner
            .lock()
            .expect("collector mutex poisoned");
        let value_ptr = {
            let slot = guard.slot(self.id)?;
            let value = slot.payload.as_any().downcast_ref::<T>()?;
            value as *const T
        };
        Some(GcBorrow {
            _guard: guard,
            value_ptr,
        })
    }

    pub fn borrow(&self) -> GcBorrow<'_, T> {
        self.try_borrow()
            .unwrap_or_else(|| panic!("attempted to borrow a collected allocation"))
    }

    pub fn try_borrow_mut(&self) -> Option<GcBorrowMut<'_, T>> {
        let mut guard = self
            .collector
            .inner
            .lock()
            .expect("collector mutex poisoned");
        let value_ptr = {
            let slot = guard.slot_mut(self.id)?;
            let value = slot.payload.as_any_mut().downcast_mut::<T>()?;
            value as *mut T
        };
        Some(GcBorrowMut {
            _guard: guard,
            value_ptr,
        })
    }

    pub fn borrow_mut(&self) -> GcBorrowMut<'_, T> {
        self.try_borrow_mut()
            .unwrap_or_else(|| panic!("attempted to mutably borrow a collected allocation"))
    }

    pub fn is_alive(&self) -> bool {
        self.collector.has_allocation(self.id)
    }

    pub fn into_root(&self) -> GcRoot<T> {
        let root = self.collector.register_root(self.id);
        GcRoot {
            handle: self.clone(),
            root,
        }
    }
}

impl<T> Clone for Gc<T>
where
    T: Trace + 'static,
{
    fn clone(&self) -> Self {
        Self {
            collector: Arc::clone(&self.collector),
            id: self.id,
            marker: PhantomData,
        }
    }
}

impl<T> Trace for Gc<T>
where
    T: Trace + 'static,
{
    fn trace(&self, ctx: &mut dyn TraceContext) {
        ctx.mark(&self.erase());
    }
}

pub struct GcBorrow<'a, T>
where
    T: Trace + 'static,
{
    _guard: MutexGuard<'a, CollectorInner>,
    value_ptr: *const T,
}

impl<'a, T> Deref for GcBorrow<'a, T>
where
    T: Trace + 'static,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: `value_ptr` was produced from a live allocation held behind
        // the mutex guard stored in this struct, guaranteeing it remains valid
        // for the guard's lifetime.
        unsafe { &*self.value_ptr }
    }
}

pub struct GcBorrowMut<'a, T>
where
    T: Trace + 'static,
{
    _guard: MutexGuard<'a, CollectorInner>,
    value_ptr: *mut T,
}

impl<'a, T> Deref for GcBorrowMut<'a, T>
where
    T: Trace + 'static,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.value_ptr }
    }
}

impl<'a, T> DerefMut for GcBorrowMut<'a, T>
where
    T: Trace + 'static,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.value_ptr }
    }
}

pub struct GcRoot<T>
where
    T: Trace + 'static,
{
    handle: Gc<T>,
    root: RootId,
}

impl<T> Drop for GcRoot<T>
where
    T: Trace + 'static,
{
    fn drop(&mut self) {
        self.handle.collector.unregister_root(self.root);
    }
}

impl<T> Deref for GcRoot<T>
where
    T: Trace + 'static,
{
    type Target = Gc<T>;

    fn deref(&self) -> &Self::Target {
        &self.handle
    }
}

impl<T> GcRoot<T>
where
    T: Trace + 'static,
{
    pub fn borrow(&self) -> GcBorrow<'_, T> {
        self.handle.borrow()
    }

    pub fn borrow_mut(&self) -> GcBorrowMut<'_, T> {
        self.handle.borrow_mut()
    }
}

#[derive(Clone)]
struct ManualHeap {
    inner: Arc<ManualHeapInner>,
}

impl ManualHeap {
    fn new() -> Self {
        Self {
            inner: Arc::new(ManualHeapInner {
                live_allocations: AtomicUsize::new(0),
                live_bytes: AtomicUsize::new(0),
            }),
        }
    }

    fn allocate<T>(
        &self,
        value: T,
        config: &MemoryConfig,
    ) -> Result<ManualBox<T>, AllocationError> {
        let size = size_of::<T>();
        self.register(size, config)?;
        let boxed = Box::new(value);
        Ok(ManualBox::new(boxed, size, self.clone()))
    }

    fn register(&self, size: usize, config: &MemoryConfig) -> Result<(), AllocationError> {
        let limit = config.manual_soft_limit_bytes;
        if limit == 0 {
            self.inner.live_bytes.fetch_add(size, Ordering::SeqCst);
            self.inner.live_allocations.fetch_add(1, Ordering::SeqCst);
            return Ok(());
        }

        let mut current = self.inner.live_bytes.load(Ordering::SeqCst);
        loop {
            let new_total = current.saturating_add(size);
            if new_total > limit {
                return Err(AllocationError {
                    kind: AllocationErrorKind::ManualLimitExceeded {
                        requested: size,
                        limit,
                    },
                });
            }

            match self.inner.live_bytes.compare_exchange_weak(
                current,
                new_total,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    self.inner.live_allocations.fetch_add(1, Ordering::SeqCst);
                    return Ok(());
                }
                Err(observed) => current = observed,
            }
        }
    }

    fn release(&self, size: usize) {
        self.inner.live_allocations.fetch_sub(1, Ordering::SeqCst);
        if size > 0 {
            self.inner.live_bytes.fetch_sub(size, Ordering::SeqCst);
        }
    }

    fn stats(&self) -> ManualStats {
        ManualStats {
            allocations: self.inner.live_allocations.load(Ordering::SeqCst),
            bytes: self.inner.live_bytes.load(Ordering::SeqCst),
        }
    }
}

struct ManualHeapInner {
    live_allocations: AtomicUsize,
    live_bytes: AtomicUsize,
}

/// Wrapper around manually managed allocations that keeps runtime statistics up-to-date.
pub struct ManualBox<T> {
    value: Option<Box<T>>,
    size: usize,
    heap: ManualHeap,
}

impl<T> ManualBox<T> {
    fn new(value: Box<T>, size: usize, heap: ManualHeap) -> Self {
        Self {
            value: Some(value),
            size,
            heap,
        }
    }

    pub fn into_inner(mut self) -> T {
        let boxed = self.value.take().expect("manual allocation already taken");
        self.heap.release(self.size);
        self.size = 0;
        *boxed
    }
}

impl<T> Drop for ManualBox<T> {
    fn drop(&mut self) {
        if self.value.is_some() {
            self.heap.release(self.size);
        }
    }
}

impl<T> Deref for ManualBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
            .as_deref()
            .expect("manual allocation already extracted")
    }
}

impl<T> DerefMut for ManualBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
            .as_deref_mut()
            .expect("manual allocation already extracted")
    }
}

impl<T> AsRef<T> for ManualBox<T> {
    fn as_ref(&self) -> &T {
        self.deref()
    }
}

impl<T> AsMut<T> for ManualBox<T> {
    fn as_mut(&mut self) -> &mut T {
        self.deref_mut()
    }
}

impl<T> fmt::Debug for ManualBox<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ManualBox").field(&self.deref()).finish()
    }
}

macro_rules! impl_trace_for_copy {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Trace for $ty {
                fn trace(&self, _ctx: &mut dyn TraceContext) {}
            }
        )*
    };
}

impl_trace_for_copy!(
    (),
    bool,
    char,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    f32,
    f64
);

include!("trace_impls.rs");
