use super::*;
pub(crate) extern "C" fn std_ml_module_new(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, _args)) = ml_args(ctx, 0) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let handle = with_ml_registry(|registry| {
            registry.modules.insert(MlModule {
                parameters: Vec::new(),
                training: true,
            })
        });
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) fn artifact_tensor_payload(handle: usize, name: &str) -> Option<crate::artifact::TensorPayload> {
    with_tensor_registry(|registry| {
        let tensor = registry.get(handle)?;
        if tensor.device != TensorDevice::Cpu || tensor.shape.is_empty() {
            return None;
        }
        let bytes = match tensor.dtype {
            TensorDType::Int => tensor
                .materialize()
                .into_iter()
                .flat_map(i64::to_le_bytes)
                .collect(),
            TensorDType::Float => tensor
                .materialize()
                .into_iter()
                .flat_map(|raw| (raw as u64).to_le_bytes())
                .collect(),
        };
        Some(crate::artifact::TensorPayload {
            name: name.to_owned(),
            dtype: tensor.dtype.name().to_owned(),
            precision: "f64".to_owned(),
            shape: tensor.shape.clone(),
            layout: "contiguous".to_owned(),
            bytes,
        })
    })
}

pub(crate) fn artifact_tensor_from_payload(payload: &crate::artifact::TensorPayload) -> Result<usize, i32> {
    let values = payload
        .bytes
        .chunks_exact(8)
        .map(|chunk| {
            let bytes: [u8; 8] = chunk.try_into().expect("validated artifact element width");
            match payload.dtype.as_str() {
                "int" => i64::from_le_bytes(bytes),
                "float" => u64::from_le_bytes(bytes) as i64,
                _ => 0,
            }
        })
        .collect::<Vec<_>>();
    let dtype = match payload.dtype.as_str() {
        "int" => TensorDType::Int,
        "float" => TensorDType::Float,
        _ => return Err(HOST_STATUS_INVALID_ARGUMENT),
    };
    tensor_alloc(dtype, payload.shape.clone(), values)
}

pub(crate) fn artifact_data_for_save(artifact: &MlArtifact) -> Option<crate::artifact::ArtifactData> {
    let tensors = artifact
        .tensors
        .iter()
        .map(|(name, handle)| artifact_tensor_payload(*handle, name))
        .collect::<Option<Vec<_>>>()?;
    Some(crate::artifact::ArtifactData {
        name: artifact.name.clone(),
        model_version: artifact.model_version.clone(),
        kind: artifact.kind.clone(),
        metadata: artifact.metadata.clone(),
        tensors,
    })
}

pub(crate) extern "C" fn std_ml_artifact_new(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(name) = read_spectra_string(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(version) = read_spectra_string(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(kind) = read_spectra_string(args[2]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if name.is_empty()
            || version.is_empty()
            || !matches!(kind.as_str(), "checkpoint" | "multi_array")
        {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let handle = with_ml_registry(|registry| {
            registry.artifacts.insert(MlArtifact {
                name,
                model_version: version,
                kind,
                metadata: BTreeMap::new(),
                tensors: HashMap::new(),
            })
        });
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_artifact_set_metadata(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (Some(key), Some(value)) = (read_spectra_string(args[1]), read_spectra_string(args[2]))
        else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if key.is_empty() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let ok = with_ml_registry(|registry| {
            registry
                .artifacts
                .get_mut(&(args[0] as usize))
                .map(|artifact| {
                    artifact.metadata.insert(key, value);
                    true
                })
                .unwrap_or(false)
        });
        if !ok {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_result(ctx_ref, 1)
    }
}

pub(crate) extern "C" fn std_ml_artifact_add_tensor(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(name) = read_spectra_string(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if name.is_empty() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        if artifact_tensor_payload(args[2] as usize, &name).is_none() {
            return HOST_STATUS_NOT_FOUND;
        }
        let ok = with_ml_registry(|registry| {
            let Some(artifact) = registry.artifacts.get_mut(&(args[0] as usize)) else {
                return false;
            };
            if artifact.tensors.contains_key(&name) {
                return false;
            }
            artifact.tensors.insert(name, args[2] as usize);
            true
        });
        if !ok {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        tensor_result(ctx_ref, 1)
    }
}

pub(crate) extern "C" fn std_ml_artifact_save(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = read_spectra_string(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(artifact) =
            with_ml_registry(|registry| registry.artifacts.get(&(args[0] as usize)).cloned())
        else {
            return HOST_STATUS_NOT_FOUND;
        };
        let Some(data) = artifact_data_for_save(&artifact) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match crate::artifact::write_atomic(Path::new(&path), &data) {
            Ok(()) => tensor_result(ctx_ref, 1),
            Err(_) => HOST_STATUS_INVALID_ARGUMENT,
        }
    }
}

pub(crate) extern "C" fn std_ml_artifact_load(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = read_spectra_string(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Ok(data) = crate::artifact::read(Path::new(&path)) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let mut tensors = HashMap::new();
        for payload in &data.tensors {
            let Ok(handle) = artifact_tensor_from_payload(payload) else {
                return HOST_STATUS_INVALID_ARGUMENT;
            };
            tensors.insert(payload.name.clone(), handle);
        }
        let handle = with_ml_registry(|registry| {
            registry.artifacts.insert(MlArtifact {
                name: data.name,
                model_version: data.model_version,
                kind: data.kind,
                metadata: data.metadata,
                tensors,
            })
        });
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_artifact_tensor(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(name) = read_spectra_string(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(handle) = with_ml_registry(|registry| {
            registry
                .artifacts
                .get(&(args[0] as usize))
                .and_then(|artifact| artifact.tensors.get(&name).copied())
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_artifact_metadata(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(key) = read_spectra_string(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(value) = with_ml_registry(|registry| {
            registry
                .artifacts
                .get(&(args[0] as usize))
                .and_then(|artifact| artifact.metadata.get(&key).cloned())
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, alloc_spectra_string(&value))
    }
}

pub(crate) extern "C" fn std_ml_artifact_validate(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = read_spectra_string(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        tensor_result(
            ctx_ref,
            crate::artifact::validate(Path::new(&path)) as SpectraHostValue,
        )
    }
}

pub(crate) extern "C" fn std_ml_artifact_free(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if with_ml_registry(|registry| registry.artifacts.remove(&(args[0] as usize))).is_none() {
            return HOST_STATUS_NOT_FOUND;
        }
        tensor_optional_result(ctx_ref, 0)
    }
}

