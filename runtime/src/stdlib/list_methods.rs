use super::*;
impl ListRegistry {
    pub(crate) fn new() -> Self {
        Self {
            lists: HandleTable::new(HandleKind::List),
        }
    }

    pub(crate) fn insert(&mut self, list: ManualBox<StdList>) -> usize {
        self.lists.insert(list).raw() as usize
    }

    pub(crate) fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    pub(crate) fn push(&mut self, handle: usize, value: SpectraHostValue) -> Result<usize, i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        list.data.push(value);
        Ok(list.data.len())
    }

    pub(crate) fn len(&self, handle: usize) -> Result<usize, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .lists
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .len())
    }


    pub(crate) fn get_option(&self, handle: usize, index: i64) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        if index < 0 || (index as usize) >= list.data.len() {
            return Ok(None);
        }
        Ok(Some(list.data[index as usize]))
    }

    pub(crate) fn set(&mut self, handle: usize, index: i64, value: SpectraHostValue) -> Result<(), i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        if index < 0 || (index as usize) >= list.data.len() {
            return Err(HOST_STATUS_NOT_FOUND);
        }
        list.data[index as usize] = value;
        Ok(())
    }

    pub(crate) fn contains(&self, handle: usize, value: SpectraHostValue) -> Result<bool, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .lists
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .iter()
            .copied()
            .any(|candidate| collection_values_equal(candidate, value)))
    }

    pub(crate) fn clear_list(&mut self, handle: usize) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.lists
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .clear();
        Ok(())
    }

    pub(crate) fn remove(&mut self, handle: usize) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.lists
            .remove(id)
            .map(|_| ())
            .map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    pub(crate) fn clear_all(&mut self) -> usize {
        self.lists.clear()
    }



    pub(crate) fn pop_option(&mut self, handle: usize) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        Ok(list.data.pop())
    }

    pub(crate) fn pop_front_option(&mut self, handle: usize) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        if list.data.is_empty() {
            return Ok(None);
        }
        Ok(Some(list.data.remove(0)))
    }

    pub(crate) fn insert_at(&mut self, handle: usize, index: i64, value: SpectraHostValue) -> Result<(), i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        let idx = index.clamp(0, list.data.len() as i64) as usize;
        list.data.insert(idx, value);
        Ok(())
    }


    pub(crate) fn remove_at_option(
        &mut self,
        handle: usize,
        index: i64,
    ) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        if index < 0 || (index as usize) >= list.data.len() {
            return Ok(None);
        }
        Ok(Some(list.data.remove(index as usize)))
    }

    pub(crate) fn index_of(&self, handle: usize, value: SpectraHostValue) -> Result<SpectraHostValue, i32> {
        let id = Self::id(handle)?;
        self.lists
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .iter()
            .position(|&v| collection_values_equal(v, value))
            .map(|i| i as i64)
            .ok_or(HOST_STATUS_NOT_FOUND)
    }

    pub(crate) fn sort_asc(&mut self, handle: usize) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.lists
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .sort();
        Ok(())
    }

    /// Returns a clone of the list's data without holding any other lock.
    pub(crate) fn snapshot(&self, handle: usize) -> Result<Vec<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .lists
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .clone())
    }

    /// Replaces a list's data with `data` (used after an out-of-lock sort/transform).
    pub(crate) fn restore(&mut self, handle: usize, data: Vec<SpectraHostValue>) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.lists
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data = data;
        Ok(())
    }
}

