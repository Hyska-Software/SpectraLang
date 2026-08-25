fn execute_async_benchmarks(bench_json: Option<PathBuf>) -> CliResult<()> {
    spectra_runtime::initialize();
    spectra_runtime::register_standard_library();
    spectra_api::register();

    let mut benchmarks = Vec::new();
    let suite_start = Instant::now();

    for &count in ASYNC_BENCH_COUNTS {
        benchmarks.push(run_async_benchmark_case(count)?);
    }

    let total_ms = duration_ms(suite_start.elapsed());
    let total_tasks = benchmarks
        .iter()
        .map(|bench| bench.concurrent_tasks)
        .sum::<usize>();
    let min_throughput_tasks_per_sec = benchmarks
        .iter()
        .map(|bench| bench.throughput_tasks_per_sec)
        .fold(f64::INFINITY, f64::min);
    let max_p99_latency_ns = benchmarks
        .iter()
        .map(|bench| bench.p99_latency_ns)
        .max()
        .unwrap_or(0);

    let report = AsyncBenchReport {
        schema: ASYNC_BENCH_SCHEMA,
        version: 1,
        runtime: "spectra-runtime-hostcalls",
        totals: AsyncBenchTotals {
            cases: benchmarks.len(),
            total_tasks,
            total_ms,
            min_throughput_tasks_per_sec,
            max_p99_latency_ns,
        },
        benchmarks,
    };

    let text = serde_json::to_string_pretty(&report).map_err(|error| {
        CliError::io(format!("Failed to serialize async bench report: {}", error))
    })?;

    if let Some(path) = bench_json {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|error| {
                    CliError::io(format!(
                        "Failed to create async bench report directory '{}': {}",
                        parent.display(),
                        error
                    ))
                })?;
            }
        }
        fs::write(&path, format!("{}\n", text)).map_err(|error| {
            CliError::io(format!(
                "Failed to write async bench report '{}': {}",
                path.display(),
                error
            ))
        })?;
        println!("     Written async bench report {}", path.display());
    } else {
        println!("{}", text);
    }

    Ok(())
}

fn run_async_benchmark_case(concurrent_tasks: usize) -> CliResult<AsyncBenchCaseReport> {
    let _ = call_async_host_i64("spectra.async.task.reset", &[])?;

    let total_start = Instant::now();
    let first_task = call_async_host_i64(
        "spectra.async.task.ready_batch",
        &[concurrent_tasks as i64, 1],
    )?;
    let sample_count = concurrent_tasks.min(ASYNC_BENCH_MAX_LATENCY_SAMPLES);
    let mut latencies = Vec::with_capacity(sample_count);

    for sample_index in 0..sample_count {
        let index = if sample_count <= 1 {
            0
        } else {
            sample_index * (concurrent_tasks - 1) / (sample_count - 1)
        };
        let task = first_task + index as i64;
        let ready = call_async_host_i64("spectra.async.task.poll", &[task])?;
        if ready == 0 {
            return Err(CliError::compilation(
                "async benchmark expected ready task to poll successfully",
            ));
        }
    }

    for sample_index in 0..sample_count {
        let index = if sample_count <= 1 {
            0
        } else {
            sample_index * (concurrent_tasks - 1) / (sample_count - 1)
        };
        let task = first_task + index as i64;
        let latency_start = Instant::now();
        let result = call_async_host_i64("spectra.async.task.result", &[task])?;
        if result != (index as i64) + 1 {
            return Err(CliError::compilation(
                "async benchmark observed an unexpected sampled task result",
            ));
        }
        latencies.push(latency_start.elapsed().as_nanos());
    }
    let checksum = call_async_host_i64(
        "spectra.async.task.batch_checksum",
        &[first_task, concurrent_tasks as i64],
    )?;

    let total_elapsed = total_start.elapsed();
    latencies.sort_unstable();

    Ok(AsyncBenchCaseReport {
        id: format!("ready_task_{}", concurrent_tasks),
        concurrent_tasks,
        concurrent_connections: concurrent_tasks,
        samples: sample_count,
        p50_latency_ns: percentile_latency(&latencies, 50),
        p95_latency_ns: percentile_latency(&latencies, 95),
        p99_latency_ns: percentile_latency(&latencies, 99),
        throughput_tasks_per_sec: concurrent_tasks as f64 / total_elapsed.as_secs_f64().max(1.0e-9),
        total_ms: duration_ms(total_elapsed),
        checksum,
    })
}

fn percentile_latency(sorted_latencies: &[u128], percentile: usize) -> u128 {
    if sorted_latencies.is_empty() {
        return 0;
    }
    let clamped = percentile.min(100);
    let idx = ((sorted_latencies.len() - 1) * clamped).div_ceil(100);
    sorted_latencies[idx]
}

fn call_async_host_i64(name: &str, args: &[i64]) -> CliResult<i64> {
    let mut results = [0i64; 1];
    let status = spectra_runtime::ffi::spectra_rt_host_invoke(
        name.as_ptr(),
        name.len(),
        args.as_ptr(),
        args.len(),
        results.as_mut_ptr(),
        results.len(),
    );
    if status != spectra_runtime::ffi::HOST_STATUS_SUCCESS {
        return Err(CliError::compilation(format!(
            "async benchmark host call '{}' failed with status {}",
            name, status
        )));
    }
    Ok(results[0])
}

#[allow(clippy::too_many_arguments)]
fn execute_plan_with_options(
    kind: BuildCommand,
    options: CompilationOptions,
    entries: Vec<PathBuf>,
    package_name: Option<String>,
    show_pipeline_summary: bool,
    show_aggregate_summary: bool,
    print_success: bool,
    verbose: bool,
    bench_json: Option<PathBuf>,
) -> CliResult<()> {
    execute_plan_with_sources(
        kind,
        options,
        entries.into_iter().map(ProjectSourceEntry::plain).collect(),
        package_name,
        show_pipeline_summary,
        show_aggregate_summary,
        print_success,
        verbose,
        bench_json,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_plan_with_sources(
    kind: BuildCommand,
    options: CompilationOptions,
    entries: Vec<ProjectSourceEntry>,
    package_name: Option<String>,
    show_pipeline_summary: bool,
    show_aggregate_summary: bool,
    print_success: bool,
    verbose: bool,
    bench_json: Option<PathBuf>,
) -> CliResult<()> {
    let plan = ProjectPlan::build_with_sources(entries)
        .map_err(|error| CliError::io(error.to_string()))?;

    if plan.modules().is_empty() {
        return Err(CliError::usage("No Spectra source files found to compile."));
    }

    if verbose {
        print_verbose_configuration(kind, &options);
        println!();
        println!(
            "Project plan contains {} module{}:",
            plan.modules().len(),
            if plan.modules().len() == 1 { "" } else { "s" }
        );
        for (index, module) in plan.modules().iter().enumerate() {
            println!(
                "  {:>2}. {} ({})",
                index + 1,
                module.name,
                module.path.display()
            );
            if !module.imports.is_empty() {
                println!("       imports: {}", module.imports.join(", "));
            }
        }
    }

    // Captured before `options` moves into the compiler.
    let collect_metrics = options.collect_metrics;
    let mut compiler = SpectraCompiler::new(options);
    if let Some(name) = package_name {
        compiler.set_package_name(name);
    }

    if show_pipeline_summary {
        compiler.set_emit_internal_metrics(false);
    }

    // For `run` without `--verbose` / `--timings`, suppress compile banners and
    // the post-execution metadata line so only the program's output is visible.
    if kind == BuildCommand::Run && !verbose {
        compiler.set_emit_output(false);
        compiler.set_quiet_execution(true);
    }

    let (has_failures, summaries) =
        compile_plan(kind, &mut compiler, &plan, show_pipeline_summary, verbose);

    // JIT debug sidecar: on `run`, write `<source>.spectra-jit-debug.json`
    // next to the entry source when `--timings` is active or the user set
    // SPECTRA_JIT_DEBUG=1. The records are compiler-proven value-label data;
    // a write failure is a warning, never fatal.
    if kind == BuildCommand::Run {
        let collected = compiler.take_jit_debug_functions();
        if !collected.is_empty() {
            let entry_source = plan.modules()[0].path.display().to_string();
            match spectra_backend::debug::write_jit_debug_sidecar(
                &entry_source,
                &collected,
                collect_metrics,
            ) {
                Ok(Some(path)) => {
                    if verbose || collect_metrics {
                        println!("JIT debug sidecar written to {}", path.display());
                    }
                }
                Ok(None) => {}
                Err(error) => eprintln!("warning: {error}"),
            }
        }
    }

    if show_aggregate_summary {
        compiler.print_aggregate_summary();
    }

    if has_failures {
        return Err(CliError::compilation(
            "could not compile due to previous error(s)",
        ));
    }

    if let Some(report) = spectra_runtime::concurrent_diagnostics_report_json() {
        println!("SPECTRA_CONCURRENT_DIAGNOSTICS={report}");
    }

    // Propagate the Spectra program's exit code when running via JIT.
    if kind == BuildCommand::Run {
        match take_last_exec_exit() {
            Some(code) => {
                if code != 0 {
                    if let Some((path, line, column)) = find_main_location(&plan) {
                        eprintln!(
                            "error[runtime]: program exited with status {}\n  --> {}:{}:{}\n   |\n   = stack:\n     0: main() at {}:{}:{}\n   = help: inspect frame 0 and rerun with '--timings' for pipeline context",
                            code,
                            path.display(),
                            line,
                            column,
                            path.display(),
                            line,
                            column
                        );
                    } else {
                        eprintln!(
                            "error[runtime]: program exited with status {}\n   = stack:\n     0: <entry point unavailable>\n   = help: rerun with '--timings' for pipeline context",
                            code
                        );
                    }
                    std::process::exit(code);
                }
            }
            None => {
                // No module defined `main` — nothing was executed.
                return Err(CliError::compilation(
                    "no entry point 'main' found; define a 'public func main() returns int' function",
                ));
            }
        }
    }

    if print_success && kind != BuildCommand::Run {
        let msg = kind.success_message();
        if !msg.is_empty() {
            println!("{}", msg);
        }
    }

    if kind == BuildCommand::Bench {
        if let Some(path) = bench_json {
            write_bench_report(&path, &summaries)?;
            println!("     Written bench report {}", path.display());
        }
    }

    Ok(())
}

