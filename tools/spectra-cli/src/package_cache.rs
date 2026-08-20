fn exported_modules(src_dirs: &[PathBuf]) -> Result<Vec<String>, PackageError> {
    let mut modules = Vec::new();
    for src in src_dirs {
        if !src.is_dir() {
            continue;
        }
        for source in discovery::discover_sources(std::slice::from_ref(src)) {
            let text = fs::read_to_string(&source).map_err(|error| PackageError::Io {
                path: source.clone(),
                error,
            })?;
            for line in text.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("module ") {
                    let module = rest.trim_end_matches(';').trim();
                    if !module.is_empty() {
                        modules.push(module.to_string());
                    }
                    break;
                }
            }
        }
    }
    modules.sort();
    modules.dedup();
    Ok(modules)
}

pub fn fetch(root: &Path, offline: bool, locked: bool) -> Result<PathBuf, PackageError> {
    let workspace = resolve_with_options(root, ResolveOptions { offline })?;
    if locked {
        verify_lockfile(&workspace)
    } else {
        write_lockfile(&workspace)
    }
}

pub fn dependency_tree(root: &Path) -> Result<Vec<String>, PackageError> {
    let workspace = resolve(root)?;
    let mut rows = Vec::new();
    for package in workspace.packages {
        if package.dependencies.is_empty() {
            rows.push(format!(
                "{} {} ({})",
                package.name,
                package.version,
                package.source.kind()
            ));
        } else {
            for dep in package.dependencies.values() {
                rows.push(format!(
                    "{} {} -> {} {} ({})",
                    package.name,
                    package.version,
                    dep.name,
                    dep.version,
                    dep.source.kind()
                ));
            }
        }
    }
    Ok(rows)
}

fn registry_package_dir(registry: &Path, name: &str, version: &str) -> PathBuf {
    let exact = registry.join(name).join(version);
    if exact.join("package.toml").is_file() {
        return exact;
    }

    if name.contains('-') {
        let dotted = name.replace('-', ".");
        let alias = registry.join(dotted).join(version);
        if alias.join("package.toml").is_file() {
            return alias;
        }
    }

    exact
}

fn find_manifest(root: &Path) -> Result<PathBuf, PackageError> {
    for name in MANIFEST_NAMES {
        let candidate = root.join(name);
        if candidate.is_file() {
            return canonicalize_existing(&candidate);
        }
    }
    Err(PackageError::MissingManifest(root.to_path_buf()))
}

fn canonicalize_existing(path: &Path) -> Result<PathBuf, PackageError> {
    fs::canonicalize(path).map_err(|error| PackageError::Io {
        path: path.to_path_buf(),
        error,
    })
}

fn relative_or_absolute(root: &Path, path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    pathdiff(&absolute, root).unwrap_or(absolute)
}

fn path_source(root: &Path, path: &Path) -> String {
    let relative = pathdiff(path, root).unwrap_or_else(|| path.to_path_buf());
    format!("path+{}", relative.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
fn toml_key(name: &str) -> String {
    if name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn pathdiff(path: &Path, base: &Path) -> Option<PathBuf> {
    let path = path.components().collect::<Vec<_>>();
    let base = base.components().collect::<Vec<_>>();
    let common = path
        .iter()
        .zip(base.iter())
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return None;
    }
    let mut result = PathBuf::new();
    for _ in common..base.len() {
        result.push("..");
    }
    for component in &path[common..] {
        result.push(component.as_os_str());
    }
    Some(result)
}

fn copy_package_payload(from: &Path, to: &Path) -> Result<(), PackageError> {
    ensure_destination_is_safe(to)?;
    for entry in fs::read_dir(from).map_err(|error| PackageError::Io {
        path: from.to_path_buf(),
        error,
    })? {
        let entry = entry.map_err(|error| PackageError::Io {
            path: from.to_path_buf(),
            error,
        })?;
        let path = entry.path();
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        let metadata = fs::symlink_metadata(&path).map_err(|error| PackageError::Io {
            path: path.clone(),
            error,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(PackageError::UnsafePath {
                path,
                reason: "symbolic links are not allowed in package payloads".to_string(),
            });
        }
        if matches!(
            name_text.as_ref(),
            "target" | ".git" | ".spectra" | LOCKFILE_NAME
        ) {
            continue;
        }
        let dest = to.join(&name);
        if dest.file_name().is_none()
            || dest
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(PackageError::UnsafePath {
                path: dest,
                reason: "payload destination escapes staging root".to_string(),
            });
        }
        if path.is_dir() {
            fs::create_dir_all(&dest).map_err(|error| PackageError::Io {
                path: dest.clone(),
                error,
            })?;
            copy_package_payload(&path, &dest)?;
        } else if path.is_file() {
            fs::copy(&path, &dest).map_err(|error| PackageError::Io {
                path: path.clone(),
                error,
            })?;
        }
    }
    Ok(())
}

fn directory_checksum(path: &Path) -> Result<String, PackageError> {
    let mut files = Vec::new();
    collect_files(path, path, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut bytes = Vec::new();
    for (relative, full) in files {
        bytes.extend_from_slice(relative.to_string_lossy().replace('\\', "/").as_bytes());
        bytes.push(0);
        let content = fs::read(&full).map_err(|error| PackageError::Io { path: full, error })?;
        bytes.extend_from_slice(&content);
        bytes.push(0);
    }
    Ok(stable_hash_hex(&bytes))
}

fn collect_files(
    root: &Path,
    current: &Path,
    out: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), PackageError> {
    for entry in fs::read_dir(current).map_err(|error| PackageError::Io {
        path: current.to_path_buf(),
        error,
    })? {
        let entry = entry.map_err(|error| PackageError::Io {
            path: current.to_path_buf(),
            error,
        })?;
        let path = entry.path();
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        let metadata = fs::symlink_metadata(&path).map_err(|error| PackageError::Io {
            path: path.clone(),
            error,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(PackageError::UnsafePath {
                path,
                reason: "symbolic links are not allowed in package checksums".to_string(),
            });
        }
        if matches!(name_text.as_ref(), "target" | ".git" | ".spectra") {
            continue;
        }
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else if path.is_file() {
            let relative = pathdiff(&path, root).unwrap_or_else(|| path.clone());
            out.push((relative, path));
        }
    }
    Ok(())
}

fn ensure_destination_is_safe(path: &Path) -> Result<(), PackageError> {
    let mut current = Some(path);
    while let Some(candidate) = current {
        if candidate.exists() {
            let metadata = fs::symlink_metadata(candidate).map_err(|error| PackageError::Io {
                path: candidate.to_path_buf(),
                error,
            })?;
            if metadata.file_type().is_symlink() {
                return Err(PackageError::UnsafePath {
                    path: candidate.to_path_buf(),
                    reason: "destination contains a symbolic link".to_string(),
                });
            }
        }
        current = candidate.parent();
    }
    Ok(())
}

fn stable_hash_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn staging_path(destination: &Path, label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("package");
    destination.with_file_name(format!(
        ".{}.{}.{}.{}",
        file_name,
        label,
        std::process::id(),
        nonce
    ))
}

struct CacheLock {
    path: PathBuf,
}

impl Drop for CacheLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn acquire_cache_lock(destination: &Path) -> Result<CacheLock, PackageError> {
    let path = PathBuf::from(format!("{}.lock", destination.display()));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| PackageError::AtomicWrite {
            path: destination.to_path_buf(),
            message: if error.kind() == io::ErrorKind::AlreadyExists {
                "another package operation is already using this cache destination".to_string()
            } else {
                error.to_string()
            },
        })?;
    Ok(CacheLock { path })
}

fn atomic_write_file(path: &Path, bytes: &[u8]) -> Result<(), PackageError> {
    let parent = path.parent().ok_or_else(|| PackageError::AtomicWrite {
        path: path.to_path_buf(),
        message: "destination has no parent directory".to_string(),
    })?;
    fs::create_dir_all(parent).map_err(|error| PackageError::Io {
        path: parent.to_path_buf(),
        error,
    })?;
    let _lock = acquire_cache_lock(path)?;
    let staging = staging_path(path, "file");
    let result = (|| {
        fs::write(&staging, bytes).map_err(|error| PackageError::Io {
            path: staging.clone(),
            error,
        })?;
        atomic_replace(&staging, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staging);
    }
    result
}

fn atomic_replace(source: &Path, destination: &Path) -> Result<(), PackageError> {
    if destination.is_dir() && source.is_dir() {
        let backup = staging_path(destination, "backup");
        fs::rename(destination, &backup).map_err(|error| PackageError::AtomicWrite {
            path: destination.to_path_buf(),
            message: error.to_string(),
        })?;
        match fs::rename(source, destination) {
            Ok(()) => {
                let _ = fs::remove_dir_all(&backup);
                return Ok(());
            }
            Err(error) => {
                let _ = fs::rename(&backup, destination);
                return Err(PackageError::AtomicWrite {
                    path: destination.to_path_buf(),
                    message: error.to_string(),
                });
            }
        }
    }
    if !destination.exists() {
        return fs::rename(source, destination).map_err(|error| PackageError::AtomicWrite {
            path: destination.to_path_buf(),
            message: error.to_string(),
        });
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let source_text = source.to_string_lossy();
        let source_text = source_text.strip_prefix(r"\\?\").unwrap_or(&source_text);
        let destination_text = destination.to_string_lossy();
        let destination_text = destination_text
            .strip_prefix(r"\\?\")
            .unwrap_or(&destination_text);
        let source_wide: Vec<u16> = std::ffi::OsStr::new(source_text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let destination_wide: Vec<u16> = std::ffi::OsStr::new(destination_text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let success = unsafe {
            MoveFileExW(
                source_wide.as_ptr(),
                destination_wide.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if success == 0 {
            return Err(PackageError::AtomicWrite {
                path: destination.to_path_buf(),
                message: io::Error::last_os_error().to_string(),
            });
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(source, destination).map_err(|error| PackageError::AtomicWrite {
            path: destination.to_path_buf(),
            message: error.to_string(),
        })
    }
}

#[cfg(windows)]
const MOVEFILE_REPLACE_EXISTING: u32 = 0x00000001;
#[cfg(windows)]
const MOVEFILE_WRITE_THROUGH: u32 = 0x00000008;
#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
}

fn is_valid_semver(version: &str) -> bool {
    let (core, suffix) = match version.split_once('-') {
        Some((core, suffix)) => (core, Some(suffix)),
        None => (version, None),
    };
    let parts = core.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.chars().all(|ch| ch.is_ascii_digit()))
    {
        return false;
    }
    if let Some(suffix) = suffix {
        !suffix.is_empty()
            && suffix
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-'))
    } else {
        true
    }
}

