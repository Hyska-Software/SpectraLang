impl PackageSource {
    fn kind(&self) -> &'static str {
        match self {
            PackageSource::Path { .. } => "path",
            PackageSource::Registry { .. } => "registry",
            PackageSource::Git { .. } => "git",
        }
    }

    fn git_url(&self) -> Option<String> {
        match self {
            PackageSource::Git { url, .. } => Some(url.clone()),
            _ => None,
        }
    }

    fn git_ref(&self) -> Option<String> {
        match self {
            PackageSource::Git { requested, .. } => Some(requested.clone()),
            _ => None,
        }
    }

    fn git_resolved(&self) -> Option<String> {
        match self {
            PackageSource::Git { resolved, .. } => Some(resolved.clone()),
            _ => None,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn add_dependency(
    root: &Path,
    name: &str,
    version: Option<&str>,
    path: Option<&Path>,
    registry: Option<&Path>,
    git: Option<&str>,
    tag: Option<&str>,
    rev: Option<&str>,
    branch: Option<&str>,
    catalog: Option<&Path>,
) -> Result<PathBuf, PackageError> {
    let root = canonicalize_existing(root)?;
    let manifest_path = find_manifest(&root)?;
    let parsed = parse_package_request(name, version)?;
    let (dependency_name, dependency_version, _dependency_path, dependency_source) =
        if let Some(path) = path {
            let version = version.unwrap_or("0.1.0");
            (
                parsed.name,
                version.to_string(),
                relative_or_absolute(&root, path),
                DependencyManifestSource::Path(relative_or_absolute(&root, path)),
            )
        } else if let Some(registry) = registry {
            let version = parsed.version.as_deref().unwrap_or("0.1.0");
            let installed = install_from_registry(&root, registry, &parsed.name, version)?;
            (
                installed.canonical_name,
                installed.version,
                installed.path.clone(),
                DependencyManifestSource::Path(relative_or_absolute(&root, &installed.path)),
            )
        } else if let Some(git) = git {
            let installed = install_from_git(
                &root,
                &parsed.name,
                parsed.version.as_deref(),
                git,
                tag,
                rev,
                branch,
                false,
            )?;
            (
                installed.name,
                installed.version,
                installed.path.clone(),
                DependencyManifestSource::Git {
                    git: git.to_string(),
                    tag: tag.map(str::to_string),
                    rev: rev.map(str::to_string),
                    branch: branch.map(str::to_string),
                    checksum: Some(installed.checksum),
                },
            )
        } else {
            let entry =
                resolve_catalog_entry(&root, &parsed.name, parsed.version.as_deref(), catalog)?;
            let installed = install_from_git(
                &root,
                &entry.name,
                Some(&entry.version),
                &entry.git,
                entry.tag.as_deref(),
                entry.rev.as_deref(),
                entry.branch.as_deref(),
                false,
            )?;
            (
                installed.name,
                installed.version,
                installed.path.clone(),
                DependencyManifestSource::Git {
                    git: entry.git,
                    tag: entry.tag,
                    rev: entry.rev,
                    branch: entry.branch,
                    checksum: Some(installed.checksum),
                },
            )
        };

    if !is_valid_semver(&dependency_version) {
        return Err(PackageError::InvalidManifest {
            path: manifest_path.clone(),
            message: format!(
                "dependency '{}' has invalid semver '{}'",
                dependency_name, dependency_version
            ),
        });
    }
    write_dependency_to_manifest(
        &manifest_path,
        &dependency_name,
        &dependency_version,
        &dependency_source,
    )?;

    let workspace = resolve(&root)?;
    write_lockfile(&workspace)
}

#[derive(Clone, Debug)]
struct PackageRequest {
    name: String,
    version: Option<String>,
}

#[derive(Clone, Debug)]
enum DependencyManifestSource {
    Path(PathBuf),
    Git {
        git: String,
        tag: Option<String>,
        rev: Option<String>,
        branch: Option<String>,
        checksum: Option<String>,
    },
}

fn parse_package_request(
    name: &str,
    version: Option<&str>,
) -> Result<PackageRequest, PackageError> {
    if let Some((left, right)) = name.rsplit_once('@') {
        if !left.is_empty() && !right.is_empty() {
            return Ok(PackageRequest {
                name: left.to_string(),
                version: Some(right.to_string()),
            });
        }
    }
    Ok(PackageRequest {
        name: name.to_string(),
        version: version.map(str::to_string),
    })
}

fn write_dependency_to_manifest(
    manifest_path: &Path,
    dependency_name: &str,
    dependency_version: &str,
    source: &DependencyManifestSource,
) -> Result<(), PackageError> {
    let text = fs::read_to_string(manifest_path).map_err(|error| PackageError::Io {
        path: manifest_path.to_path_buf(),
        error,
    })?;
    let mut doc = text
        .parse::<DocumentMut>()
        .map_err(|error| PackageError::EditParse {
            path: manifest_path.to_path_buf(),
            message: error.to_string(),
        })?;

    if !doc.as_table().contains_key("dependencies") {
        doc["dependencies"] = Item::Table(Table::new());
    }

    let mut table = Table::new();
    table["version"] = value(dependency_version);
    match source {
        DependencyManifestSource::Path(path) => {
            table["path"] = value(path.to_string_lossy().replace('\\', "/"));
        }
        DependencyManifestSource::Git {
            git,
            tag,
            rev,
            branch,
            checksum,
        } => {
            table["git"] = value(git.as_str());
            if let Some(tag) = tag {
                table["tag"] = value(tag.as_str());
            }
            if let Some(rev) = rev {
                table["rev"] = value(rev.as_str());
            }
            if let Some(branch) = branch {
                table["branch"] = value(branch.as_str());
            }
            if let Some(checksum) = checksum {
                table["checksum"] = value(checksum.as_str());
            }
        }
    }
    doc["dependencies"][dependency_name] = Item::Table(table);
    fs::write(manifest_path, doc.to_string()).map_err(|error| PackageError::Io {
        path: manifest_path.to_path_buf(),
        error,
    })
}

pub fn publish(root: &Path, registry: &Path) -> Result<PathBuf, PackageError> {
    let workspace = resolve(root)?;
    let package = workspace
        .packages
        .first()
        .ok_or_else(|| PackageError::MissingPackage("root".to_string()))?;
    let package_dir = registry.join(&package.name).join(&package.version);
    let payload_dir = package_dir.join("package");

    if package_dir.exists() {
        fs::remove_dir_all(&package_dir).map_err(|error| PackageError::Io {
            path: package_dir.clone(),
            error,
        })?;
    }
    fs::create_dir_all(&payload_dir).map_err(|error| PackageError::Io {
        path: payload_dir.clone(),
        error,
    })?;

    copy_package_payload(&package.root, &payload_dir)?;
    let checksum = directory_checksum(&payload_dir)?;
    let metadata = RegistryMetadata {
        name: package.name.clone(),
        version: package.version.clone(),
        channel: package.release.channel.as_str().to_string(),
        compatibility: package.release.compatibility.clone(),
        deprecated_since: package.release.deprecated_since.clone(),
        migration: package.release.migration.clone(),
        checksum,
        source_path: package.root.to_string_lossy().replace('\\', "/"),
    };
    let metadata_text = toml::to_string_pretty(&metadata).map_err(PackageError::Serialize)?;
    let metadata_path = package_dir.join("package.toml");
    fs::write(&metadata_path, metadata_text).map_err(|error| PackageError::Io {
        path: metadata_path.clone(),
        error,
    })?;

    Ok(package_dir)
}

pub fn write_docs(workspace: &ResolvedWorkspace) -> Result<PathBuf, PackageError> {
    let docs_dir = workspace.root.join("target").join("spectra-docs");
    fs::create_dir_all(&docs_dir).map_err(|error| PackageError::Io {
        path: docs_dir.clone(),
        error,
    })?;
    let path = docs_dir.join("packages.md");
    let mut text = String::from("# Spectra Packages\n\n");
    for package in &workspace.packages {
        text.push_str(&format!("## {} {}\n\n", package.name, package.version));
        text.push_str(&format!("- channel: `{}`\n", package.release.channel));
        text.push_str(&format!(
            "- compatibility: `{}`\n",
            package.release.compatibility
        ));
        if let Some(warning) = package.release.deprecation_warning(&package.name) {
            text.push_str(&format!("- deprecation: `{}`\n", warning));
        }
        text.push_str(&format!("- root: `{}`\n", package.root.display()));
        text.push_str(&format!("- manifest: `{}`\n", package.manifest.display()));
        if package.dependencies.is_empty() {
            text.push_str("- dependencies: none\n\n");
        } else {
            text.push_str("- dependencies:\n");
            for dependency in package.dependencies.values() {
                text.push_str(&format!(
                    "  - {} {} from `{}`\n",
                    dependency.name,
                    dependency.version,
                    dependency.path.display()
                ));
            }
            text.push('\n');
        }
    }
    fs::write(&path, text).map_err(|error| PackageError::Io {
        path: path.clone(),
        error,
    })?;
    Ok(path)
}

#[derive(Debug)]
struct LoadedPackage {
    name: String,
    version: String,
    release: ReleaseMetadata,
    root: PathBuf,
    manifest: PathBuf,
    src_dirs: Vec<PathBuf>,
    entry: Option<PathBuf>,
    workspace_members: Vec<String>,
    package_catalogs: BTreeMap<String, String>,
    dependency_specs: BTreeMap<String, DependencySpec>,
    manifest_hash: String,
}

#[allow(clippy::too_many_arguments)]
fn collect_package(
    workspace_root: &Path,
    root: &Path,
    source: PackageSource,
    visited: &mut HashSet<PathBuf>,
    by_name: &mut BTreeMap<String, PathBuf>,
    ordered: &mut Vec<ResolvedPackage>,
    active: &mut Vec<String>,
    options: ResolveOptions,
) -> Result<(), PackageError> {
    let root = canonicalize_existing(root)?;
    let manifest_path = find_manifest(&root)?;
    let loaded = load_package(&manifest_path)?;
    validate_package_compatibility(&loaded.name, &loaded.release, &loaded.root)?;

    if let Some(index) = active.iter().position(|name| name == &loaded.name) {
        let mut chain = active[index..].to_vec();
        chain.push(loaded.name);
        return Err(PackageError::DependencyCycle(chain));
    }
    if !visited.insert(root.clone()) {
        return Ok(());
    }

    if let Some(first) = by_name.insert(loaded.name.clone(), root.clone()) {
        return Err(PackageError::DuplicatePackage {
            name: loaded.name,
            first,
            second: root,
        });
    }
    active.push(loaded.name.clone());

    let mut dependencies = BTreeMap::new();
    for (dep_name, spec) in &loaded.dependency_specs {
        let (dep_root, dep_source, expected_checksum) = match spec {
            DependencySpec::Detailed {
                path: Some(path), ..
            } => {
                let dep_root = canonicalize_existing(&root.join(path))?;
                (
                    dep_root.clone(),
                    PackageSource::Path { path: dep_root },
                    None,
                )
            }
            DependencySpec::Detailed {
                registry: Some(path),
                version,
                checksum,
                ..
            } => {
                let version = version.as_deref().unwrap_or("0.1.0");
                let installed =
                    install_from_registry(workspace_root, &root.join(path), dep_name, version)?;
                (
                    installed.path.clone(),
                    PackageSource::Registry {
                        path: installed.path,
                    },
                    checksum.clone(),
                )
            }
            DependencySpec::Detailed {
                git: Some(git),
                version,
                tag,
                rev,
                branch,
                checksum,
                ..
            } => {
                let installed = install_from_git(
                    workspace_root,
                    dep_name,
                    version.as_deref(),
                    git,
                    tag.as_deref(),
                    rev.as_deref(),
                    branch.as_deref(),
                    options.offline,
                )?;
                (
                    installed.path.clone(),
                    PackageSource::Git {
                        url: git.clone(),
                        requested: git_requested_ref(
                            tag.as_deref(),
                            rev.as_deref(),
                            branch.as_deref(),
                        ),
                        resolved: installed.resolved,
                    },
                    checksum.clone().or(Some(installed.checksum)),
                )
            }
            DependencySpec::Version(_)
            | DependencySpec::Detailed {
                path: None,
                git: None,
                registry: None,
                ..
            } => {
                return Err(PackageError::InvalidManifest {
                    path: loaded.manifest.clone(),
                    message: format!(
                        "dependency '{}' must declare a local path, registry path, or git source",
                        dep_name
                    ),
                });
            }
        };
        collect_package(
            workspace_root,
            &dep_root,
            dep_source.clone(),
            visited,
            by_name,
            ordered,
            active,
            options,
        )?;
        let dep_manifest = load_package(&find_manifest(&dep_root)?)?;
        let version = match spec {
            DependencySpec::Version(version) => version.clone(),
            DependencySpec::Detailed { version, .. } => {
                version.clone().unwrap_or(dep_manifest.version)
            }
        };
        if !is_valid_semver(&version) {
            return Err(PackageError::InvalidManifest {
                path: loaded.manifest.clone(),
                message: format!("dependency '{}' has invalid semver '{}'", dep_name, version),
            });
        }
        dependencies.insert(
            dep_name.clone(),
            ResolvedDependency {
                name: dep_name.clone(),
                version,
                path: dep_root,
                source: dep_source,
            },
        );
        if let Some(expected) = expected_checksum {
            let actual = directory_checksum(&dependencies[dep_name].path)?;
            if actual != expected {
                return Err(PackageError::Registry(format!(
                    "checksum mismatch for dependency '{}'",
                    dep_name
                )));
            }
        }
    }
    let checksum = match &source {
        PackageSource::Path { .. } => loaded.manifest_hash.clone(),
        PackageSource::Registry { .. } | PackageSource::Git { .. } => {
            directory_checksum(&loaded.root)?
        }
    };

    ordered.push(ResolvedPackage {
        name: loaded.name,
        version: loaded.version,
        release: loaded.release,
        root: loaded.root,
        manifest: loaded.manifest,
        src_dirs: loaded.src_dirs,
        entry: loaded.entry,
        dependencies,
        manifest_hash: loaded.manifest_hash,
        source,
        checksum,
    });

    active.pop();

    Ok(())
}

fn topo_sort(packages: Vec<ResolvedPackage>) -> Result<Vec<ResolvedPackage>, PackageError> {
    let mut remaining: BTreeMap<String, ResolvedPackage> = packages
        .into_iter()
        .map(|package| (package.name.clone(), package))
        .collect();
    let mut emitted = BTreeSet::new();
    let mut ordered = Vec::new();

    while !remaining.is_empty() {
        let ready_name = remaining
            .iter()
            .find(|(_, package)| {
                package
                    .dependencies
                    .keys()
                    .all(|dependency| emitted.contains(dependency))
            })
            .map(|(name, _)| name.clone());

        let Some(name) = ready_name else {
            let mut chain = remaining.keys().cloned().collect::<Vec<_>>();
            if let Some(first) = chain.first().cloned() {
                chain.push(first);
            }
            return Err(PackageError::DependencyCycle(chain));
        };
        let package = remaining.remove(&name).expect("ready package exists");
        emitted.insert(name);
        ordered.push(package);
    }

    Ok(ordered)
}

