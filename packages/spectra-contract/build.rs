use serde::Deserialize;
use std::{collections::HashSet, env, fs, path::PathBuf};

#[derive(Debug, Deserialize)]
struct CatalogFile {
    entry: Vec<CatalogEntry>,
}

#[derive(Debug, Deserialize)]
struct CatalogEntry {
    path: String,
    #[serde(default = "default_entry_kind")]
    kind: String,
    binding: String,
}

fn default_entry_kind() -> String {
    "function".to_string()
}

fn binding_const_name(path: &str) -> String {
    let mut name = String::new();
    for ch in path.chars() {
        if ch.is_ascii_alphanumeric() {
            name.push(ch.to_ascii_uppercase());
        } else {
            name.push('_');
        }
    }
    name.push_str("_BINDING");
    name
}

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let catalog_path = manifest_dir.join("catalog").join("stdlib.toml");
    println!("cargo:rerun-if-changed={}", catalog_path.display());

    let source = fs::read_to_string(&catalog_path).expect("read stdlib catalog");
    let catalog: CatalogFile = toml::from_str(&source).expect("parse stdlib catalog");
    let mut paths = Vec::with_capacity(catalog.entry.len());
    let mut bindings = Vec::with_capacity(catalog.entry.len());
    let mut seen = HashSet::new();
    for entry in &catalog.entry {
        assert!(
            !entry.path.trim().is_empty(),
            "catalog path cannot be empty"
        );
        assert!(
            seen.insert(entry.path.clone()),
            "duplicate catalog path: {}",
            entry.path
        );
        assert!(
            !entry.binding.trim().is_empty(),
            "catalog binding cannot be empty: {}",
            entry.path
        );
        paths.push(entry.path.clone());
        bindings.push(entry.binding.clone());
    }

    let binding_pairs = paths
        .iter()
        .zip(bindings.iter())
        .map(|(path, binding)| format!("({path:?}, {binding:?})"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut generated_constant_names = HashSet::new();
    let binding_constants = paths
        .iter()
        .zip(bindings.iter())
        .zip(catalog.entry.iter())
        .filter(|((_, _), entry)| entry.kind == "function")
        .filter_map(|((path, binding), _)| {
            let constant_name = binding_const_name(path);
            if !generated_constant_names.insert(constant_name.clone()) {
                return None;
            }
            Some(format!("pub const {constant_name}: &str = {binding:?};"))
        })
        .collect::<Vec<_>>()
        .join("\n");

    let generated = format!(
        "pub const CATALOG_ENTRY_COUNT: usize = {};\npub const CATALOG_PATHS: &[&str] = &[{}];\npub const CATALOG_BINDINGS: &[(&str, &str)] = &[{}];\n{}\n\
         pub fn binding(path: &str) -> Option<&'static str> {{\n\
             match path {{\n{}\n                 _ => None,\n             }}\n         }}\n",
        paths.len(),
        paths
            .iter()
            .map(|path| format!("{path:?}"))
            .collect::<Vec<_>>()
            .join(", "),
        binding_pairs,
        binding_constants,
        paths
            .iter()
            .zip(bindings.iter())
            .map(|(path, binding)| format!("                {path:?} => Some({binding:?}),"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::write(out_dir.join("catalog_generated.rs"), generated).expect("write generated catalog");
}
