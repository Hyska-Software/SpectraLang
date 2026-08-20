// ── std.collections set/iterator ────────────────────────────────────────────

const SET_NEW: &str = "spectra.std.collections.set_new";
const SET_INSERT: &str = "spectra.std.collections.set_insert";
const SET_CONTAINS: &str = "spectra.std.collections.set_contains";
const SET_REMOVE: &str = "spectra.std.collections.set_remove";
const SET_LEN: &str = "spectra.std.collections.set_len";
const SET_GET: &str = "spectra.std.collections.set_get";
const SET_CLEAR: &str = "spectra.std.collections.set_clear";
const SET_FREE: &str = "spectra.std.collections.set_free";

const ITER_LIST: &str = "spectra.std.collections.list_iter";
const ITER_SET: &str = "spectra.std.collections.set_iter";
const ITER_MAP: &str = "spectra.std.collections.map_iter";
const ITER_NEXT: &str = "spectra.std.collections.iterator_next";
const ITER_REMAINING: &str = "spectra.std.collections.iterator_remaining";
const ITER_FREE: &str = "spectra.std.collections.iterator_free";
const ITER_FROM_VALUES: &str = "spectra.std.collections.iterator_from_values";

fn register_set() {
    register_host_function(SET_NEW, std_set_new);
    register_host_function(SET_INSERT, std_set_insert);
    register_host_function(SET_CONTAINS, std_set_contains);
    register_host_function(SET_REMOVE, std_set_remove);
    register_host_function(SET_LEN, std_set_len);
    register_host_function(SET_GET, std_set_get);
    register_host_function(SET_CLEAR, std_set_clear);
    register_host_function(SET_FREE, std_set_free);
}

fn register_iterator() {
    register_host_function(ITER_LIST, std_list_iter);
    register_host_function(ITER_SET, std_set_iter);
    register_host_function(ITER_MAP, std_map_iter);
    register_host_function(ITER_NEXT, std_iterator_next);
    register_host_function(ITER_REMAINING, std_iterator_remaining);
    register_host_function(ITER_FREE, std_iterator_free);
    register_host_function(ITER_FROM_VALUES, std_iterator_from_values);
}

#[derive(Default)]
struct StdSet {
    // A stable insertion-ordered representation is deliberate: it makes
    // `set_get` and iterator snapshots deterministic without imposing a hash
    // contract on every scalar ABI value.
    data: Vec<SpectraHostValue>,
}

struct SetRegistry {
    sets: HandleTable<ManualBox<StdSet>>,
}

impl SetRegistry {
    fn new() -> Self {
        Self {
            sets: HandleTable::new(HandleKind::Set),
        }
    }

    fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    fn insert(&mut self, set: ManualBox<StdSet>) -> usize {
        self.sets.insert(set).raw() as usize
    }

    fn insert_value(&mut self, handle: usize, value: SpectraHostValue) -> Result<bool, i32> {
        let id = Self::id(handle)?;
        let set = self.sets.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        if set
            .data
            .iter()
            .copied()
            .any(|candidate| collection_values_equal(candidate, value))
        {
            return Ok(false);
        }
        set.data.push(value);
        Ok(true)
    }

    fn contains(&self, handle: usize, value: SpectraHostValue) -> Result<bool, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .sets
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .iter()
            .copied()
            .any(|candidate| collection_values_equal(candidate, value)))
    }

    fn remove_value(&mut self, handle: usize, value: SpectraHostValue) -> Result<bool, i32> {
        let id = Self::id(handle)?;
        let set = self.sets.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        let Some(index) = set
            .data
            .iter()
            .position(|candidate| collection_values_equal(*candidate, value))
        else {
            return Ok(false);
        };
        set.data.remove(index);
        Ok(true)
    }

    fn len(&self, handle: usize) -> Result<usize, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .sets
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .len())
    }

    fn get_option(&self, handle: usize, index: i64) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let set = self.sets.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        if index < 0 {
            return Ok(None);
        }
        Ok(set.data.get(index as usize).copied())
    }

    fn clear(&mut self, handle: usize) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.sets
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .clear();
        Ok(())
    }

    fn remove(&mut self, handle: usize) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.sets
            .remove(id)
            .map(|_| ())
            .map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    fn snapshot(&self, handle: usize) -> Result<Vec<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .sets
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .clone())
    }
}

fn set_registry() -> &'static Mutex<SetRegistry> {
    static REGISTRY: OnceLock<Mutex<SetRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(SetRegistry::new()))
}

fn with_set_registry<F, R>(action: F) -> R
where
    F: FnOnce(&mut SetRegistry) -> R,
{
    let mut guard = lock_unpoisoned(set_registry());
    action(&mut guard)
}

extern "C" fn std_set_new(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 0 || ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let set = match initialize().memory().allocate_manual(StdSet::default()) {
            Ok(set) => set,
            Err(_) => return HOST_STATUS_INTERNAL_ERROR,
        };
        let handle = with_set_registry(|registry| registry.insert(set));
        slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len)[0] = handle as i64;
    }
    HOST_STATUS_SUCCESS
}

extern "C" fn std_set_insert(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_set_registry(|registry| registry.insert_value(args[0] as usize, args[1])) {
        Ok(inserted) => {
            results[0] = inserted as i64;
            HOST_STATUS_SUCCESS
        }
        Err(code) => code,
    }
}

extern "C" fn std_set_contains(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_set_registry(|registry| registry.contains(args[0] as usize, args[1])) {
        Ok(found) => {
            results[0] = found as i64;
            HOST_STATUS_SUCCESS
        }
        Err(code) => code,
    }
}

extern "C" fn std_set_remove(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_set_registry(|registry| registry.remove_value(args[0] as usize, args[1])) {
        Ok(removed) => {
            results[0] = removed as i64;
            HOST_STATUS_SUCCESS
        }
        Err(code) => code,
    }
}

extern "C" fn std_set_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_set_registry(|registry| registry.len(args[0] as usize)) {
        Ok(len) => {
            results[0] = len as i64;
            HOST_STATUS_SUCCESS
        }
        Err(code) => code,
    }
}

extern "C" fn std_set_get(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let value = match with_set_registry(|registry| registry.get_option(args[0] as usize, args[1])) {
        Ok(value) => value,
        Err(code) => return code,
    };
    let mut option_ctx = SpectraHostCallContext {
        args: std::ptr::null(),
        arg_len: 0,
        results: results.as_mut_ptr(),
        result_len: results.len(),
        invoke_fn: None,
    };
    write_option_result(&mut option_ctx, value)
}

extern "C" fn std_set_clear(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_call_void_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_set_registry(|registry| registry.clear(args[0] as usize)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

extern "C" fn std_set_free(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_call_void_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_set_registry(|registry| registry.remove(args[0] as usize)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

enum IteratorSource {
    Values {
        items: Vec<SpectraHostValue>,
        cursor: usize,
    },
    Range {
        current: i64,
        remaining: usize,
    },
}

struct StdIterator {
    source: IteratorSource,
}

impl Default for StdIterator {
    fn default() -> Self {
        Self {
            source: IteratorSource::Values {
                items: Vec::new(),
                cursor: 0,
            },
        }
    }
}

struct IteratorRegistry {
    iterators: HandleTable<ManualBox<StdIterator>>,
}

impl IteratorRegistry {
    fn new() -> Self {
        Self {
            iterators: HandleTable::new(HandleKind::Iterator),
        }
    }

    fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    fn insert(&mut self, iterator: ManualBox<StdIterator>) -> usize {
        self.iterators.insert(iterator).raw() as usize
    }

    fn next(&mut self, handle: usize) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let iterator = self
            .iterators
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?;
        match &mut iterator.source {
            IteratorSource::Values { items, cursor } => {
                let Some(value) = items.get(*cursor).copied() else {
                    return Ok(None);
                };
                *cursor += 1;
                Ok(Some(value))
            }
            IteratorSource::Range {
                current,
                remaining,
            } => {
                if *remaining == 0 {
                    return Ok(None);
                }
                let value = *current;
                *remaining -= 1;
                if *remaining > 0 {
                    *current = current.saturating_add(1);
                }
                Ok(Some(value))
            }
        }
    }

    fn remaining(&self, handle: usize) -> Result<usize, i32> {
        let id = Self::id(handle)?;
        let iterator = self
            .iterators
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?;
        Ok(match &iterator.source {
            IteratorSource::Values { items, cursor } => items.len().saturating_sub(*cursor),
            IteratorSource::Range { remaining, .. } => *remaining,
        })
    }

    fn remove(&mut self, handle: usize) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.iterators
            .remove(id)
            .map(|_| ())
            .map_err(|_| HOST_STATUS_NOT_FOUND)
    }
}

fn iterator_registry() -> &'static Mutex<IteratorRegistry> {
    static REGISTRY: OnceLock<Mutex<IteratorRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(IteratorRegistry::new()))
}

fn with_iterator_registry<F, R>(action: F) -> R
where
    F: FnOnce(&mut IteratorRegistry) -> R,
{
    let mut guard = lock_unpoisoned(iterator_registry());
    action(&mut guard)
}

fn insert_iterator(items: Vec<SpectraHostValue>) -> Result<usize, i32> {
    let iterator = initialize()
        .memory()
        .allocate_manual(StdIterator {
            source: IteratorSource::Values { items, cursor: 0 },
        })
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    Ok(with_iterator_registry(|registry| registry.insert(iterator)))
}

fn insert_range_iterator(range: IntRange, remaining: usize) -> Result<usize, i32> {
    let iterator = initialize()
        .memory()
        .allocate_manual(StdIterator {
            source: IteratorSource::Range {
                current: range.start,
                remaining,
            },
        })
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    Ok(with_iterator_registry(|registry| registry.insert(iterator)))
}

extern "C" fn std_list_iter(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let items = match with_list_registry(|registry| registry.snapshot(args[0] as usize)) {
        Ok(items) => items,
        Err(code) => return code,
    };
    let handle = match insert_iterator(items) {
        Ok(handle) => handle,
        Err(code) => return code,
    };
    results[0] = handle as i64;
    HOST_STATUS_SUCCESS
}

extern "C" fn std_set_iter(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let items = match with_set_registry(|registry| registry.snapshot(args[0] as usize)) {
        Ok(items) => items,
        Err(code) => return code,
    };
    let handle = match insert_iterator(items) {
        Ok(handle) => handle,
        Err(code) => return code,
    };
    results[0] = handle as i64;
    HOST_STATUS_SUCCESS
}

extern "C" fn std_map_iter(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let items = match with_map_registry(|registry| registry.keys_snapshot(args[0] as usize)) {
        Ok(items) => items,
        Err(code) => return code,
    };
    let handle = match insert_iterator(items) {
        Ok(handle) => handle,
        Err(code) => return code,
    };
    results[0] = handle as i64;
    HOST_STATUS_SUCCESS
}

extern "C" fn std_iterator_next(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let value = match with_iterator_registry(|registry| registry.next(args[0] as usize)) {
        Ok(value) => value,
        Err(code) => return code,
    };
    let mut option_ctx = SpectraHostCallContext {
        args: std::ptr::null(),
        arg_len: 0,
        results: results.as_mut_ptr(),
        result_len: results.len(),
        invoke_fn: None,
    };
    write_option_result(&mut option_ctx, value)
}

extern "C" fn std_iterator_remaining(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_iterator_registry(|registry| registry.remaining(args[0] as usize)) {
        Ok(remaining) => {
            results[0] = remaining as i64;
            HOST_STATUS_SUCCESS
        }
        Err(code) => code,
    }
}

extern "C" fn std_iterator_free(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_call_void_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match with_iterator_registry(|registry| registry.remove(args[0] as usize)) {
        Ok(()) => HOST_STATUS_SUCCESS,
        Err(code) => code,
    }
}

/// Internal lowering adapter for fixed-size arrays. The first argument is the
/// number of values followed by the scalar ABI values in iteration order.
/// Keeping the materialization in the compiler avoids making the runtime guess
/// the layout of arbitrary nested/aggregate IR arrays.
extern "C" fn std_iterator_from_values(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len == 0 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let Ok(count) = usize::try_from(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if count != args.len().saturating_sub(1) {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let handle = match insert_iterator(args[1..].to_vec()) {
            Ok(handle) => handle,
            Err(code) => return code,
        };
        slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len)[0] = handle as i64;
    }
    HOST_STATUS_SUCCESS
}

