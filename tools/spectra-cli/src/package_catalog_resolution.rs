fn is_valid_package_name(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|part| {
            let mut chars = part.chars();
            matches!(chars.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
                && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        })
}

fn catalog_entries_match(left: &CatalogPackage, right: &CatalogPackage) -> bool {
    left.git == right.git
        && left.tag == right.tag
        && left.rev == right.rev
        && left.branch == right.branch
        && (left.resolved_rev == right.resolved_rev
            || left.resolved_rev.is_none()
            || right.resolved_rev.is_none())
        && left.checksum == right.checksum
        && left.modules == right.modules
        && left.compatibility == right.compatibility
}

fn sanitize_package_component(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn catalog_paths(root: &Path, explicit: Option<&Path>) -> Result<Vec<PathBuf>, PackageError> {
    if let Some(explicit) = explicit {
        return Ok(vec![catalog_index_path(explicit)]);
    }
    let mut paths = Vec::new();
    for name in load_catalog_config(root)?.keys() {
        paths.push(catalog_cache_dir(root, name).join("package.index.toml"));
    }
    let manifest = find_manifest(root).ok();
    if let Some(manifest_path) = manifest {
        let loaded = load_package(&manifest_path)?;
        for value in loaded.package_catalogs.values() {
            paths.push(catalog_index_path(&root.join(value)));
        }
    }
    paths.push(
        root.join(".spectra")
            .join("catalogs")
            .join("package.index.toml"),
    );
    if let Some(home) = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME")) {
        paths.push(
            PathBuf::from(home)
                .join(".spectra")
                .join("catalogs")
                .join("spectralang-official")
                .join("package.index.toml"),
        );
    }
    Ok(paths)
}

fn catalog_index_path(path: &Path) -> PathBuf {
    if path.is_dir() || path.extension().is_none() {
        path.join("package.index.toml")
    } else {
        path.to_path_buf()
    }
}

fn load_catalogs(
    root: &Path,
    explicit: Option<&Path>,
) -> Result<Vec<CatalogPackage>, PackageError> {
    let mut packages = Vec::new();
    for path in catalog_paths(root, explicit)? {
        if !path.is_file() {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|error| PackageError::Io {
            path: path.clone(),
            error,
        })?;
        let catalog: CatalogIndex = toml::from_str(&text).map_err(|error| PackageError::Parse {
            path: path.clone(),
            error,
        })?;
        for package in &catalog.packages {
            validate_catalog_package(package, false).map_err(|message| {
                PackageError::Registry(format!(
                    "catalog '{}' rejected: {}",
                    path.display(),
                    message
                ))
            })?;
        }
        packages.extend(catalog.packages.into_iter().map(|mut package| {
            package.origin = path.display().to_string();
            package
        }));
    }
    Ok(packages)
}

fn resolve_catalog_entry(
    root: &Path,
    name: &str,
    version: Option<&str>,
    explicit: Option<&Path>,
) -> Result<CatalogPackage, PackageError> {
    let packages = load_catalogs(root, explicit)?;
    let matches = packages
        .into_iter()
        .filter(|package| package.name == name)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return Err(PackageError::MissingPackage(name.to_string()));
    }

    let mut unique = BTreeMap::<String, CatalogPackage>::new();
    for package in matches {
        if let Some(existing) = unique.get(&package.version) {
            if !catalog_entries_match(existing, &package) {
                return Err(PackageError::ConflictingCatalogPackage {
                    name: name.to_string(),
                    version: package.version,
                    first: existing.origin.clone(),
                    second: package.origin,
                });
            }
            continue;
        }
        unique.insert(package.version.clone(), package);
    }

    let requested = version.map(str::to_string);
    if let Some(requested_version) = requested.as_deref() {
        if !unique.contains_key(requested_version) {
            return Err(PackageError::MissingPackage(format!(
                "{}@{}",
                name, requested_version
            )));
        }
    }
    let mut compatible = unique
        .into_values()
        .filter(|package| {
            requested
                .as_deref()
                .is_none_or(|requested| package.version == requested)
        })
        .filter(|package| package.compatibility == cli_compatibility_level())
        .collect::<Vec<_>>();

    if compatible.is_empty() {
        let requested_version = requested.as_deref().unwrap_or("any");
        let found = if requested_version == "any" {
            "no compatible catalog entry".to_string()
        } else {
            load_catalogs(root, explicit)?
                .into_iter()
                .find(|package| package.name == name && package.version == requested_version)
                .map(|package| package.compatibility)
                .unwrap_or_else(|| "no compatible catalog entry".to_string())
        };
        return Err(PackageError::IncompatiblePackage {
            name: name.to_string(),
            required: cli_compatibility_level().to_string(),
            found,
            source: requested_version.to_string(),
        });
    }

    compatible.sort_by(|left, right| compare_versions(&left.version, &right.version));
    Ok(compatible
        .pop()
        .expect("non-empty compatible catalog matches"))
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    match (Version::parse(left), Version::parse(right)) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

pub fn search(
    root: &Path,
    query: &str,
    catalog: Option<&Path>,
) -> Result<Vec<String>, PackageError> {
    let root = canonicalize_existing(root)?;
    let query = query.to_ascii_lowercase();
    let mut rows = Vec::new();
    for package in load_catalogs(&root, catalog)? {
        let haystack = format!(
            "{} {} {} {}",
            package.name,
            package.description,
            package.keywords.join(" "),
            package.owner
        )
        .to_ascii_lowercase();
        if haystack.contains(&query) {
            rows.push(format!(
                "{} {} {}",
                package.name, package.version, package.description
            ));
        }
    }
    rows.sort();
    rows.dedup();
    Ok(rows)
}

pub fn info(root: &Path, name: &str, catalog: Option<&Path>) -> Result<Vec<String>, PackageError> {
    let root = canonicalize_existing(root)?;
    let packages = load_catalogs(&root, catalog)?
        .into_iter()
        .filter(|package| package.name == name)
        .collect::<Vec<_>>();
    if packages.is_empty() {
        return Err(PackageError::MissingPackage(name.to_string()));
    }
    let mut rows = Vec::new();
    for package in packages {
        rows.push(format!(
            "{} {}\ngit: {}\nref: {}\nresolved: {}\ncompatibility: {}\nlicense: {}\nkeywords: {}\nmodules: {}",
            package.name,
            package.version,
            package.git,
            git_requested_ref(package.tag.as_deref(), package.rev.as_deref(), package.branch.as_deref()),
            package.resolved_rev.as_deref().unwrap_or("<unresolved>"),
            package.compatibility,
            package.license,
            package.keywords.join(", "),
            package.modules.join(", ")
        ));
    }
    Ok(rows)
}

pub fn versions(
    root: &Path,
    name: &str,
    catalog: Option<&Path>,
) -> Result<Vec<String>, PackageError> {
    let root = canonicalize_existing(root)?;
    let mut rows = load_catalogs(&root, catalog)?
        .into_iter()
        .filter(|package| package.name == name)
        .map(|package| package.version)
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| compare_versions(left, right));
    rows.dedup();
    if rows.is_empty() {
        return Err(PackageError::MissingPackage(name.to_string()));
    }
    Ok(rows)
}

pub fn write_metadata(
    root: &Path,
    out: &Path,
    git: Option<&str>,
    tag: Option<&str>,
    rev: Option<&str>,
    branch: Option<&str>,
) -> Result<PathBuf, PackageError> {
    let root = canonicalize_existing(root)?;
    let manifest = load_package(&find_manifest(&root)?)?;
    ensure_catalog_publication_ref(tag, rev, branch)?;
    let resolved_rev = resolve_catalog_publication_rev(&root, tag, rev)?;
    let entry = CatalogPackage {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        git: git
            .ok_or_else(|| {
                PackageError::Registry(
                    "publish-metadata requires --git for catalog publication".to_string(),
                )
            })?
            .to_string(),
        tag: tag.map(str::to_string),
        rev: rev.map(str::to_string),
        branch: branch.map(str::to_string),
        resolved_rev: Some(resolved_rev),
        checksum: Some(directory_checksum(&root)?),
        description: String::new(),
        keywords: Vec::new(),
        compatibility: manifest.release.compatibility,
        license: String::new(),
        modules: exported_modules(&manifest.src_dirs)?,
        owner: String::new(),
        origin: String::new(),
    };
    validate_catalog_package(&entry, true).map_err(|message| {
        PackageError::Registry(format!("refusing to publish package metadata: {}", message))
    })?;
    let index = CatalogIndex {
        schema: catalog_schema(),
        packages: vec![entry],
    };
    let text = toml::to_string_pretty(&index).map_err(PackageError::Serialize)?;
    let path = out.to_path_buf();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| PackageError::Io {
            path: parent.to_path_buf(),
            error,
        })?;
    }
    fs::write(&path, text).map_err(|error| PackageError::Io {
        path: path.clone(),
        error,
    })?;
    Ok(path)
}

pub fn register(
    root: &Path,
    catalog: &Path,
    git: &str,
    tag: Option<&str>,
    rev: Option<&str>,
    branch: Option<&str>,
) -> Result<PathBuf, PackageError> {
    let root = canonicalize_existing(root)?;
    let manifest = load_package(&find_manifest(&root)?)?;
    ensure_catalog_publication_ref(tag, rev, branch)?;
    let resolved_rev = resolve_catalog_publication_rev(&root, tag, rev)?;
    let catalog_path = catalog_index_path(catalog);
    let mut index = if catalog_path.is_file() {
        let text = fs::read_to_string(&catalog_path).map_err(|error| PackageError::Io {
            path: catalog_path.clone(),
            error,
        })?;
        toml::from_str::<CatalogIndex>(&text).map_err(|error| PackageError::Parse {
            path: catalog_path.clone(),
            error,
        })?
    } else {
        CatalogIndex {
            schema: catalog_schema(),
            packages: Vec::new(),
        }
    };
    for package in &index.packages {
        validate_catalog_package(package, false).map_err(|message| {
            PackageError::Registry(format!(
                "catalog '{}' rejected: {}",
                catalog_path.display(),
                message
            ))
        })?;
    }
    let entry = CatalogPackage {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        git: git.to_string(),
        tag: tag.map(str::to_string),
        rev: rev.map(str::to_string),
        branch: branch.map(str::to_string),
        resolved_rev: Some(resolved_rev),
        checksum: Some(directory_checksum(&root)?),
        description: String::new(),
        keywords: Vec::new(),
        compatibility: manifest.release.compatibility,
        license: String::new(),
        modules: exported_modules(&manifest.src_dirs)?,
        owner: String::new(),
        origin: String::new(),
    };
    validate_catalog_package(&entry, true).map_err(|message| {
        PackageError::Registry(format!("refusing to register package: {}", message))
    })?;
    for existing in &index.packages {
        if existing.name == entry.name
            && existing.version == entry.version
            && !catalog_entries_match(existing, &entry)
        {
            return Err(PackageError::Registry(format!(
                "refusing to overwrite existing catalog entry '{} {}' with different source metadata",
                entry.name, entry.version
            )));
        }
    }
    index
        .packages
        .retain(|pkg| !(pkg.name == entry.name && pkg.version == entry.version));
    index.packages.push(entry);
    index.packages.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| compare_versions(&left.version, &right.version))
    });
    if let Some(parent) = catalog_path.parent() {
        fs::create_dir_all(parent).map_err(|error| PackageError::Io {
            path: parent.to_path_buf(),
            error,
        })?;
    }
    let text = toml::to_string_pretty(&index).map_err(PackageError::Serialize)?;
    fs::write(&catalog_path, text).map_err(|error| PackageError::Io {
        path: catalog_path.clone(),
        error,
    })?;
    Ok(catalog_path)
}

