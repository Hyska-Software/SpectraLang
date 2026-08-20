// ── std.time register & host functions ──────────────────────────────────────

fn register_time() {
    register_host_function(TIME_NOW_MILLIS, std_time_now_millis);
    register_host_function(TIME_NOW_SECS, std_time_now_secs);
    register_host_function(TIME_SLEEP_MS, std_time_sleep_ms);
    register_host_function(TIME_MONOTONIC_MILLIS, std_time_monotonic_millis);
    register_host_function(TIME_MONOTONIC_NANOS, std_time_monotonic_nanos);
    register_host_function(TIME_DURATION_MS, std_time_duration_ms);
    register_host_function(TIME_DURATION_SECS, std_time_duration_secs);
    register_host_function(TIME_DURATION_MILLIS, std_time_duration_millis);
    register_host_function(TIME_DURATION_SECS_VALUE, std_time_duration_secs_value);
    register_host_function(TIME_DURATION_ADD, std_time_duration_add);
    register_host_function(TIME_DURATION_SUB, std_time_duration_sub);
    register_host_function(TIME_INSTANT_NOW, std_time_instant_now);
    register_host_function(TIME_INSTANT_ELAPSED_MS, std_time_instant_elapsed_ms);
    register_host_function(TIME_INSTANT_ADD, std_time_instant_add);
    register_host_function(TIME_INSTANT_HAS_ELAPSED, std_time_instant_has_elapsed);
    register_host_function(TIME_SLEEP, std_time_sleep);
    register_host_function(TIME_UNIX_TO_UTC, std_time_unix_to_utc);
    register_host_function(TIME_UTC_YEAR, std_time_utc_year);
    register_host_function(TIME_UTC_MONTH, std_time_utc_month);
    register_host_function(TIME_UTC_DAY, std_time_utc_day);
    register_host_function(TIME_UTC_HOUR, std_time_utc_hour);
    register_host_function(TIME_UTC_MINUTE, std_time_utc_minute);
    register_host_function(TIME_UTC_SECOND, std_time_utc_second);
}

const STD_TIME_MAX_SLEEP_MS: u128 = 86_400_000;

#[derive(Clone, Copy)]
struct UtcDateTime {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
}

struct TimeHandles<T> {
    values: HandleTable<T>,
}

impl<T> TimeHandles<T> {
    fn new(kind: HandleKind) -> Self {
        Self {
            values: HandleTable::new(kind),
        }
    }

    fn insert(&mut self, value: T) -> SpectraHostValue {
        self.values.insert(value).raw() as SpectraHostValue
    }

    fn get(&self, handle: SpectraHostValue) -> Option<&T> {
        let id = HandleId::from_raw(handle).ok()?;
        self.values.get(id).ok()
    }
}

fn time_start() -> StdInstant {
    static START: OnceLock<StdInstant> = OnceLock::new();
    *START.get_or_init(StdInstant::now)
}

fn duration_handles() -> &'static Mutex<TimeHandles<Duration>> {
    static HANDLES: OnceLock<Mutex<TimeHandles<Duration>>> = OnceLock::new();
    HANDLES.get_or_init(|| Mutex::new(TimeHandles::new(HandleKind::Duration)))
}

fn instant_handles() -> &'static Mutex<TimeHandles<StdInstant>> {
    static HANDLES: OnceLock<Mutex<TimeHandles<StdInstant>>> = OnceLock::new();
    HANDLES.get_or_init(|| Mutex::new(TimeHandles::new(HandleKind::Instant)))
}

fn utc_handles() -> &'static Mutex<TimeHandles<UtcDateTime>> {
    static HANDLES: OnceLock<Mutex<TimeHandles<UtcDateTime>>> = OnceLock::new();
    HANDLES.get_or_init(|| Mutex::new(TimeHandles::new(HandleKind::UtcDateTime)))
}

fn store_duration(duration: Duration) -> SpectraHostValue {
    lock_unpoisoned(duration_handles()).insert(duration)
}

fn load_duration(handle: SpectraHostValue) -> Option<Duration> {
    lock_unpoisoned(duration_handles()).get(handle).copied()
}

fn store_instant(instant: StdInstant) -> SpectraHostValue {
    lock_unpoisoned(instant_handles()).insert(instant)
}

fn load_instant(handle: SpectraHostValue) -> Option<StdInstant> {
    lock_unpoisoned(instant_handles()).get(handle).copied()
}

fn store_utc(datetime: UtcDateTime) -> SpectraHostValue {
    lock_unpoisoned(utc_handles()).insert(datetime)
}

fn load_utc(handle: SpectraHostValue) -> Option<UtcDateTime> {
    lock_unpoisoned(utc_handles()).get(handle).copied()
}

fn host_args<'a>(
    ctx: *mut SpectraHostCallContext,
    expected_len: usize,
) -> Result<&'a [SpectraHostValue], i32> {
    if ctx.is_null() {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != expected_len || (expected_len > 0 && ctx_ref.args.is_null()) {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }
        Ok(slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len))
    }
}

fn write_host_result(ctx: *mut SpectraHostCallContext, value: SpectraHostValue) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = value;
    }
    HOST_STATUS_SUCCESS
}

fn duration_to_i64_millis(duration: Duration) -> Option<i64> {
    i64::try_from(duration.as_millis()).ok()
}

fn duration_from_millis_i64(ms: i64) -> Option<Duration> {
    (ms >= 0).then(|| Duration::from_millis(ms as u64))
}

fn utc_from_unix_seconds(secs: i64) -> UtcDateTime {
    let days = secs.div_euclid(86_400);
    let seconds_of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    UtcDateTime {
        year,
        month,
        day,
        hour: seconds_of_day / 3_600,
        minute: (seconds_of_day % 3_600) / 60,
        second: seconds_of_day % 60,
    }
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m, d)
}

/// Returns milliseconds elapsed since the Unix epoch (January 1, 1970 UTC).
/// Returns -1 if the system clock is before the epoch.
extern "C" fn std_time_now_millis(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(-1);
    }
    HOST_STATUS_SUCCESS
}

/// Returns seconds elapsed since the Unix epoch. Returns -1 on error.
extern "C" fn std_time_now_secs(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(-1);
    }
    HOST_STATUS_SUCCESS
}

/// Sleeps for `ms` milliseconds. Negative values are treated as zero.
extern "C" fn std_time_sleep_ms(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let ms = args[0].max(0) as u64;
        std::thread::sleep(Duration::from_millis(ms));
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = 0;
        }
    }
    HOST_STATUS_SUCCESS
}

extern "C" fn std_time_monotonic_millis(ctx: *mut SpectraHostCallContext) -> i32 {
    let elapsed = time_start().elapsed();
    let Some(ms) = duration_to_i64_millis(elapsed) else {
        return HOST_STATUS_INTERNAL_ERROR;
    };
    write_host_result(ctx, ms)
}

extern "C" fn std_time_monotonic_nanos(ctx: *mut SpectraHostCallContext) -> i32 {
    let elapsed = time_start().elapsed();
    let Ok(ns) = i64::try_from(elapsed.as_nanos()) else {
        return HOST_STATUS_INTERNAL_ERROR;
    };
    write_host_result(ctx, ns)
}

extern "C" fn std_time_duration_ms(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(duration) = duration_from_millis_i64(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_host_result(ctx, store_duration(duration))
}

extern "C" fn std_time_duration_secs(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if args[0] < 0 {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    write_host_result(ctx, store_duration(Duration::from_secs(args[0] as u64)))
}

extern "C" fn std_time_duration_millis(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(duration) = load_duration(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(ms) = duration_to_i64_millis(duration) else {
        return HOST_STATUS_INTERNAL_ERROR;
    };
    write_host_result(ctx, ms)
}

extern "C" fn std_time_duration_secs_value(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(duration) = load_duration(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Ok(secs) = i64::try_from(duration.as_secs()) else {
        return HOST_STATUS_INTERNAL_ERROR;
    };
    write_host_result(ctx, secs)
}

extern "C" fn std_time_duration_add(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(lhs), Some(rhs)) = (load_duration(args[0]), load_duration(args[1])) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(sum) = lhs.checked_add(rhs) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_host_result(ctx, store_duration(sum))
}

extern "C" fn std_time_duration_sub(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(lhs), Some(rhs)) = (load_duration(args[0]), load_duration(args[1])) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(diff) = lhs.checked_sub(rhs) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_host_result(ctx, store_duration(diff))
}

extern "C" fn std_time_instant_now(ctx: *mut SpectraHostCallContext) -> i32 {
    write_host_result(ctx, store_instant(StdInstant::now()))
}

extern "C" fn std_time_instant_elapsed_ms(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(instant) = load_instant(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(ms) = duration_to_i64_millis(instant.elapsed()) else {
        return HOST_STATUS_INTERNAL_ERROR;
    };
    write_host_result(ctx, ms)
}

extern "C" fn std_time_instant_add(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 2) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let (Some(instant), Some(duration)) = (load_instant(args[0]), load_duration(args[1])) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(deadline) = instant.checked_add(duration) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_host_result(ctx, store_instant(deadline))
}

extern "C" fn std_time_instant_has_elapsed(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(instant) = load_instant(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_host_result(ctx, (StdInstant::now() >= instant) as i64)
}

extern "C" fn std_time_sleep(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(duration) = load_duration(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    if duration.as_millis() > STD_TIME_MAX_SLEEP_MS {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    std::thread::sleep(duration);
    HOST_STATUS_SUCCESS
}

extern "C" fn std_time_unix_to_utc(ctx: *mut SpectraHostCallContext) -> i32 {
    let Ok(args) = host_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_host_result(ctx, store_utc(utc_from_unix_seconds(args[0])))
}

fn std_time_utc_field(ctx: *mut SpectraHostCallContext, field: fn(UtcDateTime) -> i64) -> i32 {
    let Ok(args) = host_args(ctx, 1) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    let Some(datetime) = load_utc(args[0]) else {
        return HOST_STATUS_INVALID_ARGUMENT;
    };
    write_host_result(ctx, field(datetime))
}

extern "C" fn std_time_utc_year(ctx: *mut SpectraHostCallContext) -> i32 {
    std_time_utc_field(ctx, |dt| dt.year)
}

extern "C" fn std_time_utc_month(ctx: *mut SpectraHostCallContext) -> i32 {
    std_time_utc_field(ctx, |dt| dt.month)
}

extern "C" fn std_time_utc_day(ctx: *mut SpectraHostCallContext) -> i32 {
    std_time_utc_field(ctx, |dt| dt.day)
}

extern "C" fn std_time_utc_hour(ctx: *mut SpectraHostCallContext) -> i32 {
    std_time_utc_field(ctx, |dt| dt.hour)
}

extern "C" fn std_time_utc_minute(ctx: *mut SpectraHostCallContext) -> i32 {
    std_time_utc_field(ctx, |dt| dt.minute)
}

extern "C" fn std_time_utc_second(ctx: *mut SpectraHostCallContext) -> i32 {
    std_time_utc_field(ctx, |dt| dt.second)
}

