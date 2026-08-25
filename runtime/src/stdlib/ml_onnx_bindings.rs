use super::*;
pub(crate) extern "C" fn std_ml_onnx_export(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(kind) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(model) = ml_onnx_model_spec(&kind) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if let Some(parent) = std::path::Path::new(&path).parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return HOST_STATUS_INTERNAL_ERROR;
            }
        }
        let payload = ml_onnx_model_proto(&model);
        if std::fs::write(&path, payload).is_err() {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        tensor_result(ctx_ref, alloc_spectra_string(&path))
    }
}

pub(crate) extern "C" fn std_ml_onnx_import_summary(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let bytes = match std::fs::read(&path) {
            Ok(value) => value,
            Err(_) => return HOST_STATUS_NOT_FOUND,
        };
        // With the `onnx` feature the summary comes from a real
        // onnxruntime session; otherwise (or if ORT cannot load the model)
        // fall back to the protobuf walker over the actual bytes.
        #[cfg(feature = "onnx")]
        if let Some(summary) = ml_onnx_real_summary_from_bytes(&bytes) {
            return tensor_result(ctx_ref, alloc_spectra_string(&summary));
        }
        let Some(summary) = ml_onnx_import_summary_from_bytes(&bytes) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        tensor_result(ctx_ref, alloc_spectra_string(&summary))
    }
}

pub(crate) extern "C" fn std_ml_onnx_validate(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let bytes = match std::fs::read(&path) {
            Ok(value) => value,
            Err(_) => return HOST_STATUS_NOT_FOUND,
        };
        let valid = ml_onnx_import_summary_from_bytes(&bytes)
            .as_deref()
            .map(ml_onnx_validate_summary)
            .unwrap_or(false);
        tensor_result(ctx_ref, if valid { 1 } else { 0 })
    }
}

pub(crate) extern "C" fn std_ml_onnx_roundtrip(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(input_path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(output_path) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let bytes = match std::fs::read(&input_path) {
            Ok(value) => value,
            Err(_) => return HOST_STATUS_NOT_FOUND,
        };
        let Some(summary) = ml_onnx_import_summary_from_bytes(&bytes) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if !ml_onnx_validate_summary(&summary) {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if let Some(parent) = std::path::Path::new(&output_path).parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return HOST_STATUS_INTERNAL_ERROR;
            }
        }
        if std::fs::write(&output_path, bytes).is_err() {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        tensor_result(ctx_ref, alloc_spectra_string(&output_path))
    }
}

/// Typed failure for the inference hosts when the runtime is compiled
/// without the `onnx` feature: callers receive a tagged `Error` record
/// (tag 1) instead of a bare status code.
#[cfg(not(feature = "onnx"))]
pub(crate) unsafe fn ml_onnx_unavailable_result(
    ctx_ref: &mut SpectraHostCallContext,
    operation: &str,
) -> i32 {
    if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
        return HOST_STATUS_INTERNAL_ERROR;
    }
    let message = format!(
        "'{operation}' requires real onnxruntime inference; rebuild spectra-runtime with --features onnx"
    );
    let error = error::alloc_error(3, &message, operation, "onnx", "std.ml.onnx", false);
    if error == 0 {
        return HOST_STATUS_INTERNAL_ERROR;
    }
    let tagged = alloc_tagged_payload(1, error);
    if tagged == 0 {
        return HOST_STATUS_INTERNAL_ERROR;
    }
    *ctx_ref.results = tagged;
    HOST_STATUS_SUCCESS
}

/// `spectra.std.ml.onnx_session_from_bytes(path) -> handle`
///
/// Loads the ONNX ModelProto bytes from `path`, commits an onnxruntime
/// session from memory and returns a process-local session handle.
pub(crate) extern "C" fn std_ml_onnx_session_from_bytes(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let bytes = match std::fs::read(&path) {
            Ok(value) => value,
            Err(_) => return HOST_STATUS_NOT_FOUND,
        };
        #[cfg(feature = "onnx")]
        {
            return match ml_onnx_commit_session(&bytes) {
                Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
                Err(_) => HOST_STATUS_INVALID_ARGUMENT,
            };
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = bytes;
            return ml_onnx_unavailable_result(ctx_ref, ML_ONNX_SESSION_FROM_BYTES);
        }
    }
}

/// `spectra.std.ml.onnx_run(session_handle, input_tensor_handle, output_name) -> tensor_handle`
///
/// Real inference through onnxruntime: the stdlib tensor (f64 storage) is
/// narrowed to f32, executed against the committed session, and the named
/// output comes back as a fresh stdlib float tensor handle.
pub(crate) extern "C" fn std_ml_onnx_run(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[0] <= 0 || args[1] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let session_handle = args[0] as u64;
        let tensor_handle = args[1] as usize;
        let Some(output_name) = read_spectra_string(args[2]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        #[cfg(feature = "onnx")]
        {
            return match ml_onnx_run_inner(session_handle, tensor_handle, &output_name) {
                Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
                Err(status) => status,
            };
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = (session_handle, tensor_handle, output_name);
            return ml_onnx_unavailable_result(ctx_ref, ML_ONNX_RUN);
        }
    }
}

/// `spectra.std.ml.onnx_session_free(handle) -> int`
///
/// Releases a committed onnxruntime session. Returns HOST_STATUS_NOT_FOUND
/// for unknown handles.
pub(crate) extern "C" fn std_ml_onnx_session_free(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[0] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let session_handle = args[0] as u64;
        #[cfg(feature = "onnx")]
        {
            if ml_onnx_sessions_lock().remove(&session_handle).is_some() {
                return tensor_result(ctx_ref, 1);
            }
            return HOST_STATUS_NOT_FOUND;
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = session_handle;
            return ml_onnx_unavailable_result(ctx_ref, ML_ONNX_SESSION_FREE);
        }
    }
}


// ── OnnxMultiInput: multi-graph-input inference ──

/// `spectra.std.ml.onnx_run_multi(session_handle, names_list_handle, tensors_list_handle) -> tensor_handle`
///
/// Real multi-graph-input inference through onnxruntime. `names_list_handle`
/// is a List<string> of graph input names and `tensors_list_handle` a
/// parallel List<int> of stdlib tensor handles; both are validated against
/// the real ORT session metadata (names, coverage, f32 shapes) by
/// `ml_onnx_run_multi_inner`. Semantic failures (unknown/missing input name,
/// shape mismatch, non-f32 model input) come back as a tagged typed `Error`
/// record instead of a bare status.
pub(crate) extern "C" fn std_ml_onnx_run_multi(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[0] <= 0 || args[1] <= 0 || args[2] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let session_handle = args[0] as u64;
        let names_handle = args[1] as usize;
        let tensors_handle = args[2] as usize;

        let name_values =
            match with_list_registry(|registry| registry.snapshot(names_handle)) {
                Ok(values) => values,
                Err(code) => return code,
            };
        let tensor_values = match with_list_registry(|registry| registry.snapshot(tensors_handle))
        {
            Ok(values) => values,
            Err(code) => return code,
        };
        let mut names = Vec::with_capacity(name_values.len());
        for value in &name_values {
            let Some(name) = read_spectra_string(*value) else {
                return HOST_STATUS_INVALID_ARGUMENT;
            };
            names.push(name);
        }
        let mut tensor_handles = Vec::with_capacity(tensor_values.len());
        for value in &tensor_values {
            if *value <= 0 {
                return HOST_STATUS_INVALID_ARGUMENT;
            }
            tensor_handles.push(*value as usize);
        }

        #[cfg(feature = "onnx")]
        {
            return match ml_onnx_run_multi_inner(session_handle, &names, &tensor_handles) {
                Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
                Err(failure) => {
                    if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
                        return failure.status;
                    }
                    let error = error::alloc_error(
                        failure.code,
                        &failure.message,
                        ML_ONNX_RUN_MULTI,
                        "onnx",
                        "std.ml.onnx",
                        false,
                    );
                    if error == 0 {
                        return HOST_STATUS_INTERNAL_ERROR;
                    }
                    let tagged = alloc_tagged_payload(1, error);
                    if tagged == 0 {
                        return HOST_STATUS_INTERNAL_ERROR;
                    }
                    *ctx_ref.results = tagged;
                    HOST_STATUS_SUCCESS
                }
            };
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = (session_handle, names, tensor_handles);
            return ml_onnx_unavailable_result(ctx_ref, ML_ONNX_RUN_MULTI);
        }
    }
}
