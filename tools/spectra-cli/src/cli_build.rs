fn execute_build_command(kind: BuildCommand, invocation: CliInvocation) -> CliResult<()> {
    let CliInvocation {
        entries,
        options,
        show_pipeline_summary,
        verbose,
        json_output,
        sarif_output,
        emit_object,
        emit_exe,
        bench_json,
        async_bench,
        program_args,
    } = invocation;

    if kind == BuildCommand::Bench && async_bench {
        return execute_async_benchmarks(bench_json);
    }

    if json_output || sarif_output {
        return match kind {
            BuildCommand::Compile | BuildCommand::Check | BuildCommand::Lint => {
                execute_structured_diagnostics(entries, options, sarif_output)
            }
            BuildCommand::Run | BuildCommand::Bench => Err(usage_error(
                "'--json' and '--sarif' are only supported with the 'compile', 'check', or 'lint' commands. Use '--bench-json <path>' for bench reports.",
            )),
        };
    }

    // AOT object emission: compile the first entry file and write the object bytes.
    if let Some(ref exe_path) = emit_exe {
        if entries.first().is_some_and(|entry| entry.is_dir()) {
            return execute_project_executable(entries, options, exe_path, verbose);
        }
    }

    if let Some(ref obj_path) = emit_object {
        if entries.is_empty() {
            return Err(CliError::usage("--emit-object requires a source file."));
        }
        let source_path = &entries[0];
        let source = fs::read_to_string(source_path)
            .map_err(|e| CliError::io(format!("Cannot read '{}': {}", source_path.display(), e)))?;
        let filename = source_path.to_string_lossy().to_string();
        let native_debug = matches!(options.debug_info, DebugInfoMode::Native);
        let mut compiler = SpectraCompiler::new(options);
        compiler.set_emit_output(false);
        let (obj_bytes, debug_metadata) = compiler
            .compile_to_object_with_debug_metadata(&source, &filename)
            .map_err(CliError::compilation)?;
        fs::write(obj_path, &obj_bytes)
            .map_err(|e| CliError::io(format!("Cannot write '{}': {}", obj_path.display(), e)))?;
        if native_debug {
            attach_native_debug(obj_path, source_path, &debug_metadata)?;
        }
        let debug_map_path = write_aot_debug_map(
            source_path,
            obj_path,
            &source,
            AotArtifactKind::Object,
            &["gdb", "lldb"],
            native_debug,
        )?;
        println!("     Written object {}", obj_path.display());
        println!("     Written debug map {}", debug_map_path.display());
        return Ok(());
    }

    // Executable compilation: compile → exe-object (with main shim) → link.
    if let Some(ref exe_path) = emit_exe {
        if entries.is_empty() {
            return Err(CliError::usage("--emit-exe requires a source file."));
        }
        let source_path = &entries[0];
        let source = fs::read_to_string(source_path)
            .map_err(|e| CliError::io(format!("Cannot read '{}': {}", source_path.display(), e)))?;
        let filename = source_path.to_string_lossy().to_string();

        // Locate the runtime static library before spending time compiling.
        let runtime_lib = runtime_lib::find_runtime_lib().ok_or_else(|| {
            CliError::compilation(
                "Cannot find libspectra_runtime.a / spectra_runtime.lib.\n\
                 Build the workspace first (`cargo build`) or set the \
                 SPECTRA_RUNTIME_LIB environment variable.",
            )
        })?;
        runtime_lib::validate_required_symbols(&runtime_lib)
            .map_err(CliError::compilation)?;
        let api_lib = runtime_lib::find_api_lib().ok_or_else(|| {
            CliError::compilation(
                "Cannot find libspectra_api.a / spectra_api.lib required by AOT API registration.\n\
                 Build the workspace first (`cargo build`) or set the SPECTRA_API_LIB environment variable.",
            )
        })?;
        runtime_lib::validate_api_required_symbols(&api_lib)
            .map_err(CliError::compilation)?;

        // Write the executable object to a temporary path next to the output.
        let obj_path = exe_path.with_extension("spectra_tmp.obj");

        let native_debug = matches!(options.debug_info, DebugInfoMode::Native);
        let mut compiler = SpectraCompiler::new(options);
        compiler.set_emit_output(false);
        let (obj_bytes, debug_metadata) = compiler
            .compile_to_executable_object_with_debug_metadata(&source, &filename)
            .map_err(CliError::compilation)?;

        fs::write(&obj_path, &obj_bytes).map_err(|e| {
            CliError::io(format!(
                "Cannot write temporary object '{}': {}",
                obj_path.display(),
                e
            ))
        })?;

        if native_debug {
            attach_native_debug(&obj_path, source_path, &debug_metadata)?;
        }

        let link_result = linker::link_executable(
            &obj_path,
            &runtime_lib,
            Some(&api_lib),
            exe_path,
            native_debug,
        );
        let _ = fs::remove_file(&obj_path); // always clean up the temp object
        link_result.map_err(CliError::compilation)?;

        let debug_map_path = write_aot_debug_map(
            source_path,
            exe_path,
            &source,
            AotArtifactKind::Executable,
            &["gdb", "lldb", "cdb"],
            native_debug,
        )?;
        println!("     Written executable {}", exe_path.display());
        println!("     Written debug map {}", debug_map_path.display());
        return Ok(());
    }

    // For `run`: forward program arguments to the runtime before executing.
    // argv[0] is conventionally the script/exe path; additional args follow.
    if kind == BuildCommand::Run {
        let script_path = entries
            .first()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut effective_args = vec![script_path];
        effective_args.extend(program_args);
        forward_program_args(effective_args);
    }

    // If a single directory is given (or current dir when no entries), look for spectra.toml.
    let project_root: Option<std::path::PathBuf> = if entries.len() <= 1 {
        let candidate = entries
            .first()
            .map(|p| {
                if p.is_dir() {
                    p.clone()
                } else {
                    p.parent()
                        .map(|par| par.to_path_buf())
                        .unwrap_or_else(|| std::path::PathBuf::from("."))
                }
            })
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        Some(candidate)
    } else {
        None
    };

    let (final_entries, package_name) = if let Some(ref root) = project_root {
        match config::try_load_config(root) {
            Ok(Some(cfg)) => {
                if verbose {
                    println!(
                        "Loaded project config '{}' v{}",
                        cfg.name(),
                        cfg.project.version
                    );
                }
                match package::resolve(root) {
                    Ok(workspace) if workspace.packages.len() > 1 => (
                        workspace.source_entries_with_origins(),
                        workspace.root_package_name(),
                    ),
                    _ => {
                        let src_dirs = cfg.src_dirs(root);
                        let sources = discovery::discover_sources(&src_dirs);
                        (
                            sources.into_iter().map(ProjectSourceEntry::plain).collect(),
                            Some(cfg.name().to_string()),
                        )
                    }
                }
            }
            Ok(None) => (
                entries.into_iter().map(ProjectSourceEntry::plain).collect(),
                None,
            ),
            Err(err) => {
                return Err(CliError::io(format!(
                    "Failed to load spectra.toml: {}",
                    err
                )));
            }
        }
    } else {
        (
            entries.into_iter().map(ProjectSourceEntry::plain).collect(),
            None,
        )
    };

    execute_plan_with_sources(
        kind,
        options,
        final_entries,
        package_name,
        show_pipeline_summary,
        show_pipeline_summary,
        true,
        verbose,
        bench_json,
    )
}

/// Compile a project into one native executable by preserving one relocatable
/// object per source module. The JIT project path already compiles modules in
/// dependency order; this path reuses the same order and lets the native linker
/// resolve the cross-module symbols.
fn execute_project_executable(
    entries: Vec<PathBuf>,
    options: CompilationOptions,
    exe_path: &Path,
    _verbose: bool,
) -> CliResult<()> {
    let plan = ProjectPlan::build(entries).map_err(|error| CliError::io(error.to_string()))?;
    let runtime_lib = runtime_lib::find_runtime_lib().ok_or_else(|| {
        CliError::compilation(
            "Cannot find libspectra_runtime.a / spectra_runtime.lib.\n\
             Build the workspace first (`cargo build`) or set the \
             SPECTRA_RUNTIME_LIB environment variable.",
        )
    })?;
    runtime_lib::validate_required_symbols(&runtime_lib).map_err(CliError::compilation)?;
    let api_lib = runtime_lib::find_api_lib().ok_or_else(|| {
        CliError::compilation(
            "Cannot find libspectra_api.a / spectra_api.lib required by AOT API registration.\n\
             Build the workspace first (`cargo build`) or set the SPECTRA_API_LIB environment variable.",
        )
    })?;
    runtime_lib::validate_api_required_symbols(&api_lib)
        .map_err(CliError::compilation)?;

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temp_dir = env::temp_dir().join(format!(
        "spectralang-aot-{}-{}",
        process::id(),
        nonce
    ));
    fs::create_dir_all(&temp_dir).map_err(|error| {
        CliError::io(format!(
            "Cannot create temporary AOT directory '{}': {}",
            temp_dir.display(),
            error
        ))
    })?;

    let result = (|| {
        let native_debug = matches!(options.debug_info, DebugInfoMode::Native);
        let mut compiler = SpectraCompiler::new(options);
        compiler.set_emit_output(false);
        let mut object_paths = Vec::with_capacity(plan.modules().len());
        let mut main_source_path: Option<PathBuf> = None;
        let mut main_source = String::new();
        let mut main_count = 0usize;

        for (index, module) in plan.modules().iter().enumerate() {
            compiler.set_current_package_name(module.package_name.clone());
            let source = fs::read_to_string(&module.path).map_err(|error| {
                CliError::io(format!(
                    "Cannot read '{}': {}",
                    module.path.display(),
                    error
                ))
            })?;
            let owned_source;
            // A synthetic `module` header shifts every compiled line by one;
            // the shift travels with the sources so diagnostics render
            // against the on-disk text (D6).
            let (effective_source, line_shift) = if source_has_module_decl(&source) {
                (source.as_str(), 0)
            } else {
                owned_source = format!("module {}\n{}", module.name, source);
                (owned_source.as_str(), 1)
            };
            let defines_main = source_defines_main(effective_source);
            if defines_main {
                main_count += 1;
                if main_count > 1 {
                    return Err(CliError::compilation(
                        "A project AOT build must contain exactly one `main` function.",
                    ));
                }
                main_source_path = Some(module.path.clone());
                main_source = source.clone();
            }

            let object_path = temp_dir.join(format!("module-{index:04}.obj"));
            let module_filename = module.path.to_string_lossy();
            let (object_bytes, debug_metadata) = if defines_main {
                compiler
                    .compile_to_executable_object_with_line_shift(
                        effective_source,
                        &module_filename,
                        &source,
                        line_shift,
                    )
                    .map_err(CliError::compilation)?
            } else {
                compiler
                    .compile_to_object_with_line_shift(
                        effective_source,
                        &module_filename,
                        &source,
                        line_shift,
                    )
                    .map_err(CliError::compilation)?
            };
            fs::write(&object_path, object_bytes).map_err(|error| {
                CliError::io(format!(
                    "Cannot write temporary object '{}': {}",
                    object_path.display(),
                    error
                ))
            })?;
            if native_debug && !debug_metadata.functions.is_empty() {
                attach_native_debug(&object_path, &module.path, &debug_metadata)?;
            }
            object_paths.push(object_path);
        }

        let main_source_path = main_source_path.ok_or_else(|| {
            CliError::compilation(
                "Project AOT compilation requires one function named `main`.",
            )
        })?;
        linker::link_executable_many(
            &object_paths,
            &runtime_lib,
            Some(&api_lib),
            exe_path,
            native_debug,
        )
            .map_err(CliError::compilation)?;
        let debug_map_path = write_aot_debug_map(
            &main_source_path,
            exe_path,
            &main_source,
            AotArtifactKind::Executable,
            &["gdb", "lldb", "cdb"],
            native_debug,
        )?;
        println!("     Written executable {}", exe_path.display());
        println!("     Written debug map {}", debug_map_path.display());
        Ok(())
    })();

    let _ = fs::remove_dir_all(&temp_dir);
    result
}

fn source_defines_main(source: &str) -> bool {
    let Ok(tokens) = Lexer::new(source).tokenize() else {
        return false;
    };
    let Ok(module) = Parser::new(tokens, HashSet::new()).parse() else {
        return false;
    };
    module.items.iter().any(|item| {
        matches!(item, Item::Function(function) if function.name == "main")
    })
}

fn compile_plan(
    kind: BuildCommand,
    compiler: &mut SpectraCompiler,
    plan: &ProjectPlan,
    show_pipeline_summary: bool,
    verbose: bool,
) -> (bool, Vec<ModulePipelineSummary>) {
    // When running via JIT without verbose/timings, suppress build progress output
    // so only the Spectra program's own stdout/stderr reaches the terminal.
    let quiet = kind == BuildCommand::Run && !verbose;
    let mut has_failures = false;
    let mut summaries = Vec::new();

    for module in plan.modules() {
        if !quiet {
            println!(
                "{:>12} {} ({})",
                kind.module_verb(),
                module.name,
                module.path.display()
            );
        }

        if verbose {
            if module.imports.is_empty() {
                println!("             imports: (none)");
            } else {
                println!("             imports: {}", module.imports.join(", "));
            }
        }

        let filename = module.path.to_string_lossy().to_string();
        if module.package_name.is_some() {
            compiler.set_current_package_name(module.package_name.clone());
        }
        match fs::read_to_string(&module.path) {
            Ok(source) => {
                // When the source file has no explicit `module` declaration the
                // project plan already derived a name from the filename stem.
                // Prepend a synthetic header so the parser receives a valid AST
                // without requiring boilerplate in every script.
                let owned;
                let (effective_source, line_shift): (&str, usize) =
                    if source_has_module_decl(&source) {
                        (&source, 0)
                    } else {
                        owned = format!("module {}\n{}", module.name, source);
                        (&owned, 1)
                    };

                match compiler.compile_with_line_shift(
                    effective_source,
                    &filename,
                    &source,
                    line_shift,
                ) {
                    Ok(()) => {
                        if let Some(summary) = compiler.take_last_summary() {
                            if show_pipeline_summary {
                                print_pipeline_summary(&summary);
                            }
                            summaries.push(summary);
                        }
                    }
                    Err(error) => {
                        has_failures = true;
                        // Print the pre-formatted diagnostic block directly to stderr.
                        // `render_errors()` already produces a fully structured
                        // "error[phase]: msg\n  --> file:line:col\n ..." block,
                        // including aligned source spans and carets.  Passing it
                        // through `log_error()` would add a spurious "error: "
                        // prefix to the first line and 7-space indent to every
                        // subsequent line, breaking gutter alignment.
                        eprint!("{}", error);
                    }
                }
            }
            Err(error) => {
                has_failures = true;
                eprintln!(
                    "error[io]: cannot read '{}': {}",
                    module.path.display(),
                    error
                );
            }
        }
    }

    (has_failures, summaries)
}

/// Returns `true` when the source already contains an explicit `module <name>`
/// declaration at the start of the file, ignoring blank lines and both `//`
/// line comments and `/* */` block comments.
fn source_has_module_decl(source: &str) -> bool {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    loop {
        // Skip whitespace
        while i < len && matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n') {
            i += 1;
        }

        if i >= len {
            return false;
        }

        if bytes[i] == b'/' {
            if i + 1 < len && bytes[i + 1] == b'/' {
                // Skip line comment
                i += 2;
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            } else if i + 1 < len && bytes[i + 1] == b'*' {
                // Skip block comment
                i += 2;
                while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2; // consume '*/'
                continue;
            }
        }

        // Next non-whitespace, non-comment content: check for `module `
        return bytes[i..].starts_with(b"module ");
    }
}

fn print_pipeline_summary(summary: &ModulePipelineSummary) {
    println!("    Pipeline summary:");
    println!("      Source: {}", summary.filename);

    if let Some(metrics) = &summary.frontend_metrics {
        println!("      Front-end total: {:?}", metrics.total);
        println!("        - Lexing:    {:?}", metrics.lexing);
        println!("        - Parsing:   {:?}", metrics.parsing);
        println!("        - Semantic:  {:?}", metrics.semantic);
        println!("        - Backend:   {:?}", metrics.backend);
    }

    println!("      Lowering: {:?}", summary.lowering_duration);
    println!("      Codegen:  {:?}", summary.codegen_duration);

    if !summary.passes.is_empty() {
        println!("      Passes:");
        for pass in &summary.passes {
            let status = if pass.modified {
                "modified"
            } else {
                "no change"
            };
            println!(
                "        - {:<24} {:>10?} ({})",
                pass.name, pass.duration, status
            );
        }
    }
}

fn write_bench_report(path: &Path, summaries: &[ModulePipelineSummary]) -> CliResult<()> {
    let report = BenchReport {
        version: 1,
        modules: summaries.iter().map(BenchModuleReport::from).collect(),
        totals: BenchTotals {
            modules: summaries.len(),
            frontend_ms: summaries
                .iter()
                .filter_map(|summary| summary.frontend_metrics.as_ref())
                .map(|metrics| duration_ms(metrics.total))
                .sum(),
            lowering_ms: summaries
                .iter()
                .map(|summary| duration_ms(summary.lowering_duration))
                .sum(),
            codegen_ms: summaries
                .iter()
                .map(|summary| duration_ms(summary.codegen_duration))
                .sum(),
            passes_ms: summaries
                .iter()
                .flat_map(|summary| summary.passes.iter())
                .map(|pass| duration_ms(pass.duration))
                .sum(),
        },
    };
    let text = serde_json::to_string_pretty(&report)
        .map_err(|error| CliError::io(format!("Failed to serialize bench report: {}", error)))?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|error| {
                CliError::io(format!(
                    "Failed to create bench report directory '{}': {}",
                    parent.display(),
                    error
                ))
            })?;
        }
    }
    fs::write(path, text).map_err(|error| {
        CliError::io(format!(
            "Failed to write bench report '{}': {}",
            path.display(),
            error
        ))
    })
}

#[derive(Serialize)]
struct BenchReport {
    version: u8,
    modules: Vec<BenchModuleReport>,
    totals: BenchTotals,
}

#[derive(Serialize)]
struct BenchModuleReport {
    file: String,
    frontend_ms: Option<f64>,
    lexing_ms: Option<f64>,
    parsing_ms: Option<f64>,
    semantic_ms: Option<f64>,
    backend_ms: Option<f64>,
    lowering_ms: f64,
    codegen_ms: f64,
    passes: Vec<BenchPassReport>,
}

impl From<&ModulePipelineSummary> for BenchModuleReport {
    fn from(summary: &ModulePipelineSummary) -> Self {
        let metrics = summary.frontend_metrics.as_ref();
        Self {
            file: summary.filename.clone(),
            frontend_ms: metrics.map(|metrics| duration_ms(metrics.total)),
            lexing_ms: metrics.map(|metrics| duration_ms(metrics.lexing)),
            parsing_ms: metrics.map(|metrics| duration_ms(metrics.parsing)),
            semantic_ms: metrics.map(|metrics| duration_ms(metrics.semantic)),
            backend_ms: metrics.map(|metrics| duration_ms(metrics.backend)),
            lowering_ms: duration_ms(summary.lowering_duration),
            codegen_ms: duration_ms(summary.codegen_duration),
            passes: summary
                .passes
                .iter()
                .map(|pass| BenchPassReport {
                    name: pass.name.to_string(),
                    duration_ms: duration_ms(pass.duration),
                    modified: pass.modified,
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct BenchPassReport {
    name: String,
    duration_ms: f64,
    modified: bool,
}

#[derive(Serialize)]
struct BenchTotals {
    modules: usize,
    frontend_ms: f64,
    lowering_ms: f64,
    codegen_ms: f64,
    passes_ms: f64,
}

fn duration_ms(duration: std::time::Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

const ASYNC_BENCH_COUNTS: &[usize] = &[1_000, 10_000, 100_000];
const ASYNC_BENCH_SCHEMA: &str = "spectra.r2111.async_benchmark.v1";
const ASYNC_BENCH_MAX_LATENCY_SAMPLES: usize = 1_024;

#[derive(Serialize)]
struct AsyncBenchReport {
    schema: &'static str,
    version: u8,
    runtime: &'static str,
    benchmarks: Vec<AsyncBenchCaseReport>,
    totals: AsyncBenchTotals,
}

#[derive(Serialize)]
struct AsyncBenchCaseReport {
    id: String,
    concurrent_tasks: usize,
    concurrent_connections: usize,
    samples: usize,
    p50_latency_ns: u128,
    p95_latency_ns: u128,
    p99_latency_ns: u128,
    throughput_tasks_per_sec: f64,
    total_ms: f64,
    checksum: i64,
}

#[derive(Serialize)]
struct AsyncBenchTotals {
    cases: usize,
    total_tasks: usize,
    total_ms: f64,
    min_throughput_tasks_per_sec: f64,
    max_p99_latency_ns: u128,
}

