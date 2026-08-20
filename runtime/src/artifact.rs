//! Spectra Artifact Container v1.
//!
//! The container is deliberately small and deterministic: a fixed header,
//! canonical JSON metadata/table, contiguous little-endian array payloads,
//! and a SHA-256 digest over everything except the digest itself.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAGIC: &[u8; 8] = b"SPARART1";
const FORMAT_VERSION: u32 = 1;
const HEADER_LEN: usize = 28;
const DIGEST_LEN: usize = 32;
const MAX_METADATA: usize = 16 * 1024 * 1024;
const MAX_PAYLOAD: usize = 1024 * 1024 * 1024;
const MAX_ARRAYS: usize = 4096;

#[derive(Debug, Clone)]
pub(crate) struct TensorPayload {
    pub name: String,
    pub dtype: String,
    pub precision: String,
    pub shape: Vec<usize>,
    pub layout: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct ArtifactData {
    pub name: String,
    pub model_version: String,
    pub kind: String,
    pub metadata: BTreeMap<String, String>,
    pub tensors: Vec<TensorPayload>,
}

#[derive(Debug)]
pub(crate) enum ArtifactError {
    Io(io::Error),
    Invalid(String),
}

impl std::fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "artifact I/O error: {error}"),
            Self::Invalid(message) => write!(f, "invalid artifact: {message}"),
        }
    }
}

impl From<io::Error> for ArtifactError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

fn invalid(message: impl Into<String>) -> ArtifactError {
    ArtifactError::Invalid(message.into())
}

fn digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn checked_u64(value: usize, field: &str) -> Result<u64, ArtifactError> {
    u64::try_from(value).map_err(|_| invalid(format!("{field} exceeds u64")))
}

fn metadata_value(metadata: &BTreeMap<String, String>) -> Value {
    let mut object = Map::new();
    for (key, value) in metadata {
        object.insert(key.clone(), Value::String(value.clone()));
    }
    Value::Object(object)
}

#[cfg(windows)]
fn atomic_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
    }
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    let source: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

pub(crate) fn write_atomic(path: &Path, data: &ArtifactData) -> Result<(), ArtifactError> {
    if data.name.is_empty() || data.model_version.is_empty() {
        return Err(invalid("name and model_version are required"));
    }
    if data.kind != "checkpoint" && data.kind != "multi_array" {
        return Err(invalid("kind must be checkpoint or multi_array"));
    }
    if data.tensors.is_empty() || data.tensors.len() > MAX_ARRAYS {
        return Err(invalid(
            "artifact must contain between one and MAX_ARRAYS tensors",
        ));
    }

    let mut names = std::collections::HashSet::new();
    let mut payload = Vec::new();
    let mut arrays = Vec::with_capacity(data.tensors.len());
    for tensor in &data.tensors {
        if tensor.name.is_empty() || !names.insert(tensor.name.clone()) {
            return Err(invalid("tensor names must be non-empty and unique"));
        }
        if tensor.dtype != "int" && tensor.dtype != "float" {
            return Err(invalid("unsupported tensor dtype"));
        }
        if tensor.precision != "f64" {
            return Err(invalid("only f64 physical precision is supported in v1"));
        }
        if tensor.shape.is_empty() || tensor.shape.contains(&0) {
            return Err(invalid("tensor shape must be non-empty and non-zero"));
        }
        let element_count = tensor
            .shape
            .iter()
            .try_fold(1usize, |acc, dim| acc.checked_mul(*dim))
            .ok_or_else(|| invalid("tensor shape overflows usize"))?;
        let expected_bytes = element_count
            .checked_mul(8)
            .ok_or_else(|| invalid("tensor byte length overflows usize"))?;
        if expected_bytes != tensor.bytes.len() {
            return Err(invalid(format!(
                "tensor {} has incompatible byte length",
                tensor.name
            )));
        }
        let offset = checked_u64(payload.len(), "payload offset")?;
        let length = checked_u64(tensor.bytes.len(), "payload length")?;
        payload.extend_from_slice(&tensor.bytes);
        arrays.push(serde_json::json!({
            "name": tensor.name,
            "dtype": tensor.dtype,
            "precision": tensor.precision,
            "shape": tensor.shape,
            "layout": tensor.layout,
            "offset": offset,
            "length": length,
            "checksum": digest(&tensor.bytes),
        }));
    }
    if payload.len() > MAX_PAYLOAD {
        return Err(invalid("payload exceeds maximum size"));
    }
    let manifest = serde_json::json!({
        "schema": "spectralang.artifact.v1",
        "format_version": FORMAT_VERSION,
        "kind": data.kind,
        "name": data.name,
        "model_version": data.model_version,
        "compatibility": {
            "container": "spectralang.artifact.v1",
            "tensor_encoding": "little-endian-f64-slots",
        },
        "metadata": metadata_value(&data.metadata),
        "arrays": arrays,
    });
    let manifest_bytes =
        serde_json::to_vec(&manifest).map_err(|error| invalid(error.to_string()))?;
    if manifest_bytes.len() > MAX_METADATA {
        return Err(invalid("manifest exceeds maximum size"));
    }

    let mut bytes =
        Vec::with_capacity(HEADER_LEN + manifest_bytes.len() + payload.len() + DIGEST_LEN);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    bytes.extend_from_slice(&(manifest_bytes.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&manifest_bytes);
    bytes.extend_from_slice(&payload);
    let checksum = Sha256::digest(&bytes);
    bytes.extend_from_slice(&checksum);

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = PathBuf::from(path);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    temporary.set_extension(format!("tmp-{}-{nonce}", std::process::id()));
    fs::write(&temporary, &bytes)?;
    if let Err(error) = atomic_replace(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(ArtifactError::Io(error));
    }
    Ok(())
}

fn expect_string(object: &Map<String, Value>, key: &str) -> Result<String, ArtifactError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid(format!("missing string field {key}")))
}

pub(crate) fn read(path: &Path) -> Result<ArtifactData, ArtifactError> {
    let bytes = fs::read(path)?;
    if bytes.len() < HEADER_LEN + DIGEST_LEN || &bytes[..8] != MAGIC {
        return Err(invalid("bad magic or truncated header"));
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    if version != FORMAT_VERSION {
        return Err(invalid("unsupported format version"));
    }
    let metadata_len = usize::try_from(u64::from_le_bytes(bytes[12..20].try_into().unwrap()))
        .map_err(|_| invalid("manifest length does not fit usize"))?;
    let payload_len = usize::try_from(u64::from_le_bytes(bytes[20..28].try_into().unwrap()))
        .map_err(|_| invalid("payload length does not fit usize"))?;
    if metadata_len > MAX_METADATA || payload_len > MAX_PAYLOAD {
        return Err(invalid("artifact exceeds configured limits"));
    }
    let expected_len = HEADER_LEN
        .checked_add(metadata_len)
        .and_then(|value| value.checked_add(payload_len))
        .and_then(|value| value.checked_add(DIGEST_LEN))
        .ok_or_else(|| invalid("artifact length overflows usize"))?;
    if bytes.len() != expected_len {
        return Err(invalid("artifact has unexpected trailing or missing bytes"));
    }
    let actual_digest = Sha256::digest(&bytes[..bytes.len() - DIGEST_LEN]);
    if actual_digest.as_slice() != &bytes[bytes.len() - DIGEST_LEN..] {
        return Err(invalid("global checksum mismatch"));
    }
    let manifest: Value = serde_json::from_slice(&bytes[HEADER_LEN..HEADER_LEN + metadata_len])
        .map_err(|error| invalid(format!("invalid manifest JSON: {error}")))?;
    let object = manifest
        .as_object()
        .ok_or_else(|| invalid("manifest must be an object"))?;
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "schema"
                | "format_version"
                | "kind"
                | "name"
                | "model_version"
                | "compatibility"
                | "metadata"
                | "arrays"
        ) {
            return Err(invalid(format!("unknown manifest field {key}")));
        }
    }
    if object.get("schema").and_then(Value::as_str) != Some("spectralang.artifact.v1")
        || object.get("format_version").and_then(Value::as_u64) != Some(FORMAT_VERSION as u64)
    {
        return Err(invalid("manifest schema/version mismatch"));
    }
    let compatibility = object
        .get("compatibility")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("compatibility must be an object"))?;
    if compatibility.get("container").and_then(Value::as_str) != Some("spectralang.artifact.v1")
        || compatibility.get("tensor_encoding").and_then(Value::as_str)
            != Some("little-endian-f64-slots")
    {
        return Err(invalid("unsupported compatibility contract"));
    }
    let kind = expect_string(object, "kind")?;
    if kind != "checkpoint" && kind != "multi_array" {
        return Err(invalid("invalid artifact kind"));
    }
    let metadata_object = object
        .get("metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("metadata must be an object"))?;
    let mut metadata = BTreeMap::new();
    for (key, value) in metadata_object {
        metadata.insert(
            key.clone(),
            value
                .as_str()
                .ok_or_else(|| invalid("metadata values must be strings"))?
                .to_owned(),
        );
    }
    let array_values = object
        .get("arrays")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("arrays must be an array"))?;
    if array_values.is_empty() || array_values.len() > MAX_ARRAYS {
        return Err(invalid("invalid array count"));
    }
    let payload_start = HEADER_LEN + metadata_len;
    let payload = &bytes[payload_start..payload_start + payload_len];
    let mut names = std::collections::HashSet::new();
    let mut ranges = Vec::with_capacity(array_values.len());
    let mut tensors = Vec::with_capacity(array_values.len());
    for value in array_values {
        let item = value
            .as_object()
            .ok_or_else(|| invalid("array entry must be an object"))?;
        for key in item.keys() {
            if !matches!(
                key.as_str(),
                "name"
                    | "dtype"
                    | "precision"
                    | "shape"
                    | "layout"
                    | "offset"
                    | "length"
                    | "checksum"
            ) {
                return Err(invalid(format!("unknown array field {key}")));
            }
        }
        let name = expect_string(item, "name")?;
        if name.is_empty() {
            return Err(invalid("array name must not be empty"));
        }
        if !names.insert(name.clone()) {
            return Err(invalid("duplicate array name"));
        }
        let dtype = expect_string(item, "dtype")?;
        let precision = expect_string(item, "precision")?;
        let layout = expect_string(item, "layout")?;
        if dtype != "int" && dtype != "float" || precision != "f64" || layout != "contiguous" {
            return Err(invalid("unsupported array representation"));
        }
        let shape = item
            .get("shape")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("array shape must be an array"))?
            .iter()
            .map(|dim| {
                dim.as_u64()
                    .and_then(|value| usize::try_from(value).ok())
                    .ok_or_else(|| invalid("array shape must contain integers"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if shape.is_empty() || shape.contains(&0) {
            return Err(invalid("array shape must be non-empty and non-zero"));
        }
        let offset = item
            .get("offset")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| invalid("invalid array offset"))?;
        let length = item
            .get("length")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| invalid("invalid array length"))?;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| invalid("array range overflows"))?;
        if end > payload.len() {
            return Err(invalid("array range exceeds payload"));
        }
        ranges.push((offset, end, name.clone()));
        let elements = shape
            .iter()
            .try_fold(1usize, |acc, dim| acc.checked_mul(*dim))
            .ok_or_else(|| invalid("array shape overflows"))?;
        if elements.checked_mul(8) != Some(length) {
            return Err(invalid("array length does not match shape"));
        }
        let array_bytes = payload[offset..end].to_vec();
        if expect_string(item, "checksum")? != digest(&array_bytes) {
            return Err(invalid(format!("checksum mismatch for array {name}")));
        }
        tensors.push(TensorPayload {
            name,
            dtype,
            precision,
            shape,
            layout,
            bytes: array_bytes,
        });
    }
    ranges.sort_unstable_by_key(|(start, _, _)| *start);
    for window in ranges.windows(2) {
        if window[0].1 > window[1].0 {
            return Err(invalid(format!(
                "array ranges overlap: {} and {}",
                window[0].2, window[1].2
            )));
        }
    }
    let name = expect_string(object, "name")?;
    let model_version = expect_string(object, "model_version")?;
    if name.is_empty() || model_version.is_empty() {
        return Err(invalid("name and model_version are required"));
    }
    Ok(ArtifactData {
        name,
        model_version,
        kind,
        metadata,
        tensors,
    })
}

pub(crate) fn validate(path: &Path) -> bool {
    read(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ArtifactData {
        ArtifactData {
            name: "test".to_string(),
            model_version: "v1".to_string(),
            kind: "multi_array".to_string(),
            metadata: BTreeMap::from([("owner".to_string(), "test".to_string())]),
            tensors: vec![TensorPayload {
                name: "weights".to_string(),
                dtype: "float".to_string(),
                precision: "f64".to_string(),
                shape: vec![2],
                layout: "contiguous".to_string(),
                bytes: vec![0; 16],
            }],
        }
    }

    #[test]
    fn round_trip_and_checksum_validation() {
        let path = std::env::temp_dir().join(format!("spectra-r3003-{}.spar", std::process::id()));
        write_atomic(&path, &sample()).unwrap();
        assert!(validate(&path));
        write_atomic(&path, &sample()).unwrap();
        assert!(validate(&path));
        let mut bytes = fs::read(&path).unwrap();
        bytes[HEADER_LEN + 1] ^= 1;
        fs::write(&path, bytes).unwrap();
        assert!(!validate(&path));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_truncation_and_unsupported_version() {
        let path =
            std::env::temp_dir().join(format!("spectra-r3003-invalid-{}.spar", std::process::id()));
        write_atomic(&path, &sample()).unwrap();
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
        assert!(!validate(&path));
        fs::write(&path, &bytes).unwrap();
        let mut unsupported = bytes;
        unsupported[8..12].copy_from_slice(&99u32.to_le_bytes());
        fs::write(&path, unsupported).unwrap();
        assert!(!validate(&path));
        let _ = fs::remove_file(path);
    }
}
