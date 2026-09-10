use super::*;
/// R-3052 full: incoming gradient to a parent during backward. Either
/// a host `Vec<f64>` (the CPU path or the first seed from a host loss)
/// or a device `DeviceBuffer` (the GPU residency path).
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) enum ParentGrad {
    Host(Vec<f64>),
    #[cfg(feature = "gpu")]
    Device(crate::gpu::DeviceBuffer),
}

/// R-3052 full: accumulate a parent grad into the tensor. If the
/// tensor is device-resident and the incoming grad is a device buffer,
/// add into the tensor's `device_grad`; otherwise fall back to the
/// host `Vec<f64>` accumulation.
#[cfg(feature = "gpu")]
pub(crate) fn accumulate_parent_grad(
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
pub(crate) fn accumulate_parent_grad(
    _registry: &mut TensorRegistry,
    tensor: &mut StdTensor,
    grad: ParentGrad,
) -> bool {
    match grad {
        ParentGrad::Host(values) => accumulate_tensor_grad(tensor, &values),
    }
}

/// Loud-CPU-fallback for device gradients: when no GPU backward kernel
/// handled the op, transfer the incoming device gradient back to the host
/// and run the exact same CPU backward math used by the pure-CPU tape path
/// (`autograd_parent_grads_cpu`). This replaces the previous silent stop,
/// which emitted all-zero parent gradients for any op whose GPU backward
/// was missing. The cached host copies on `AutogradNode` (`left`, `right`,
/// `input`, `output`) are populated even for device-resident tensors, so
/// the CPU backward needs nothing else. If even the readback fails, an
/// explicit host-call error status is propagated instead of zeros.
#[cfg(feature = "gpu")]
pub(crate) fn autograd_parent_grads_device_readback(
    node: &AutogradNode,
    grad_buf: &crate::gpu::DeviceBuffer,
    registry: &mut TensorRegistry,
) -> Result<Vec<(usize, ParentGrad)>, i32> {
    use crate::gpu;

    registry.note_cpu_fallback();
    if !gpu::is_available() {
        return Err(HOST_STATUS_INTERNAL_ERROR);
    }
    let readback =
        gpu::with_device_queue(|device, queue| gpu::readback_device_f32(grad_buf, device, queue));
    let f32_values = match readback {
        Ok(Ok(values)) => values,
        Ok(Err(err)) | Err(err) => {
            registry.note_gpu_error(err.kind);
            return Err(HOST_STATUS_INTERNAL_ERROR);
        }
    };
    if f32_values.is_empty() {
        // Nothing flowed into this node; the correct accumulation is empty.
        return Ok(Vec::new());
    }
    let host_values: Vec<f64> = f32_values.iter().map(|v| *v as f64).collect();
    autograd_parent_grads_cpu(node, &ParentGrad::Host(host_values))
        .ok_or(HOST_STATUS_INTERNAL_ERROR)
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
pub(crate) fn autograd_parent_grads_gpu_dispatch(
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
