use super::*;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::hash::{Hash, Hasher};

const HASH_SET_NEW: &str = "spectra.std.collections.hash_set_new";
const HASH_SET_WITH_CAPACITY: &str = "spectra.std.collections.hash_set_with_capacity";
const HASH_SET_CAPACITY: &str = "spectra.std.collections.hash_set_capacity";
const HASH_SET_INSERT: &str = "spectra.std.collections.hash_set_insert";
const HASH_SET_CONTAINS: &str = "spectra.std.collections.hash_set_contains";
const HASH_SET_REMOVE: &str = "spectra.std.collections.hash_set_remove";
const HASH_SET_LEN: &str = "spectra.std.collections.hash_set_len";
const HASH_SET_CLEAR: &str = "spectra.std.collections.hash_set_clear";
const HASH_SET_ITER: &str = "spectra.std.collections.hash_set_iter";
const HASH_SET_FREE: &str = "spectra.std.collections.hash_set_free";

const ORDERED_MAP_NEW: &str = "spectra.std.collections.ordered_map_new";
const ORDERED_MAP_SET: &str = "spectra.std.collections.ordered_map_set";
const ORDERED_MAP_GET: &str = "spectra.std.collections.ordered_map_get";
const ORDERED_MAP_CONTAINS: &str = "spectra.std.collections.ordered_map_contains";
const ORDERED_MAP_REMOVE: &str = "spectra.std.collections.ordered_map_remove";
const ORDERED_MAP_LEN: &str = "spectra.std.collections.ordered_map_len";
const ORDERED_MAP_ITER: &str = "spectra.std.collections.ordered_map_iter";
const ORDERED_MAP_RANGE_KEYS: &str = "spectra.std.collections.ordered_map_range_keys";
const ORDERED_MAP_FREE: &str = "spectra.std.collections.ordered_map_free";

const PRIORITY_QUEUE_NEW: &str = "spectra.std.collections.priority_queue_new";
const PRIORITY_QUEUE_NEW_MIN: &str = "spectra.std.collections.priority_queue_new_min";
const PRIORITY_QUEUE_WITH_CAPACITY: &str = "spectra.std.collections.priority_queue_with_capacity";
const PRIORITY_QUEUE_CAPACITY: &str = "spectra.std.collections.priority_queue_capacity";
const PRIORITY_QUEUE_PUSH: &str = "spectra.std.collections.priority_queue_push";
const PRIORITY_QUEUE_PEEK: &str = "spectra.std.collections.priority_queue_peek";
const PRIORITY_QUEUE_POP: &str = "spectra.std.collections.priority_queue_pop";
const PRIORITY_QUEUE_LEN: &str = "spectra.std.collections.priority_queue_len";
const PRIORITY_QUEUE_CLEAR: &str = "spectra.std.collections.priority_queue_clear";
const PRIORITY_QUEUE_FREE: &str = "spectra.std.collections.priority_queue_free";

const BITSET_NEW: &str = "spectra.std.collections.bitset_new";
const BITSET_WITH_CAPACITY: &str = "spectra.std.collections.bitset_with_capacity";
const BITSET_CAPACITY: &str = "spectra.std.collections.bitset_capacity";
const BITSET_INSERT: &str = "spectra.std.collections.bitset_insert";
const BITSET_REMOVE: &str = "spectra.std.collections.bitset_remove";
const BITSET_CONTAINS: &str = "spectra.std.collections.bitset_contains";
const BITSET_COUNT: &str = "spectra.std.collections.bitset_count";
const BITSET_UNION_WITH: &str = "spectra.std.collections.bitset_union_with";
const BITSET_INTERSECT_WITH: &str = "spectra.std.collections.bitset_intersect_with";
const BITSET_DIFFERENCE_WITH: &str = "spectra.std.collections.bitset_difference_with";
const BITSET_ITER: &str = "spectra.std.collections.bitset_iter";
const BITSET_FREE: &str = "spectra.std.collections.bitset_free";

const DISJOINT_SET_NEW: &str = "spectra.std.collections.disjoint_set_new";
const DISJOINT_SET_ADD: &str = "spectra.std.collections.disjoint_set_add";
const DISJOINT_SET_FIND: &str = "spectra.std.collections.disjoint_set_find";
const DISJOINT_SET_UNION: &str = "spectra.std.collections.disjoint_set_union";
const DISJOINT_SET_CONNECTED: &str = "spectra.std.collections.disjoint_set_connected";
const DISJOINT_SET_COUNT: &str = "spectra.std.collections.disjoint_set_count";
const DISJOINT_SET_FREE: &str = "spectra.std.collections.disjoint_set_free";

pub(crate) fn register_advanced_collections() {
    let registrations: &[(&str, crate::ffi::HostFunction)] = &[
        (HASH_SET_NEW, std_hash_set_new),
        (HASH_SET_WITH_CAPACITY, std_hash_set_with_capacity),
        (HASH_SET_CAPACITY, std_hash_set_capacity),
        (HASH_SET_INSERT, std_hash_set_insert),
        (HASH_SET_CONTAINS, std_hash_set_contains),
        (HASH_SET_REMOVE, std_hash_set_remove),
        (HASH_SET_LEN, std_hash_set_len),
        (HASH_SET_CLEAR, std_hash_set_clear),
        (HASH_SET_ITER, std_hash_set_iter),
        (HASH_SET_FREE, std_hash_set_free),
        (ORDERED_MAP_NEW, std_ordered_map_new),
        (ORDERED_MAP_SET, std_ordered_map_set),
        (ORDERED_MAP_GET, std_ordered_map_get),
        (ORDERED_MAP_CONTAINS, std_ordered_map_contains),
        (ORDERED_MAP_REMOVE, std_ordered_map_remove),
        (ORDERED_MAP_LEN, std_ordered_map_len),
        (ORDERED_MAP_ITER, std_ordered_map_iter),
        (ORDERED_MAP_RANGE_KEYS, std_ordered_map_range_keys),
        (ORDERED_MAP_FREE, std_ordered_map_free),
        (PRIORITY_QUEUE_NEW, std_priority_queue_new),
        (PRIORITY_QUEUE_NEW_MIN, std_priority_queue_new_min),
        (
            PRIORITY_QUEUE_WITH_CAPACITY,
            std_priority_queue_with_capacity,
        ),
        (PRIORITY_QUEUE_CAPACITY, std_priority_queue_capacity),
        (PRIORITY_QUEUE_PUSH, std_priority_queue_push),
        (PRIORITY_QUEUE_PEEK, std_priority_queue_peek),
        (PRIORITY_QUEUE_POP, std_priority_queue_pop),
        (PRIORITY_QUEUE_LEN, std_priority_queue_len),
        (PRIORITY_QUEUE_CLEAR, std_priority_queue_clear),
        (PRIORITY_QUEUE_FREE, std_priority_queue_free),
        (BITSET_NEW, std_bitset_new),
        (BITSET_WITH_CAPACITY, std_bitset_with_capacity),
        (BITSET_CAPACITY, std_bitset_capacity),
        (BITSET_INSERT, std_bitset_insert),
        (BITSET_REMOVE, std_bitset_remove),
        (BITSET_CONTAINS, std_bitset_contains),
        (BITSET_COUNT, std_bitset_count),
        (BITSET_UNION_WITH, std_bitset_union_with),
        (BITSET_INTERSECT_WITH, std_bitset_intersect_with),
        (BITSET_DIFFERENCE_WITH, std_bitset_difference_with),
        (BITSET_ITER, std_bitset_iter),
        (BITSET_FREE, std_bitset_free),
        (DISJOINT_SET_NEW, std_disjoint_set_new),
        (DISJOINT_SET_ADD, std_disjoint_set_add),
        (DISJOINT_SET_FIND, std_disjoint_set_find),
        (DISJOINT_SET_UNION, std_disjoint_set_union),
        (DISJOINT_SET_CONNECTED, std_disjoint_set_connected),
        (DISJOINT_SET_COUNT, std_disjoint_set_count),
        (DISJOINT_SET_FREE, std_disjoint_set_free),
    ];
    for (name, function) in registrations {
        register_host_function(name, *function);
    }
}

fn with_result_call(
    ctx: *mut SpectraHostCallContext,
    expected_args: usize,
    action: impl FnOnce(&[SpectraHostValue]) -> Result<SpectraHostValue, i32>,
) -> i32 {
    let Ok((args, results)) = host_call_args(ctx, expected_args) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    match action(args) {
        Ok(value) => {
            results[0] = value;
            HOST_STATUS_SUCCESS
        }
        Err(code) => code,
    }
}

fn with_void_call(
    ctx: *mut SpectraHostCallContext,
    expected_args: usize,
    action: impl FnOnce(&[SpectraHostValue]) -> Result<(), i32>,
) -> i32 {
    let Ok(args) = host_call_void_args(ctx, expected_args) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    action(args).map_or_else(|code| code, |_| HOST_STATUS_SUCCESS)
}

fn checked_capacity(raw: SpectraHostValue, element_size: usize) -> Result<usize, i32> {
    let capacity = usize::try_from(raw).map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    let bytes = capacity
        .checked_mul(element_size)
        .ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
    if bytes > isize::MAX as usize {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    Ok(capacity)
}

fn allocate_handle<T>(value: T) -> Result<ManualBox<T>, i32> {
    initialize()
        .memory()
        .allocate_manual(value)
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)
}

#[derive(Clone, Debug)]
struct AdvancedKey {
    kind: i64,
    value: CollectionKey,
}

impl AdvancedKey {
    fn new(raw: SpectraHostValue, kind: i64) -> Result<Self, i32> {
        if !list_methods::is_supported_list_sort_kind(kind) {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }

        let value = if kind == spectra_contract::collection_sort::STRING {
            if raw == 0 {
                CollectionKey::String {
                    value: String::new(),
                    raw,
                }
            } else {
                let limit =
                    crate::ffi::manual_allocation_size(raw).ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
                let bytes = unsafe { slice::from_raw_parts(raw as *const u8, limit) };
                let nul = bytes
                    .iter()
                    .position(|byte| *byte == 0)
                    .ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
                if bytes[nul..].iter().any(|byte| *byte != 0) {
                    return Err(HOST_STATUS_INVALID_ARGUMENT);
                }
                let source =
                    std::str::from_utf8(&bytes[..nul]).map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
                let mut owned = String::new();
                owned
                    .try_reserve_exact(source.len())
                    .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
                owned.push_str(source);
                CollectionKey::String { value: owned, raw }
            }
        } else {
            CollectionKey::Scalar(normalize_scalar_key(raw, kind)?)
        };
        Ok(Self { kind, value })
    }

    fn raw_value(&self) -> SpectraHostValue {
        self.value.raw_value()
    }
}

fn normalize_scalar_key(raw: SpectraHostValue, kind: i64) -> Result<i64, i32> {
    use spectra_contract::collection_sort as sort_kind;
    if kind == sort_kind::BOOL {
        return Ok((raw != 0) as i64);
    }
    if kind == sort_kind::CHAR {
        let character = u32::try_from(raw).map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
        if char::from_u32(character).is_none() {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }
        return Ok(character as i64);
    }
    let signed_width = kind - sort_kind::SIGNED_EXACT_BASE;
    if matches!(signed_width, 8 | 16 | 32 | 64) {
        let shift = 64 - signed_width as u32;
        return Ok(((((raw as u64) << shift) as i64) >> shift) as i64);
    }
    let unsigned_width = kind - sort_kind::UNSIGNED_EXACT_BASE;
    if matches!(unsigned_width, 8 | 16 | 32 | 64) {
        if unsigned_width == 64 {
            return Ok(raw);
        }
        let mask = (1_u64 << unsigned_width) - 1;
        return Ok(((raw as u64) & mask) as i64);
    }
    Ok(raw)
}

impl PartialEq for AdvancedKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for AdvancedKey {}

impl PartialOrd for AdvancedKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AdvancedKey {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.kind != other.kind {
            return self.kind.cmp(&other.kind);
        }
        if self.kind == spectra_contract::collection_sort::STRING {
            return self.value.cmp(&other.value);
        }
        list_methods::compare_list_sort_values(
            self.value.raw_value(),
            other.value.raw_value(),
            self.kind,
        )
    }
}

impl Hash for AdvancedKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.kind.hash(state);
        self.value.hash(state);
    }
}

#[derive(Default)]
struct StdHashSet {
    values: HashSet<AdvancedKey>,
    kind: Option<i64>,
}

struct HashSetRegistry {
    values: HandleTable<ManualBox<StdHashSet>>,
}

impl HashSetRegistry {
    fn new() -> Self {
        Self {
            values: HandleTable::new(HandleKind::HashSet),
        }
    }

    fn insert(&mut self, value: ManualBox<StdHashSet>) -> usize {
        self.values.insert(value).raw() as usize
    }

    fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }
}

fn with_hash_sets<F, R>(action: F) -> R
where
    F: FnOnce(&mut HashSetRegistry) -> R,
{
    static REGISTRY: OnceLock<Mutex<HashSetRegistry>> = OnceLock::new();
    let mut registry = lock_unpoisoned(REGISTRY.get_or_init(|| Mutex::new(HashSetRegistry::new())));
    action(&mut registry)
}

pub(crate) extern "C" fn std_hash_set_new(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 0, |_| {
        let value = allocate_handle(StdHashSet::default())?;
        Ok(with_hash_sets(|registry| registry.insert(value)) as SpectraHostValue)
    })
}

pub(crate) extern "C" fn std_hash_set_with_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 1, |args| {
        let capacity = checked_capacity(args[0], size_of::<AdvancedKey>())?;
        let mut data = StdHashSet::default();
        data.values
            .try_reserve(capacity)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        let value = allocate_handle(data)?;
        Ok(with_hash_sets(|registry| registry.insert(value)) as SpectraHostValue)
    })
}

pub(crate) extern "C" fn std_hash_set_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 1, |args| {
        with_hash_sets(|registry| {
            let id = HashSetRegistry::id(args[0] as usize)?;
            let set = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            i64::try_from(set.values.capacity()).map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

pub(crate) extern "C" fn std_hash_set_insert(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 3, |args| {
        let handle = args[0] as usize;
        let key = AdvancedKey::new(args[1], args[2])?;
        crate::ffi::escape_stored_value(key.raw_value());
        with_hash_sets(|registry| {
            let id = HashSetRegistry::id(handle)?;
            let set = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            if set.kind.is_some_and(|kind| kind != key.kind) {
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            if set.values.contains(&key) {
                return Ok(0);
            }
            set.values
                .try_reserve(1)
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
            set.kind.get_or_insert(key.kind);
            set.values.insert(key);
            Ok(1)
        })
    })
}

pub(crate) extern "C" fn std_hash_set_contains(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 3, |args| {
        let key = AdvancedKey::new(args[1], args[2])?;
        with_hash_sets(|registry| {
            let id = HashSetRegistry::id(args[0] as usize)?;
            let set = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            if set.kind.is_some_and(|kind| kind != key.kind) {
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            Ok(set.values.contains(&key) as SpectraHostValue)
        })
    })
}

pub(crate) extern "C" fn std_hash_set_remove(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 3, |args| {
        let key = AdvancedKey::new(args[1], args[2])?;
        with_hash_sets(|registry| {
            let id = HashSetRegistry::id(args[0] as usize)?;
            let set = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            if set.kind.is_some_and(|kind| kind != key.kind) {
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            Ok(set.values.remove(&key) as SpectraHostValue)
        })
    })
}

pub(crate) extern "C" fn std_hash_set_len(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 1, |args| {
        with_hash_sets(|registry| {
            let id = HashSetRegistry::id(args[0] as usize)?;
            let set = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            i64::try_from(set.values.len()).map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

pub(crate) extern "C" fn std_hash_set_clear(ctx: *mut SpectraHostCallContext) -> i32 {
    with_void_call(ctx, 1, |args| {
        with_hash_sets(|registry| {
            let id = HashSetRegistry::id(args[0] as usize)?;
            let set = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            set.values.clear();
            Ok(())
        })
    })
}

pub(crate) extern "C" fn std_hash_set_iter(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 1, |args| {
        let snapshot = with_hash_sets(|registry| {
            let id = HashSetRegistry::id(args[0] as usize)?;
            let set = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            let mut values = Vec::new();
            values
                .try_reserve_exact(set.values.len())
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
            values.extend(set.values.iter().map(AdvancedKey::raw_value));
            Ok::<Vec<SpectraHostValue>, i32>(values)
        })?;
        Ok(insert_iterator(snapshot)? as SpectraHostValue)
    })
}

pub(crate) extern "C" fn std_hash_set_free(ctx: *mut SpectraHostCallContext) -> i32 {
    with_void_call(ctx, 1, |args| {
        with_hash_sets(|registry| {
            let id = HashSetRegistry::id(args[0] as usize)?;
            registry
                .values
                .remove(id)
                .map(|_| ())
                .map_err(|_| HOST_STATUS_NOT_FOUND)
        })
    })
}

#[derive(Clone)]
struct AvlNode {
    key: AdvancedKey,
    value: SpectraHostValue,
    left: Option<usize>,
    right: Option<usize>,
    height: u32,
}

#[derive(Default)]
struct StdOrderedMap {
    nodes: Vec<Option<AvlNode>>,
    free: Vec<usize>,
    root: Option<usize>,
    len: usize,
    kind: Option<i64>,
}

impl StdOrderedMap {
    fn node(&self, index: usize) -> &AvlNode {
        self.nodes[index]
            .as_ref()
            .expect("live AVL links reference occupied nodes")
    }

    fn node_mut(&mut self, index: usize) -> &mut AvlNode {
        self.nodes[index]
            .as_mut()
            .expect("live AVL links reference occupied nodes")
    }

    fn height(&self, index: Option<usize>) -> u32 {
        index.map_or(0, |index| self.node(index).height)
    }

    fn fix_height(&mut self, index: usize) {
        let (left, right) = {
            let node = self.node(index);
            (node.left, node.right)
        };
        self.node_mut(index).height = 1 + self.height(left).max(self.height(right));
    }

    fn balance_factor(&self, index: usize) -> i64 {
        let node = self.node(index);
        self.height(node.left) as i64 - self.height(node.right) as i64
    }

    fn rotate_right(&mut self, root: usize) -> usize {
        let pivot = self
            .node(root)
            .left
            .expect("right rotation requires a left child");
        let transfer = self.node(pivot).right;
        self.node_mut(root).left = transfer;
        self.fix_height(root);
        self.node_mut(pivot).right = Some(root);
        self.fix_height(pivot);
        pivot
    }

    fn rotate_left(&mut self, root: usize) -> usize {
        let pivot = self
            .node(root)
            .right
            .expect("left rotation requires a right child");
        let transfer = self.node(pivot).left;
        self.node_mut(root).right = transfer;
        self.fix_height(root);
        self.node_mut(pivot).left = Some(root);
        self.fix_height(pivot);
        pivot
    }

    fn rebalance(&mut self, index: usize) -> usize {
        self.fix_height(index);
        let factor = self.balance_factor(index);
        if factor > 1 {
            let left = self
                .node(index)
                .left
                .expect("positive balance has left child");
            if self.balance_factor(left) < 0 {
                let rotated = self.rotate_left(left);
                self.node_mut(index).left = Some(rotated);
            }
            return self.rotate_right(index);
        }
        if factor < -1 {
            let right = self
                .node(index)
                .right
                .expect("negative balance has right child");
            if self.balance_factor(right) > 0 {
                let rotated = self.rotate_right(right);
                self.node_mut(index).right = Some(rotated);
            }
            return self.rotate_left(index);
        }
        index
    }

    fn find_index(&self, key: &AdvancedKey) -> Option<usize> {
        let mut current = self.root;
        while let Some(index) = current {
            let node = self.node(index);
            match key.cmp(&node.key) {
                Ordering::Less => current = node.left,
                Ordering::Greater => current = node.right,
                Ordering::Equal => return Some(index),
            }
        }
        None
    }

    fn insert_link(&mut self, root: Option<usize>, inserted: usize) -> Option<usize> {
        let Some(index) = root else {
            return Some(inserted);
        };
        let order = self.node(inserted).key.cmp(&self.node(index).key);
        if order == Ordering::Less {
            let child = self.node(index).left;
            let inserted_root = self.insert_link(child, inserted);
            self.node_mut(index).left = inserted_root;
        } else {
            let child = self.node(index).right;
            let inserted_root = self.insert_link(child, inserted);
            self.node_mut(index).right = inserted_root;
        }
        Some(self.rebalance(index))
    }

    fn insert(&mut self, key: AdvancedKey, value: SpectraHostValue) -> Result<(), i32> {
        if let Some(index) = self.find_index(&key) {
            self.node_mut(index).value = value;
            return Ok(());
        }
        let index = if let Some(index) = self.free.pop() {
            index
        } else {
            self.nodes
                .try_reserve(1)
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
            self.nodes.len()
        };
        let node = AvlNode {
            key,
            value,
            left: None,
            right: None,
            height: 1,
        };
        if index == self.nodes.len() {
            self.nodes.push(Some(node));
        } else {
            self.nodes[index] = Some(node);
        }
        self.root = self.insert_link(self.root, index);
        self.len += 1;
        Ok(())
    }

    fn detach_min(&mut self, root: usize) -> (Option<usize>, usize) {
        let left = self.node(root).left;
        let Some(left) = left else {
            let right = self.node(root).right;
            return (right, root);
        };
        let (new_left, minimum) = self.detach_min(left);
        self.node_mut(root).left = new_left;
        (Some(self.rebalance(root)), minimum)
    }

    fn remove_link(
        &mut self,
        root: Option<usize>,
        key: &AdvancedKey,
        removed: &mut Option<SpectraHostValue>,
    ) -> Option<usize> {
        let index = root?;
        match key.cmp(&self.node(index).key) {
            Ordering::Less => {
                let child = self.node(index).left;
                let new_child = self.remove_link(child, key, removed);
                self.node_mut(index).left = new_child;
                Some(self.rebalance(index))
            }
            Ordering::Greater => {
                let child = self.node(index).right;
                let new_child = self.remove_link(child, key, removed);
                self.node_mut(index).right = new_child;
                Some(self.rebalance(index))
            }
            Ordering::Equal => {
                *removed = Some(self.node(index).value);
                let (left, right) = {
                    let node = self.node(index);
                    (node.left, node.right)
                };
                match (left, right) {
                    (None, child) | (child, None) => {
                        self.nodes[index] = None;
                        self.free.push(index);
                        child
                    }
                    (Some(_), Some(right)) => {
                        let (new_right, successor) = self.detach_min(right);
                        let successor_node = self.nodes[successor]
                            .take()
                            .expect("successor is a live AVL node");
                        let node = self.node_mut(index);
                        node.key = successor_node.key;
                        node.value = successor_node.value;
                        node.right = new_right;
                        self.free.push(successor);
                        Some(self.rebalance(index))
                    }
                }
            }
        }
    }

    fn remove(&mut self, key: &AdvancedKey) -> Result<Option<SpectraHostValue>, i32> {
        if self.find_index(key).is_none() {
            return Ok(None);
        }
        self.free
            .try_reserve(1)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        let mut removed = None;
        self.root = self.remove_link(self.root, key, &mut removed);
        self.len -= 1;
        Ok(removed)
    }

    fn collect_in_order(
        &self,
        root: Option<usize>,
        output: &mut Vec<SpectraHostValue>,
    ) -> Result<(), i32> {
        let mut pending = Vec::new();
        if self.len > 0 {
            pending
                .try_reserve(self.height(self.root) as usize)
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        }
        let mut current = root;
        loop {
            while let Some(index) = current {
                pending.push(index);
                current = self.node(index).left;
            }
            let Some(index) = pending.pop() else {
                break;
            };
            let node = self.node(index);
            output.push(node.key.raw_value());
            current = node.right;
        }
        Ok(())
    }

    fn keys_snapshot(&self) -> Result<Vec<SpectraHostValue>, i32> {
        let mut values = Vec::new();
        values
            .try_reserve_exact(self.len)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        self.collect_in_order(self.root, &mut values)?;
        Ok(values)
    }

    fn collect_range(
        &self,
        root: Option<usize>,
        start: &AdvancedKey,
        end: &AdvancedKey,
        output: &mut Vec<SpectraHostValue>,
    ) -> Result<(), i32> {
        let Some(index) = root else {
            return Ok(());
        };
        let node = self.node(index);
        let key_order_start = node.key.cmp(start);
        let key_order_end = node.key.cmp(end);
        let left = node.left;
        let right = node.right;
        let value = node.key.raw_value();
        if key_order_start != Ordering::Less {
            self.collect_range(left, start, end, output)?;
        }
        if key_order_start != Ordering::Less && key_order_end == Ordering::Less {
            if output.len() == output.capacity() {
                output
                    .try_reserve(1)
                    .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
            }
            output.push(value);
        }
        if key_order_end == Ordering::Less {
            self.collect_range(right, start, end, output)?;
        }
        Ok(())
    }

    fn range_snapshot(
        &self,
        start: &AdvancedKey,
        end: &AdvancedKey,
    ) -> Result<Vec<SpectraHostValue>, i32> {
        let mut values = Vec::new();
        if start < end {
            self.collect_range(self.root, start, end, &mut values)?;
        }
        Ok(values)
    }
}

struct OrderedMapRegistry {
    values: HandleTable<ManualBox<StdOrderedMap>>,
}

impl OrderedMapRegistry {
    fn new() -> Self {
        Self {
            values: HandleTable::new(HandleKind::OrderedMap),
        }
    }

    fn insert(&mut self, value: ManualBox<StdOrderedMap>) -> usize {
        self.values.insert(value).raw() as usize
    }

    fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }
}

fn with_ordered_maps<F, R>(action: F) -> R
where
    F: FnOnce(&mut OrderedMapRegistry) -> R,
{
    static REGISTRY: OnceLock<Mutex<OrderedMapRegistry>> = OnceLock::new();
    let mut registry =
        lock_unpoisoned(REGISTRY.get_or_init(|| Mutex::new(OrderedMapRegistry::new())));
    action(&mut registry)
}

pub(crate) extern "C" fn std_ordered_map_new(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 0, |_| {
        let value = allocate_handle(StdOrderedMap::default())?;
        Ok(with_ordered_maps(|registry| registry.insert(value)) as SpectraHostValue)
    })
}

pub(crate) extern "C" fn std_ordered_map_set(ctx: *mut SpectraHostCallContext) -> i32 {
    with_void_call(ctx, 4, |args| {
        let handle = args[0] as usize;
        let key = AdvancedKey::new(args[1], args[3])?;
        crate::ffi::escape_stored_value(key.raw_value());
        crate::ffi::escape_stored_value(args[2]);
        with_ordered_maps(|registry| {
            let id = OrderedMapRegistry::id(handle)?;
            let map = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            if map.kind.is_some_and(|kind| kind != key.kind) {
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            let kind = key.kind;
            map.insert(key, args[2])?;
            map.kind.get_or_insert(kind);
            Ok(())
        })
    })
}

fn with_ordered_map_option(
    ctx: *mut SpectraHostCallContext,
    expected_args: usize,
    action: impl FnOnce(&[SpectraHostValue]) -> Result<Option<SpectraHostValue>, i32>,
) -> i32 {
    let result = {
        let Ok(args) = host_call_void_args(ctx, expected_args) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        action(args)
    };
    match result {
        Ok(value) => unsafe { write_option_result(&mut *ctx, value) },
        Err(code) => code,
    }
}

pub(crate) extern "C" fn std_ordered_map_get(ctx: *mut SpectraHostCallContext) -> i32 {
    with_ordered_map_option(ctx, 3, |args| {
        let key = AdvancedKey::new(args[1], args[2])?;
        with_ordered_maps(|registry| {
            let id = OrderedMapRegistry::id(args[0] as usize)?;
            let map = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            if map.kind.is_some_and(|kind| kind != key.kind) {
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            Ok(map.find_index(&key).map(|index| map.node(index).value))
        })
    })
}

pub(crate) extern "C" fn std_ordered_map_contains(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 3, |args| {
        let key = AdvancedKey::new(args[1], args[2])?;
        with_ordered_maps(|registry| {
            let id = OrderedMapRegistry::id(args[0] as usize)?;
            let map = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            if map.kind.is_some_and(|kind| kind != key.kind) {
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            Ok(map.find_index(&key).is_some() as SpectraHostValue)
        })
    })
}

pub(crate) extern "C" fn std_ordered_map_remove(ctx: *mut SpectraHostCallContext) -> i32 {
    with_ordered_map_option(ctx, 3, |args| {
        let key = AdvancedKey::new(args[1], args[2])?;
        with_ordered_maps(|registry| {
            let id = OrderedMapRegistry::id(args[0] as usize)?;
            let map = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            if map.kind.is_some_and(|kind| kind != key.kind) {
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            map.remove(&key)
        })
    })
}

pub(crate) extern "C" fn std_ordered_map_len(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 1, |args| {
        with_ordered_maps(|registry| {
            let id = OrderedMapRegistry::id(args[0] as usize)?;
            let map = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            i64::try_from(map.len).map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

fn ordered_map_iterator_call(
    ctx: *mut SpectraHostCallContext,
    action: impl FnOnce(usize) -> Result<Vec<SpectraHostValue>, i32>,
) -> i32 {
    with_result_call(ctx, 1, |args| {
        let snapshot = action(args[0] as usize)?;
        Ok(insert_iterator(snapshot)? as SpectraHostValue)
    })
}

pub(crate) extern "C" fn std_ordered_map_iter(ctx: *mut SpectraHostCallContext) -> i32 {
    ordered_map_iterator_call(ctx, |handle| {
        with_ordered_maps(|registry| {
            let id = OrderedMapRegistry::id(handle)?;
            registry
                .values
                .get(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?
                .keys_snapshot()
        })
    })
}

fn ordered_map_range_call(
    ctx: *mut SpectraHostCallContext,
    action: impl FnOnce(&[SpectraHostValue]) -> Result<Vec<SpectraHostValue>, i32>,
) -> i32 {
    with_result_call(ctx, 4, |args| {
        let snapshot = action(args)?;
        Ok(insert_iterator(snapshot)? as SpectraHostValue)
    })
}

pub(crate) extern "C" fn std_ordered_map_range_keys(ctx: *mut SpectraHostCallContext) -> i32 {
    ordered_map_range_call(ctx, |args| {
        let start = AdvancedKey::new(args[1], args[3])?;
        let end = AdvancedKey::new(args[2], args[3])?;
        with_ordered_maps(|registry| {
            let id = OrderedMapRegistry::id(args[0] as usize)?;
            let map = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            if map.kind.is_some_and(|kind| kind != start.kind) {
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            map.range_snapshot(&start, &end)
        })
    })
}

pub(crate) extern "C" fn std_ordered_map_free(ctx: *mut SpectraHostCallContext) -> i32 {
    with_void_call(ctx, 1, |args| {
        with_ordered_maps(|registry| {
            let id = OrderedMapRegistry::id(args[0] as usize)?;
            registry
                .values
                .remove(id)
                .map(|_| ())
                .map_err(|_| HOST_STATUS_NOT_FOUND)
        })
    })
}

#[derive(Clone, Debug)]
struct PriorityEntry {
    key: AdvancedKey,
    min_first: bool,
}

impl PartialEq for PriorityEntry {
    fn eq(&self, other: &Self) -> bool {
        self.min_first == other.min_first && self.key == other.key
    }
}

impl Eq for PriorityEntry {}

impl PartialOrd for PriorityEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        let order = self.key.cmp(&other.key);
        if self.min_first {
            order.reverse()
        } else {
            order
        }
    }
}

struct StdPriorityQueue {
    values: BinaryHeap<PriorityEntry>,
    kind: Option<i64>,
    min_first: bool,
}

impl StdPriorityQueue {
    fn new(min_first: bool) -> Self {
        Self {
            values: BinaryHeap::new(),
            kind: None,
            min_first,
        }
    }
}

struct PriorityQueueRegistry {
    values: HandleTable<ManualBox<StdPriorityQueue>>,
}

impl PriorityQueueRegistry {
    fn new() -> Self {
        Self {
            values: HandleTable::new(HandleKind::PriorityQueue),
        }
    }

    fn insert(&mut self, value: ManualBox<StdPriorityQueue>) -> usize {
        self.values.insert(value).raw() as usize
    }

    fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }
}

fn with_priority_queues<F, R>(action: F) -> R
where
    F: FnOnce(&mut PriorityQueueRegistry) -> R,
{
    static REGISTRY: OnceLock<Mutex<PriorityQueueRegistry>> = OnceLock::new();
    let mut registry =
        lock_unpoisoned(REGISTRY.get_or_init(|| Mutex::new(PriorityQueueRegistry::new())));
    action(&mut registry)
}

fn create_priority_queue(
    min_first: bool,
    capacity: Option<usize>,
) -> Result<SpectraHostValue, i32> {
    let mut queue = StdPriorityQueue::new(min_first);
    if let Some(capacity) = capacity {
        queue
            .values
            .try_reserve(capacity)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    }
    let handle = allocate_handle(queue)?;
    Ok(with_priority_queues(|registry| registry.insert(handle)) as SpectraHostValue)
}

pub(crate) extern "C" fn std_priority_queue_new(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 0, |_| create_priority_queue(false, None))
}

pub(crate) extern "C" fn std_priority_queue_new_min(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 0, |_| create_priority_queue(true, None))
}

pub(crate) extern "C" fn std_priority_queue_with_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 1, |args| {
        let capacity = checked_capacity(args[0], size_of::<PriorityEntry>())?;
        create_priority_queue(false, Some(capacity))
    })
}

pub(crate) extern "C" fn std_priority_queue_push(ctx: *mut SpectraHostCallContext) -> i32 {
    with_void_call(ctx, 3, |args| {
        let key = AdvancedKey::new(args[1], args[2])?;
        crate::ffi::escape_stored_value(key.raw_value());
        with_priority_queues(|registry| {
            let id = PriorityQueueRegistry::id(args[0] as usize)?;
            let queue = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            if queue.kind.is_some_and(|kind| kind != key.kind) {
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            queue
                .values
                .try_reserve(1)
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
            queue.kind.get_or_insert(key.kind);
            let min_first = queue.min_first;
            queue.values.push(PriorityEntry { key, min_first });
            Ok(())
        })
    })
}

fn with_priority_queue_option(
    ctx: *mut SpectraHostCallContext,
    action: impl FnOnce(usize) -> Result<Option<SpectraHostValue>, i32>,
) -> i32 {
    let result = {
        let Ok(args) = host_call_void_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        action(args[0] as usize)
    };
    match result {
        Ok(value) => unsafe { write_option_result(&mut *ctx, value) },
        Err(code) => code,
    }
}

pub(crate) extern "C" fn std_priority_queue_peek(ctx: *mut SpectraHostCallContext) -> i32 {
    with_priority_queue_option(ctx, |handle| {
        with_priority_queues(|registry| {
            let id = PriorityQueueRegistry::id(handle)?;
            let queue = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            Ok(queue.values.peek().map(|entry| entry.key.raw_value()))
        })
    })
}

pub(crate) extern "C" fn std_priority_queue_pop(ctx: *mut SpectraHostCallContext) -> i32 {
    with_priority_queue_option(ctx, |handle| {
        with_priority_queues(|registry| {
            let id = PriorityQueueRegistry::id(handle)?;
            let queue = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            Ok(queue.values.pop().map(|entry| entry.key.raw_value()))
        })
    })
}

pub(crate) extern "C" fn std_priority_queue_len(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 1, |args| {
        with_priority_queues(|registry| {
            let id = PriorityQueueRegistry::id(args[0] as usize)?;
            let queue = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            i64::try_from(queue.values.len()).map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

pub(crate) extern "C" fn std_priority_queue_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 1, |args| {
        with_priority_queues(|registry| {
            let id = PriorityQueueRegistry::id(args[0] as usize)?;
            let queue = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            i64::try_from(queue.values.capacity()).map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

pub(crate) extern "C" fn std_priority_queue_clear(ctx: *mut SpectraHostCallContext) -> i32 {
    with_void_call(ctx, 1, |args| {
        with_priority_queues(|registry| {
            let id = PriorityQueueRegistry::id(args[0] as usize)?;
            let queue = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            queue.values.clear();
            Ok(())
        })
    })
}

pub(crate) extern "C" fn std_priority_queue_free(ctx: *mut SpectraHostCallContext) -> i32 {
    with_void_call(ctx, 1, |args| {
        with_priority_queues(|registry| {
            let id = PriorityQueueRegistry::id(args[0] as usize)?;
            registry
                .values
                .remove(id)
                .map(|_| ())
                .map_err(|_| HOST_STATUS_NOT_FOUND)
        })
    })
}

#[derive(Default)]
struct StdBitSet {
    words: Vec<u64>,
    count: usize,
}

impl StdBitSet {
    fn with_capacity(bits: usize) -> Result<Self, i32> {
        let word_count = bits.checked_add(63).ok_or(HOST_STATUS_INVALID_ARGUMENT)? / 64;
        let mut words = Vec::new();
        words
            .try_reserve_exact(word_count)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        Ok(Self { words, count: 0 })
    }

    fn word_and_mask(raw_index: SpectraHostValue) -> Result<(usize, u64), i32> {
        let index = usize::try_from(raw_index).map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
        let word = index / 64;
        word.checked_add(1).ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
        Ok((word, 1_u64 << (index % 64)))
    }

    fn ensure_word(&mut self, word: usize) -> Result<(), i32> {
        if word < self.words.len() {
            return Ok(());
        }
        let target_len = word.checked_add(1).ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
        let new_bytes = target_len
            .checked_mul(size_of::<u64>())
            .ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
        if new_bytes > isize::MAX as usize {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }
        self.words
            .try_reserve(target_len - self.words.len())
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        self.words.resize(target_len, 0);
        Ok(())
    }

    fn insert(&mut self, raw_index: SpectraHostValue) -> Result<bool, i32> {
        let (word, mask) = Self::word_and_mask(raw_index)?;
        self.ensure_word(word)?;
        let was_present = self.words[word] & mask != 0;
        if !was_present {
            self.words[word] |= mask;
            self.count += 1;
        }
        Ok(!was_present)
    }

    fn remove(&mut self, raw_index: SpectraHostValue) -> Result<bool, i32> {
        let (word, mask) = Self::word_and_mask(raw_index)?;
        let Some(bits) = self.words.get_mut(word) else {
            return Ok(false);
        };
        let was_present = *bits & mask != 0;
        if was_present {
            *bits &= !mask;
            self.count -= 1;
        }
        Ok(was_present)
    }

    fn contains(&self, raw_index: SpectraHostValue) -> Result<bool, i32> {
        let (word, mask) = Self::word_and_mask(raw_index)?;
        Ok(self.words.get(word).is_some_and(|bits| bits & mask != 0))
    }

    fn snapshot(&self) -> Result<Vec<SpectraHostValue>, i32> {
        let mut values = Vec::new();
        values
            .try_reserve_exact(self.count)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        for (word_index, word) in self.words.iter().copied().enumerate() {
            let mut bits = word;
            while bits != 0 {
                let offset = bits.trailing_zeros() as usize;
                let index = word_index
                    .checked_mul(64)
                    .and_then(|base| base.checked_add(offset))
                    .ok_or(HOST_STATUS_INTERNAL_ERROR)?;
                values.push(i64::try_from(index).map_err(|_| HOST_STATUS_INTERNAL_ERROR)?);
                bits &= bits - 1;
            }
        }
        Ok(values)
    }

    fn apply(&mut self, other: &[u64], operation: BitSetOperation) -> Result<(), i32> {
        if operation == BitSetOperation::Union && other.len() > self.words.len() {
            let target_bytes = other
                .len()
                .checked_mul(size_of::<u64>())
                .ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
            if target_bytes > isize::MAX as usize {
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            self.words
                .try_reserve(other.len() - self.words.len())
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
            self.words.resize(other.len(), 0);
        }
        for (index, word) in self.words.iter_mut().enumerate() {
            let rhs = other.get(index).copied().unwrap_or(0);
            *word = match operation {
                BitSetOperation::Union => *word | rhs,
                BitSetOperation::Intersection => *word & rhs,
                BitSetOperation::Difference => *word & !rhs,
            };
        }
        while self.words.last() == Some(&0) {
            self.words.pop();
        }
        self.count = self
            .words
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum();
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BitSetOperation {
    Union,
    Intersection,
    Difference,
}

struct BitSetRegistry {
    values: HandleTable<ManualBox<StdBitSet>>,
}

impl BitSetRegistry {
    fn new() -> Self {
        Self {
            values: HandleTable::new(HandleKind::BitSet),
        }
    }

    fn insert(&mut self, value: ManualBox<StdBitSet>) -> usize {
        self.values.insert(value).raw() as usize
    }

    fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }
}

fn with_bit_sets<F, R>(action: F) -> R
where
    F: FnOnce(&mut BitSetRegistry) -> R,
{
    static REGISTRY: OnceLock<Mutex<BitSetRegistry>> = OnceLock::new();
    let mut registry = lock_unpoisoned(REGISTRY.get_or_init(|| Mutex::new(BitSetRegistry::new())));
    action(&mut registry)
}

fn create_bit_set(bits: usize) -> Result<SpectraHostValue, i32> {
    let value = allocate_handle(StdBitSet::with_capacity(bits)?)?;
    Ok(with_bit_sets(|registry| registry.insert(value)) as SpectraHostValue)
}

pub(crate) extern "C" fn std_bitset_new(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 0, |_| create_bit_set(0))
}

pub(crate) extern "C" fn std_bitset_with_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 1, |args| {
        let bits = checked_capacity(args[0], size_of::<u8>())?;
        create_bit_set(bits)
    })
}

pub(crate) extern "C" fn std_bitset_insert(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 2, |args| {
        with_bit_sets(|registry| {
            let id = BitSetRegistry::id(args[0] as usize)?;
            let set = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            Ok(set.insert(args[1])? as SpectraHostValue)
        })
    })
}

pub(crate) extern "C" fn std_bitset_remove(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 2, |args| {
        with_bit_sets(|registry| {
            let id = BitSetRegistry::id(args[0] as usize)?;
            let set = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            Ok(set.remove(args[1])? as SpectraHostValue)
        })
    })
}

pub(crate) extern "C" fn std_bitset_contains(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 2, |args| {
        with_bit_sets(|registry| {
            let id = BitSetRegistry::id(args[0] as usize)?;
            let set = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            Ok(set.contains(args[1])? as SpectraHostValue)
        })
    })
}

pub(crate) extern "C" fn std_bitset_count(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 1, |args| {
        with_bit_sets(|registry| {
            let id = BitSetRegistry::id(args[0] as usize)?;
            let set = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            i64::try_from(set.count).map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

pub(crate) extern "C" fn std_bitset_capacity(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 1, |args| {
        with_bit_sets(|registry| {
            let id = BitSetRegistry::id(args[0] as usize)?;
            let set = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            let capacity = set
                .words
                .capacity()
                .checked_mul(64)
                .ok_or(HOST_STATUS_INTERNAL_ERROR)?;
            i64::try_from(capacity).map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

fn bitset_apply(ctx: *mut SpectraHostCallContext, operation: BitSetOperation) -> i32 {
    with_void_call(ctx, 2, |args| {
        let target_id = BitSetRegistry::id(args[0] as usize)?;
        let source_id = BitSetRegistry::id(args[1] as usize)?;
        with_bit_sets(|registry| {
            if target_id == source_id {
                return if operation == BitSetOperation::Difference {
                    let target = registry
                        .values
                        .get_mut(target_id)
                        .map_err(|_| HOST_STATUS_NOT_FOUND)?;
                    target.words.clear();
                    target.count = 0;
                    Ok(())
                } else {
                    registry
                        .values
                        .get(target_id)
                        .map(|_| ())
                        .map_err(|_| HOST_STATUS_NOT_FOUND)
                };
            }
            let source = registry
                .values
                .get(source_id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            let mut words = Vec::new();
            words
                .try_reserve_exact(source.words.len())
                .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
            words.extend_from_slice(&source.words);
            let target = registry
                .values
                .get_mut(target_id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            target.apply(&words, operation)
        })
    })
}

pub(crate) extern "C" fn std_bitset_union_with(ctx: *mut SpectraHostCallContext) -> i32 {
    bitset_apply(ctx, BitSetOperation::Union)
}

pub(crate) extern "C" fn std_bitset_intersect_with(ctx: *mut SpectraHostCallContext) -> i32 {
    bitset_apply(ctx, BitSetOperation::Intersection)
}

pub(crate) extern "C" fn std_bitset_difference_with(ctx: *mut SpectraHostCallContext) -> i32 {
    bitset_apply(ctx, BitSetOperation::Difference)
}

pub(crate) extern "C" fn std_bitset_iter(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 1, |args| {
        let snapshot = with_bit_sets(|registry| {
            let id = BitSetRegistry::id(args[0] as usize)?;
            registry
                .values
                .get(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?
                .snapshot()
        })?;
        Ok(insert_iterator(snapshot)? as SpectraHostValue)
    })
}

pub(crate) extern "C" fn std_bitset_free(ctx: *mut SpectraHostCallContext) -> i32 {
    with_void_call(ctx, 1, |args| {
        with_bit_sets(|registry| {
            let id = BitSetRegistry::id(args[0] as usize)?;
            registry
                .values
                .remove(id)
                .map(|_| ())
                .map_err(|_| HOST_STATUS_NOT_FOUND)
        })
    })
}

#[derive(Default)]
struct StdDisjointSet {
    parent: Vec<usize>,
    size: Vec<usize>,
    components: usize,
}

impl StdDisjointSet {
    fn with_size(raw_size: SpectraHostValue) -> Result<Self, i32> {
        let size = checked_capacity(raw_size, size_of::<usize>())?;
        let mut parent = Vec::new();
        let mut sizes = Vec::new();
        parent
            .try_reserve_exact(size)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        sizes
            .try_reserve_exact(size)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        parent.extend(0..size);
        sizes.resize(size, 1);
        Ok(Self {
            parent,
            size: sizes,
            components: size,
        })
    }

    fn add(&mut self) -> Result<usize, i32> {
        let id = self.parent.len();
        id.checked_add(1).ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
        self.parent
            .try_reserve(1)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        self.size
            .try_reserve(1)
            .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        self.parent.push(id);
        self.size.push(1);
        self.components += 1;
        Ok(id)
    }

    fn checked_id(&self, raw_id: SpectraHostValue) -> Result<usize, i32> {
        let id = usize::try_from(raw_id).map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
        if id >= self.parent.len() {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }
        Ok(id)
    }

    fn find(&mut self, raw_id: SpectraHostValue) -> Result<usize, i32> {
        let mut id = self.checked_id(raw_id)?;
        while self.parent[id] != id {
            let parent = self.parent[id];
            self.parent[id] = self.parent[parent];
            id = self.parent[id];
        }
        Ok(id)
    }

    fn union(
        &mut self,
        raw_left: SpectraHostValue,
        raw_right: SpectraHostValue,
    ) -> Result<bool, i32> {
        let mut left = self.find(raw_left)?;
        let mut right = self.find(raw_right)?;
        if left == right {
            return Ok(false);
        }
        if self.size[left] < self.size[right] {
            std::mem::swap(&mut left, &mut right);
        }
        let merged_size = self.size[left]
            .checked_add(self.size[right])
            .ok_or(HOST_STATUS_INTERNAL_ERROR)?;
        self.parent[right] = left;
        self.size[left] = merged_size;
        self.components -= 1;
        Ok(true)
    }
}

struct DisjointSetRegistry {
    values: HandleTable<ManualBox<StdDisjointSet>>,
}

impl DisjointSetRegistry {
    fn new() -> Self {
        Self {
            values: HandleTable::new(HandleKind::DisjointSet),
        }
    }

    fn insert(&mut self, value: ManualBox<StdDisjointSet>) -> usize {
        self.values.insert(value).raw() as usize
    }

    fn id(handle: usize) -> Result<HandleId, i32> {
        HandleId::from_raw(handle as i64).map_err(|_| HOST_STATUS_NOT_FOUND)
    }
}

fn with_disjoint_sets<F, R>(action: F) -> R
where
    F: FnOnce(&mut DisjointSetRegistry) -> R,
{
    static REGISTRY: OnceLock<Mutex<DisjointSetRegistry>> = OnceLock::new();
    let mut registry =
        lock_unpoisoned(REGISTRY.get_or_init(|| Mutex::new(DisjointSetRegistry::new())));
    action(&mut registry)
}

pub(crate) extern "C" fn std_disjoint_set_new(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 1, |args| {
        let value = allocate_handle(StdDisjointSet::with_size(args[0])?)?;
        Ok(with_disjoint_sets(|registry| registry.insert(value)) as SpectraHostValue)
    })
}

pub(crate) extern "C" fn std_disjoint_set_add(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 1, |args| {
        with_disjoint_sets(|registry| {
            let id = DisjointSetRegistry::id(args[0] as usize)?;
            let set = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            i64::try_from(set.add()?).map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

pub(crate) extern "C" fn std_disjoint_set_find(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 2, |args| {
        with_disjoint_sets(|registry| {
            let id = DisjointSetRegistry::id(args[0] as usize)?;
            let set = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            i64::try_from(set.find(args[1])?).map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

pub(crate) extern "C" fn std_disjoint_set_union(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 3, |args| {
        with_disjoint_sets(|registry| {
            let id = DisjointSetRegistry::id(args[0] as usize)?;
            let set = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            Ok(set.union(args[1], args[2])? as SpectraHostValue)
        })
    })
}

pub(crate) extern "C" fn std_disjoint_set_connected(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 3, |args| {
        with_disjoint_sets(|registry| {
            let id = DisjointSetRegistry::id(args[0] as usize)?;
            let set = registry
                .values
                .get_mut(id)
                .map_err(|_| HOST_STATUS_NOT_FOUND)?;
            let left = set.find(args[1])?;
            let right = set.find(args[2])?;
            Ok((left == right) as SpectraHostValue)
        })
    })
}

pub(crate) extern "C" fn std_disjoint_set_count(ctx: *mut SpectraHostCallContext) -> i32 {
    with_result_call(ctx, 1, |args| {
        with_disjoint_sets(|registry| {
            let id = DisjointSetRegistry::id(args[0] as usize)?;
            let set = registry.values.get(id).map_err(|_| HOST_STATUS_NOT_FOUND)?;
            i64::try_from(set.components).map_err(|_| HOST_STATUS_INTERNAL_ERROR)
        })
    })
}

pub(crate) extern "C" fn std_disjoint_set_free(ctx: *mut SpectraHostCallContext) -> i32 {
    with_void_call(ctx, 1, |args| {
        with_disjoint_sets(|registry| {
            let id = DisjointSetRegistry::id(args[0] as usize)?;
            registry
                .values
                .remove(id)
                .map(|_| ())
                .map_err(|_| HOST_STATUS_NOT_FOUND)
        })
    })
}
