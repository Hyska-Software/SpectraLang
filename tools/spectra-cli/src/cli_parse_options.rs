fn parse_compilation_invocation<I>(
    args: &mut std::iter::Peekable<I>,
    command: BuildCommand,
    lint_default: bool,
) -> CliResult<CliInvocation>
where
    I: Iterator<Item = String>,
{
    let mut options = CompilationOptions::default();
    let mut lint_enabled_cli = lint_default;
    let mut lint_allow_cli: Vec<LintRule> = Vec::new();
    let mut lint_deny_cli: Vec<LintRule> = Vec::new();
    let mut entries = Vec::new();
    let mut show_pipeline_summary = false;
    let mut verbose = false;
    let mut json_output = false;
    let mut sarif_output = false;
    let mut emit_object: Option<PathBuf> = None;
    let mut emit_exe: Option<PathBuf> = None;
    let mut bench_json: Option<PathBuf> = None;
    let mut async_bench = false;
    let mut program_args: Vec<String> = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" => {
                // For `run`: everything after `--` is forwarded as program arguments.
                // For other commands: treat remaining tokens as additional source files
                // (preserves previous behaviour for `compile`, `check`, `lint`).
                let remaining: Vec<String> = args.by_ref().collect();
                if command == BuildCommand::Run {
                    program_args.extend(remaining);
                } else {
                    entries.extend(remaining.into_iter().map(PathBuf::from));
                }
                break;
            }
            "--dump-ast" => options.dump_ast = true,
            "--dump-ir" => options.dump_ir = true,
            "--debug-info=native" => options.debug_info = DebugInfoMode::Native,
            "--debug-info=none" => options.debug_info = DebugInfoMode::None,
            "--timings" | "-T" => {
                options.collect_metrics = true;
                show_pipeline_summary = true;
            }
            "--no-optimize" | "-O0" => {
                options.optimize = false;
                options.opt_level = 0;
            }
            "-O1" => {
                options.optimize = true;
                options.opt_level = 1;
            }
            "-O2" => {
                options.optimize = true;
                options.opt_level = 2;
            }
            "-O3" => {
                options.optimize = true;
                options.opt_level = 3;
            }
            "--verbose" | "-v" => verbose = true,
            "--run" | "-r" => {
                if command == BuildCommand::Check {
                    return Err(usage_error(
                        "'--run' cannot be used with the 'check' command.",
                    ));
                }
                options.run_jit = true;
            }
            "--summary" | "--pipeline-summary" => {
                options.collect_metrics = true;
                show_pipeline_summary = true;
            }
            flag if flag.starts_with("--allow=") => {
                let value = flag.trim_start_matches("--allow=");
                let rule = parse_lint_rule_cli(value)?;
                lint_allow_cli.push(rule);
            }
            flag if flag.starts_with("--deny=") => {
                let value = flag.trim_start_matches("--deny=");
                let rule = parse_lint_rule_cli(value)?;
                lint_deny_cli.push(rule);
            }
            "--lint" => {
                lint_enabled_cli = true;
            }
            "--allow" => {
                let value = args
                    .next()
                    .ok_or_else(|| usage_error("Missing rule name after '--allow'."))?;
                let rule = parse_lint_rule_cli(&value)?;
                lint_allow_cli.push(rule);
            }
            "--deny" => {
                let value = args
                    .next()
                    .ok_or_else(|| usage_error("Missing rule name after '--deny'."))?;
                let rule = parse_lint_rule_cli(&value)?;
                lint_deny_cli.push(rule);
            }
            "--json" => {
                if matches!(command, BuildCommand::Run | BuildCommand::Bench) {
                    return Err(usage_error(
                        "'--json' is only supported with the 'compile', 'check', or 'lint' commands. Use '--bench-json <path>' for bench reports.",
                    ));
                }
                if sarif_output {
                    return Err(usage_error(
                        "'--json' and '--sarif' cannot be used together.",
                    ));
                }
                json_output = true;
            }
            "--sarif" => {
                if matches!(command, BuildCommand::Run | BuildCommand::Bench) {
                    return Err(usage_error(
                        "'--sarif' is only supported with the 'compile', 'check', or 'lint' commands. Use '--bench-json <path>' for bench reports.",
                    ));
                }
                if json_output {
                    return Err(usage_error(
                        "'--json' and '--sarif' cannot be used together.",
                    ));
                }
                sarif_output = true;
            }
            "--bench-json" => {
                let path = args
                    .next()
                    .ok_or_else(|| usage_error("Missing output path after '--bench-json'."))?;
                bench_json = Some(PathBuf::from(path));
            }
            "--async" => {
                if command != BuildCommand::Bench {
                    return Err(usage_error(
                        "'--async' is only supported with the 'bench' command.",
                    ));
                }
                async_bench = true;
            }
            "--emit-object" | "-o" => {
                let path = args
                    .next()
                    .ok_or_else(|| usage_error("Missing output path after '--emit-object'."))?;
                emit_object = Some(PathBuf::from(path));
            }
            "--emit-exe" | "-e" => {
                let path = args
                    .next()
                    .ok_or_else(|| usage_error("Missing output path after '--emit-exe'."))?;
                emit_exe = Some(PathBuf::from(path));
            }
            flag if flag.starts_with('-') => {
                return Err(usage_error(&format!("Unknown option: {}", flag)));
            }
            _ => entries.push(PathBuf::from(arg)),
        }
    }

    if entries.is_empty() && !(command == BuildCommand::Bench && async_bench) {
        return Err(usage_error("No source files or directories were provided."));
    }

    match command {
        BuildCommand::Run => options.run_jit = true,
        BuildCommand::Check | BuildCommand::Lint | BuildCommand::Bench => options.run_jit = false,
        BuildCommand::Compile => {}
    }
    if command == BuildCommand::Bench {
        options.collect_metrics = true;
        show_pipeline_summary = true;
    }

    configure_lint_options(
        &mut options,
        &entries,
        lint_enabled_cli,
        &lint_allow_cli,
        &lint_deny_cli,
    )?;

    Ok(CliInvocation {
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
    })
}

#[derive(Debug, Deserialize, Default)]
struct ManifestLintSection {
    enabled: Option<bool>,
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    deny: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct SpectraManifest {
    #[serde(default)]
    lint: Option<ManifestLintSection>,
}

fn parse_raw_lint_rule(value: &str) -> Result<LintRule, String> {
    LintRule::from_str(value).map_err(|_| {
        format!(
            "Unknown lint rule '{}' (valid rules: {}).",
            value,
            lint_rule_list()
        )
    })
}

fn parse_lint_rule_cli(value: &str) -> CliResult<LintRule> {
    parse_raw_lint_rule(value).map_err(|message| usage_error(&message))
}

fn parse_lint_rule_config(value: &str, path: &Path) -> CliResult<LintRule> {
    parse_raw_lint_rule(value)
        .map_err(|message| CliError::usage(format!("{} (found in '{}').", message, path.display())))
}

fn lint_rule_list() -> String {
    LintRule::all()
        .iter()
        .map(LintRule::code)
        .collect::<Vec<_>>()
        .join(", ")
}

fn configure_lint_options(
    options: &mut CompilationOptions,
    entries: &[PathBuf],
    lint_enabled_cli: bool,
    cli_allow: &[LintRule],
    cli_deny: &[LintRule],
) -> CliResult<()> {
    let manifest_path = locate_manifest(entries)?;

    let mut manifest_enabled = None;
    let mut manifest_allow: Vec<LintRule> = Vec::new();
    let mut manifest_deny: Vec<LintRule> = Vec::new();

    if let Some(path) = &manifest_path {
        let contents = fs::read_to_string(path).map_err(|error| {
            CliError::io(format!("Failed to read '{}': {}", path.display(), error))
        })?;

        let manifest: SpectraManifest = toml::from_str(&contents).map_err(|error| {
            CliError::io(format!("Failed to parse '{}': {}", path.display(), error))
        })?;

        if let Some(lint) = manifest.lint {
            manifest_enabled = lint.enabled;
            for rule in lint.allow {
                manifest_allow.push(parse_lint_rule_config(&rule, path)?);
            }
            for rule in lint.deny {
                manifest_deny.push(parse_lint_rule_config(&rule, path)?);
            }
        }
    }

    let mut enable_lints = lint_enabled_cli;
    if let Some(flag) = manifest_enabled {
        enable_lints = flag;
    }
    if lint_enabled_cli {
        enable_lints = true;
    }

    if enable_lints {
        options.lint = LintOptions::all();
    } else {
        options.lint = LintOptions::disabled();
    }

    for rule in manifest_allow {
        options.lint.disable_rule(rule);
    }
    for &rule in cli_allow {
        options.lint.disable_rule(rule);
    }

    for rule in manifest_deny {
        options.lint.deny_rule(rule);
    }
    for &rule in cli_deny {
        options.lint.deny_rule(rule);
    }

    Ok(())
}

fn locate_manifest(entries: &[PathBuf]) -> CliResult<Option<PathBuf>> {
    for entry in entries {
        let metadata = fs::metadata(entry).map_err(|error| {
            CliError::io(format!(
                "Failed to inspect '{}': {}",
                entry.display(),
                error
            ))
        })?;

        let mut current = if metadata.is_dir() {
            Some(entry.clone())
        } else {
            entry.parent().map(Path::to_path_buf)
        };

        while let Some(dir) = current {
            let candidate = dir.join("spectra.toml");
            if candidate.is_file() {
                let canonical = fs::canonicalize(&candidate).map_err(|error| {
                    CliError::io(format!(
                        "Failed to resolve configuration '{}': {}",
                        candidate.display(),
                        error
                    ))
                })?;
                return Ok(Some(canonical));
            }
            current = dir.parent().map(Path::to_path_buf);
        }
    }

    Ok(None)
}

fn parse_repl_invocation<I>(args: &mut std::iter::Peekable<I>) -> CliResult<ReplOptions>
where
    I: Iterator<Item = String>,
{
    let mut options = CompilationOptions::default();
    let mut preload = Vec::new();
    let mut autorun = false;
    let mut show_pipeline_summary = false;
    let mut verbose = false;
    let mut json_output = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" => {
                for remaining in args {
                    preload.push(PathBuf::from(remaining));
                }
                break;
            }
            "--dump-ast" => options.dump_ast = true,
            "--dump-ir" => options.dump_ir = true,
            "--timings" | "-T" => {
                options.collect_metrics = true;
                show_pipeline_summary = true;
            }
            "--no-optimize" | "-O0" => {
                options.optimize = false;
                options.opt_level = 0;
            }
            "-O1" => {
                options.optimize = true;
                options.opt_level = 1;
            }
            "-O2" => {
                options.optimize = true;
                options.opt_level = 2;
            }
            "-O3" => {
                options.optimize = true;
                options.opt_level = 3;
            }
            "--run" | "-r" => {
                autorun = true;
                options.run_jit = true;
            }
            "--summary" | "--pipeline-summary" => {
                options.collect_metrics = true;
                show_pipeline_summary = true;
            }
            "--json" => {
                json_output = true;
            }
            "--verbose" | "-v" => verbose = true,
            flag if flag.starts_with('-') => {
                return Err(usage_error(&format!("Unknown option: {}", flag)));
            }
            _ => preload.push(PathBuf::from(arg)),
        }
    }

    Ok(ReplOptions {
        base_options: options,
        preload,
        autorun,
        show_pipeline_summary,
        verbose,
        json_output,
    })
}

fn parse_new_project_invocation<I>(
    args: &mut std::iter::Peekable<I>,
) -> CliResult<NewProjectOptions>
where
    I: Iterator<Item = String>,
{
    let mut path: Option<PathBuf> = None;
    let mut force = false;

    for arg in args.by_ref() {
        match arg.as_str() {
            "--force" | "-f" => force = true,
            flag if flag.starts_with('-') => {
                return Err(usage_error(&format!("Unknown option: {}", flag)));
            }
            value => {
                if path.is_some() {
                    return Err(usage_error(
                        "Multiple locations provided. Supply exactly one project path.",
                    ));
                }
                path = Some(PathBuf::from(value));
            }
        }
    }

    let path = path.ok_or_else(|| usage_error("No project path supplied."))?;

    Ok(NewProjectOptions { path, force })
}

fn parse_release_info_invocation<I>(
    args: &mut std::iter::Peekable<I>,
) -> CliResult<ReleaseInfoOptions>
where
    I: Iterator<Item = String>,
{
    let mut root = PathBuf::from(".");
    let mut json = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--root" => {
                let value = args
                    .next()
                    .ok_or_else(|| usage_error("Missing path after '--root'."))?;
                root = PathBuf::from(value);
            }
            flag if flag.starts_with('-') => {
                return Err(usage_error(&format!(
                    "Unknown release-info option: {}",
                    flag
                )));
            }
            value => {
                root = PathBuf::from(value);
            }
        }
    }

    Ok(ReleaseInfoOptions { root, json })
}

