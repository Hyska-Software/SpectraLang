use spectra_compiler::{CompilationOptions, CompilationPipeline};

fn run(data: &[u8]) {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    if source.len() > 16 * 1024 {
        return;
    }
    let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
    let _ = pipeline.compile(source, "<fuzz>.spectra");
}

#[cfg(fuzzing)]
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    run(data);
});

#[cfg(not(fuzzing))]
fn main() {
    spectralang_fuzz::replay_main(run);
}
