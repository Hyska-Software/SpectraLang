use super::*;
// ── std.fs host functions ────────────────────────────────────────────────────

pub(crate) struct FsFailure {
    pub(crate) code: SpectraHostValue,
    pub(crate) message: String,
    pub(crate) operation: &'static str,
    pub(crate) context: String,
    pub(crate) retryable: bool,
}

pub(crate) fn fs_failure(
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

pub(crate) fn fs_error_code(error: &std::io::Error) -> SpectraHostValue {
    match error.kind() {
        std::io::ErrorKind::NotFound => 1,         // ErrorCode::NotFound
        std::io::ErrorKind::PermissionDenied => 2, // ErrorCode::PermissionDenied
        std::io::ErrorKind::InvalidInput
        | std::io::ErrorKind::InvalidData
        | std::io::ErrorKind::UnexpectedEof => 0, // ErrorCode::InvalidArgument
        _ => 3,                                   // ErrorCode::Io
    }
}

pub(crate) fn fs_error_retryable(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::ConnectionReset
    )
}

pub(crate) fn fs_io_failure(operation: &'static str, path: &Path, error: std::io::Error) -> FsFailure {
    fs_failure(
        fs_error_code(&error),
        error.to_string(),
        operation,
        path.to_string_lossy(),
        fs_error_retryable(&error),
    )
}

pub(crate) fn begin_fs_span(operation: &'static str, path: &Path) -> Option<u64> {
    let span = tracing::begin_external_span(SpanKind::Internal, operation).ok()?;
    let path_label = path.to_string_lossy();
    let _ = tracing::span_set_attribute(span, "fs.path", &path_label);
    Some(span)
}

pub(crate) fn end_fs_span(span: Option<u64>, status: SpanStatus) {
    if let Some(span) = span {
        let _ = tracing::span_set_status(span, status);
        let _ = tracing::span_end(span);
    }
}

pub(crate) fn write_fs_result(
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

pub(crate) unsafe fn required_fs_path(arg: SpectraHostValue, operation: &'static str) -> Result<PathBuf, FsFailure> {
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

pub(crate) extern "C" fn std_fs_read(ctx: *mut SpectraHostCallContext) -> i32 {
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

pub(crate) extern "C" fn std_fs_write(ctx: *mut SpectraHostCallContext) -> i32 {
    std_fs_write_common(ctx, false)
}

pub(crate) extern "C" fn std_fs_append(ctx: *mut SpectraHostCallContext) -> i32 {
    std_fs_write_common(ctx, true)
}

pub(crate) fn std_fs_write_common(ctx: *mut SpectraHostCallContext, append: bool) -> i32 {
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

pub(crate) extern "C" fn std_fs_exists(ctx: *mut SpectraHostCallContext) -> i32 {
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

pub(crate) extern "C" fn std_fs_remove(ctx: *mut SpectraHostCallContext) -> i32 {
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

pub(crate) extern "C" fn std_fs_create_dir_all(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let path = match required_fs_path(args[0], "fs_create_dir_all") {
            Ok(path) => path,
            Err(failure) => return write_fs_result(ctx_ref, Err(failure)),
        };
        let span = begin_fs_span("filesystem.create_dir_all", &path);
        let create_result = std::fs::create_dir_all(&path);
        end_fs_span(
            span,
            if create_result.is_ok() {
                SpanStatus::Ok
            } else {
                SpanStatus::Error
            },
        );
        if let Err(error) = create_result {
            return write_fs_result(
                ctx_ref,
                Err(fs_io_failure("fs_create_dir_all", &path, error)),
            );
        }
        write_fs_result(ctx_ref, Ok(1))
    }
}

pub(crate) extern "C" fn std_fs_remove_dir(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let path = match required_fs_path(args[0], "fs_remove_dir") {
            Ok(path) => path,
            Err(failure) => return write_fs_result(ctx_ref, Err(failure)),
        };
        let span = begin_fs_span("filesystem.remove_dir", &path);
        let remove_result = std::fs::remove_dir(&path);
        end_fs_span(
            span,
            if remove_result.is_ok() {
                SpanStatus::Ok
            } else {
                SpanStatus::Error
            },
        );
        if let Err(error) = remove_result {
            return write_fs_result(ctx_ref, Err(fs_io_failure("fs_remove_dir", &path, error)));
        }
        write_fs_result(ctx_ref, Ok(1))
    }
}

pub(crate) extern "C" fn std_fs_rename(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let from = match required_fs_path(args[0], "fs_rename") {
            Ok(path) => path,
            Err(failure) => return write_fs_result(ctx_ref, Err(failure)),
        };
        let to = match required_fs_path(args[1], "fs_rename") {
            Ok(path) => path,
            Err(failure) => return write_fs_result(ctx_ref, Err(failure)),
        };
        let span = begin_fs_span("filesystem.rename", &from);
        let rename_result = std::fs::rename(&from, &to);
        end_fs_span(
            span,
            if rename_result.is_ok() {
                SpanStatus::Ok
            } else {
                SpanStatus::Error
            },
        );
        if let Err(error) = rename_result {
            return write_fs_result(ctx_ref, Err(fs_io_failure("fs_rename", &from, error)));
        }
        write_fs_result(ctx_ref, Ok(1))
    }
}

pub(crate) extern "C" fn std_fs_copy(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let from = match required_fs_path(args[0], "fs_copy") {
            Ok(path) => path,
            Err(failure) => return write_fs_result(ctx_ref, Err(failure)),
        };
        let to = match required_fs_path(args[1], "fs_copy") {
            Ok(path) => path,
            Err(failure) => return write_fs_result(ctx_ref, Err(failure)),
        };
        let span = begin_fs_span("filesystem.copy", &from);
        let copy_result = std::fs::copy(&from, &to);
        end_fs_span(
            span,
            if copy_result.is_ok() {
                SpanStatus::Ok
            } else {
                SpanStatus::Error
            },
        );
        let copied = match copy_result {
            Ok(bytes) => bytes,
            Err(error) => {
                return write_fs_result(ctx_ref, Err(fs_io_failure("fs_copy", &from, error)))
            }
        };
        let bytes = SpectraHostValue::try_from(copied).unwrap_or(i64::MAX);
        write_fs_result(ctx_ref, Ok(bytes))
    }
}

pub(crate) extern "C" fn std_fs_read_dir(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let path = match required_fs_path(args[0], "fs_read_dir") {
            Ok(path) => path,
            Err(failure) => return write_fs_result(ctx_ref, Err(failure)),
        };
        let span = begin_fs_span("filesystem.read_dir", &path);
        let read_result = std::fs::read_dir(&path).map(|entries| {
            let mut names: Vec<String> = entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        });
        end_fs_span(
            span,
            if read_result.is_ok() {
                SpanStatus::Ok
            } else {
                SpanStatus::Error
            },
        );
        let names = match read_result {
            Ok(names) => names,
            Err(error) => {
                return write_fs_result(ctx_ref, Err(fs_io_failure("fs_read_dir", &path, error)))
            }
        };
        let memory = crate::initialize().memory();
        let list = match memory.allocate_manual(StdList::default()) {
            Ok(list) => list,
            Err(_) => return HOST_STATUS_INTERNAL_ERROR,
        };
        let handle = with_list_registry(|reg| reg.insert(list));
        for name in &names {
            let ptr = alloc_spectra_string(name);
            if ptr == 0 {
                return HOST_STATUS_INTERNAL_ERROR;
            }
            let _ = with_list_registry(|reg| reg.push(handle, ptr));
        }
        write_fs_result(ctx_ref, Ok(handle as SpectraHostValue))
    }
}

