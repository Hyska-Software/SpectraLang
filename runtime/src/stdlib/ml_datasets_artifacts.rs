fn ml_read_path_arg(arg: SpectraHostValue) -> Option<String> {
    let path = unsafe { read_spectra_string(arg)? };
    if path.trim().is_empty() {
        return None;
    }
    Some(path)
}

fn ml_parse_csv_numeric(path: &str, has_header: bool) -> Result<(usize, usize, Vec<f64>), i32> {
    let content = std::fs::read_to_string(path).map_err(|_| HOST_STATUS_NOT_FOUND)?;
    let mut rows = 0usize;
    let mut cols = None;
    let mut values = Vec::new();
    for (line_index, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if has_header && line_index == 0 {
            continue;
        }
        let parsed = line
            .split(',')
            .map(|part| part.trim().parse::<f64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
        if parsed.is_empty() {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }
        match cols {
            Some(expected) if expected != parsed.len() => return Err(HOST_STATUS_INVALID_ARGUMENT),
            None => cols = Some(parsed.len()),
            _ => {}
        }
        rows = rows.saturating_add(1);
        values.extend(parsed);
    }
    let cols = cols.ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
    if rows == 0 {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    Ok((rows, cols, values))
}

fn ml_dataset_from_flat_parts(
    features: Vec<f64>,
    labels: Vec<f64>,
    len: usize,
) -> Result<usize, i32> {
    if len == 0 || features.is_empty() || labels.is_empty() {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    if !features.len().is_multiple_of(len) || !labels.len().is_multiple_of(len) {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let features_handle = tensor_alloc(
        TensorDType::Float,
        vec![len, features.len() / len],
        f64_values_to_host(&features),
    )?;
    let labels_handle = tensor_alloc(
        TensorDType::Float,
        vec![len, labels.len() / len],
        f64_values_to_host(&labels),
    )?;
    let handle = with_ml_registry(|registry| {
        registry.datasets.insert(MlDataset {
            features: features_handle,
            labels: labels_handle,
            len,
        })
    });
    Ok(handle)
}

fn ml_dataset_from_csv_path(path: &str, label_col: usize, has_header: bool) -> Result<usize, i32> {
    let (rows, cols, values) = ml_parse_csv_numeric(path, has_header)?;
    if cols < 2 || label_col >= cols {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let mut features = Vec::with_capacity(rows * (cols - 1));
    let mut labels = Vec::with_capacity(rows);
    for row in 0..rows {
        for col in 0..cols {
            let value = values[row * cols + col];
            if col == label_col {
                labels.push(value);
            } else {
                features.push(value);
            }
        }
    }
    ml_dataset_from_flat_parts(features, labels, rows)
}

fn ml_parse_json_number_after(key: &str, input: &str) -> Option<f64> {
    let start = input.find(key)? + key.len();
    let rest = input[start..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let end = rest
        .find(|ch: char| !(ch.is_ascii_digit() || matches!(ch, '-' | '+' | '.' | 'e' | 'E')))
        .unwrap_or(rest.len());
    rest[..end].parse::<f64>().ok()
}

fn ml_parse_json_features(input: &str) -> Option<Vec<f64>> {
    let key_pos = input.find("\"features\"")?;
    let after_key = &input[key_pos..];
    let open = after_key.find('[')? + key_pos;
    let close = input[open..].find(']')? + open;
    input[open + 1..close]
        .split(',')
        .map(|part| part.trim().parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()
}

fn ml_dataset_from_jsonl_path(path: &str) -> Result<usize, i32> {
    let content = std::fs::read_to_string(path).map_err(|_| HOST_STATUS_NOT_FOUND)?;
    let mut features = Vec::new();
    let mut labels = Vec::new();
    let mut row_width = None;
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let row = ml_parse_json_features(line).ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
        let label =
            ml_parse_json_number_after("\"label\"", line).ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
        if row.is_empty() {
            return Err(HOST_STATUS_INVALID_ARGUMENT);
        }
        match row_width {
            Some(expected) if expected != row.len() => return Err(HOST_STATUS_INVALID_ARGUMENT),
            None => row_width = Some(row.len()),
            _ => {}
        }
        features.extend(row);
        labels.push(label);
    }
    ml_dataset_from_flat_parts(features, labels.clone(), labels.len())
}

fn ml_parse_npy_f64_1d(path: &str) -> Result<Vec<f64>, i32> {
    let bytes = std::fs::read(path).map_err(|_| HOST_STATUS_NOT_FOUND)?;
    if bytes.len() < 16 || &bytes[0..6] != b"\x93NUMPY" || bytes[6] != 1 || bytes[7] != 0 {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let header_len = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
    let header_start = 10usize;
    let data_start = header_start
        .checked_add(header_len)
        .ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
    if data_start > bytes.len() {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let header = std::str::from_utf8(&bytes[header_start..data_start])
        .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    if !header.contains("'descr': '<f8'") && !header.contains("\"descr\": \"<f8\"") {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    if header.contains("True") {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let open = header.find('(').ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
    let close = header[open..]
        .find(')')
        .map(|idx| open + idx)
        .ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
    let shape_text = header[open + 1..close].trim().trim_end_matches(',');
    let len = shape_text
        .parse::<usize>()
        .map_err(|_| HOST_STATUS_INVALID_ARGUMENT)?;
    let expected_bytes = len
        .checked_mul(8)
        .and_then(|n| data_start.checked_add(n))
        .ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
    if expected_bytes != bytes.len() {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let mut values = Vec::with_capacity(len);
    for chunk in bytes[data_start..].chunks_exact(8) {
        values.push(f64::from_le_bytes([
            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
        ]));
    }
    Ok(values)
}

fn ml_dataset_subset(dataset: MlDataset, start: usize, len: usize) -> Result<usize, i32> {
    let (feature_shape, feature_data, _) =
        ml_tensor_float_data(dataset.features).ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
    let (label_shape, label_data, _) =
        ml_tensor_float_data(dataset.labels).ok_or(HOST_STATUS_INVALID_ARGUMENT)?;
    if feature_shape.is_empty() || label_shape.is_empty() || feature_shape[0] != dataset.len {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    if start.saturating_add(len) > dataset.len {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let feature_width = feature_data.len() / dataset.len;
    let label_width = label_data.len() / dataset.len;
    let f_start = start * feature_width;
    let l_start = start * label_width;
    ml_dataset_from_flat_parts(
        feature_data[f_start..f_start + len * feature_width].to_vec(),
        label_data[l_start..l_start + len * label_width].to_vec(),
        len,
    )
}

fn ml_fnv64_file(path: &str) -> Result<(u64, String), i32> {
    let bytes = std::fs::read(path).map_err(|_| HOST_STATUS_NOT_FOUND)?;
    let mut hash = 0xcbf29ce484222325u64;
    for byte in &bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Ok((bytes.len() as u64, format!("{hash:016x}")))
}

fn ml_artifact_record(path: String) -> Result<MlArtifactRecord, i32> {
    if path.trim().is_empty() {
        return Err(HOST_STATUS_INVALID_ARGUMENT);
    }
    let (size, fnv64) = ml_fnv64_file(&path)?;
    Ok(MlArtifactRecord { path, size, fnv64 })
}

fn ml_json_string(value: &str) -> String {
    format!("\"{}\"", json_escape(value))
}

fn ml_artifact_json(record: &MlArtifactRecord) -> String {
    format!(
        "{{\"path\":{},\"size\":{},\"fnv64\":{}}}",
        ml_json_string(&record.path),
        record.size,
        ml_json_string(&record.fnv64)
    )
}

fn ml_experiment_manifest_json(experiment: &MlExperiment) -> String {
    let mut configs = experiment.configs.clone();
    configs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let configs_json = configs
        .iter()
        .map(|(key, value)| {
            format!(
                "{{\"key\":{},\"value\":{}}}",
                ml_json_string(key),
                ml_json_string(value)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let metrics_json = experiment
        .metrics
        .iter()
        .map(|metric| {
            format!(
                "{{\"name\":{},\"step\":{},\"value\":{}}}",
                ml_json_string(&metric.name),
                metric.step,
                metric.value
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let artifacts_json = experiment
        .artifacts
        .iter()
        .map(ml_artifact_json)
        .collect::<Vec<_>>()
        .join(",");
    let lockfile_json = experiment
        .lockfile
        .as_ref()
        .map(ml_artifact_json)
        .unwrap_or_else(|| "null".to_string());
    let model_output_json = experiment
        .model_output
        .as_ref()
        .map(ml_artifact_json)
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"schema\":\"spectra.ml.experiment.v1\",\"name\":{},\"seed\":{},\"configs\":[{}],\"metrics\":[{}],\"artifacts\":[{}],\"lockfile\":{},\"model_output\":{},\"manifest_path\":{},\"reproduction_command\":{}}}",
        ml_json_string(&experiment.name),
        experiment.seed,
        configs_json,
        metrics_json,
        artifacts_json,
        lockfile_json,
        model_output_json,
        ml_json_string(&experiment.manifest_path),
        ml_json_string(&experiment.reproduction_command)
    )
}

fn ml_manifest_section(source: &str, key: &str) -> Option<String> {
    let start = source.find(key)? + key.len();
    let bytes = source.as_bytes();
    let mut index = start;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if index >= bytes.len() || bytes[index] != b':' {
        return None;
    }
    index += 1;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    let open = *bytes.get(index)?;
    let close = match open {
        b'[' => b']',
        b'{' => b'}',
        b'"' => b'"',
        b'n' => return Some("null".to_string()),
        b'-' | b'0'..=b'9' => {
            let mut end = index + 1;
            while end < bytes.len()
                && (bytes[end].is_ascii_digit()
                    || matches!(bytes[end], b'.' | b'e' | b'E' | b'+' | b'-'))
            {
                end += 1;
            }
            return Some(source[index..end].to_string());
        }
        _ => return None,
    };
    if open == b'"' {
        let mut end = index + 1;
        while end < bytes.len() {
            if bytes[end] == b'"' && bytes[end - 1] != b'\\' {
                return Some(source[index..=end].to_string());
            }
            end += 1;
        }
        return None;
    }
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for end in index..bytes.len() {
        let byte = bytes[end];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        if byte == b'"' {
            in_string = true;
        } else if byte == open {
            depth += 1;
        } else if byte == close {
            depth -= 1;
            if depth == 0 {
                return Some(source[index..=end].to_string());
            }
        }
    }
    None
}

fn ml_compare_manifest_payloads(left: &str, right: &str) -> bool {
    for key in [
        "\"configs\"",
        "\"metrics\"",
        "\"artifacts\"",
        "\"lockfile\"",
        "\"model_output\"",
        "\"seed\"",
    ] {
        if ml_manifest_section(left, key) != ml_manifest_section(right, key) {
            return false;
        }
    }
    true
}

fn ml_distributed_worker_json(worker: &MlDistributedWorker) -> String {
    format!(
        "{{\"worker_id\":{},\"step_count\":{},\"sample_count\":{},\"accumulator\":{},\"active\":{}}}",
        worker.worker_id,
        worker.step_count,
        worker.sample_count,
        worker.accumulator,
        if worker.active { "true" } else { "false" }
    )
}

fn ml_distributed_session_json(session: &MlDistributedSession) -> String {
    let workers_json = session
        .workers
        .iter()
        .map(ml_distributed_worker_json)
        .collect::<Vec<_>>()
        .join(",");
    let checkpoint_json = session
        .last_checkpoint_path
        .as_ref()
        .map(|path| ml_json_string(path))
        .unwrap_or_else(|| "null".to_string());
    let interrupted_json = session
        .interrupted_worker
        .map(|worker_id| worker_id.to_string())
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"schema\":\"spectra.ml.distributed_checkpoint.v1\",\"name\":{},\"topology\":\"single-machine-simulated-workers\",\"seed\":{},\"worker_count\":{},\"global_step\":{},\"interrupted_worker\":{},\"last_checkpoint_path\":{},\"workers\":[{}]}}",
        ml_json_string(&session.name),
        session.seed,
        session.worker_count,
        session.global_step,
        interrupted_json,
        checkpoint_json,
        workers_json
    )
}

fn ml_distributed_summary_json(session: &MlDistributedSession) -> String {
    let total_samples: i64 = session
        .workers
        .iter()
        .map(|worker| worker.sample_count)
        .sum();
    let total_worker_steps: i64 = session.workers.iter().map(|worker| worker.step_count).sum();
    format!(
        "{{\"schema\":\"spectra.ml.distributed_summary.v1\",\"name\":{},\"topology\":\"single-machine-simulated-workers\",\"worker_count\":{},\"global_step\":{},\"total_worker_steps\":{},\"total_samples\":{},\"checkpoint\":{}}}",
        ml_json_string(&session.name),
        session.worker_count,
        session.global_step,
        total_worker_steps,
        total_samples,
        session
            .last_checkpoint_path
            .as_ref()
            .map(|path| ml_json_string(path))
            .unwrap_or_else(|| "null".to_string())
    )
}

fn ml_checkpoint_number(source: &str, key: &str) -> Option<i64> {
    ml_manifest_section(source, key)?.trim().parse::<i64>().ok()
}

fn ml_checkpoint_string(source: &str, key: &str) -> Option<String> {
    let encoded = ml_manifest_section(source, key)?;
    if encoded == "null" {
        return None;
    }
    let trimmed = encoded.trim();
    if trimmed.len() < 2 || !trimmed.starts_with('"') || !trimmed.ends_with('"') {
        return None;
    }
    Some(
        trimmed[1..trimmed.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\"),
    )
}

fn ml_checkpoint_worker_number(worker_source: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{}\":", key);
    let start = worker_source.find(&needle)? + needle.len();
    let bytes = worker_source.as_bytes();
    let mut end = start;
    while end < bytes.len()
        && (bytes[end].is_ascii_digit() || matches!(bytes[end], b'.' | b'e' | b'E' | b'+' | b'-'))
    {
        end += 1;
    }
    worker_source[start..end]
        .parse::<f64>()
        .ok()
        .map(|value| value as i64)
}

fn ml_checkpoint_worker_float(worker_source: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{}\":", key);
    let start = worker_source.find(&needle)? + needle.len();
    let bytes = worker_source.as_bytes();
    let mut end = start;
    while end < bytes.len()
        && (bytes[end].is_ascii_digit() || matches!(bytes[end], b'.' | b'e' | b'E' | b'+' | b'-'))
    {
        end += 1;
    }
    worker_source[start..end].parse::<f64>().ok()
}

fn ml_checkpoint_worker_bool(worker_source: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{}\":", key);
    let start = worker_source.find(&needle)? + needle.len();
    if worker_source[start..].starts_with("true") {
        Some(true)
    } else if worker_source[start..].starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn ml_distributed_session_from_checkpoint(
    source: &str,
    checkpoint_path: String,
) -> Option<MlDistributedSession> {
    if !source.contains("\"schema\":\"spectra.ml.distributed_checkpoint.v1\"") {
        return None;
    }
    let name = ml_checkpoint_string(source, "\"name\"")?;
    let seed = ml_checkpoint_number(source, "\"seed\"")?;
    let worker_count = ml_checkpoint_number(source, "\"worker_count\"")? as usize;
    let global_step = ml_checkpoint_number(source, "\"global_step\"")?;
    let interrupted_worker = match ml_manifest_section(source, "\"interrupted_worker\"")?.trim() {
        "null" => None,
        value => Some(value.parse::<usize>().ok()?),
    };
    let workers_section = ml_manifest_section(source, "\"workers\"")?;
    let mut workers = Vec::new();
    let mut offset = 0usize;
    while let Some(relative_start) = workers_section[offset..].find('{') {
        let start = offset + relative_start;
        let end = workers_section[start..].find('}')? + start;
        let item = &workers_section[start..=end];
        workers.push(MlDistributedWorker {
            worker_id: ml_checkpoint_worker_number(item, "worker_id")? as usize,
            step_count: ml_checkpoint_worker_number(item, "step_count")?,
            sample_count: ml_checkpoint_worker_number(item, "sample_count")?,
            accumulator: ml_checkpoint_worker_float(item, "accumulator")?,
            active: ml_checkpoint_worker_bool(item, "active")?,
        });
        offset = end + 1;
    }
    if workers.len() != worker_count {
        return None;
    }
    Some(MlDistributedSession {
        name,
        out_dir: std::path::Path::new(&checkpoint_path)
            .parent()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string()),
        worker_count,
        seed,
        global_step,
        interrupted_worker,
        workers,
        last_checkpoint_path: Some(checkpoint_path),
    })
}
