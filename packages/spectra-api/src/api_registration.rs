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
    let ptr = ptr_value as *const i64;
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
            if !(0..=255).contains(&value) {
                return None;
            }
            bytes.push(value as u8);
            offset += 1;
            if offset > 1_048_576 {
                return None;
            }
        }
    }
    String::from_utf8(bytes).ok()
}

pub(crate) fn alloc_spectra_string(value: &str) -> SpectraHostValue {
    let len = value.len() + 1;
    let bytes = len * std::mem::size_of::<i64>();
    let ptr = spectra_runtime::ffi::spectra_rt_manual_alloc(bytes) as *mut i64;
    if ptr.is_null() {
        return 0;
    }
    unsafe {
        for (idx, byte) in value.bytes().enumerate() {
            *ptr.add(idx) = byte as i64;
        }
        *ptr.add(value.len()) = 0;
    }
    ptr as SpectraHostValue
}
