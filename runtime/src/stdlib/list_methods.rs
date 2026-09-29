use super::*;
use std::cmp::Ordering as CmpOrdering;
use std::ffi::CStr;
use std::os::raw::c_char;

fn exact_sort_integer_width(kind: i64, base: i64) -> Option<u32> {
    let width = kind.checked_sub(base)?;
    match width {
        8 | 16 | 32 | 64 => Some(width as u32),
        _ => None,
    }
}

pub(crate) fn is_supported_list_sort_kind(kind: i64) -> bool {
    use spectra_contract::collection_sort as sort_kind;

    matches!(
        kind,
        sort_kind::INT | sort_kind::FLOAT | sort_kind::BOOL | sort_kind::STRING | sort_kind::CHAR
    ) || exact_sort_integer_width(kind, sort_kind::SIGNED_EXACT_BASE).is_some()
        || exact_sort_integer_width(kind, sort_kind::UNSIGNED_EXACT_BASE).is_some()
}

fn compare_exact_signed(left: i64, right: i64, width: u32) -> CmpOrdering {
    let shift = 64 - width;
    let left = (((left as u64) << shift) as i64) >> shift;
    let right = (((right as u64) << shift) as i64) >> shift;
    left.cmp(&right)
}

fn compare_exact_unsigned(left: i64, right: i64, width: u32) -> CmpOrdering {
    if width == 64 {
        return (left as u64).cmp(&(right as u64));
    }
    let mask = (1_u64 << width) - 1;
    ((left as u64) & mask).cmp(&((right as u64) & mask))
}

fn compare_spectra_strings(left: i64, right: i64) -> CmpOrdering {
    let left_is_null = left == 0;
    let right_is_null = right == 0;
    match (left_is_null, right_is_null) {
        (true, true) => return CmpOrdering::Equal,
        (true, false) => return CmpOrdering::Less,
        (false, true) => return CmpOrdering::Greater,
        (false, false) => {}
    }

    // Spectra string values are valid NUL-terminated UTF-8 pointers. The
    // compiler supplies this comparison mode only for a statically typed
    // `List<string>`; null is accepted as the empty-string sentinel.
    unsafe {
        CStr::from_ptr(left as *const c_char)
            .to_bytes()
            .cmp(CStr::from_ptr(right as *const c_char).to_bytes())
    }
}

pub(crate) fn compare_list_sort_values(left: i64, right: i64, kind: i64) -> CmpOrdering {
    use spectra_contract::collection_sort as sort_kind;

    match kind {
        sort_kind::INT => left.cmp(&right),
        sort_kind::FLOAT => f64::from_bits(left as u64).total_cmp(&f64::from_bits(right as u64)),
        sort_kind::BOOL => (left != 0).cmp(&(right != 0)),
        sort_kind::STRING => compare_spectra_strings(left, right),
        sort_kind::CHAR => (left as u32).cmp(&(right as u32)),
        _ => {
            if let Some(width) = exact_sort_integer_width(kind, sort_kind::SIGNED_EXACT_BASE) {
                compare_exact_signed(left, right, width)
            } else if let Some(width) =
                exact_sort_integer_width(kind, sort_kind::UNSIGNED_EXACT_BASE)
            {
                compare_exact_unsigned(left, right, width)
            } else {
                CmpOrdering::Equal
            }
        }
    }
}

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
        list.data.push_back(value);
        Ok(list.data.len())
    }

    pub(crate) fn extend<I>(&mut self, handle: usize, values: I) -> Result<(), i32>
    where
        I: IntoIterator<Item = SpectraHostValue>,
    {
        let id = Self::id(handle)?;
        let list = self.lists.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        list.data.extend(values);
        Ok(())
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

    pub(crate) fn get_option(
        &self,
        handle: usize,
        index: i64,
    ) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        if index < 0 || (index as usize) >= list.data.len() {
            return Ok(None);
        }
        Ok(Some(list.data[index as usize]))
    }

    pub(crate) fn set(
        &mut self,
        handle: usize,
        index: i64,
        value: SpectraHostValue,
    ) -> Result<(), i32> {
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
        Ok(list.data.pop_back())
    }

    pub(crate) fn pop_front_option(
        &mut self,
        handle: usize,
    ) -> Result<Option<SpectraHostValue>, i32> {
        let id = Self::id(handle)?;
        let list = self.lists.get_mut(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
        Ok(list.data.pop_front())
    }

    pub(crate) fn insert_at(
        &mut self,
        handle: usize,
        index: i64,
        value: SpectraHostValue,
    ) -> Result<(), i32> {
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
        Ok(list.data.remove(index as usize))
    }

    pub(crate) fn index_of(
        &self,
        handle: usize,
        value: SpectraHostValue,
    ) -> Result<SpectraHostValue, i32> {
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

    pub(crate) fn sort_asc_typed(&mut self, handle: usize, kind: i64) -> Result<(), i32> {
        if !is_supported_list_sort_kind(kind) {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }
        let id = Self::id(handle)?;
        self.lists
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data
            .make_contiguous()
            .sort_by(|left, right| compare_list_sort_values(*left, *right, kind));
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
            .iter()
            .copied()
            .collect())
    }

    /// Replaces a list's data with `data` (used after an out-of-lock sort/transform).
    pub(crate) fn restore(
        &mut self,
        handle: usize,
        data: Vec<SpectraHostValue>,
    ) -> Result<(), i32> {
        let id = Self::id(handle)?;
        self.lists
            .get_mut(id)
            .map_err(|_| HOST_STATUS_NOT_FOUND)?
            .data = data.into_iter().collect();
        Ok(())
    }
}

#[cfg(test)]
mod list_sort_tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn strings_are_compared_by_utf8_contents_and_prefix() {
        let alphabetic = CString::new("src/a.spectra").expect("valid string");
        let nested = CString::new("src/a/sub.spectra").expect("valid string");
        let later = CString::new("src/z.spectra").expect("valid string");
        let unicode = CString::new("src/é.spectra").expect("valid string");

        assert_eq!(
            compare_list_sort_values(
                alphabetic.as_ptr() as i64,
                nested.as_ptr() as i64,
                spectra_contract::collection_sort::STRING,
            ),
            CmpOrdering::Less
        );
        assert_eq!(
            compare_list_sort_values(
                nested.as_ptr() as i64,
                later.as_ptr() as i64,
                spectra_contract::collection_sort::STRING,
            ),
            CmpOrdering::Less
        );
        assert_eq!(
            compare_list_sort_values(
                later.as_ptr() as i64,
                unicode.as_ptr() as i64,
                spectra_contract::collection_sort::STRING,
            ),
            CmpOrdering::Less
        );
        assert_eq!(
            compare_list_sort_values(
                0,
                alphabetic.as_ptr() as i64,
                spectra_contract::collection_sort::STRING,
            ),
            CmpOrdering::Less
        );
    }

    #[test]
    fn scalar_sort_kinds_use_their_numeric_order() {
        use spectra_contract::collection_sort as kind;

        assert_eq!(
            compare_list_sort_values(-7, 3, kind::INT),
            CmpOrdering::Less
        );
        assert_eq!(
            compare_list_sort_values(
                (-1.5_f64).to_bits() as i64,
                0.25_f64.to_bits() as i64,
                kind::FLOAT
            ),
            CmpOrdering::Less
        );
        assert_eq!(
            compare_list_sort_values(0, 1, kind::BOOL),
            CmpOrdering::Less
        );
        assert_eq!(
            compare_list_sort_values(65, 233, kind::CHAR),
            CmpOrdering::Less
        );
        assert_eq!(
            compare_list_sort_values(0xff, 1, kind::SIGNED_EXACT_BASE + 8),
            CmpOrdering::Less,
            "0xff in i8 storage represents -1"
        );
        assert_eq!(
            compare_list_sort_values(0xff, 1, kind::UNSIGNED_EXACT_BASE + 8),
            CmpOrdering::Greater,
            "0xff in u8 storage represents 255"
        );
        assert_eq!(
            compare_list_sort_values(i64::MIN, 0, kind::UNSIGNED_EXACT_BASE + 64),
            CmpOrdering::Greater,
            "u64 ordering must not reinterpret the high bit as a sign"
        );
    }

    #[test]
    fn unknown_sort_kind_is_rejected() {
        assert!(!is_supported_list_sort_kind(
            spectra_contract::collection_sort::INVALID
        ));
        assert!(!is_supported_list_sort_kind(999));
    }
}
