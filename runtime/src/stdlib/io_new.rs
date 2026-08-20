// ── std.io new functions ─────────────────────────────────────────────────────

/// Prints `prompt` (without newline), flushes stdout, then reads a line from
/// stdin. Strips the trailing newline before returning.
extern "C" fn std_io_input(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        if let Some(prompt) = read_spectra_string(args[0]) {
            let mut stdout = io::stdout();
            let _ = write!(stdout, "{}", prompt);
            let _ = stdout.flush();
        }
        let mut line = String::new();
        if io::stdin().lock().read_line(&mut line).is_err() {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        let ptr = alloc_spectra_string(&line);
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = ptr;
    }
    HOST_STATUS_SUCCESS
}

