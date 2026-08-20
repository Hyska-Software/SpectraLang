impl ListRegistry {
    fn new() -> Self {
        Self {
            lists: HandleTable::new(HandleKind::List),
        }
    }

    fn insert(&mut self, list: ManualBox<StdList>) -> usize {
        self.lists.insert(list).raw() as usize
    }

    fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    fn push(&mut self, handle: usize, value: SpectraHostValue) -> Result<usize, i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        list.data.push(value);
        Ok(list.data.len())
    }

    fn len(&self, handle: usize) -> Result<usize, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .lists
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .len())
    }

    fn get(&self, handle: usize, index: i64) -> Result<SpectraHostValue, i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        if index < 0 || (index as usize) >= list.data.len() {
            return Err(HOST_STATUS_NOT_FOUND);
        }
        Ok(list.data[index as usize])
    }

    fn get_option(&self, handle: usize, index: i64) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        if index < 0 || (index as usize) >= list.data.len() {
            return Ok(None);
        }
        Ok(Some(list.data[index as usize]))
    }

    fn set(&mut self, handle: usize, index: i64, value: SpectraHostValue) -> Result<(), i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        if index < 0 || (index as usize) >= list.data.len() {
            return Err(HOST_STATUS_NOT_FOUND);
        }
        list.data[index as usize] = value;
        Ok(())
    }

    fn contains(&self, handle: usize, value: SpectraHostValue) -> Result<bool, i32> {
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

    fn clear_list(&mut self, handle: usize) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.lists
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .clear();
        Ok(())
    }

    fn remove(&mut self, handle: usize) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.lists
            .remove(id)
            .map(|_| ())
            .map_err(|_| HOST_STATUS_NOT_FOUND)
    }

    fn clear_all(&mut self) -> usize {
        self.lists.clear()
    }

    fn pop(&mut self, handle: usize) -> Result<SpectraHostValue, i32> {
        let id = Self::id(handle)?;
        self.lists
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .pop()
            .ok_or(HOST_STATUS_NOT_FOUND)
    }

    fn pop_front(&mut self, handle: usize) -> Result<SpectraHostValue, i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        if list.data.is_empty() {
            return Err(HOST_STATUS_NOT_FOUND);
        }
        Ok(list.data.remove(0))
    }

    fn pop_option(&mut self, handle: usize) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        Ok(list.data.pop())
    }

    fn pop_front_option(&mut self, handle: usize) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        if list.data.is_empty() {
            return Ok(None);
        }
        Ok(Some(list.data.remove(0)))
    }

    fn insert_at(&mut self, handle: usize, index: i64, value: SpectraHostValue) -> Result<(), i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        let idx = index.clamp(0, list.data.len() as i64) as usize;
        list.data.insert(idx, value);
        Ok(())
    }

    fn remove_at(&mut self, handle: usize, index: i64) -> Result<SpectraHostValue, i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        if index < 0 || (index as usize) >= list.data.len() {
            return Err(HOST_STATUS_NOT_FOUND);
        }
        Ok(list.data.remove(index as usize))
    }

    fn remove_at_option(
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

    fn index_of(&self, handle: usize, value: SpectraHostValue) -> Result<SpectraHostValue, i32> {
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

    fn sort_asc(&mut self, handle: usize) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.lists
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .sort();
        Ok(())
    }

    /// Returns a clone of the list's data without holding any other lock.
    fn snapshot(&self, handle: usize) -> Result<Vec<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        Ok(self
            .lists
            .get(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .clone())
    }

    /// Replaces a list's data with `data` (used after an out-of-lock sort/transform).
    fn restore(&mut self, handle: usize, data: Vec<SpectraHostValue>) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.lists
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data = data;
        Ok(())
    }
}

