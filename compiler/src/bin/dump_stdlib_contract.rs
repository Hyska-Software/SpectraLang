//! Emit the compiler-owned builtin semantic contract for tooling.
//!
//! This binary is intentionally small and dependency-light.  Catalog tooling
//! can consume its JSON output instead of maintaining a second parser for the
//! semantic registration tables.

use serde_json::json;
use spectra_compiler::semantic::builtin_modules::builtin_contract_symbols;

fn main() {
    let symbols = builtin_contract_symbols()
        .into_iter()
        .map(|symbol| {
            json!({
                "path": symbol.path,
                "kind": symbol.kind,
                "signature": symbol.signature,
            })
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::to_string_pretty(&symbols).expect("builtin contract JSON must serialize")
    );
}
