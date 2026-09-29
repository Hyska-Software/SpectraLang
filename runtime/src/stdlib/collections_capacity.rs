use super::*;
use std::alloc::Layout;
use std::mem::size_of;

pub(crate) const LIST_WITH_CAPACITY: &str = "spectra.std.collections.list_with_capacity";
pub(crate) const LIST_RESERVE: &str = "spectra.std.collections.list_reserve";
pub(crate) const LIST_CAPACITY: &str = "spectra.std.collections.list_capacity";
pub(crate) const MAP_WITH_CAPACITY: &str = "spectra.std.collections.map_with_capacity";
pub(crate) const MAP_RESERVE: &str = "spectra.std.collections.map_reserve";
pub(crate) const MAP_CAPACITY: &str = "spectra.std.collections.map_capacity";
pub(crate) const SET_WITH_CAPACITY: &str = "spectra.std.collections.set_with_capacity";
pub(crate) const SET_RESERVE: &str = "spectra.std.collections.set_reserve";
pub(crate) const SET_CAPACITY: &str = "spectra.std.collections.set_capacity";
pub(crate) const STACK_WITH_CAPACITY: &str = "spectra.std.collections.stack_with_capacity";
pub(crate) const STACK_RESERVE: &str = "spectra.std.collections.stack_reserve";
pub(crate) const STACK_CAPACITY: &str = "spectra.std.collections.stack_capacity";
pub(crate) const QUEUE_WITH_CAPACITY: &str = "spectra.std.collections.queue_with_capacity";
pub(crate) const QUEUE_RESERVE: &str = "spectra.std.collections.queue_reserve";
pub(crate) const QUEUE_CAPACITY: &str = "spectra.std.collections.queue_capacity";

pub(crate) fn register_collection_capacity() {
    register_host_function(LIST_WITH_CAPACITY, std_list_with_capacity);
    register_host_function(LIST_RESERVE, std_list_reserve);
    register_host_function(LIST_CAPACITY, std_list_capacity);
    register_host_function(MAP_WITH_CAPACITY, std_map_with_capacity);
    register_host_function(MAP_RESERVE, std_map_reserve);
    register_host_function(MAP_CAPACITY, std_map_capacity);
    register_host_function(SET_WITH_CAPACITY, std_set_with_capacity);
    register_host_function(SET_RESERVE, std_set_reserve);
    register_host_function(SET_CAPACITY, std_set_capacity);
    register_host_function(STACK_WITH_CAPACITY, std_stack_with_capacity);
    register_host_function(STACK_RESERVE, std_stack_reserve);
    register_host_function(STACK_CAPACITY, std_stack_capacity);
    register_host_function(QUEUE_WITH_CAPACITY, std_queue_with_capacity);
    register_host_function(QUEUE_RESERVE, std_queue_reserve);
    register_host_function(QUEUE_CAPACITY, std_queue_capacity);
}

fn with_capacity_query(
    ctx: *mut SpectraHostCallContext,
    action: impl FnOnce(usize) -> Result<usize, i32>,
) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match action(args[0] as usize).and_then(|capacity| {
        SpectraHostValue::try_from(capacity).map_err(|_| HOST_STATUS_INTERNAL_ERROR)
    }) {
        Ok(capacity) => {
            results[0] = capacity;
            HOST_STATUS_SUCCESS
        }
        Err(code) => code,
    }
}

fn checked_capacity(raw: SpectraHostValue, element_size: usize) -> Result<usize, i32> {
    let capacity = usize::try_from(raw).map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    let bytes = capacity
        .checked_mul(element_size)
        .ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
    if bytes > isize::MAX as usize {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    Ok(capacity)
}

fn checked_reservation(
    len: usize,
    raw_additional: SpectraHostValue,
    element_size: usize,
) -> Result<usize, i32> {
    let additional = checked_capacity(raw_additional, element_size)?;
    let target = len
        .checked_add(additional)
        .ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
    Layout::array::<u8>(
        target
            .checked_mul(element_size)
            .ok_or(HOST_STATUS_INVALID_ARGUMENT)?,
    )
    .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    Ok(additional)
}

fn with_constructor_result(
    ctx: *mut SpectraHostCallContext,
    action: impl FnOnce(SpectraHostValue) -> Result<SpectraHostValue, i32>,
) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match action(args[0]) {
        Ok(handle) => {
            results[0] = handle;
            HOST_STATUS_SUCCESS
        }
        Err(code) => code,
    }
}

fn with_reserve_args(
    ctx: *mut SpectraHostCallContext,
    action: impl FnOnce(usize, SpectraHostValue) -> Result<(), i32>,
) -> i32 {
    let Ok(args) = host_call_void_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    action(args[0] as usize, args[1]).map_or_else(|code| code, |_| HOST_STATUS_SUCCESS)
}

pub(crate) extern "C" fn std_list_with_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_constructor_result(ctx, |raw| {
        let capacity = checked_capacity(raw, size_of::<SpectraHostValue>())?;
        let mut value = StdList::default();
        value
            .data
            .try_reserve_exact(capacity)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        let list = initialize()
            .memory()
            .allocate_manual(value)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        Ok(with_list_registry(|registry| registry.insert(list)) as SpectraHostValue)
    })
}

pub(crate) extern "C" fn std_list_reserve(ctx: *mut SpectraHostCallContext) -> i32 {
    with_reserve_args(ctx, |handle, raw| {
        with_list_registry(|registry| {
            let id = ListRegistry::id(handle)?;
            let list = registry
                .lists
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            let additional =
                checked_reservation(list.data.len(), raw, size_of::<SpectraHostValue>())?;
            list.data
                .try_reserve(additional)
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

pub(crate) extern "C" fn std_list_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_capacity_query(ctx, |handle| {
        with_list_registry(|registry| {
            let id = ListRegistry::id(handle)?;
            Ok(registry
                .lists
                .get(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?
                .data
                .capacity())
        })
    })
}

pub(crate) extern "C" fn std_map_with_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_constructor_result(ctx, |raw| {
        let capacity = checked_capacity(raw, size_of::<(CollectionKey, SpectraHostValue)>())?;
        let mut map = StdMap::default();
        map.data
            .try_reserve(capacity)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        Ok(
            with_map_registry(|registry| registry.insert(Arc::new(Mutex::new(map))))
                as SpectraHostValue,
        )
    })
}

pub(crate) extern "C" fn std_map_reserve(ctx: *mut SpectraHostCallContext) -> i32 {
    with_reserve_args(ctx, |handle, raw| {
        let map = map_fast_get(handle).ok_or(HOST_STATUS_NOT_FOUND)?;
        let mut map = lock_unpoisoned(&map);
        let additional = checked_reservation(
            map.data.len(),
            raw,
            size_of::<(CollectionKey, SpectraHostValue)>(),
        )?;
        map.data
            .try_reserve(additional)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)
    })
}

pub(crate) extern "C" fn std_map_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_capacity_query(ctx, |handle| {
        let map = map_fast_get(handle).ok_or(HOST_STATUS_NOT_FOUND)?;
        let capacity = lock_unpoisoned(&map).data.capacity();
        Ok(capacity)
    })
}

pub(crate) extern "C" fn std_set_with_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_constructor_result(ctx, |raw| {
        let capacity = checked_capacity(raw, size_of::<SpectraHostValue>())?;
        let mut value = StdSet::default();
        value
            .data
            .try_reserve_exact(capacity)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        let set = initialize()
            .memory()
            .allocate_manual(value)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        Ok(with_set_registry(|registry| registry.insert(set)) as SpectraHostValue)
    })
}

pub(crate) extern "C" fn std_set_reserve(ctx: *mut SpectraHostCallContext) -> i32 {
    with_reserve_args(ctx, |handle, raw| {
        with_set_registry(|registry| {
            let id = SetRegistry::id(handle)?;
            let set = registry
                .sets
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            let additional =
                checked_reservation(set.data.len(), raw, size_of::<SpectraHostValue>())?;
            set.data
                .try_reserve(additional)
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

pub(crate) extern "C" fn std_set_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_capacity_query(ctx, |handle| {
        with_set_registry(|registry| {
            let id = SetRegistry::id(handle)?;
            Ok(registry
                .sets
                .get(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?
                .data
                .capacity())
        })
    })
}

pub(crate) extern "C" fn std_stack_with_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_constructor_result(ctx, |raw| {
        let capacity = checked_capacity(raw, size_of::<SpectraHostValue>())?;
        let mut value = StdStack::default();
        value
            .data
            .try_reserve_exact(capacity)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        let stack = initialize()
            .memory()
            .allocate_manual(value)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        Ok(with_stack_registry(|registry| registry.insert(stack)) as SpectraHostValue)
    })
}

pub(crate) extern "C" fn std_stack_reserve(ctx: *mut SpectraHostCallContext) -> i32 {
    with_reserve_args(ctx, |handle, raw| {
        with_stack_registry(|registry| {
            let id = StackRegistry::id(handle)?;
            let stack = registry
                .stacks
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            let additional =
                checked_reservation(stack.data.len(), raw, size_of::<SpectraHostValue>())?;
            stack
                .data
                .try_reserve(additional)
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

pub(crate) extern "C" fn std_stack_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_capacity_query(ctx, |handle| {
        with_stack_registry(|registry| {
            let id = StackRegistry::id(handle)?;
            Ok(registry
                .stacks
                .get(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?
                .data
                .capacity())
        })
    })
}

pub(crate) extern "C" fn std_queue_with_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_constructor_result(ctx, |raw| {
        let capacity = checked_capacity(raw, size_of::<SpectraHostValue>())?;
        let mut value = StdQueue::default();
        value
            .data
            .try_reserve_exact(capacity)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        let queue = initialize()
            .memory()
            .allocate_manual(value)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        Ok(with_queue_registry(|registry| registry.insert(queue)) as SpectraHostValue)
    })
}

pub(crate) extern "C" fn std_queue_reserve(ctx: *mut SpectraHostCallContext) -> i32 {
    with_reserve_args(ctx, |handle, raw| {
        with_queue_registry(|registry| {
            let id = QueueRegistry::id(handle)?;
            let queue = registry
                .queues
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            let additional =
                checked_reservation(queue.data.len(), raw, size_of::<SpectraHostValue>())?;
            queue
                .data
                .try_reserve(additional)
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

pub(crate) extern "C" fn std_queue_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_capacity_query(ctx, |handle| {
        with_queue_registry(|registry| {
            let id = QueueRegistry::id(handle)?;
            Ok(registry
                .queues
                .get(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?
                .data
                .capacity())
        })
    })
}
