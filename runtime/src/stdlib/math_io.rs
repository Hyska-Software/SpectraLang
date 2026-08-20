extern "C" fn std_math_abs(ctx: *mut SpectraHostCallContext) -> i32 {
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

        let args_ptr = ctx_ref.args;
        let args_len = ctx_ref.arg_len;
        let results_ptr = ctx_ref.results;
        let results_len = ctx_ref.result_len;

        let args = slice::from_raw_parts(args_ptr, args_len);
        let results = slice::from_raw_parts_mut(results_ptr, results_len);
        results[0] = args[0].abs();
    }

    HOST_STATUS_SUCCESS
}

extern "C" fn std_math_min(ctx: *mut SpectraHostCallContext) -> i32 {
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

        let args_ptr = ctx_ref.args;
        let args_len = ctx_ref.arg_len;
        let results_ptr = ctx_ref.results;
        let results_len = ctx_ref.result_len;

        let args = slice::from_raw_parts(args_ptr, args_len);
        let results = slice::from_raw_parts_mut(results_ptr, results_len);
        results[0] = args[0].min(args[1]);
    }

    HOST_STATUS_SUCCESS
}

extern "C" fn std_math_max(ctx: *mut SpectraHostCallContext) -> i32 {
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

        let args_ptr = ctx_ref.args;
        let args_len = ctx_ref.arg_len;
        let results_ptr = ctx_ref.results;
        let results_len = ctx_ref.result_len;

        let args = slice::from_raw_parts(args_ptr, args_len);
        let results = slice::from_raw_parts_mut(results_ptr, results_len);
        results[0] = args[0].max(args[1]);
    }

    HOST_STATUS_SUCCESS
}

/// Clamp an integer value between min and max (inclusive).
extern "C" fn std_math_clamp(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 3 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = args[0].clamp(args[1], args[2]);
    }
    HOST_STATUS_SUCCESS
}

/// Square root. Value and result are f64 bits reinterpreted as i64.
extern "C" fn std_math_sqrt_f(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let f = f64::from_bits(args[0] as u64).sqrt();
        results[0] = f.to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Power. Both arguments and result are f64 bits reinterpreted as i64.
extern "C" fn std_math_pow_f(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let base = f64::from_bits(args[0] as u64);
        let exp = f64::from_bits(args[1] as u64);
        results[0] = base.powf(exp).to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Floor. Value and result are f64 bits reinterpreted as i64.
extern "C" fn std_math_floor_f(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let f = f64::from_bits(args[0] as u64).floor();
        results[0] = f.to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Ceil. Value and result are f64 bits reinterpreted as i64.
extern "C" fn std_math_ceil_f(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let f = f64::from_bits(args[0] as u64).ceil();
        results[0] = f.to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Round. Value and result are f64 bits reinterpreted as i64.
extern "C" fn std_math_round_f(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let f = f64::from_bits(args[0] as u64).round();
        results[0] = f.to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Sine. Argument and result are f64 bits reinterpreted as i64.
extern "C" fn std_math_sin_f(ctx: *mut SpectraHostCallContext) -> i32 {
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
        results[0] = f64::from_bits(args[0] as u64).sin().to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Cosine. Argument and result are f64 bits reinterpreted as i64.
extern "C" fn std_math_cos_f(ctx: *mut SpectraHostCallContext) -> i32 {
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
        results[0] = f64::from_bits(args[0] as u64).cos().to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Tangent. Argument and result are f64 bits reinterpreted as i64.
extern "C" fn std_math_tan_f(ctx: *mut SpectraHostCallContext) -> i32 {
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
        results[0] = f64::from_bits(args[0] as u64).tan().to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Natural logarithm. Argument and result are f64 bits reinterpreted as i64.
extern "C" fn std_math_log_f(ctx: *mut SpectraHostCallContext) -> i32 {
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
        results[0] = f64::from_bits(args[0] as u64).ln().to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Base-2 logarithm. Argument and result are f64 bits reinterpreted as i64.
extern "C" fn std_math_log2_f(ctx: *mut SpectraHostCallContext) -> i32 {
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
        results[0] = f64::from_bits(args[0] as u64).log2().to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Base-10 logarithm. Argument and result are f64 bits reinterpreted as i64.
extern "C" fn std_math_log10_f(ctx: *mut SpectraHostCallContext) -> i32 {
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
        results[0] = f64::from_bits(args[0] as u64).log10().to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Two-argument arctangent (atan2). Arguments y, x and result are f64 bits.
extern "C" fn std_math_atan2_f(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let y = f64::from_bits(args[0] as u64);
        let x = f64::from_bits(args[1] as u64);
        results[0] = y.atan2(x).to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Returns the mathematical constant PI as f64 bits.
extern "C" fn std_math_pi(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = std::f64::consts::PI.to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Returns the mathematical constant E as f64 bits.
extern "C" fn std_math_e_const(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = std::f64::consts::E.to_bits() as i64;
    }
    HOST_STATUS_SUCCESS
}

/// Polymorphic print function (no trailing newline — use `println` for newline).
///
/// Arguments are (type_tag: i64, value: i64) pairs:
///   - tag 0 → print as integer
///   - tag 1 → print as null-terminated string (value is a pointer)
///   - tag 2 → print as bool ("true"/"false")
///   - tag 3 → print as float (value reinterpreted as f64 bits)
extern "C" fn std_io_print(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }

    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len > 0 && ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }

        let args_len = ctx_ref.arg_len;
        let args = if args_len == 0 {
            &[] as &[SpectraHostValue]
        } else {
            slice::from_raw_parts(ctx_ref.args, args_len)
        };

        let mut stdout = io::stdout();
        let values_count = args_len / 2;

        for i in 0..values_count {
            if i > 0
                && write!(stdout, " ").is_err() {
                    return HOST_STATUS_INTERNAL_ERROR;
                }
            let tag = args[i * 2];
            let value = args[i * 2 + 1];
            let ok = match tag {
                PRINT_TAG_STR => {
                    // String buffer stores each byte as a separate i64 slot
                    let ptr = value as *const i64;
                    if ptr.is_null() {
                        write!(stdout, "(null)").is_ok()
                    } else {
                        let mut bytes: Vec<u8> = Vec::new();
                        let mut offset = 0usize;
                        loop {
                            let b = *ptr.add(offset) as u8;
                            if b == 0 {
                                break;
                            }
                            bytes.push(b);
                            offset += 1;
                        }
                        match String::from_utf8(bytes) {
                            Ok(s) => write!(stdout, "{}", s).is_ok(),
                            Err(_) => write!(stdout, "(invalid utf8)").is_ok(),
                        }
                    }
                }
                PRINT_TAG_BOOL => {
                    write!(stdout, "{}", if value != 0 { "true" } else { "false" }).is_ok()
                }
                PRINT_TAG_FLOAT => {
                    let f = f64::from_bits(value as u64);
                    write!(stdout, "{}", f).is_ok()
                }
                _ => write!(stdout, "{}", value).is_ok(), // PRINT_TAG_INT or unknown
            };
            if !ok {
                return HOST_STATUS_INTERNAL_ERROR;
            }
        }

        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = values_count as SpectraHostValue;
        }
    }

    HOST_STATUS_SUCCESS
}

/// Polymorphic println: same as print but appends a trailing newline.
extern "C" fn std_io_println(ctx: *mut SpectraHostCallContext) -> i32 {
    let status = std_io_print(ctx);
    if status != HOST_STATUS_SUCCESS {
        return status;
    }
    if writeln!(io::stdout()).is_err() {
        return HOST_STATUS_INTERNAL_ERROR;
    }
    HOST_STATUS_SUCCESS
}

/// Same as io.print but writes to stderr (no trailing newline).
extern "C" fn std_io_eprint(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }

    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len > 0 && ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }

        let args_len = ctx_ref.arg_len;
        let args = if args_len == 0 {
            &[] as &[SpectraHostValue]
        } else {
            slice::from_raw_parts(ctx_ref.args, args_len)
        };

        let mut stderr = io::stderr();
        let values_count = args_len / 2;

        for i in 0..values_count {
            if i > 0
                && write!(stderr, " ").is_err() {
                    return HOST_STATUS_INTERNAL_ERROR;
                }
            let tag = args[i * 2];
            let value = args[i * 2 + 1];
            let ok = match tag {
                PRINT_TAG_STR => {
                    // String buffer stores each byte as a separate i64 slot
                    let ptr = value as *const i64;
                    if ptr.is_null() {
                        write!(stderr, "(null)").is_ok()
                    } else {
                        let mut bytes: Vec<u8> = Vec::new();
                        let mut offset = 0usize;
                        loop {
                            let b = *ptr.add(offset) as u8;
                            if b == 0 {
                                break;
                            }
                            bytes.push(b);
                            offset += 1;
                        }
                        match String::from_utf8(bytes) {
                            Ok(s) => write!(stderr, "{}", s).is_ok(),
                            Err(_) => write!(stderr, "(invalid utf8)").is_ok(),
                        }
                    }
                }
                PRINT_TAG_BOOL => {
                    write!(stderr, "{}", if value != 0 { "true" } else { "false" }).is_ok()
                }
                PRINT_TAG_FLOAT => {
                    let f = f64::from_bits(value as u64);
                    write!(stderr, "{}", f).is_ok()
                }
                _ => write!(stderr, "{}", value).is_ok(),
            };
            if !ok {
                return HOST_STATUS_INTERNAL_ERROR;
            }
        }

        if ctx_ref.result_len > 0 && !ctx_ref.results.is_null() {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            results[0] = values_count as SpectraHostValue;
        }
    }

    HOST_STATUS_SUCCESS
}

/// Polymorphic eprintln: same as eprint but appends a trailing newline.
extern "C" fn std_io_eprintln(ctx: *mut SpectraHostCallContext) -> i32 {
    let status = std_io_eprint(ctx);
    if status != HOST_STATUS_SUCCESS {
        return status;
    }
    if writeln!(io::stderr()).is_err() {
        return HOST_STATUS_INTERNAL_ERROR;
    }
    HOST_STATUS_SUCCESS
}

extern "C" fn std_io_read_line(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_err() {
        return HOST_STATUS_INTERNAL_ERROR;
    }
    // Strip trailing CRLF or LF
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let ptr = alloc_spectra_string(&line);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = ptr;
    }
    HOST_STATUS_SUCCESS
}

extern "C" fn std_io_flush(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }

    if io::stdout().flush().is_err() {
        return HOST_STATUS_INTERNAL_ERROR;
    }

    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.result_len > 0 && ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }

        if ctx_ref.result_len > 0 {
            let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
            if !results.is_empty() {
                results[0] = 0;
            }
        }
    }

    HOST_STATUS_SUCCESS
}

