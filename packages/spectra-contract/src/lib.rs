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
    /// Anonymous parameters split from the semantic signature (`arg0`, ...).
    #[serde(default)]
    pub params: Vec<CatalogParam>,
    /// Semantic return type spelled by the compiler signature.
    #[serde(default)]
    pub returns: String,
    /// Return type rendered with the catalog IR type grammar.
    #[serde(default)]
    pub ir_return: String,
    /// Whether the midend host descriptor keeps the returned value.
    #[serde(default)]
    pub returns_value: bool,
    /// Host-call target in `packages/spectra-api/src/host_calls.rs`; only
    /// `std.api.*` functions own a host-call symbol.
    #[serde(default)]
    pub rust_symbol: String,
    /// Cargo feature gating the host call (`http3`), empty when unconditional.
    #[serde(default)]
    pub cfg_feature: String,
    /// Write-side effect classification (`effects` contains `mutation`).
    #[serde(default)]
    pub sink: bool,
    /// Scope predicate keys consumed by governance; empty when scoped globally.
    #[serde(default)]
    pub scope_keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CatalogParam {
    pub name: String,
    pub ty: String,
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

    /// Compiler aliases resolve to a sibling host call, so they own no
    /// `HostCallSpec` and carry no `rust_symbol` (mirrored in
    /// `scripts/validate_r3007_stdlib_contract.py`).
    const HOST_CALL_ALIASES: &[&str] = &["std.api.routing.router"];

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
    fn function_entries_carry_lowering_and_host_metadata() {
        let raw: toml::Value = toml::from_str(CATALOG_SOURCE).expect("catalog must be valid TOML");
        let raw_entries = raw
            .get("entry")
            .and_then(toml::Value::as_array)
            .expect("catalog must declare entries");
        let typed = catalog();
        assert_eq!(raw_entries.len(), typed.entry.len());
        for (raw_entry, item) in raw_entries.iter().zip(typed.entry.iter()) {
            // `returns_value` deserializes with a default, so presence is
            // asserted against the raw TOML the generator writes.
            if item.kind != "function" {
                continue;
            }
            for key in ["returns", "ir_return", "returns_value"] {
                assert!(
                    raw_entry.get(key).is_some(),
                    "{} must declare {key}",
                    item.path
                );
            }
            assert!(!item.returns.is_empty(), "{} must declare returns", item.path);
            assert!(
                !item.ir_return.is_empty(),
                "{} must declare ir_return",
                item.path
            );
            // The API host-call table is generated from `rust_symbol`, so every
            // `std.api.*` function must name its Rust target.
            if item.path.starts_with("std.api.") && !HOST_CALL_ALIASES.contains(&item.path.as_str()) {
                assert!(
                    !item.rust_symbol.is_empty(),
                    "{} must declare rust_symbol",
                    item.path
                );
            }
            assert_eq!(
                item.sink,
                item.effects.iter().any(|effect| effect == "mutation"),
                "{} sink must follow the effects classification",
                item.path
            );
            for key in &item.scope_keys {
                assert!(!key.is_empty(), "{} has an empty scope key", item.path);
            }
        }
    }

    #[test]
    fn generated_bindings_match_catalog_entries() {
        for item in catalog().entry {
            assert_eq!(binding(&item.path), Some(item.binding.as_str()));
        }
    }
}
