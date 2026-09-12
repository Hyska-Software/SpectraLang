fn default_version() -> String {
    "0.1.0".to_string()
}

fn catalog_schema() -> String {
    "spectra-package-catalog-v1".to_string()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CatalogStateFile {
    version: u32,
    catalogs: Vec<CatalogState>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CatalogState {
    pub name: String,
    pub source: String,
    pub cache: String,
    pub resolved_rev: Option<String>,
    pub index_hash: String,
    #[serde(default)]
    pub synced_at: String,
}

fn catalog_config_path(root: &Path) -> PathBuf {
    root.join(".spectra").join("catalogs").join("catalogs.toml")
}

fn catalog_state_path(root: &Path) -> PathBuf {
    root.join(".spectra").join("catalogs").join("catalogs.lock")
}

fn catalog_cache_root(root: &Path) -> PathBuf {
    root.join(".spectra").join("catalogs")
}

fn load_catalog_config(root: &Path) -> Result<BTreeMap<String, String>, PackageError> {
    let path = catalog_config_path(root);
    if !path.is_file() {
        return Ok(BTreeMap::new());
    }
    let text = fs::read_to_string(&path).map_err(|error| PackageError::Io {
        path: path.clone(),
        error,
    })?;
    let config: BTreeMap<String, String> = toml::from_str(&text).map_err(|error| {
        PackageError::EditParse {
            path: path.clone(),
            message: error.to_string(),
        }
    })?;
    for (name, source) in &config {
        validate_package_name(name)?;
        if source.trim().is_empty() {
            return Err(PackageError::CatalogSync {
                name: name.clone(),
                source: source.clone(),
                path: path.clone(),
                reason: "catalog source is empty".to_string(),
            });
        }
    }
    Ok(config)
}

fn load_catalog_state(root: &Path) -> Result<CatalogStateFile, PackageError> {
    let path = catalog_state_path(root);
    if !path.is_file() {
        return Ok(CatalogStateFile {
            version: 1,
            catalogs: Vec::new(),
        });
    }
    let text = fs::read_to_string(&path).map_err(|error| PackageError::Io {
        path: path.clone(),
        error,
    })?;
    toml::from_str(&text).map_err(|error| PackageError::EditParse {
        path,
        message: error.to_string(),
    })
}

fn write_catalog_state(root: &Path, mut state: CatalogStateFile) -> Result<(), PackageError> {
    state.version = 1;
    state.catalogs.sort_by(|left, right| left.name.cmp(&right.name));
    let text = toml::to_string_pretty(&state).map_err(PackageError::Serialize)?;
    let path = catalog_state_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| PackageError::Io {
            path: parent.to_path_buf(),
            error,
        })?;
    }
    atomic_write_file(&path, text.as_bytes())
}

fn normalized_catalog_index(path: &Path) -> Result<(CatalogIndex, String), PackageError> {
    let text = fs::read_to_string(path).map_err(|error| PackageError::Io {
        path: path.to_path_buf(),
        error,
    })?;
    let mut index: CatalogIndex = toml::from_str(&text).map_err(|error| PackageError::Parse {
        path: path.to_path_buf(),
        error,
    })?;
    if index.schema != catalog_schema() {
        return Err(PackageError::CatalogSync {
            name: path.display().to_string(),
            source: path.display().to_string(),
            path: path.to_path_buf(),
            reason: format!("unsupported schema '{}'", index.schema),
        });
    }
    for package in &index.packages {
        validate_catalog_package(package, false).map_err(|reason| PackageError::CatalogSync {
            name: package.name.clone(),
            source: package.git.clone(),
            path: path.to_path_buf(),
            reason,
        })?;
    }
    index.packages.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| compare_versions(&left.version, &right.version))
            .then_with(|| left.git.cmp(&right.git))
    });
    let normalized = toml::to_string_pretty(&index).map_err(PackageError::Serialize)?;
    Ok((index, normalized))
}

fn catalog_source_index(source: &str, root: &Path) -> PathBuf {
    let path = PathBuf::from(source);
    let path = if path.is_absolute() { path } else { root.join(path) };
    catalog_index_path(&path)
}

fn catalog_cache_dir(root: &Path, name: &str) -> PathBuf {
    catalog_cache_root(root).join(sanitize_package_component(name))
}

fn catalog_state_for<'a>(state: &'a CatalogStateFile, name: &str) -> Option<&'a CatalogState> {
    state.catalogs.iter().find(|entry| entry.name == name)
}

fn sync_catalog(
    root: &Path,
    name: &str,
    source: &str,
    offline: bool,
) -> Result<CatalogState, PackageError> {
    let cache_dir = catalog_cache_dir(root, name);
    let cache_root = catalog_cache_root(root);
    fs::create_dir_all(&cache_root).map_err(|error| PackageError::Io {
        path: cache_root.clone(),
        error,
    })?;
    check_git_locator_allowed(source)?;

    let staging = staging_path(&cache_dir, "catalog");
    let result = (|| {
        if offline {
            if !cache_dir.is_dir() {
                return Err(PackageError::CatalogSync {
                    name: name.to_string(),
                    source: source.to_string(),
                    path: cache_dir.clone(),
                    reason: "catalog cache is missing while offline".to_string(),
                });
            }
            ensure_within_root(&cache_root, &cache_dir)?;
            let index_path = cache_dir.join("package.index.toml");
            let (_, normalized) = normalized_catalog_index(&index_path)?;
            let resolved_rev = if cache_dir.join(".git").is_dir() {
                Some(git_output(&["rev-parse", "HEAD"], &cache_dir).map_err(|_| {
                    PackageError::CatalogSync {
                        name: name.to_string(),
                        source: source.to_string(),
                        path: cache_dir.clone(),
                        reason: "cached Git repository cannot resolve HEAD".to_string(),
                    }
                })?)
            } else {
                None
            };
            return Ok(CatalogState {
                name: name.to_string(),
                source: source.to_string(),
                cache: relative_catalog_cache(root, &cache_dir),
                resolved_rev,
                index_hash: stable_hash_hex(normalized.as_bytes()),
                synced_at: String::new(),
            });
        }

        fs::create_dir_all(&staging).map_err(|error| PackageError::Io {
            path: staging.clone(),
            error,
        })?;
        let local_source = catalog_source_index(source, root)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(source));
        let is_local_git = local_source.join(".git").is_dir();
        if remote_git_host(source).is_some() || is_local_git {
            let target = git_path_arg(&staging);
            if is_local_git {
                let source_path = git_path_arg(&local_source);
                run_catalog_git(
                    &["clone", "--quiet", "--local", source_path.as_str(), target.as_str()],
                    root,
                    name,
                    source,
                    &staging,
                )?;
            } else {
                run_catalog_git(
                    &["clone", "--quiet", source, target.as_str()],
                    root,
                    name,
                    source,
                    &staging,
                )?;
            }
        } else {
            let index_path = catalog_source_index(source, root);
            let (index, normalized) = normalized_catalog_index(&index_path)?;
            let _ = index;
            atomic_write_file(&staging.join("package.index.toml"), normalized.as_bytes())?;
        }

        let index_path = staging.join("package.index.toml");
        let (_, normalized) = normalized_catalog_index(&index_path)?;
        atomic_write_file(&index_path, normalized.as_bytes())?;
        let resolved_rev = if staging.join(".git").is_dir() {
            Some(run_catalog_git_output(
                &["rev-parse", "HEAD"],
                &staging,
                name,
                source,
                &staging,
            )?)
        } else {
            None
        };
        atomic_replace(&staging, &cache_dir)?;
        Ok(CatalogState {
            name: name.to_string(),
            source: source.to_string(),
            cache: relative_catalog_cache(root, &cache_dir),
            resolved_rev,
            index_hash: stable_hash_hex(normalized.as_bytes()),
            synced_at: unix_timestamp_string(),
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn relative_catalog_cache(root: &Path, cache: &Path) -> String {
    cache.strip_prefix(root).unwrap_or(cache).to_string_lossy().replace('\\', "/")
}

fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn run_catalog_git(args: &[&str], cwd: &Path, name: &str, source: &str, path: &Path) -> Result<(), PackageError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| PackageError::CatalogSync {
            name: name.to_string(),
            source: source.to_string(),
            path: path.to_path_buf(),
            reason: format!("failed to launch Git: {}", error),
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(PackageError::CatalogSync {
            name: name.to_string(),
            source: source.to_string(),
            path: path.to_path_buf(),
            reason: "Git synchronization failed".to_string(),
        })
    }
}

fn run_catalog_git_output(args: &[&str], cwd: &Path, name: &str, source: &str, path: &Path) -> Result<String, PackageError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| PackageError::CatalogSync {
            name: name.to_string(),
            source: source.to_string(),
            path: path.to_path_buf(),
            reason: format!("failed to launch Git: {}", error),
        })?;
    if !output.status.success() {
        return Err(PackageError::CatalogSync {
            name: name.to_string(),
            source: source.to_string(),
            path: path.to_path_buf(),
            reason: "Git synchronization failed".to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn catalog_add(root: &Path, name: &str, source: &str) -> Result<(), PackageError> {
    let root = canonicalize_existing(root)?;
    validate_package_name(name)?;
    check_git_locator_allowed(source)?;
    let mut config = load_catalog_config(&root)?;
    config.insert(name.to_string(), source.to_string());
    let text = toml::to_string_pretty(&config).map_err(PackageError::Serialize)?;
    let path = catalog_config_path(&root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| PackageError::Io {
            path: parent.to_path_buf(),
            error,
        })?;
    }
    atomic_write_file(&path, text.as_bytes())
}

pub fn catalog_sync(root: &Path, offline: bool, locked: bool) -> Result<Vec<CatalogState>, PackageError> {
    let root = canonicalize_existing(root)?;
    let config = load_catalog_config(&root)?;
    let existing = load_catalog_state(&root)?;
    let cache_only = offline || locked;
    let mut synced = Vec::new();
    for (name, source) in &config {
        let entry = sync_catalog(&root, name, source, cache_only)?;
        if cache_only {
            let previous = catalog_state_for(&existing, name).ok_or_else(|| PackageError::CatalogSync {
                name: name.clone(),
                source: source.clone(),
                path: catalog_state_path(&root),
                reason: "catalog state is missing while offline or locked".to_string(),
            })?;
            if previous.source != entry.source
                || previous.cache != entry.cache
                || previous.resolved_rev != entry.resolved_rev
                || previous.index_hash != entry.index_hash
            {
                return Err(PackageError::CatalogSync {
                    name: name.clone(),
                    source: source.clone(),
                    path: catalog_state_path(&root),
                    reason: "cached catalog differs from catalogs.lock".to_string(),
                });
            }
            synced.push(entry);
        } else {
            synced.push(entry);
        }
    }
    if !cache_only {
        write_catalog_state(
            &root,
            CatalogStateFile {
                version: 1,
                catalogs: synced.clone(),
            },
        )?;
    }
    Ok(synced)
}

pub fn catalog_list(root: &Path) -> Result<Vec<String>, PackageError> {
    let root = canonicalize_existing(root)?;
    let config = load_catalog_config(&root)?;
    let state = load_catalog_state(&root)?;
    Ok(config
        .iter()
        .map(|(name, source)| {
            if let Some(entry) = catalog_state_for(&state, name) {
                format!("{}\t{}\tready\t{}", name, source, entry.resolved_rev.as_deref().unwrap_or("local"))
            } else {
                format!("{}\t{}\tmissing\t-", name, source)
            }
        })
        .collect())
}

pub fn catalog_remove(root: &Path, name: &str) -> Result<(), PackageError> {
    let root = canonicalize_existing(root)?;
    validate_package_name(name)?;
    let mut config = load_catalog_config(&root)?;
    config.remove(name);
    let config_path = catalog_config_path(&root);
    if config_path.is_file() {
        let text = toml::to_string_pretty(&config).map_err(PackageError::Serialize)?;
        atomic_write_file(&config_path, text.as_bytes())?;
    }
    let mut state = load_catalog_state(&root)?;
    state.catalogs.retain(|entry| entry.name != name);
    write_catalog_state(&root, state)?;
    let cache_root = catalog_cache_root(&root);
    let cache = catalog_cache_dir(&root, name);
    if cache.is_dir() {
        ensure_within_root(&cache_root, &cache)?;
        fs::remove_dir_all(&cache).map_err(|error| PackageError::Io {
            path: cache,
            error,
        })?;
    }
    Ok(())
}

pub fn resolve(root: &Path) -> Result<ResolvedWorkspace, PackageError> {
    resolve_with_options(root, ResolveOptions::default())
}

pub fn resolve_with_options(
    root: &Path,
    options: ResolveOptions,
) -> Result<ResolvedWorkspace, PackageError> {
    let root = canonicalize_existing(root)?;
    let manifest_path = find_manifest(&root)?;
    let root_package = load_package(&manifest_path)?;
    let root_name = root_package.name.clone();
    let mut package_roots = Vec::new();
    package_roots.push(root_package.root.clone());

    for member in &root_package.workspace_members {
        package_roots.push(canonicalize_existing(&root_package.root.join(member))?);
    }

    let mut visited = HashSet::new();
    let mut by_name = BTreeMap::new();
    let mut ordered = Vec::new();
    let mut active = Vec::new();

    for package_root in package_roots {
        collect_package(
            &root,
            &package_root,
            PackageSource::Path {
                path: package_root.clone(),
            },
            &mut visited,
            &mut by_name,
            &mut ordered,
            &mut active,
            options,
        )?;
    }

    let packages = topo_sort(ordered)?;
    Ok(ResolvedWorkspace {
        root,
        root_name,
        packages,
    })
}

pub fn write_lockfile(workspace: &ResolvedWorkspace) -> Result<PathBuf, PackageError> {
    let path = workspace.root.join(LOCKFILE_NAME);
    let lockfile = lockfile_for_workspace(workspace);
    let text = toml::to_string_pretty(&lockfile).map_err(PackageError::Serialize)?;
    atomic_write_file(&path, text.as_bytes())?;
    Ok(path)
}

fn lockfile_for_workspace(workspace: &ResolvedWorkspace) -> Lockfile {
    Lockfile {
        version: 2,
        root: workspace
            .root_package_name()
            .unwrap_or_else(|| "workspace".to_string()),
        packages: workspace
            .packages
            .iter()
            .map(|package| LockPackage {
                name: package.name.clone(),
                version: package.version.clone(),
                channel: package.release.channel.as_str().to_string(),
                compatibility: package.release.compatibility.clone(),
                deprecated_since: package.release.deprecated_since.clone(),
                migration: package.release.migration.clone(),
                source: path_source(&workspace.root, &package.root),
                source_kind: package.source.kind().to_string(),
                checksum: package.checksum.clone(),
                git_url: package.source.git_url(),
                git_ref: package.source.git_ref(),
                resolved_rev: package.source.git_resolved(),
                git_warning: match &package.source {
                    PackageSource::Git {
                        floating: true, ..
                    } => Some(
                        "resolved from mutable git HEAD; pin tag/rev/branch or declare \
                         'allow-floating-git = true' to acknowledge"
                            .to_string(),
                    ),
                    _ => None,
                },
                manifest_hash: package.manifest_hash.clone(),
                dependencies: package
                    .dependencies
                    .values()
                    .map(|dep| LockDependency {
                        name: dep.name.clone(),
                        version: dep.version.clone(),
                        source: path_source(&workspace.root, &dep.path),
                        source_kind: dep.source.kind().to_string(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

pub fn verify_lockfile(workspace: &ResolvedWorkspace) -> Result<PathBuf, PackageError> {
    let path = workspace.root.join(LOCKFILE_NAME);
    if !path.is_file() {
        return Err(PackageError::LockfileMissing(path));
    }
    let text = fs::read_to_string(&path).map_err(|error| PackageError::Io {
        path: path.clone(),
        error,
    })?;
    let actual: Lockfile = toml::from_str(&text).map_err(|error| PackageError::AtomicWrite {
        path: path.clone(),
        message: format!("invalid lockfile: {}", error),
    })?;
    let expected = lockfile_for_workspace(workspace);
    if actual.version != expected.version {
        return Err(PackageError::LockfileTampered {
            path,
            package: expected.root.clone(),
            field: "version".to_string(),
        });
    }
    if actual.root != expected.root {
        return Err(PackageError::LockfileTampered {
            path,
            package: expected.root.clone(),
            field: "root".to_string(),
        });
    }
    if actual.packages.len() != expected.packages.len() {
        return Err(PackageError::LockfileTampered {
            path,
            package: expected.root.clone(),
            field: "packages".to_string(),
        });
    }
    for (actual_package, expected_package) in actual.packages.iter().zip(&expected.packages) {
        if actual_package != expected_package {
            return Err(PackageError::LockfileTampered {
                path,
                package: expected_package.name.clone(),
                field: "package graph, source, checksum, or dependencies".to_string(),
            });
        }
    }
    Ok(path)
}

