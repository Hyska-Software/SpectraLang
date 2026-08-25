fn print_global_help() {
    println!("SpectraLang CLI");
    println!();
    println!("USAGE:");
    println!("    spectralang <COMMAND> [OPTIONS] <paths>...");
    println!();
    println!("COMMANDS:");
    println!("    compile    Compile Spectra modules (default)");
    println!("    check      Type-check modules and report diagnostics");
    println!("    run        Compile modules and execute the entry point via JIT");
    println!("    lint       Run lint checks across Spectra modules");
    println!("    bench      Compile with benchmark timings and optional JSON report");
    println!("    repl       Start an interactive Spectra prompt");
    println!("    new        Scaffold a new Spectra project");
    println!("    release-info  Report CLI and package release channel metadata");
    println!("    package    Resolve, lock, build, publish, and consume packages");
    println!("    db         Apply, inspect, and roll back database migrations");
    println!("    fmt        Format Spectra source files");
    println!("    help       Print this help message");
    println!();
    println!("GLOBAL OPTIONS:");
    println!("    -h, --help             Print this help message");
    println!("    --list-experimental    Report active experimental language gates");
    println!();
    print_compilation_options(None);
    println!();
    println!("EXAMPLES:");
    println!("    spectralang compile src/main.spectra");
    println!("    spectralang check examples/");
    println!("    spectralang run -O3 app.spectra");
    println!("    spectralang lint src/");
    println!("    spectralang bench --bench-json target/bench.json src/");
    println!("    spectralang repl --run");
    println!("    spectralang new my-project");
    println!("    spectralang release-info --json --root .");
    println!("    spectralang package build --root .");
    println!("    spectralang package add math --path ../math");
    println!("    spectralang db migrate --database app.sqlite --migrations-dir migrations");
    println!("    spectralang --list-experimental");
    println!("    spectralang fmt src/");
    println!("    spectralang fmt --stdin < file.spectra");
    println!();
    print_experimental_features();
    println!();
    println!("EXIT CODES:");
    println!("    0   Success");
    println!("    64  Usage error (invalid flags, missing inputs)");
    println!("    65  Compilation failed");
    println!("    74  I/O failure while reading or writing files");
    println!();
    println!("LOGGING:");
    println!("    Errors are emitted as 'error: <message>' for easy parsing.");
}

fn print_build_help(command: BuildCommand) {
    println!("SpectraLang CLI - '{}' command", command.name());
    println!();
    println!("USAGE:");
    println!("    spectralang {} [OPTIONS] <paths>...", command.name());
    println!();
    println!("{}", command.description());
    println!();
    print_compilation_options(Some(command));
    println!();
    println!("Examples:");
    match command {
        BuildCommand::Compile => {
            println!("    spectralang compile src/main.spectra");
            println!("    spectralang compile --dump-ir project/");
        }
        BuildCommand::Check => {
            println!("    spectralang check src/");
            println!("    spectralang check --dump-ast main.spectra");
        }
        BuildCommand::Run => {
            println!("    spectralang run app.spectra");
            println!("    spectralang run --timings src/main.spectra");
        }
        BuildCommand::Lint => {
            println!("    spectralang lint src/");
            println!("    spectralang lint --deny shadowing examples/");
        }
        BuildCommand::Bench => {
            println!("    spectralang bench src/");
            println!("    spectralang bench --bench-json target/bench.json tests/validation/");
            println!("    spectralang bench --async --bench-json target/async-bench.json");
        }
    }
    println!();
    println!("Use 'spectralang --list-experimental' to see active experimental language gates.");
}

fn print_repl_help() {
    println!("SpectraLang CLI - 'repl' command");
    println!();
    println!("USAGE:");
    println!("    spectralang repl [OPTIONS] [paths]...");
    println!();
    println!("Starts an interactive prompt that can compile, check, or run Spectra modules.");
    println!();
    println!("OPTIONS:");
    println!("    --dump-ast             Print the AST for debugging when compiling");
    println!("    --dump-ir              Print the IR for debugging when compiling");
    println!("    --timings, -T          Report compilation and execution timings");
    println!("    --summary              Show pipeline summaries for compiled modules");
    println!("    --verbose, -v          Print additional build details");
    println!("    --no-optimize, -O0     Disable all optimizations");
    println!("    -O1/-O2/-O3            Set optimization level");
    println!("    --run, -r              Automatically run modules after compiling");
    println!("    --enable-experimental <feature>");
    println!("                           Compatibility no-op for older scripts (no active experimental language gates)");
    println!();
    println!("Interactive commands:");
    println!("    <declaration>          Append a func/record/enum/import/... to the session buffer");
    println!("                           (validated by recompiling the whole buffer; rejected");
    println!("                           input never changes the buffer)");
    println!("    <expression>           Evaluate immediately and print the value");
    println!("    :{{ ... }}:              Paste a multi-line block, processed as one input");
    println!("    :type <expr>           Show the inferred type of an expression");
    println!("    :buffer                Print the current session module source");
    println!("    :reset                 Clear the session buffer");
    println!("    :save <path>           Write the session buffer to a file");
    println!("    :load <path>           Merge a file into the session buffer (module header stripped)");
    println!("    :run/:check/:compile <paths>...");
    println!("                           Compile external files without touching the buffer");
    println!("    :help                  Show this help text");
    println!("    :quit                  Exit the REPL");
}

fn print_new_help() {
    println!("SpectraLang CLI - 'new' command");
    println!();
    println!("USAGE:");
    println!("    spectralang new [OPTIONS] <path>");
    println!();
    println!("Create a new Spectra project with a starter module and manifest.");
    println!();
    println!("OPTIONS:");
    println!("    -f, --force        Scaffold even if the directory already exists");
    println!();
    println!("Examples:");
    println!("    spectralang new hello-world");
    println!("    spectralang new --force .");
}

fn print_release_info_help() {
    println!("SpectraLang CLI - 'release-info' command");
    println!();
    println!("USAGE:");
    println!("    spectralang release-info [OPTIONS] [root]");
    println!();
    println!("Report CLI and package release channel metadata.");
    println!();
    println!("OPTIONS:");
    println!("    --root <path>     Package or workspace root (default: .)");
    println!("    --json            Emit machine-readable JSON");
    println!();
    println!("Examples:");
    println!("    spectralang release-info --root .");
    println!("    spectralang release-info --json --root tests/projects/valid/package_workspace");
}

fn print_db_help() {
    println!("SpectraLang CLI - 'db' command");
    println!();
    println!("USAGE:");
    println!("    spectralang db <migrate|rollback|status> [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    --database <path>          SQLite database path");
    println!("    --migrations-dir <path>    Migration files directory");
    println!("    --steps <count>            Number of migrations to roll back (default: 1)");
    println!("    --json                     Emit JSON (status only)");
    println!();
    println!("EXAMPLES:");
    println!("    spectralang db migrate --database app.sqlite --migrations-dir migrations");
    println!("    spectralang db rollback --database app.sqlite --migrations-dir migrations --steps 1");
    println!("    spectralang db status --database app.sqlite --migrations-dir migrations --json");
}

fn print_package_help() {
    println!("SpectraLang CLI - 'package' command");
    println!();
    println!("USAGE:");
    println!("    spectralang package <SUBCOMMAND> [OPTIONS]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    lock       Resolve packages and write spectra.lock");
    println!("    build      Resolve, lock, and compile a package workspace");
    println!("    check      Resolve, lock, and type-check a package workspace");
    println!("    run        Resolve, lock, compile, and run the workspace entry point");
    println!("    test       Resolve, lock, list/filter, and run #[spectra_async_test] tests");
    println!("    bench      Resolve, lock, and check with pipeline timings");
    println!("    doc        Generate package documentation into target/spectra-docs");
    println!(
        "    add        Add a path, registry, Git, or catalog dependency and refresh spectra.lock"
    );
    println!("    fetch      Download/cache dependencies and refresh spectra.lock");
    println!("    search     Search configured package catalogs");
    println!("    info       Show catalog metadata for a package");
    println!("    versions   List catalog versions for a package");
    println!("    tree       Print resolved dependency tree");
    println!("    register   Register current package metadata in a catalog");
    println!("    publish-metadata  Write package catalog metadata");
    println!("    catalog    Manage local catalog references");
    println!("    update     Refresh spectra.lock from current manifests");
    println!("    publish    Publish the root package into a local registry directory");
    println!();
    println!("OPTIONS:");
    println!("    --root <path>          Package or workspace root (default: .)");
    println!("    --path <path>          Local dependency path for 'add'");
    println!("    --version <version>    Dependency version for 'add'");
    println!("    --registry <path>      Local registry path for 'add' or 'publish'");
    println!("    --git <url>            Git package source for 'add' or 'register'");
    println!("    --tag <tag>            Git tag for package source");
    println!("    --rev <sha>            Git commit/revision for package source");
    println!("    --branch <name>        Git branch for package source");
    println!("    --catalog <path>       Catalog index/directory for search/add/register");
    println!("    --out <path>           Output path for 'publish-metadata'");
    println!("    --offline              Use only restored package caches (package commands)");
    println!("    --locked               Require an existing, unmodified spectra.lock");
    println!("    --list                 List async tests for 'test'");
    println!("    --filter <text>        Run or list async tests whose name/path contains text");
    println!("    --json                 Emit JSON report for 'test'");
    println!();
    println!("Examples:");
    println!("    spectralang package lock --root .");
    println!("    spectralang package build --root examples/workspace");
    println!("    spectralang package test --root . --filter api");
    println!("    spectralang package add math --path ../math --version 0.1.0");
    println!("    spectralang package search api");
    println!("    spectralang package add spectra.api");
    println!("    spectralang package add math --git https://github.com/org/math.git --tag v1.2.3");
    println!("    spectralang package register --root . --git https://github.com/org/math.git --tag v1.2.3 --catalog ./catalog");
    println!("    spectralang package publish --root packages/math --registry .spectra-registry");
    println!("    spectralang package add math --version 0.1.0 --registry .spectra-registry");
}

fn print_format_help() {
    println!("SpectraLang CLI - 'fmt' command");
    println!();
    println!("USAGE:");
    println!("    spectralang fmt [OPTIONS] <paths>...");
    println!();
    println!("Format Spectra source files in-place or verify formatting with --check.");
    println!();
    println!("OPTIONS:");
    println!("    --check              Verify formatting without writing changes");
    println!("    --stdin              Read Spectra source from standard input");
    println!("    --stdout             Write the formatted result to stdout instead of files (single input file)");
    println!("    --explain[=json]     Show diffs (text by default, json for machine-readable) and implies --check");
    println!("    --stats              Emit a JSON summary of the formatter run");
    println!("    --config <path>      Load formatter configuration from an explicit Spectra.toml");
    println!("    -h, --help          Show this help text");
    println!();
    println!("Examples:");
    println!("    spectralang fmt src/");
    println!("    spectralang fmt --check examples/test.spectra");
    println!("    spectralang fmt --stdin < script.spectra");
    println!("    spectralang fmt --stdout src/main.spectra");
}

fn print_lint_help() {
    println!("SpectraLang CLI - 'lint' command");
    println!();
    println!("USAGE:");
    println!("    spectralang lint [OPTIONS] <paths>...");
    println!();
    println!("Run Spectra's lint checks across the provided files or directories.");
    println!("Warnings are reported to stdout; denied rules cause the command to fail with exit code 65.");
    println!();
    println!("OPTIONS:");
    println!("    --lint              Redundant; 'lint' always enables lint rules");
    println!("    --allow <rule>      Allow (suppress) a lint rule (may be repeated)");
    println!("    --deny <rule>       Deny a lint rule and escalate matches to errors");
    println!("    --dump-ast          Dump the parsed AST for debugging");
    println!("    --timings, -T       Collect front-end timings");
    println!("    --summary           Print pipeline summaries (semantic + lint)");
    println!("    --verbose, -v       Print additional plan diagnostics");
    println!("    --json              Emit diagnostics as JSON");
    println!("    --enable-experimental <feature>");
    println!("                        Compatibility no-op for older scripts");
    println!(
        "    -O0/-O1/-O2/-O3     Set optimization level (ignored by lint but accepted for parity)"
    );
    println!();
    println!("Available lint rules: {}", lint_rule_list());
    println!();
    println!("Examples:");
    println!("    spectralang lint src/");
    println!("    spectralang lint --deny shadowing examples/");
}

fn print_compilation_options(command: Option<BuildCommand>) {
    println!("COMPILATION OPTIONS:");
    println!("    --dump-ast             Print the AST for debugging");
    println!("    --dump-ir              Print the IR for debugging");
    println!("    --timings, -T          Report compilation and execution timings");
    println!("    --summary              Show pipeline summaries for compiled modules");
    println!("    --verbose, -v          Print additional build details");
    println!("    --no-optimize, -O0     Disable all optimizations");
    println!("    -O1                    Enable basic optimizations");
    println!("    -O2                    Enable moderate optimizations (default)");
    println!("    -O3                    Enable aggressive optimizations");
    match command {
        Some(BuildCommand::Check) | Some(BuildCommand::Lint) => {
            println!("    --run, -r              Not available for the 'check' command");
        }
        Some(BuildCommand::Run) => {
            println!("    --run, -r              Redundant; 'run' always executes after compiling");
        }
        _ => {
            println!("    --run, -r              Execute the program with the JIT after compiling");
        }
    }
    println!("    --enable-experimental <feature>");
    println!("                           Compatibility no-op for older scripts (no active experimental language gates)");
    if matches!(command, Some(BuildCommand::Lint)) {
        println!("    --lint                 Redundant; 'lint' always enables lint rules");
    } else {
        println!("    --lint                 Enable lint checks for the selected command");
    }
    println!("    --allow <rule>         Allow (suppress) a lint rule (may be repeated)");
    println!("    --deny <rule>          Deny a lint rule and escalate matches to errors");
    println!("    --json                 Emit diagnostics as JSON");
    println!("    --sarif                Emit diagnostics as SARIF 2.1.0");
    println!("    --bench-json <path>    Write benchmark timings as JSON (bench only)");
    if matches!(command, Some(BuildCommand::Bench)) {
        println!("    --async                Run Phase 21 async runtime microbenchmarks");
    }
    println!(
        "                           Available rules: {}",
        lint_rule_list()
    );
}

fn print_experimental_features() {
    println!("Experimental language features: none");
    if !KNOWN_EXPERIMENTAL_FEATURES.is_empty() {
        println!("Enable with --enable-experimental <feature>:");
        for feature in KNOWN_EXPERIMENTAL_FEATURES {
            println!("    - {}", feature);
        }
    }
}

fn usage_error(message: &str) -> CliError {
    let trimmed = message.trim_end();
    let formatted = format!("{}\nUse 'spectra --help' for usage information.", trimmed);
    CliError::usage(formatted)
}

