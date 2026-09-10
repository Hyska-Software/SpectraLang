//! Anti-drift contract tests between the four hand-maintained copies of the
//! stdlib/API surface:
//!
//! 1. `spectra_api::HOST_CALLS` (packages/spectra-api/src/host_calls.rs)
//! 2. the embedded stdlib catalog (`spectra_contract::catalog()`, sourced from
//!    packages/spectra-contract/catalog/stdlib.toml)
//! 3. the mid-end `std.api` lowering table
//!    (`spectra_midend::lowering::std_api_host_call_target`)
//! 4. the compiler's manual builtin tables
//!    (`STD_API_PUBLIC_FUNCTIONS` / `STD_API_PUBLIC_TYPES` /
//!    `STD_API_MODULE_PATHS`)
//!
//! Any copy drifting from the code-registered reality must fail here.

use spectra_api::HOST_CALLS;
use spectra_compiler::semantic::builtin_modules::{
    builtin_contract_symbols, STD_API_MODULE_PATHS, STD_API_PUBLIC_FUNCTIONS, STD_API_PUBLIC_TYPES,
};
use spectra_contract::catalog;

/// Every host call implemented by the API package must be catalogued with a
/// matching binding.
#[test]
fn every_host_call_is_catalogued() {
    let catalog = catalog();
    let mut bindings: Vec<&str> = catalog
        .entry
        .iter()
        .filter(|entry| entry.kind == "function")
        .map(|entry| entry.binding.as_str())
        .collect();
    bindings.sort_unstable();

    let mut missing: Vec<&str> = HOST_CALLS
        .iter()
        .map(|spec| spec.name)
        .filter(|name| bindings.binary_search(name).is_err())
        .collect();
    missing.sort_unstable();
    assert!(
        missing.is_empty(),
        "HOST_CALLS entries missing from the stdlib catalog: {missing:?}"
    );
}

/// Every `std.api.*` function in the catalog must resolve either to a host
/// call registered by this crate or to a runtime standard-library host.
#[test]
fn every_catalog_std_api_function_resolves_to_a_host() {
    // Make runtime-stdlib hosts resolvable for the `spectra.std.*` branch.
    spectra_api::register();

    let unresolved: Vec<String> = catalog()
        .entry
        .into_iter()
        .filter(|entry| entry.kind == "function" && entry.path.starts_with("std.api."))
        .filter(|entry| {
            let registered_here = HOST_CALLS.iter().any(|spec| spec.name == entry.binding);
            let runtime_stdlib = entry.binding.starts_with("spectra.std.")
                && spectra_runtime::ffi::lookup_host_function(&entry.binding).is_some();
            !registered_here && !runtime_stdlib
        })
        .map(|entry| format!("{} -> {}", entry.path, entry.binding))
        .collect();
    assert!(
        unresolved.is_empty(),
        "catalog std.api functions with no host resolution: {unresolved:?}"
    );
}

/// Every binding reachable through the mid-end `std.api` lowering table must
/// exist in `HOST_CALLS`. Candidate (module, function) pairs are derived from
/// the compiler registry and the catalog, which are the only sources that can
/// reach the lowering at compile time.
#[test]
fn every_lowering_target_exists_in_host_calls() {
    let host_names: Vec<&str> = HOST_CALLS.iter().map(|spec| spec.name).collect();

    let split_std_api_path = |path: &str| -> Option<(String, String)> {
        let rest = path.strip_prefix("std.api.")?;
        let (module, function) = rest.rsplit_once('.')?;
        Some((module.to_string(), function.to_string()))
    };

    let mut candidates: Vec<(String, String)> = Vec::new();
    for symbol in builtin_contract_symbols() {
        if symbol.kind == "function" {
            candidates.extend(split_std_api_path(&symbol.path));
        }
    }
    for entry in catalog().entry {
        if entry.kind == "function" {
            candidates.extend(split_std_api_path(&entry.path));
        }
    }

    let mut dangling: Vec<String> = Vec::new();
    for (module, function) in candidates {
        if let Some(target) = spectra_midend::lowering::std_api_host_call_target(&module, &function)
        {
            if !host_names.contains(&target) {
                dangling.push(format!("{module}.{function} -> {target}"));
            }
        }
    }
    assert!(
        dangling.is_empty(),
        "mid-end lowering targets missing from HOST_CALLS: {dangling:?}"
    );
}

/// The manual public-function/type tables must stay a subset of the symbols
/// derived from the actual builtin module registry.
#[test]
fn manual_public_tables_are_subset_of_registry_symbols() {
    let derived_symbols = builtin_contract_symbols();
    let derived: std::collections::HashSet<&str> = derived_symbols
        .iter()
        .map(|symbol| symbol.path.as_str())
        .collect();

    let stale_types: Vec<&str> = STD_API_PUBLIC_TYPES
        .iter()
        .map(|(path, _)| *path)
        .filter(|path| !derived.contains(path))
        .collect();
    assert!(
        stale_types.is_empty(),
        "STD_API_PUBLIC_TYPES entries absent from the builtin registry: {stale_types:?}"
    );

    let stale_functions: Vec<&str> = STD_API_PUBLIC_FUNCTIONS
        .iter()
        .map(|(path, _)| *path)
        .filter(|path| !derived.contains(path))
        .collect();
    assert!(
        stale_functions.is_empty(),
        "STD_API_PUBLIC_FUNCTIONS entries absent from the builtin registry: {stale_functions:?}"
    );
}

/// `STD_API_MODULE_PATHS` must cover every module the compiler registers under
/// `std.api` (including dotted submodules such as `db.sqlite`).
#[test]
fn module_paths_cover_registered_builtin_modules() {
    let declared: std::collections::HashSet<&str> = STD_API_MODULE_PATHS.iter().copied().collect();

    let registered_modules = builtin_contract_symbols();
    let unlisted: Vec<&str> = registered_modules
        .iter()
        .filter(|symbol| symbol.kind == "module")
        .map(|symbol| symbol.path.as_str())
        .filter(|path| path.starts_with("std.api.") && !declared.contains(path))
        .collect();
    assert!(
        unlisted.is_empty(),
        "registered std.api modules missing from STD_API_MODULE_PATHS: {unlisted:?}"
    );
}

/// Every `std.api.*` function exported by the compiler's builtin registry
/// must lower to a registered host call. A registry export without a
/// mid-end lowering target is a ghost surface: semantic analysis accepts
/// the call and codegen rejects it.
#[test]
fn every_registered_std_api_function_has_a_lowering_target() {
    let host_names: Vec<&str> = HOST_CALLS.iter().map(|spec| spec.name).collect();

    let mut ghosts: Vec<String> = Vec::new();
    for symbol in builtin_contract_symbols() {
        if symbol.kind != "function" || !symbol.path.starts_with("std.api.") {
            continue;
        }
        let rest = &symbol.path["std.api.".len()..];
        let Some((module, function)) = rest.rsplit_once('.') else {
            continue;
        };
        match spectra_midend::lowering::std_api_host_call_target(module, function) {
            Some(target) if host_names.contains(&target) => {}
            Some(target) => ghosts.push(format!(
                "{} -> {target} (lowering target is not a registered host call)",
                symbol.path
            )),
            None => ghosts.push(format!("{} (no lowering target)", symbol.path)),
        }
    }
    assert!(
        ghosts.is_empty(),
        "ghost std.api surface exported by the compiler registry: {ghosts:?}"
    );
}
