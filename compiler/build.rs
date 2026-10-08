use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn collect_sources(current: &Path, output: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(current)
        .unwrap_or_else(|error| {
            panic!(
                "cannot read stdlib source directory {}: {error}",
                current.display()
            )
        })
        .map(|entry| entry.expect("stdlib directory entry"))
        .map(|entry| entry.path())
        .collect();
    entries.sort();

    for path in entries {
        println!("cargo:rerun-if-changed={}", path.display());
        if path.is_dir() {
            collect_sources(&path, output);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "spectra")
        {
            output.push(path);
        }
    }
}

fn module_name(root: &Path, path: &Path) -> String {
    let relative = path
        .strip_prefix(root)
        .unwrap_or_else(|_| panic!("{} is outside {}", path.display(), root.display()));
    let mut segments = vec!["std".to_string()];
    for component in relative.with_extension("").components() {
        let value = component.as_os_str().to_string_lossy();
        let normalized = value.replace('-', "_");
        assert!(
            !normalized.is_empty()
                && normalized
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_'),
            "invalid std module path component '{value}' in {}",
            path.display()
        );
        segments.push(normalized);
    }
    segments.join(".")
}

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let repo_root = manifest_dir.parent().unwrap_or(&manifest_dir);
    let stdlib_root = repo_root.join("stdlib");
    let source_root = stdlib_root.join("src");
    println!("cargo:rerun-if-changed={}", source_root.display());

    let mut sources = Vec::new();
    collect_sources(&source_root, &mut sources);
    sources.sort();

    let mut modules = Vec::with_capacity(sources.len());
    for path in sources {
        let module = module_name(&source_root, &path);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let relative = path
            .strip_prefix(&stdlib_root)
            .unwrap_or_else(|_| panic!("{} is outside stdlib", path.display()))
            .to_string_lossy()
            .replace('\\', "/");
        modules.push((
            module,
            path.to_string_lossy().replace('\\', "/"),
            format!("stdlib/{relative}"),
            source,
        ));
    }

    modules.sort_by(|left, right| left.0.cmp(&right.0));

    for pair in modules.windows(2) {
        assert_ne!(pair[0].0, pair[1].0, "duplicate std module {}", pair[0].0);
    }

    let entries = modules
        .iter()
        .map(|(module, absolute_path, display_path, source)| {
            format!(
                "EmbeddedStdlibSource {{ module: {module:?}, path: {absolute_path:?}, display_path: {display_path:?}, source: {source:?} }}"
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    let mut fingerprint = 0xcbf29ce484222325_u64;
    for (module, _, _, source) in &modules {
        for byte in module
            .bytes()
            .chain([0])
            .chain(source.bytes())
            .chain([0xff])
        {
            fingerprint ^= u64::from(byte);
            fingerprint = fingerprint.wrapping_mul(0x100000001b3);
        }
    }
    let bundle_id = format!("fnv1a64-{fingerprint:016x}");
    let generated = format!(
        "pub const EMBEDDED_STDLIB_BUNDLE_ID: &str = {bundle_id:?};\npub static EMBEDDED_STDLIB_SOURCES: &[EmbeddedStdlibSource] = &[\n{entries}\n];\n"
    );
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::write(out_dir.join("embedded_stdlib.rs"), generated)
        .expect("write embedded standard-library source index");
}
