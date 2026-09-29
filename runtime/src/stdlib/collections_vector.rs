use super::*;
use std::alloc::Layout;
use std::mem::size_of;

const VECTOR_NEW: &str = "spectra.std.collections.vector_new";
const VECTOR_WITH_CAPACITY: &str = "spectra.std.collections.vector_with_capacity";
const VECTOR_CAPACITY: &str = "spectra.std.collections.vector_capacity";
const VECTOR_RESERVE: &str = "spectra.std.collections.vector_reserve";
const VECTOR_PUSH: &str = "spectra.std.collections.vector_push";
const VECTOR_POP: &str = "spectra.std.collections.vector_pop";
const VECTOR_GET: &str = "spectra.std.collections.vector_get";
const VECTOR_SET: &str = "spectra.std.collections.vector_set";
const VECTOR_INSERT_AT: &str = "spectra.std.collections.vector_insert_at";
const VECTOR_REMOVE_AT: &str = "spectra.std.collections.vector_remove_at";
const VECTOR_CONTAINS: &str = "spectra.std.collections.vector_contains";
const VECTOR_INDEX_OF: &str = "spectra.std.collections.vector_index_of";
const VECTOR_LEN: &str = "spectra.std.collections.vector_len";
const VECTOR_IS_EMPTY: &str = "spectra.std.collections.vector_is_empty";
const VECTOR_CLEAR: &str = "spectra.std.collections.vector_clear";
const VECTOR_ITER: &str = "spectra.std.collections.vector_iter";
const VECTOR_FREE: &str = "spectra.std.collections.vector_free";

pub(crate) fn register_vector_collection() {
    for (name, function) in [
        (VECTOR_NEW, std_vector_new as crate::ffi::HostFunction),
        (VECTOR_WITH_CAPACITY, std_vector_with_capacity),
        (VECTOR_CAPACITY, std_vector_capacity),
        (VECTOR_RESERVE, std_vector_reserve),
        (VECTOR_PUSH, std_vector_push),
        (VECTOR_POP, std_vector_pop),
        (VECTOR_GET, std_vector_get),
        (VECTOR_SET, std_vector_set),
        (VECTOR_INSERT_AT, std_vector_insert_at),
        (VECTOR_REMOVE_AT, std_vector_remove_at),
        (VECTOR_CONTAINS, std_vector_contains),
        (VECTOR_INDEX_OF, std_vector_index_of),
        (VECTOR_LEN, std_vector_len),
        (VECTOR_IS_EMPTY, std_vector_is_empty),
        (VECTOR_CLEAR, std_vector_clear),
        (VECTOR_ITER, std_vector_iter),
        (VECTOR_FREE, std_vector_free),
    ] {
        register_host_function(name, function);
    }
}

#[derive(Default)]
struct StdVector {
    data: Vec<SpectraHostValue>,
}

struct VectorRegistry {
    values: HandleTable<ManualBox<StdVector>>,
}

impl VectorRegistry {
    fn new() -> Self {
        Self {
            values: HandleTable::new(HandleKind::Vector),
        }
    }

    fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    fn insert(&mut self, value: ManualBox<StdVector>) -> usize {
        self.values.insert(value).raw() as usize
    }
}

fn vector_registry() -> &'static Mutex<VectorRegistry> {
    static REGISTRY: OnceLock<Mutex<VectorRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(VectorRegistry::new()))
}

fn with_vectors<F, R>(action: F) -> R
where
    F: FnOnce(&mut VectorRegistry) -> R,
{
    action(&mut lock_unpoisoned(vector_registry()))
}

fn with_vector_result(
    ctx: *mut SpectraHostCallContext,
    expected_args: usize,
    action: impl FnOnce(&[SpectraHostValue]) -> Result<SpectraHostValue, i32>,
) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, expected_args) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match action(args) {
        Ok(value) => {
            results[0] = value;
            HOST_STATUS_SUCCESS
        }
        Err(code) => code,
    }
}

fn with_vector_void(
    ctx: *mut SpectraHostCallContext,
    expected_args: usize,
    action: impl FnOnce(&[SpectraHostValue]) -> Result<(), i32>,
) -> i32 {
    let Ok(args) = host_call_void_args(ctx, expected_args) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    action(args).map_or_else(|code| code, |_| HOST_STATUS_SUCCESS)
}

fn checked_vector_capacity(raw: SpectraHostValue) -> Result<usize, i32> {
    let capacity = usize::try_from(raw).map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    let bytes = capacity
        .checked_mul(size_of::<SpectraHostValue>())
        .ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
    if bytes > isize::MAX as usize {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    Layout::array::<SpectraHostValue>(capacity).map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    Ok(capacity)
}

fn new_vector(capacity: usize) -> Result<SpectraHostValue, i32> {
    let mut vector = StdVector::default();
    vector
        .data
        .try_reserve_exact(capacity)
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    let boxed = initialize()
        .memory()
        .allocate_manual(vector)
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    Ok(with_vectors(|registry| registry.insert(boxed)) as SpectraHostValue)
}

pub(crate) extern "C" fn std_vector_new(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_result(ctx, 0, |_| new_vector(0))
}

pub(crate) extern "C" fn std_vector_with_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_result(ctx, 1, |args| new_vector(checked_vector_capacity(args[0])?))
}

pub(crate) extern "C" fn std_vector_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_result(ctx, 1, |args| {
        with_vectors(|registry| {
            let id = VectorRegistry::id(args[0] as usize)?;
            let vector = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            i64::try_from(vector.data.capacity()).map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

pub(crate) extern "C" fn std_vector_reserve(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_void(ctx, 2, |args| {
        let additional = checked_vector_capacity(args[1])?;
        with_vectors(|registry| {
            let id = VectorRegistry::id(args[0] as usize)?;
            let vector = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            let target = vector
                .data
                .len()
                .checked_add(additional)
                .ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
            Layout::array::<SpectraHostValue>(target).map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
            vector
                .data
                .try_reserve(additional)
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

pub(crate) extern "C" fn std_vector_push(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_void(ctx, 2, |args| {
        with_vectors(|registry| {
            let id = VectorRegistry::id(args[0] as usize)?;
            let vector = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            vector
                .data
                .try_reserve(1)
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
            crate::ffi::escape_stored_value(args[1]);
            vector.data.push(args[1]);
            Ok(())
        })
    })
}

pub(crate) fn vector_push_fast(handle: usize, value: SpectraHostValue) -> i32 {
    crate::ffi::escape_stored_value(value);
    with_vectors(|registry| {
        let id = VectorRegistry::id(handle)?;
        let vector = registry
            .values
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?;
        vector
            .data
            .try_reserve(1)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        vector.data.push(value);
        Ok(())
    })
    .map_or_else(|code| code, |_| HOST_STATUS_SUCCESS)
}

fn with_vector_option(
    ctx: *mut SpectraHostCallContext,
    expected_args: usize,
    action: impl FnOnce(&[SpectraHostValue]) -> Result<Option<SpectraHostValue>, i32>,
) -> i32 {
    let result = {
        let Ok(args) = host_call_void_args(ctx, expected_args) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        action(args)
    };
    match result {
        Ok(value) => unsafe { write_option_result(&mut *ctx, value) },
        Err(code) => code,
    }
}

pub(crate) extern "C" fn std_vector_pop(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_option(ctx, 1, |args| {
        with_vectors(|registry| {
            let id = VectorRegistry::id(args[0] as usize)?;
            let vector = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            Ok(vector.data.pop())
        })
    })
}

pub(crate) extern "C" fn std_vector_get(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_option(ctx, 2, |args| {
        let index = usize::try_from(args[1]).ok();
        with_vectors(|registry| {
            let id = VectorRegistry::id(args[0] as usize)?;
            let vector = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            Ok(index.and_then(|index| vector.data.get(index).copied()))
        })
    })
}

pub(crate) fn vector_get_fast(handle: usize, index: SpectraHostValue) -> SpectraHostValue {
    let value = usize::try_from(index).ok().and_then(|index| {
        with_vectors(|registry| -> Result<Option<SpectraHostValue>, i32> {
            let id = VectorRegistry::id(handle).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            let vector = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            Ok(vector.data.get(index).copied())
        })
        .unwrap_or(None)
    });
    collection_option_handle(value)
}

pub(crate) extern "C" fn std_vector_set(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_void(ctx, 3, |args| {
        let index = usize::try_from(args[1]).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        with_vectors(|registry| {
            let id = VectorRegistry::id(args[0] as usize)?;
            let vector = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            let slot = vector.data.get_mut(index).ok_or(HOST_STATUS_NOT_FOUND)?;
            crate::ffi::escape_stored_value(args[2]);
            *slot = args[2];
            Ok(())
        })
    })
}

pub(crate) extern "C" fn std_vector_insert_at(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_void(ctx, 3, |args| {
        let index = usize::try_from(args[1]).map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
        with_vectors(|registry| {
            let id = VectorRegistry::id(args[0] as usize)?;
            let vector = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            if index > vector.data.len() {
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            vector
                .data
                .try_reserve(1)
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
            crate::ffi::escape_stored_value(args[2]);
            vector.data.insert(index, args[2]);
            Ok(())
        })
    })
}

pub(crate) extern "C" fn std_vector_remove_at(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_option(ctx, 2, |args| {
        let index = usize::try_from(args[1]).ok();
        with_vectors(|registry| {
            let id = VectorRegistry::id(args[0] as usize)?;
            let vector = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            Ok(index
                .filter(|index| *index < vector.data.len())
                .map(|index| vector.data.remove(index)))
        })
    })
}

pub(crate) extern "C" fn std_vector_contains(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_result(ctx, 2, |args| {
        with_vectors(|registry| {
            let id = VectorRegistry::id(args[0] as usize)?;
            let vector = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            Ok(vector
                .data
                .iter()
                .copied()
                .any(|candidate| collection_values_equal(candidate, args[1]))
                as SpectraHostValue)
        })
    })
}

pub(crate) extern "C" fn std_vector_index_of(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_result(ctx, 2, |args| {
        with_vectors(|registry| {
            let id = VectorRegistry::id(args[0] as usize)?;
            let vector = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            vector
                .data
                .iter()
                .position(|candidate| collection_values_equal(*candidate, args[1]))
                .ok_or(HOST_STATUS_NOT_FOUND)
                .and_then(|index| i64::try_from(index).map_err(|_| HOST_STATUS_INTERNAL_ERROR))
        })
    })
}

pub(crate) extern "C" fn std_vector_len(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_result(ctx, 1, |args| {
        with_vectors(|registry| {
            let id = VectorRegistry::id(args[0] as usize)?;
            let vector = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            i64::try_from(vector.data.len()).map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

pub(crate) extern "C" fn std_vector_is_empty(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_result(ctx, 1, |args| {
        with_vectors(|registry| {
            let id = VectorRegistry::id(args[0] as usize)?;
            let vector = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            Ok(vector.data.is_empty() as SpectraHostValue)
        })
    })
}

pub(crate) extern "C" fn std_vector_clear(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_void(ctx, 1, |args| {
        with_vectors(|registry| {
            let id = VectorRegistry::id(args[0] as usize)?;
            let vector = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            vector.data.clear();
            Ok(())
        })
    })
}

pub(crate) extern "C" fn std_vector_iter(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_result(ctx, 1, |args| {
        let items = with_vectors(|registry| {
            let id = VectorRegistry::id(args[0] as usize)?;
            let vector = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            let mut snapshot = Vec::new();
            snapshot
                .try_reserve_exact(vector.data.len())
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
            snapshot.extend_from_slice(&vector.data);
            Ok::<Vec<SpectraHostValue>, i32>(snapshot)
        })?;
        Ok(insert_iterator(items)? as SpectraHostValue)
    })
}

pub(crate) extern "C" fn std_vector_free(ctx: *mut SpectraHostCallContext) -> i32 {
    with_vector_void(ctx, 1, |args| {
        with_vectors(|registry| {
            let id = VectorRegistry::id(args[0] as usize)?;
            registry
                .values
                .remove(id)
                .map(|_| ())
                .map_err(|_| HOST_STATUS_NOT_FOUND)
        })
    })
}
