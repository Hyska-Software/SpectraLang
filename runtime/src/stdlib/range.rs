use super::*;
// ── std.range register & host functions ─────────────────────────────────────

pub(crate) fn register_range() {
    register_host_function(RANGE_CREATE, std_range_create);
    register_host_function(RANGE_LEN, std_range_len);
    register_host_function(RANGE_AT, std_range_at);
    register_host_function(RANGE_EQ, std_range_eq);
    register_host_function(RANGE_START, std_range_start);
    register_host_function(RANGE_END, std_range_end);
    register_host_function(RANGE_IS_INCLUSIVE, std_range_is_inclusive);
    register_host_function(RANGE_ITER, std_range_iter);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IntRange {
    pub(crate) start: i64,
    pub(crate) end: i64,
    pub(crate) inclusive: bool,
}

pub(crate) fn range_handles() -> &'static Mutex<TimeHandles<IntRange>> {
    static HANDLES: OnceLock<Mutex<TimeHandles<IntRange>>> = OnceLock::new();
    HANDLES.get_or_init(|| Mutex::new(TimeHandles::new(HandleKind::Range)))
}

pub(crate) fn store_range(range: IntRange) -> SpectraHostValue {
    lock_unpoisoned(range_handles()).insert(range)
}

pub(crate) fn load_range(handle: SpectraHostValue) -> Option<IntRange> {
    lock_unpoisoned(range_handles()).get(handle).copied()
}

pub(crate) fn range_len_value(range: IntRange) -> Option<i64> {
    if range.start > range.end {
        return Some(0);
    }
    let raw = i128::from(range.end) - i128::from(range.start);
    let len = raw + if range.inclusive { 1 } else { 0 };
    if len < 0 {
        return Some(0);
    }
    i64::try_from(len).ok()
}

pub(crate) fn range_at_value(range: IntRange, index: i64) -> Option<i64> {
    if index < 0 {
        return None;
    }
    let len = range_len_value(range)?;
    if index >= len {
        return None;
    }
    let value = i128::from(range.start) + i128::from(index);
    i64::try_from(value).ok()
}

pub(crate) extern "C" fn std_range_create(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 3) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let inclusive = match args[2] {
        0 => false,
        1 => true,
        _ => return HOST_STATUS_INVALID_ARGUMENT,
    };
    write_host_result(
        ctx,
        store_range(IntRange {
            start: args[0],
            end: args[1],
            inclusive,
        }),
    )
}

pub(crate) extern "C" fn std_range_len(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(range) = load_range(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(len) = range_len_value(range) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_host_result(ctx, len)
}

pub(crate) extern "C" fn std_range_at(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(range) = load_range(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(value) = range_at_value(range, args[1]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_host_result(ctx, value)
}

pub(crate) extern "C" fn std_range_eq(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(lhs), Some(rhs)) = (load_range(args[0]), load_range(args[1])) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_host_result(ctx, (lhs == rhs) as i64)
}

pub(crate) extern "C" fn std_range_start(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(range) = load_range(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_host_result(ctx, range.start)
}

pub(crate) extern "C" fn std_range_end(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(range) = load_range(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_host_result(ctx, range.end)
}

pub(crate) extern "C" fn std_range_is_inclusive(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(range) = load_range(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_host_result(ctx, range.inclusive as i64)
}

pub(crate) extern "C" fn std_range_iter(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(range) = load_range(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let length = match range_len_value(range).and_then(|value| usize::try_from(value).ok()) {
        Some(length) => length,
        None => return HOST_STATUS_INVALID_ARGUMENT,
    };
    let handle = match insert_range_iterator(range, length) {
        Ok(handle) => handle,
        Err(code) => return code,
    };
    write_host_result(ctx, handle as i64)
}
