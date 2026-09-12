use super::*;
pub(crate) fn ml_classification_loss(
    ctx: *mut SpectraHostCallContext,
    op: AutogradOp,
    from_logits: bool,
) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((shape, scores, requires_grad)) = ml_tensor_float_data(args[0] as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(targets) = ml_tensor_int_data(args[1] as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if shape.len() != 2 || targets.len() != shape[0] {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let (batch, classes) = (shape[0], shape[1]);
        let mut loss = 0.0;
        let mut grad = vec![0.0; scores.len()];
        for row in 0..batch {
            let target = targets[row] as usize;
            if target >= classes {
                return HOST_STATUS_INVALID_ARGUMENT;
            }
            if from_logits {
                let row_scores = &scores[row * classes..row * classes + classes];
                let max_score = row_scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let denom = row_scores
                    .iter()
                    .map(|v| (v - max_score).exp())
                    .sum::<f64>();
                for col in 0..classes {
                    let prob = (row_scores[col] - max_score).exp() / denom;
                    grad[row * classes + col] = prob;
                }
                loss -= (grad[row * classes + target]).ln();
                grad[row * classes + target] -= 1.0;
            } else {
                loss -= scores[row * classes + target];
                grad[row * classes + target] = -1.0;
            }
        }
        loss /= batch as f64;
        let creator = (requires_grad && tensor_is_grad_enabled()).then(|| AutogradNode {
            op,
            parents: vec![args[0] as usize],
            input_shape: shape,
            left_shape: Vec::new(),
            right_shape: Vec::new(),
            input: Vec::new(),
            output: grad,
            left: scores,
            right: Vec::new(),
            aux: vec![batch, classes],
            #[cfg(feature = "gpu")]
            device_aux: None,
        });
        ml_loss_tensor(
            ctx_ref,
            loss,
            requires_grad && tensor_is_grad_enabled(),
            creator,
        )
    }
}

pub(crate) extern "C" fn std_ml_cross_entropy_loss(ctx: *mut SpectraHostCallContext) -> i32 {
    ml_classification_loss(ctx, AutogradOp::MlCrossEntropy, true)
}

pub(crate) extern "C" fn std_ml_nll_loss(ctx: *mut SpectraHostCallContext) -> i32 {
    ml_classification_loss(ctx, AutogradOp::MlNll, false)
}

pub(crate) fn ml_optimizer_update(
    param_handle: usize,
    update: impl Fn(f64, f64, usize) -> f64,
) -> bool {
    with_tensor_registry(|registry| {
        let Some(param) = registry.get_mut(param_handle) else {
            return false;
        };
        if param.dtype != TensorDType::Float {
            return false;
        }
        let Some(grad) = param.grad.clone() else {
            return false;
        };
        let mut values = tensor_values_as_f64(param);
        if values.len() != grad.len() {
            return false;
        }
        for (idx, value) in values.iter_mut().enumerate() {
            *value = update(*value, grad[idx], idx);
        }
        param.storage = Arc::new(f64_values_to_host(&values));
        param.offset = 0;
        param.layout = TensorLayout::Contiguous;
        param.strides = tensor_strides(&param.shape);
        param.grad = None;
        true
    })
}

pub(crate) extern "C" fn std_ml_sgd_step(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let lr = f64::from_bits(args[1] as u64);
        if !lr.is_finite() || lr < 0.0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        #[cfg(feature = "gpu")]
        {
            let device_ok = with_tensor_registry(|registry| {
                let handle = args[0] as usize;
                let Some(mut boxed) = registry.take(handle) else {
                    return None;
                };
                let param = boxed.as_mut();
                let param_buf = match param.device_storage.remove(&crate::gpu::PoolDevice::Wgpu) {
                    Some(buf) => buf.clone(),
                    None => {
                        let _ = registry.put(handle, boxed);
                        return Some(false);
                    }
                };
                let grad_buf = match param.device_grad.remove(&crate::gpu::PoolDevice::Wgpu) {
                    Some(buf) => buf,
                    None => {
                        let _ = registry.put(handle, boxed);
                        return Some(false);
                    }
                };
                if param_buf.elements != grad_buf.elements || param.dtype != TensorDType::Float {
                    param
                        .device_storage
                        .insert(crate::gpu::PoolDevice::Wgpu, param_buf);
                    registry.device_arena.release(grad_buf);
                    let _ = registry.put(handle, boxed);
                    return Some(false);
                }
                let result = crate::gpu::with_device_queue(|device, queue| {
                    let out_buf = registry.device_arena.acquire(
                        crate::gpu::PoolDevice::Wgpu,
                        crate::gpu::PoolDType::Float,
                        param_buf.elements,
                        device,
                    );
                    let update_result = crate::gpu::sgd_update_device(
                        &param_buf, &grad_buf, lr as f32, &out_buf, device, queue,
                    );
                    (update_result, out_buf)
                });
                registry.device_arena.release(grad_buf);
                match result {
                    Ok((Ok(()), out_buf)) => {
                        registry.device_arena.release(param_buf);
                        param
                            .device_storage
                            .insert(crate::gpu::PoolDevice::Wgpu, out_buf);
                        param.grad = None;
                        registry.note_gpu_kernel();
                        let _ = registry.put(handle, boxed);
                        Some(true)
                    }
                    Ok((Err(err), out_buf)) => {
                        registry.device_arena.release(out_buf);
                        param
                            .device_storage
                            .insert(crate::gpu::PoolDevice::Wgpu, param_buf);
                        registry.note_gpu_error(err.kind);
                        registry.note_cpu_fallback();
                        let _ = registry.put(handle, boxed);
                        Some(false)
                    }
                    Err(err) => {
                        param
                            .device_storage
                            .insert(crate::gpu::PoolDevice::Wgpu, param_buf);
                        registry.note_gpu_error(err.kind);
                        registry.note_cpu_fallback();
                        let _ = registry.put(handle, boxed);
                        Some(false)
                    }
                }
            });
            match device_ok {
                Some(true) => return tensor_optional_result(ctx_ref, 0),
                None => return HOST_STATUS_NOT_FOUND,
                Some(false) => {}
            }
        }
        if !ml_optimizer_update(args[0] as usize, |value, grad, _| value - lr * grad) {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_ml_sgd_momentum_step(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 4) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (param_h, velocity_h) = (args[0] as usize, args[1] as usize);
        let (lr, momentum) = (
            f64::from_bits(args[2] as u64),
            f64::from_bits(args[3] as u64),
        );
        let Some((_shape, mut velocity, _)) = ml_tensor_float_data(velocity_h) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let ok = with_tensor_registry(|registry| {
            let Some(param) = registry.get_mut(param_h) else {
                return false;
            };
            let Some(grad) = param.grad.clone() else {
                return false;
            };
            let mut values = tensor_values_as_f64(param);
            if values.len() != grad.len() || velocity.len() != grad.len() {
                return false;
            }
            for idx in 0..values.len() {
                velocity[idx] = momentum * velocity[idx] + grad[idx];
                values[idx] -= lr * velocity[idx];
            }
            param.storage = Arc::new(f64_values_to_host(&values));
            param.offset = 0;
            param.layout = TensorLayout::Contiguous;
            param.strides = tensor_strides(&param.shape);
            param.grad = None;
            true
        });
        if !ok || !ml_store_float_tensor(velocity_h, velocity) {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_ml_adam_step(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 8) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (param_h, m_h, v_h) = (args[0] as usize, args[1] as usize, args[2] as usize);
        let lr = f64::from_bits(args[3] as u64);
        let beta1 = f64::from_bits(args[4] as u64);
        let beta2 = f64::from_bits(args[5] as u64);
        let eps = f64::from_bits(args[6] as u64);
        let step = args[7].max(1) as i32;
        let Some((_m_shape, mut m, _)) = ml_tensor_float_data(m_h) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((_v_shape, mut v, _)) = ml_tensor_float_data(v_h) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let ok = with_tensor_registry(|registry| {
            let Some(param) = registry.get_mut(param_h) else {
                return false;
            };
            let Some(grad) = param.grad.clone() else {
                return false;
            };
            let mut values = tensor_values_as_f64(param);
            if values.len() != grad.len() || m.len() != grad.len() || v.len() != grad.len() {
                return false;
            }
            for idx in 0..values.len() {
                m[idx] = beta1 * m[idx] + (1.0 - beta1) * grad[idx];
                v[idx] = beta2 * v[idx] + (1.0 - beta2) * grad[idx] * grad[idx];
                let m_hat = m[idx] / (1.0 - beta1.powi(step));
                let v_hat = v[idx] / (1.0 - beta2.powi(step));
                values[idx] -= lr * m_hat / (v_hat.sqrt() + eps);
            }
            param.storage = Arc::new(f64_values_to_host(&values));
            param.offset = 0;
            param.layout = TensorLayout::Contiguous;
            param.strides = tensor_strides(&param.shape);
            param.grad = None;
            true
        });
        if !ok || !ml_store_float_tensor(m_h, m) || !ml_store_float_tensor(v_h, v) {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_ml_adamw_step(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 9) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (param_h, m_h, v_h) = (args[0] as usize, args[1] as usize, args[2] as usize);
        let lr = f64::from_bits(args[3] as u64);
        let beta1 = f64::from_bits(args[4] as u64);
        let beta2 = f64::from_bits(args[5] as u64);
        let eps = f64::from_bits(args[6] as u64);
        let step = args[7].max(1) as i32;
        let weight_decay = f64::from_bits(args[8] as u64);
        let Some((_m_shape, mut m, _)) = ml_tensor_float_data(m_h) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some((_v_shape, mut v, _)) = ml_tensor_float_data(v_h) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let ok = with_tensor_registry(|registry| {
            let Some(param) = registry.get_mut(param_h) else {
                return false;
            };
            let Some(grad) = param.grad.clone() else {
                return false;
            };
            let mut values = tensor_values_as_f64(param);
            if values.len() != grad.len() || m.len() != grad.len() || v.len() != grad.len() {
                return false;
            }
            for idx in 0..values.len() {
                m[idx] = beta1 * m[idx] + (1.0 - beta1) * grad[idx];
                v[idx] = beta2 * v[idx] + (1.0 - beta2) * grad[idx] * grad[idx];
                let m_hat = m[idx] / (1.0 - beta1.powi(step));
                let v_hat = v[idx] / (1.0 - beta2.powi(step));
                values[idx] =
                    values[idx] * (1.0 - lr * weight_decay) - lr * m_hat / (v_hat.sqrt() + eps);
            }
            param.storage = Arc::new(f64_values_to_host(&values));
            param.offset = 0;
            param.layout = TensorLayout::Contiguous;
            param.strides = tensor_strides(&param.shape);
            param.grad = None;
            true
        });
        if !ok || !ml_store_float_tensor(m_h, m) || !ml_store_float_tensor(v_h, v) {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

pub(crate) extern "C" fn std_ml_exp_lr(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let base = f64::from_bits(args[0] as u64);
        let gamma = f64::from_bits(args[1] as u64);
        let step = args[2] as i32;
        tensor_result(
            ctx_ref,
            (base * gamma.powi(step)).to_bits() as SpectraHostValue,
        )
    }
}

pub(crate) extern "C" fn std_ml_unscale_grad(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let scale = f64::from_bits(args[1] as u64);
        if !scale.is_finite() || scale == 0.0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let ok = with_tensor_registry(|registry| {
            let Some(tensor) = registry.get_mut(args[0] as usize) else {
                return false;
            };
            let Some(grad) = tensor.grad.as_mut() else {
                return false;
            };
            for value in grad.iter_mut() {
                *value /= scale;
            }
            true
        });
        if !ok {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}
