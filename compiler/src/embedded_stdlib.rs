//! Repository-owned `.spectra` modules embedded into the compiler at build time.

#[derive(Debug, Clone, Copy)]
pub struct EmbeddedStdlibSource {
    /// Canonical standard-library module name, such as `std.algorithms`.
    pub module: &'static str,
    /// Absolute path in the source checkout used for source maps and navigation.
    pub path: &'static str,
    /// Stable path relative to the repository root used in diagnostics.
    pub display_path: &'static str,
    pub source: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/embedded_stdlib.rs"));

/// Deterministic content identity for this compiler's embedded std source set.
pub fn embedded_stdlib_bundle_id() -> &'static str {
    EMBEDDED_STDLIB_BUNDLE_ID
}

/// All source modules in deterministic module-name order.
pub fn embedded_stdlib_sources() -> &'static [EmbeddedStdlibSource] {
    EMBEDDED_STDLIB_SOURCES
}

/// Find an embedded source module by its canonical `std.*` name.
pub fn embedded_stdlib_source(module: &str) -> Option<&'static EmbeddedStdlibSource> {
    EMBEDDED_STDLIB_SOURCES
        .binary_search_by_key(&module, |source| source.module)
        .ok()
        .map(|index| &EMBEDDED_STDLIB_SOURCES[index])
}

/// Return true when the source tree owns this canonical standard module.
pub fn has_embedded_stdlib_module(module: &str) -> bool {
    embedded_stdlib_source(module).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_index_is_sorted_unique_and_has_a_stable_identity() {
        let sources = embedded_stdlib_sources();
        assert!(sources
            .windows(2)
            .all(|pair| pair[0].module < pair[1].module));
        assert!(!embedded_stdlib_bundle_id().is_empty());
        for source in sources {
            assert_eq!(
                embedded_stdlib_source(source.module).map(|item| item.path),
                Some(source.path)
            );
        }
    }
}
