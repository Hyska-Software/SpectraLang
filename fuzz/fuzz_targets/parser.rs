use spectra_compiler::{Lexer, Parser};
use std::collections::HashSet;

fn run(data: &[u8]) {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    if source.len() > 16 * 1024 {
        return;
    }
    if let Ok(tokens) = Lexer::new(source).tokenize() {
        let _ = Parser::new(tokens, HashSet::new()).parse();
    }
}

#[cfg(fuzzing)]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    run(data);
});

#[cfg(not(fuzzing))]
fn main() {
    spectralang_fuzz::replay_main(run);
}
