use spectra_compiler::{analyze_modules, Lexer, Parser};
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
    let Ok(mut module) = Parser::new(tokens, HashSet::new()).parse() else {
        return;
    };
    let mut modules = vec![&mut module];
    let _ = analyze_modules(modules.as_mut_slice());
}

#[cfg(fuzzing)]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    run(data);
});

#[cfg(not(fuzzing))]
fn main() {
    spectralang_fuzz::replay_main(run);
}
