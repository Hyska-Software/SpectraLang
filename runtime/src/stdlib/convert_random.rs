// ── std.convert extras ───────────────────────────────────────────────────────

/// Parses a string as int; returns `default` if parsing fails.
extern "C" fn std_convert_string_to_int_or(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let default_val = args[1];
        let result = match read_spectra_string(args[0]) {
            Some(s) => s.trim().parse::<i64>().unwrap_or(default_val),
            None => default_val,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = result;
        }
    }
    HOST_STATUS_SUCCESS
}

/// Parses a string as float; returns `default` (f64 bits) if parsing fails.
extern "C" fn std_convert_string_to_float_or(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let default_val = args[1];
        let result = match read_spectra_string(args[0]) {
            Some(s) => match s.trim().parse::<f64>() {
                Ok(f) => f.to_bits() as i64,
                Err(_) => default_val,
            },
            None => default_val,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = result;
        }
    }
    HOST_STATUS_SUCCESS
}

/// Returns 1 (true) if the string equals "true" (case-insensitive), 0 otherwise.
extern "C" fn std_convert_string_to_bool(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let result = match read_spectra_string(args[0]) {
            Some(s) => s.trim().eq_ignore_ascii_case("true") as SpectraHostValue,
            None => 0,
        };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = result;
        }
    }
    HOST_STATUS_SUCCESS
}

/// Converts a bool to int: true → 1, false → 0.
extern "C" fn std_convert_bool_to_int(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let result: SpectraHostValue = if args[0] != 0 { 1 } else { 0 };
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = result;
        }
    }
    HOST_STATUS_SUCCESS
}

// ── std.random ───────────────────────────────────────────────────────────────

fn random_state() -> &'static Mutex<u64> {
    static STATE: OnceLock<Mutex<u64>> = OnceLock::new();
    STATE.get_or_init(|| {
        // Default seed derived from the system time for variety across runs.
        use std::time::{SystemTime, UNIX_EPOCH};
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64 ^ d.as_secs().wrapping_mul(6364136223846793005))
            .unwrap_or(12345);
        Mutex::new(seed)
    })
}

/// xorshift64* step. Returns the new 64-bit state.
///
/// Replaces the earlier LCG; the historical helper name is kept because it is
/// shared by other stdlib modules. The public seeding API and post-seed
/// determinism are unchanged: a given seed always produces the same sequence.
#[inline]
fn lcg_next(state: &mut u64) -> u64 {
    // xorshift64* never escapes the zero state, so substitute the golden
    // ratio constant when seeded with 0 to keep the stream alive.
    let mut x = if *state == 0 {
        0x9E37_79B9_7F4A_7C15
    } else {
        *state
    };
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

/// Draws an unbiased value in `[0, range)` via Lemire's method: 128-bit
/// multiply with rejection, avoiding the modulo bias of `rand % range`.
/// Returns `0` when `range` is 0.
#[inline]
fn unbiased_below(state: &mut u64, range: u64) -> u64 {
    if range == 0 {
        return 0;
    }
    let mut m = (lcg_next(state) as u128) * (range as u128);
    let mut low = m as u64;
    if low < range {
        let threshold = range.wrapping_neg() % range;
        while low < threshold {
            m = (lcg_next(state) as u128) * (range as u128);
            low = m as u64;
        }
    }
    (m >> 64) as u64
}

#[inline]
fn random_unit_f64(state: &mut u64) -> f64 {
    (lcg_next(state) >> 11) as f64 / (1u64 << 53) as f64
}

fn register_random() {
    register_host_function(RAND_SEED, std_random_seed);
    register_host_function(RAND_INT, std_random_int);
    register_host_function(RAND_FLOAT, std_random_float);
    register_host_function(RAND_BOOL, std_random_bool);
}

/// Sets the random seed.
extern "C" fn std_random_seed(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        *lock_unpoisoned(random_state()) = args[0] as u64;
        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = 0;
        }
    }
    HOST_STATUS_SUCCESS
}

/// Returns a random integer in [min, max). Returns `min` when min >= max.
extern "C" fn std_random_int(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let min = args[0];
        let max = args[1];
        let result = if min >= max {
            min
        } else {
            // Compute in i128 so the span and the final offset stay in
            // range even when [min, max) covers nearly all of i64; the
            // span of two i64 values always fits in a u64.
            let range = ((max as i128) - (min as i128)) as u64;
            let value = unbiased_below(&mut lock_unpoisoned(random_state()), range) as i128;
            ((min as i128) + value) as i64
        };
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = result;
    }
    HOST_STATUS_SUCCESS
}

/// Returns a random float in [0.0, 1.0) as f64 bits.
extern "C" fn std_random_float(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let f = random_unit_f64(&mut lock_unpoisoned(random_state()));
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = f.to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Returns a random bool (0 or 1).
extern "C" fn std_random_bool(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let rand = lcg_next(&mut lock_unpoisoned(random_state()));
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = (rand & 1) as i64;
    }
    HOST_STATUS_SUCCESS
}

