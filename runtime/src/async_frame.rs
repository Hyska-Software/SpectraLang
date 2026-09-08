//! Private ownership and scheduling core for stackless asynchronous tasks.
//!
//! This module deliberately does not allocate host task handles.  Frames are
//! keyed by the `SpectraHostValue` allocated by `AsyncHandleTable<AsyncTask>`.

use crate::ffi::SpectraHostValue;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, ThreadId};

pub(crate) type AsyncPollFn = unsafe extern "C" fn(i64, i64, i64) -> i64;
pub(crate) type AsyncDropFn = unsafe extern "C" fn(i64, i64, i64);
type SlotDropFn = unsafe fn(SpectraHostValue);
type ResultDropFn = unsafe fn(SpectraHostValue);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum AsyncPollStatus {
    Pending = 0,
    Ready = 1,
    Failed = 2,
    Cancelled = 3,
}

impl AsyncPollStatus {
    pub(crate) fn from_abi(code: i64) -> Self {
        match code {
            0 => Self::Pending,
            1 => Self::Ready,
            2 => Self::Failed,
            3 => Self::Cancelled,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AsyncOwnedValue {
    Scalar(SpectraHostValue),
    Bytes(Vec<u8>),
    String(String),
    Aggregate(Vec<SpectraHostValue>),
    ProtocolHandle { domain: u16, value: SpectraHostValue },
}

impl AsyncOwnedValue {
    pub(crate) fn host_value(&self) -> Option<SpectraHostValue> {
        match self {
            Self::Scalar(value) | Self::ProtocolHandle { value, .. } => Some(*value),
            Self::Bytes(_) | Self::String(_) | Self::Aggregate(_) => None,
        }
    }
}

/// An owned typed result or error. Drop metadata is invoked exactly once.
pub(crate) struct AsyncResultStorage {
    value: AsyncOwnedValue,
    drop_glue: Option<ResultDropFn>,
    released: bool,
}

impl std::fmt::Debug for AsyncResultStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AsyncResultStorage")
            .field("value", &self.value)
            .field("has_drop_glue", &self.drop_glue.is_some())
            .field("released", &self.released)
            .finish()
    }
}

impl AsyncResultStorage {
    pub(crate) fn scalar(value: SpectraHostValue) -> Self {
        Self { value: AsyncOwnedValue::Scalar(value), drop_glue: None, released: false }
    }

    pub(crate) fn bytes(value: impl Into<Vec<u8>>) -> Self {
        Self { value: AsyncOwnedValue::Bytes(value.into()), drop_glue: None, released: false }
    }

    pub(crate) fn string(value: impl Into<String>) -> Self {
        Self { value: AsyncOwnedValue::String(value.into()), drop_glue: None, released: false }
    }

    pub(crate) fn aggregate(value: Vec<SpectraHostValue>) -> Self {
        Self { value: AsyncOwnedValue::Aggregate(value), drop_glue: None, released: false }
    }

    pub(crate) fn protocol_handle(
        domain: u16,
        value: SpectraHostValue,
        drop_glue: Option<ResultDropFn>,
    ) -> Self {
        Self { value: AsyncOwnedValue::ProtocolHandle { domain, value }, drop_glue, released: false }
    }

    pub(crate) fn scalar_with_drop(value: SpectraHostValue, drop_glue: ResultDropFn) -> Self {
        Self { value: AsyncOwnedValue::Scalar(value), drop_glue: Some(drop_glue), released: false }
    }

    pub(crate) fn as_value(&self) -> &AsyncOwnedValue { &self.value }
    pub(crate) fn host_value(&self) -> Option<SpectraHostValue> { self.value.host_value() }
}

impl Drop for AsyncResultStorage {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        if let (Some(drop_glue), Some(value)) = (self.drop_glue.take(), self.value.host_value()) {
            unsafe { drop_glue(value) };
        }
    }
}

pub(crate) struct AsyncFrameSlot {
    pub(crate) value: SpectraHostValue,
    initialized: bool,
    drop_glue: SlotDropFn,
}

impl AsyncFrameSlot {
    pub(crate) fn new(drop_glue: SlotDropFn) -> Self {
        Self { value: 0, initialized: false, drop_glue }
    }

    pub(crate) fn initialized(value: SpectraHostValue, drop_glue: SlotDropFn) -> Self {
        Self { value, initialized: true, drop_glue }
    }

    pub(crate) fn initialize(&mut self, value: SpectraHostValue) {
        self.drop_now();
        self.value = value;
        self.initialized = true;
    }

    pub(crate) fn is_initialized(&self) -> bool { self.initialized }

    pub(crate) fn drop_now(&mut self) {
        if !self.initialized {
            return;
        }
        self.initialized = false;
        let value = self.value;
        unsafe { (self.drop_glue)(value) };
    }
}

impl Drop for AsyncFrameSlot {
    fn drop(&mut self) { self.drop_now(); }
}

/// Runtime-owned context for a single generated poll invocation.
pub(crate) struct AsyncPollContext {
    result: Option<AsyncResultStorage>,
    error: Option<AsyncResultStorage>,
    woke: bool,
}

impl AsyncPollContext {
    pub(crate) fn new() -> Self { Self { result: None, error: None, woke: false } }
    pub(crate) fn set_result(&mut self, value: AsyncResultStorage) { self.result = Some(value); }
    pub(crate) fn set_error(&mut self, value: AsyncResultStorage) { self.error = Some(value); }
    pub(crate) fn wake_parent(&mut self) { self.woke = true; }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AsyncTaskState {
    Created,
    Polling,
    Pending,
    Ready,
    Failed,
    Cancelled,
    Dropped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AsyncAffinity {
    Any,
    Thread(ThreadId),
}

impl AsyncAffinity {
    pub(crate) fn current_thread() -> Self { Self::Thread(thread::current().id()) }
    fn allows(self) -> bool {
        matches!(self, Self::Any)
            || matches!(self, Self::Thread(id) if id == thread::current().id())
    }
}

struct AsyncFrameHeader {
    state_index: u32,
    drop_invoked: bool,
}

/// Private generated coroutine frame allocation.
pub(crate) struct AsyncFrame {
    header: AsyncFrameHeader,
    slots: Vec<AsyncFrameSlot>,
    poll: AsyncPollFn,
    drop: AsyncDropFn,
}

impl AsyncFrame {
    pub(crate) fn new(
        slots: Vec<AsyncFrameSlot>,
        poll: AsyncPollFn,
        drop: AsyncDropFn,
    ) -> Self {
        Self {
            header: AsyncFrameHeader { state_index: 0, drop_invoked: false },
            slots,
            poll,
            drop,
        }
    }

    /// Creates a frame owned by the codegen ABI before its callbacks are
    /// attached by `CoroutineCreate`.
    pub(crate) fn allocated(slot_count: usize) -> Self {
        unsafe fn no_drop(_: SpectraHostValue) {}
        unsafe extern "C" fn no_poll(_: i64, _: i64, _: i64) -> i64 {
            AsyncPollStatus::Failed as i64
        }
        unsafe extern "C" fn no_frame_drop(_: i64, _: i64, _: i64) {}
        Self::new(
            (0..slot_count).map(|_| AsyncFrameSlot::new(no_drop)).collect(),
            no_poll,
            no_frame_drop,
        )
    }

    pub(crate) fn set_callbacks(&mut self, poll: AsyncPollFn, drop: AsyncDropFn) {
        self.poll = poll;
        self.drop = drop;
    }

    pub(crate) fn state_index(&self) -> u32 { self.header.state_index }
    pub(crate) fn set_state_index(&mut self, state: u32) { self.header.state_index = state; }
    pub(crate) fn slots(&self) -> &[AsyncFrameSlot] { &self.slots }

    pub(crate) fn store_slot(&mut self, slot: usize, value: SpectraHostValue) -> bool {
        let Some(slot) = self.slots.get_mut(slot) else { return false };
        slot.initialize(value);
        true
    }

    pub(crate) fn load_slot(&self, slot: usize) -> Option<SpectraHostValue> {
        let slot = self.slots.get(slot)?;
        slot.is_initialized().then_some(slot.value)
    }

    pub(crate) fn pointer(&mut self) -> *mut c_void { (self as *mut AsyncFrame).cast::<c_void>() }

    pub(crate) unsafe fn invoke_poll(
        &mut self,
        task: SpectraHostValue,
        context: *mut AsyncPollContext,
    ) -> i64 {
        (self.poll)(self.pointer() as i64, task, context as i64)
    }

    unsafe fn invoke_drop(&mut self, task: SpectraHostValue, state: i32) {
        if self.header.drop_invoked {
            return;
        }
        self.header.drop_invoked = true;
        (self.drop)(self.pointer() as i64, task, state as i64);
        for slot in &mut self.slots {
            slot.drop_now();
        }
    }
}

struct ChildSubscription { child: SpectraHostValue }

#[derive(Default)]
struct WaiterState { notified: bool }


pub(crate) struct AsyncTaskWaiter {
    state: Arc<(Mutex<WaiterState>, Condvar)>,
}

impl AsyncTaskWaiter {
    pub(crate) fn wait(&self) {
        let (lock, signal) = &*self.state;
        let mut state = lock.lock().unwrap_or_else(|poison| poison.into_inner());
        while !state.notified {
            state = signal.wait(state).unwrap_or_else(|poison| poison.into_inner());
        }
    }

    pub(crate) fn is_notified(&self) -> bool {
        self.state.0.lock().unwrap_or_else(|poison| poison.into_inner()).notified
    }
}

struct FrameRecord {
    frame: Box<AsyncFrame>,
    state: AsyncTaskState,
    affinity: AsyncAffinity,
    polling: bool,
    drop_in_progress: bool,
    cancel_requested: bool,
    child: Option<ChildSubscription>,
    parents: Vec<SpectraHostValue>,
    waiters: Vec<Arc<(Mutex<WaiterState>, Condvar)>>,
    result: Option<AsyncResultStorage>,
    error: Option<AsyncResultStorage>,
}

impl FrameRecord {
    fn new(frame: AsyncFrame, affinity: AsyncAffinity) -> Self {
        Self::new_boxed(Box::new(frame), affinity)
    }

    fn new_boxed(frame: Box<AsyncFrame>, affinity: AsyncAffinity) -> Self {
        Self {
            frame,
            state: AsyncTaskState::Created,
            affinity,
            polling: false,
            drop_in_progress: false,
            cancel_requested: false,
            child: None,
            parents: Vec::new(),
            waiters: Vec::new(),
            result: None,
            error: None,
        }
    }
}

struct RegistryInner {
    frames: HashMap<SpectraHostValue, FrameRecord>,
    wake_queue: Vec<SpectraHostValue>,
}

impl RegistryInner {
    fn new() -> Self { Self { frames: HashMap::new(), wake_queue: Vec::new() } }
}


pub(crate) struct AsyncFramePoll {
    pub(crate) frame: *mut AsyncFrame,
    pub(crate) cancel_before_poll: bool,
}

pub(crate) struct AsyncFrameDrop {
    frame: Box<AsyncFrame>,
    task: SpectraHostValue,
    state: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum AsyncPollOutcome {
    Pending = 0,
    Ready = 1,
    Failed = 2,
    Cancelled = 3,
    AlreadyPolling = 4,
    Stale = 5,
    AffinityRejected = 6,
}

#[derive(Clone)]
pub(crate) struct AsyncFrameRegistry {
    inner: Arc<Mutex<RegistryInner>>,
}

impl Default for AsyncFrameRegistry {
    fn default() -> Self { Self::new() }
}

impl AsyncFrameRegistry {
    pub(crate) fn new() -> Self { Self { inner: Arc::new(Mutex::new(RegistryInner::new())) } }

    pub(crate) fn attach_frame(
        &self,
        task: SpectraHostValue,
        frame: AsyncFrame,
        affinity: AsyncAffinity,
    ) -> bool {
        self.attach_boxed_frame(task, Box::new(frame), affinity)
    }

    pub(crate) fn attach_boxed_frame(
        &self,
        task: SpectraHostValue,
        frame: Box<AsyncFrame>,
        affinity: AsyncAffinity,
    ) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        if inner.frames.contains_key(&task) {
            return false;
        }
        inner.frames.insert(task, FrameRecord::new_boxed(frame, affinity));
        true
    }

    pub(crate) fn contains(&self, task: SpectraHostValue) -> bool {
        self.inner.lock().unwrap_or_else(|poison| poison.into_inner()).frames.contains_key(&task)
    }

    pub(crate) fn state(&self, task: SpectraHostValue) -> Option<AsyncTaskState> {
        self.inner.lock().unwrap_or_else(|poison| poison.into_inner()).frames.get(&task).map(|record| record.state)
    }

    pub(crate) fn store_slot_ptr(
        &self,
        frame_ptr: i64,
        slot: usize,
        value: SpectraHostValue,
    ) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        inner.frames.values_mut().any(|record| {
            (record.frame.as_mut() as *mut AsyncFrame as i64 == frame_ptr)
                && record.frame.store_slot(slot, value)
        })
    }

    pub(crate) fn load_slot_ptr(&self, frame_ptr: i64, slot: usize) -> Option<SpectraHostValue> {
        let inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        inner
            .frames
            .values()
            .find(|record| record.frame.as_ref() as *const AsyncFrame as i64 == frame_ptr)
            .and_then(|record| record.frame.load_slot(slot))
    }

    pub(crate) fn state_ptr(&self, frame_ptr: i64) -> Option<u32> {
        let inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        inner
            .frames
            .values()
            .find(|record| record.frame.as_ref() as *const AsyncFrame as i64 == frame_ptr)
            .map(|record| record.frame.state_index())
    }

    pub(crate) fn store_state_ptr(&self, frame_ptr: i64, state: u32) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        let Some(record) = inner
            .frames
            .values_mut()
            .find(|record| record.frame.as_ref() as *const AsyncFrame as i64 == frame_ptr)
        else {
            return false;
        };
        record.frame.set_state_index(state);
        true
    }

    pub(crate) fn begin_poll(
        &self,
        task: SpectraHostValue,
    ) -> Result<AsyncFramePoll, AsyncPollOutcome> {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        let record = inner.frames.get_mut(&task).ok_or(AsyncPollOutcome::Stale)?;
        if !record.affinity.allows() {
            return Err(AsyncPollOutcome::AffinityRejected);
        }
        if record.polling {
            return Err(AsyncPollOutcome::AlreadyPolling);
        }
        match record.state {
            AsyncTaskState::Ready => return Err(AsyncPollOutcome::Ready),
            AsyncTaskState::Failed => return Err(AsyncPollOutcome::Failed),
            AsyncTaskState::Cancelled | AsyncTaskState::Dropped => {
                return Err(AsyncPollOutcome::Cancelled)
            }
            AsyncTaskState::Created | AsyncTaskState::Pending => {}
            AsyncTaskState::Polling => return Err(AsyncPollOutcome::AlreadyPolling),
        }
        record.polling = true;
        record.state = AsyncTaskState::Polling;
        let frame = record.frame.as_mut();
        Ok(AsyncFramePoll { frame: frame as *mut AsyncFrame, cancel_before_poll: record.cancel_requested })
    }

    pub(crate) fn finish_poll(
        &self,
        task: SpectraHostValue,
        callback_status: AsyncPollStatus,
        mut context: AsyncPollContext,
    ) -> Result<(AsyncPollOutcome, Option<AsyncFrameDrop>), AsyncPollOutcome> {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        let mut parents = Vec::new();
        let mut waiters = Vec::new();
        let mut child_to_unlink = None;
        let woke = context.woke;
        let (outcome, action) = {
            let record = inner.frames.get_mut(&task).ok_or(AsyncPollOutcome::Stale)?;
            record.polling = false;
            let status = if record.cancel_requested { AsyncPollStatus::Cancelled } else { callback_status };
            let outcome = match status {
                AsyncPollStatus::Pending => { record.state = AsyncTaskState::Pending; AsyncPollOutcome::Pending }
                AsyncPollStatus::Ready => { record.state = AsyncTaskState::Ready; AsyncPollOutcome::Ready }
                AsyncPollStatus::Failed => { record.state = AsyncTaskState::Failed; AsyncPollOutcome::Failed }
                AsyncPollStatus::Cancelled => { record.state = AsyncTaskState::Cancelled; AsyncPollOutcome::Cancelled }
            };
            if let Some(result) = context.result.take() {
                if status == AsyncPollStatus::Ready { record.result = Some(result); }
            }
            if let Some(error) = context.error.take() {
                if status == AsyncPollStatus::Failed { record.error = Some(error); }
            }
            let action = if !matches!(status, AsyncPollStatus::Pending)
                && !record.drop_in_progress
                && !record.frame.header.drop_invoked
            {
                record.drop_in_progress = true;
                let frame = std::mem::replace(
                    &mut record.frame,
                    Box::new(AsyncFrame::new(Vec::new(), dummy_poll, dummy_drop)),
                );
                Some(AsyncFrameDrop { frame, task, state: status as i32 })
            } else { None };
            if !matches!(status, AsyncPollStatus::Pending) {
                parents = std::mem::take(&mut record.parents);
                waiters = std::mem::take(&mut record.waiters);
                child_to_unlink = record.child.take().map(|subscription| subscription.child);
            }
            (outcome, action)
        };
        if woke && !inner.wake_queue.contains(&task) { inner.wake_queue.push(task); }
        for parent in parents {
            if !inner.wake_queue.contains(&parent) { inner.wake_queue.push(parent); }
        }
        if let Some(child) = child_to_unlink {
            if let Some(record) = inner.frames.get_mut(&child) {
                record.parents.retain(|parent| *parent != task);
            }
        }
        drop(inner);
        for waiter in waiters { notify_waiter(&waiter); }
        Ok((outcome, action))
    }

    pub(crate) fn complete_drop(&self, task: SpectraHostValue) {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(record) = inner.frames.get_mut(&task) { record.drop_in_progress = false; }
    }

    pub(crate) fn request_cancel(&self, task: SpectraHostValue) -> Result<(), bool> {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        let record = inner.frames.get_mut(&task).ok_or(false)?;
        if matches!(record.state, AsyncTaskState::Ready | AsyncTaskState::Failed | AsyncTaskState::Cancelled | AsyncTaskState::Dropped) {
            return Ok(());
        }
        record.cancel_requested = true;
        if !record.polling { record.state = AsyncTaskState::Cancelled; }
        Ok(())
    }

    pub(crate) fn take_cancel_drop(&self, task: SpectraHostValue) -> Result<Option<AsyncFrameDrop>, bool> {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        let record = inner.frames.get_mut(&task).ok_or(false)?;
        if record.polling || record.drop_in_progress || !record.cancel_requested { return Ok(None); }
        record.state = AsyncTaskState::Cancelled;
        if record.frame.header.drop_invoked { return Ok(None); }
        record.drop_in_progress = true;
        let frame = std::mem::replace(&mut record.frame, Box::new(AsyncFrame::new(Vec::new(), dummy_poll, dummy_drop)));
        Ok(Some(AsyncFrameDrop { frame, task, state: AsyncPollStatus::Cancelled as i32 }))
    }

    pub(crate) fn take_drop_task(&self, task: SpectraHostValue) -> Result<Option<AsyncFrameDrop>, bool> {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        let record = inner.frames.get(&task).ok_or(false)?;
        if record.polling || record.drop_in_progress { return Ok(None); }
        let record = inner.frames.remove(&task).ok_or(false)?;
        let mut frame = record.frame;
        if frame.header.drop_invoked { return Ok(None); }
        Ok(Some(AsyncFrameDrop {
            frame: std::mem::replace(&mut frame, Box::new(AsyncFrame::new(Vec::new(), dummy_poll, dummy_drop))),
            task,
            state: AsyncPollStatus::Cancelled as i32,

        }))
    }
    
    pub(crate) fn set_result(&self, task: SpectraHostValue, value: AsyncResultStorage) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        let Some(record) = inner.frames.get_mut(&task) else { return false; };
        record.result = Some(value);
        true
    }

    pub(crate) fn set_error(&self, task: SpectraHostValue, value: AsyncResultStorage) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        let Some(record) = inner.frames.get_mut(&task) else { return false; };
        record.error = Some(value);
        true
    }

    pub(crate) fn take_result(&self, task: SpectraHostValue) -> Option<Result<AsyncResultStorage, AsyncResultStorage>> {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        let record = inner.frames.get_mut(&task)?;
        match record.state {
            AsyncTaskState::Ready => record.result.take().map(Ok),
            AsyncTaskState::Failed => record.error.take().map(Err),
            _ => None,
        }
    }

    pub(crate) fn register_waiter(&self, task: SpectraHostValue) -> Option<AsyncTaskWaiter> {
        let waiter = AsyncTaskWaiter { state: Arc::new((Mutex::new(WaiterState::default()), Condvar::new())) };
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        let record = inner.frames.get_mut(&task)?;
        if matches!(record.state, AsyncTaskState::Ready | AsyncTaskState::Failed | AsyncTaskState::Cancelled | AsyncTaskState::Dropped) {
            notify_waiter(&waiter.state);
        } else {
            record.waiters.push(Arc::clone(&waiter.state));
        }
        Some(waiter)
    }
    pub(crate) fn take_all_drops(&self) -> Vec<AsyncFrameDrop> {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        let records = std::mem::take(&mut inner.frames);
        inner.wake_queue.clear();
        records
            .into_iter()
            .filter_map(|(task, record)| {
                if record.frame.header.drop_invoked { return None; }
                Some(AsyncFrameDrop {
                    frame: record.frame,
                    task,
                    state: AsyncPollStatus::Cancelled as i32,
                })
            })
            .collect()
    }

    pub(crate) fn subscribe_child(&self, parent: SpectraHostValue, child: SpectraHostValue) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        if parent == child || !inner.frames.contains_key(&parent) || !inner.frames.contains_key(&child) { return false; }
        let old = inner.frames.get_mut(&parent).and_then(|record| record.child.replace(ChildSubscription { child })).map(|subscription| subscription.child);
        if let Some(old) = old { if let Some(record) = inner.frames.get_mut(&old) { record.parents.retain(|value| *value != parent); } }
        if let Some(record) = inner.frames.get_mut(&child) {
            if !record.parents.contains(&parent) { record.parents.push(parent); }
            if matches!(record.state, AsyncTaskState::Ready | AsyncTaskState::Failed | AsyncTaskState::Cancelled | AsyncTaskState::Dropped) && !inner.wake_queue.contains(&parent) { inner.wake_queue.push(parent); }
            true
        } else { false }
    }

    pub(crate) fn wake(&self, task: SpectraHostValue) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        if !inner.frames.contains_key(&task) { return false; }
        if !inner.wake_queue.contains(&task) { inner.wake_queue.push(task); }
        let parents = inner.frames.get(&task).map(|record| record.parents.clone()).unwrap_or_default();
        for parent in parents { if !inner.wake_queue.contains(&parent) { inner.wake_queue.push(parent); } }
        true
    }

    pub(crate) fn result_host_value(&self, task: SpectraHostValue) -> Option<SpectraHostValue> {
        let inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        inner.frames.get(&task).and_then(|record| {
            record.result.as_ref().and_then(AsyncResultStorage::host_value)
        })
    }

    pub(crate) fn take_wakes(&self) -> Vec<SpectraHostValue> {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        std::mem::take(&mut inner.wake_queue)
    }

    pub(crate) fn clear(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|poison| poison.into_inner());
        inner.frames.clear();
        inner.wake_queue.clear();
    }

    /// Standalone helper retained for unit tests; integration uses begin/finish
    /// so the outer task-registry lock is not held during generated callbacks.
    #[allow(dead_code)]
    pub(crate) fn poll(&self, task: SpectraHostValue) -> AsyncPollOutcome {
        let invocation = match self.begin_poll(task) {
            Ok(invocation) => invocation,
            Err(outcome) => return outcome,
        };
        let mut context = AsyncPollContext::new();
        let status = if invocation.cancel_before_poll {
            AsyncPollStatus::Cancelled
        } else {
            AsyncPollStatus::from_abi(unsafe {
                (*invocation.frame).invoke_poll(task, &mut context)
            })
        };
        let (outcome, drop) = match self.finish_poll(task, status, context) {
            Ok(result) => result,
            Err(outcome) => return outcome,
        };
        if let Some(mut drop) = drop {
            unsafe { drop.frame.invoke_drop(drop.task, drop.state) };
            self.complete_drop(task);
        }
        outcome
    }
}
unsafe extern "C" fn dummy_poll(_: i64, _: i64, _: i64) -> i64 { 3 }
unsafe extern "C" fn dummy_drop(_: i64, _: i64, _: i64) {}

pub(crate) unsafe fn invoke_frame_drop(mut action: AsyncFrameDrop) {
    action.frame.invoke_drop(action.task, action.state);
}

fn notify_waiter(waiter: &Arc<(Mutex<WaiterState>, Condvar)>) {
    let (lock, signal) = &**waiter;
    let mut state = lock.lock().unwrap_or_else(|poison| poison.into_inner());
    state.notified = true;
    signal.notify_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static DROPS: AtomicUsize = AtomicUsize::new(0);
    unsafe extern "C" fn ready(_: i64, _: i64, context: i64) -> i64 {
        (*((context as *mut AsyncPollContext))).set_result(AsyncResultStorage::scalar(42));
        1
    }
    unsafe extern "C" fn drop_frame(_: i64, _: i64, _: i64) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
    unsafe fn drop_slot(_: SpectraHostValue) {}

    #[test]
    fn host_id_keyed_frame_lifecycle() {
        DROPS.store(0, Ordering::SeqCst);
        let store = AsyncFrameRegistry::new();
        assert!(store.attach_frame(6, AsyncFrame::new(vec![AsyncFrameSlot::new(drop_slot)], ready, drop_frame), AsyncAffinity::Any));
        assert_eq!(store.poll(6), AsyncPollOutcome::Ready);
        assert_eq!(store.take_result(6).and_then(|result| result.ok()).and_then(|value| value.host_value()), Some(42));
        assert_eq!(DROPS.load(Ordering::SeqCst), 1);
        assert_eq!(store.poll(6), AsyncPollOutcome::Ready);
    }

    #[test]
    fn typed_owned_values_are_not_scalar_reinterpreted() {
        let value = AsyncResultStorage::string("owned");
        assert!(matches!(value.as_value(), AsyncOwnedValue::String(text) if text == "owned"));
        let value = AsyncResultStorage::aggregate(vec![1, 2]);
        assert!(matches!(value.as_value(), AsyncOwnedValue::Aggregate(values) if values == &vec![1, 2]));
    }
}
