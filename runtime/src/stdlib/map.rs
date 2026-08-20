// ── std.collections map ─────────────────────────────────────────────────────

const MAP_NEW: &str = "spectra.std.collections.map_new";
const MAP_SET: &str = "spectra.std.collections.map_set";
const MAP_GET: &str = spectra_contract::STD_COLLECTIONS_MAP_GET_BINDING;
const MAP_GET_OPTION: &str = spectra_contract::STD_COLLECTIONS_MAP_GET_OPTION_BINDING;
const MAP_GET_COMPAT: &str = spectra_contract::STD_COMPAT_COLLECTIONS_MAP_GET_BINDING;
const MAP_CONTAINS: &str = "spectra.std.collections.map_contains";
const MAP_REMOVE: &str = spectra_contract::STD_COLLECTIONS_MAP_REMOVE_BINDING;
const MAP_REMOVE_OPTION: &str = spectra_contract::STD_COLLECTIONS_MAP_REMOVE_OPTION_BINDING;
const MAP_REMOVE_COMPAT: &str = spectra_contract::STD_COMPAT_COLLECTIONS_MAP_REMOVE_BINDING;
const MAP_LEN: &str = "spectra.std.collections.map_len";
const MAP_CLEAR: &str = "spectra.std.collections.map_clear";
const MAP_FREE: &str = "spectra.std.collections.map_free";

fn register_map() {
    register_host_function(MAP_NEW, std_map_new);
    register_host_function(MAP_SET, std_map_set);
    register_host_function(MAP_GET, std_map_get_option);
    register_host_function(MAP_GET_OPTION, std_map_get_option);
    register_host_function(MAP_GET_COMPAT, std_map_get);
    register_host_function(MAP_CONTAINS, std_map_contains);
    register_host_function(MAP_REMOVE, std_map_remove_option);
    register_host_function(MAP_REMOVE_OPTION, std_map_remove_option);
    register_host_function(MAP_REMOVE_COMPAT, std_map_remove);
    register_host_function(MAP_LEN, std_map_len);
    register_host_function(MAP_CLEAR, std_map_clear);
    register_host_function(MAP_FREE, std_map_free);
}

struct MapRegistry {
    maps: HandleTable<Arc<Mutex<StdMap>>>,
}

#[derive(Default)]
struct StdMap {
    data: HashMap<CollectionKey, SpectraHostValue>,
}

impl MapRegistry {
    fn new() -> Self {
        Self {
            maps: HandleTable::new(HandleKind::Map),
        }
    }

    fn insert(&mut self, map: Arc<Mutex<StdMap>>) -> usize {
        self.maps.insert(map).raw() as usize
    }

    fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    fn get(&self, handle: usize) -> Option<Arc<Mutex<StdMap>>> {
        let id = Self::id(handle).ok()?;
        self.maps.get(id).ok().cloned()
    }

    fn lookup_value(
        &self,
        handle: usize,
        key: SpectraHostValue,
    ) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let map = self.maps.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        Ok(lock_unpoisoned(map)
            .data
            .get(&collection_key(key))
            .copied())
    }

    fn keys_snapshot(&self, handle: usize) -> Result<Vec<SpectraHostValue>, i32> {
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

    fn remove(&mut self, handle: usize) -> Result<Arc<Mutex<StdMap>>, i32> {
        let id = Self::id(handle)?;
        self.maps.remove(id).map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    fn remove_value(
        &mut self,
        handle: usize,
        key: SpectraHostValue,
    ) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let map = self.maps.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        Ok(lock_unpoisoned(map).data.remove(&collection_key(key)))
    }
}

fn map_registry() -> &'static Mutex<MapRegistry> {
    static REGISTRY: OnceLock<Mutex<MapRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(MapRegistry::new()))
}

fn with_map_registry<F, R>(action: F) -> R
where
    F: FnOnce(&mut MapRegistry) -> R,
{
    let registry = map_registry();
    let mut guard = lock_unpoisoned(registry);
    action(&mut guard)
}

/// Creates a new empty map and returns its handle.
extern "C" fn std_map_new(ctx: *mut SpectraHostCallContext) -> i32 {
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
extern "C" fn std_map_set(ctx: *mut SpectraHostCallContext) -> i32 {
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

/// Returns the value for `key` in the map, or 0 if not found.
/// Args: [handle, key]. Returns: value.
extern "C" fn std_map_get(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let value = match map_arc {
            Some(map_arc) => lock_unpoisoned(&map_arc)
                .data
                .get(&collection_key(key))
                .copied()
                .unwrap_or(0),
            None => 0,
        };
        results[0] = value;
    }
    HOST_STATUS_SUCCESS
}

extern "C" fn std_map_get_option(ctx: *mut SpectraHostCallContext) -> i32 {
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
extern "C" fn std_map_contains(ctx: *mut SpectraHostCallContext) -> i32 {
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

/// Removes `key` from the map. Returns the removed value, or 0 if not present.
/// Args: [handle, key]. Returns: removed_value.
extern "C" fn std_map_remove(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let handle = args[0] as usize;
        let key = args[1];
        let map_arc = with_map_registry(|reg| reg.get(handle));
        let removed = match map_arc {
            Some(map_arc) => lock_unpoisoned(&map_arc)
                .data
                .remove(&collection_key(key))
                .unwrap_or(0),
            None => 0,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = removed;
        }
    }
    HOST_STATUS_SUCCESS
}

extern "C" fn std_map_remove_option(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let value = with_map_registry(|registry| {
            registry.remove_value(args[0] as usize, args[1])
        })
        .unwrap_or(None);
        write_option_result(ctx_ref, value)
    }
}

/// Returns the number of entries in the map.
/// Args: [handle]. Returns: len.
extern "C" fn std_map_len(ctx: *mut SpectraHostCallContext) -> i32 {
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

/// Removes all entries from the map without freeing the handle.
/// Args: [handle]. Returns 0.
extern "C" fn std_map_clear(ctx: *mut SpectraHostCallContext) -> i32 {
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
extern "C" fn std_map_free(ctx: *mut SpectraHostCallContext) -> i32 {
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

