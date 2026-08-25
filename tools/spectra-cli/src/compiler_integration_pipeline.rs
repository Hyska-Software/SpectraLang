/// Complete compiler that integrates all phases
pub struct SpectraCompiler {
    options: CompilationOptions,
    pipeline: CompilationPipeline<FullPipelineBackend>,
    aggregate: Option<AggregateMetrics>,
    last_summary: Option<ModulePipelineSummary>,
    emit_internal_metrics: bool,
    emit_output: bool,
}

impl SpectraCompiler {
    pub fn new(options: CompilationOptions) -> Self {
        let aggregate = if options.collect_metrics {
            Some(AggregateMetrics::new())
        } else {
            None
        };

        let pipeline =
            CompilationPipeline::new(options.clone()).with_backend(FullPipelineBackend::new());

        Self {
            options,
            pipeline,
            aggregate,
            last_summary: None,
            emit_internal_metrics: true,
            emit_output: true,
        }
    }

    /// Set the package name so the semantic analyzer can enforce `internal` visibility.
    pub fn set_package_name(&mut self, name: impl Into<String>) {
        self.pipeline.package_name = Some(name.into());
    }

    pub fn set_current_package_name(&mut self, name: Option<String>) {
        self.pipeline.package_name = name;
    }

    /// Compile source code to native code
    pub fn compile(&mut self, source: &str, filename: &str) -> Result<(), String> {
        let report = self
            .compile_to_report(source, filename)
            .map_err(|errors| render_errors(&errors, source, filename, "compilation"))?;

        if self.emit_output {
            self.emit_lint_warnings(&report.warnings, filename, source);

            if self.options.optimize && self.emit_internal_metrics && self.options.collect_metrics {
                let modified_passes: Vec<_> = report
                    .artifacts
                    .passes
                    .iter()
                    .filter(|entry| entry.modified)
                    .map(|entry| entry.name)
                    .collect();

                if !modified_passes.is_empty() {
                    println!("optimization passes: {}", modified_passes.join(", "));
                }
            }

            if self.options.collect_metrics
                && self.emit_internal_metrics
                && !report.artifacts.passes.is_empty()
            {
                println!("pass timings:");
                for entry in &report.artifacts.passes {
                    let status = if entry.modified {
                        "modified"
                    } else {
                        "no change"
                    };
                    println!("  {:<28} {:>10?}  {}", entry.name, entry.duration, status);
                }
            }

            if self.options.collect_metrics && self.emit_internal_metrics {
                println!("  lowering   {:?}", report.artifacts.lowering_duration);
                println!("  codegen    {:?}", report.artifacts.codegen_duration);
            }

            if self.emit_internal_metrics {
                if let Some(metrics) = report.metrics.as_ref() {
                    println!("front-end timings:");
                    println!("  lexing     {:?}", metrics.lexing);
                    println!("  parsing    {:?}", metrics.parsing);
                    println!("  semantic   {:?}", metrics.semantic);
                    println!("  backend    {:?}", metrics.backend);
                    println!("  total      {:?}", metrics.total);
                }
            }
        }

        if self.options.run_jit {
            self.pipeline
                .execute_artifacts(&report.artifacts)
                .map_err(|errors| render_errors(&errors, source, filename, "execution"))?;
        }

        Ok(())
    }

    /// Compile a source file to a native object file. Returns the raw object bytes.
    #[allow(dead_code)]
    pub fn compile_to_object_bytes(
        &mut self,
        source: &str,
        filename: &str,
    ) -> Result<Vec<u8>, String> {
        let (bytes, _) = self.compile_to_object_with_debug_metadata(source, filename)?;
        Ok(bytes)
    }

    pub fn compile_to_object_with_debug_metadata(
        &mut self,
        source: &str,
        filename: &str,
    ) -> Result<(Vec<u8>, NativeDebugMetadata), String> {
        let report = self
            .compile_to_report(source, filename)
            .map_err(|errors| render_errors(&errors, source, filename, "compilation"))?;
        let metadata = native_debug_metadata(&report.artifacts.ir_module, false);

        let aot = AotCodeGenerator::new();
        let (bytes, debug_locations, batch_stats, debug_line_rows, debug_frame_sizes) = aot
            .compile_to_object_with_locations_and_stats(
                &report.artifacts.ir_module,
                &AotOptions {
                    native_debug: matches!(
                        self.options.debug_info,
                        spectra_compiler::DebugInfoMode::Native
                    ),
                    ..AotOptions::default()
                },
            )
            .map_err(|err| err.to_string())?;
        emit_r3105_hostcall_stats("aot", batch_stats);
        let mut metadata = metadata;
        for (function_name, value_id, location) in debug_locations {
            if let Some(function) = metadata
                .functions
                .iter_mut()
                .find(|f| f.name == function_name)
            {
                if let Some(ir_function) = report
                    .artifacts
                    .ir_module
                    .functions
                    .iter()
                    .find(|f| f.name == function_name)
                {
                    for (index, _) in ir_function
                        .locals
                        .iter()
                        .enumerate()
                        .filter(|(_, local)| local.value_id == Some(value_id))
                    {
                        if let NativeValueLocation::CfaOffset(offset) = location.location {
                            function.local_offsets[index] = Some(offset);
                        }
                        function.local_locations[index].push(location);
                    }
                }
            }
        }
        for (function_name, offset, line) in debug_line_rows {
            if let Some(function) = metadata
                .functions
                .iter_mut()
                .find(|f| f.name == function_name)
            {
                function.line_rows.push((offset, line));
            }
        }
        for (function_name, frame_size) in debug_frame_sizes {
            if let Some(function) = metadata
                .functions
                .iter_mut()
                .find(|f| f.name == function_name)
            {
                function.frame_size = frame_size;
            }
        }
        Ok((bytes, metadata))
    }

    /// Compile a source file to a native object file that contains a full
    /// executable entry point (`main` shim + `spectra_rt_startup_with_args`).
    /// The resulting bytes must be linked with `libspectra_runtime.a` to produce
    /// a standalone executable.
    #[allow(dead_code)]
    pub fn compile_to_executable_object_bytes(
        &mut self,
        source: &str,
        filename: &str,
    ) -> Result<Vec<u8>, String> {
        let (bytes, _) = self.compile_to_executable_object_with_debug_metadata(source, filename)?;
        Ok(bytes)
    }

    pub fn compile_to_executable_object_with_debug_metadata(
        &mut self,
        source: &str,
        filename: &str,
    ) -> Result<(Vec<u8>, NativeDebugMetadata), String> {
        let report = self
            .compile_to_report(source, filename)
            .map_err(|errors| render_errors(&errors, source, filename, "compilation"))?;
        let metadata = native_debug_metadata(&report.artifacts.ir_module, true);

        let aot = AotCodeGenerator::new();
        let (bytes, debug_locations, batch_stats, debug_line_rows, debug_frame_sizes) = aot
            .compile_to_object_with_locations_and_stats(
                &report.artifacts.ir_module,
                &AotOptions {
                    emit_executable: true,
                    register_api: true,
                    native_debug: matches!(
                        self.options.debug_info,
                        spectra_compiler::DebugInfoMode::Native
                    ),
                },
            )
            .map_err(|err| err.to_string())?;
        emit_r3105_hostcall_stats("aot", batch_stats);
        let mut metadata = metadata;
        for (function_name, value_id, location) in debug_locations {
            let metadata_name = if function_name == "main" {
                "spectra_user_main"
            } else {
                &function_name
            };
            if let Some(function) = metadata
                .functions
                .iter_mut()
                .find(|f| f.name == metadata_name)
            {
                let ir_name = if function_name == "spectra_user_main" {
                    "main"
                } else {
                    &function_name
                };
                if let Some(ir_function) = report
                    .artifacts
                    .ir_module
                    .functions
                    .iter()
                    .find(|f| f.name == ir_name)
                {
                    for (index, _) in ir_function
                        .locals
                        .iter()
                        .enumerate()
                        .filter(|(_, local)| local.value_id == Some(value_id))
                    {
                        if let NativeValueLocation::CfaOffset(offset) = location.location {
                            function.local_offsets[index] = Some(offset);
                        }
                        function.local_locations[index].push(location);
                    }
                }
            }
        }
        for (function_name, offset, line) in debug_line_rows {
            // The executable shim renames the user's `main`; translate back.
            let metadata_name = if function_name == "main" {
                "spectra_user_main".to_string()
            } else {
                function_name
            };
            if let Some(function) = metadata
                .functions
                .iter_mut()
                .find(|f| f.name == metadata_name)
            {
                function.line_rows.push((offset, line));
            }
        }
        for (function_name, frame_size) in debug_frame_sizes {
            let metadata_name = if function_name == "main" {
                "spectra_user_main".to_string()
            } else {
                function_name
            };
            if let Some(function) = metadata
                .functions
                .iter_mut()
                .find(|f| f.name == metadata_name)
            {
                function.frame_size = frame_size;
            }
        }
        Ok((bytes, metadata))
    }

    fn compile_to_report(
        &mut self,
        source: &str,
        filename: &str,
    ) -> Result<CompilationReport, Vec<CompilerError>> {
        self.last_summary = None;

        let compilation = self.pipeline.compile(source, filename)?;

        let CompilationResult {
            backend_artifacts: artifacts,
            metrics,
            warnings,
            ..
        } = compilation;

        if let Some(aggregate) = self.aggregate.as_mut() {
            aggregate.record(&artifacts, metrics.as_ref());
        }

        let pass_summaries = artifacts
            .passes
            .iter()
            .map(|entry| PassSummary {
                name: entry.name,
                duration: entry.duration,
                modified: entry.modified,
            })
            .collect();

        self.last_summary = Some(ModulePipelineSummary {
            filename: filename.to_string(),
            lowering_duration: artifacts.lowering_duration,
            codegen_duration: artifacts.codegen_duration,
            frontend_metrics: metrics.clone(),
            passes: pass_summaries,
        });

        Ok(CompilationReport {
            artifacts,
            metrics,
            warnings,
        })
    }

    pub fn set_emit_output(&mut self, emit: bool) {
        self.emit_output = emit;
    }

    /// When `true`, suppresses the "Execution completed" meta-information line
    /// so that only the Spectra program's own output reaches the terminal.
    /// Automatically cleared when `--timings` / `collect_metrics` is active.
    pub fn set_quiet_execution(&mut self, quiet: bool) {
        self.pipeline.backend_mut().quiet_execution = quiet;
    }

    /// Drain the JIT debug sidecar entries accumulated by the JIT backend
    /// during code generation. The run path writes them via
    /// [`spectra_backend::debug::write_jit_debug_sidecar`].
    pub fn take_jit_debug_functions(
        &mut self,
    ) -> Vec<(String, Vec<spectra_backend::debug::JitDebugVariable>)> {
        self.pipeline.backend_mut().take_jit_debug_functions()
    }

    pub fn compile_for_diagnostics(
        &mut self,
        source: &str,
        filename: &str,
    ) -> Result<Vec<LintDiagnostic>, Vec<CompilerError>> {
        let report = self.compile_to_report(source, filename)?;
        Ok(report.warnings)
    }

    pub fn print_aggregate_summary(&self) {
        if let Some(aggregate) = &self.aggregate {
            aggregate.print_summary();
        }
    }

    pub fn take_last_summary(&mut self) -> Option<ModulePipelineSummary> {
        self.last_summary.take()
    }

    pub fn set_emit_internal_metrics(&mut self, emit: bool) {
        self.emit_internal_metrics = emit;
    }

    fn emit_lint_warnings(&self, warnings: &[LintDiagnostic], filename: &str, source: &str) {
        if warnings.is_empty() {
            return;
        }

        for warning in warnings {
            let message = render_lint_warning(warning, filename, source);
            // Print directly — render_lint_warning already produces the full
            // "warning[...]: ..." diagnostic block; adding another prefix would
            // double-wrap the first line and misalign the source-span gutter.
            eprint!("{}", message);
        }
    }

    /// Compile and execute (JIT)
    #[allow(dead_code)]
    pub fn compile_and_execute(&mut self, source: &str) -> Result<(), String> {
        let compilation = self
            .pipeline
            .compile(source, "<jit>")
            .map_err(|errors| render_errors(&errors, source, "<jit>", "compilation"))?;

        let CompilationResult {
            backend_artifacts: artifacts,
            metrics,
            warnings,
            ..
        } = compilation;

        self.emit_lint_warnings(&warnings, "<jit>", source);

        if self.options.optimize && self.options.collect_metrics {
            let modified_passes: Vec<_> = artifacts
                .passes
                .iter()
                .filter(|report| report.modified)
                .map(|report| report.name)
                .collect();

            if !modified_passes.is_empty() {
                println!("optimization passes: {}", modified_passes.join(", "));
            }
        }

        if self.options.collect_metrics && !artifacts.passes.is_empty() {
            println!("pass timings:");
            for report in &artifacts.passes {
                let status = if report.modified {
                    "modified"
                } else {
                    "no change"
                };
                println!("  {:<28} {:>10?}  {}", report.name, report.duration, status);
            }
        }

        if self.options.collect_metrics {
            println!("  lowering   {:?}", artifacts.lowering_duration);
            println!("  codegen    {:?}", artifacts.codegen_duration);
        }

        if let Some(metrics) = metrics.as_ref() {
            println!("front-end timings:");
            println!("  lexing     {:?}", metrics.lexing);
            println!("  parsing    {:?}", metrics.parsing);
            println!("  semantic   {:?}", metrics.semantic);
            println!("  backend    {:?}", metrics.backend);
            println!("  total      {:?}", metrics.total);
        }

        self.pipeline
            .execute_artifacts(&artifacts)
            .map_err(|errors| render_errors(&errors, source, "<jit>", "execution"))?;

        Ok(())
    }
}
