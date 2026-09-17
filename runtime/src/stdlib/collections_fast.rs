use super::*;

// These helpers are the collection-side implementation of the stable fast
// ABI. They deliberately reuse the checked generational registries so the
// direct JIT/AOT calls retain stale-handle isolation and ownership semantics.

#[inline]
fn option_from_result(value: Result<Option<SpectraHostValue>, i32>) -> SpectraHostValue {
    collection_option_handle(value.unwrap_or(None))
}

pub fn list_new_fast() -> SpectraHostValue {
    let list = match initialize().memory().allocate_manual(StdList::default()) {
        Ok(list) => list,
        Err(_) => return 0,
    };
    with_list_registry(|registry| registry.insert(list) as SpectraHostValue)
}

pub fn list_push_fast(handle: usize, value: SpectraHostValue) -> i32 {
    crate::ffi::escape_stored_value(value);
    match with_list_registry(|registry| registry.push(handle, value)) {
        Ok(_) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub fn list_len_fast(handle: usize) -> SpectraHostValue {
    with_list_registry(|registry| registry.len(handle))
        .map(|len| len as SpectraHostValue)
        .unwrap_or(0)
}

pub fn list_get_fast(handle: usize, index: SpectraHostValue) -> SpectraHostValue {
    option_from_result(with_list_registry(|registry| {
        registry.get_option(handle, index)
    }))
}

pub fn list_set_fast(handle: usize, index: SpectraHostValue, value: SpectraHostValue) -> i32 {
    crate::ffi::escape_stored_value(value);
    match with_list_registry(|registry| registry.set(handle, index, value)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub fn list_contains_fast(handle: usize, value: SpectraHostValue) -> SpectraHostValue {
    with_list_registry(|registry| registry.contains(handle, value))
        .map(SpectraHostValue::from)
        .unwrap_or(0)
}

pub fn list_clear_fast(handle: usize) -> i32 {
    match with_list_registry(|registry| registry.clear_list(handle)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub fn list_free_fast(handle: usize) -> i32 {
    match with_list_registry(|registry| registry.remove(handle)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub fn list_free_all_fast() -> SpectraHostValue {
    with_list_registry(|registry| registry.clear_all() as SpectraHostValue)
}

pub fn list_pop_fast(handle: usize) -> SpectraHostValue {
    option_from_result(with_list_registry(|registry| registry.pop_option(handle)))
}

pub fn list_pop_front_fast(handle: usize) -> SpectraHostValue {
    option_from_result(with_list_registry(|registry| {
        registry.pop_front_option(handle)
    }))
}

pub fn list_insert_at_fast(handle: usize, index: SpectraHostValue, value: SpectraHostValue) -> i32 {
    crate::ffi::escape_stored_value(value);
    match with_list_registry(|registry| registry.insert_at(handle, index, value)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub fn list_remove_at_fast(handle: usize, index: SpectraHostValue) -> SpectraHostValue {
    option_from_result(with_list_registry(|registry| {
        registry.remove_at_option(handle, index)
    }))
}

pub fn list_index_of_fast(handle: usize, value: SpectraHostValue) -> SpectraHostValue {
    with_list_registry(|registry| registry.index_of(handle, value)).unwrap_or(-1)
}

pub fn list_sort_fast(handle: usize) -> i32 {
    match with_list_registry(|registry| registry.sort_asc(handle)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub fn map_new_fast_collection() -> SpectraHostValue {
    map_new_fast()
}

pub fn map_set_fast_collection(
    handle: usize,
    key: SpectraHostValue,
    value: SpectraHostValue,
) -> i32 {
    map_set_fast(handle, key, value)
}

pub fn map_get_fast(handle: usize, key: SpectraHostValue) -> SpectraHostValue {
    option_from_result(with_map_registry(|registry| {
        registry.lookup_value(handle, key)
    }))
}

pub fn map_contains_fast_collection(handle: usize, key: SpectraHostValue) -> SpectraHostValue {
    map_contains_fast(handle, key)
}

pub fn map_remove_fast(handle: usize, key: SpectraHostValue) -> SpectraHostValue {
    option_from_result(with_map_registry(|registry| {
        registry.remove_value(handle, key)
    }))
}

pub fn map_len_fast_collection(handle: usize) -> SpectraHostValue {
    map_len_fast(handle)
}

pub fn map_is_empty_fast(handle: usize) -> SpectraHostValue {
    with_map_registry(|registry| registry.get(handle))
        .map(|map| lock_unpoisoned(&map).data.is_empty() as SpectraHostValue)
        .unwrap_or(0)
}

pub fn map_clear_fast_collection(handle: usize) -> i32 {
    map_clear_fast(handle);
    HOST_STATUS_SUCCESS
}

pub fn map_free_fast_collection(handle: usize) -> i32 {
    map_free_fast(handle);
    HOST_STATUS_SUCCESS
}

pub fn map_free_all_fast() -> SpectraHostValue {
    with_map_registry(|registry| registry.clear_all() as SpectraHostValue)
}

pub fn stack_new_fast() -> SpectraHostValue {
    let stack = match initialize().memory().allocate_manual(StdStack::default()) {
        Ok(stack) => stack,
        Err(_) => return 0,
    };
    with_stack_registry(|registry| registry.insert(stack) as SpectraHostValue)
}

pub fn stack_push_fast(handle: usize, value: SpectraHostValue) -> i32 {
    crate::ffi::escape_stored_value(value);
    match with_stack_registry(|registry| registry.push(handle, value)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub fn stack_pop_fast(handle: usize) -> SpectraHostValue {
    option_from_result(with_stack_registry(|registry| registry.pop(handle)))
}

pub fn stack_peek_fast(handle: usize) -> SpectraHostValue {
    option_from_result(with_stack_registry(|registry| registry.peek(handle)))
}

pub fn stack_len_fast(handle: usize) -> SpectraHostValue {
    with_stack_registry(|registry| registry.len(handle))
        .map(|len| len as SpectraHostValue)
        .unwrap_or(0)
}

pub fn stack_is_empty_fast(handle: usize) -> SpectraHostValue {
    with_stack_registry(|registry| registry.is_empty(handle))
        .map(SpectraHostValue::from)
        .unwrap_or(0)
}

pub fn stack_clear_fast(handle: usize) -> i32 {
    match with_stack_registry(|registry| registry.clear(handle)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub fn stack_free_fast(handle: usize) -> i32 {
    match with_stack_registry(|registry| registry.remove(handle)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub fn stack_free_all_fast() -> SpectraHostValue {
    with_stack_registry(|registry| registry.clear_all() as SpectraHostValue)
}

pub fn queue_new_fast() -> SpectraHostValue {
    let queue = match initialize().memory().allocate_manual(StdQueue::default()) {
        Ok(queue) => queue,
        Err(_) => return 0,
    };
    with_queue_registry(|registry| registry.insert(queue) as SpectraHostValue)
}

pub fn queue_enqueue_fast(handle: usize, value: SpectraHostValue) -> i32 {
    crate::ffi::escape_stored_value(value);
    match with_queue_registry(|registry| registry.enqueue(handle, value)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub fn queue_dequeue_fast(handle: usize) -> SpectraHostValue {
    option_from_result(with_queue_registry(|registry| registry.dequeue(handle)))
}

pub fn queue_peek_fast(handle: usize) -> SpectraHostValue {
    option_from_result(with_queue_registry(|registry| registry.peek(handle)))
}

pub fn queue_len_fast(handle: usize) -> SpectraHostValue {
    with_queue_registry(|registry| registry.len(handle))
        .map(|len| len as SpectraHostValue)
        .unwrap_or(0)
}

pub fn queue_is_empty_fast(handle: usize) -> SpectraHostValue {
    with_queue_registry(|registry| registry.is_empty(handle))
        .map(SpectraHostValue::from)
        .unwrap_or(0)
}

pub fn queue_clear_fast(handle: usize) -> i32 {
    match with_queue_registry(|registry| registry.clear(handle)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub fn queue_free_fast(handle: usize) -> i32 {
    match with_queue_registry(|registry| registry.remove(handle)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

pub fn queue_free_all_fast() -> SpectraHostValue {
    with_queue_registry(|registry| registry.clear_all() as SpectraHostValue)
}

pub fn iterator_next_fast(handle: usize) -> SpectraHostValue {
    option_from_result(with_iterator_registry(|registry| registry.next(handle)))
}

/// Returns the next iterator payload after the lowering has checked
/// `iterator_remaining() > 0`. This avoids materializing an intermediate
/// `Option<T>` on every collection loop iteration.
pub fn iterator_next_unchecked_fast(handle: usize) -> SpectraHostValue {
    match with_iterator_registry(|registry| registry.next(handle)) {
        Ok(Some(value)) => value,
        Ok(None) | Err(_) => 0,
    }
}

pub fn iterator_remaining_fast(handle: usize) -> SpectraHostValue {
    with_iterator_registry(|registry| registry.remaining(handle))
        .map(|remaining| remaining as SpectraHostValue)
        .unwrap_or(0)
}

pub fn iterator_free_fast(handle: usize) -> i32 {
    match with_iterator_registry(|registry| registry.remove(handle)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}
