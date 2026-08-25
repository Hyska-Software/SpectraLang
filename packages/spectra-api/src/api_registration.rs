pub fn register() -> usize {
    spectra_runtime::initialize();
    spectra_runtime::register_standard_library();
    let mut inserted = 0;
    for spec in HOST_CALLS {
        if register_host_function(spec.name, spec.function) {
            inserted += 1;
        }
    }
    inserted
}

#[no_mangle]
pub extern "C" fn spectra_api_register_host_calls() -> usize {
    register()
}

#[no_mangle]
pub extern "C" fn spectra_api_host_call_count() -> usize {
    HOST_CALLS.len()
}

extern "C" fn api_version_major(ctx: *mut SpectraHostCallContext) -> i32 {
    write_result(ctx, VERSION_MAJOR)
}

extern "C" fn api_version_minor(ctx: *mut SpectraHostCallContext) -> i32 {
    write_result(ctx, VERSION_MINOR)
}

extern "C" fn api_version_patch(ctx: *mut SpectraHostCallContext) -> i32 {
    write_result(ctx, VERSION_PATCH)
}

pub(crate) fn read_args<'a>(
    ctx: *mut SpectraHostCallContext,
    expected: usize,
) -> Result<&'a [SpectraHostValue], i32> {
    if ctx.is_null() {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let ctx_ref = unsafe { &*ctx };
    if ctx_ref.arg_len < expected || (ctx_ref.arg_len > 0 && ctx_ref.args.is_null()) {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let args = if ctx_ref.arg_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len) }
    };
    Ok(args)
}

pub(crate) fn write_result(ctx: *mut SpectraHostCallContext, value: SpectraHostValue) -> i32 {
    if ctx.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let ctx_ref = unsafe { &mut *ctx };
    if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    let results = unsafe { std::slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len) };
    results[0] = value;
    HOST_STATUS_SUCCESS
}

pub(crate) fn read_spectra_string(ptr_value: SpectraHostValue) -> Option<String> {
    if ptr_value == 0 {
        return None;
    }
    // Strings cross the ABI as NUL-terminated packed UTF-8 byte buffers
    // (one byte per address unit — see runtime/src/ffi_fast_paths.rs).
    let ptr = ptr_value as *const u8;
    if ptr.is_null() {
        return None;
    }
    let mut bytes = Vec::new();
    unsafe {
        let mut offset = 0usize;
        loop {
            let value = *ptr.add(offset);
            if value == 0 {
                break;
            }
            bytes.push(value);
            offset += 1;
            if offset > 1_048_576 {
                return None;
            }
        }
    }
    String::from_utf8(bytes).ok()
}

pub(crate) fn alloc_spectra_string(value: &str) -> SpectraHostValue {
    let total = value.len() + 1; // payload + single NUL terminator byte
    let ptr = spectra_runtime::ffi::spectra_rt_manual_alloc(total) as *mut u8;
    if ptr.is_null() {
        return 0;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(value.as_ptr(), ptr, value.len());
        *ptr.add(value.len()) = 0;
    }
    ptr as SpectraHostValue
}
