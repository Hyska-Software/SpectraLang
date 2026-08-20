fn git_path_arg(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    text.strip_prefix("//?/").unwrap_or(&text).to_string()
}

fn run_git(args: &[&str], cwd: &Path) -> Result<(), PackageError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| PackageError::Registry(format!("failed to launch git: {}", error)))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(PackageError::Registry(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn git_output(args: &[&str], cwd: &Path) -> Result<String, PackageError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| PackageError::Registry(format!("failed to launch git: {}", error)))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(PackageError::Registry(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn reject_git_symlinks(root: &Path, package: &str) -> Result<(), PackageError> {
    let entries = git_output(&["ls-files", "-s"], root)?;
    for line in entries.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() == Some("120000") {
            let path = fields.nth(2).unwrap_or(".");
            return Err(PackageError::UnsafePath {
                path: root.join(path),
                reason: format!("package '{}' contains a Git symbolic link", package),
            });
        }
    }
    Ok(())
}

fn git_requested_ref(tag: Option<&str>, rev: Option<&str>, branch: Option<&str>) -> String {
    if let Some(rev) = rev {
        format!("rev:{}", rev)
    } else if let Some(tag) = tag {
        format!("tag:{}", tag)
    } else if let Some(branch) = branch {
        format!("branch:{}", branch)
    } else {
        "HEAD".to_string()
    }
}

fn ensure_catalog_publication_ref(
    tag: Option<&str>,
    rev: Option<&str>,
    branch: Option<&str>,
) -> Result<(), PackageError> {
    let ref_count = [tag, rev, branch]
        .iter()
        .filter(|value| value.is_some())
        .count();
    if ref_count != 1 {
        return Err(PackageError::Registry(
            "catalog publication requires exactly one of --tag or --rev; branch-only refs are not accepted"
                .to_string(),
        ));
    }
    if branch.is_some() {
        return Err(PackageError::Registry(
            "catalog publication requires --tag or --rev; branch refs are mutable".to_string(),
        ));
    }
    Ok(())
}

fn resolve_catalog_publication_rev(
    root: &Path,
    tag: Option<&str>,
    rev: Option<&str>,
) -> Result<String, PackageError> {
    let requested = tag.or(rev).ok_or_else(|| {
        PackageError::Registry("catalog publication requires --tag or --rev".to_string())
    })?;
    let resolved = git_output(&["rev-parse", &format!("{}^{{commit}}", requested)], root)?;
    let head = git_output(&["rev-parse", "HEAD"], root)?;
    if resolved != head {
        return Err(PackageError::Registry(format!(
            "catalog ref '{}' resolves to {}, but package root is checked out at {}",
            requested, resolved, head
        )));
    }
    Ok(resolved)
}

fn validate_catalog_package(package: &CatalogPackage, publication: bool) -> Result<(), String> {
    if !is_valid_package_name(&package.name) {
        return Err(format!("package name '{}' is invalid", package.name));
    }
    if !is_valid_semver(&package.version) {
        return Err(format!(
            "package '{}' has invalid semver '{}'",
            package.name, package.version
        ));
    }
    validate_clean_text("git", &package.git)?;
    if package.git.trim().is_empty() {
        return Err(format!("package '{}' has empty git URL", package.name));
    }
    if !is_allowed_git_locator(&package.git) {
        return Err(format!(
            "package '{}' uses unsupported git locator '{}'",
            package.name, package.git
        ));
    }

    let ref_count = [&package.tag, &package.rev, &package.branch]
        .iter()
        .filter(|value| value.is_some())
        .count();
    if ref_count > 1 {
        return Err(format!(
            "package '{}' declares more than one git ref",
            package.name
        ));
    }
    if publication && package.tag.is_none() && package.rev.is_none() {
        return Err(format!(
            "package '{}' must publish with immutable tag or rev",
            package.name
        ));
    }
    if publication && package.branch.is_some() {
        return Err(format!(
            "package '{}' cannot publish mutable branch refs",
            package.name
        ));
    }
    if let Some(tag) = &package.tag {
        validate_clean_text("tag", tag)?;
        let plain = package.version.as_str();
        let prefixed = format!("v{}", package.version);
        if publication && tag != plain && tag != &prefixed {
            return Err(format!(
                "package '{}' tag '{}' must match version '{}' or 'v{}'",
                package.name, tag, package.version, package.version
            ));
        }
    }
    if let Some(rev) = &package.rev {
        validate_git_ref_text("rev", rev)?;
        if publication && !is_hex_sha(rev, 7) {
            return Err(format!(
                "package '{}' publication rev must be a commit SHA",
                package.name
            ));
        }
    }
    if let Some(branch) = &package.branch {
        validate_git_ref_text("branch", branch)?;
    }
    if let Some(resolved) = &package.resolved_rev {
        if !is_hex_sha(resolved, 40) {
            return Err(format!(
                "package '{}' resolved_rev must be a commit SHA",
                package.name
            ));
        }
    } else if publication {
        return Err(format!(
            "package '{}' publication metadata missing resolved_rev",
            package.name
        ));
    }
    if let Some(checksum) = &package.checksum {
        if !is_hex_sha(checksum, 64) {
            return Err(format!(
                "package '{}' checksum must be a SHA-256 hex digest",
                package.name
            ));
        }
    } else if publication {
        return Err(format!(
            "package '{}' metadata missing checksum",
            package.name
        ));
    }

    validate_clean_text("description", &package.description)?;
    validate_clean_text("compatibility", &package.compatibility)?;
    validate_clean_text("license", &package.license)?;
    validate_clean_text("owner", &package.owner)?;
    for keyword in &package.keywords {
        validate_clean_text("keyword", keyword)?;
    }
    if publication && package.modules.is_empty() {
        return Err(format!(
            "package '{}' must export at least one module",
            package.name
        ));
    }
    for module in &package.modules {
        if !is_valid_package_name(module) {
            return Err(format!(
                "package '{}' exports invalid module '{}'",
                package.name, module
            ));
        }
        if module != &package.name && !module.starts_with(&format!("{}.", package.name)) {
            return Err(format!(
                "package '{}' exports module '{}' outside its namespace",
                package.name, module
            ));
        }
    }
    Ok(())
}

fn validate_clean_text(field: &str, value: &str) -> Result<(), String> {
    if value.chars().any(|ch| ch.is_control()) {
        Err(format!("{} contains control characters", field))
    } else {
        Ok(())
    }
}

fn validate_git_ref_text(field: &str, value: &str) -> Result<(), String> {
    validate_clean_text(field, value)?;
    if value.is_empty()
        || value.starts_with('-')
        || value.contains("..")
        || value.contains("@{")
        || value.ends_with(".lock")
        || value.ends_with('/')
        || value
            .chars()
            .any(|ch| matches!(ch, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\'))
    {
        Err(format!("{} '{}' is not a safe git ref", field, value))
    } else {
        Ok(())
    }
}

fn is_allowed_git_locator(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    if lowered.starts_with("https://")
        || lowered.starts_with("ssh://")
        || (value.starts_with("git@") && value.contains(':'))
    {
        return true;
    }
    if Path::new(value)
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return false;
    }
    Path::new(value).is_absolute()
        || value.starts_with("./")
        || value.starts_with(".\\")
        || value.contains('/')
        || value.contains('\\')
}

fn validate_package_name(name: &str) -> Result<(), PackageError> {
    if !is_valid_package_name(name) || name.contains('/') || name.contains('\\') {
        return Err(PackageError::UnsafePath {
            path: PathBuf::from(name),
            reason: "package name contains an invalid or escaping component".to_string(),
        });
    }
    Ok(())
}

fn validate_package_identity(name: &str, version: &str) -> Result<(), PackageError> {
    validate_package_name(name)?;
    if !is_valid_semver(version) {
        return Err(PackageError::UnsafePath {
            path: PathBuf::from(version),
            reason: "package version is not an exact semver value".to_string(),
        });
    }
    Ok(())
}

fn ensure_within_root(root: &Path, path: &Path) -> Result<(), PackageError> {
    let root = canonicalize_existing(root)?;
    let path = canonicalize_existing(path)?;
    if !path.starts_with(&root) {
        return Err(PackageError::UnsafePath {
            path,
            reason: format!("path escapes package root '{}'", root.display()),
        });
    }
    Ok(())
}

fn remote_git_host(locator: &str) -> Option<String> {
    if let Some(rest) = locator.strip_prefix("git@") {
        return rest.split(':').next().map(str::to_ascii_lowercase);
    }
    if locator.starts_with("https://") || locator.starts_with("ssh://") {
        let rest = locator.split_once("://")?.1;
        return rest
            .split(['/', '?', '#'])
            .next()
            .filter(|host| !host.is_empty())
            .map(str::to_ascii_lowercase);
    }
    None
}

fn check_git_locator_allowed(locator: &str) -> Result<(), PackageError> {
    let Some(host) = remote_git_host(locator) else {
        return Ok(());
    };
    let Ok(raw) = env::var("SPECTRA_PACKAGE_ALLOWED_HOSTS") else {
        return Ok(());
    };
    if host_is_allowed(&host, &raw) {
        Ok(())
    } else {
        Err(PackageError::HostNotAllowed {
            host,
            locator: locator.to_string(),
        })
    }
}

fn host_is_allowed(host: &str, raw: &str) -> bool {
    let allowed = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>();
    allowed.contains(host)
}

fn is_hex_sha(value: &str, min_len: usize) -> bool {
    value.len() >= min_len && value.len() <= 64 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

