// ── std.fs host functions ────────────────────────────────────────────────────

struct FsFailure {
    code: SpectraHostValue,
    message: String,
    operation: &'static str,
    context: String,
    retryable: bool,
}

fn fs_failure(
    code: SpectraHostValue,
    message: impl Into<String>,
    operation: &'static str,
    context: impl Into<String>,
    retryable: bool,
) -> FsFailure {
    FsFailure {
        code,
        message: message.into(),
        operation,
        context: context.into(),
        retryable,
    }
}

fn fs_error_code(error: &std::io::Error) -> SpectraHostValue {
    match error.kind() {
        std::io::ErrorKind::NotFound => 1,         // ErrorCode::NotFound
        std::io::ErrorKind::PermissionDenied => 2, // ErrorCode::PermissionDenied
        std::io::ErrorKind::InvalidInput
        | std::io::ErrorKind::InvalidData
        | std::io::ErrorKind::UnexpectedEof => 0, // ErrorCode::InvalidArgument
        _ => 3,                                   // ErrorCode::Io
    }
}

fn fs_error_retryable(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::ConnectionReset
    )
}

fn fs_io_failure(operation: &'static str, path: &Path, error: std::io::Error) -> FsFailure {
    fs_failure(
        fs_error_code(&error),
        error.to_string(),
        operation,
        path.to_string_lossy(),
        fs_error_retryable(&error),
    )
}

fn begin_fs_span(operation: &'static str, path: &Path) -> Option<u64> {
    let span = tracing::begin_external_span(SpanKind::Internal, operation).ok()?;
    let path_label = path.to_string_lossy();
    let _ = tracing::span_set_attribute(span, "fs.path", &path_label);
    Some(span)
}

fn end_fs_span(span: Option<u64>, status: SpanStatus) {
    if let Some(span) = span {
        let _ = tracing::span_set_status(span, status);
        let _ = tracing::span_end(span);
    }
}

fn write_fs_result(
    ctx: &mut SpectraHostCallContext,
    value: Result<SpectraHostValue, FsFailure>,
) -> i32 {
    if ctx.result_len == 0 || ctx.results.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }

    let tagged = unsafe {
        match value {
            Ok(payload) => alloc_tagged_payload(0, payload),
            Err(failure) => {
                let error = error::alloc_error(
                    failure.code,
                    &failure.message,
                    failure.operation,
                    &failure.context,
                    "std.fs",
                    failure.retryable,
                );
                if error == 0 {
                    return HOST_STATUS_INTERNAL_ERROR;
                }
                alloc_tagged_payload(1, error)
            }
        }
    };
    if tagged == 0 {
        return HOST_STATUS_INTERNAL_ERROR;
    }
    unsafe {
        let results = slice::from_raw_parts_mut(ctx.results, ctx.result_len);
        results[0] = tagged;
    }
    HOST_STATUS_SUCCESS
}

unsafe fn required_fs_path(arg: SpectraHostValue, operation: &'static str) -> Result<PathBuf, FsFailure> {
    match read_fs_path_arg(arg) {
        Ok(Some(path)) => Ok(path),
        Ok(None) => Err(fs_failure(
            0,
            "filesystem path must not be empty or contain NUL",
            operation,
            "path",
            false,
        )),
        Err(_) => Err(fs_failure(
            0,
            "filesystem path is not a valid Spectra string",
            operation,
            "path",
            false,
        )),
    }
}

extern "C" fn std_fs_read(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let path = match required_fs_path(args[0], "fs_read") {
            Ok(path) => path,
            Err(failure) => return write_fs_result(ctx_ref, Err(failure)),
        };
        let span = begin_fs_span("filesystem.read", &path);
        let read_result = std::fs::read_to_string(&path);
        end_fs_span(
            span,
            if read_result.is_ok() {
                SpanStatus::Ok
            } else {
                SpanStatus::Error
            },
        );
        let content = match read_result {
            Ok(content) => content,
            Err(error) => {
                return write_fs_result(ctx_ref, Err(fs_io_failure("fs_read", &path, error)))
            }
        };
        let value = alloc_spectra_string(&content);
        if value == 0 {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        write_fs_result(ctx_ref, Ok(value))
    }
}

extern "C" fn std_fs_write(ctx: *mut SpectraHostCallContext) -> i32 {
    std_fs_write_common(ctx, false)
}

extern "C" fn std_fs_append(ctx: *mut SpectraHostCallContext) -> i32 {
    std_fs_write_common(ctx, true)
}

fn std_fs_write_common(ctx: *mut SpectraHostCallContext, append: bool) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let operation = if append { "fs_append" } else { "fs_write" };
        let path = match required_fs_path(args[0], operation) {
            Ok(path) => path,
            Err(failure) => return write_fs_result(ctx_ref, Err(failure)),
        };
        let span_name = if append {
            "filesystem.append"
        } else {
            "filesystem.write"
        };
        let span = begin_fs_span(span_name, &path);
        let content = match read_spectra_string(args[1]) {
            Some(content) => content,
            None => {
                end_fs_span(span, SpanStatus::Error);
                return write_fs_result(
                    ctx_ref,
                    Err(fs_failure(
                        0,
                        "filesystem content is not a valid Spectra string",
                        operation,
                        "content",
                        false,
                    )),
                )
            }
        };
        let result = fs_write_text_result(&path, &content, append)
            .map(|_| 1)
            .map_err(|error| fs_io_failure(operation, &path, error));
        end_fs_span(
            span,
            if result.is_ok() {
                SpanStatus::Ok
            } else {
                SpanStatus::Error
            },
        );
        write_fs_result(ctx_ref, result)
    }
}

extern "C" fn std_fs_exists(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let path = match required_fs_path(args[0], "fs_exists") {
            Ok(path) => path,
            Err(failure) => return write_fs_result(ctx_ref, Err(failure)),
        };
        let span = begin_fs_span("filesystem.exists", &path);
        let result = match std::fs::metadata(&path) {
            Ok(_) => Ok(1),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(error) => Err(fs_io_failure("fs_exists", &path, error)),
        };
        end_fs_span(
            span,
            if result.is_ok() {
                SpanStatus::Ok
            } else {
                SpanStatus::Error
            },
        );
        write_fs_result(ctx_ref, result)
    }
}

extern "C" fn std_fs_remove(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let path = match required_fs_path(args[0], "fs_remove") {
            Ok(path) => path,
            Err(failure) => return write_fs_result(ctx_ref, Err(failure)),
        };
        let span = begin_fs_span("filesystem.remove", &path);
        let result = std::fs::remove_file(&path)
            .map(|_| 1)
            .map_err(|error| fs_io_failure("fs_remove", &path, error));
        end_fs_span(
            span,
            if result.is_ok() {
                SpanStatus::Ok
            } else {
                SpanStatus::Error
            },
        );
        write_fs_result(ctx_ref, result)
    }
}

// ── std.compat.fs host functions ────────────────────────────────────────────

extern "C" fn std_fs_compat_read(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let path = match read_fs_path_arg(args[0]) {
            Ok(Some(path)) => path,
            Ok(None) => {
                let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
                results[0] = alloc_spectra_string("");
                return HOST_STATUS_SUCCESS;
            }
            Err(status) => return status,
        };
        let path_label = path.to_string_lossy().to_string();
        let span = tracing::begin_external_span(SpanKind::Internal, "filesystem.read").ok();
        if let Some(id) = span {
            let _ = tracing::span_set_attribute(id, "fs.path", &path_label);
        }
        let read = std::fs::read_to_string(&path);
        if let Some(id) = span {
            let _ = tracing::span_set_status(
                id,
                if read.is_ok() {
                    SpanStatus::Ok
                } else {
                    SpanStatus::Error
                },
            );
            let _ = tracing::span_end(id);
        }
        let content = read.unwrap_or_default();
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = alloc_spectra_string(&content);
    }
    HOST_STATUS_SUCCESS
}

extern "C" fn std_fs_compat_write(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let path = match read_fs_path_arg(args[0]) {
            Ok(Some(path)) => path,
            Ok(None) => {
                let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
                results[0] = 0;
                return HOST_STATUS_SUCCESS;
            }
            Err(status) => return status,
        };
        let content = read_spectra_string(args[1]).unwrap_or_default();
        let path_label = path.to_string_lossy().to_string();
        let span = tracing::begin_external_span(SpanKind::Internal, "filesystem.write").ok();
        if let Some(id) = span {
            let _ = tracing::span_set_attribute(id, "fs.path", &path_label);
        }
        let ok = fs_write_text(&path, &content, false);
        if let Some(id) = span {
            let _ = tracing::span_set_status(
                id,
                if ok {
                    SpanStatus::Ok
                } else {
                    SpanStatus::Error
                },
            );
            let _ = tracing::span_end(id);
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = ok as SpectraHostValue;
    }
    HOST_STATUS_SUCCESS
}

extern "C" fn std_fs_compat_append(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let path = match read_fs_path_arg(args[0]) {
            Ok(Some(path)) => path,
            Ok(None) => {
                let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
                results[0] = 0;
                return HOST_STATUS_SUCCESS;
            }
            Err(status) => return status,
        };
        let content = read_spectra_string(args[1]).unwrap_or_default();
        let path_label = path.to_string_lossy().to_string();
        let span = tracing::begin_external_span(SpanKind::Internal, "filesystem.append").ok();
        if let Some(id) = span {
            let _ = tracing::span_set_attribute(id, "fs.path", &path_label);
        }
        let ok = fs_write_text(&path, &content, true);
        if let Some(id) = span {
            let _ = tracing::span_set_status(
                id,
                if ok {
                    SpanStatus::Ok
                } else {
                    SpanStatus::Error
                },
            );
            let _ = tracing::span_end(id);
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = ok as SpectraHostValue;
    }
    HOST_STATUS_SUCCESS
}

extern "C" fn std_fs_compat_exists(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let path = match read_fs_path_arg(args[0]) {
            Ok(Some(path)) => path,
            Ok(None) => {
                let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
                results[0] = 0;
                return HOST_STATUS_SUCCESS;
            }
            Err(status) => return status,
        };
        let path_label = path.to_string_lossy().to_string();
        let span = tracing::begin_external_span(SpanKind::Internal, "filesystem.exists").ok();
        if let Some(id) = span {
            let _ = tracing::span_set_attribute(id, "fs.path", &path_label);
        }
        let exists = path.exists();
        if let Some(id) = span {
            let _ = tracing::span_set_status(id, SpanStatus::Ok);
            let _ = tracing::span_end(id);
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = exists as SpectraHostValue;
    }
    HOST_STATUS_SUCCESS
}

extern "C" fn std_fs_compat_remove(ctx: *mut SpectraHostCallContext) -> i32 {
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
        let path = match read_fs_path_arg(args[0]) {
            Ok(Some(path)) => path,
            Ok(None) => {
                let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
                results[0] = 0;
                return HOST_STATUS_SUCCESS;
            }
            Err(status) => return status,
        };
        let path_label = path.to_string_lossy().to_string();
        let span = tracing::begin_external_span(SpanKind::Internal, "filesystem.remove").ok();
        if let Some(id) = span {
            let _ = tracing::span_set_attribute(id, "fs.path", &path_label);
        }
        let ok = std::fs::remove_file(&path).is_ok();
        if let Some(id) = span {
            let _ = tracing::span_set_status(
                id,
                if ok {
                    SpanStatus::Ok
                } else {
                    SpanStatus::Error
                },
            );
            let _ = tracing::span_end(id);
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = ok as SpectraHostValue;
    }
    HOST_STATUS_SUCCESS
}

