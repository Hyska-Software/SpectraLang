//! Dependency-light source of truth for public Spectra STD/API contracts.
//!
//! Compiler, runtime and API crates may add adapters around this crate, but
//! none of those adapters may introduce a second public symbol declaration.

use serde::Deserialize;

include!(concat!(env!("OUT_DIR"), "/catalog_generated.rs"));

pub const CATALOG_SOURCE: &str = include_str!("../catalog/stdlib.toml");

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CatalogFile {
    pub entry: Vec<CatalogEntry>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CatalogEntry {
    pub path: String,
    #[serde(default = "default_entry_kind")]
    pub kind: String,
    pub namespace: String,
    pub signature: String,
    pub abi: String,
    pub effects: Vec<String>,
    pub error_model: String,
    pub binding: String,
    pub maturity: String,
    pub owner: String,
    pub docs: String,
    pub fixture: String,
}

fn default_entry_kind() -> String {
    "function".to_string()
}

pub fn catalog() -> CatalogFile {
    toml::from_str(CATALOG_SOURCE).expect("embedded STD catalog must be valid TOML")
}

pub fn entry(path: &str) -> Option<CatalogEntry> {
    catalog().entry.into_iter().find(|item| item.path == path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_is_typed_and_unique() {
        let catalog = catalog();
        assert_eq!(catalog.entry.len(), CATALOG_ENTRY_COUNT);
        let paths: HashSet<_> = catalog
            .entry
            .iter()
            .map(|item| item.path.as_str())
            .collect();
        assert_eq!(paths.len(), catalog.entry.len());
        assert_eq!(paths.len(), CATALOG_PATHS.len());
        assert_eq!(paths.len(), CATALOG_BINDINGS.len());
    }

    #[test]
    fn every_entry_has_contract_metadata() {
        for item in catalog().entry {
            assert!(item.path.starts_with("std.") || item.path.starts_with("spectra.api."));
            assert!(!item.signature.is_empty());
            assert!(!item.abi.is_empty());
            assert!(!item.binding.is_empty());
            assert!(!item.owner.is_empty());
            assert!(!item.docs.is_empty());
            assert!(!item.fixture.is_empty());
        }
    }

    #[test]
    fn generated_bindings_match_catalog_entries() {
        for item in catalog().entry {
            assert_eq!(binding(&item.path), Some(item.binding.as_str()));
        }
    }
}
