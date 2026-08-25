use super::*;
#[track_caller]
pub(crate) fn tensor_alloc(
    dtype: TensorDType,
    shape: Vec<usize>,
    data: Vec<SpectraHostValue>,
) -> Result<usize, i32> {
    let data = with_tensor_registry(|registry| {
        let mut buffer = registry.take_buffer(data.len());
        buffer.copy_from_slice(&data);
        buffer
    });
    let Some(tensor) = StdTensor::new(dtype, shape, data) else {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    };
    let memory = initialize().memory();
    let tensor = memory
        .allocate_manual(tensor)
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    let site = tensor_allocation_site(std::panic::Location::caller());
    Ok(with_tensor_registry(|registry| {
        registry.insert(tensor, site)
    }))
}

#[track_caller]
pub(crate) fn tensor_alloc_buffered(
    dtype: TensorDType,
    shape: Vec<usize>,
    buffer: Vec<SpectraHostValue>,
) -> Result<usize, i32> {
    let Some(tensor) = StdTensor::new(dtype, shape, buffer) else {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    };
    let memory = initialize().memory();
    let tensor = memory
        .allocate_manual(tensor)
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    let site = tensor_allocation_site(std::panic::Location::caller());
    Ok(with_tensor_registry(|registry| {
        registry.insert(tensor, site)
    }))
}

#[track_caller]
pub(crate) fn tensor_insert(tensor: StdTensor) -> Result<usize, i32> {
    let memory = initialize().memory();
    let tensor = memory
        .allocate_manual(tensor)
        .map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
    let site = tensor_allocation_site(std::panic::Location::caller());
    Ok(with_tensor_registry(|registry| {
        registry.insert(tensor, site)
    }))
}

#[track_caller]
pub(crate) fn tensor_alloc_autograd(
    dtype: TensorDType,
    shape: Vec<usize>,
    data: Vec<SpectraHostValue>,
    requires_grad: bool,
    creator: Option<AutogradNode>,
) -> Result<usize, i32> {
    let data = with_tensor_registry(|registry| {
        let mut buffer = registry.take_buffer(data.len());
        buffer.copy_from_slice(&data);
        buffer
    });
    let Some(mut tensor) = StdTensor::new(dtype, shape, data) else {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    };
    tensor.requires_grad = requires_grad && dtype == TensorDType::Float;
    tensor.creator = if tensor.requires_grad { creator } else { None };
    tensor_insert(tensor)
}

#[track_caller]
pub(crate) fn tensor_alloc_autograd_on_device(
    dtype: TensorDType,
    shape: Vec<usize>,
    data: Vec<SpectraHostValue>,
    requires_grad: bool,
    creator: Option<AutogradNode>,
    device: TensorDevice,
    precision: TensorPrecision,
) -> Result<usize, i32> {
    let data = with_tensor_registry(|registry| {
        let mut buffer = registry.take_buffer(data.len());
        buffer.copy_from_slice(&data);
        buffer
    });
    let Some(mut tensor) = StdTensor::new(dtype, shape, data) else {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    };
    tensor.device = device;
    tensor.precision = precision;
    tensor.requires_grad = requires_grad && dtype == TensorDType::Float;
    tensor.creator = if tensor.requires_grad { creator } else { None };
    #[cfg(feature = "gpu")]
    if device == TensorDevice::Wgpu && dtype == TensorDType::Float {
        let f32_data = tensor_values_as_f32(&tensor).ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
        let n = tensor.len();
        let upload = with_tensor_registry(|registry| {
            crate::gpu::with_device_queue(|device, queue| {
                let buf = registry.device_arena.acquire(
                    crate::gpu::PoolDevice::Wgpu,
                    crate::gpu::PoolDType::Float,
                    n,
                    device,
                );
                queue.write_buffer(&buf.buffer, 0, bytemuck::cast_slice(&f32_data));
                queue.submit(None);
                buf
            })
        });
        let buf = upload.map_err(|_| HOST_STATUS_INTERNAL_ERROR)?;
        tensor
            .device_storage
            .insert(crate::gpu::PoolDevice::Wgpu, buf);
    }
    tensor_insert(tensor)
}

/// R-3052 full: like `tensor_alloc_autograd_on_device` but stores a
/// pre-acquired `DeviceBuffer` into the new tensor's `device_storage`
/// (Wgpu). The host data is a placeholder (zeros are fine — the GPU
/// output is the source of truth and the next residency-aware op will
/// read from the device buffer). Increments `note_device_resident` so
/// `stats_device_resident_tensors` tracks every residency output.
#[cfg(feature = "gpu")]
#[track_caller]
pub(crate) fn tensor_alloc_autograd_on_device_with_buffer(
    dtype: TensorDType,
    shape: Vec<usize>,
    data: Vec<SpectraHostValue>,
    requires_grad: bool,
    creator: Option<AutogradNode>,
    device: TensorDevice,
    precision: TensorPrecision,
    buf: crate::gpu::DeviceBuffer,
) -> Result<usize, i32> {
    let data = with_tensor_registry(|registry| {
        let mut buffer = registry.take_buffer(data.len());
        buffer.copy_from_slice(&data);
        buffer
    });
    let Some(mut tensor) = StdTensor::new(dtype, shape, data) else {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    };
    tensor.device = device;
    tensor.precision = precision;
    tensor.requires_grad = requires_grad && dtype == TensorDType::Float;
    tensor.creator = if tensor.requires_grad { creator } else { None };
    tensor
        .device_storage
        .insert(crate::gpu::PoolDevice::Wgpu, buf);
    let handle = tensor_insert(tensor)?;
    with_tensor_registry(|registry| {
        registry.note_device_resident();
    });
    Ok(handle)
}

pub(crate) fn tensor_allocation_site(location: &'static std::panic::Location<'static>) -> String {
    format!("{}:{}", location.file(), location.line())
}

pub(crate) fn tensor_result(ctx_ref: &mut SpectraHostCallContext, value: SpectraHostValue) -> i32 {
    if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
        return HOST_STATUS_INVALID_ARGUMENT;
    }
    unsafe {
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = value;
    }
    HOST_STATUS_SUCCESS
}

pub(crate) fn tensor_optional_result(ctx_ref: &mut SpectraHostCallContext, value: SpectraHostValue) -> i32 {
    if ctx_ref.result_len == 0 || ctx_ref.results.is_null() {
        return HOST_STATUS_SUCCESS;
    }
    unsafe {
        let results = slice::from_raw_parts_mut(ctx_ref.results, ctx_ref.result_len);
        results[0] = value;
    }
    HOST_STATUS_SUCCESS
}

#[inline]
pub(crate) fn fill_i64_pattern(buffer: &mut [SpectraHostValue], value: SpectraHostValue) {
    for slot in buffer.iter_mut() {
        *slot = value;
    }
}

pub(crate) unsafe fn tensor_args<'a>(
    ctx: *mut SpectraHostCallContext,
    expected: usize,
) -> Result<(&'a mut SpectraHostCallContext, &'a [SpectraHostValue]), i32> {
    if ctx.is_null() {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let ctx_ref = &mut *ctx;
    if ctx_ref.arg_len != expected || (expected > 0 && ctx_ref.args.is_null()) {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let args = if expected == 0 {
        &[] as &[SpectraHostValue]
    } else {
        slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len)
    };
    Ok((ctx_ref, args))
}

pub(crate) fn tensor_create_1d(
    ctx: *mut SpectraHostCallContext,
    value: SpectraHostValue,
    dtype: TensorDType,
) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let size = args[0];
        if size <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let data = vec![value; size as usize];
        match tensor_alloc(dtype, vec![size as usize], data) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) fn tensor_create_2d(
    ctx: *mut SpectraHostCallContext,
    value: SpectraHostValue,
    dtype: TensorDType,
) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let rows = args[0];
        let cols = args[1];
        if rows <= 0 || cols <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(len) = (rows as usize).checked_mul(cols as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let data = vec![value; len];
        match tensor_alloc(dtype, vec![rows as usize, cols as usize], data) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_zeros(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_create_1d(ctx, 0, TensorDType::Int)
}

pub(crate) extern "C" fn std_tensor_ones(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_create_1d(ctx, 1, TensorDType::Int)
}

pub(crate) extern "C" fn std_tensor_full(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[0] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let data = vec![args[1]; args[0] as usize];
        match tensor_alloc(TensorDType::Int, vec![args[0] as usize], data) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_full_f(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let n = args[0];
        if n <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let len = n as usize;
        let value = args[1];
        let buffer = with_tensor_registry(|registry| {
            if let Some(buffer) = registry.take_buffer_unfilled(len) {
                registry.metrics.reused_buffers = registry.metrics.reused_buffers.saturating_add(1);
                registry.metrics.pool_hits = registry.metrics.pool_hits.saturating_add(1);
                Some(buffer)
            } else {
                registry.metrics.pool_misses = registry.metrics.pool_misses.saturating_add(1);
                None
            }
        });
        let mut buffer = buffer.unwrap_or_else(|| Vec::with_capacity(len));
        if buffer.len() < len {
            buffer.resize(len, 0);
        }
        fill_i64_pattern(&mut buffer, value);
        match tensor_alloc_buffered(TensorDType::Float, vec![len], buffer) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_refill(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let handle = args[0] as usize;
        let value = args[1];
        let result = with_tensor_registry(|registry| {
            let Some(tensor) = registry.get_mut(handle) else {
                return Err(HOST_STATUS_NOT_FOUND);
            };
            if tensor.dtype != TensorDType::Float {
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            if tensor.requires_grad {
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            if !tensor.is_contiguous() || tensor.offset != 0 {
                return Err(HOST_STATUS_INVALID_ARGUMENT);
            }
            let len = tensor.len();
            let storage = Arc::make_mut(&mut tensor.storage);
            if storage.len() < len {
                storage.resize(len, 0);
            }
            fill_i64_pattern(storage, value);
            Ok(())
        });
        match result {
            Ok(()) => tensor_optional_result(ctx_ref, 0),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_literal(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_literal_1d(ctx, TensorDType::Int)
}

pub(crate) extern "C" fn std_tensor_literal_f(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_literal_1d(ctx, TensorDType::Float)
}

pub(crate) extern "C" fn std_tensor_literal2(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_literal_2d(ctx, TensorDType::Int)
}

pub(crate) extern "C" fn std_tensor_literal2_f(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_literal_2d(ctx, TensorDType::Float)
}

pub(crate) fn tensor_literal_1d(ctx: *mut SpectraHostCallContext, dtype: TensorDType) -> i32 {
    unsafe {
        if ctx.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len == 0 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let len = args[0];
        if len < 0 || args.len() != (len as usize).saturating_add(1) {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let data = args[1..].to_vec();
        match tensor_alloc(dtype, vec![len as usize], data) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) fn tensor_literal_2d(ctx: *mut SpectraHostCallContext, dtype: TensorDType) -> i32 {
    unsafe {
        if ctx.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let ctx_ref = &mut *ctx;
        if ctx_ref.arg_len < 2 || ctx_ref.args.is_null() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let args = slice::from_raw_parts(ctx_ref.args, ctx_ref.arg_len);
        let rows = args[0];
        let cols = args[1];
        if rows < 0 || cols < 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(len) = (rows as usize).checked_mul(cols as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args.len() != len.saturating_add(2) {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let data = args[2..].to_vec();
        match tensor_alloc(dtype, vec![rows as usize, cols as usize], data) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_arange(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (start, end, step) = (args[0], args[1], args[2]);
        if step == 0 || (step > 0 && start >= end) || (step < 0 && start <= end) {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let mut data = Vec::new();
        let mut current = start;
        while (step > 0 && current < end) || (step < 0 && current > end) {
            data.push(current);
            current = current.saturating_add(step);
            if data.len() > 10_000_000 {
                return HOST_STATUS_INVALID_ARGUMENT;
            }
        }
        match tensor_alloc(TensorDType::Int, vec![data.len()], data) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_zeros2(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_create_2d(ctx, 0, TensorDType::Int)
}

pub(crate) extern "C" fn std_tensor_ones2(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_create_2d(ctx, 1, TensorDType::Int)
}

pub(crate) extern "C" fn std_tensor_full2(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[0] <= 0 || args[1] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(len) = (args[0] as usize).checked_mul(args[1] as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let data = vec![args[2]; len];
        match tensor_alloc(
            TensorDType::Int,
            vec![args[0] as usize, args[1] as usize],
            data,
        ) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_full2_f(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[0] <= 0 || args[1] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(len) = (args[0] as usize).checked_mul(args[1] as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let data = vec![args[2]; len];
        match tensor_alloc(
            TensorDType::Float,
            vec![args[0] as usize, args[1] as usize],
            data,
        ) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_len(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_query_i64(ctx, |tensor| tensor.len() as SpectraHostValue)
}

pub(crate) extern "C" fn std_tensor_rank(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_query_i64(ctx, |tensor| tensor.shape.len() as SpectraHostValue)
}

pub(crate) extern "C" fn std_tensor_dim(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let value = with_tensor_registry(|registry| {
            registry
                .get(args[0] as usize)
                .and_then(|tensor| tensor.shape.get(args[1] as usize).copied())
                .map(|dim| dim as SpectraHostValue)
                .unwrap_or(-1)
        });
        tensor_result(ctx_ref, value)
    }
}

pub(crate) extern "C" fn std_tensor_rows(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_query_i64(ctx, |tensor| {
        tensor.shape.first().copied().unwrap_or(0) as i64
    })
}

pub(crate) extern "C" fn std_tensor_cols(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_query_i64(ctx, |tensor| {
        tensor.shape.get(1).copied().unwrap_or(1) as i64
    })
}

pub(crate) extern "C" fn std_tensor_is_valid(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let valid = with_tensor_registry(|registry| registry.get(args[0] as usize).is_some());
        tensor_result(ctx_ref, valid as SpectraHostValue)
    }
}

pub(crate) fn tensor_query_i64(
    ctx: *mut SpectraHostCallContext,
    query: impl FnOnce(&StdTensor) -> SpectraHostValue,
) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(value) =
            with_tensor_registry(|registry| registry.get(args[0] as usize).map(query))
        else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, value)
    }
}

pub(crate) extern "C" fn std_tensor_get(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_get_linear(ctx, false)
}

pub(crate) extern "C" fn std_tensor_get_f(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_get_linear(ctx, true)
}

pub(crate) fn tensor_get_linear(ctx: *mut SpectraHostCallContext, as_float: bool) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let index = args[1];
        if index < 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(value) = with_tensor_registry(|registry| {
            registry
                .get(args[0] as usize)
                .and_then(|tensor| tensor.value_at_linear(index as usize))
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let value = if as_float {
            value
        } else {
            f64_bits_to_i64_if_needed(value)
        };
        tensor_result(ctx_ref, value)
    }
}

pub(crate) extern "C" fn std_tensor_set(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_set_linear(ctx, false)
}

pub(crate) extern "C" fn std_tensor_set_f(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_set_linear(ctx, true)
}

pub(crate) fn tensor_set_linear(ctx: *mut SpectraHostCallContext, is_float: bool) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[1] < 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let ok = with_tensor_registry(|registry| {
            let Some(tensor) = registry.get_mut(args[0] as usize) else {
                return false;
            };
            if (args[1] as usize) >= tensor.len() {
                return false;
            }
            tensor.dtype = if is_float {
                TensorDType::Float
            } else {
                TensorDType::Int
            };
            tensor.set_linear(args[1] as usize, args[2])
        });
        if !ok {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_tensor_get2(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_get_2d(ctx, false)
}

pub(crate) extern "C" fn std_tensor_get2_f(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_get_2d(ctx, true)
}

pub(crate) fn tensor_get_2d(ctx: *mut SpectraHostCallContext, as_float: bool) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[1] < 0 || args[2] < 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(value) = with_tensor_registry(|registry| {
            let tensor = registry.get(args[0] as usize)?;
            let offset = tensor.offset(&[args[1] as usize, args[2] as usize])?;
            tensor.storage.get(offset).copied()
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let value = if as_float {
            value
        } else {
            f64_bits_to_i64_if_needed(value)
        };
        tensor_result(ctx_ref, value)
    }
}

pub(crate) extern "C" fn std_tensor_set2(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_set_2d(ctx, false)
}

pub(crate) extern "C" fn std_tensor_set2_f(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_set_2d(ctx, true)
}

pub(crate) fn tensor_set_2d(ctx: *mut SpectraHostCallContext, is_float: bool) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 4) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[1] < 0 || args[2] < 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let ok = with_tensor_registry(|registry| {
            let Some(tensor) = registry.get_mut(args[0] as usize) else {
                return false;
            };
            let Some(offset) = tensor.offset(&[args[1] as usize, args[2] as usize]) else {
                return false;
            };
            tensor.dtype = if is_float {
                TensorDType::Float
            } else {
                TensorDType::Int
            };
            let storage = Arc::make_mut(&mut tensor.storage);
            if offset >= storage.len() {
                return false;
            }
            storage[offset] = args[3];
            true
        });
        if !ok {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_tensor_reshape(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (handle, rows, cols) = (args[0] as usize, args[1], args[2]);
        if rows <= 0 || cols <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(new_len) = (rows as usize).checked_mul(cols as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(tensor) = with_tensor_registry(|registry| {
            registry.get(handle).and_then(|tensor| {
                if tensor.len() != new_len {
                    return None;
                }
                let mut result = StdTensor::from_storage(
                    tensor.dtype,
                    vec![rows as usize, cols as usize],
                    tensor_strides(&[rows as usize, cols as usize]),
                    tensor.storage.clone(),
                    tensor.offset,
                    if tensor.is_contiguous() {
                        TensorLayout::Contiguous
                    } else {
                        TensorLayout::View
                    },
                )?;
                result.device = tensor.device;
                result.precision = tensor.precision;
                let requires_grad = tensor.dtype == TensorDType::Float
                    && tensor_requires_autograd(registry, &[handle]);
                if requires_grad {
                    result.requires_grad = true;
                    result.creator = Some(AutogradNode {
                        op: AutogradOp::View,
                        parents: vec![handle],
                        input_shape: tensor.shape.clone(),
                        left_shape: Vec::new(),
                        right_shape: Vec::new(),
                        input: tensor_values_as_f64(tensor),
                        output: Vec::new(),
                        left: Vec::new(),
                        right: Vec::new(),
                        aux: Vec::new(),
                        #[cfg(feature = "gpu")]
                        device_aux: None,
                    });
                }
                Some(result)
            })
        }) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match tensor_insert(tensor) {
            Ok(new_handle) => tensor_result(ctx_ref, new_handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_flatten(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(tensor) = with_tensor_registry(|registry| {
            registry.get(args[0] as usize).and_then(|tensor| {
                let mut result = if tensor.is_contiguous() {
                    StdTensor::from_storage(
                        tensor.dtype,
                        vec![tensor.len()],
                        vec![1],
                        tensor.storage.clone(),
                        tensor.offset,
                        TensorLayout::Contiguous,
                    )
                } else {
                    let data = tensor.materialize();
                    StdTensor::new(tensor.dtype, vec![data.len()], data)
                }?;
                result.device = tensor.device;
                result.precision = tensor.precision;
                let requires_grad = tensor.dtype == TensorDType::Float
                    && tensor_requires_autograd(registry, &[args[0] as usize]);
                if requires_grad {
                    result.requires_grad = true;
                    result.creator = Some(AutogradNode {
                        op: AutogradOp::View,
                        parents: vec![args[0] as usize],
                        input_shape: tensor.shape.clone(),
                        left_shape: Vec::new(),
                        right_shape: Vec::new(),
                        input: tensor_values_as_f64(tensor),
                        output: Vec::new(),
                        left: Vec::new(),
                        right: Vec::new(),
                        aux: Vec::new(),
                        #[cfg(feature = "gpu")]
                        device_aux: None,
                    });
                }
                Some(result)
            })
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        match tensor_insert(tensor) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_permute(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[1] < 0 || args[2] < 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(tensor) = with_tensor_registry(|registry| {
            let tensor = registry.get(args[0] as usize)?;
            let (axis_a, axis_b) = (args[1] as usize, args[2] as usize);
            if axis_a >= tensor.shape.len() || axis_b >= tensor.shape.len() {
                return None;
            }
            let mut shape = tensor.shape.clone();
            let mut strides = tensor.strides.clone();
            shape.swap(axis_a, axis_b);
            strides.swap(axis_a, axis_b);
            let mut result = StdTensor::from_storage(
                tensor.dtype,
                shape,
                strides,
                tensor.storage.clone(),
                tensor.offset,
                TensorLayout::View,
            )?;
            result.device = tensor.device;
            result.precision = tensor.precision;
            Some(result)
        }) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match tensor_insert(tensor) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_slice(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (handle, start, end) = (args[0] as usize, args[1], args[2]);
        if start < 0 || end < start {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(tensor) = with_tensor_registry(|registry| {
            let tensor = registry.get(handle)?;
            if tensor.shape.len() != 1 || end as usize > tensor.len() || start == end {
                return None;
            }
            let base_offset = tensor.linear_offset(start as usize)?;
            let mut result = StdTensor::from_storage(
                tensor.dtype,
                vec![(end - start) as usize],
                vec![tensor.strides[0]],
                tensor.storage.clone(),
                base_offset,
                TensorLayout::View,
            )?;
            result.device = tensor.device;
            result.precision = tensor.precision;
            Some(result)
        }) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match tensor_insert(tensor) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_concat(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((dtype, shape, data)) = with_tensor_registry(|registry| {
            let left = registry.get(args[0] as usize)?;
            let right = registry.get(args[1] as usize)?;
            if left.dtype != right.dtype || left.shape.len() != right.shape.len() {
                return None;
            }
            let mut shape = left.shape.clone();
            if left.shape.len() == 1 {
                shape[0] = left.shape[0].checked_add(right.shape[0])?;
            } else {
                if left.shape[1..] != right.shape[1..] {
                    return None;
                }
                shape[0] = left.shape[0].checked_add(right.shape[0])?;
            }
            let mut data = left.materialize();
            data.extend(right.materialize());
            let dtype = left.dtype;
            registry.note_kernel(data.len());
            Some((dtype, shape, data))
        }) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match tensor_alloc(dtype, shape, data) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_stack(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((dtype, shape, data)) = with_tensor_registry(|registry| {
            let left = registry.get(args[0] as usize)?;
            let right = registry.get(args[1] as usize)?;
            if left.dtype != right.dtype || left.shape != right.shape {
                return None;
            }
            let mut shape = Vec::with_capacity(left.shape.len() + 1);
            shape.push(2);
            shape.extend(left.shape.iter().copied());
            let mut data = left.materialize();
            data.extend(right.materialize());
            let dtype = left.dtype;
            registry.note_kernel(data.len());
            Some((dtype, shape, data))
        }) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match tensor_alloc(dtype, shape, data) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_tensor_add(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_binary(ctx, AutogradOp::Add, |a, b| a + b, |a, b| a + b)
}

pub(crate) extern "C" fn std_tensor_sub(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_binary(ctx, AutogradOp::Sub, |a, b| a - b, |a, b| a - b)
}

pub(crate) extern "C" fn std_tensor_mul(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_binary(ctx, AutogradOp::Mul, |a, b| a * b, |a, b| a * b)
}

pub(crate) extern "C" fn std_tensor_div(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_binary(
        ctx,
        AutogradOp::Div,
        |a, b| if b == 0 { 0 } else { a / b },
        |a, b| if b == 0.0 { f64::NAN } else { a / b },
    )
}
