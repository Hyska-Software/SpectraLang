use spectra_backend::CodeGenerator;
use spectra_compiler::{Lexer, Parser};
use spectra_midend::ASTLowering;

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
    let Ok(module) = Parser::new(tokens).parse() else {
        return;
    };
    // Drive well-formed syntax all the way into Cranelift code generation:
    // lowering failures are legitimate results (the target hunts panics and
    // verifier ICEs, not rejected programs).
    let mut lowering = ASTLowering::new();
    let Ok(ir_module) = lowering.lower_module(&module) else {
        return;
    };
    let mut codegen = CodeGenerator::new();
    let _ = codegen.generate_module(&ir_module);
}

#[cfg(fuzzing)]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    run(data);
});

#[cfg(not(fuzzing))]
fn main() {
    spectralang_fuzz::replay_main(run);
}
