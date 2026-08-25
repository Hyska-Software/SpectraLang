use super::*;
pub(crate) extern "C" fn std_ml_dataset_from_tensors(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[2] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let len = args[2] as usize;
        let valid = with_tensor_registry(|registry| {
            registry.get(args[0] as usize).is_some() && registry.get(args[1] as usize).is_some()
        });
        if !valid {
            return HOST_STATUS_NOT_FOUND;
        }
        let handle = with_ml_registry(|registry| {
            registry.datasets.insert(MlDataset {
                features: args[0] as usize,
                labels: args[1] as usize,
                len,
            })
        });
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_dataset_from_csv(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[1] < 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match ml_dataset_from_csv_path(&path, args[1] as usize, args[2] != 0) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_dataset_from_jsonl(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        match ml_dataset_from_jsonl_path(&path) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_dataset_from_npy(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[2] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(features_path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(labels_path) = ml_read_path_arg(args[1]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let features = match ml_parse_npy_f64_1d(&features_path) {
            Ok(values) => values,
            Err(code) => return code,
        };
        let labels = match ml_parse_npy_f64_1d(&labels_path) {
            Ok(values) => values,
            Err(code) => return code,
        };
        match ml_dataset_from_flat_parts(features, labels, args[2] as usize) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_dataset_from_directory(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let features_path = std::path::Path::new(&path).join("features.csv");
        let labels_path = std::path::Path::new(&path).join("labels.csv");
        let (rows, _feature_cols, features) =
            match ml_parse_csv_numeric(features_path.to_string_lossy().as_ref(), true) {
                Ok(parts) => parts,
                Err(code) => return code,
            };
        let (label_rows, _label_cols, labels) =
            match ml_parse_csv_numeric(labels_path.to_string_lossy().as_ref(), true) {
                Ok(parts) => parts,
                Err(code) => return code,
            };
        if label_rows != rows {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        match ml_dataset_from_flat_parts(features, labels, rows) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_dataset_len(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(len) = with_ml_registry(|registry| {
            registry
                .datasets
                .get(&(args[0] as usize))
                .map(|dataset| dataset.len)
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, len as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_dataset_map_features(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let dataset =
            with_ml_registry(|registry| registry.datasets.get(&(args[0] as usize)).copied());
        let Some(dataset) = dataset else {
            return HOST_STATUS_NOT_FOUND;
        };
        let (feature_shape, feature_data, _) = match ml_tensor_float_data(dataset.features) {
            Some(parts) => parts,
            None => return HOST_STATUS_INVALID_ARGUMENT,
        };
        let (_label_shape, label_data, _) = match ml_tensor_float_data(dataset.labels) {
            Some(parts) => parts,
            None => return HOST_STATUS_INVALID_ARGUMENT,
        };
        if feature_shape.is_empty() || feature_shape[0] != dataset.len {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let scale = f64::from_bits(args[1] as u64);
        let bias = f64::from_bits(args[2] as u64);
        if !scale.is_finite() || !bias.is_finite() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let mapped = feature_data
            .into_iter()
            .map(|value| value * scale + bias)
            .collect::<Vec<_>>();
        match ml_dataset_from_flat_parts(mapped, label_data, dataset.len) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_dataset_filter_label_min(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let dataset =
            with_ml_registry(|registry| registry.datasets.get(&(args[0] as usize)).copied());
        let Some(dataset) = dataset else {
            return HOST_STATUS_NOT_FOUND;
        };
        let (feature_shape, feature_data, _) = match ml_tensor_float_data(dataset.features) {
            Some(parts) => parts,
            None => return HOST_STATUS_INVALID_ARGUMENT,
        };
        let (_label_shape, label_data, _) = match ml_tensor_float_data(dataset.labels) {
            Some(parts) => parts,
            None => return HOST_STATUS_INVALID_ARGUMENT,
        };
        if feature_shape.is_empty() || dataset.len == 0 || feature_data.len() % dataset.len != 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let min_label = f64::from_bits(args[1] as u64);
        if !min_label.is_finite() {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let feature_width = feature_data.len() / dataset.len;
        let label_width = label_data.len() / dataset.len;
        let mut out_features = Vec::new();
        let mut out_labels = Vec::new();
        let mut out_len = 0usize;
        for row in 0..dataset.len {
            let label = label_data[row * label_width];
            if label >= min_label {
                out_features.extend_from_slice(
                    &feature_data[row * feature_width..row * feature_width + feature_width],
                );
                out_labels.extend_from_slice(
                    &label_data[row * label_width..row * label_width + label_width],
                );
                out_len = out_len.saturating_add(1);
            }
        }
        match ml_dataset_from_flat_parts(out_features, out_labels, out_len) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) fn std_ml_dataset_split(ctx: *mut SpectraHostCallContext, train: bool) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[1] < 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let dataset =
            with_ml_registry(|registry| registry.datasets.get(&(args[0] as usize)).copied());
        let Some(dataset) = dataset else {
            return HOST_STATUS_NOT_FOUND;
        };
        let train_len = args[1] as usize;
        if train_len == 0 || train_len >= dataset.len {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let (start, len) = if train {
            (0, train_len)
        } else {
            (train_len, dataset.len - train_len)
        };
        match ml_dataset_subset(dataset, start, len) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_dataset_train_split(ctx: *mut SpectraHostCallContext) -> i32 {
    std_ml_dataset_split(ctx, true)
}

pub(crate) extern "C" fn std_ml_dataset_test_split(ctx: *mut SpectraHostCallContext) -> i32 {
    std_ml_dataset_split(ctx, false)
}

pub(crate) extern "C" fn std_ml_dataloader_new(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 3) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[1] <= 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let dataset = args[0] as usize;
        let exists = with_ml_registry(|registry| registry.datasets.contains_key(&dataset));
        if !exists {
            return HOST_STATUS_NOT_FOUND;
        }
        let handle = with_ml_registry(|registry| {
            registry.loaders.insert(MlDataLoader {
                dataset,
                batch_size: args[1] as usize,
                shuffle_seed: args[2] as u64,
            })
        });
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_dataloader_batch_count(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(count) = with_ml_registry(|registry| {
            let loader = registry.loaders.get(&(args[0] as usize))?;
            let dataset = registry.datasets.get(&loader.dataset)?;
            Some(dataset.len.div_ceil(loader.batch_size))
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, count as SpectraHostValue)
    }
}

pub(crate) fn ml_batch_indices(len: usize, batch_size: usize, batch_index: usize, seed: u64) -> Vec<usize> {
    let start = batch_index.saturating_mul(batch_size);
    let end = (start + batch_size).min(len);
    let mut indices = (start..end).collect::<Vec<_>>();
    if seed != 0 {
        indices.sort_by_key(|idx| {
            ((*idx as u64)
                .wrapping_mul(6364136223846793005)
                .wrapping_add(seed))
                >> 32
        });
    }
    indices
}

pub(crate) fn std_ml_dataloader_batch(ctx: *mut SpectraHostCallContext, labels: bool) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[1] < 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some((tensor_handle, len, batch_size, seed)) = with_ml_registry(|registry| {
            let loader = registry.loaders.get(&(args[0] as usize))?;
            let dataset = registry.datasets.get(&loader.dataset)?;
            Some((
                if labels {
                    dataset.labels
                } else {
                    dataset.features
                },
                dataset.len,
                loader.batch_size,
                loader.shuffle_seed,
            ))
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        let indices = ml_batch_indices(len, batch_size, args[1] as usize, seed);
        let Some((shape, data, _requires_grad)) = ml_tensor_float_data(tensor_handle) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if shape.is_empty() || shape[0] != len {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let row_width = data.len() / len;
        let mut out = Vec::with_capacity(indices.len() * row_width);
        for index in indices {
            out.extend_from_slice(&data[index * row_width..index * row_width + row_width]);
        }
        match tensor_alloc(
            TensorDType::Float,
            vec![out.len()],
            f64_values_to_host(&out),
        ) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

pub(crate) extern "C" fn std_ml_dataloader_batch_features(ctx: *mut SpectraHostCallContext) -> i32 {
    std_ml_dataloader_batch(ctx, false)
}

pub(crate) extern "C" fn std_ml_dataloader_batch_labels(ctx: *mut SpectraHostCallContext) -> i32 {
    std_ml_dataloader_batch(ctx, true)
}

pub(crate) extern "C" fn std_ml_dataframe_from_csv(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(path) = ml_read_path_arg(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let (rows, cols, data) = match ml_parse_csv_numeric(&path, args[1] != 0) {
            Ok(parts) => parts,
            Err(code) => return code,
        };
        let handle = with_ml_registry(|registry| {
            registry.dataframes.insert(MlDataFrame { rows, cols, data })
        });
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_dataframe_rows(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(rows) = with_ml_registry(|registry| {
            registry
                .dataframes
                .get(&(args[0] as usize))
                .map(|frame| frame.rows)
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, rows as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_dataframe_cols(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(cols) = with_ml_registry(|registry| {
            registry
                .dataframes
                .get(&(args[0] as usize))
                .map(|frame| frame.cols)
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, cols as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_dataframe_column(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[1] < 0 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(values) = with_ml_registry(|registry| {
            let frame = registry.dataframes.get(&(args[0] as usize))?;
            let col = args[1] as usize;
            if col >= frame.cols {
                return None;
            }
            Some(
                (0..frame.rows)
                    .map(|row| frame.data[row * frame.cols + col])
                    .collect::<Vec<_>>(),
            )
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        match tensor_alloc(
            TensorDType::Float,
            vec![values.len()],
            f64_values_to_host(&values),
        ) {
            Ok(handle) => tensor_result(ctx_ref, handle as SpectraHostValue),
            Err(code) => code,
        }
    }
}

