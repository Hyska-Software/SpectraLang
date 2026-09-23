//! C ABI bridge used by generated coroutine ramps and poll functions.
//!
//! The ABI owns pending frame allocations until `CoroutineCreate`. Once a
//! frame is attached, the existing Async task handle owns it; pointer-based
//! slot/state operations resolve through the task registry instead of a second
//! frame-handle domain.

use crate::async_frame::{AsyncFrame, AsyncPollOutcome, AsyncPollStatus};
use crate::ffi::{
    SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT, HOST_STATUS_NOT_FOUND, HOST_STATUS_SUCCESS,
};
use crate::stdlib::{
    cancel_coroutine_task, create_coroutine_task_boxed, poll_coroutine_task, poll_task_once,
    set_coroutine_error, set_coroutine_result, subscribe_task_child, task_result_value,
    wake_coroutine_task,
};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

enum FrameOwner {
    Pending(Box<AsyncFrame>),
}

static FRAME_OWNERS: LazyLock<Mutex<HashMap<SpectraHostValue, FrameOwner>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn frames() -> &'static Mutex<HashMap<SpectraHostValue, FrameOwner>> {
    &FRAME_OWNERS
}
fn status(code: i32) -> i64 {
    code as i64
}

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_frame_alloc(slot_count: i64) -> i64 {
    let Ok(slot_count) = usize::try_from(slot_count) else {
        return 0;
    };
    if slot_count > 65_536 {
        return 0;
    }
    let frame = Box::new(AsyncFrame::allocated(slot_count));
    let ptr = (&*frame as *const AsyncFrame) as i64;
    let mut guard = frames().lock().unwrap_or_else(|poison| poison.into_inner());
    if guard.insert(ptr, FrameOwner::Pending(frame)).is_some() {
        return 0;
    }
    ptr
}

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_frame_store(frame_ptr: i64, slot: i64, value: i64) -> i64 {
    let Ok(slot) = usize::try_from(slot) else {
        return status(HOST_STATUS_INVALID_ARGUMENT);
    };
    let mut guard = frames().lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(FrameOwner::Pending(frame)) = guard.get_mut(&frame_ptr) {
        return status(if frame.store_slot(slot, value) {
            HOST_STATUS_SUCCESS
        } else {
            HOST_STATUS_INVALID_ARGUMENT
        });
    }
    drop(guard);
    let registry = match crate::stdlib::lock_async_task_registry() {
        Ok(registry) => registry.coroutine_frames.clone(),
        Err(status) => return status as i64,
    };
    status(if registry.store_slot_ptr(frame_ptr, slot, value) {
        HOST_STATUS_SUCCESS
    } else {
        HOST_STATUS_NOT_FOUND
    })
}

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_frame_load(frame_ptr: i64, slot: i64) -> i64 {
    let Ok(slot) = usize::try_from(slot) else {
        return -1;
    };
    let mut guard = frames().lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(FrameOwner::Pending(frame)) = guard.get_mut(&frame_ptr) {
        return frame.load_slot(slot).unwrap_or(0);
    }
    drop(guard);
    let registry = match crate::stdlib::lock_async_task_registry() {
        Ok(registry) => registry.coroutine_frames.clone(),
        Err(_) => return -1,
    };
    registry.load_slot_ptr(frame_ptr, slot).unwrap_or(-1)
}

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_state_load(frame_ptr: i64) -> i64 {
    let guard = frames().lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(FrameOwner::Pending(frame)) = guard.get(&frame_ptr) {
        return frame.state_index() as i64;
    }
    drop(guard);
    let registry = match crate::stdlib::lock_async_task_registry() {
        Ok(registry) => registry.coroutine_frames.clone(),
        Err(_) => return -1,
    };
    registry.state_ptr(frame_ptr).map(i64::from).unwrap_or(-1)
}

/// Returns durable per-frame storage for a promoted-local slot, allocating its
/// backing bytes on first use. The address stays valid across suspension until
/// the frame is dropped, so generated code may keep it in a frame slot.
#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_local_ptr(frame_ptr: i64, slot: i64, size: i64) -> i64 {
    let (Ok(slot), Ok(size)) = (usize::try_from(slot), usize::try_from(size)) else {
        return 0;
    };
    let mut guard = frames().lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(FrameOwner::Pending(frame)) = guard.get_mut(&frame_ptr) {
        return frame.local_ptr(slot, size).unwrap_or(0);
    }
    drop(guard);
    let registry = match crate::stdlib::lock_async_task_registry() {
        Ok(registry) => registry.coroutine_frames.clone(),
        Err(_) => return 0,
    };
    registry.local_ptr(frame_ptr, slot, size).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_state_store(frame_ptr: i64, state: i64) -> i64 {
    let Ok(state) = u32::try_from(state) else {
        return status(HOST_STATUS_INVALID_ARGUMENT);
    };
    let mut guard = frames().lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(FrameOwner::Pending(frame)) = guard.get_mut(&frame_ptr) {
        frame.set_state_index(state);
        return status(HOST_STATUS_SUCCESS);
    }
    drop(guard);
    let registry = match crate::stdlib::lock_async_task_registry() {
        Ok(registry) => registry.coroutine_frames.clone(),
        Err(status) => return status as i64,
    };
    status(if registry.store_state_ptr(frame_ptr, state) {
        HOST_STATUS_SUCCESS
    } else {
        HOST_STATUS_NOT_FOUND
    })
}

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_create(frame_ptr: i64, poll_ptr: i64, drop_ptr: i64) -> i64 {
    if frame_ptr == 0 || poll_ptr == 0 || drop_ptr == 0 {
        return 0;
    }
    let mut frame = {
        let mut guard = frames().lock().unwrap_or_else(|poison| poison.into_inner());
        match guard.remove(&frame_ptr) {
            Some(FrameOwner::Pending(frame)) => frame,
            None => return 0,
        }
    };
    let poll = unsafe {
        std::mem::transmute::<usize, unsafe extern "C" fn(i64, i64, i64) -> i64>(poll_ptr as usize)
    };
    let drop = unsafe {
        std::mem::transmute::<usize, unsafe extern "C" fn(i64, i64, i64)>(drop_ptr as usize)
    };
    frame.set_callbacks(poll, drop);
    create_coroutine_task_boxed(frame, None, crate::async_frame::AsyncAffinity::Any).unwrap_or(0)
}

/// Maps one `poll_coroutine_task` outcome to the status the parent's `await`
/// observes.
///
/// Extracted so the mapping is testable and so the two "someone else is inside
/// the child" outcomes cannot drift back into failures:
///
/// * `AlreadyPolling` — another context (the tool-dispatch worker drives the
///   same tree as the waiting caller) is inside the child right now. The parent
///   must wait for it: it is woken by the task's subscription, so reporting a
///   terminal failure here turns a healthy concurrent poll into a failed run.
/// * `AffinityRejected` — unreachable for coroutines created with
///   `AsyncAffinity::Any` (every frame this ABI creates), but a wrong-thread
///   poll must not be reported as a failure either.
///
/// `Stale` is the opposite case: the task is gone, so no wait can succeed.
fn child_poll_status(outcome: Result<AsyncPollOutcome, i32>) -> AsyncPollStatus {
    match outcome {
        Ok(AsyncPollOutcome::Ready) => AsyncPollStatus::Ready,
        Ok(AsyncPollOutcome::Pending | AsyncPollOutcome::AlreadyPolling) => {
            AsyncPollStatus::Pending
        }
        Ok(AsyncPollOutcome::Failed | AsyncPollOutcome::Stale) => AsyncPollStatus::Failed,
        Ok(AsyncPollOutcome::Cancelled) => AsyncPollStatus::Cancelled,
        Ok(AsyncPollOutcome::AffinityRejected) => AsyncPollStatus::Pending,
        Err(_) => AsyncPollStatus::Failed,
    }
}

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_poll_child(task: i64) -> i64 {
    let outcome = poll_coroutine_task(task);
    if let Err(HOST_STATUS_NOT_FOUND) = outcome {
        // Not a coroutine this registry owns: fall back to the scalar task
        // registry, which is the only other family of tasks in the table.
        return match poll_task_once(task) {
            Ok(true) => AsyncPollStatus::Ready as i64,
            Ok(false) => {
                let status = crate::stdlib::lock_async_task_registry()
                    .ok()
                    .and_then(|registry| {
                        registry.tasks.get(task).map(|task| {
                            if task.cancelled {
                                AsyncPollStatus::Cancelled
                            } else if task.failed {
                                AsyncPollStatus::Failed
                            } else {
                                AsyncPollStatus::Pending
                            }
                        })
                    })
                    .unwrap_or(AsyncPollStatus::Failed);
                status as i64
            }
            Err(_) => AsyncPollStatus::Failed as i64,
        };
    }
    // Name the outcome before the parent collapses it: without this line a
    // failed run says only that `block_on` failed, never which child failed or
    // why (see docs/architecture/agent-block-on-flake-known-failure.md).
    if matches!(
        outcome,
        Ok(AsyncPollOutcome::Failed | AsyncPollOutcome::Stale) | Err(_)
    ) {
        let outcome_name = match outcome {
            Ok(AsyncPollOutcome::Failed) => "failed",
            Ok(AsyncPollOutcome::Stale) => "stale",
            _ => "unavailable",
        };
        eprintln!("spectra.async.task.poll_child: task {task} outcome {outcome_name}");
    }
    child_poll_status(outcome) as i64
}

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_poll_result(task: i64) -> i64 {
    task_result_value(task).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_subscribe(task: i64, parent: i64) -> i64 {
    match subscribe_task_child(parent, task) {
        Ok(true) => status(HOST_STATUS_SUCCESS),
        Ok(false) => status(HOST_STATUS_NOT_FOUND),
        Err(code) => status(code),
    }
}

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_wake(task: i64) -> i64 {
    match wake_coroutine_task(task) {
        Ok(true) => status(HOST_STATUS_SUCCESS),
        Ok(false) => status(HOST_STATUS_NOT_FOUND),
        Err(code) => status(code),
    }
}

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_suspend(_task: i64, _state: i64) -> i64 {
    status(HOST_STATUS_SUCCESS)
}

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_complete(task: i64, value: i64) -> i64 {
    match set_coroutine_result(task, crate::async_frame::AsyncResultStorage::scalar(value)) {
        Ok(true) => status(HOST_STATUS_SUCCESS),
        Ok(false) => status(HOST_STATUS_NOT_FOUND),
        Err(code) => status(code),
    }
}

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_error(task: i64, error: i64) -> i64 {
    match set_coroutine_error(task, crate::async_frame::AsyncResultStorage::scalar(error)) {
        Ok(true) => status(HOST_STATUS_SUCCESS),
        Ok(false) => status(HOST_STATUS_NOT_FOUND),
        Err(code) => status(code),
    }
}

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_cancelled(task: i64) -> i64 {
    match cancel_coroutine_task(task) {
        Ok(true) => status(HOST_STATUS_SUCCESS),
        Ok(false) => status(HOST_STATUS_NOT_FOUND),
        Err(code) => status(code),
    }
}

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_poll_return(_status: i64) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_abi_validates_slots_and_stale_pointers() {
        let _guard = crate::runtime_test_guard();
        let frame = spectra_rt_coroutine_frame_alloc(1);
        assert_ne!(frame, 0);
        assert_eq!(spectra_rt_coroutine_frame_store(frame, 0, 0x1234), 0);
        assert_eq!(spectra_rt_coroutine_frame_load(frame, 0), 0x1234);
        assert_eq!(
            spectra_rt_coroutine_frame_store(frame, 1, 1),
            HOST_STATUS_INVALID_ARGUMENT as i64
        );
        assert_eq!(spectra_rt_coroutine_frame_load(0, 0), -1);
    }

    unsafe extern "C" fn ready_poll(frame_ptr: i64, task: i64, _: i64) -> i64 {
        assert_eq!(spectra_rt_coroutine_frame_load(frame_ptr, 0), 7);
        assert_eq!(spectra_rt_coroutine_complete(task, 42), 0);
        1
    }
    unsafe extern "C" fn ready_drop(_: i64, _: i64, _: i64) {}

    #[test]
    fn attached_frame_keeps_pointer_identity_for_generated_operations() {
        let _guard = crate::runtime_test_guard();
        let frame = spectra_rt_coroutine_frame_alloc(1);
        assert_eq!(spectra_rt_coroutine_frame_store(frame, 0, 7), 0);
        let task = spectra_rt_coroutine_create(
            frame,
            ready_poll as *const () as usize as i64,
            ready_drop as *const () as usize as i64,
        );
        assert_ne!(task, 0);
        assert_eq!(spectra_rt_coroutine_frame_load(frame, 0), 7);
        assert_eq!(spectra_rt_coroutine_state_store(frame, 3), 0);
        assert_eq!(spectra_rt_coroutine_state_load(frame), 3);
        assert_eq!(spectra_rt_coroutine_poll_child(task), 1);
        assert_eq!(spectra_rt_coroutine_poll_result(task), 42);
        assert!(crate::stdlib::drop_coroutine_task(task).expect("drop task"));
        assert_eq!(spectra_rt_coroutine_frame_load(frame, 0), -1);
    }

    /// Freed frame addresses stay pinned in a bounded quarantine, so a newly
    /// created frame cannot reuse an address a stale raw frame pointer may
    /// still reference (the ABI carries no nonce; pinning is the strongest
    /// runtime-only ABA guard available).
    #[test]
    fn freed_frame_addresses_are_quarantined_against_immediate_reuse() {
        let _guard = crate::runtime_test_guard();
        let frame = spectra_rt_coroutine_frame_alloc(1);
        assert_ne!(frame, 0);
        assert_eq!(spectra_rt_coroutine_frame_store(frame, 0, 7), 0);
        let task = spectra_rt_coroutine_create(
            frame,
            ready_poll as *const () as usize as i64,
            ready_drop as *const () as usize as i64,
        );
        assert_ne!(task, 0);
        assert!(crate::stdlib::drop_coroutine_task(task).expect("drop task"));

        // The stale raw pointer no longer resolves to a live frame...
        assert_eq!(spectra_rt_coroutine_frame_load(frame, 0), -1);
        assert_eq!(spectra_rt_coroutine_state_load(frame), -1);

        // ...and its address is reserved: `Box<AsyncFrame>` has a fixed size,
        // so without the quarantine the allocator could hand the very same
        // address back and the stale pointer would alias the new frame.
        let replacement = spectra_rt_coroutine_frame_alloc(1);
        assert_ne!(
            replacement, frame,
            "a freed frame address must be quarantined, not reused"
        );
    }

    #[test]
    fn child_poll_status_maps_every_outcome() {
        // The mapping is what the parent's `await` observes. `AlreadyPolling`
        // and `AffinityRejected` are contention, not failure: a background tool
        // worker drives the same task tree as the waiting caller, so treating
        // them as terminal turns a healthy concurrent poll into a failed run
        // (`docs/architecture/agent-block-on-flake-known-failure.md`).
        assert_eq!(
            child_poll_status(Ok(AsyncPollOutcome::Ready)),
            AsyncPollStatus::Ready
        );
        assert_eq!(
            child_poll_status(Ok(AsyncPollOutcome::Pending)),
            AsyncPollStatus::Pending
        );
        assert_eq!(
            child_poll_status(Ok(AsyncPollOutcome::Failed)),
            AsyncPollStatus::Failed
        );
        assert_eq!(
            child_poll_status(Ok(AsyncPollOutcome::Cancelled)),
            AsyncPollStatus::Cancelled
        );
        assert_eq!(
            child_poll_status(Ok(AsyncPollOutcome::AlreadyPolling)),
            AsyncPollStatus::Pending
        );
        assert_eq!(
            child_poll_status(Ok(AsyncPollOutcome::Stale)),
            AsyncPollStatus::Failed
        );
        assert_eq!(
            child_poll_status(Ok(AsyncPollOutcome::AffinityRejected)),
            AsyncPollStatus::Pending
        );
        assert_eq!(
            child_poll_status(Err(HOST_STATUS_NOT_FOUND)),
            AsyncPollStatus::Failed
        );
    }

    #[test]
    fn concurrent_polls_report_contention_not_failure() {
        // The shape the flake came from: a background tool worker and the
        // waiting caller touch the same task tree, so one of them can arrive
        // while the other is inside the frame. Pre-fix that collision was
        // mapped to `Failed`; here it must stay contention.
        use std::sync::atomic::{AtomicUsize, Ordering};
        static POLLS: AtomicUsize = AtomicUsize::new(0);
        static CONTENTION: AtomicUsize = AtomicUsize::new(0);
        static CONTENTION_CHECKED: AtomicUsize = AtomicUsize::new(0);

        unsafe extern "C" fn slow_poll(_: i64, _: i64, _: i64) -> i64 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            if POLLS.fetch_add(1, Ordering::SeqCst) + 1 >= 40 {
                return AsyncPollStatus::Ready as i64;
            }
            AsyncPollStatus::Pending as i64
        }
        unsafe extern "C" fn slow_drop(_: i64, _: i64, _: i64) {}

        let _guard = crate::runtime_test_guard();
        POLLS.store(0, Ordering::SeqCst);
        CONTENTION.store(0, Ordering::SeqCst);
        CONTENTION_CHECKED.store(0, Ordering::SeqCst);
        let frame = spectra_rt_coroutine_frame_alloc(0);
        assert_ne!(frame, 0);
        let task = spectra_rt_coroutine_create(
            frame,
            slow_poll as *const () as usize as i64,
            slow_drop as *const () as usize as i64,
        );
        assert_ne!(task, 0);

        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for _ in 0..40 {
                        match crate::stdlib::poll_coroutine_task(task) {
                            Ok(AsyncPollOutcome::AlreadyPolling) => {
                                CONTENTION.fetch_add(1, Ordering::SeqCst);
                                // The raw outcome says another thread is inside
                                // the child; the status this ABI hands the
                                // parent's `await` must not call that failure.
                                let mapped = spectra_rt_coroutine_poll_child(task);
                                CONTENTION_CHECKED.fetch_add(1, Ordering::SeqCst);
                                assert_ne!(
                                    mapped,
                                    AsyncPollStatus::Failed as i64,
                                    "a contended poll was reported as a failure"
                                );
                            }
                            Ok(AsyncPollOutcome::Failed) => {
                                panic!("a concurrent poll reported failure")
                            }
                            _ => {}
                        }
                    }
                });
            }
        });

        // The test is only meaningful if the threads actually collided.
        assert!(
            CONTENTION.load(Ordering::SeqCst) > 0,
            "no thread observed contention; the collision never happened"
        );
        assert!(
            CONTENTION_CHECKED.load(Ordering::SeqCst) > 0,
            "contention was observed but never mapped through the ABI"
        );
        assert!(crate::stdlib::drop_coroutine_task(task).expect("drop task"));
    }

    #[test]
    fn child_poll_status_keeps_contention_pending() {
        // Direct check of the mapping the ABI applies to the outcomes above.
        for outcome in [AsyncPollOutcome::AlreadyPolling, AsyncPollOutcome::AffinityRejected] {
            assert_eq!(child_poll_status(Ok(outcome)), AsyncPollStatus::Pending);
        }
    }
}
