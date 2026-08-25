// Package/project error enums intentionally retain structured paths and source
// context; boxing every result would make the CLI API less ergonomic without
// improving the ownership model.
#![allow(clippy::result_large_err)]

mod compiler_integration;
mod config;
mod discovery;
mod formatter;
mod linker;
mod package;
mod project;
mod release_channel;
mod runtime_lib;

use compiler_integration::{
    forward_program_args, take_last_exec_exit, ModulePipelineSummary, NativeDebugMetadata,
    SpectraCompiler,
};
use formatter::{run as run_formatter, ExplainMode, FormatOptions};
use package::{PackageCommand, PackageInvocation};
use project::{ProjectPlan, ProjectSourceEntry};
use release_channel::{cli_channel, cli_compatibility_level};
use serde::{Deserialize, Serialize};
use serde_json::json;
use spectra_compiler::{
    ast::{Item, Module, TypeAnnotationKind},
    error::CompilerError,
    lint::LintDiagnostic,
    span::Span,
    CompilationOptions, DebugInfoMode, Lexer, LintOptions, LintRule, Parser,
    analyze_document, collect_let_inlay_hints,
};
use spectra_db::{migrations::SqliteMigrator, sqlite::SqliteConnection};
use std::collections::{BTreeMap, HashSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::{env, fs, process};

const KNOWN_EXPERIMENTAL_FEATURES: &[&str] = &[];
const AOT_DEBUG_MAP_SCHEMA_VERSION: u32 = 1;

#[repr(i32)]
#[derive(Copy, Clone, Debug)]
enum ExitCode {
    Success = 0,
    Usage = 64,
    CompilationFailed = 65,
    IoError = 74,
}

impl ExitCode {
    fn as_i32(self) -> i32 {
        self as i32
    }
}

#[derive(Debug)]
struct CliError {
    message: String,
    code: ExitCode,
}

impl CliError {
    fn new(message: impl Into<String>, code: ExitCode) -> Self {
        Self {
            message: message.into(),
            code,
        }
    }

    fn usage(message: impl Into<String>) -> Self {
        Self::new(message, ExitCode::Usage)
    }

    fn compilation(message: impl Into<String>) -> Self {
        Self::new(message, ExitCode::CompilationFailed)
    }

    fn io(message: impl Into<String>) -> Self {
        Self::new(message, ExitCode::IoError)
    }
}

type CliResult<T> = Result<T, CliError>;

fn log_error(message: &str) {
    for (index, line) in message.lines().enumerate() {
        if index == 0 {
            eprintln!("error: {}", line);
        } else if line.is_empty() {
            eprintln!();
        } else {
            eprintln!("       {}", line);
        }
    }
}

#[derive(Debug)]
struct CliInvocation {
    entries: Vec<PathBuf>,
    options: CompilationOptions,
    show_pipeline_summary: bool,
    verbose: bool,
    json_output: bool,
    sarif_output: bool,
    /// When `Some(path)`, emit a native object file at `path` instead of / in addition to JIT.
    emit_object: Option<PathBuf>,
    /// When `Some(path)`, compile to a native executable at `path`.
    emit_exe: Option<PathBuf>,
    /// When `Some(path)`, write a machine-readable benchmark report.
    bench_json: Option<PathBuf>,
    /// Run the Phase 21 async runtime microbenchmark suite instead of compiler timing benchmarks.
    async_bench: bool,
    /// Arguments forwarded to the Spectra program when running via JIT (`run` command).
    /// These are accessible through `std.env.env_arg` / `std.env.env_args_count`.
    program_args: Vec<String>,
}

#[derive(Debug)]
struct ReplOptions {
    base_options: CompilationOptions,
    preload: Vec<PathBuf>,
    autorun: bool,
    show_pipeline_summary: bool,
    verbose: bool,
    json_output: bool,
}

#[derive(Debug)]
struct NewProjectOptions {
    path: PathBuf,
    force: bool,
}

#[derive(Debug)]
struct ReleaseInfoOptions {
    root: PathBuf,
    json: bool,
}

#[derive(Debug)]
struct DbInvocation {
    command: DbCommand,
    database: PathBuf,
    migrations_dir: PathBuf,
    steps: usize,
    json: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum DbCommand {
    Migrate,
    Rollback,
    Status,
}

#[derive(Debug)]
enum CliAction {
    Help(HelpTopic),
    ListExperimental,
    Build {
        kind: BuildCommand,
        invocation: CliInvocation,
    },
    Repl(ReplOptions),
    NewProject(NewProjectOptions),
    ReleaseInfo(ReleaseInfoOptions),
    Package(PackageInvocation),
    Format(FormatOptions),
    Db(DbInvocation),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum HelpTopic {
    Global,
    Build(BuildCommand),
    Repl,
    NewProject,
    ReleaseInfo,
    Package,
    Format,
    Lint,
    Db,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum BuildCommand {
    Compile,
    Check,
    Run,
    Lint,
    Bench,
}

impl BuildCommand {
    fn name(self) -> &'static str {
        match self {
            BuildCommand::Compile => "compile",
            BuildCommand::Check => "check",
            BuildCommand::Run => "run",
            BuildCommand::Lint => "lint",
            BuildCommand::Bench => "bench",
        }
    }

    fn description(self) -> &'static str {
        match self {
            BuildCommand::Compile => "Compile Spectra modules (default).",
            BuildCommand::Check => "Type-check modules and report diagnostics without executing.",
            BuildCommand::Run => "Compile modules and execute the entry point via JIT.",
            BuildCommand::Lint => "Run lint checks and report warnings or denied rules.",
            BuildCommand::Bench => "Compile modules with timing metrics and optional JSON report.",
        }
    }

    fn success_message(self) -> &'static str {
        match self {
            BuildCommand::Compile => "    Finished",
            BuildCommand::Check => "    Finished (no errors detected)",
            BuildCommand::Run => "",
            BuildCommand::Lint => "    Finished (no lint findings)",
            BuildCommand::Bench => "    Finished bench",
        }
    }

    fn module_verb(self) -> &'static str {
        match self {
            BuildCommand::Check => "Checking",
            BuildCommand::Lint => "Linting",
            BuildCommand::Bench => "Benchmarking",
            BuildCommand::Compile | BuildCommand::Run => "Compiling",
        }
    }
}

fn main() {
    let exit_code = match run_cli() {
        Ok(()) => ExitCode::Success,
        Err(error) => {
            log_error(&error.message);
            error.code
        }
    };

    process::exit(exit_code.as_i32());
}

include!("cli_parse_core.rs");
include!("cli_parse_options.rs");
include!("cli_parse_package.rs");
include!("cli_build.rs");
include!("cli_bench.rs");
include!("cli_aot.rs");
include!("repl_session.rs");
include!("cli_repl_project.rs");
include!("cli_package.rs");
include!("cli_diagnostics.rs");
include!("cli_help.rs");
include!("cli_tests.rs");
