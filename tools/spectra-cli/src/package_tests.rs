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
}
