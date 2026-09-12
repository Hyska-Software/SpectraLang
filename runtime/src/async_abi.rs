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

#[no_mangle]
pub extern "C" fn spectra_rt_coroutine_poll_child(task: i64) -> i64 {
    match poll_coroutine_task(task) {
        Ok(AsyncPollOutcome::Pending) => AsyncPollStatus::Pending as i64,
        Ok(AsyncPollOutcome::Ready) => AsyncPollStatus::Ready as i64,
        Ok(AsyncPollOutcome::Failed | AsyncPollOutcome::AlreadyPolling) => {
            AsyncPollStatus::Failed as i64
        }
        Ok(AsyncPollOutcome::Cancelled) => AsyncPollStatus::Cancelled as i64,
        Ok(AsyncPollOutcome::Stale | AsyncPollOutcome::AffinityRejected) => {
            AsyncPollStatus::Failed as i64
        }
        Err(HOST_STATUS_NOT_FOUND) => match poll_task_once(task) {
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
        },
        Err(_) => AsyncPollStatus::Failed as i64,
    }
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
}
