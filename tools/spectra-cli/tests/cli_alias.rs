//! Binary-level coverage for the `spc` CLI alias (R-1005).
//!
//! Both command names must be real entry points over the same implementation:
//! they describe themselves in help output and produce identical results for
//! identical arguments, including the usage-error hint.

use std::path::PathBuf;
use std::process::{Command, Output};

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

fn run_spc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spc"))
        .args(args)
        .output()
        .expect("run spc binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn alias_help_reports_invoked_name() {
    let alias = run_spc(&["--help"]);
    assert!(
        alias.status.success(),
        "spc --help failed: {}",
        stderr(&alias)
    );
    let alias_stdout = stdout(&alias);
    assert!(
        alias_stdout.contains("    spc <COMMAND> [OPTIONS] <paths>..."),
        "spc help must document the spc command:\n{alias_stdout}"
    );
    assert!(alias_stdout.contains("    spc compile src/main.spectra"));

    let canonical = run_spectralang(&["--help"]);
    assert!(canonical.status.success());
    let canonical_stdout = stdout(&canonical);
    assert!(canonical_stdout.contains("    spectralang <COMMAND> [OPTIONS] <paths>..."));
    assert!(
        !canonical_stdout.contains("    spc <COMMAND>"),
        "canonical help must not advertise the alias:\n{canonical_stdout}"
    );
}

#[test]
fn alias_topic_help_reports_invoked_name() {
    let alias = run_spc(&["help", "db"]);
    assert!(
        alias.status.success(),
        "spc help db failed: {}",
        stderr(&alias)
    );
    let alias_stdout = stdout(&alias);
    assert!(
        alias_stdout.contains("    spc db <migrate|rollback|status> [OPTIONS]"),
        "spc help db must document the spc command:\n{alias_stdout}"
    );
}

#[test]
fn alias_and_canonical_agree_on_results() {
    let fixture = fixture("../../tests/cli/lint_clean.spectra");
    let fixture = fixture.to_str().expect("fixture path is valid UTF-8");

    for args in [
        vec!["--list-experimental"],
        vec!["check", fixture],
        vec!["run", fixture],
    ] {
        let canonical = run_spectralang(&args);
        let alias = run_spc(&args);

        assert_eq!(
            canonical.status.code(),
            alias.status.code(),
            "exit codes differ for {args:?}: canonical stderr={} alias stderr={}",
            stderr(&canonical),
            stderr(&alias)
        );
        assert_eq!(
            stdout(&canonical),
            stdout(&alias),
            "stdout differs for {args:?}"
        );
    }
}

#[test]
fn alias_usage_hint_names_the_invoked_binary() {
    let alias = run_spc(&[]);
    assert_eq!(alias.status.code(), Some(64));
    assert!(
        stderr(&alias).contains("Use 'spc --help' for usage information."),
        "alias usage hint must name spc: {}",
        stderr(&alias)
    );

    let canonical = run_spectralang(&[]);
    assert_eq!(canonical.status.code(), Some(64));
    assert!(
        stderr(&canonical).contains("Use 'spectralang --help' for usage information."),
        "canonical usage hint must name spectralang: {}",
        stderr(&canonical)
    );
}
