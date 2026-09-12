pub fn discover_test_entries(workspace: &ResolvedWorkspace) -> Vec<PathBuf> {
    let mut entries = Vec::new();
    for package in &workspace.packages {
        let tests_dir = package.root.join("tests");
        if tests_dir.is_dir() {
            entries.extend(discovery::discover_sources(&[tests_dir]));
        }
    }
    entries
}

pub fn deprecation_warnings(workspace: &ResolvedWorkspace) -> Vec<String> {
    workspace
        .packages
        .iter()
        .filter_map(|package| package.release.deprecation_warning(&package.name))
        .collect()
}

