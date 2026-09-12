// Thin `spectralang` entry point. The implementation lives in `lib.rs` and is
// shared verbatim with the `spc` alias bin target (`src/bin/spc.rs`), so both
// command names cannot drift apart.
fn main() {
    std::process::exit(spectra_cli::run());
}
