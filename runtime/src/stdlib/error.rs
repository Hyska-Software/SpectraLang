//! Structured standard-library errors.
//!
//! `Error` is an opaque, runtime-owned record whose fields are laid out as
//! six machine words.  Keeping the allocation layout identical to the IR
//! record layout lets filesystem/API host calls return `Result<T, Error>`
//! without converting failures to a sentinel payload.

use super::*;

pub(crate) const ERROR_NEW: &str = spectra_contract::STD_ERROR_NEW_BINDING;
pub(crate) const ERROR_CODE: &str = spectra_contract::STD_ERROR_CODE_BINDING;
pub(crate) const ERROR_MESSAGE: &str = spectra_contract::STD_ERROR_MESSAGE_BINDING;
pub(crate) const ERROR_OPERATION: &str = spectra_contract::STD_ERROR_OPERATION_BINDING;
pub(crate) const ERROR_CONTEXT: &str = spectra_contract::STD_ERROR_CONTEXT_BINDING;
pub(crate) const ERROR_ORIGIN: &str = spectra_contract::STD_ERROR_ORIGIN_BINDING;
pub(crate) const ERROR_RETRYABLE: &str = spectra_contract::STD_ERROR_RETRYABLE_BINDING;

pub(crate) const ERROR_FIELD_CODE: usize = 0;
pub(crate) const ERROR_FIELD_MESSAGE: usize = 1;
pub(crate) const ERROR_FIELD_OPERATION: usize = 2;
pub(crate) const ERROR_FIELD_CONTEXT: usize = 3;
pub(crate) const ERROR_FIELD_ORIGIN: usize = 4;
pub(crate) const ERROR_FIELD_RETRYABLE: usize = 5;
pub(crate) const ERROR_FIELD_COUNT: usize = 6;

pub(crate) static ERROR_HANDLES: OnceLock<Mutex<HashSet<SpectraHostValue>>> = OnceLock::new();

pub(crate) fn error_handles() -> &'static Mutex<HashSet<SpectraHostValue>> {
    ERROR_HANDLES.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(super) fn register() {
    register_host_function(ERROR_NEW, std_error_new);
    register_host_function(ERROR_CODE, std_error_code);
    register_host_function(ERROR_MESSAGE, std_error_message);
    register_host_function(ERROR_OPERATION, std_error_operation);
    register_host_function(ERROR_CONTEXT, std_error_context);
    register_host_function(ERROR_ORIGIN, std_error_origin);
    register_host_function(ERROR_RETRYABLE, std_error_retryable);
}

/// Allocate an `Error` record using the same word layout as the midend.
///
/// The returned pointer is intentionally owned by the runtime manual arena,
/// just like strings and tagged `Option`/`Result` values.  The current arena
/// has process lifetime, so a `Result` can safely carry this pointer across a
/// host-call boundary and through a later `match`/unwrap operation.
pub(super) unsafe fn alloc_error(
    code: SpectraHostValue,
    message: &str,
    operation: &str,
    context: &str,
    origin: &str,
    retryable: bool,
) -> SpectraHostValue {
    use crate::ffi::spectra_rt_manual_alloc;

    let raw = spectra_rt_manual_alloc(ERROR_FIELD_COUNT * std::mem::size_of::<i64>()) as *mut i64;
    if raw.is_null() {
        return 0;
    }

    *raw.add(ERROR_FIELD_CODE) = code;
    *raw.add(ERROR_FIELD_MESSAGE) = alloc_spectra_string(message);
    *raw.add(ERROR_FIELD_OPERATION) = alloc_spectra_string(operation);
    *raw.add(ERROR_FIELD_CONTEXT) = alloc_spectra_string(context);
    *raw.add(ERROR_FIELD_ORIGIN) = alloc_spectra_string(origin);
    *raw.add(ERROR_FIELD_RETRYABLE) = retryable as i64;
    let value = raw as SpectraHostValue;
    error_handles()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(value);
    value
}

pub(crate) unsafe fn read_error_field(error: SpectraHostValue, field: usize) -> Option<SpectraHostValue> {
    if error == 0 || field >= ERROR_FIELD_COUNT {
        return None;
    }
    if !error_handles()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .contains(&error)
    {
        return None;
    }
    Some(*((error as *const i64).add(field)))
}

pub(crate) extern "C" fn std_error_new(ctx: *mut SpectraHostCallContext) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }

    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 6
            || ctx_ref.args.is_null()
            || ctx_ref.result_len == 0
            || ctx_ref.results.is_null()
        {
            return HOST_STATUS_INVALID_ARGUMENT;
        }

        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let code = args[0];
        if !(0..=5).contains(&code) {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let message = match read_spectra_string(args[1]) {
            Some(value) => value,
            None => return HOST_STATUS_INVALID_ARGUMENT,
        };
        let operation = match read_spectra_string(args[2]) {
            Some(value) => value,
            None => return HOST_STATUS_INVALID_ARGUMENT,
        };
        let context = match read_spectra_string(args[3]) {
            Some(value) => value,
            None => return HOST_STATUS_INVALID_ARGUMENT,
        };
        let origin = match read_spectra_string(args[4]) {
            Some(value) => value,
            None => return HOST_STATUS_INVALID_ARGUMENT,
        };

        let error = alloc_error(code, &message, &operation, &context, &origin, args[5] != 0);
        if error == 0 {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = error;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) fn write_error_scalar(ctx: *mut SpectraHostCallContext, field: usize) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len != 1
            || ctx_ref.args.is_null()
            || ctx_ref.result_len == 0
            || ctx_ref.results.is_null()
        {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let value = match read_error_field(args[0], field) {
            Some(value) => value,
            None => return HOST_STATUS_INVALID_ARGUMENT,
        };
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = value;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) fn write_error_string(ctx: *mut SpectraHostCallContext, field: usize) -> i32 {
    write_error_scalar(ctx, field)
}

pub(crate) extern "C" fn std_error_code(ctx: *mut SpectraHostCallContext) -> i32 {
    write_error_scalar(ctx, ERROR_FIELD_CODE)
}

pub(crate) extern "C" fn std_error_message(ctx: *mut SpectraHostCallContext) -> i32 {
    write_error_string(ctx, ERROR_FIELD_MESSAGE)
}

pub(crate) extern "C" fn std_error_operation(ctx: *mut SpectraHostCallContext) -> i32 {
    write_error_string(ctx, ERROR_FIELD_OPERATION)
}

pub(crate) extern "C" fn std_error_context(ctx: *mut SpectraHostCallContext) -> i32 {
    write_error_string(ctx, ERROR_FIELD_CONTEXT)
}

pub(crate) extern "C" fn std_error_origin(ctx: *mut SpectraHostCallContext) -> i32 {
    write_error_string(ctx, ERROR_FIELD_ORIGIN)
}

pub(crate) extern "C" fn std_error_retryable(ctx: *mut SpectraHostCallContext) -> i32 {
    write_error_scalar(ctx, ERROR_FIELD_RETRYABLE)
}
