// APPEND-ONLY: gated GPU reduction-op tests (SUM|MEAN|MIN|MAX|ARGMAX).
// Included from gpu.rs under `#[cfg(test)]`; every test self-skips when
// no GPU adapter is present so the suite stays green on CPU-only hosts.

/// Deterministic pseudo-random f32 data in [-1000, 1000].
fn reduce_test_data(n: usize) -> Vec<f32> {
    let mut state: u64 = 0x9E3779B97F4A7C15 ^ (n as u64);
    (0..n)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let unit = ((state >> 40) as f64) / ((1u64 << 24) as f64);
            (unit * 2000.0 - 1000.0) as f32
        })
        .collect()
}

fn cpu_sum(data: &[f32]) -> f64 {
    data.iter().map(|&v| f64::from(v)).sum::<f64>()
}

fn cpu_mean(data: &[f32]) -> f64 {
    cpu_sum(data) / data.len() as f64
}

fn cpu_min(data: &[f32]) -> f32 {
    data.iter().copied().fold(f32::INFINITY, f32::min)
}

fn cpu_max(data: &[f32]) -> f32 {
    data.iter().copied().fold(f32::NEG_INFINITY, f32::max)
}

fn cpu_argmax(data: &[f32]) -> usize {
    let mut best = 0usize;
    for (i, &v) in data.iter().enumerate() {
        if v > data[best] {
            best = i;
        }
    }
    best
}

const REDUCE_SIZES: [usize; 5] = [1, 255, 257, 65_537, 100_000];

fn assert_close(gpu: f32, reference: f64, what: &str) {
    let abs_diff = (f64::from(gpu) - reference).abs();
    let tol = 1e-3 + reference.abs() * 1e-4;
    assert!(
        abs_diff <= tol,
        "{what}: gpu={gpu} cpu={reference} abs_diff={abs_diff} tol={tol}"
    );
}

#[test]
fn gpu_reduce_host_path_matches_cpu_reference_across_sizes() {
    use crate::gpu::{argmax, is_available, max, mean, min, sum};

    if !is_available() {
        eprintln!("skip gpu_reduce_host_path_matches_cpu_reference_across_sizes: no GPU adapter");
        return;
    }
    for n in REDUCE_SIZES {
        let data = reduce_test_data(n);

        let gpu_sum = sum(&data).expect("gpu sum");
        assert_close(gpu_sum, cpu_sum(&data), &format!("sum[{n}]"));

        let gpu_mean = mean(&data).expect("gpu mean");
        assert_close(gpu_mean, cpu_mean(&data), &format!("mean[{n}]"));

        // min/max perform no arithmetic, so results are exact.
        assert_eq!(min(&data).expect("gpu min"), cpu_min(&data), "min[{n}]");
        assert_eq!(max(&data).expect("gpu max"), cpu_max(&data), "max[{n}]");

        let expected_idx = cpu_argmax(&data);
        assert_eq!(
            argmax(&data).expect("gpu argmax"),
            expected_idx,
            "argmax[{n}]"
        );
    }
}

#[test]
fn gpu_reduce_argmax_tie_prefers_lowest_index() {
    use crate::gpu::{argmax, is_available};

    if !is_available() {
        eprintln!("skip gpu_reduce_argmax_tie_prefers_lowest_index: no GPU adapter");
        return;
    }
    let mut data = vec![0.5f32; 10_000];
    // Duplicate maxima at scattered indices across workgroup tiles.
    for idx in [0usize, 511, 512, 4095, 8192, 9_999] {
        data[idx] = 42.0;
    }
    assert_eq!(argmax(&data).expect("gpu argmax tie"), 0);

    let mut data2 = vec![0.25f32; 70_000];
    data2[33_333] = 7.0;
    data2[65_000] = 7.0;
    assert_eq!(argmax(&data2).expect("gpu argmax tie 2"), 33_333);
}

#[test]
fn gpu_reduce_device_paths_match_cpu_reference_across_sizes() {
    use crate::gpu::{
        argmax_device, max_device, mean_device, min_device, readback_scalar_device,
        reduction_plan, with_device_queue, DeviceArena, GpuError, GpuReduceOp, PoolDevice,
        PoolDType,
    };

    if !crate::gpu::is_available() {
        eprintln!("skip gpu_reduce_device_paths_match_cpu_reference_across_sizes: no GPU adapter");
        return;
    }
    for n in REDUCE_SIZES {
        let data = reduce_test_data(n);

        let results =
            with_device_queue(|device, queue| -> Result<Vec<(GpuReduceOp, f32)>, GpuError> {
                let mut arena = DeviceArena::default();
                let input_buf = arena.acquire(PoolDevice::Wgpu, PoolDType::Float, n, device);
                queue.write_buffer(&input_buf.buffer, 0, bytemuck::cast_slice(&data));
                let out_buf = arena.acquire(PoolDevice::Wgpu, PoolDType::Float, 1, device);

                let (_, _, partials) = reduction_plan(n);
                assert!(
                    partials >= 1 && partials <= 4096,
                    "plan for {n}: {partials}"
                );

                let mut got = Vec::new();

                mean_device(&input_buf, &out_buf, device, queue)?;
                got.push((
                    GpuReduceOp::Mean,
                    readback_scalar_device(&out_buf, device, queue)?,
                ));

                min_device(&input_buf, &out_buf, device, queue)?;
                got.push((
                    GpuReduceOp::Min,
                    readback_scalar_device(&out_buf, device, queue)?,
                ));

                max_device(&input_buf, &out_buf, device, queue)?;
                got.push((
                    GpuReduceOp::Max,
                    readback_scalar_device(&out_buf, device, queue)?,
                ));

                argmax_device(&input_buf, &out_buf, device, queue)?;
                got.push((
                    GpuReduceOp::ArgMax,
                    readback_scalar_device(&out_buf, device, queue)?,
                ));

                Ok(got)
            })
            .expect("device queue must be available")
            .expect("device reductions must succeed");

        for (op, value) in results {
            match op {
                GpuReduceOp::Mean => {
                    assert_close(value, cpu_mean(&data), &format!("mean_device[{n}]"))
                }
                GpuReduceOp::Min => assert_eq!(value, cpu_min(&data), "min_device[{n}]"),
                GpuReduceOp::Max => assert_eq!(value, cpu_max(&data), "max_device[{n}]"),
                GpuReduceOp::ArgMax => {
                    let idx = f32::to_bits(value) as usize;
                    assert_eq!(idx, cpu_argmax(&data), "argmax_device[{n}]");
                }
                other => panic!("unexpected op in device test: {other:?}"),
            }
        }
    }
}

#[test]
fn gpu_reduce_device_sum_still_matches_cpu_reference() {
    // Guard against regression of the pre-existing sum_device while the
    // shared core was parametrized.
    use crate::gpu::{
        readback_scalar_device, sum_device, with_device_queue, DeviceArena, PoolDevice,
        PoolDType,
    };

    if !crate::gpu::is_available() {
        eprintln!("skip gpu_reduce_device_sum_still_matches_cpu_reference: no GPU adapter");
        return;
    }
    let data = reduce_test_data(100_000);
    let gpu_sum = with_device_queue(|device, queue| -> Result<f32, crate::gpu::GpuError> {
        let mut arena = DeviceArena::default();
        let input_buf = arena.acquire(PoolDevice::Wgpu, PoolDType::Float, data.len(), device);
        queue.write_buffer(&input_buf.buffer, 0, bytemuck::cast_slice(&data));
        let out_buf = arena.acquire(PoolDevice::Wgpu, PoolDType::Float, 1, device);
        sum_device(&input_buf, &out_buf, device, queue)?;
        readback_scalar_device(&out_buf, device, queue)
    })
    .expect("device queue must be available")
    .expect("sum_device must succeed");
    assert_close(gpu_sum, cpu_sum(&data), "sum_device[100000]");
}
