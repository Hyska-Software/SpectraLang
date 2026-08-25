fn load_package(manifest_path: &Path) -> Result<LoadedPackage, PackageError> {
    let manifest_path = canonicalize_existing(manifest_path)?;
    let root = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| PackageError::MissingManifest(manifest_path.clone()))?;
    let text = fs::read_to_string(&manifest_path).map_err(|error| PackageError::Io {
        path: manifest_path.clone(),
        error,
    })?;
    let manifest: Manifest = toml::from_str(&text).map_err(|error| PackageError::Parse {
        path: manifest_path.clone(),
        error,
    })?;
    if !is_valid_semver(&manifest.project.version) {
        return Err(PackageError::InvalidManifest {
            path: manifest_path.clone(),
            message: format!(
                "project.version '{}' is not valid semver MAJOR.MINOR.PATCH",
                manifest.project.version
            ),
        });
    }
    manifest
        .release
        .validate()
        .map_err(|message| PackageError::InvalidManifest {
            path: manifest_path.clone(),
            message,
        })?;
    let src_dirs = if manifest.project.src_dirs.is_empty() {
        vec![root.join("src")]
    } else {
        manifest
            .project
            .src_dirs
            .iter()
            .map(|src| root.join(src))
            .collect()
    };

    // APPEND-ONLY (RemoteRegistry): optional [registry].remote base URL.
    let remote_registry = manifest
        .registry
        .as_ref()
        .and_then(|section| section.remote.clone());
    Ok(LoadedPackage {
        name: manifest.project.name,
        version: manifest.project.version,
        release: manifest.release,
        root,
        manifest: manifest_path,
        src_dirs,
        entry: manifest.project.entry.map(PathBuf::from),
        workspace_members: manifest.workspace.members,
        package_catalogs: manifest.package.catalogs,
        dependency_specs: manifest.dependencies,
        remote_registry,
        manifest_hash: stable_hash_hex(text.as_bytes()),
    })
}

fn validate_package_compatibility(
    name: &str,
    release: &ReleaseMetadata,
    root: &Path,
) -> Result<(), PackageError> {
    let required = cli_compatibility_level();
    if release.compatibility != required {
        return Err(PackageError::IncompatiblePackage {
            name: name.to_string(),
            required: required.to_string(),
            found: release.compatibility.clone(),
            source: root.display().to_string(),
        });
    }
    Ok(())
}

fn install_from_registry(
    root: &Path,
    registry: &Path,
    name: &str,
    version: &str,
) -> Result<InstalledRegistryPackage, PackageError> {
    validate_package_identity(name, version)?;
    let registry_package = registry_package_dir(registry, name, version);
    let metadata_path = registry_package.join("package.toml");
    let metadata_text = fs::read_to_string(&metadata_path).map_err(|error| PackageError::Io {
        path: metadata_path.clone(),
        error,
    })?;
    let metadata: RegistryMetadata =
        toml::from_str(&metadata_text).map_err(|error| PackageError::Parse {
            path: metadata_path.clone(),
            error,
        })?;
    if metadata.version != version {
        return Err(PackageError::Registry(format!(
            "registry metadata version '{}' does not match requested version '{}'",
            metadata.version, version
        )));
    }
    let payload = registry_package.join("package");
    let registry_root = canonicalize_existing(registry)?;
    let payload = canonicalize_existing(&payload)?;
    ensure_within_root(&registry_root, &payload)?;
    let checksum = directory_checksum(&payload)?;
    if checksum != metadata.checksum {
        return Err(PackageError::Registry(format!(
            "checksum mismatch for {} {}",
            name, version
        )));
    }

    let vendor_dir = root.join(".spectra").join("packages").join(format!(
        "{}-{}",
        name.replace('/', "_"),
        version
    ));
    if let Some(parent) = vendor_dir.parent() {
        fs::create_dir_all(parent).map_err(|error| PackageError::Io {
            path: parent.to_path_buf(),
            error,
        })?;
    }
    let _lock = acquire_cache_lock(&vendor_dir)?;
    let staging = staging_path(&vendor_dir, "dir");
    let result = (|| {
        fs::create_dir_all(&staging).map_err(|error| PackageError::Io {
            path: staging.clone(),
            error,
        })?;
        copy_package_payload(&payload, &staging)?;
        atomic_replace(&staging, &vendor_dir)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    Ok(InstalledRegistryPackage {
        canonical_name: metadata.name,
        version: metadata.version,
        path: vendor_dir,
    })
}

#[allow(clippy::too_many_arguments)]
fn install_from_git(
    workspace_root: &Path,
    name: &str,
    requested_version: Option<&str>,
    git: &str,
    tag: Option<&str>,
    rev: Option<&str>,
    branch: Option<&str>,
    allow_floating: bool,
    anchor_rev: Option<&str>,
    offline: bool,
) -> Result<InstalledGitPackage, PackageError> {
    validate_package_name(name)?;
    check_git_locator_allowed(git)?;
    if let Some(value) = tag {
        validate_git_ref_text("tag", value).map_err(PackageError::Registry)?;
    }
    if let Some(value) = rev {
        validate_git_ref_text("rev", value).map_err(PackageError::Registry)?;
    }
    if tag.is_none() && rev.is_none() && branch.is_none() && anchor_rev.is_none() && !allow_floating
    {
        return Err(PackageError::Registry(format!(
            "git dependency '{}' does not pin a stable ref and would track the mutable \
             upstream HEAD; declare 'tag', 'rev', or 'branch' in the manifest, pass \
             '--allow-floating-git' (or set 'allow-floating-git = true' on the dependency) \
             to accept a floating checkout, or restore spectra.lock so its recorded \
             resolved_rev can anchor the install",
            name
        )));
    }
    if let Some(value) = branch {
        validate_git_ref_text("branch", value).map_err(PackageError::Registry)?;
    }
    let cache_root = workspace_root.join(".spectra").join("git");
    let clone_dir = cache_root.join(sanitize_package_component(name));
    if offline && !clone_dir.join(".git").is_dir() {
        return Err(PackageError::CacheCorrupt {
            package: name.to_string(),
            path: clone_dir,
            reason: "Git cache is missing or is not a repository while offline".to_string(),
        });
    }

    fs::create_dir_all(&cache_root).map_err(|error| PackageError::Io {
        path: cache_root.clone(),
        error,
    })?;
    let _cache_lock = acquire_cache_lock(&clone_dir)?;
    let staging = staging_path(&clone_dir, "git");
    let clone_target = git_path_arg(&staging);
    let clone_result = if offline {
        let source = git_path_arg(&clone_dir);
        run_git(
            &[
                "clone",
                "--quiet",
                "--local",
                source.as_str(),
                clone_target.as_str(),
            ],
            workspace_root,
        )
    } else {
        run_git(
            &["clone", "--quiet", git, clone_target.as_str()],
            workspace_root,
        )
    };
    let result = (|| {
        clone_result?;
        if let Some(rev) = rev {
            run_git(&["checkout", "--quiet", rev], &staging)?;
        } else if let Some(tag) = tag {
            run_git(&["checkout", "--quiet", tag], &staging)?;
        } else if let Some(branch) = branch {
            run_git(&["checkout", "--quiet", branch], &staging)?;
        } else if let Some(anchor) = anchor_rev {
            // Locked install: the lockfile's resolved_rev anchors the checkout even
            // though the manifest itself declares no fixed ref.
            run_git(&["checkout", "--quiet", anchor], &staging)?;
        }
        reject_git_symlinks(&staging, name)?;
        let resolved = git_output(&["rev-parse", "HEAD"], &staging)?;
        let manifest_path = find_manifest(&staging)?;
        let loaded = load_package(&manifest_path)?;
        validate_package_compatibility(&loaded.name, &loaded.release, &staging)?;
        let version = requested_version
            .map(str::to_string)
            .unwrap_or_else(|| loaded.version.clone());
        if loaded.name != name {
            return Err(PackageError::Registry(format!(
                "git package manifest name '{}' does not match requested '{}'",
                loaded.name, name
            )));
        }
        if loaded.version != version {
            return Err(PackageError::Registry(format!(
                "git package '{}' version '{}' does not match requested '{}'",
                name, loaded.version, version
            )));
        }
        atomic_replace(&staging, &clone_dir)?;
        Ok((resolved, loaded, version))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    let (resolved, loaded, version) = result?;

    let vendor_dir = workspace_root
        .join(".spectra")
        .join("packages")
        .join(format!("{}-{}", sanitize_package_component(name), version));
    if let Some(parent) = vendor_dir.parent() {
        fs::create_dir_all(parent).map_err(|error| PackageError::Io {
            path: parent.to_path_buf(),
            error,
        })?;
    }
    let _vendor_lock = acquire_cache_lock(&vendor_dir)?;
    let vendor_staging = staging_path(&vendor_dir, "dir");
    let vendor_result = (|| {
        fs::create_dir_all(&vendor_staging).map_err(|error| PackageError::Io {
            path: vendor_staging.clone(),
            error,
        })?;
        copy_package_payload(&clone_dir, &vendor_staging)?;
        let checksum = directory_checksum(&vendor_staging)?;
        atomic_replace(&vendor_staging, &vendor_dir)?;
        Ok(checksum)
    })();
    if vendor_result.is_err() {
        let _ = fs::remove_dir_all(&vendor_staging);
    }
    let checksum = vendor_result?;
    Ok(InstalledGitPackage {
        name: loaded.name,
        version,
        path: vendor_dir,
        resolved,
        checksum,
    })
}

