use std::fmt;
use std::mem::size_of;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Configures the runtime memory manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryConfig {
    /// Maximum number of bytes that may be tracked on the manual heap before allocations
    /// start failing. A value of `0` disables the limit and allows unbounded manual usage.
    pub manual_soft_limit_bytes: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            manual_soft_limit_bytes: 32 * 1024 * 1024,
        }
    }
}

/// Aggregated memory usage information for the manual allocator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryStats {
    pub manual: ManualStats,
}

/// Statistics for the manual heap segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ManualStats {
    pub allocations: usize,
    pub bytes: usize,
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

/// Entry point for Spectra's manual memory manager.
///
/// Spectra values produced by generated code live on the manual heap: every
/// allocation returns a [`ManualBox`] that owns its value and keeps runtime
/// statistics up to date until it is dropped or extracted.
#[derive(Clone)]
pub struct ManualMemory {
    manual: ManualHeap,
    config: MemoryConfig,
}

impl fmt::Debug for ManualMemory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ManualMemory")
            .field("config", &self.config)
            .field("stats", &self.stats())
            .finish()
    }
}

impl ManualMemory {
    /// Creates a memory manager using the provided configuration.
    pub fn with_config(config: MemoryConfig) -> Self {
        Self {
            manual: ManualHeap::new(),
            config,
        }
    }

    /// Allocates a manually managed value. The caller assumes responsibility for
    /// holding on to the returned box and dropping or extracting it when finished.
    pub fn allocate_manual<T>(&self, value: T) -> Result<ManualBox<T>, AllocationError> {
        self.manual.allocate(value, &self.config)
    }

    /// Allocates a zero-initialised byte buffer of `len` bytes. Unlike the
    /// generic [`ManualMemory::allocate_manual`] (whose statistics can only
    /// record `size_of::<T>()`), this records the full buffer length, so
    /// `stats().manual.bytes` reflects the real payload size.
    pub fn allocate_manual_bytes(&self, len: usize) -> Result<ManualBox<Vec<u8>>, AllocationError> {
        self.manual.register(len, &self.config)?;
        let boxed = Box::new(vec![0u8; len]);
        Ok(ManualBox::new(boxed, len, self.manual.clone()))
    }

    /// Retrieves the current memory usage statistics.
    pub fn stats(&self) -> MemoryStats {
        MemoryStats {
            manual: self.manual.stats(),
        }
    }

    /// Returns the configuration that initialised this memory manager.
    pub fn config(&self) -> MemoryConfig {
        self.config
    }
}

impl Default for ManualMemory {
    fn default() -> Self {
        Self::with_config(MemoryConfig::default())
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
