use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
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

struct ManualRaw {
    bytes: Vec<u8>,
}

impl ManualRaw {
    fn new(size: usize) -> Self {
        Self {
            bytes: vec![0u8; size],
        }
    }

    fn ptr(&mut self) -> *mut u8 {
        self.bytes.as_mut_ptr()
    }
}

struct ManualAllocation {
    frame_id: usize,
    _storage: ManualBox<ManualRaw>,
}

struct Frame {
    id: usize,
    allocations: Vec<usize>,
}

struct AllocationTable {
    allocations: HashMap<usize, ManualAllocation>,
    frames: Vec<Frame>,
    next_frame: usize,
}

impl AllocationTable {
    fn new() -> Self {
        Self {
            allocations: HashMap::new(),
            frames: vec![Frame {
                id: 0,
                allocations: Vec::new(),
            }],
            next_frame: 1,
        }
    }

    fn push_frame(&mut self) -> usize {
        let id = self.next_frame;
        self.next_frame = self.next_frame.wrapping_add(1).max(1);
        self.frames.push(Frame {
            id,
            allocations: Vec::new(),
        });
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
        if let Some(frame) = self
            .frames
            .iter_mut()
            .rev()
            .find(|frame| frame.id == frame_id)
        {
            if let Some((index, _)) = frame
                .allocations
                .iter()
                .enumerate()
                .find(|(_, &stored)| stored == ptr)
            {
                frame.allocations.swap_remove(index);
            }
        }
    }

    fn clear_all(&mut self) {
        self.allocations.clear();
        self.frames.clear();
        self.frames.push(Frame {
            id: 0,
            allocations: Vec::new(),
        });
        self.next_frame = 1;
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
        }

        self.allocations.iter().all(|(ptr, allocation)| {
            frame_allocations.contains(ptr) && frame_ids.contains(&allocation.frame_id)
        })
    }
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
