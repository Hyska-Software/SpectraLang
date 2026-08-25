use super::*;
pub(crate) fn with_string_builder_registry<F, R>(action: F) -> R
where
    F: FnOnce(&mut StringBuilderRegistry) -> R,
{
    let registry = string_builder_registry();
    let mut guard = lock_unpoisoned(registry);
    action(&mut guard)
}

#[allow(dead_code)]
pub(crate) fn lock_string_builder_registry(
) -> Result<std::sync::MutexGuard<'static, StringBuilderRegistry>, i32> {
    string_builder_registry()
        .lock()
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)
}

/// Fast-path helper for `str.builder_new(capacity)` called from JIT code
/// via the `spectra_rt_builder_new` fast ABI entry.
pub fn string_builder_new_fast(capacity: usize) -> SpectraHostValue {
    with_string_builder_registry(|reg| {
        let builder = match initialize()
            .memory()
            .allocate_manual(StringBuilder::new(capacity))
        {
            Ok(b) => b,
            Err(_) => return 0,
        };
        let handle = reg.insert(builder);
        handle as SpectraHostValue
    })
}

/// Fast-path helper for `str.builder_push(handle, str_ptr)`. Reads the
/// Spectra string directly into the builder buffer without allocating an
/// intermediate `String`.
pub fn string_builder_push_fast(handle: usize, str_ptr: SpectraHostValue) {
    with_string_builder_registry(|reg| {
        let _ = reg.push_spectra_string(handle, str_ptr);
    });
}

/// Fast-path helper for `str.builder_len(handle)`.
pub fn string_builder_len_fast(handle: usize) -> SpectraHostValue {
    with_string_builder_registry(|reg| match reg.len(handle) {
        Ok(n) => n as SpectraHostValue,
        Err(_) => 0,
    })
}

/// Fast-path helper for `str.builder_finish(handle)`. Returns a Spectra
/// string handle.
pub fn string_builder_finish_fast(handle: usize) -> SpectraHostValue {
    with_string_builder_registry(|reg| match reg.finish(handle) {
        Ok(s) => unsafe { alloc_spectra_string(&s) },
        Err(_) => 0,
    })
}

/// Fast-path helper for `str.builder_free(handle)`.
pub fn string_builder_free_fast(handle: usize) {
    with_string_builder_registry(|reg| {
        let _ = reg.discard(handle);
    });
}

/// Fast-path helper for `col.map_new()`.
///
/// Creates an empty map and returns its handle. Skips the generic
/// host-call dispatch and the result-slice validation path.
pub fn map_new_fast() -> i64 {
    with_map_registry(|reg| reg.insert(Arc::new(Mutex::new(StdMap::default()))) as i64)
}

/// Fast-path helper for `col.map_set(handle, key, value)`.
///
/// Returns 0 on success, `HOST_STATUS_NOT_FOUND` if the handle is invalid.
/// Handle 0 is a sentinel for "no map" and is a no-op (returns NOT_FOUND).
pub fn map_set_fast(handle: usize, key: i64, value: i64) -> i32 {
    let map_arc = with_map_registry(|reg| reg.get(handle));
    match map_arc {
        Some(map_arc) => {
            lock_unpoisoned(&map_arc)
                .data
                .insert(collection_key(key), value);
            HOST_STATUS_SUCCESS
        }
        None => HOST_STATUS_NOT_FOUND,
    }
}

/// Fast-path helper for `col.map_get(handle, key)`.
///
/// Returns the value for the key, or 0 if the key is absent or the handle
/// is invalid. Note: cannot distinguish "stored value is 0" from "key
/// absent / invalid handle".
pub fn map_get_fast(handle: usize, key: i64) -> i64 {
    let map_arc = with_map_registry(|reg| reg.get(handle));
    match map_arc {
        Some(map_arc) => lock_unpoisoned(&map_arc)
            .data
            .get(&collection_key(key))
            .copied()
            .unwrap_or(0),
        None => 0,
    }
}

/// Fast-path helper for `col.map_contains(handle, key)`.
///
/// Returns 1 if the key is present in the map, 0 otherwise (including
/// invalid handle).
pub fn map_contains_fast(handle: usize, key: i64) -> i64 {
    let map_arc = with_map_registry(|reg| reg.get(handle));
    match map_arc {
        Some(map_arc)
            if lock_unpoisoned(&map_arc)
                .data
                .contains_key(&collection_key(key))
            => {
                1
            }
        Some(_) | None => 0,
    }
}

/// Fast-path helper for `col.map_remove(handle, key)`.
///
/// Returns the removed value, or 0 if the key was absent or the handle
/// is invalid. Same caveat as `map_get_fast` regarding stored 0.
pub fn map_remove_fast(handle: usize, key: i64) -> i64 {
    let map_arc = with_map_registry(|reg| reg.get(handle));
    match map_arc {
        Some(map_arc) => lock_unpoisoned(&map_arc)
            .data
            .remove(&collection_key(key))
            .unwrap_or(0),
        None => 0,
    }
}

/// Fast-path helper for `col.map_len(handle)`.
///
/// Returns the number of entries in the map, or 0 for an invalid handle.
pub fn map_len_fast(handle: usize) -> i64 {
    let map_arc = with_map_registry(|reg| reg.get(handle));
    match map_arc {
        Some(map_arc) => lock_unpoisoned(&map_arc).data.len() as i64,
        None => 0,
    }
}

/// Fast-path helper for `col.map_clear(handle)`.
///
/// Removes all entries from the map. No-op for an invalid handle.
pub fn map_clear_fast(handle: usize) {
    let map_arc = with_map_registry(|reg| reg.get(handle));
    if let Some(map_arc) = map_arc {
        lock_unpoisoned(&map_arc).data.clear();
    }
}

/// Fast-path helper for `col.map_free(handle)`.
///
/// Removes the map from the registry and drops the last `Arc` reference.
/// No-op for an invalid handle.
pub fn map_free_fast(handle: usize) {
    with_map_registry(|reg| {
        let _ = reg.remove(handle);
    });
}
