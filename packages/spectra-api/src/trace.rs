use spectra_runtime::ffi::{
    HostFunction, SpectraHostCallContext, HOST_STATUS_INVALID_ARGUMENT, HOST_STATUS_SUCCESS,
};
use spectra_runtime::tracing::{self, SpanKind, SpanStatus};

unsafe fn string(value: i64) -> Option<String> {
    if value == 0 {
        return None;
    }
    let ptr = value as *const u8;
    let mut bytes = Vec::new();
    for index in 0..4096 {
        let byte = *ptr.add(index) as u8;
        if byte == 0 {
            return String::from_utf8(bytes).ok();
        }
        bytes.push(byte);
    }
    None
}
unsafe fn args<'a>(ctx: *mut SpectraHostCallContext) -> Option<(&'a [i64], &'a mut [i64])> {
    if ctx.is_null() {
        return None;
    }
    let context = &*ctx;
    let input = if context.args.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(context.args, context.arg_len)
    };
    let output = if context.results.is_null() {
        &mut []
    } else {
        std::slice::from_raw_parts_mut(context.results, context.result_len)
    };
    Some((input, output))
}
fn result(results: &mut [i64], value: i64) -> i32 {
    if results.is_empty() {
        HOST_STATUS_INVALID_ARGUMENT
    } else {
        results[0] = value;
        HOST_STATUS_SUCCESS
    }
}
fn ok(value: Result<(), &'static str>, results: &mut [i64]) -> i32 {
    if results.is_empty() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    results[0] = value.is_ok() as i64;
    HOST_STATUS_SUCCESS
}

pub extern "C" fn config_new(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (Some(endpoint), Some(service)) = (string(a[0]), string(a[1])) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        result(
            r,
            tracing::config_new(&endpoint, &service)
                .map(|id| id as i64)
                .unwrap_or(0),
        )
    }
}
pub extern "C" fn config_set_sample_rate(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        ok(
            tracing::config_set_sample_rate(a[0] as u64, f64::from_bits(a[1] as u64)),
            r,
        )
    }
}
pub extern "C" fn config_set_batch_size(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        ok(
            tracing::config_set_batch_size(a[0] as u64, a[1] as usize),
            r,
        )
    }
}
pub extern "C" fn config_start(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        ok(tracing::config_start(a[0] as u64), r)
    }
}
pub extern "C" fn config_shutdown(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        ok(tracing::config_shutdown(a[0] as u64), r)
    }
}
pub extern "C" fn span_start(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(name) = string(a[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let kind = match a[1] {
            2 => SpanKind::Server,
            3 => SpanKind::Client,
            4 => SpanKind::Producer,
            5 => SpanKind::Consumer,
            _ => SpanKind::Internal,
        };
        result(
            r,
            tracing::span_start(&name, kind)
                .map(|id| id as i64)
                .unwrap_or(0),
        )
    }
}
pub extern "C" fn span_set_attribute(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 3 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (Some(k), Some(v)) = (string(a[1]), string(a[2])) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        ok(tracing::span_set_attribute(a[0] as u64, &k, &v), r)
    }
}
pub extern "C" fn span_set_attribute_int(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 3 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(k) = string(a[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        ok(tracing::span_set_attribute_int(a[0] as u64, &k, a[2]), r)
    }
}
pub extern "C" fn span_set_attribute_bool(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 3 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(k) = string(a[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        ok(
            tracing::span_set_attribute_bool(a[0] as u64, &k, a[2] != 0),
            r,
        )
    }
}
pub extern "C" fn span_set_status(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let status = match a[1] {
            1 => SpanStatus::Ok,
            2 => SpanStatus::Error,
            _ => SpanStatus::Unset,
        };
        ok(tracing::span_set_status(a[0] as u64, status), r)
    }
}
pub extern "C" fn span_end(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        ok(tracing::span_end(a[0] as u64), r)
    }
}
pub extern "C" fn current(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((_, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        result(r, tracing::current().map(|v| v as i64).unwrap_or(0))
    }
}
pub extern "C" fn flush(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((_, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match tracing::flush() {
            Ok(n) => result(r, n as i64),
            Err(_) => result(r, -1),
        }
    }
}
pub extern "C" fn inject(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match tracing::inject(a[0] as u64) {
            Ok(_) => result(r, 1),
            Err(_) => result(r, 0),
        }
    }
}
pub extern "C" fn extract(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((a, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if a.len() != 1 {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match string(a[0]).and_then(|s| tracing::extract(&s).ok()) {
            Some(_) => result(r, 1),
            None => result(r, 0),
        }
    }
}
pub extern "C" fn parent(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((_, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        result(r, 0)
    }
}
pub extern "C" fn last_error(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Some((_, r)) = args(ctx) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        result(
            r,
            tracing::last_error()
                .map(|error| tracing::alloc_string(&error))
                .unwrap_or_else(|| tracing::alloc_string("")),
        )
    }
}

pub const HOST_CALLS: &[(&str, HostFunction)] = &[
    ("spectra.api.trace.config_new", config_new),
    (
        "spectra.api.trace.config_set_sample_rate",
        config_set_sample_rate,
    ),
    (
        "spectra.api.trace.config_set_batch_size",
        config_set_batch_size,
    ),
    ("spectra.api.trace.config_start", config_start),
    ("spectra.api.trace.config_shutdown", config_shutdown),
    ("spectra.api.trace.span_start", span_start),
    ("spectra.api.trace.span_set_attribute", span_set_attribute),
    (
        "spectra.api.trace.span_set_attribute_int",
        span_set_attribute_int,
    ),
    (
        "spectra.api.trace.span_set_attribute_bool",
        span_set_attribute_bool,
    ),
    ("spectra.api.trace.span_set_status", span_set_status),
    ("spectra.api.trace.span_end", span_end),
    ("spectra.api.trace.current", current),
    ("spectra.api.trace.parent", parent),
    ("spectra.api.trace.inject", inject),
    ("spectra.api.trace.extract", extract),
    ("spectra.api.trace.flush", flush),
    ("spectra.api.trace.last_error", last_error),
];
