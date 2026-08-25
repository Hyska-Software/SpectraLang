#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn stable_hash_is_deterministic() {
        assert_eq!(stable_hash_hex(b"abc"), stable_hash_hex(b"abc"));
        assert_ne!(stable_hash_hex(b"abc"), stable_hash_hex(b"abcd"));
    }

    #[test]
    fn semver_validation_accepts_exact_versions_and_prerelease() {
        assert!(is_valid_semver("1.2.3"));
        assert!(is_valid_semver("1.2.3-alpha.1"));
        assert!(!is_valid_semver("1.2"));
        assert!(!is_valid_semver("latest"));
    }

    #[test]
    fn toml_key_quotes_dotted_package_names() {
        assert_eq!(toml_key("spectra-api"), "spectra-api");
        assert_eq!(toml_key("spectra.api"), "\"spectra.api\"");
    }

    #[test]
    fn security_rejects_escaping_names_and_matches_hosts_exactly() {
        assert!(validate_package_name("../escape").is_err());
        assert!(validate_package_name("safe-package").is_ok());
        assert_eq!(
            remote_git_host("https://GitHub.com/org/pkg.git"),
            Some("github.com".to_string())
        );
        assert!(host_is_allowed("github.com", "github.com,gitlab.com"));
        assert!(!host_is_allowed("evil.github.com", "github.com"));
    }

    #[test]
    fn lockfile_v2_round_trips_for_semantic_comparison() {
        let lockfile = Lockfile {
            version: 2,
            root: "root".to_string(),
            packages: Vec::new(),
        };
        let text = toml::to_string_pretty(&lockfile).expect("serialize lockfile");
        let decoded: Lockfile = toml::from_str(&text).expect("deserialize lockfile");
        assert_eq!(decoded, lockfile);
    }

    fn temp_catalog(contents: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("spectralang-r905-{}.toml", nonce));
        fs::write(&path, contents).expect("catalog write");
        path
    }

    #[test]
    fn catalog_resolution_deduplicates_and_selects_highest_compatible_version() {
        let catalog = temp_catalog(
            r#"
schema = "spectra-package-catalog-v1"

[[packages]]
name = "demo"
version = "1.0.0"
git = "C:\\packages\\demo-1"
compatibility = "spectralang-0.1"

[[packages]]
name = "demo"
version = "1.1.0"
git = "C:\\packages\\demo-11"
compatibility = "spectralang-0.1"

[[packages]]
name = "demo"
version = "1.1.0"
git = "C:\\packages\\demo-11"
compatibility = "spectralang-0.1"

[[packages]]
name = "demo"
version = "2.0.0"
git = "C:\\packages\\demo-2"
compatibility = "spectralang-0.2"
"#,
        );
        let resolved = resolve_catalog_entry(Path::new("."), "demo", None, Some(&catalog))
            .expect("compatible catalog entry");
        assert_eq!(resolved.version, "1.1.0");
        assert_eq!(resolved.git, "C:\\packages\\demo-11");
        let _ = fs::remove_file(catalog);
    }

    #[test]
    fn catalog_resolution_reports_conflicting_same_version_entries() {
        let catalog = temp_catalog(
            r#"
[[packages]]
name = "demo"
version = "1.0.0"
git = "C:\\packages\\one"
compatibility = "spectralang-0.1"

[[packages]]
name = "demo"
version = "1.0.0"
git = "C:\\packages\\two"
compatibility = "spectralang-0.1"
"#,
        );
        let error = resolve_catalog_entry(Path::new("."), "demo", None, Some(&catalog))
            .expect_err("conflicting entries must fail");
        assert!(error.to_string().contains("version '1.0.0' conflict"));
        let _ = fs::remove_file(catalog);
    }

    #[test]
    fn package_diagnostics_include_origins_and_cycle_chain() {
        let duplicate = PackageError::DuplicatePackage {
            name: "demo".to_string(),
            first: PathBuf::from("one"),
            second: PathBuf::from("two"),
        };
        assert!(duplicate.to_string().contains("'one' and 'two'"));
        let cycle =
            PackageError::DependencyCycle(vec!["a".to_string(), "b".to_string(), "a".to_string()]);
        assert!(cycle.to_string().contains("a -> b -> a"));
    }

    #[test]
    fn offline_git_resolution_rejects_missing_cache_before_git() {
        let root = std::env::temp_dir().join(format!(
            "spectralang-r913-cache-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("cache root");
        let result = install_from_git(
            &root,
            "missing-package",
            Some("1.0.0"),
            "https://example.invalid/missing.git",
            Some("v1.0.0"),
            None,
            None,
            false,
            None,
            true,
        );
        let error = match result {
            Ok(_) => panic!("offline resolution must not use a missing cache"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cache") || error.to_string().contains("offline"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn locked_fetch_requires_an_existing_lockfile() {
        let root = std::env::temp_dir().join(format!(
            "spectralang-r913-lock-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(root.join("src")).expect("workspace root");
        fs::write(
            root.join("spectra.toml"),
            "[project]\nname = \"r913_root\"\nversion = \"0.1.0\"\n",
        )
        .expect("manifest");
        let error = fetch(&root, true, true).expect_err("locked fetch must require a lockfile");
        assert!(error.to_string().contains("lockfile"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn catalog_config_is_loaded_in_deterministic_order() {
        let root = std::env::temp_dir().join(format!(
            "spectralang-r911-config-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let config = root.join(".spectra/catalogs/catalogs.toml");
        fs::create_dir_all(config.parent().expect("config parent")).expect("config directory");
        fs::write(&config, "zeta = \"z\"\nalpha = \"a\"\n").expect("config");
        let loaded = load_catalog_config(&root).expect("catalog config");
        let names = loaded.keys().cloned().collect::<Vec<_>>();
        assert_eq!(names, vec!["alpha", "zeta"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn catalog_state_round_trip_preserves_semantic_fields() {
        let root = std::env::temp_dir().join(format!(
            "spectralang-r911-state-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let state = CatalogStateFile {
            version: 1,
            catalogs: vec![CatalogState {
                name: "alpha".to_string(),
                source: "local".to_string(),
                cache: ".spectra/catalogs/alpha".to_string(),
                resolved_rev: Some("a".repeat(40)),
                index_hash: "b".repeat(64),
                synced_at: "123".to_string(),
            }],
        };
        write_catalog_state(&root, state.clone()).expect("state write");
        let loaded = load_catalog_state(&root).expect("state read");
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.catalogs[0].name, "alpha");
        assert_eq!(loaded.catalogs[0].index_hash, "b".repeat(64));
        let _ = fs::remove_dir_all(root);
    }

    fn unique_git_policy_dir(label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "spectralang-gitref-{}-{}-{}",
            label, nonce, id
        ))
    }

    fn init_git_repo(path: &Path, package_name: &str) {
        fs::create_dir_all(path).expect("upstream dir");
        fs::write(
            path.join("spectra.toml"),
            format!("[project]\nname = \"{}\"\nversion = \"0.1.0\"\n", package_name),
        )
        .expect("upstream manifest");
        run_git(&["init"], path).expect("git init");
        run_git(&["config", "user.email", "test@example.com"], path).expect("git email");
        run_git(&["config", "user.name", "spectra-test"], path).expect("git name");
    }

    fn commit_all(path: &Path, message: &str) -> String {
        run_git(&["add", "."], path).expect("git add");
        run_git(
            &["commit", "--quiet", "--allow-empty", "-m", message],
            path,
        )
        .expect("git commit");
        git_output(&["rev-parse", "HEAD"], path).expect("git rev-parse")
    }

    fn write_consumer_manifest(root: &Path, upstream: &Path, floating: bool) {
        let upstream_text = upstream.to_string_lossy().replace('\\', "/");
        let opt_in = if floating {
            "\nallow-floating-git = true\n"
        } else {
            ""
        };
        fs::write(
            root.join("spectra.toml"),
            format!(
                "[project]\nname = \"consumer\"\nversion = \"0.1.0\"\n\n\
                 [dependencies.floatdemo]\nversion = \"0.1.0\"\ngit = \"{}\"{}\n",
                upstream_text, opt_in
            ),
        )
        .expect("consumer manifest");
    }

    #[test]
    fn floating_git_dependency_requires_a_fixed_ref() {
        let upstream = unique_git_policy_dir("upstream-a");
        init_git_repo(&upstream, "floatdemo");
        commit_all(&upstream, "initial");
        let root = unique_git_policy_dir("workspace-a");
        fs::create_dir_all(&root).expect("workspace dir");
        write_consumer_manifest(&root, &upstream, false);

        let error = fetch(&root, false, false).expect_err("floating HEAD must be rejected");
        let message = error.to_string();
        assert!(message.contains("stable ref"), "unexpected error: {}", message);
        assert!(message.contains("tag"), "error must suggest pinning: {}", message);

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(upstream);
    }

    #[test]
    fn allow_floating_git_resolves_head_and_records_warning_in_lock() {
        let upstream = unique_git_policy_dir("upstream-b");
        init_git_repo(&upstream, "floatdemo");
        let head = commit_all(&upstream, "initial");
        let root = unique_git_policy_dir("workspace-b");
        fs::create_dir_all(&root).expect("workspace dir");
        write_consumer_manifest(&root, &upstream, true);

        let lock_path = fetch(&root, false, false).expect("floating install must succeed");
        let lock_text = fs::read_to_string(&lock_path).expect("lockfile read");
        assert!(lock_text.contains("git_ref = \"HEAD\""), "lock: {}", lock_text);
        assert!(lock_text.contains(&head), "lock must record resolved rev: {}", lock_text);
        assert!(
            lock_text.contains("mutable git HEAD"),
            "lock must warn about floating checkout: {}",
            lock_text
        );

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(upstream);
    }

    #[test]
    fn existing_lock_anchors_git_dependency_without_manifest_ref() {
        let upstream = unique_git_policy_dir("upstream-c");
        init_git_repo(&upstream, "floatdemo");
        let first = commit_all(&upstream, "initial");

        // First resolve opts into a floating checkout and records the anchor.
        let root = unique_git_policy_dir("workspace-c");
        fs::create_dir_all(&root).expect("workspace dir");
        write_consumer_manifest(&root, &upstream, true);
        fetch(&root, false, false).expect("initial floating resolve");

        // Drop caches so the next resolve must reinstall from the remote.
        fs::remove_dir_all(root.join(".spectra")).expect("drop caches");
        // Upstream advances; the locked revision must not move.
        let second = commit_all(&upstream, "advance");

        // Manifest no longer allows floating; only the lock anchors the install.
        write_consumer_manifest(&root, &upstream, false);
        let lock_path = fetch(&root, false, false).expect("locked resolve must succeed");
        let lock_text = fs::read_to_string(&lock_path).expect("lockfile read");
        assert!(
            lock_text.contains(&first) && !lock_text.contains(&second),
            "lock must stay anchored at the previously resolved rev:\nfirst={}\nsecond={}\n{}",
            first,
            second,
            lock_text
        );
        assert!(
            !lock_text.contains("mutable git HEAD"),
            "anchored installs must not carry the floating warning: {}",
            lock_text
        );

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(upstream);
    }
}
