use std::fmt;
use std::mem::size_of;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Configures the runtime memory manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryConfig {
    /// Maximum resident bytes charged to the manual heap before allocations
    /// start failing. This includes live values and tracked freed blocks held
    /// by the stale-pointer quarantine. A value of `0` disables the limit.
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
    SystemAllocationFailed { requested: usize },
}

impl AllocationError {
    /// Returns details about a manual-allocation soft limit overflow, if applicable.
    pub fn manual_limit_exceeded(&self) -> Option<(usize, usize)> {
        match self.kind {
            AllocationErrorKind::ManualLimitExceeded { requested, limit } => {
                Some((requested, limit))
            }
            AllocationErrorKind::SystemAllocationFailed { .. } => None,
        }
    }

    pub(crate) fn system_allocation_failed(&self) -> bool {
        matches!(
            self.kind,
            AllocationErrorKind::SystemAllocationFailed { .. }
        )
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
            AllocationErrorKind::SystemAllocationFailed { requested } => write!(
                f,
                "system allocator could not reserve {} bytes for the manual heap",
                requested
            ),
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
    /// `stats().manual.bytes` reflects the live payload size. Reservation
    /// failure is returned before a buffer is exposed to generated code.
    pub fn allocate_manual_bytes(&self, len: usize) -> Result<ManualBox<Vec<u8>>, AllocationError> {
        self.manual.register(len, &self.config)?;
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(len).is_err() {
            self.manual.release(len);
            return Err(AllocationError {
                kind: AllocationErrorKind::SystemAllocationFailed { requested: len },
            });
        }
        bytes.resize(len, 0);
        let boxed = Box::new(bytes);
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
                resident_bytes: AtomicUsize::new(0),
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
            self.inner.resident_bytes.fetch_add(size, Ordering::SeqCst);
            self.inner.live_bytes.fetch_add(size, Ordering::SeqCst);
            self.inner.live_allocations.fetch_add(1, Ordering::SeqCst);
            return Ok(());
        }

        let mut current = self.inner.resident_bytes.load(Ordering::SeqCst);
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

            match self.inner.resident_bytes.compare_exchange_weak(
                current,
                new_total,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    self.inner.live_bytes.fetch_add(size, Ordering::SeqCst);
                    self.inner.live_allocations.fetch_add(1, Ordering::SeqCst);
                    return Ok(());
                }
                Err(observed) => current = observed,
            }
        }
    }

    fn release(&self, size: usize) {
        self.release_live(size);
        self.release_resident(size);
    }

    /// Removes a value from live metrics while transferring its resident-byte
    /// charge to a `QuarantinedManualBox`.
    fn release_live_to_quarantine(&self, size: usize) {
        self.inner.live_allocations.fetch_sub(1, Ordering::SeqCst);
        if size > 0 {
            self.inner.live_bytes.fetch_sub(size, Ordering::SeqCst);
        }
    }

    fn release_live(&self, size: usize) {
        self.inner.live_allocations.fetch_sub(1, Ordering::SeqCst);
        if size > 0 {
            self.inner.live_bytes.fetch_sub(size, Ordering::SeqCst);
        }
    }

    fn release_resident(&self, size: usize) {
        if size > 0 {
            self.inner.resident_bytes.fetch_sub(size, Ordering::SeqCst);
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
    /// Bytes charged against the configured limit: live values plus blocks
    /// retained by the FFI stale-pointer quarantine.
    resident_bytes: AtomicUsize,
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
        let value = *boxed;
        self.heap.release(self.size);
        self.size = 0;
        value
    }

    /// Transfers a live allocation into a quarantine reservation. Live
    /// telemetry is released immediately, but its resident-byte charge stays
    /// until the returned wrapper is dropped after FIFO eviction.
    pub(crate) fn into_quarantine(mut self) -> QuarantinedManualBox<T> {
        let value = self.value.take().expect("manual allocation already taken");
        self.heap.release_live_to_quarantine(self.size);
        let quarantined = QuarantinedManualBox {
            value: Some(value),
            size: self.size,
            heap: self.heap.clone(),
        };
        self.size = 0;
        quarantined
    }
}

impl<T> Drop for ManualBox<T> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            drop(value);
            self.heap.release(self.size);
        }
    }
}

/// A freed allocation retained only to prevent pointer-address reuse. It no
/// longer contributes to live statistics but continues to consume the same
/// resident-byte budget as live allocations.
pub(crate) struct QuarantinedManualBox<T> {
    value: Option<Box<T>>,
    size: usize,
    heap: ManualHeap,
}

impl<T> QuarantinedManualBox<T> {
    pub(crate) fn tracked_size(&self) -> usize {
        self.size
    }
}

impl<T> Drop for QuarantinedManualBox<T> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            drop(value);
            self.heap.release_resident(self.size);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resident_limit_includes_quarantined_manual_allocations() {
        let memory = ManualMemory::with_config(MemoryConfig {
            manual_soft_limit_bytes: 64,
        });
        let live = memory
            .allocate_manual_bytes(48)
            .expect("allocation fits the resident-byte limit");
        let quarantined = live.into_quarantine();

        assert_eq!(memory.stats().manual, ManualStats::default());
        assert_eq!(quarantined.tracked_size(), 48);
        let error = memory
            .allocate_manual_bytes(17)
            .expect_err("live plus quarantined bytes must share the same limit");
        assert_eq!(error.manual_limit_exceeded(), Some((17, 64)));

        drop(quarantined);
        let full_budget = memory
            .allocate_manual_bytes(64)
            .expect("evicting the quarantine releases its resident-byte charge");
        assert_eq!(memory.stats().manual.bytes, 64);
        drop(full_budget);
        assert_eq!(memory.stats().manual, ManualStats::default());
    }

    #[test]
    fn impossible_byte_reservation_returns_error_and_releases_accounting() {
        let memory = ManualMemory::with_config(MemoryConfig {
            manual_soft_limit_bytes: 0,
        });
        let error = memory
            .allocate_manual_bytes(usize::MAX)
            .expect_err("capacity overflow must be reported without real exhaustion");
        assert!(error.system_allocation_failed());
        assert_eq!(memory.stats().manual, ManualStats::default());
    }
}
