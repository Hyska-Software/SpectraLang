use super::*;
use std::cell::RefCell;
use std::sync::Weak;
// ── std.collections map ─────────────────────────────────────────────────────

pub(crate) const MAP_NEW: &str = "spectra.std.collections.map_new";
pub(crate) const MAP_SET: &str = "spectra.std.collections.map_set";
pub(crate) const MAP_GET: &str = spectra_contract::STD_COLLECTIONS_MAP_GET_BINDING;
pub(crate) const MAP_GET_OPTION: &str = spectra_contract::STD_COLLECTIONS_MAP_GET_OPTION_BINDING;
pub(crate) const MAP_CONTAINS: &str = "spectra.std.collections.map_contains";
pub(crate) const MAP_REMOVE: &str = spectra_contract::STD_COLLECTIONS_MAP_REMOVE_BINDING;
pub(crate) const MAP_REMOVE_OPTION: &str =
    spectra_contract::STD_COLLECTIONS_MAP_REMOVE_OPTION_BINDING;
pub(crate) const MAP_LEN: &str = "spectra.std.collections.map_len";
pub(crate) const MAP_IS_EMPTY: &str = "spectra.std.collections.map_is_empty";
pub(crate) const MAP_CLEAR: &str = "spectra.std.collections.map_clear";
pub(crate) const MAP_FREE: &str = "spectra.std.collections.map_free";
pub(crate) const MAP_FREE_ALL: &str = "spectra.std.collections.map_free_all";

pub(crate) fn register_map() {
    register_host_function(MAP_NEW, std_map_new);
    register_host_function(MAP_SET, std_map_set);
    register_host_function(MAP_GET, std_map_get_option);
    register_host_function(MAP_GET_OPTION, std_map_get_option);
    register_host_function(MAP_CONTAINS, std_map_contains);
    register_host_function(MAP_REMOVE, std_map_remove_option);
    register_host_function(MAP_REMOVE_OPTION, std_map_remove_option);
    register_host_function(MAP_LEN, std_map_len);
    register_host_function(MAP_IS_EMPTY, std_map_is_empty);
    register_host_function(MAP_CLEAR, std_map_clear);
    register_host_function(MAP_FREE, std_map_free);
    register_host_function(MAP_FREE_ALL, std_map_free_all);
}

pub(crate) struct MapRegistry {
    pub(crate) maps: HandleTable<Arc<Mutex<StdMap>>>,
}

#[derive(Default)]
pub(crate) struct StdMap {
    pub(crate) data: HashMap<CollectionKey, SpectraHostValue>,
}

// Direct collection calls tend to touch the same map handle repeatedly. The
// registry mutex is still required to validate a handle, but a thread-local
// weak cache avoids reacquiring it for every operation while preserving map
// reclamation: a freed map is not kept alive by the cache.
static MAP_REGISTRY_EPOCH: AtomicU64 = AtomicU64::new(0);

struct MapFastCache {
    handle: usize,
    epoch: u64,
    map: Weak<Mutex<StdMap>>,
}

impl Default for MapFastCache {
    fn default() -> Self {
        Self {
            handle: 0,
            epoch: 0,
            map: Weak::new(),
        }
    }
}

thread_local! {
    static MAP_FAST_CACHE: RefCell<MapFastCache> = RefCell::new(MapFastCache::default());
}

impl MapRegistry {
    pub(crate) fn new() -> Self {
        Self {
            maps: HandleTable::new(HandleKind::Map),
        }
    }

    pub(crate) fn insert(&mut self, map: Arc<Mutex<StdMap>>) -> usize {
        self.maps.insert(map).raw() as usize
    }

    pub(crate) fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    pub(crate) fn get(&self, handle: usize) -> Option<Arc<Mutex<StdMap>>> {
        let id = Self::id(handle).ok()?;
        self.maps.get(id).ok().cloned()
    }

    pub(crate) fn lookup_value(
        &self,
        handle: usize,
        key: SpectraHostValue,
    ) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let map = self.maps.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        Ok(lock_unpoisoned(map).data.get(&collection_key(key)).copied())
    }

    pub(crate) fn keys_snapshot(&self, handle: usize) -> Result<Vec<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let map = self.maps.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        let mut keys = lock_unpoisoned(map)
            .data
            .keys()
            .map(CollectionKey::raw_value)
            .collect::<Vec<_>>();
        keys.sort_unstable();
        Ok(keys)
    }

    pub(crate) fn values_snapshot(&self, handle: usize) -> Result<Vec<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let map = self.maps.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        let mut values = lock_unpoisoned(map)
            .data
            .iter()
            .map(|(key, value)| (key.raw_value(), *value))
            .collect::<Vec<_>>();
        values.sort_unstable_by_key(|(key, _)| *key);
        Ok(values.into_iter().map(|(_, value)| value).collect())
    }

    pub(crate) fn remove(&mut self, handle: usize) -> Result<Arc<Mutex<StdMap>>, i32> {
        let id = Self::id(handle)?;
        let map = self.maps.remove(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        MAP_REGISTRY_EPOCH.fetch_add(1, Ordering::AcqRel);
        Ok(map)
    }

    pub(crate) fn remove_value(
        &mut self,
        handle: usize,
        key: SpectraHostValue,
    ) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let map = self.maps.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        Ok(lock_unpoisoned(map).data.remove(&collection_key(key)))
    }

    pub(crate) fn clear_all(&mut self) -> usize {
        let count = self.maps.clear();
        MAP_REGISTRY_EPOCH.fetch_add(1, Ordering::AcqRel);
        count
    }
}

pub(crate) fn map_registry() -> &'static Mutex<MapRegistry> {
    static REGISTRY: OnceLock<Mutex<MapRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(MapRegistry::new()))
}

pub(crate) fn with_map_registry<F, R>(action: F) -> R
where
    F: FnOnce(&mut MapRegistry) -> R,
{
    let registry = map_registry();
    let mut guard = lock_unpoisoned(registry);
    action(&mut guard)
}

/// Returns a validated map reference for direct collection calls.
///
/// The cache is invalidated by the registry epoch whenever a handle is
/// released. The exact generational handle is also part of the key, so a
/// recycled slot cannot reuse an older cached map.
pub(crate) fn map_fast_get(handle: usize) -> Option<Arc<Mutex<StdMap>>> {
    let epoch = MAP_REGISTRY_EPOCH.load(Ordering::Acquire);
    if let Some(map) = MAP_FAST_CACHE.with(|cache| {
        let cache = cache.borrow();
        (cache.handle == handle && cache.epoch == epoch)
            .then(|| cache.map.upgrade())
            .flatten()
    }) {
        return Some(map);
    }

    let map = with_map_registry(|registry| registry.get(handle));
    if let Some(ref map) = map {
        MAP_FAST_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            cache.handle = handle;
            cache.epoch = epoch;
            cache.map = Arc::downgrade(map);
        });
    }
    map
}

/// Creates a new empty map and returns its handle.
pub(crate) extern "C" fn std_map_new(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        let handle = with_map_registry(|reg| reg.insert(Arc::new(Mutex::new(StdMap::default()))));
        results[0] = handle as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Inserts or updates `key → value` in the map identified by `handle`.
/// Args: [handle, key, value]. Returns 0.
pub(crate) extern "C" fn std_map_set(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 3 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let handle = args[0] as usize;
        let key = args[1];
        let value = args[2];
        // Both the key and the value outlive the frame that stored them.
        crate::ffi::escape_stored_value(key);
        crate::ffi::escape_stored_value(value);
        let map_arc = with_map_registry(|reg| reg.get(handle));
        let Some(map_arc) = map_arc else {
            return HOST_STATUS_NOT_FOUND;
        };
        let mut map = lock_unpoisoned(&map_arc);
        map.data.insert(collection_key(key), value);
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = 0;
        }
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_map_get_option(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let value = with_map_registry(|registry| registry.lookup_value(args[0] as usize, args[1]))
            .unwrap_or(None);
        write_option_result(ctx_ref, value)
    }
}

/// Returns 1 if the map contains `key`, 0 otherwise.
/// Args: [handle, key]. Returns: bool as i64.
pub(crate) extern "C" fn std_map_contains(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        let handle = args[0] as usize;
        let key = args[1];
        let map_arc = with_map_registry(|reg| reg.get(handle));
        let found = match map_arc {
            Some(map_arc) => lock_unpoisoned(&map_arc)
                .data
                .contains_key(&collection_key(key)),
            None => false,
        };
        results[0] = if found { 1 } else { 0 };
    }
    HOST_STATUS_SUCCESS
}

pub(crate) extern "C" fn std_map_remove_option(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let value = with_map_registry(|registry| registry.remove_value(args[0] as usize, args[1]))
            .unwrap_or(None);
        write_option_result(ctx_ref, value)
    }
}

/// Returns the number of entries in the map.
/// Args: [handle]. Returns: len.
pub(crate) extern "C" fn std_map_len(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        let handle = args[0] as usize;
        let map_arc = with_map_registry(|reg| reg.get(handle));
        let len = match map_arc {
            Some(map_arc) => lock_unpoisoned(&map_arc).data.len(),
            None => 0,
        };
        results[0] = len as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Returns whether the map contains no entries.
pub(crate) extern "C" fn std_map_is_empty(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let map_arc = with_map_registry(|registry| registry.get(args[0] as usize));
    let Some(map_arc) = map_arc else {
        return HOST_STATUS_NOT_FOUND;
    };
    results[0] = lock_unpoisoned(&map_arc).data.is_empty() as SpectraHostValue;
    HOST_STATUS_SUCCESS
}

/// Removes all entries from the map without freeing the handle.
/// Args: [handle]. Returns 0.
pub(crate) extern "C" fn std_map_clear(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let handle = args[0] as usize;
        let map_arc = with_map_registry(|reg| reg.get(handle));
        if let Some(map_arc) = map_arc {
            lock_unpoisoned(&map_arc).data.clear();
        }
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = 0;
        }
    }
    HOST_STATUS_SUCCESS
}

/// Frees the map and its handle.
/// Args: [handle].
pub(crate) extern "C" fn std_map_free(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let handle = args[0] as usize;
        match with_map_registry(|reg| reg.remove(handle)) {
            Ok(_) => HOST_STATUS_SUCCESS,
            Err(code) => code,
        }
    }
}

/// Frees every live map and returns the number of released handles.
pub(crate) extern "C" fn std_map_free_all(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok((_, results)) = host_call_args(ctx, 0) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    results[0] = with_map_registry(|registry| registry.clear_all()) as SpectraHostValue;
    HOST_STATUS_SUCCESS
}
