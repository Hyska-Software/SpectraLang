use std::collections::{HashMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::{mem, ptr, slice, str};

use crate::abi::SpectraHostCallCache;
use crate::initialize;

// ── Program argument store ───────────────────────────────────────────────────

/// Program arguments forwarded from the host to Spectra code.
/// Set by the JIT runner before execution or by [`spectra_rt_startup_with_args`]
/// in AOT executables. Uses `OnceLock` so it can be set exactly once per process.
static PROGRAM_ARGV: OnceLock<Vec<String>> = OnceLock::new();

/// Returns the program arguments if they have been set, otherwise `None`.
pub(crate) fn get_program_args() -> Option<&'static Vec<String>> {
    PROGRAM_ARGV.get()
}

/// Sets the program arguments visible to `std.env` host functions.
/// Subsequent calls are silently ignored (can only be set once per process).
pub fn set_program_args(args: Vec<String>) {
    let _ = PROGRAM_ARGV.set(args);
}
use crate::memory::ManualBox;


struct ManualAllocation {
    frame_id: usize,
    /// Zero-initialised buffer of exactly the requested size. Statistics
    /// and `manual_allocation_size` report its true length.
    _storage: ManualBox<Vec<u8>>,
}

impl ManualAllocation {
    /// Byte length of the backing buffer (mirrors `manual_allocation_size`,
    /// usable while the table lock is already held).
    fn byte_len(&self) -> usize {
        self._storage.len()
    }
}

/// Maximum number of tombstones retained in the free quarantine (FIFO).
/// This bounds the stale-pointer detection window and its memory cost:
/// at most [`QUARANTINE_CAPACITY`] buffers stay resident-but-unreachable
/// at any time. Tune together with typical allocation churn.
pub(crate) const QUARANTINE_CAPACITY: usize = 64;

/// Tombstone left behind when a tracked manual allocation is freed.
///
/// The backing heap block is deliberately kept alive for the whole
/// quarantine window: while the tombstone exists, the system allocator
/// cannot hand the same address to `spectra_rt_manual_alloc` again, so a
/// stale pointer freed inside the window is reliably detected as
/// [`HOST_STATUS_INVALID_ARGUMENT`] instead of silently releasing an
/// unrelated live object (wrong-free). The buffer's live-statistics are
/// released at free time; only the raw block stays resident until the
/// entry is evicted from the FIFO.
struct QuarantineEntry {
    ptr_value: usize,
    /// Monotonic counter value at the moment the pointer was freed. Purely
    /// diagnostic today (FIFO eviction is order-based), but lets debug
    /// tooling reason about how long an address has been quarantined.
    freed_epoch: u64,
    /// Kept alive to pin the address; dropped on eviction.
    _storage: Vec<u8>,
}

struct Frame {
    id: usize,
    allocations: Vec<usize>,
    /// Maps a live pointer to its position in `allocations`, so removal is
    /// O(1). The table pins freed addresses in quarantine, therefore a live
    /// pointer can never appear twice in the same frame.
    index: HashMap<usize, usize>,
}

impl Frame {
    fn new(id: usize) -> Self {
        Self {
            id,
            allocations: Vec::new(),
            index: HashMap::new(),
        }
    }

    fn track(&mut self, ptr: usize) {
        self.index.insert(ptr, self.allocations.len());
        self.allocations.push(ptr);
    }

    fn untrack(&mut self, ptr: usize) {
        let Some(&pos) = self.index.get(&ptr) else {
            return;
        };
        self.allocations.swap_remove(pos);
        self.index.remove(&ptr);
        // `swap_remove` moved the last element into `pos`; fix its index.
        if let Some(&moved) = self.allocations.get(pos) {
            self.index.insert(moved, pos);
        }
    }
}

struct AllocationTable {
    allocations: HashMap<usize, ManualAllocation>,
    frames: Vec<Frame>,
    next_frame: usize,
    /// Freed-but-recently-live addresses (FIFO, bounded by
    /// [`QUARANTINE_CAPACITY`]). See [`QuarantineEntry`].
    quarantine: VecDeque<QuarantineEntry>,
    next_freed_epoch: u64,
}


impl AllocationTable {
    fn new() -> Self {
        Self {
            allocations: HashMap::new(),
            frames: vec![Frame::new(0)],
            next_frame: 1,
            quarantine: VecDeque::new(),
            next_freed_epoch: 0,
        }
    }

    fn push_frame(&mut self) -> usize {
        let id = self.next_frame;
        self.next_frame = self.next_frame.wrapping_add(1).max(1);
        self.frames.push(Frame::new(id));
        id
    }

    fn pop_frame(&mut self, frame_id: usize) -> Vec<usize> {
        // Pop frames from the top until we find the target frame, collecting
        // all allocations from every frame that we remove (including those
        // above the target).  This prevents leaks when frames are closed out
        // of order — which should not happen in well-formed code, but we
        // handle it defensively.
        let mut collected: Vec<usize> = Vec::new();
        while let Some(frame) = self.frames.last() {
            // Never remove the implicit base frame (id == 0).
            if frame.id == 0 {
                break;
            }
            let frame = self.frames.pop().unwrap();
            let found = frame.id == frame_id;
            collected.extend(frame.allocations);
            if found {
                return collected;
            }
        }
        // frame_id was not found — return whatever we collected so far
        // (callers will still free those allocations).
        collected
    }

    fn current_frame_mut(&mut self) -> Option<&mut Frame> {
        self.frames.last_mut()
    }

    fn remove_from_frame(&mut self, frame_id: usize, ptr: usize) {
        // Frame lookup stays linear: frame depth is the call-stack depth
        // (tiny). The per-frame membership test is O(1) via `Frame::index`,
        // so freeing no longer scans the whole live set of the frame.
        if let Some(frame) = self
            .frames
            .iter_mut()
            .rev()
            .find(|frame| frame.id == frame_id)
        {
            frame.untrack(ptr);
        }
    }

    fn clear_all(&mut self) {
        self.allocations.clear();
        self.frames.clear();
        self.frames.push(Frame::new(0));
        self.next_frame = 1;
        // A full clear also drops every tombstone: after
        // `spectra_rt_manual_clear` there is no live state left to protect,
        // and retaining pinned blocks across a reset would leak them for
        // the rest of the process.
        self.quarantine.clear();
        self.next_freed_epoch = 0;
    }

    /// Frees the tracked allocation at `ptr_value`, leaving a quarantine
    /// tombstone behind so the address cannot be reused (and a stale free
    /// of it is detected) for at least [`QUARANTINE_CAPACITY`] subsequent
    /// frees.
    ///
    /// Returns [`HOST_STATUS_SUCCESS`] on success or
    /// [`HOST_STATUS_INVALID_ARGUMENT`] when `ptr_value` is unknown or
    /// already quarantined (double free / stale pointer inside the window).
    pub(crate) fn free_tracked(&mut self, ptr_value: usize) -> i32 {
        let Some(entry) = self.allocations.remove(&ptr_value) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };

        // `into_inner` releases the live-statistics accounting while
        // keeping the heap block itself alive inside the tombstone, which
        // pins the address against reuse by `spectra_rt_manual_alloc`.
        let storage = entry._storage.into_inner();
        self.remove_from_frame(entry.frame_id, ptr_value);

        let freed_epoch = self.next_freed_epoch;
        self.next_freed_epoch = self.next_freed_epoch.wrapping_add(1);
        self.quarantine.push_back(QuarantineEntry {
            ptr_value,
            freed_epoch,
            _storage: storage,
        });
        self.evict_quarantine_overflow();

        HOST_STATUS_SUCCESS
    }

    /// Drops the oldest tombstones once the FIFO exceeds its capacity.
    ///
    /// After eviction the address becomes legally reusable. A stale free
    /// targeting an evicted address degrades to the generic unknown-address
    /// path (`HOST_STATUS_INVALID_ARGUMENT`): still a detectable error,
    /// never a silent wrong-free, but the double-free-specific signal is
    /// lost once the entry leaves the window.
    fn evict_quarantine_overflow(&mut self) {
        while self.quarantine.len() > QUARANTINE_CAPACITY {
            self.quarantine.pop_front();
        }
    }

    /// Current number of retained tombstones (bounded by
    /// [`QUARANTINE_CAPACITY`]).
    pub(crate) fn quarantine_len(&self) -> usize {
        self.quarantine.len()
    }

    fn check_invariants(&self) -> bool {
        if self.frames.first().map(|frame| frame.id) != Some(0) {
            return false;
        }

        let mut frame_ids = std::collections::HashSet::new();
        let mut frame_allocations = std::collections::HashSet::new();
        for frame in &self.frames {
            if !frame_ids.insert(frame.id) {
                return false;
            }
            for ptr in &frame.allocations {
                if !frame_allocations.insert(*ptr) {
                    return false;
                }
                match self.allocations.get(ptr) {
                    Some(allocation) if allocation.frame_id == frame.id => {}
                    _ => return false,
                }
            }
            // Per-frame removal index must mirror the allocation vector.
            if frame.index.len() != frame.allocations.len() {
                return false;
            }
            for (pos, ptr) in frame.allocations.iter().enumerate() {
                if frame.index.get(ptr) != Some(&pos) {
                    return false;
                }
            }
        }

        if !self.allocations.iter().all(|(ptr, allocation)| {
            frame_allocations.contains(ptr) && frame_ids.contains(&allocation.frame_id)
        }) {
            return false;
        }
        // newest (FIFO order).
        let mut previous_epoch = None;
        self.quarantine.iter().all(|entry| {
            if self.allocations.contains_key(&entry.ptr_value)
                || frame_allocations.contains(&entry.ptr_value)
            {
                return false;
            }
            match previous_epoch {
                Some(epoch) if entry.freed_epoch <= epoch => return false,
                _ => {}
            }
            previous_epoch = Some(entry.freed_epoch);
            true
        })
    }
}

/// Status recorded by the most recent `spectra_rt_manual_free` call.
///
/// The JIT import keeps its historical `void` signature (no ABI change), so
/// detection of invalid frees is out-of-band: callers and debug tooling read
/// this via `spectra_rt_manual_free_last_status`. Process-wide by design —
/// the allocation table itself is process-global.
static LAST_MANUAL_FREE_STATUS: AtomicI32 = AtomicI32::new(HOST_STATUS_SUCCESS);

pub(crate) fn record_manual_free_status(status: i32) {
    LAST_MANUAL_FREE_STATUS.store(status, Ordering::Release);
}

pub(crate) fn last_manual_free_status() -> i32 {
    LAST_MANUAL_FREE_STATUS.load(Ordering::Acquire)
}

fn allocation_table() -> &'static Mutex<AllocationTable> {
    static TABLE: OnceLock<Mutex<AllocationTable>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(AllocationTable::new()))
}

/// Primary scalar type exchanged through host call contexts.
pub type SpectraHostValue = i64;

/// Status codes returned by host functions.
pub const HOST_STATUS_SUCCESS: i32 = 0;
pub const HOST_STATUS_INVALID_ARGUMENT: i32 = 1;
pub const HOST_STATUS_NOT_FOUND: i32 = 2;
pub const HOST_STATUS_INTERNAL_ERROR: i32 = 3;

/// Context passed to host functions containing argument and result buffers.
#[repr(C)]
pub struct SpectraHostCallContext {
    pub args: *const SpectraHostValue,
    pub arg_len: usize,
    pub results: *mut SpectraHostValue,
    pub result_len: usize,
    /// Populated by the runtime dispatcher before invoking a host function.
    /// Allows host functions to call back into JIT-compiled Spectra closures
    /// (e.g., for higher-order functions like `list_map` and `list_filter`).
    ///
    /// Signature: `fn(fn_ptr: i64, args: *const i64, n_args: usize, result: *mut i64) -> i32`.
    /// Use [`spectra_rt_invoke_closure`] as the concrete implementation.
    /// `None` when the runtime does not support closure callbacks in this context.
    pub invoke_fn: Option<unsafe extern "C" fn(i64, *const i64, usize, *mut i64) -> i32>,
}

impl SpectraHostCallContext {
    /// Returns a slice view over the argument buffer.
    ///
    /// # Safety
    ///
    /// When `arg_len` is non-zero, `args` must be non-null, properly aligned,
    /// and point to `arg_len` readable values for the duration of the call.
    pub unsafe fn args_slice(&self) -> &[SpectraHostValue] {
        if self.args.is_null() || self.arg_len == 0 {
            &[]
        } else {
            slice::from_raw_parts(self.args, self.arg_len)
        }
    }

    /// Returns a mutable slice view over the result buffer.
    ///
    /// # Safety
    ///
    /// When `result_len` is non-zero, `results` must be non-null, properly
    /// aligned, and point to `result_len` writable values for the duration of
    /// the call.
    pub unsafe fn results_slice_mut(&mut self) -> &mut [SpectraHostValue] {
        if self.results.is_null() || self.result_len == 0 {
            &mut []
        } else {
            slice::from_raw_parts_mut(self.results, self.result_len)
        }
    }
}

/// Signature expected for runtime host functions.
pub type HostFunction = extern "C" fn(*mut SpectraHostCallContext) -> i32;
