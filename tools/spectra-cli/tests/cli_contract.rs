//! Binary-level contract tests for CLI exit-code propagation and flag
//! validation. These spawn the real `spectralang` binary so the whole chain
//! (`main` -> `spectra_cli::run()` -> exit status) is covered, complementing
//! the in-process unit tests in `src/cli_tests.rs`.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

/// Fixture path that does not depend on the test process working directory.
fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn run_spectralang(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spectralang"))
        .args(args)
        .output()
        .expect("run spectralang binary")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Fresh temp workspace that cleans itself up on drop.
struct TempWorkspace {
    root: PathBuf,
}

impl TempWorkspace {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "spectra-cli-contract-{}-{}-{}",
            label,
            std::process::id(),
            nonce
        ));
        fs::create_dir_all(&root).expect("create temp workspace");
        Self { root }
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, contents).expect("write source");
        path
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn utf8(path: &PathBuf) -> &str {
    path.to_str().expect("temp path is valid UTF-8")
}

#[test]
fn run_propagates_the_program_exit_status_to_the_process_status() {
    let source = fixture("../../tests/cli/runtime_nonzero.spectra");
    let output = run_spectralang(&["run", utf8(&source)]);

    assert_eq!(
        output.status.code(),
        Some(7),
        "program status must become the process status; stderr: {}",
        stderr(&output)
    );
    let err = stderr(&output);
    assert!(
        err.contains("program exited with status 7"),
        "runtime diagnostic must report the status: {err}"
    );
    assert!(
        err.contains("0: main()"),
        "runtime frame trace must be preserved: {err}"
    );
}

#[test]
fn compile_run_propagates_the_program_exit_status() {
    let source = fixture("../../tests/cli/runtime_nonzero.spectra");
    let output = run_spectralang(&["compile", "--run", utf8(&source)]);

    assert_eq!(
        output.status.code(),
        Some(7),
        "`compile --run` must propagate the program status (it used to be
         swallowed because exit propagation only ran for the `run` kind);
         stderr: {}",
        stderr(&output)
    );
    let err = stderr(&output);
    assert!(
        err.contains("program exited with status 7"),
        "runtime diagnostic must report the status: {err}"
    );
}

#[test]
fn run_without_an_entry_point_is_rejected_with_a_clear_diagnostic() {
    let workspace = TempWorkspace::new("no-main");
    let source = workspace.write(
        "lib.spectra",
        "module contract_lib\n\nfunc helper() returns int {\n    return 1\n}\n",
    );

    let output = run_spectralang(&["run", utf8(&source)]);
    assert_eq!(
        output.status.code(),
        Some(65),
        "stderr: {}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains("no entry point 'main' found"),
        "diagnostic must name the missing entry point: {}",
        stderr(&output)
    );
}

#[test]
fn run_with_multiple_entry_points_is_rejected_like_aot() {
    let workspace = TempWorkspace::new("two-mains");
    workspace.write(
        "a.spectra",
        "module contract_a\n\npublic func main() returns int {\n    return 0\n}\n",
    );
    workspace.write(
        "b.spectra",
        "module contract_b\n\npublic func main() returns int {\n    return 1\n}\n",
    );

    let output = run_spectralang(&["run", utf8(&workspace.root)]);
    assert_eq!(
        output.status.code(),
        Some(65),
        "JIT execution must enforce the same single-entry rule as AOT; stderr: {}",
        stderr(&output)
    );
    let err = stderr(&output);
    assert!(
        err.contains("exactly one `main`"),
        "diagnostic must name the violation: {err}"
    );
}

#[test]
fn run_flag_is_rejected_outside_run_and_compile_with_exit_64() {
    let source = fixture("../../tests/cli/lint_clean.spectra");

    let lint = run_spectralang(&["lint", "--run", utf8(&source)]);
    assert_eq!(lint.status.code(), Some(64), "stderr: {}", stderr(&lint));
    assert!(
        stderr(&lint).contains("--run"),
        "usage error must name the flag: {}",
        stderr(&lint)
    );

    let check = run_spectralang(&["check", "-r", utf8(&source)]);
    assert_eq!(check.status.code(), Some(64), "stderr: {}", stderr(&check));
}

#[test]
fn emit_flag_combinations_are_rejected_with_exit_64() {
    let workspace = TempWorkspace::new("emit-flags");
    let source = fixture("../../tests/cli/lint_clean.spectra");
    let object = workspace.root.join("out.o");
    let executable = workspace.root.join("out.exe");

    let run = run_spectralang(&[
        "run",
        "--emit-exe",
        utf8(&executable),
        utf8(&source),
    ]);
    assert_eq!(run.status.code(), Some(64), "stderr: {}", stderr(&run));
    assert!(
        stderr(&run).contains("only supported with the 'compile' command"),
        "{}",
        stderr(&run)
    );

    let check = run_spectralang(&[
        "check",
        "--emit-object",
        utf8(&object),
        utf8(&source),
    ]);
    assert_eq!(check.status.code(), Some(64), "stderr: {}", stderr(&check));

    let both = run_spectralang(&[
        "compile",
        "--emit-object",
        utf8(&object),
        "--emit-exe",
        utf8(&executable),
        utf8(&source),
    ]);
    assert_eq!(both.status.code(), Some(64), "stderr: {}", stderr(&both));
    assert!(
        stderr(&both).contains("cannot be used together"),
        "{}",
        stderr(&both)
    );
}
