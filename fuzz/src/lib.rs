//! Shared support code for the SpectraLang fuzz targets.
//!
//! Targets built by cargo-fuzz define `--cfg fuzzing`; their `fuzz_target!`
//! macro drives them with libFuzzer-generated input. Targets built with plain
//! cargo (no `--cfg fuzzing`) have no libFuzzer entry point, so they fall back
//! to `replay_main`, which replays corpus files passed as command-line
//! arguments (directories are walked recursively) or stdin when no argument is
//! given. This keeps `cargo check -p spectralang-fuzz` and corpus smoke runs
//! possible on machines without cargo-fuzz.

use std::io::Read;
use std::path::{Path, PathBuf};

fn collect_inputs(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = match std::fs::read_dir(path) {
            Ok(read_dir) => read_dir.filter_map(|entry| entry.ok()).map(|entry| entry.path()).collect(),
            Err(_) => return,
        };
        entries.sort();
        for entry in entries {
            collect_inputs(&entry, out);
        }
    } else {
        out.push(path.to_path_buf());
    }
}

/// Fallback entry point for non-cargo-fuzz builds: replays each input file
/// through `run`. Panics if an argument cannot be read so CI smoke runs fail
/// loudly instead of silently skipping inputs.
pub fn replay_main(run: fn(&[u8])) {
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut saw_argument = false;
    for argument in std::env::args_os().skip(1) {
        saw_argument = true;
        collect_inputs(Path::new(&argument), &mut inputs);
    }

    if !saw_argument {
        let mut data = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut data);
        run(&data);
        return;
    }

    let mut unreadable = 0usize;
    for input in &inputs {
        match std::fs::read(input) {
            Ok(data) => run(&data),
            Err(error) => {
                eprintln!("failed to read {}: {error}", input.display());
                unreadable += 1;
            }
        }
    }
    assert_eq!(unreadable, 0, "{unreadable} fuzz input(s) failed to read");
}
