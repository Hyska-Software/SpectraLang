// Thin `spc` entry point: alias of the `spectralang` CLI over the shared
// implementation in `lib.rs`.
fn main() {
    std::process::exit(spectra_cli::run());
}
