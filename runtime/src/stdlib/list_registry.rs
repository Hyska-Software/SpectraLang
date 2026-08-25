fn with_list_registry<F, R>(action: F) -> R
where
    F: FnOnce(&mut ListRegistry) -> R,
{
    let registry = list_registry();
    let mut guard = lock_unpoisoned(registry);
    action(&mut guard)
}

fn list_registry() -> &'static Mutex<ListRegistry> {
    static REGISTRY: OnceLock<Mutex<ListRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(ListRegistry::new()))
}

#[derive(Default)]
struct StdList {
    data: Vec<SpectraHostValue>,
}

struct ListRegistry {
    lists: HandleTable<ManualBox<StdList>>,
}

// ── std.string string builder (R-3108) ──────────────────────────────────────

struct StringBuilder {
    buf: Vec<u8>,
    len: usize,
}

impl StringBuilder {
    fn new(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity.max(256)),
            len: 0,
        }
    }

    #[allow(dead_code)]
    fn push_bytes(&mut self, bytes: &[u8]) {
        let needed = self.len + bytes.len();
        if needed > self.buf.len() {
            self.buf.resize(needed, 0);
        }
        self.buf[self.len..needed].copy_from_slice(bytes);
        self.len = needed;
    }

    fn push_spectra_string(&mut self, str_ptr: i64) {
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

    fn current_len(&self) -> usize {
        self.len
    }

    fn finish(&mut self) -> String {
        let s = String::from_utf8(self.buf[..self.len].to_vec()).unwrap_or_default();
        self.len = 0;
        s
    }
}

struct StringBuilderRegistry {
    builders: HandleTable<ManualBox<StringBuilder>>,
}

impl StringBuilderRegistry {
    fn new() -> Self {
        Self {
            builders: HandleTable::new(HandleKind::StringBuilder),
        }
    }

    fn insert(&mut self, builder: ManualBox<StringBuilder>) -> usize {
        self.builders.insert(builder).raw() as usize
    }

    fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    fn push_spectra_string(&mut self, handle: usize, str_ptr: i64) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.builders
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .push_spectra_string(str_ptr);
        Ok(())
    }

    fn len(&self, handle: usize) -> Result<usize, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .builders
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .current_len())
    }

    fn finish(&mut self, handle: usize) -> Result<String, i32> {
        let id = Self::id(handle)?;
        let mut builder = self
            .builders
            .remove(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?;
        Ok(builder.finish())
    }

    fn discard(&mut self, handle: usize) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.builders
            .remove(id)
            .map(|_| ())
            .map_err(|_| HOST_STATUS_NOT_FOUND)
    }
}

fn string_builder_registry() -> &'static Mutex<StringBuilderRegistry> {
    static REGISTRY: OnceLock<Mutex<StringBuilderRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(StringBuilderRegistry::new()))
}

