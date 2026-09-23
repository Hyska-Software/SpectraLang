use spectra_compiler::{ast::Item, Lexer, Parser};
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

/// Advance `index` past whitespace and both comment forms (`//` line and
/// `/* */` block), i.e. past everything the lexer would discard before the
/// next token.
fn skip_trivia(bytes: &[u8], index: &mut usize) {
    let len = bytes.len();
    loop {
        while *index < len && bytes[*index].is_ascii_whitespace() {
            *index += 1;
        }
        if *index + 1 < len && bytes[*index] == b'/' && bytes[*index + 1] == b'/' {
            *index += 2;
            while *index < len && bytes[*index] != b'\n' {
                *index += 1;
            }
            continue;
        }
        if *index + 1 < len && bytes[*index] == b'/' && bytes[*index + 1] == b'*' {
            *index += 2;
            while *index + 1 < len && !(bytes[*index] == b'*' && bytes[*index + 1] == b'/') {
                *index += 1;
            }
            // Consume the closing delimiter when present; a dangling `/*`
            // runs to EOF and the lexer reports the real error later.
            *index = (*index + 2).min(len);
            continue;
        }
        return;
    }
}

fn ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn ident_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Extract the declared module name from a leading `module <name>`
/// declaration, mirroring how the parser reads it: the `module` keyword
/// followed by any whitespace (including newlines and tabs), with `//` line
/// comments and `/* */` block comments skipped everywhere trivia appears,
/// and a name of identifier segments joined by `.`.
///
/// This is the single source of truth for module-header detection: string
/// literals and comments are never scanned, `modulex` is not the keyword,
/// and an incomplete declaration reports `None` so the build paths fall
/// back to the file-stem name and let compilation report the real error.
fn extract_module_name(source: &str) -> Option<String> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut index = 0;
    skip_trivia(bytes, &mut index);

    if index >= len || !source[index..].starts_with("module") {
        return None;
    }
    let mut cursor = index + "module".len();
    if cursor < len && ident_char(bytes[cursor]) {
        // `modulex` / `module_...` are ordinary identifiers, not the keyword.
        return None;
    }

    skip_trivia(bytes, &mut cursor);
    if cursor >= len || !ident_start(bytes[cursor]) {
        // The keyword must be followed by a module name.
        return None;
    }
    let name_start = cursor;
    while cursor < len && ident_char(bytes[cursor]) {
        cursor += 1;
    }
    let mut name = source[name_start..cursor].to_string();

    // Dotted segments: `module app.main`.
    loop {
        let mut probe = cursor;
        skip_trivia(bytes, &mut probe);
        if probe >= len || bytes[probe] != b'.' {
            break;
        }
        probe += 1;
        skip_trivia(bytes, &mut probe);
        if probe >= len || !ident_start(bytes[probe]) {
            break;
        }
        let segment_start = probe;
        while probe < len && ident_char(bytes[probe]) {
            probe += 1;
        }
        name.push('.');
        name.push_str(&source[segment_start..probe]);
        cursor = probe;
    }
    Some(name)
}

/// Returns `true` when the source already opens with a real `module <name>`
/// declaration; used to decide whether the build paths must prepend a
/// synthetic header. Shared by the JIT plan, the AOT project build, and the
/// async package-test discovery so all three agree on what a header is.
pub fn source_has_module_decl(source: &str) -> bool {
    extract_module_name(source).is_some()
}

/// `Some(defines_main)` when the source lexes and parses cleanly, `None`
/// when it does not. Callers enforcing an entry-point rule must treat
/// `None` as inconclusive and let compilation report the real errors
/// instead of claiming the file has no `main`.
pub fn scan_source_main(source: &str) -> Option<bool> {
    let tokens = Lexer::new(source).tokenize().ok()?;
    let module = Parser::new(tokens).parse().ok()?;
    Some(module.items.iter().any(|item| {
        matches!(item, Item::Function(function) if function.name == "main")
    }))
}

/// Returns `true` when the source parses and declares a top-level `main`.
/// Lex/parse failures report `false`; use [`scan_source_main`] when the
/// distinction matters.
pub fn source_defines_main(source: &str) -> bool {
    scan_source_main(source).unwrap_or(false)
}

/// Shared diagnostic for a project without an entry point.
pub fn missing_main_message() -> &'static str {
    "no entry point 'main' found; define a 'public func main() returns int' function"
}

fn multiple_mains_message(count: usize, mains: &[PathBuf]) -> String {
    let files = mains
        .iter()
        .map(|path| format!("'{}'", path.display()))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "a project must contain exactly one `main` function; found {} in: {}\n\
         help: keep the entry point in a single module so JIT execution and AOT executables agree",
        count, files
    )
}

/// Result of scanning a compilation plan for `main` entry points before any
/// code is generated. Shared by the JIT execution paths (`run`,
/// `compile --run`) and the AOT executable path (`compile --emit-exe`) so
/// both enforce the same single-entry rule.
#[derive(Debug, Default)]
pub struct EntryPointScan {
    /// Modules whose effective source parses and defines a top-level `main`.
    mains: Vec<PathBuf>,
    /// Modules whose source could not be read or did not parse. A zero
    /// `main` count is inconclusive while this is non-empty.
    unreliable: Vec<PathBuf>,
}

impl EntryPointScan {
    /// The exactly-one-`main` rule shared by every path that executes a
    /// program or emits an executable:
    ///
    /// - `Err(message)` — the plan definitively violates the rule (more than
    ///   one `main`, or none at all while every module scanned cleanly);
    /// - `Ok(Some(path))` — exactly one module defines `main`;
    /// - `Ok(None)` — a zero count is inconclusive because some module could
    ///   not be read or did not parse; compilation reports those errors and
    ///   the caller proceeds without an entry-point rejection.
    pub fn single_main(&self) -> Result<Option<PathBuf>, String> {
        match self.mains.len() {
            0 if self.unreliable.is_empty() => Err(missing_main_message().to_string()),
            0 => Ok(None),
            1 => Ok(Some(self.mains[0].clone())),
            count => Err(multiple_mains_message(count, &self.mains)),
        }
    }
}

/// Scan every module of `plan` for `main`, reading the same effective source
/// the build paths compile: the on-disk text when it declares a module
/// header, the synthetic `module <name>` header otherwise.
pub fn scan_entry_points(plan: &ProjectPlan) -> EntryPointScan {
    let mut scan = EntryPointScan::default();
    for module in &plan.modules {
        let source = match fs::read_to_string(&module.path) {
            Ok(source) => source,
            Err(_) => {
                // Unreadable right now: inconclusive. The compile loop
                // reports the I/O error with its own diagnostic.
                scan.unreliable.push(module.path.clone());
                continue;
            }
        };
        let owned;
        let effective = if source_has_module_decl(&source) {
            source.as_str()
        } else {
            owned = format!("module {}\n{}", module.name, source);
            owned.as_str()
        };
        match scan_source_main(effective) {
            Some(true) => scan.mains.push(module.path.clone()),
            Some(false) => {}
            None => scan.unreliable.push(module.path.clone()),
        }
    }
    scan
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

    // --- Module header detection (shared source_has_module_decl) ---

    #[test]
    fn module_header_accepts_any_whitespace_separator() {
        // The lexer accepts any whitespace between the keyword and the name,
        // so the header detector must too (a literal space is not enough).
        assert_eq!(extract_module_name("module\nname"), Some("name".to_string()));
        assert_eq!(extract_module_name("module\tname"), Some("name".to_string()));
        assert_eq!(extract_module_name("module  name\n"), Some("name".to_string()));
        assert_eq!(
            extract_module_name("module app.main\n"),
            Some("app.main".to_string())
        );
        assert!(source_has_module_decl("module\nname"));
        assert!(source_has_module_decl("module\tname"));
    }

    #[test]
    fn module_header_after_block_comment_is_detected() {
        assert_eq!(
            extract_module_name("/* header */ module after_block\n"),
            Some("after_block".to_string())
        );
        assert_eq!(
            extract_module_name("/*\n multi-line\n*/\nmodule after_block\n"),
            Some("after_block".to_string())
        );
        assert_eq!(
            extract_module_name("// line comment\nmodule after_line\n"),
            Some("after_line".to_string())
        );
    }

    #[test]
    fn module_text_inside_string_literal_is_not_a_declaration() {
        assert_eq!(extract_module_name("module sneaky\n"), Some("sneaky".to_string()));
        // A string literal is a token, not trivia: nothing inside it counts.
        assert_eq!(extract_module_name("\"module sneaky\"\n"), None);
        assert_eq!(extract_module_name("let text = \"module sneaky\"\n"), None);
        assert!(!source_has_module_decl("let text = \"module sneaky\"\n"));
    }

    #[test]
    fn module_text_only_in_comment_is_not_a_declaration() {
        assert_eq!(extract_module_name("// module commented\n"), None);
        assert_eq!(extract_module_name("// module commented\nlet x = 1\n"), None);
        assert_eq!(extract_module_name("/* module commented */\n"), None);
        // Keyword boundary: `modulex` is an identifier, and a bare keyword
        // without a name is an incomplete declaration.
        assert_eq!(extract_module_name("modulex\n"), None);
        assert_eq!(extract_module_name("module"), None);
        assert_eq!(extract_module_name("module \n"), None);
    }

    // --- Entry-point counting (shared scan_entry_points) ---

    #[test]
    fn entry_scan_accepts_exactly_one_main() {
        let temp = TempProject::new("entry-one-main");
        let lib = temp.source("lib", "core.spectra", "module lib.core\n");
        let app = temp.source(
            "app",
            "main.spectra",
            "module app.main\npublic func main() returns int {\n    return 0\n}\n",
        );

        let plan = ProjectPlan::build_with_sources(vec![app, lib]).expect("build plan");
        let main = scan_entry_points(&plan)
            .single_main()
            .expect("exactly one main must be accepted")
            .expect("the main module path must be reported");
        assert!(main.ends_with("main.spectra"));
    }

    #[test]
    fn entry_scan_counts_main_in_headerless_sources() {
        let temp = TempProject::new("entry-headerless-main");
        // No module header: the scan must apply the same synthetic header
        // the build paths compile, or a valid headerless script would look
        // entry-point-free.
        let app = temp.source(
            "app",
            "main.spectra",
            "public func main() returns int {\n    return 0\n}\n",
        );

        let plan = ProjectPlan::build_with_sources(vec![app]).expect("build plan");
        let main = scan_entry_points(&plan)
            .single_main()
            .expect("headerless main must be detected")
            .expect("the main module path must be reported");
        assert!(main.ends_with("main.spectra"));
    }

    #[test]
    fn entry_scan_rejects_zero_and_multiple_mains() {
        let temp = TempProject::new("entry-count");

        let lib = temp.source("lib", "core.spectra", "module lib.core\n");
        let plan = ProjectPlan::build_with_sources(vec![lib]).expect("build plan");
        let error = scan_entry_points(&plan)
            .single_main()
            .expect_err("zero mains must be rejected");
        assert!(error.contains("no entry point 'main' found"), "got: {error}");

        let first = temp.source(
            "one",
            "main.spectra",
            "module one.main\npublic func main() returns int {\n    return 0\n}\n",
        );
        let second = temp.source(
            "two",
            "main.spectra",
            "module two.main\npublic func main() returns int {\n    return 1\n}\n",
        );
        let plan = ProjectPlan::build_with_sources(vec![first, second]).expect("build plan");
        let error = scan_entry_points(&plan)
            .single_main()
            .expect_err("two mains must be rejected");
        assert!(error.contains("exactly one `main`"), "got: {error}");
        assert!(error.contains("main.spectra"), "got: {error}");
    }

    #[test]
    fn entry_scan_defers_when_a_source_cannot_be_scanned() {
        let temp = TempProject::new("entry-unreliable");
        // Parses cleanly but has no main ... plus a source that cannot parse:
        // a zero count must be inconclusive so compilation reports the real
        // parse error instead of a bogus "no entry point" rejection.
        let lib = temp.source("lib", "core.spectra", "module lib.core\n");
        let broken = temp.source("broken", "broken.spectra", "module broken\nfunc ( {\n");

        let plan = ProjectPlan::build_with_sources(vec![lib, broken.clone()]).expect("build plan");
        let result = scan_entry_points(&plan)
            .single_main()
            .expect("inconclusive scan must not reject");
        assert!(result.is_none());

        // But multiple detected mains are authoritative even alongside an
        // unscannable module.
        let one = temp.source(
            "one",
            "main.spectra",
            "module one.main\npublic func main() returns int {\n    return 0\n}\n",
        );
        let two = temp.source(
            "two",
            "main.spectra",
            "module two.main\npublic func main() returns int {\n    return 1\n}\n",
        );
        let plan = ProjectPlan::build_with_sources(vec![one, two, broken]).expect("build plan");
        let error = scan_entry_points(&plan)
            .single_main()
            .expect_err("two mains must still be rejected");
        assert!(error.contains("exactly one `main`"), "got: {error}");
    }
}
