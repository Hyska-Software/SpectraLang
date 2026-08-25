use spectra_compiler::{Lexer, Parser};
use spectra_midend::ASTLowering;
use std::collections::HashSet;

fn run(data: &[u8]) {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    if source.len() > 16 * 1024 {
        return;
    }
    let Ok(tokens) = Lexer::new(source).tokenize() else {
        return;
    };
    let Ok(module) = Parser::new(tokens, HashSet::new()).parse() else {
        return;
    };
    let _ = ASTLowering::new().lower_module(&module);
}

#[cfg(fuzzing)]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    run(data);
});

#[cfg(not(fuzzing))]
fn main() {
    spectralang_fuzz::replay_main(run);
}
