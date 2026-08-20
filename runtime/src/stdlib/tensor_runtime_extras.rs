extern "C" fn std_tensor_seed(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        *lock_unpoisoned(random_state()) = args[0] as u64;
        tensor_optional_result(ctx_ref, 0)
    }
}

extern "C" fn std_tensor_uniform(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (size, min, max) = (args[0], args[1], args[2]);
        if size <= 0 || min >= max {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let range = (max - min) as u64;
        let mut state = lock_unpoisoned(random_state());
        let data = (0..size as usize)
            .map(|_| min + (lcg_next(&mut state) % range) as i64)
            .collect::<Vec<_>>();
        drop(state);
        with_tensor_registry(|registry| registry.note_kernel(data.len()));
        match tensor_alloc(TensorDType::Int, vec![size as usize], data) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

extern "C" fn std_tensor_uniform_f(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (size, min, max) = (
            args[0],
            f64::from_bits(args[1] as u64),
            f64::from_bits(args[2] as u64),
        );
        if size <= 0 || !min.is_finite() || !max.is_finite() || min >= max {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let mut state = lock_unpoisoned(random_state());
        let data = (0..size as usize)
            .map(|_| {
                let unit = random_unit_f64(&mut state);
                (min + (max - min) * unit).to_bits() as i64
            })
            .collect::<Vec<_>>();
        drop(state);
        with_tensor_registry(|registry| registry.note_kernel(data.len()));
        match tensor_alloc(TensorDType::Float, vec![size as usize], data) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

extern "C" fn std_tensor_normal_f(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (size, mean, stddev) = (
            args[0],
            f64::from_bits(args[1] as u64),
            f64::from_bits(args[2] as u64),
        );
        if size <= 0 || !mean.is_finite() || !stddev.is_finite() || stddev < 0.0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let mut state = lock_unpoisoned(random_state());
        let mut data = Vec::with_capacity(size as usize);
        while data.len() < size as usize {
            let u1 = random_unit_f64(&mut state).max(f64::MIN_POSITIVE);
            let u2 = random_unit_f64(&mut state);
            let radius = (-2.0 * u1.ln()).sqrt();
            let theta = std::f64::consts::TAU * u2;
            data.push((mean + stddev * radius * theta.cos()).to_bits() as i64);
            if data.len() < size as usize {
                data.push((mean + stddev * radius * theta.sin()).to_bits() as i64);
            }
        }
        drop(state);
        with_tensor_registry(|registry| registry.note_kernel(data.len()));
        match tensor_alloc(TensorDType::Float, vec![size as usize], data) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

extern "C" fn std_tensor_set_deterministic_mode(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let enabled = args[0] != 0;
        *lock_unpoisoned(tensor_deterministic_mode()) = enabled;
        if enabled {
            *lock_unpoisoned(random_state()) = 0x5350_4543_5452_4131;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

extern "C" fn std_tensor_deterministic_mode(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let enabled = *lock_unpoisoned(tensor_deterministic_mode());
        tensor_result(ctx_ref, enabled as SpectraHostValue)
    }
}

extern "C" fn std_tensor_tolerance_abs(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        tensor_result(
            ctx_ref,
            NUMERICAL_TOLERANCE_ABS.to_bits() as SpectraHostValue,
        )
    }
}

extern "C" fn std_tensor_tolerance_rel(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        tensor_result(
            ctx_ref,
            NUMERICAL_TOLERANCE_REL.to_bits() as SpectraHostValue,
        )
    }
}

extern "C" fn std_tensor_bernoulli(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (size, p) = (args[0], f64::from_bits(args[1] as u64));
        if size <= 0 || !(0.0..=1.0).contains(&p) {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let mut state = lock_unpoisoned(random_state());
        let data = (0..size as usize)
            .map(|_| {
                if random_unit_f64(&mut state) < p {
                    1
                } else {
                    0
                }
            })
            .collect::<Vec<_>>();
        drop(state);
        with_tensor_registry(|registry| registry.note_kernel(data.len()));
        match tensor_alloc(TensorDType::Int, vec![size as usize], data) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

extern "C" fn std_tensor_categorical(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (size, probabilities_handle) = (args[0], args[1] as usize);
        if size <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(probabilities) = with_tensor_registry(|registry| {
            let tensor = registry.get(probabilities_handle)?;
            let source = tensor.materialize();
            if tensor.shape.len() != 1 || source.is_empty() {
                return None;
            }
            let mut total = 0.0f64;
            let mut weights = Vec::with_capacity(source.len());
            for raw in &source {
                let weight = match tensor.dtype {
                    TensorDType::Int => *raw as f64,
                    TensorDType::Float => f64::from_bits(*raw as u64),
                };
                if !weight.is_finite() || weight < 0.0 {
                    return None;
                }
                total += weight;
                weights.push(weight);
            }
            if total <= 0.0 {
                return None;
            }
            Some((weights, total))
        }) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (weights, total) = probabilities;
        let mut state = lock_unpoisoned(random_state());
        let mut data = Vec::with_capacity(size as usize);
        for _ in 0..size as usize {
            let mut sample = random_unit_f64(&mut state) * total;
            let mut selected = weights.len().saturating_sub(1);
            for (index, weight) in weights.iter().enumerate() {
                if sample < *weight {
                    selected = index;
                    break;
                }
                sample -= *weight;
            }
            data.push(selected as SpectraHostValue);
        }
        drop(state);
        with_tensor_registry(|registry| registry.note_kernel(data.len()));
        match tensor_alloc(TensorDType::Int, vec![size as usize], data) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

extern "C" fn std_tensor_device(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(device) = with_tensor_registry(|registry| {
            registry
                .get(args[0] as usize)
                .map(|tensor| tensor.device.code())
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, device)
    }
}

extern "C" fn std_tensor_device_available(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(device) = TensorDevice::from_code(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if !device.is_implemented() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        tensor_result(ctx_ref, if device.is_available() { 1 } else { 0 })
    }
}

extern "C" fn std_tensor_device_status(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(device) = TensorDevice::from_code(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if !device.is_implemented() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        tensor_result(ctx_ref, device.status_code())
    }
}

extern "C" fn std_tensor_to_device(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(target_device) = TensorDevice::from_code(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if !target_device.is_implemented() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if !target_device.is_available() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }

        let Some(source) = with_tensor_registry(|registry| registry.get(args[0] as usize).cloned())
        else {
            return HOST_STATUS_NOT_FOUND;
        };
        if target_device.is_accelerator() && source.dtype != TensorDType::Float {
            return HOST_STATUS_INVALID_ARGUMENT;
        }

        let data = source.materialize();
        let mut moved = match StdTensor::new(source.dtype, source.shape.clone(), data) {
            Some(t) => t,
            None => return HOST_STATUS_INTERNAL_ERROR,
        };
        moved.device = target_device;
        moved.precision = if target_device == TensorDevice::Wgpu {
            TensorPrecision::F32
        } else {
            source.precision
        };
        moved.requires_grad = source.requires_grad;
        moved.grad = source.grad.clone();

        // R-3021: when target is Wgpu, do a real device upload through the
        // pool. The acquire/write/submit sequence keeps the queue ordered so
        // the next kernel dispatch sees the data.
        if target_device == TensorDevice::Wgpu {
            #[cfg(feature = "gpu")]
            {
                let n = moved.len();
                let f32_data = match tensor_values_as_f32(&moved) {
                    Some(values) => values,
                    None => return HOST_STATUS_INVALID_ARGUMENT,
                };
                let upload_result = with_tensor_registry(|registry| {
                    crate::gpu::with_device_queue(|device, queue| {
                        let buf = registry.device_arena.acquire(
                            crate::gpu::PoolDevice::Wgpu,
                            crate::gpu::PoolDType::Float,
                            n,
                            device,
                        );
                        let bytes = bytemuck::cast_slice(&f32_data);
                        queue.write_buffer(&buf.buffer, 0, bytes);
                        queue.submit(None);
                        buf
                    })
                });
                let buf = match upload_result {
                    Ok(buf) => buf,
                    Err(_) => return HOST_STATUS_INTERNAL_ERROR,
                };
                moved
                    .device_storage
                    .insert(crate::gpu::PoolDevice::Wgpu, buf);
            }
            #[cfg(not(feature = "gpu"))]
            {
                return HOST_STATUS_INVALID_ARGUMENT;
            }
        }

        match tensor_insert(moved) {
            Ok(handle) => {
                with_tensor_registry(|registry| {
                    registry.note_device_transfer();
                    if target_device == TensorDevice::Wgpu {
                        registry.note_device_resident();
                    }
                });
                tensor_result(ctx_ref, handle as SpectraHostValue)
            }
            Err(code) => code,
        }
    }
}

extern "C" fn std_tensor_cpu(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let transfer_args = [args[0], TensorDevice::Cpu.code()];
        let mut transfer_ctx = SpectraHostCallContext {
            args: transfer_args.as_ptr(),
            arg_len: transfer_args.len(),
            results: ctx_ref.results,
            result_len: ctx_ref.result_len,
            invoke_fn: ctx_ref.invoke_fn,
        };
        std_tensor_to_device(&mut transfer_ctx)
    }
}

extern "C" fn std_tensor_sync(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let exists = with_tensor_registry(|registry| registry.get(args[0] as usize).is_some());
        if !exists {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

extern "C" fn std_tensor_precision(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(precision) = with_tensor_registry(|registry| {
            registry
                .get(args[0] as usize)
                .map(|tensor| tensor.precision.code())
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, precision)
    }
}

extern "C" fn std_tensor_to_precision(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(target_precision) = TensorPrecision::from_code(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(source) = with_tensor_registry(|registry| registry.get(args[0] as usize).cloned())
        else {
            return HOST_STATUS_NOT_FOUND;
        };
        if source.dtype != TensorDType::Float {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let data = source
            .materialize()
            .iter()
            .map(|raw| {
                target_precision
                    .quantize(f64::from_bits(*raw as u64))
                    .to_bits() as SpectraHostValue
            })
            .collect::<Vec<_>>();
        let Some(mut converted) = StdTensor::new(source.dtype, source.shape.clone(), data) else {
            return HOST_STATUS_INTERNAL_ERROR;
        };
        converted.device = source.device;
        converted.precision = target_precision;
        converted.requires_grad = source.requires_grad;
        converted.grad = source.grad.clone();
        match tensor_insert(converted) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

extern "C" fn std_tensor_stats_allocations(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_metric(ctx, |metrics| metrics.allocations)
}

extern "C" fn std_tensor_stats_active(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_metric(ctx, |metrics| metrics.active_tensors)
}

extern "C" fn std_tensor_stats_peak_bytes(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_metric(ctx, |metrics| metrics.peak_bytes)
}

extern "C" fn std_tensor_stats_reused_buffers(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_metric(ctx, |metrics| metrics.reused_buffers)
}

extern "C" fn std_tensor_stats_pool_hits(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_metric(ctx, |metrics| metrics.pool_hits)
}

extern "C" fn std_tensor_stats_pool_misses(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_metric(ctx, |metrics| metrics.pool_misses)
}

extern "C" fn std_tensor_stats_active_bytes(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_metric(ctx, |metrics| metrics.active_bytes)
}

extern "C" fn std_tensor_stats_scratch_reuses(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_metric(ctx, |metrics| metrics.scratch_reuses)
}

extern "C" fn std_tensor_kernel_strategy(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        tensor_result(ctx_ref, TensorKernelStrategy::current().code())
    }
}

extern "C" fn std_tensor_stats_kernel_ops(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_metric(ctx, |metrics| metrics.kernel_ops)
}

extern "C" fn std_tensor_stats_kernel_elements(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_metric(ctx, |metrics| metrics.kernel_elements)
}

extern "C" fn std_tensor_stats_device_transfers(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_metric(ctx, |metrics| metrics.device_transfers)
}

#[cfg(feature = "gpu")]
extern "C" fn std_tensor_stats_device_pool_hits(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let value =
            with_tensor_registry(|registry| registry.device_arena.hits()) as SpectraHostValue;
        tensor_result(ctx_ref, value)
    }
}

#[cfg(feature = "gpu")]
extern "C" fn std_tensor_stats_device_pool_misses(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let value =
            with_tensor_registry(|registry| registry.device_arena.misses()) as SpectraHostValue;
        tensor_result(ctx_ref, value)
    }
}

#[cfg(feature = "gpu")]
extern "C" fn std_tensor_stats_device_pool_bytes_resident(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let value = with_tensor_registry(|registry| registry.device_arena.bytes_resident())
            as SpectraHostValue;
        tensor_result(ctx_ref, value)
    }
}

#[cfg(feature = "gpu")]
extern "C" fn std_tensor_storage_device(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(code) = with_tensor_registry(|registry| {
            registry.get(args[0] as usize).map(|t| match t.device {
                TensorDevice::Wgpu => 6,
                _ => 0,
            })
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, code)
    }
}

extern "C" fn std_tensor_stats_gpu_kernel_ops(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_metric(ctx, |metrics| metrics.gpu_kernel_ops)
}

extern "C" fn std_tensor_stats_cpu_fallbacks(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_metric(ctx, |metrics| metrics.cpu_fallbacks)
}

extern "C" fn std_tensor_stats_gpu_errors(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let code = args[0];
        let slot = if code < 0 { usize::MAX } else { code as usize };
        let value = with_tensor_registry(|registry| {
            if slot < registry.metrics.gpu_errors.len() {
                registry.metrics.gpu_errors[slot]
            } else {
                0
            }
        });
        tensor_result(ctx_ref, value as SpectraHostValue)
    }
}

extern "C" fn std_tensor_stats_device_resident_tensors(ctx: *mut SpectraHostCallContext) -> i32 {
    tensor_metric(ctx, |metrics| metrics.device_resident_tensors)
}

extern "C" fn std_tensor_stats_gpu_backward_ops(ctx: *mut SpectraHostCallContext) -> i32 {
    let value = gpu_backward_ops_counter().load(std::sync::atomic::Ordering::Relaxed);
    tensor_metric(ctx, move |_| value)
}

extern "C" fn std_tensor_stats_graph_nodes(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let count = with_tensor_registry(|registry| {
            registry
                .tensors
                .values()
                .filter(|tensor| tensor.creator.is_some())
                .count()
        });
        tensor_result(ctx_ref, count as SpectraHostValue)
    }
}

extern "C" fn std_tensor_stats_lifetime_records(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let value = with_tensor_registry(|registry| registry.lifetimes.len());
        tensor_result(ctx_ref, value as SpectraHostValue)
    }
}

extern "C" fn std_tensor_stats_released_lifetimes(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let value = with_tensor_registry(|registry| registry.released_lifetime_count());
        tensor_result(ctx_ref, value as SpectraHostValue)
    }
}

extern "C" fn std_tensor_stats_allocation_sites(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let value = with_tensor_registry(|registry| registry.allocation_site_count());
        tensor_result(ctx_ref, value as SpectraHostValue)
    }
}

extern "C" fn std_tensor_stats_reuse_rate_per_mille(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let value = with_tensor_registry(|registry| registry.reuse_rate_per_mille());
        tensor_result(ctx_ref, value as SpectraHostValue)
    }
}

extern "C" fn std_tensor_memory_report(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let report = with_tensor_registry(|registry| registry.memory_report_json());
        let ptr = alloc_spectra_string(&report);
        if ptr == 0 {
            return HOST_STATUS_INTERNAL_ERROR;
        }
        tensor_result(ctx_ref, ptr)
    }
}

extern "C" fn std_tensor_reset_stats(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        with_tensor_registry(|registry| registry.reset_metrics());
        tensor_optional_result(ctx_ref, 0)
    }
}

fn tensor_metric(
    ctx: *mut SpectraHostCallContext,
    read: impl FnOnce(TensorMetrics) -> usize,
) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = tensor_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let value = with_tensor_registry(|registry| read(registry.metrics)) as SpectraHostValue;
        tensor_result(ctx_ref, value)
    }
}

extern "C" fn std_tensor_free(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = tensor_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match with_tensor_registry(|registry| registry.remove(args[0] as usize)) {
            Ok(()) => tensor_optional_result(ctx_ref, 0),
            Err(code) => code,
        }
    }
}

extern "C" fn std_tensor_free_all(ctx: *mut SpectraHostCallContext) -> i32 {
    let freed = with_tensor_registry(|registry| registry.clear_all());
    if ctx.is_null() {
        return HOST_STATUS_SUCCESS;
    }
    unsafe {
        let ctx_ref = &mut *ctx;
        tensor_result(ctx_ref, freed as SpectraHostValue)
    }
}

fn f64_bits_to_i64_if_needed(value: SpectraHostValue) -> SpectraHostValue {
    let as_float = f64::from_bits(value as u64);
    if as_float.is_finite() && as_float.fract() == 0.0 && as_float.abs() <= i64::MAX as f64 {
        as_float as i64
    } else {
        value
    }
}

