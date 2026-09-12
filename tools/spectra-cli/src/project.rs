use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectSourceEntry {
    pub path: PathBuf,
    pub package_name: Option<String>,
    pub package_root: Option<PathBuf>,
}

impl ProjectSourceEntry {
    pub fn plain(path: PathBuf) -> Self {
        Self {
            path,
            package_name: None,
            package_root: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedModule {
    pub name: String,
    pub path: PathBuf,
    pub imports: Vec<String>,
    pub package_name: Option<String>,
    pub package_root: Option<PathBuf>,
}

#[derive(Debug)]
pub struct ProjectPlan {
    modules: Vec<ResolvedModule>,
}

impl ProjectPlan {
    pub fn build(entries: Vec<PathBuf>) -> Result<Self, ProjectError> {
        Self::build_with_sources(entries.into_iter().map(ProjectSourceEntry::plain).collect())
    }

    pub fn build_with_sources(entries: Vec<ProjectSourceEntry>) -> Result<Self, ProjectError> {
        if entries.is_empty() {
            return Ok(Self {
                modules: Vec::new(),
            });
        }

        let mut discovered: BTreeMap<PathBuf, ProjectSourceEntry> = BTreeMap::new();
        let mut entry_set: HashSet<PathBuf> = HashSet::new();

        for entry in entries {
            let normalized = normalize_path(&entry.path).map_err(|error| ProjectError::Io {
                path: entry.path.clone(),
                error,
            })?;
            entry_set.insert(normalized.clone());
            collect_sources(&normalized, &entry, &mut discovered)?;
        }

        if discovered.is_empty() {
            return Err(ProjectError::NoSourcesFound(
                entry_set.into_iter().collect(),
            ));
        }

        let mut modules = Vec::new();
        let mut module_map: HashMap<String, ModuleOrigin> = HashMap::new();
        let mut package_roots = BTreeMap::new();

        for (path, source_entry) in discovered {
            if let (Some(package_name), Some(package_root)) =
                (&source_entry.package_name, &source_entry.package_root)
            {
                package_roots
                    .entry(package_name.clone())
                    .or_insert_with(|| package_root.clone());
            }
            let source = fs::read_to_string(&path).map_err(|error| ProjectError::Io {
                path: path.clone(),
                error,
            })?;
            let module = extract_module_name(&source).unwrap_or_else(|| {
                // No explicit `module <name>` declaration — derive the name
                // from the file stem so that single-file scripts and simple
                // projects work without a boilerplate header.
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "main".to_string())
            });

            let origin = ModuleOrigin {
                path: path.clone(),
                package_name: source_entry.package_name.clone(),
                package_root: source_entry.package_root.clone(),
            };

            if let Some(existing) = module_map.get(&module) {
                return Err(ProjectError::DuplicateModule {
                    module,
                    existing: existing.clone(),
                    duplicate: origin,
                });
            }

            let imports = extract_imports(&source);
            module_map.insert(module.clone(), origin);
            modules.push(ResolvedModule {
                name: module,
                path,
                imports,
                package_name: source_entry.package_name,
                package_root: source_entry.package_root,
            });
        }

        let missing = collect_missing_dependencies(&modules, &module_map, &package_roots);
        if !missing.is_empty() {
            return Err(ProjectError::MissingDependencies(missing));
        }

        let order = topological_order(&modules)?;
        let ordered_modules = order
            .into_iter()
            .map(|index| modules[index].clone())
            .collect();

        Ok(Self {
            modules: ordered_modules,
        })
    }

    pub fn modules(&self) -> &[ResolvedModule] {
        &self.modules
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ModuleOrigin {
    path: PathBuf,
    package_name: Option<String>,
    package_root: Option<PathBuf>,
}

#[derive(Debug)]
pub enum ProjectError {
    Io {
        path: PathBuf,
        error: io::Error,
    },
    /// Kept for potential programmatic use; the CLI itself now derives a module
    /// name from the file stem when no `module` declaration is present.
    #[allow(dead_code)]
    MissingModuleHeader {
        path: PathBuf,
    },
    DuplicateModule {
        module: String,
        existing: ModuleOrigin,
        duplicate: ModuleOrigin,
    },
    MissingDependencies(Vec<MissingDependency>),
    CyclicDependency(Vec<String>),
    NoSourcesFound(Vec<PathBuf>),
}

#[derive(Debug)]
pub struct MissingDependency {
    pub module: String,
    pub missing: Vec<String>,
    pub package_name: Option<String>,
    pub package_root: Option<PathBuf>,
    pub missing_package: Option<String>,
    pub missing_package_root: Option<PathBuf>,
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectError::Io { path, error } => {
                write!(f, "failed to read '{}': {}", path.display(), error)
            }
            ProjectError::MissingModuleHeader { path } => {
                write!(
                    f,
                    "file '{}' is missing a module declaration\n\
                     help: add 'module <name>' as the first non-comment line of the file",
                    path.display()
                )
            }
            ProjectError::DuplicateModule {
                module,
                existing,
                duplicate,
            } => {
                write!(
                    f,
                    "module '{}' is declared by two package sources:\n  \
                     first:  package '{}' at '{}' (root '{}')\n  \
                     second: package '{}' at '{}' (root '{}')\n\
                     help: each module name must be unique within a project",
                    module,
                    display_package_name(existing.package_name.as_deref()),
                    existing.path.display(),
                    display_package_root(existing.package_root.as_deref()),
                    display_package_name(duplicate.package_name.as_deref()),
                    duplicate.path.display(),
                    display_package_root(duplicate.package_root.as_deref())
                )
            }
            ProjectError::MissingDependencies(items) => {
                writeln!(f, "unresolved imports:")?;
                for item in items {
                    for missing in &item.missing {
                        let package_context = item
                            .package_name
                            .as_deref()
                            .map(|name| {
                                format!(
                                    " in package '{}' (root '{}')",
                                    name,
                                    display_package_root(item.package_root.as_deref())
                                )
                            })
                            .unwrap_or_default();
                        let requested_context = item
                            .missing_package
                            .as_deref()
                            .zip(item.missing_package_root.as_deref())
                            .map(|(name, root)| {
                                format!(" (package '{}' source: {})", name, root.display())
                            })
                            .unwrap_or_default();
                        writeln!(
                            f,
                            "  - module '{}'{} imports '{}', but no file declaring 'module {}' was found{}",
                            item.module, package_context, missing, missing, requested_context
                        )?;
                    }
                }
                write!(
                    f,
                    "help: create a source file with 'module <name>' for each missing module"
                )
            }
            ProjectError::CyclicDependency(cycle) => {
                write!(
                    f,
                    "cyclic dependency detected: {}\n\
                     help: restructure your modules to break the circular import chain",
                    cycle.join(" -> ")
                )
            }
            ProjectError::NoSourcesFound(paths) => {
                writeln!(f, "no Spectra source files found in the given path(s):")?;
                for path in paths {
                    writeln!(f, "  - {}", path.display())?;
                }
                write!(
                    f,
                    "help: source files must have a .spectra or .spc extension"
                )
            }
        }
    }
}

impl std::error::Error for ProjectError {}

fn collect_sources(
    path: &Path,
    origin: &ProjectSourceEntry,
    out: &mut BTreeMap<PathBuf, ProjectSourceEntry>,
) -> Result<(), ProjectError> {
    let metadata = fs::metadata(path).map_err(|error| ProjectError::Io {
        path: path.to_path_buf(),
        error,
    })?;

    if metadata.is_dir() {
        if should_skip_directory(path) {
            return Ok(());
        }
        for entry in fs::read_dir(path).map_err(|error| ProjectError::Io {
            path: path.to_path_buf(),
            error,
        })? {
            let entry = entry.map_err(|error| ProjectError::Io {
                path: path.to_path_buf(),
                error,
            })?;
            let child_path = entry.path();
            collect_sources(&child_path, origin, out)?;
        }
    } else if metadata.is_file() && is_source_file(path) {
        let normalized = normalize_path(path).map_err(|error| ProjectError::Io {
            path: path.to_path_buf(),
            error,
        })?;
        out.entry(normalized).or_insert_with(|| origin.clone());
    }

    Ok(())
}

fn is_source_file(path: &Path) -> bool {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("spectra") => true,
        Some(ext) if ext.eq_ignore_ascii_case("spc") => true,
        _ => false,
    }
}

fn should_skip_directory(path: &Path) -> bool {
    match path.file_name().and_then(|name| name.to_str()) {
        Some(name) if name.starts_with('.') => true,
        Some("target" | "build" | "dist" | "out") => true,
        _ => false,
    }
}

fn normalize_path(path: &Path) -> Result<PathBuf, io::Error> {
    fs::canonicalize(path)
}

fn extract_module_name(source: &str) -> Option<String> {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("module ") {
            let rest = rest.split("//").next().unwrap_or(rest).trim();
            let rest = rest.trim_end_matches(';').trim();
            if rest.is_empty() {
                return None;
            }
            return Some(rest.to_string());
        }

        // Stop scanning once we reach non-comment, non-module tokens.
        break;
    }
    None
}

fn extract_imports(source: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();

        // The canonical form is `public from path import name`.
        let trimmed = trimmed.strip_prefix("public ").unwrap_or(trimmed);

        if let Some(rest) = trimmed.strip_prefix("from ") {
            // `from path.to.module import name, other as alias`
            if let Some((module_name, _)) = rest.split_once(" import ") {
                let module_name = module_name.trim();
                if !module_name.is_empty() {
                    imports.push(module_name.to_string());
                }
            }
            continue;
        }

        if !trimmed.starts_with("import ") {
            continue;
        }

        let rest = &trimmed["import ".len()..];
        let rest = rest.split("//").next().unwrap_or(rest).trim();
        let rest = rest.trim_end_matches(';').trim();
        if rest.is_empty() {
            continue;
        }

        // `import { a, b } from path.to.module`
        let module_name = if rest.starts_with('{') {
            if let Some(from_pos) = rest.find("} from ") {
                let after_from = rest[from_pos + "} from ".len()..].trim();
                after_from.split_whitespace().next().unwrap_or("").trim()
            } else {
                continue;
            }
        } else {
            // `import path.to.module` or `import path.to.module as alias`
            if let Some((module, _alias)) = rest.split_once(" as ") {
                module.trim()
            } else {
                rest
            }
        };

        if !module_name.is_empty() {
            imports.push(module_name.to_string());
        }
    }
    imports
}

fn collect_missing_dependencies(
    modules: &[ResolvedModule],
    module_map: &HashMap<String, ModuleOrigin>,
    package_roots: &BTreeMap<String, PathBuf>,
) -> Vec<MissingDependency> {
    let mut missing = Vec::new();

    for module in modules {
        let unresolved: Vec<String> = module
            .imports
            .iter()
            .filter(|dep| !is_builtin_module(dep) && !module_map.contains_key(*dep))
            .cloned()
            .collect();

        if !unresolved.is_empty() {
            let package_match = unresolved
                .iter()
                .filter_map(|dependency| {
                    package_roots
                        .keys()
                        .filter(|package| {
                            dependency == *package
                                || dependency.starts_with(&format!("{}.", package))
                        })
                        .max_by_key(|package| package.len())
                        .cloned()
                })
                .next();
            let package_match_root = package_match
                .as_ref()
                .and_then(|package| package_roots.get(package).cloned());
            missing.push(MissingDependency {
                module: module.name.clone(),
                missing: unresolved,
                package_name: module.package_name.clone(),
                package_root: module.package_root.clone(),
                missing_package: package_match,
                missing_package_root: package_match_root,
            });
        }
    }

    missing
}

fn display_package_name(name: Option<&str>) -> &str {
    name.unwrap_or("<unscoped>")
}

fn display_package_root(root: Option<&Path>) -> String {
    root.map(|path| path.display().to_string())
        .unwrap_or_else(|| "<unknown>".to_string())
}

fn is_builtin_module(name: &str) -> bool {
    name == "std"
        || name.starts_with("std.")
        || name == "spectra.std"
        || name.starts_with("spectra.std.")
}

fn topological_order(modules: &[ResolvedModule]) -> Result<Vec<usize>, ProjectError> {
    #[derive(Copy, Clone, PartialEq)]
    enum VisitState {
        Unvisited,
        Visiting,
        Visited,
    }

    let mut state = vec![VisitState::Unvisited; modules.len()];
    let mut order = Vec::with_capacity(modules.len());
    let mut stack = Vec::new();
    let name_to_index: HashMap<&str, usize> = modules
        .iter()
        .enumerate()
        .map(|(index, module)| (module.name.as_str(), index))
        .collect();

    fn dfs(
        index: usize,
        modules: &[ResolvedModule],
        state: &mut [VisitState],
        order: &mut Vec<usize>,
        stack: &mut Vec<String>,
        name_to_index: &HashMap<&str, usize>,
    ) -> Result<(), ProjectError> {
        if state[index] == VisitState::Visiting {
            let module = &modules[index];
            stack.push(module.name.clone());
            return Err(ProjectError::CyclicDependency(stack.clone()));
        }
        if state[index] == VisitState::Visited {
            return Ok(());
        }

        state[index] = VisitState::Visiting;
        stack.push(modules[index].name.clone());

        for dep in &modules[index].imports {
            if is_builtin_module(dep) {
                continue;
            }
            if let Some(&dep_index) = name_to_index.get(dep.as_str()) {
                dfs(dep_index, modules, state, order, stack, name_to_index)?;
            }
        }

        stack.pop();
        state[index] = VisitState::Visited;
        order.push(index);
        Ok(())
    }

    for index in 0..modules.len() {
        dfs(
            index,
            modules,
            &mut state,
            &mut order,
            &mut stack,
            &name_to_index,
        )?;
    }

    // Post-order DFS already produces dependencies-first topological order.
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempProject {
        root: PathBuf,
    }

    impl TempProject {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos();
            let root = std::env::temp_dir().join(format!("spectra-r906-{label}-{nonce}"));
            fs::create_dir_all(&root).expect("create temp project");
            Self { root }
        }

        fn source(&self, package: &str, file: &str, contents: &str) -> ProjectSourceEntry {
            let root = self.root.join(package);
            let path = root.join("src").join(file);
            fs::create_dir_all(path.parent().expect("source parent")).expect("create source");
            fs::write(&path, contents).expect("write source");
            ProjectSourceEntry {
                path: root.join("src"),
                package_name: Some(package.to_string()),
                package_root: Some(root),
            }
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn package_origins_are_preserved_and_order_is_dependency_first() {
        let temp = TempProject::new("origins");
        let app = temp.source("app", "main.spectra", "module app.main\nimport lib.core\n");
        let lib = temp.source("lib", "core.spectra", "module lib.core\n");

        let plan = ProjectPlan::build_with_sources(vec![app, lib]).expect("build plan");
        assert_eq!(plan.modules()[0].name, "lib.core");
        assert_eq!(plan.modules()[0].package_name.as_deref(), Some("lib"));
        assert_eq!(plan.modules()[1].package_name.as_deref(), Some("app"));
    }

    #[test]
    fn canonical_named_imports_and_reexports_are_dependencies() {
        let temp = TempProject::new("named-imports");
        let app = temp.source(
            "app",
            "main.spectra",
            "module app.main\nfrom app.prelude import total\n",
        );
        let prelude = temp.source(
            "app",
            "prelude.spectra",
            "module app.prelude\npublic from app.support import answer\n",
        );
        let support = temp.source(
            "app",
            "support.spectra",
            "module app.support\npublic func answer() returns int {\n return 42\n}\n",
        );

        let plan =
            ProjectPlan::build_with_sources(vec![app, prelude, support]).expect("build plan");
        let names: Vec<_> = plan
            .modules()
            .iter()
            .map(|module| module.name.as_str())
            .collect();
        assert_eq!(names, vec!["app.support", "app.prelude", "app.main"]);
    }

    #[test]
    fn duplicate_module_diagnostic_contains_both_packages_and_roots() {
        let temp = TempProject::new("duplicate");
        let first = temp.source("alpha", "same.spectra", "module shared.same\n");
        let second = temp.source("beta", "same.spectra", "module shared.same\n");

        let error = ProjectPlan::build_with_sources(vec![first, second])
            .expect_err("duplicate module must fail");
        let text = error.to_string();
        assert!(text.contains("shared.same"));
        assert!(text.contains("alpha"));
        assert!(text.contains("beta"));
        assert!(text.contains("root"));
    }

    #[test]
    fn missing_import_diagnostic_identifies_known_package_source() {
        let temp = TempProject::new("missing");
        let app = temp.source(
            "app",
            "main.spectra",
            "module app.main\nimport lib.missing\n",
        );
        let lib = temp.source("lib", "core.spectra", "module lib.core\n");

        let error = ProjectPlan::build_with_sources(vec![app, lib])
            .expect_err("missing package module must fail");
        let text = error.to_string();
        assert!(text.contains("lib.missing"));
        assert!(text.contains("package 'lib' source:"));
        assert!(text.contains("package 'app'"));
    }
}
