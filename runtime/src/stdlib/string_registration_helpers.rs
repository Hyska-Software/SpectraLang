use super::*;
// ── std.string & std.convert registrations ─────────────────────────────────

pub(crate) fn register_string() {
    register_host_function(STR_LEN, std_string_len);
    register_host_function(STR_CONTAINS, std_string_contains);
    register_host_function(STR_TO_UPPER, std_string_to_upper);
    register_host_function(STR_TO_LOWER, std_string_to_lower);
    register_host_function(STR_TRIM, std_string_trim);
    register_host_function(STR_STARTS_WITH, std_string_starts_with);
    register_host_function(STR_ENDS_WITH, std_string_ends_with);
    register_host_function(STR_EQ, std_string_eq);
    register_host_function(STR_CONCAT, std_string_concat);
    register_host_function(STR_REPEAT, std_string_repeat);
    register_host_function(STR_BUILDER_NEW, std_string_builder_new);
    register_host_function(STR_BUILDER_PUSH, std_string_builder_push);
    register_host_function(STR_BUILDER_LEN, std_string_builder_len);
    register_host_function(STR_BUILDER_FINISH, std_string_builder_finish);
    register_host_function(STR_BUILDER_FREE, std_string_builder_free);
    register_host_function(STR_CHAR_AT, std_string_char_at);
    register_host_function(STR_SUBSTRING, std_string_substring);
    register_host_function(STR_REPLACE, std_string_replace);
    register_host_function(STR_INDEX_OF, std_string_index_of);
    register_host_function(STR_SPLIT_FIRST, std_string_split_first);
    register_host_function(STR_SPLIT_LAST, std_string_split_last);
    register_host_function(STR_IS_EMPTY, std_string_is_empty);
    register_host_function(STR_COUNT, std_string_count_occurrences);
    register_host_function(STR_SPLIT_BY, std_string_split_by);
    register_host_function(STR_PAD_LEFT, std_string_pad_left);
    register_host_function(STR_PAD_RIGHT, std_string_pad_right);
    register_host_function(STR_REVERSE, std_string_reverse);
}

pub(crate) fn register_convert() {
    register_host_function(CONV_INT_TO_STRING, std_convert_int_to_string);
    register_host_function(CONV_FLOAT_TO_STRING, std_convert_float_to_string);
    register_host_function(CONV_BOOL_TO_STRING, std_convert_bool_to_string);
    register_host_function(CONV_STRING_TO_INT, std_convert_string_to_int);
    register_host_function(CONV_STRING_TO_FLOAT, std_convert_string_to_float);
    register_host_function(CONV_INT_TO_FLOAT, std_convert_int_to_float);
    register_host_function(CONV_FLOAT_TO_INT, std_convert_float_to_int);
    register_host_function(CONV_STRING_TO_INT_OR, std_convert_string_to_int_or);
    register_host_function(CONV_STRING_TO_FLOAT_OR, std_convert_string_to_float_or);
    register_host_function(CONV_STRING_TO_BOOL, std_convert_string_to_bool);
    register_host_function(CONV_BOOL_TO_INT, std_convert_bool_to_int);
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Read a Spectra string (packed UTF-8 bytes with a single-byte NUL
/// terminator) from a raw pointer value.
/// Returns `None` if the pointer is null or the bytes are not valid UTF-8.
pub(crate) unsafe fn read_spectra_string(ptr_val: SpectraHostValue) -> Option<String> {
    if ptr_val == 0 {
        return None;
    }
    let raw = ptr_val as *const u8;
    let mut bytes: Vec<u8> = Vec::new();
    let mut offset = 0usize;
    let limit = crate::ffi::string_scan_limit_bytes(ptr_val);
    while offset < limit {
        let b = *raw.add(offset);
        if b == 0 {
            break;
        }
        bytes.push(b);
        offset += 1;
    }
    if offset >= limit {
        // No NUL terminator within the allocation/scan bound: treat as invalid.
        return None;
    }
    let value = String::from_utf8(bytes).ok()?;
    Some(value)
}
/// Probe whether `value` points at a tracked packed string buffer
/// (UTF-8 bytes with a single-byte NUL terminator).
///
/// Only pointers known to the manual AllocationTable are dereferenced;
/// untracked values (raw scalars, interned JIT literals, stack data) are
/// rejected without touching memory, so any host value can be probed safely.
/// Beyond requiring a NUL terminator inside the allocation, every trailing
/// byte must be zero: genuine string allocations are zero-padded, while
/// multi-word payloads (e.g. tagged `Option`/`Result` slots) carry nonzero
/// words past the first zero byte and therefore stay scalar keys.
pub(crate) unsafe fn try_read_packed_string(value: SpectraHostValue) -> Option<String> {
    if value == 0 {
        return None;
    }
    let limit = crate::ffi::manual_allocation_size(value)?;
    let raw = value as *const u8;
    let mut bytes: Vec<u8> = Vec::new();
    let mut offset = 0usize;
    while offset < limit {
        // SAFETY: offset stays within the bounds of the tracked allocation.
        let byte = *raw.add(offset);
        if byte == 0 {
            break;
        }
        bytes.push(byte);
        offset += 1;
    }
    if offset >= limit {
        // No NUL terminator within the tracked allocation: not a packed string.
        return None;
    }
    while offset + 1 < limit {
        offset += 1;
        // SAFETY: offset stays within the bounds of the tracked allocation.
        if *raw.add(offset) != 0 {
            return None;
        }
    }
    String::from_utf8(bytes).ok()
}

/// Allocate a new Spectra string using the runtime manual allocator.
/// The UTF-8 bytes are packed one byte per byte; the buffer is terminated
/// with a single NUL byte (`len + 1` bytes total).
/// Returns the pointer cast to `i64`, or `0` on allocation failure.
pub(crate) unsafe fn alloc_spectra_string(s: &str) -> SpectraHostValue {
    use crate::ffi::spectra_rt_manual_alloc;
    let bytes = s.as_bytes();
    let total_bytes = bytes.len() + 1;
    let raw = spectra_rt_manual_alloc(total_bytes);
    if raw.is_null() {
        return 0;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), raw, bytes.len());
    *raw.add(bytes.len()) = 0; // null terminator

    raw as i64
}

/// Allocate the common two-word representation used by compiler-generated
/// `Option<T>` and `Result<T, E>` values: tag at slot zero, payload at slot one.
/// The allocation is intentionally manual so the value can cross a host-call
/// boundary and remain valid after this function returns.
pub(crate) unsafe fn alloc_tagged_payload(
    tag: SpectraHostValue,
    payload: SpectraHostValue,
) -> SpectraHostValue {
    use crate::ffi::spectra_rt_manual_alloc;
    let raw = spectra_rt_manual_alloc(2 * std::mem::size_of::<i64>()) as *mut i64;
    if raw.is_null() {
        return 0;
    }
    *raw = tag;
    *raw.add(1) = payload;
    raw as SpectraHostValue
}

/// Mirrors `std_string_len` but skips the generic host-call dispatch AND the
/// unnecessary `String` allocation in `read_spectra_string`. Walks the
/// NUL-terminated byte buffer directly to count bytes.
///
/// Returns the string length (>0) or `0` for an invalid handle.
pub fn string_len_fast(s: SpectraHostValue) -> SpectraHostValue {
    if s == 0 {
        return 0;
    }
    let raw = s as *const u8;
    let limit = crate::ffi::string_scan_limit_bytes(s);
    let mut len: usize = 0;
    unsafe {
        while len < limit && *raw.add(len) != 0 {
            len += 1;
        }
    }
    len as SpectraHostValue
}

/// Fast-path helper for `str.char_at(s, index)`.
///
/// Mirrors `std_string_char_at` but skips the generic host-call dispatch AND
/// the unnecessary `String` allocation in `read_spectra_string`. Reads the
/// byte at the given index directly from the NUL-terminated byte buffer.
///
/// Returns the byte value (0-255) on success or `-1` for an out-of-bounds
/// access (null handle, negative index, or index past the null terminator).
pub fn string_char_at_fast(s: SpectraHostValue, index: SpectraHostValue) -> SpectraHostValue {
    if s == 0 || index < 0 {
        return -1;
    }
    let idx = index as usize;
    let raw = s as *const u8;

    unsafe {
        let limit = crate::ffi::string_scan_limit_bytes(s);
        let mut len: usize = 0;
        while len < limit && *raw.add(len) != 0 {
            len += 1;
        }
        if idx >= len {
            return -1;
        }
        *raw.add(idx) as SpectraHostValue
    }
}

pub(crate) fn fs_path_from_string(path: String) -> Option<PathBuf> {
    if path.trim().is_empty() || path.contains('\0') {
        return None;
    }
    Some(PathBuf::from(path))
}

pub(crate) unsafe fn read_fs_path_arg(arg: SpectraHostValue) -> Result<Option<PathBuf>, i32> {
    match read_spectra_string(arg) {
        Some(path) => Ok(fs_path_from_string(path)),
        None => Err(HOST_STATUS_INVALID_ARGUMENT),
    }
}

pub(crate) fn ensure_file_parent(path: &Path) -> bool {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => std::fs::create_dir_all(parent).is_ok(),
        _ => true,
    }
}

pub(crate) fn fs_write_text(path: &Path, content: &str, append: bool) -> bool {
    fs_write_text_result(path, content, append).is_ok()
}

pub(crate) fn fs_write_text_result(
    path: &Path,
    content: &str,
    append: bool,
) -> Result<(), std::io::Error> {
    if !ensure_file_parent(path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "unable to create filesystem parent directory",
        ));
    }

    if append {
        std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .and_then(|mut file| file.write_all(content.as_bytes()))
    } else {
        std::fs::write(path, content.as_bytes())
    }
}
