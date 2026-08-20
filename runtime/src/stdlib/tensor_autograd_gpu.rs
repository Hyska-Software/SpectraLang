/// R-3052 full: incoming gradient to a parent during backward. Either
/// a host `Vec<f64>` (the CPU path or the first seed from a host loss)
/// or a device `DeviceBuffer` (the GPU residency path).
#[derive(Clone)]
#[allow(dead_code)]
enum ParentGrad {
    Host(Vec<f64>),
    #[cfg(feature = "gpu")]
    Device(crate::gpu::DeviceBuffer),
}

/// R-3052 full: accumulate a parent grad into the tensor. If the
/// tensor is device-resident and the incoming grad is a device buffer,
/// add into the tensor's `device_grad`; otherwise fall back to the
/// host `Vec<f64>` accumulation.
#[cfg(feature = "gpu")]
fn accumulate_parent_grad(
    registry: &mut TensorRegistry,
    tensor: &mut StdTensor,
    grad: ParentGrad,
) -> bool {
    match grad {
        ParentGrad::Host(values) => {
            if !accumulate_tensor_grad(tensor, &values) {
                return false;
            }
            if tensor.dtype == TensorDType::Float
                && tensor
                    .device_storage
                    .contains_key(&crate::gpu::PoolDevice::Wgpu)
            {
                let f32_values: Vec<f32> = values.iter().map(|v| *v as f32).collect();
                let upload = crate::gpu::with_device_queue(|device, queue| {
                    let buf = registry.device_arena.acquire(
                        crate::gpu::PoolDevice::Wgpu,
                        crate::gpu::PoolDType::Float,
                        f32_values.len(),
                        device,
                    );
                    queue.write_buffer(&buf.buffer, 0, bytemuck::cast_slice(&f32_values));
                    queue.submit(None);
                    buf
                });
                if let Ok(incoming) = upload {
                    if let Some(existing) = tensor
                        .device_grad
                        .get(&crate::gpu::PoolDevice::Wgpu)
                        .cloned()
                    {
                        let add_result = crate::gpu::with_device_queue(|device, queue| {
                            crate::gpu::add_into_device(&existing, &incoming, device, queue)
                        });
                        registry.device_arena.release(incoming);
                        if let Err(err) = add_result.and_then(|r| r) {
                            registry.note_gpu_error(err.kind);
                            registry.note_cpu_fallback();
                        } else {
                            registry.note_gpu_kernel();
                        }
                    } else {
                        tensor
                            .device_grad
                            .insert(crate::gpu::PoolDevice::Wgpu, incoming);
                    }
                }
            }
            true
        }
        ParentGrad::Device(incoming) => {
            if tensor.dtype != TensorDType::Float
                || !tensor
                    .device_storage
                    .contains_key(&crate::gpu::PoolDevice::Wgpu)
            {
                // Parent is not device-resident; fall back to host.
                let host = crate::gpu::with_device_queue(|device, queue| {
                    crate::gpu::readback_device_f32(&incoming, device, queue)
                });
                registry.device_arena.release(incoming);
                match host {
                    Ok(Ok(values)) => {
                        let f64_values: Vec<f64> = values.into_iter().map(|v| v as f64).collect();
                        accumulate_tensor_grad(tensor, &f64_values)
                    }
                    Ok(Err(err)) | Err(err) => {
                        registry.note_gpu_error(err.kind);
                        registry.note_cpu_fallback();
                        false
                    }
                }
            } else if tensor.len() != incoming.elements {
                registry.device_arena.release(incoming);
                false
            } else if let Some(existing) = tensor
                .device_grad
                .get(&crate::gpu::PoolDevice::Wgpu)
                .cloned()
            {
                let add_result = crate::gpu::with_device_queue(|device, queue| {
                    crate::gpu::add_into_device(&existing, &incoming, device, queue)
                });
                registry.device_arena.release(incoming);
                match add_result {
                    Ok(Ok(())) => {
                        registry.note_gpu_kernel();
                        true
                    }
                    Ok(Err(err)) | Err(err) => {
                        registry.note_gpu_error(err.kind);
                        registry.note_cpu_fallback();
                        false
                    }
                }
            } else {
                tensor
                    .device_grad
                    .insert(crate::gpu::PoolDevice::Wgpu, incoming);
                true
            }
        }
    }
}

#[cfg(not(feature = "gpu"))]
fn accumulate_parent_grad(
    _registry: &mut TensorRegistry,
    tensor: &mut StdTensor,
    grad: ParentGrad,
) -> bool {
    match grad {
        ParentGrad::Host(values) => accumulate_tensor_grad(tensor, &values),
    }
}

/// R-3080: GPU-accelerated backward for autograd. Returns `None` to
/// signal that the CPU path should be used. GPU is only attempted when
/// the parents are on the Wgpu device and the op has a WGSL backward
/// kernel in `runtime/src/gpu.rs`.
///
/// R-3052 full: when the parents have a Wgpu `device_storage` and the
/// incoming `grad` is a `ParentGrad::Device`, the dispatch consumes
/// the device buffer and returns device buffers for each parent. The
/// caller stores them in the parents' `device_grad` slots via
/// `accumulate_parent_grad`. When the incoming grad is host (the seed
/// from a host loss or a CPU step), the dispatch falls back to the
/// readback path: it uploads the host grad to a 1-element or N-element
/// device buffer and then runs the device backward.
///
/// Precision: the GPU module is f32, while the autograd graph is f64.
/// Conversion is f64 → f32 → GPU → f32 → f64. The result is
/// numerically equivalent to the CPU path within R-1503 tolerance
/// for the production benchmarks; callers should not rely on bitwise
/// equality between the two paths.
#[cfg(feature = "gpu")]
fn autograd_parent_grads_gpu_dispatch(
    node: &AutogradNode,
    grad: &ParentGrad,
    registry: &mut TensorRegistry,
) -> Option<Vec<(usize, ParentGrad)>> {
    use crate::gpu;

    if !gpu::is_available() {
        return None;
    }

    let expected_grad_len = match node.op {
        AutogradOp::Matmul => node.left_shape.first()? * node.right_shape.get(1)?,
        AutogradOp::MlLinear => node.aux.first()? * node.aux.get(2)?,
        AutogradOp::Relu => node.input.len(),
        AutogradOp::MlMse => 1,
        _ => return None,
    };
    if expected_grad_len == 0 {
        return None;
    }

    let uploaded_grad = match grad {
        ParentGrad::Host(values) => {
            if values.len() != expected_grad_len {
                return None;
            }
            let f32_values: Vec<f32> = values.iter().map(|v| *v as f32).collect();
            match gpu::with_device_queue(|device, queue| {
                let buf = registry.device_arena.acquire(
                    crate::gpu::PoolDevice::Wgpu,
                    crate::gpu::PoolDType::Float,
                    f32_values.len(),
                    device,
                );
                queue.write_buffer(&buf.buffer, 0, bytemuck::cast_slice(&f32_values));
                queue.submit(None);
                buf
            }) {
                Ok(buf) => Some(buf),
                Err(err) => {
                    registry.note_gpu_error(err.kind);
                    registry.note_cpu_fallback();
                    return None;
                }
            }
        }
        ParentGrad::Device(buf) => {
            if buf.elements != expected_grad_len {
                return None;
            }
            None
        }
    };
    let grad_buf = match grad {
        ParentGrad::Host(_) => uploaded_grad.as_ref()?,
        ParentGrad::Device(buf) => buf,
    };

    let output = match node.op {
        AutogradOp::Matmul if node.parents.len() == 2 => {
            let (m, k, n) = (node.left_shape[0], node.left_shape[1], node.right_shape[1]);
            let left_buf = registry
                .get(node.parents[0])?
                .device_storage
                .get(&crate::gpu::PoolDevice::Wgpu)?
                .clone();
            let right_buf = registry
                .get(node.parents[1])?
                .device_storage
                .get(&crate::gpu::PoolDevice::Wgpu)?
                .clone();
            let result = gpu::with_device_queue(|device, queue| {
                let out_left = registry.device_arena.acquire(
                    crate::gpu::PoolDevice::Wgpu,
                    crate::gpu::PoolDType::Float,
                    m * k,
                    device,
                );
                let out_right = registry.device_arena.acquire(
                    crate::gpu::PoolDevice::Wgpu,
                    crate::gpu::PoolDType::Float,
                    k * n,
                    device,
                );
                let left_result = gpu::backward_matmul_left_device(
                    grad_buf, &right_buf, m, k, n, &out_left, device, queue,
                );
                let right_result = gpu::backward_matmul_right_device(
                    &left_buf, grad_buf, m, k, n, &out_right, device, queue,
                );
                (left_result, right_result, out_left, out_right)
            });
            match result {
                Ok((Ok(()), Ok(()), out_left, out_right)) => Some(vec![
                    (node.parents[0], ParentGrad::Device(out_left)),
                    (node.parents[1], ParentGrad::Device(out_right)),
                ]),
                Ok((left_result, right_result, out_left, out_right)) => {
                    registry.device_arena.release(out_left);
                    registry.device_arena.release(out_right);
                    if let Err(err) = left_result {
                        registry.note_gpu_error(err.kind);
                    }
                    if let Err(err) = right_result {
                        registry.note_gpu_error(err.kind);
                    }
                    registry.note_cpu_fallback();
                    None
                }
                Err(err) => {
                    registry.note_gpu_error(err.kind);
                    registry.note_cpu_fallback();
                    None
                }
            }
        }
        AutogradOp::MlLinear if node.parents.len() == 3 => {
            let (batch, in_features, out_features) = (node.aux[0], node.aux[1], node.aux[2]);
            let input_buf = registry
                .get(node.parents[0])?
                .device_storage
                .get(&crate::gpu::PoolDevice::Wgpu)?
                .clone();
            let weight_buf = registry
                .get(node.parents[1])?
                .device_storage
                .get(&crate::gpu::PoolDevice::Wgpu)?
                .clone();
            let result = gpu::with_device_queue(|device, queue| {
                let out_input = registry.device_arena.acquire(
                    crate::gpu::PoolDevice::Wgpu,
                    crate::gpu::PoolDType::Float,
                    batch * in_features,
                    device,
                );
                let out_weight = registry.device_arena.acquire(
                    crate::gpu::PoolDevice::Wgpu,
                    crate::gpu::PoolDType::Float,
                    in_features * out_features,
                    device,
                );
                let out_bias = registry.device_arena.acquire(
                    crate::gpu::PoolDevice::Wgpu,
                    crate::gpu::PoolDType::Float,
                    out_features,
                    device,
                );
                let input_result = gpu::linear_grad_input_device(
                    grad_buf,
                    &weight_buf,
                    batch,
                    in_features,
                    out_features,
                    &out_input,
                    device,
                    queue,
                );
                let weight_result = gpu::backward_matmul_right_device(
                    &input_buf,
                    grad_buf,
                    batch,
                    in_features,
                    out_features,
                    &out_weight,
                    device,
                    queue,
                );
                let bias_result = gpu::sum_columns_device(
                    grad_buf,
                    batch,
                    out_features,
                    &out_bias,
                    device,
                    queue,
                );
                (
                    input_result,
                    weight_result,
                    bias_result,
                    out_input,
                    out_weight,
                    out_bias,
                )
            });
            match result {
                Ok((Ok(()), Ok(()), Ok(()), out_input, out_weight, out_bias)) => Some(vec![
                    (node.parents[0], ParentGrad::Device(out_input)),
                    (node.parents[1], ParentGrad::Device(out_weight)),
                    (node.parents[2], ParentGrad::Device(out_bias)),
                ]),
                Ok((input_result, weight_result, bias_result, out_input, out_weight, out_bias)) => {
                    registry.device_arena.release(out_input);
                    registry.device_arena.release(out_weight);
                    registry.device_arena.release(out_bias);
                    for err in [input_result.err(), weight_result.err(), bias_result.err()]
                        .into_iter()
                        .flatten()
                    {
                        registry.note_gpu_error(err.kind);
                    }
                    registry.note_cpu_fallback();
                    None
                }
                Err(err) => {
                    registry.note_gpu_error(err.kind);
                    registry.note_cpu_fallback();
                    None
                }
            }
        }
        AutogradOp::Relu if node.parents.len() == 1 => {
            let input_buf = registry
                .get(node.parents[0])?
                .device_storage
                .get(&crate::gpu::PoolDevice::Wgpu)?
                .clone();
            let result = gpu::with_device_queue(|device, queue| {
                let out = registry.device_arena.acquire(
                    crate::gpu::PoolDevice::Wgpu,
                    crate::gpu::PoolDType::Float,
                    grad_buf.elements,
                    device,
                );
                let relu_result =
                    gpu::backward_relu_device(grad_buf, &input_buf, &out, device, queue);
                (relu_result, out)
            });
            match result {
                Ok((Ok(()), out)) => Some(vec![(node.parents[0], ParentGrad::Device(out))]),
                Ok((Err(err), out)) => {
                    registry.device_arena.release(out);
                    registry.note_gpu_error(err.kind);
                    registry.note_cpu_fallback();
                    None
                }
                Err(err) => {
                    registry.note_gpu_error(err.kind);
                    registry.note_cpu_fallback();
                    None
                }
            }
        }
        AutogradOp::MlMse if node.parents.len() == 1 => {
            let pred_buf = registry
                .get(node.parents[0])?
                .device_storage
                .get(&crate::gpu::PoolDevice::Wgpu)?
                .clone();
            let target_buf = node.device_aux.as_ref()?.clone();
            let result = gpu::with_device_queue(|device, queue| {
                let out = registry.device_arena.acquire(
                    crate::gpu::PoolDevice::Wgpu,
                    crate::gpu::PoolDType::Float,
                    pred_buf.elements,
                    device,
                );
                let mse_result =
                    gpu::mse_backward_device(&pred_buf, &target_buf, grad_buf, &out, device, queue);
                (mse_result, out)
            });
            match result {
                Ok((Ok(()), out)) => Some(vec![(node.parents[0], ParentGrad::Device(out))]),
                Ok((Err(err), out)) => {
                    registry.device_arena.release(out);
                    registry.note_gpu_error(err.kind);
                    registry.note_cpu_fallback();
                    None
                }
                Err(err) => {
                    registry.note_gpu_error(err.kind);
                    registry.note_cpu_fallback();
                    None
                }
            }
        }
        _ => None,
    };

    if let Some(buf) = uploaded_grad {
        registry.device_arena.release(buf);
    }
    if output.is_some() {
        note_gpu_backward_op();
    }
    output
}

#[cfg(all(feature = "gpu", any()))]
fn autograd_parent_grads_gpu_dispatch(
    node: &AutogradNode,
    grad: &ParentGrad,
    registry: &TensorRegistry,
) -> Option<Vec<(usize, ParentGrad)>> {
    use crate::gpu;

    if !gpu::is_available() {
        return None;
    }
    let parents_on_wgpu = |handles: &[usize]| -> bool {
        handles.iter().all(|h| {
            registry
                .get(*h)
                .map(|t| t.device == TensorDevice::Wgpu)
                .unwrap_or(false)
        })
    };
    let parents_have_storage = |handles: &[usize]| -> bool {
        handles.iter().all(|h| {
            registry
                .get(*h)
                .map(|t| t.device_storage.contains_key(&crate::gpu::PoolDevice::Wgpu))
                .unwrap_or(false)
        })
    };
    let residency_applies = parents_on_wgpu(&node.parents) && parents_have_storage(&node.parents);

    // If the incoming grad is host, we need to promote it to a device
    // buffer for the GPU backward. The promotion is a one-time host->device
    // upload (not a "readback" in the hot-path sense).
    let host_grad_f32: Vec<f32> = match grad {
        ParentGrad::Host(values) => values.iter().map(|v| *v as f32).collect(),
        ParentGrad::Device(_) => Vec::new(),
    };
    let host_grad_owned: ParentGrad = ParentGrad::Host(Vec::new());

    let grad_for_kernel: Option<crate::gpu::DeviceBuffer> = match grad {
        ParentGrad::Device(buf) => Some(buf.clone()),
        ParentGrad::Host(values) => {
            // Upload host grad to a device buffer. The host->device upload
            // is the boundary entry to the GPU backward, not a chained op.
            if !residency_applies {
                return None;
            }
            let n = values.len();
            if n == 0 {
                return None;
            }
            let upload = with_tensor_registry(|reg| {
                let f32_values: Vec<f32> = values.iter().map(|v| *v as f32).collect();
                gpu::with_device_queue(|device, queue| {
                    let buf = reg.device_arena.acquire(
                        crate::gpu::PoolDevice::Wgpu,
                        crate::gpu::PoolDType::Float,
                        n,
                        device,
                    );
                    queue.write_buffer(&buf.buffer, 0, bytemuck::cast_slice(&f32_values));
                    queue.submit(None);
                    buf
                })
            });
            match upload {
                Ok(buf) => Some(buf),
                Err(_) => return None,
            }
        }
    };
    let _ = host_grad_owned;
    let grad_f32_local = host_grad_f32; // suppress unused
    let _ = grad_f32_local;

    let grad_buf = grad_for_kernel.as_ref()?;

    let result: Option<Vec<(usize, ParentGrad)>> = match node.op {
        AutogradOp::Matmul if parents_on_wgpu(&node.parents) => {
            let m = node.left_shape.first().copied().unwrap_or(0);
            let k = node.left_shape.get(1).copied().unwrap_or(0);
            let n = node.right_shape.get(1).copied().unwrap_or(0);
            if m == 0 || k == 0 || n == 0 || grad_buf.elements != m * n {
                return None;
            }
            let left_buf = registry
                .get(node.parents[0])?
                .device_storage
                .get(&crate::gpu::PoolDevice::Wgpu)?
                .clone();
            let right_buf = registry
                .get(node.parents[1])?
                .device_storage
                .get(&crate::gpu::PoolDevice::Wgpu)?
                .clone();
            let (gl, gr) = with_tensor_registry(|reg| {
                gpu::with_device_queue(|device, queue| -> Option<(crate::gpu::DeviceBuffer, crate::gpu::DeviceBuffer)> {
                    let out_left = reg.device_arena.acquire(
                        crate::gpu::PoolDevice::Wgpu, crate::gpu::PoolDType::Float, m * k, device);
                    let out_right = reg.device_arena.acquire(
                        crate::gpu::PoolDevice::Wgpu, crate::gpu::PoolDType::Float, k * n, device);
                    let rl = gpu::backward_matmul_left_device(grad_buf, &right_buf, m, k, n, &out_left, device, queue);
                    let rr = gpu::backward_matmul_right_device(&left_buf, grad_buf, m, k, n, &out_right, device, queue);
                    match (rl, rr) {
                        (Ok(()), Ok(())) => Some((out_left, out_right)),
                        _ => None,
                    }
                })
            });
            match gl {
                Ok(Some((gl, gr))) => {
                    note_gpu_backward_op();
                    Some(vec![
                        (node.parents[0], ParentGrad::Device(gl)),
                        (node.parents[1], ParentGrad::Device(gr)),
                    ])
                }
                _ => None,
            }
        }
        AutogradOp::MlLinear if node.parents.len() == 3 && parents_on_wgpu(&node.parents) => {
            let batch = node.aux.first().copied().unwrap_or(0);
            let in_features = node.aux.get(1).copied().unwrap_or(0);
            let out_features = node.aux.get(2).copied().unwrap_or(0);
            if batch == 0
                || in_features == 0
                || out_features == 0
                || grad_buf.elements != batch * out_features
            {
                return None;
            }
            let left_buf = registry
                .get(node.parents[0])?
                .device_storage
                .get(&crate::gpu::PoolDevice::Wgpu)?
                .clone();
            let right_buf = registry
                .get(node.parents[1])?
                .device_storage
                .get(&crate::gpu::PoolDevice::Wgpu)?
                .clone();
            let (gi, gw, gb) = with_tensor_registry(|reg| {
                gpu::with_device_queue(
                    |device,
                     queue|
                     -> Option<(
                        crate::gpu::DeviceBuffer,
                        crate::gpu::DeviceBuffer,
                        crate::gpu::DeviceBuffer,
                    )> {
                        let out_input = reg.device_arena.acquire(
                            crate::gpu::PoolDevice::Wgpu,
                            crate::gpu::PoolDType::Float,
                            batch * in_features,
                            device,
                        );
                        let out_weight = reg.device_arena.acquire(
                            crate::gpu::PoolDevice::Wgpu,
                            crate::gpu::PoolDType::Float,
                            in_features * out_features,
                            device,
                        );
                        let out_bias = reg.device_arena.acquire(
                            crate::gpu::PoolDevice::Wgpu,
                            crate::gpu::PoolDType::Float,
                            out_features,
                            device,
                        );
                        let ri = gpu::backward_matmul_left_device(
                            grad_buf,
                            &right_buf,
                            batch,
                            out_features,
                            in_features,
                            &out_input,
                            device,
                            queue,
                        );
                        let rw = gpu::backward_matmul_right_device(
                            &left_buf,
                            grad_buf,
                            batch,
                            in_features,
                            out_features,
                            &out_weight,
                            device,
                            queue,
                        );
                        let rb = gpu::backward_matmul_right_device(
                            grad_buf,
                            grad_buf,
                            batch,
                            out_features,
                            1,
                            &out_bias,
                            device,
                            queue,
                        );
                        // rb is wrong above; we need a sum-reduction kernel for grad_bias.
                        // For now, fall through to the CPU path for grad_bias.
                        let _ = rb;
                        match (ri, rw) {
                            (Ok(()), Ok(())) => Some((out_input, out_weight, out_bias)),
                            _ => None,
                        }
                    },
                )
            });
            match gi {
                Ok(Some((gi, gw, gb))) => {
                    note_gpu_backward_op();
                    Some(vec![
                        (node.parents[0], ParentGrad::Device(gi)),
                        (node.parents[1], ParentGrad::Device(gw)),
                        (node.parents[2], ParentGrad::Device(gb)),
                    ])
                }
                _ => None,
            }
        }
        AutogradOp::Relu if parents_on_wgpu(&node.parents) => {
            if grad_buf.elements != node.input.len() {
                return None;
            }
            // For relu, output > 0 iff input > 0, so we can use the
            // parent's input device buffer as the gate.
            let input_buf = registry
                .get(node.parents[0])?
                .device_storage
                .get(&crate::gpu::PoolDevice::Wgpu)?
                .clone();
            let out_buf = with_tensor_registry(|reg| {
                gpu::with_device_queue(|device, queue| -> Option<crate::gpu::DeviceBuffer> {
                    let n = grad_buf.elements;
                    let buf = reg.device_arena.acquire(
                        crate::gpu::PoolDevice::Wgpu,
                        crate::gpu::PoolDType::Float,
                        n,
                        device,
                    );
                    let r = gpu::backward_relu_device(grad_buf, &input_buf, &buf, device, queue);
                    match r {
                        Ok(()) => Some(buf),
                        _ => None,
                    }
                })
            });
            match out_buf {
                Ok(Some(buf)) => {
                    note_gpu_backward_op();
                    Some(vec![(node.parents[0], ParentGrad::Device(buf))])
                }
                _ => None,
            }
        }
        AutogradOp::Sigmoid if parents_on_wgpu(&node.parents) => {
            // Sigmoid has no forward GPU kernel (plan: do not add it).
            // Fall back to the CPU path.
            None
        }
        _ => None,
    };
    // Release the temporary grad buffer if we uploaded from host.
    if matches!(grad, ParentGrad::Host(_)) {
        if let Some(buf) = grad_for_kernel {
            with_tensor_registry(|reg| {
                reg.device_arena.release(buf);
            });
        }
    } else {
        // The caller owns the device grad; do not release.
    }
    result
}
