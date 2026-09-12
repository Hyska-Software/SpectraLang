use spectra_runtime::ffi::SpectraHostValue;
use spectra_runtime::handles::{HandleId, HandleKind, HandleTable};

pub(crate) struct ApiHandleTable<T> {
    table: HandleTable<T>,
}

impl<T> ApiHandleTable<T> {
    pub(crate) fn new(kind: HandleKind) -> Self {
        Self {
            table: HandleTable::new(kind),
        }
    }

    pub(crate) fn insert(&mut self, value: T) -> SpectraHostValue {
        self.table.insert(value).raw()
    }

    pub(crate) fn get(&self, raw: &SpectraHostValue) -> Option<&T> {
        let handle = HandleId::from_raw(*raw).ok()?;
        self.table.get(handle).ok()
    }

    pub(crate) fn get_mut(&mut self, raw: &SpectraHostValue) -> Option<&mut T> {
        let handle = HandleId::from_raw(*raw).ok()?;
        self.table.get_mut(handle).ok()
    }

    pub(crate) fn remove(&mut self, raw: &SpectraHostValue) -> Option<T> {
        let handle = HandleId::from_raw(*raw).ok()?;
        self.table.remove(handle).ok()
    }

    #[cfg(test)]
    pub(crate) fn contains_key(&self, raw: &SpectraHostValue) -> bool {
        self.get(raw).is_some()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (SpectraHostValue, &T)> + '_ {
        self.table
            .iter()
            .map(|(handle, value)| (handle.raw(), value))
    }
}
