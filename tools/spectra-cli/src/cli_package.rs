fn execute_format(options: FormatOptions) -> CliResult<()> {
    run_formatter(options)
}

fn execute_release_info(options: ReleaseInfoOptions) -> CliResult<()> {
    let cli_channel = cli_channel();
    let cli_compatibility = cli_compatibility_level();

    let workspace = match package::resolve(&options.root) {
        Ok(workspace) => Some(workspace),
        Err(package::PackageError::MissingManifest(_)) => None,
        Err(error) => return Err(CliError::io(error.to_string())),
    };

    let warnings = workspace
        .as_ref()
        .map(package::deprecation_warnings)
        .unwrap_or_default();
    for warning in &warnings {
        eprintln!("{}", warning);
    }

    if options.json {
        let packages = workspace
            .as_ref()
            .map(|workspace| {
                workspace
                    .packages
                    .iter()
                    .map(|package| {
                        json!({
                            "name": package.name,
                            "version": package.version,
                            "channel": package.release.channel.as_str(),
                            "compatibility": package.release.compatibility,
                            "deprecated_since": package.release.deprecated_since,
                            "migration": package.release.migration,
                            "manifest": package.manifest.to_string_lossy(),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let payload = json!({
            "schema": "spectralang.release-info.v1",
            "cli": {
                "version": env!("CARGO_PKG_VERSION"),
                "channel": cli_channel.as_str(),
                "compatibility": cli_compatibility,
            },
            "packages": packages,
            "warnings": warnings,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload)
                .map_err(|error| CliError::io(error.to_string()))?
        );
        return Ok(());
    }

    println!("SpectraLang release information");
    println!("  cli.version: {}", env!("CARGO_PKG_VERSION"));
    println!("  cli.channel: {}", cli_channel);
    println!("  cli.compatibility: {}", cli_compatibility);

    if let Some(workspace) = workspace {
        println!("  packages:");
        for package in &workspace.packages {
            println!(
                "    {} {} channel={} compatibility={}",
                package.name,
                package.version,
                package.release.channel,
                package.release.compatibility
            );
            if let Some(deprecated_since) = &package.release.deprecated_since {
                println!(
                    "      deprecated_since={} migration={}",
                    deprecated_since,
                    package.release.migration.as_deref().unwrap_or("")
                );
            }
        }
    } else {
        println!("  packages: none");
    }

    Ok(())
}

fn emit_package_deprecation_warnings(workspace: &package::ResolvedWorkspace) {
    for warning in package::deprecation_warnings(workspace) {
        eprintln!("{}", warning);
    }
}

#[derive(Clone, Debug)]
struct AsyncTestCase {
    name: String,
    module_name: String,
    function_name: String,
    path: PathBuf,
    source: String,
    returns_int: bool,
}

#[derive(Serialize)]
struct PackageTestReport {
    schema: &'static str,
    success: bool,
    listed: bool,
    tests: Vec<PackageTestCaseReport>,
}

#[derive(Serialize)]
struct PackageTestCaseReport {
    name: String,
    path: String,
    status: String,
    detail: Option<String>,
}

fn discover_async_test_cases(entries: &[PathBuf]) -> CliResult<Vec<AsyncTestCase>> {
    let mut cases = Vec::new();

    for path in entries {
        let source = fs::read_to_string(path).map_err(|error| {
            CliError::io(format!(
                "Cannot read async test source '{}': {}",
                path.display(),
                error
            ))
        })?;
        let owned;
        // Same synthetic-header shift as the build plan (D6): raw error spans
        // point one line past the on-disk location when the header is present.
        let (effective_source, line_shift) = if source_has_module_decl(&source) {
            (source.as_str(), 0)
        } else {
            let module_name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(sanitize_module_name)
                .unwrap_or_else(|| "test_module".to_string());
            owned = format!("module {}\n{}", module_name, source);
            (owned.as_str(), 1)
        };

        let tokens = Lexer::new(effective_source)
            .tokenize()
            .map_err(|mut errors| {
                for error in &mut errors {
                    shift_span_lines(&mut error.span, line_shift);
                }
                CliError::compilation(format!(
                    "Cannot lex async test source '{}': {:?}",
                    path.display(),
                    errors
                ))
            })?;
        let module: Module = Parser::new(tokens, HashSet::new())
            .parse()
            .map_err(|mut errors| {
                for error in &mut errors {
                    shift_span_lines(&mut error.span, line_shift);
                }
                CliError::compilation(format!(
                    "Cannot parse async test source '{}': {:?}",
                    path.display(),
                    errors
                ))
            })?;

        for item in &module.items {
            let Item::Function(function) = item else {
                continue;
            };
            if !function
                .attributes
                .iter()
                .any(|attribute| attribute.name == "spectra_async_test")
            {
                continue;
            }
            if !function.is_async {
                return Err(CliError::compilation(format!(
                    "{}::{} uses #[spectra_async_test] but is not async",
                    module.name, function.name
                )));
            }
            if !function.params.is_empty() {
                return Err(CliError::compilation(format!(
                    "{}::{} uses #[spectra_async_test] but declares parameters",
                    module.name, function.name
                )));
            }

            let returns_int = function.return_type.as_ref().is_some_and(|annotation| {
                matches!(
                    &annotation.kind,
                    TypeAnnotationKind::Simple { segments }
                        if segments.len() == 1 && segments[0] == "int"
                )
            });
            cases.push(AsyncTestCase {
                name: format!("{}::{}", module.name, function.name),
                module_name: module.name.clone(),
                function_name: function.name.clone(),
                path: path.clone(),
                source: effective_source.to_string(),
                returns_int,
            });
        }
    }

    cases.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(cases)
}

fn filter_async_tests(cases: Vec<AsyncTestCase>, filter: Option<&str>) -> Vec<AsyncTestCase> {
    match filter {
        Some(filter) => cases
            .into_iter()
            .filter(|case| {
                case.name.contains(filter) || case.path.to_string_lossy().contains(filter)
            })
            .collect(),
        None => cases,
    }
}

fn async_test_wrapper_source(case: &AsyncTestCase) -> String {
    let body = if case.returns_int {
        format!("    return block_on({}())", case.function_name)
    } else {
        format!("    block_on({}());\n    return 0;", case.function_name)
    };

    format!(
        "{}\n\nfunc main() returns int {{\n{}\n}}\n",
        case.source.trim_end(),
        body
    )
}

fn run_async_test_case(
    workspace: &package::ResolvedWorkspace,
    case: &AsyncTestCase,
    index: usize,
) -> PackageTestCaseReport {
    let target_dir = workspace
        .root
        .join("target")
        .join("spectra-async-tests")
        .join(format!("{:04}_{}", index, sanitize_module_name(&case.name)));
    if let Err(error) = fs::create_dir_all(&target_dir) {
        return PackageTestCaseReport {
            name: case.name.clone(),
            path: display_path(&case.path),
            status: "failed".to_string(),
            detail: Some(format!("cannot create test target directory: {}", error)),
        };
    }

    let wrapper_path = target_dir.join(format!(
        "{}.spectra",
        sanitize_module_name(&case.module_name)
    ));
    if let Err(error) = fs::write(&wrapper_path, async_test_wrapper_source(case)) {
        return PackageTestCaseReport {
            name: case.name.clone(),
            path: display_path(&case.path),
            status: "failed".to_string(),
            detail: Some(format!("cannot write generated test wrapper: {}", error)),
        };
    }

    let mut entries = workspace.source_entries_with_origins();
    entries.push(ProjectSourceEntry::plain(wrapper_path));
    let plan = match ProjectPlan::build_with_sources(entries) {
        Ok(plan) => plan,
        Err(error) => {
            return PackageTestCaseReport {
                name: case.name.clone(),
                path: display_path(&case.path),
                status: "failed".to_string(),
                detail: Some(error.to_string()),
            };
        }
    };

    let mut compiler = SpectraCompiler::new(CompilationOptions {
        run_jit: true,
        ..CompilationOptions::default()
    });
    compiler.set_emit_output(false);
    compiler.set_quiet_execution(true);
    if let Some(name) = workspace.root_package_name() {
        compiler.set_package_name(name);
    }

    let (has_failures, _) = compile_plan(BuildCommand::Run, &mut compiler, &plan, false, false);
    if has_failures {
        return PackageTestCaseReport {
            name: case.name.clone(),
            path: display_path(&case.path),
            status: "failed".to_string(),
            detail: Some("compilation failed".to_string()),
        };
    }

    match take_last_exec_exit() {
        Some(0) => PackageTestCaseReport {
            name: case.name.clone(),
            path: display_path(&case.path),
            status: "passed".to_string(),
            detail: None,
        },
        Some(code) => PackageTestCaseReport {
            name: case.name.clone(),
            path: display_path(&case.path),
            status: "failed".to_string(),
            detail: Some(format!("program exited with status {}", code)),
        },
        None => PackageTestCaseReport {
            name: case.name.clone(),
            path: display_path(&case.path),
            status: "failed".to_string(),
            detail: Some("generated wrapper did not execute a main entry point".to_string()),
        },
    }
}

fn execute_package_tests(
    root: &Path,
    options: package::PackageTestOptions,
    offline: bool,
    locked: bool,
) -> CliResult<()> {
    let workspace = package::resolve_with_options(
        root,
        package::ResolveOptions { offline: offline || locked },
    )
    .map_err(|error| CliError::io(error.to_string()))?;
    emit_package_deprecation_warnings(&workspace);
    let test_entries = package::discover_test_entries(&workspace);
    let all_cases = discover_async_test_cases(&test_entries)?;
    let cases = filter_async_tests(all_cases, options.filter.as_deref());
    let lock_path = write_or_verify_package_lockfile(&workspace, locked)?;

    if options.list {
        let tests = cases
            .iter()
            .map(|case| PackageTestCaseReport {
                name: case.name.clone(),
                path: display_path(&case.path),
                status: "listed".to_string(),
                detail: None,
            })
            .collect::<Vec<_>>();
        if options.json {
            let report = PackageTestReport {
                schema: "spectra-package-test-report-v1",
                success: true,
                listed: true,
                tests,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&report)
                    .map_err(|error| CliError::io(error.to_string()))?
            );
        } else {
            println!("     Locked {}", lock_path.display());
            for test in tests {
                println!("{}", test.name);
            }
            println!("     Listed {} async test(s)", cases.len());
        }
        return Ok(());
    }

    if cases.is_empty() {
        let mut entries = workspace.source_entries_with_origins();
        entries.extend(test_entries.into_iter().map(ProjectSourceEntry::plain));
        if !options.json {
            println!("     Locked {}", lock_path.display());
        }
        return execute_plan_with_sources(
            BuildCommand::Check,
            CompilationOptions::default(),
            entries,
            workspace.root_package_name(),
            false,
            false,
            !options.json,
            false,
            None,
        );
    }

    if !options.json {
        println!("     Locked {}", lock_path.display());
        println!("     Running {} async test(s)", cases.len());
    }

    let mut results = Vec::with_capacity(cases.len());
    for (index, case) in cases.iter().enumerate() {
        let result = run_async_test_case(&workspace, case, index);
        if !options.json {
            let marker = if result.status == "passed" {
                "ok"
            } else {
                "FAILED"
            };
            println!("test {} ... {}", result.name, marker);
            if let Some(detail) = &result.detail {
                println!("     {}", detail);
            }
        }
        results.push(result);
    }

    let success = results.iter().all(|result| result.status == "passed");
    if options.json {
        let report = PackageTestReport {
            schema: "spectra-package-test-report-v1",
            success,
            listed: false,
            tests: results,
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .map_err(|error| CliError::io(error.to_string()))?
        );
    } else {
        let passed = results
            .iter()
            .filter(|result| result.status == "passed")
            .count();
        println!(
            "     Result: {} passed; {} failed",
            passed,
            results.len() - passed
        );
    }

    if success {
        Ok(())
    } else {
        Err(CliError::compilation("one or more async tests failed"))
    }
}

fn write_or_verify_package_lockfile(
    workspace: &package::ResolvedWorkspace,
    locked: bool,
) -> CliResult<PathBuf> {
    if locked {
        package::verify_lockfile(workspace).map_err(|error| CliError::io(error.to_string()))
    } else {
        package::write_lockfile(workspace).map_err(|error| CliError::io(error.to_string()))
    }
}

fn execute_package_command(invocation: PackageInvocation) -> CliResult<()> {
    let offline = invocation.offline;
    let locked = invocation.locked;
    match invocation.command {
        PackageCommand::Lock | PackageCommand::Update => {
            let workspace = package::resolve_with_options(
                &invocation.root,
                package::ResolveOptions { offline: offline || locked },
            )
                .map_err(|error| CliError::io(error.to_string()))?;
            emit_package_deprecation_warnings(&workspace);
            let path = write_or_verify_package_lockfile(&workspace, locked)?;
            println!("     Locked {}", path.display());
            Ok(())
        }
        PackageCommand::Build | PackageCommand::Check | PackageCommand::Run => {
            let workspace = package::resolve_with_options(
                &invocation.root,
                package::ResolveOptions { offline: offline || locked },
            )
                .map_err(|error| CliError::io(error.to_string()))?;
            emit_package_deprecation_warnings(&workspace);
            let lock_path = write_or_verify_package_lockfile(&workspace, locked)?;
            let entries = workspace.source_entries_with_origins();
            let kind = match invocation.command {
                PackageCommand::Build => BuildCommand::Compile,
                PackageCommand::Check => BuildCommand::Check,
                PackageCommand::Run => BuildCommand::Run,
                _ => unreachable!(),
            };
            let mut options = CompilationOptions::default();
            if kind == BuildCommand::Run {
                options.run_jit = true;
            }
            println!("     Locked {}", lock_path.display());
            execute_plan_with_sources(
                kind,
                options,
                entries,
                workspace.root_package_name(),
                false,
                false,
                true,
                false,
                None,
            )
        }
        PackageCommand::Test(options) => {
            execute_package_tests(&invocation.root, options, offline, locked)
        }
        PackageCommand::Bench => {
            let workspace = package::resolve_with_options(
                &invocation.root,
                package::ResolveOptions { offline: offline || locked },
            )
                .map_err(|error| CliError::io(error.to_string()))?;
            emit_package_deprecation_warnings(&workspace);
            let lock_path = write_or_verify_package_lockfile(&workspace, locked)?;
            println!("     Locked {}", lock_path.display());
            execute_plan_with_sources(
                BuildCommand::Bench,
                CompilationOptions {
                    collect_metrics: true,
                    ..CompilationOptions::default()
                },
                workspace.source_entries_with_origins(),
                workspace.root_package_name(),
                true,
                true,
                true,
                false,
                None,
            )
        }
        PackageCommand::Doc => {
            let workspace = package::resolve_with_options(
                &invocation.root,
                package::ResolveOptions { offline: offline || locked },
            )
                .map_err(|error| CliError::io(error.to_string()))?;
            emit_package_deprecation_warnings(&workspace);
            let lock_path = write_or_verify_package_lockfile(&workspace, locked)?;
            let docs_path =
                package::write_docs(&workspace).map_err(|error| CliError::io(error.to_string()))?;
            println!("     Locked {}", lock_path.display());
            println!("     Written docs {}", docs_path.display());
            Ok(())
        }
        PackageCommand::Fetch { offline } => {
            let lock_path = package::fetch(&invocation.root, offline || invocation.offline, locked)
                .map_err(|error| CliError::io(error.to_string()))?;
            println!("     Fetched {}", lock_path.display());
            Ok(())
        }
        PackageCommand::Search { query, catalog } => {
            let rows = package::search(&invocation.root, &query, catalog.as_deref())
                .map_err(|error| CliError::io(error.to_string()))?;
            for row in rows {
                println!("{}", row);
            }
            Ok(())
        }
        PackageCommand::Info { name, catalog } => {
            let rows = package::info(&invocation.root, &name, catalog.as_deref())
                .map_err(|error| CliError::io(error.to_string()))?;
            for row in rows {
                println!("{}", row);
            }
            Ok(())
        }
        PackageCommand::Versions { name, catalog } => {
            let rows = package::versions(&invocation.root, &name, catalog.as_deref())
                .map_err(|error| CliError::io(error.to_string()))?;
            for row in rows {
                println!("{}", row);
            }
            Ok(())
        }
        PackageCommand::Tree => {
            let rows = package::dependency_tree(&invocation.root)
                .map_err(|error| CliError::io(error.to_string()))?;
            for row in rows {
                println!("{}", row);
            }
            Ok(())
        }
        PackageCommand::Add {
            name,
            version,
            path,
            registry,
            git,
            tag,
            rev,
            branch,
            catalog,
            allow_floating_git,
        } => {
            let lock_path = package::add_dependency(
                &invocation.root,
                &name,
                version.as_deref(),
                path.as_deref(),
                registry.as_deref(),
                git.as_deref(),
                tag.as_deref(),
                rev.as_deref(),
                branch.as_deref(),
                catalog.as_deref(),
                allow_floating_git,
            )
            .map_err(|error| CliError::io(error.to_string()))?;
            println!("     Added {}", name);
            println!("     Locked {}", lock_path.display());
            Ok(())
        }
        PackageCommand::Register {
            git,
            tag,
            rev,
            branch,
            catalog,
        } => {
            let path = package::register(
                &invocation.root,
                &catalog,
                &git,
                tag.as_deref(),
                rev.as_deref(),
                branch.as_deref(),
            )
            .map_err(|error| CliError::io(error.to_string()))?;
            println!("     Registered {}", path.display());
            Ok(())
        }
        PackageCommand::PublishMetadata {
            out,
            git,
            tag,
            rev,
            branch,
        } => {
            let path = package::write_metadata(
                &invocation.root,
                &out,
                git.as_deref(),
                tag.as_deref(),
                rev.as_deref(),
                branch.as_deref(),
            )
            .map_err(|error| CliError::io(error.to_string()))?;
            println!("     Written metadata {}", path.display());
            Ok(())
        }
        PackageCommand::Catalog(command) => {
            execute_package_catalog_command(&invocation.root, command)
        }
        // APPEND-ONLY (RemoteRegistry): --registry accepts a local path or an http(s) URL.
        PackageCommand::Publish { registry } => {
            let workspace = package::resolve(&invocation.root)
                .map_err(|error| CliError::io(error.to_string()))?;
            emit_package_deprecation_warnings(&workspace);
            let registry_text = registry.to_string_lossy();
            if package::is_remote_registry(&registry_text) {
                let published_url = package::publish_remote(&invocation.root, &registry_text)
                    .map_err(|error| CliError::io(error.to_string()))?;
                println!("     Published {}", published_url);
            } else {
                let package_path = package::publish(&invocation.root, &registry)
                    .map_err(|error| CliError::io(error.to_string()))?;
                println!("     Published {}", package_path.display());
            }
            Ok(())
        }
    }
}

fn execute_package_catalog_command(root: &Path, command: package::CatalogCommand) -> CliResult<()> {
    match command {
        package::CatalogCommand::Add { name, source } => {
            package::catalog_add(root, &name, &source)
                .map_err(|error| CliError::io(error.to_string()))?;
            println!("     Added catalog {} -> {}", name, source);
            Ok(())
        }
        package::CatalogCommand::List => {
            for row in package::catalog_list(root)
                .map_err(|error| CliError::io(error.to_string()))?
            {
                println!("{}", row);
            }
            Ok(())
        }
        package::CatalogCommand::Sync { offline, locked } => {
            let entries = package::catalog_sync(root, offline, locked)
                .map_err(|error| CliError::io(error.to_string()))?;
            for entry in entries {
                println!(
                    "     Synced {} ({})",
                    entry.name,
                    entry.resolved_rev.as_deref().unwrap_or("local")
                );
            }
            Ok(())
        }
        package::CatalogCommand::Remove { name } => {
            package::catalog_remove(root, &name)
                .map_err(|error| CliError::io(error.to_string()))?;
            println!("     Removed catalog {}", name);
            Ok(())
        }
    }
}

