//! Local generation bridge (R-3211 T2).
//!
//! The plan wires this to `std.ml.generate_ex` (the only language-reachable
//! generation entry, with temperature/top_k/seed) plus the shared tokenizer.
//! That entry and every tensor/registry constructor around it are `pub(crate)`
//! inside `spectra-runtime`, so this crate cannot reach the generation path
//! without exporting new runtime surface. Rather than duplicate model
//! loading, the local provider reports a typed not-configured error and the
//! deterministic path lives in the mock provider.

use super::{Provider, ProviderError, ProviderRequest, ProviderResponse};

/// Placeholder local provider selected by `endpoint: "local:"` or a `local/`
/// model prefix.
#[derive(Default)]
pub(crate) struct LocalProvider;

const NOT_CONFIGURED: &str = "local provider is not configured: spectra-runtime does not expose its \
     ML generation entry (std.ml.generate_ex) outside the crate, so R-3211 cannot bridge to it; \
     use endpoint \"mock:\" for the deterministic path or an OpenAI-compatible endpoint";

impl Provider for LocalProvider {
    fn name(&self) -> &'static str {
        "local"
    }

    fn is_configured(&self) -> bool {
        false
    }

    fn honors_seed(&self) -> bool {
        false
    }

    fn complete(&self, _request: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        Err(ProviderError::NotConfigured(NOT_CONFIGURED.to_string()))
    }

    fn embed(&self, _text: &str) -> Result<Vec<f64>, ProviderError> {
        Err(ProviderError::NotConfigured(NOT_CONFIGURED.to_string()))
    }
}
