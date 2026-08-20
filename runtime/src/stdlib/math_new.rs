// ── std.math new functions ───────────────────────────────────────────────────

/// Returns the sign of `n`: -1 for negative, 0 for zero, 1 for positive.
extern "C" fn std_math_sign(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = args[0].signum();
    }
    HOST_STATUS_SUCCESS
}

/// Greatest common divisor of `a` and `b` (always non-negative; gcd(0,0) = 0).
extern "C" fn std_math_gcd(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        let mut a = args[0].unsigned_abs();
        let mut b = args[1].unsigned_abs();
        while b != 0 {
            let t = b;
            b = a % b;
            a = t;
        }
        results[0] = a as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Least common multiple of `a` and `b` (always non-negative; lcm(n,0) = 0).
extern "C" fn std_math_lcm(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        let a = args[0].unsigned_abs();
        let b = args[1].unsigned_abs();
        if a == 0 || b == 0 {
            results[0] = 0;
        } else {
            let mut ga = a;
            let mut gb = b;
            while gb != 0 {
                let t = gb;
                gb = ga % gb;
                ga = t;
            }
            // ga is now gcd(a, b)
            results[0] = ((a / ga) * b) as i64;
        }
    }
    HOST_STATUS_SUCCESS
}

/// Returns 1 if the float value is NaN, 0 otherwise. Argument is f64 bits as i64.
extern "C" fn std_math_is_nan_f(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = f64::from_bits(args[0] as u64).is_nan() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Returns 1 if the float value is +∞ or −∞, 0 otherwise. Argument is f64 bits.
extern "C" fn std_math_is_infinite_f(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = f64::from_bits(args[0] as u64).is_infinite() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Returns |x| for a float. Argument and result are f64 bits as i64.
extern "C" fn std_math_abs_f(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = f64::from_bits(args[0] as u64).abs().to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

