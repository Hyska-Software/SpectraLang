use super::*;
pub(crate) fn with_list_registry<F, R>(action: F) -> R
where
    F: FnOnce(&mut ListRegistry) -> R,
{
    let registry = list_registry();
    let mut guard = lock_unpoisoned(registry);
    action(&mut guard)
}

pub(crate) fn list_registry() -> &'static Mutex<ListRegistry> {
    static REGISTRY: LazyLock<Mutex<ListRegistry>> =
        LazyLock::new(|| Mutex::new(ListRegistry::new()));
    &REGISTRY
}

/// Reads the elements of a list handle.
///
/// Public because sibling host-call provider crates have to serialize
/// collection values: the JSON encoder in `spectra-api` turns a `List<T>` into
/// a JSON array and must see the elements. The registry itself stays private;
/// this read seam and [`list_create`] are the whole cross-crate surface.
pub fn list_elements(handle: i64) -> Result<Vec<SpectraHostValue>, i32> {
    with_list_registry(|registry| {
        let len = registry.len(handle as usize)?;
        let mut values = Vec::with_capacity(len);
        for index in 0..len {
            match registry.get_option(handle as usize, index as i64)? {
                Some(value) => values.push(value),
                // `len` and `get_option` agree on the same table entry; a
                // missing index means the list shrank mid-read, which no path
                // does, so stopping short is the safe reading.
                None => break,
            }
        }
        Ok(values)
    })
}

/// Creates a list holding `elements` and returns its handle.
///
/// Public for the same reason as [`list_elements`]: a host-call provider that
/// decodes a JSON array into a `List<T>` needs a list to fill. Elements are
/// escaped exactly like `list_push` escapes the values it stores, so a decoded
/// string survives the frame that produced it.
pub fn list_create(elements: &[SpectraHostValue]) -> Result<i64, i32> {
    let memory = initialize().memory();
    let list = memory
        .allocate_manual(StdList::with_capacity(elements.len()))
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;

    // Escape values before taking the registry lock, then fill the reserved
    // vector under one registry critical section. JSON decoding can otherwise
    // pay one registry lock/unlock pair per element.
    for value in elements {
        crate::ffi::escape_stored_value(*value);
    }
    with_list_registry(|registry| {
        let handle = registry.insert(list);
        for value in elements {
            registry.push(handle, *value)?;
        }
        Ok(handle as i64)
    })
}

#[derive(Default)]
pub(crate) struct StdList {
    /// A deque keeps indexed access O(1) while making `pop_front` O(1), which
    /// is the important distinction from a plain `Vec` for FIFO-style list
    /// workloads. Sorting temporarily makes the ring contiguous.
    pub(crate) data: VecDeque<SpectraHostValue>,
}

impl StdList {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            data: VecDeque::with_capacity(capacity),
        }
    }
}

pub(crate) struct ListRegistry {
    pub(crate) lists: HandleTable<ManualBox<StdList>>,
}

// ── std.string string builder (R-3108) ──────────────────────────────────────

pub(crate) struct StringBuilder {
    pub(crate) buf: Vec<u8>,
    pub(crate) len: usize,
}

impl StringBuilder {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity.max(256)),
            len: 0,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn push_bytes(&mut self, bytes: &[u8]) {
        let needed = self.len + bytes.len();
        if needed > self.buf.len() {
            self.buf.resize(needed, 0);
        }
        self.buf[self.len..needed].copy_from_slice(bytes);
        self.len = needed;
    }

    pub(crate) fn push_spectra_string(&mut self, str_ptr: i64) {
        let raw = str_ptr as *const u8;
        if raw.is_null() {
            return;
        }
        let mut offset = 0;
        loop {
            // Spectra string: packed UTF-8 bytes, NUL-terminated.
            let byte = unsafe { *raw.add(offset) };
            if byte == 0 {
                break;
            }
            self.buf.push(byte);
            self.len += 1;
            offset += 1;
        }
    }

    pub(crate) fn current_len(&self) -> usize {
        self.len
    }

    pub(crate) fn finish(&mut self) -> String {
        let s = String::from_utf8(self.buf[..self.len].to_vec()).unwrap_or_default();
        self.len = 0;
        s
    }
}

pub(crate) struct StringBuilderRegistry {
    pub(crate) builders: HandleTable<ManualBox<StringBuilder>>,
}

impl StringBuilderRegistry {
    pub(crate) fn new() -> Self {
        Self {
            builders: HandleTable::new(HandleKind::StringBuilder),
        }
    }

    pub(crate) fn insert(&mut self, builder: ManualBox<StringBuilder>) -> usize {
        self.builders.insert(builder).raw() as usize
    }

    pub(crate) fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    pub(crate) fn push_spectra_string(&mut self, handle: usize, str_ptr: i64) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.builders
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .push_spectra_string(str_ptr);
        Ok(())
    }

    pub(crate) fn len(&self, handle: usize) -> Result<usize, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .builders
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .current_len())
    }

    pub(crate) fn finish(&mut self, handle: usize) -> Result<String, i32> {
        let id = Self::id(handle)?;
        let mut builder = self
            .builders
            .remove(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?;
        Ok(builder.finish())
    }

    pub(crate) fn discard(&mut self, handle: usize) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.builders
            .remove(id)
            .map(|_| ())
            .map_err(|_| HOST_STATUS_NOT_FOUND)
    }
}

pub(crate) fn string_builder_registry() -> &'static Mutex<StringBuilderRegistry> {
    static REGISTRY: OnceLock<Mutex<StringBuilderRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(StringBuilderRegistry::new()))
}
