pub fn relu_forward(input: &[f32]) -> Result<Vec<f32>, GpuError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let shader = format!(
        r#"
@group(0) @binding(0) var<storage, read> input_values: array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
    let i = id.x;
    if (i >= {len}u) {{
        return;
    }}
    out[i] = max(input_values[i], 0.0);
}}
"#,
        len = input.len()
    );
    dispatch_one_input(input, input.len(), &shader, [input.len() as u32, 1, 1])
}

fn context() -> Result<&'static Mutex<GpuContext>, GpuError> {
    CONTEXT
        .get_or_init(|| pollster::block_on(create_context()).map(Mutex::new))
        .as_ref()
        .map_err(|err| err.clone())
}

async fn create_context() -> Result<GpuContext, GpuError> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..Default::default()
    });
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
        .ok_or_else(|| GpuError::new(GpuErrorKind::Other, "no GPU adapter available"))?;
    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: Some("spectra-runtime-gpu-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
            },
            None,
        )
        .await
        .map_err(|err| {
            GpuError::new(
                GpuErrorKind::BufferAlloc,
                format!("failed to create GPU device: {err}"),
            )
        })?;
    Ok(GpuContext { device, queue })
}

fn dispatch_one_input(
    input: &[f32],
    output_len: usize,
    shader_source: &str,
    dispatch_size: [u32; 3],
) -> Result<Vec<f32>, GpuError> {
    let mut guard = context()?
        .lock()
        .map_err(|_| GpuError::new(GpuErrorKind::Other, "gpu context poisoned"))?;
    let input_buffer = storage_buffer(&guard.device, bytemuck::cast_slice(input), "input");
    let output_buffer = output_buffer(&guard.device, output_len);
    let readback_buffer = readback_buffer(&guard.device, output_len);
    run_compute(
        &mut guard,
        shader_source,
        &[
            binding(0, &input_buffer, true),
            binding(1, &output_buffer, false),
        ],
        &[&input_buffer, &output_buffer],
        &output_buffer,
        &readback_buffer,
        output_len,
        dispatch_size,
    )
}

fn dispatch_two_inputs(
    left: &[f32],
    right: &[f32],
    output_len: usize,
    shader_source: &str,
    dispatch_size: [u32; 3],
) -> Result<Vec<f32>, GpuError> {
    let mut guard = context()?
        .lock()
        .map_err(|_| GpuError::new(GpuErrorKind::Other, "gpu context poisoned"))?;
    let left_buffer = storage_buffer(&guard.device, bytemuck::cast_slice(left), "left");
    let right_buffer = storage_buffer(&guard.device, bytemuck::cast_slice(right), "right");
    let output_buffer = output_buffer(&guard.device, output_len);
    let readback_buffer = readback_buffer(&guard.device, output_len);
    run_compute(
        &mut guard,
        shader_source,
        &[
            binding(0, &left_buffer, true),
            binding(1, &right_buffer, true),
            binding(2, &output_buffer, false),
        ],
        &[&left_buffer, &right_buffer, &output_buffer],
        &output_buffer,
        &readback_buffer,
        output_len,
        dispatch_size,
    )
}

fn dispatch_three_inputs(
    first: &[f32],
    second: &[f32],
    third: &[f32],
    output_len: usize,
    shader_source: &str,
    dispatch_size: [u32; 3],
) -> Result<Vec<f32>, GpuError> {
    let mut guard = context()?
        .lock()
        .map_err(|_| GpuError::new(GpuErrorKind::Other, "gpu context poisoned"))?;
    let first_buffer = storage_buffer(&guard.device, bytemuck::cast_slice(first), "first");
    let second_buffer = storage_buffer(&guard.device, bytemuck::cast_slice(second), "second");
    let third_buffer = storage_buffer(&guard.device, bytemuck::cast_slice(third), "third");
    let output_buffer = output_buffer(&guard.device, output_len);
    let readback_buffer = readback_buffer(&guard.device, output_len);
    run_compute(
        &mut guard,
        shader_source,
        &[
            binding(0, &first_buffer, true),
            binding(1, &second_buffer, true),
            binding(2, &third_buffer, true),
            binding(3, &output_buffer, false),
        ],
        &[&first_buffer, &second_buffer, &third_buffer, &output_buffer],
        &output_buffer,
        &readback_buffer,
        output_len,
        dispatch_size,
    )
}

struct BindingSpec<'a> {
    binding: u32,
    buffer: &'a wgpu::Buffer,
    readonly: bool,
}

fn binding(binding: u32, buffer: &wgpu::Buffer, readonly: bool) -> BindingSpec<'_> {
    BindingSpec {
        binding,
        buffer,
        readonly,
    }
}

fn storage_buffer(device: &wgpu::Device, bytes: &[u8], label: &str) -> wgpu::Buffer {
    use wgpu::util::DeviceExt;
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytes,
        usage: wgpu::BufferUsages::STORAGE,
    })
}

fn output_buffer(device: &wgpu::Device, len: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("output"),
        size: byte_len(len),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

fn readback_buffer(device: &wgpu::Device, len: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: byte_len(len),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn byte_len(len: usize) -> u64 {
    (len * std::mem::size_of::<f32>()) as u64
}

/// Shared construction of a storage-buffer bind group (+ its layout)
/// from `(binding, buffer, readonly)` specs. Used by the no-readback
/// dispatchers; the layout is returned alongside because it must stay
/// alive through pipeline creation.
fn create_storage_bind_group(
    device: &wgpu::Device,
    label: &str,
    bindings: &[(u32, &wgpu::Buffer, bool)],
) -> (wgpu::BindGroupLayout, wgpu::BindGroup) {
    let layout_entries = bindings
        .iter()
        .map(|(binding, _buf, readonly)| wgpu::BindGroupLayoutEntry {
            binding: *binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: *readonly,
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect::<Vec<_>>();
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(&format!("{label}-bind-layout")),
        entries: &layout_entries,
    });
    let entries = bindings
        .iter()
        .map(|(binding, buf, _readonly)| wgpu::BindGroupEntry {
            binding: *binding,
            resource: buf.as_entire_binding(),
        })
        .collect::<Vec<_>>();
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(&format!("{label}-bind-group")),
        layout: &bind_group_layout,
        entries: &entries,
    });
    (bind_group_layout, bind_group)
}

fn run_compute(
    ctx: &mut GpuContext,
    shader_source: &str,
    bindings: &[BindingSpec<'_>],
    _keep_alive: &[&wgpu::Buffer],
    output: &wgpu::Buffer,
    readback: &wgpu::Buffer,
    output_len: usize,
    dispatch_size: [u32; 3],
) -> Result<Vec<f32>, GpuError> {
    let shader = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("spectra-runtime-gpu-shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(shader_source)),
        });
    let bind_group_layout_entries = bindings
        .iter()
        .map(|entry| wgpu::BindGroupLayoutEntry {
            binding: entry.binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: entry.readonly,
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect::<Vec<_>>();
    let bind_group_layout = ctx
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("spectra-runtime-gpu-bind-layout"),
            entries: &bind_group_layout_entries,
        });
    let bind_group_entries = bindings
        .iter()
        .map(|entry| wgpu::BindGroupEntry {
            binding: entry.binding,
            resource: entry.buffer.as_entire_binding(),
        })
        .collect::<Vec<_>>();
    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("spectra-runtime-gpu-bind-group"),
        layout: &bind_group_layout,
        entries: &bind_group_entries,
    });
    let pipeline_layout = ctx
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("spectra-runtime-gpu-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
    let pipeline = ctx
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("spectra-runtime-gpu-pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: "main",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        });
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("spectra-runtime-gpu-encoder"),
        });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("spectra-runtime-gpu-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(
            dispatch_size[0].div_ceil(64).max(1),
            dispatch_size[1].max(1),
            dispatch_size[2].max(1),
        );
    }
    encoder.copy_buffer_to_buffer(output, 0, readback, 0, byte_len(output_len));
    ctx.queue.submit(Some(encoder.finish()));
    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    ctx.device.poll(wgpu::Maintain::Wait);
    receiver
        .recv()
        .map_err(|_| GpuError::new(GpuErrorKind::Readback, "gpu readback callback failed"))?
        .map_err(|err| {
            GpuError::new(
                GpuErrorKind::Readback,
                format!("gpu readback failed: {err:?}"),
            )
        })?;
    let mapped = slice.get_mapped_range();
    let values = bytemuck::cast_slice(&mapped).to_vec();
    drop(mapped);
    readback.unmap();
    Ok(values)
}

/// R-3052 full: same pipeline construction as `run_compute` but skips
/// the copy-to-readback + map_async step. The output stays on device
/// for the next op in the chain. The caller owns the output buffer.
#[allow(clippy::too_many_arguments)]
fn run_compute_no_readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    shader_source: &str,
    bindings: &[(u32, &wgpu::Buffer, bool)],
    dispatch_size: [u32; 3],
) -> Result<(), GpuError> {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("spectra-runtime-gpu-device-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(shader_source)),
    });
    let (bind_group_layout, bind_group) =
        create_storage_bind_group(device, "spectra-runtime-gpu-device", bindings);
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("spectra-runtime-gpu-device-pipeline-layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("spectra-runtime-gpu-device-pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: "main",
        compilation_options: wgpu::PipelineCompilationOptions::default(),
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("spectra-runtime-gpu-device-encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("spectra-runtime-gpu-device-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(
            dispatch_size[0].div_ceil(64).max(1),
            dispatch_size[1].max(1),
            dispatch_size[2].max(1),
        );
    }
    queue.submit(Some(encoder.finish()));
    Ok(())
}

/// Two-stage compute submission sharing one bind group across two entry
/// points of the same shader module. Used by the parallel sum
/// reduction: stage one writes per-workgroup partial sums, stage two
/// folds them into a single scalar. Dispatch sizes are EXACT workgroup
/// counts (unlike `run_compute*`, which take element counts). Both
/// dispatches are encoded in a single compute pass — WebGPU orders
/// dispatches within a pass and makes prior storage writes visible to
/// later dispatches — so there is exactly one submit and the scalar
/// stays on device (no intermediate readback).
fn run_compute_two_stage_no_readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    shader_source: &str,
    bindings: &[(u32, &wgpu::Buffer, bool)],
    stage_one: (&str, [u32; 3]),
    stage_two: (&str, [u32; 3]),
) -> Result<(), GpuError> {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("spectra-runtime-gpu-device-two-stage-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(shader_source)),
    });
    let (bind_group_layout, bind_group) =
        create_storage_bind_group(device, "spectra-runtime-gpu-device-two-stage", bindings);
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("spectra-runtime-gpu-device-two-stage-pipeline-layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });
    let make_pipeline = |entry_point: &str| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("spectra-runtime-gpu-device-two-stage-pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point,
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        })
    };
    let pipeline_one = make_pipeline(stage_one.0);
    let pipeline_two = make_pipeline(stage_two.0);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("spectra-runtime-gpu-device-two-stage-encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("spectra-runtime-gpu-device-two-stage-pass"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_pipeline(&pipeline_one);
        pass.dispatch_workgroups(stage_one.1[0].max(1), stage_one.1[1].max(1), stage_one.1[2].max(1));
        pass.set_pipeline(&pipeline_two);
        pass.dispatch_workgroups(stage_two.1[0].max(1), stage_two.1[1].max(1), stage_two.1[2].max(1));
    }
    queue.submit(Some(encoder.finish()));
    Ok(())
}

/// Host-materializing parallel tree reduction used by
/// `gpu_core_kernels::sum`. Runs both reduction stages in a single
/// compute pass and reads back only the final 1-f32 buffer.
pub fn dispatch_tree_reduction(input: &[f32]) -> Result<Vec<f32>, GpuError> {
    if input.is_empty() {
        return Err(GpuError::new(
            GpuErrorKind::ShapeMismatch,
            "gpu reduction requires at least one element",
        ));
    }
    let guard = context()?
        .lock()
        .map_err(|_| GpuError::new(GpuErrorKind::Other, "gpu context poisoned"))?;
    let (workgroups, total_threads, partials_count) = reduction_plan(input.len());
    let shader_source = reduction_shader(input.len(), total_threads, partials_count);
    let input_buffer = storage_buffer(
        &guard.device,
        bytemuck::cast_slice(input),
        "spectra-reduce-input",
    );
    let partials_buffer = output_buffer(&guard.device, partials_count as usize);
    let out_buffer = output_buffer(&guard.device, 1);
    let readback = readback_buffer(&guard.device, 1);
    let shader = guard.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("spectra-runtime-gpu-reduce-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(shader_source.as_str())),
    });
    let local_bindings = [
        (0u32, &input_buffer, true),
        (1u32, &partials_buffer, false),
        (2u32, &out_buffer, false),
    ];
    let (bind_group_layout, bind_group) =
        create_storage_bind_group(&guard.device, "spectra-runtime-gpu-reduce", &local_bindings);
    let pipeline_layout = guard.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("spectra-runtime-gpu-reduce-pipeline-layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });
    let reduce_pipeline = guard.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("spectra-runtime-gpu-reduce-pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: "reduce",
        compilation_options: wgpu::PipelineCompilationOptions::default(),
    });
    let final_pipeline = guard.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("spectra-runtime-gpu-reduce-final-pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: "final_reduce",
        compilation_options: wgpu::PipelineCompilationOptions::default(),
    });
    let mut encoder = guard.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("spectra-runtime-gpu-reduce-encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("spectra-runtime-gpu-reduce-pass"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_pipeline(&reduce_pipeline);
        pass.dispatch_workgroups(workgroups.max(1), 1, 1);
        pass.set_pipeline(&final_pipeline);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&out_buffer, 0, &readback, 0, byte_len(1));
    guard.queue.submit(Some(encoder.finish()));
    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    guard.device.poll(wgpu::Maintain::Wait);
    receiver
        .recv()
        .map_err(|_| GpuError::new(GpuErrorKind::Readback, "gpu readback callback failed"))?
        .map_err(|err| {
            GpuError::new(
                GpuErrorKind::Readback,
                format!("gpu readback failed: {err:?}"),
            )
        })?;
    let mapped = slice.get_mapped_range();
    let values = bytemuck::cast_slice(&mapped).to_vec();
    drop(mapped);
    readback.unmap();
    Ok(values)
}
