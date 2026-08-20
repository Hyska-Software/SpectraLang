// Full compiler integration
// Provides a backend driver that plugs midend + backend into the shared pipeline.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use spectra_backend::{
    aot::{NativeValueLocation, NativeValueLocationRange}, AotCodeGenerator, AotOptions,
    CodeGenerator, HostCallBatchStats,
};
use spectra_compiler::{
    error::MidendError, lint::LintDiagnostic, pipeline::CompilationMetrics, span::Span,
    BackendDriver, BackendError, CompilationOptions, CompilationPipeline, CompilationResult,
    CompilerError,
};
use spectra_midend::{
    ir::{pretty::format_module, Module as IRModule},
    lowering::ASTLowering,
    passes::{
        concurrent_spawn_join_fusion::ConcurrentSpawnJoinFusion, constant_folding::ConstantFolding,
        dead_code_elimination::DeadCodeElimination, function_inlining::FunctionInlining,
        validation::LoopStructureValidation, verification::verify_module, Pass,
    },
    TensorDevice, TensorGraph,
};

// Thread-local that propagates the Spectra program's return value (used as exit
// code) back to the CLI without changing the BackendDriver trait signature.
thread_local! {
    static LAST_EXEC_EXIT: std::cell::Cell<Option<i32>> =
        const { std::cell::Cell::new(None) };
}

/// Returns and clears the exit code stored by the last JIT execution, if any.
pub fn take_last_exec_exit() -> Option<i32> {
    LAST_EXEC_EXIT.with(|cell| cell.replace(None))
}

/// Sets the program arguments forwarded to `std.env` host functions.
pub fn forward_program_args(args: Vec<String>) {
    spectra_runtime::set_program_args(args);
}

fn emit_r3105_hostcall_stats(label: &str, stats: HostCallBatchStats) {
    if std::env::var_os("SPECTRA_R3105_STATS").is_none() {
        return;
    }
    println!(
        "r3105_hostcall_stats_{label} batched_sites={} batched_hostcalls={} fallback_hostcalls={} argument_arena_bytes={} result_arena_bytes={}",
        stats.batched_sites,
        stats.batched_hostcalls,
        stats.fallback_hostcalls,
        stats.argument_arena_bytes,
        stats.result_arena_bytes,
    );
}

#[derive(Debug)]
struct PassReport {
    name: &'static str,
    duration: Duration,
    modified: bool,
}

#[derive(Debug, Clone)]
pub struct PassSummary {
    pub name: &'static str,
    pub duration: Duration,
    pub modified: bool,
}

#[derive(Debug, Clone)]
pub struct ModulePipelineSummary {
    pub filename: String,
    pub lowering_duration: Duration,
    pub codegen_duration: Duration,
    pub frontend_metrics: Option<CompilationMetrics>,
    pub passes: Vec<PassSummary>,
}

struct CompilationReport {
    artifacts: FullPipelineArtifacts,
    metrics: Option<CompilationMetrics>,
    warnings: Vec<LintDiagnostic>,
}

#[derive(Default, Debug)]
struct PassAggregate {
    total_duration: Duration,
    runs: usize,
    modified_runs: usize,
}

#[derive(Default, Debug)]
struct AggregateMetrics {
    files: usize,
    lowering_total: Duration,
    codegen_total: Duration,
    front_total: Duration,
    lexing_total: Duration,
    parsing_total: Duration,
    semantic_total: Duration,
    backend_total: Duration,
    passes: HashMap<&'static str, PassAggregate>,
}

impl AggregateMetrics {
    fn new() -> Self {
        Self::default()
    }

    fn record(
        &mut self,
        artifacts: &FullPipelineArtifacts,
        front_metrics: Option<&CompilationMetrics>,
    ) {
        self.files += 1;
        self.lowering_total += artifacts.lowering_duration;
        self.codegen_total += artifacts.codegen_duration;

        if let Some(metrics) = front_metrics {
            self.front_total += metrics.total;
            self.lexing_total += metrics.lexing;
            self.parsing_total += metrics.parsing;
            self.semantic_total += metrics.semantic;
            self.backend_total += metrics.backend;
        }

        for pass in &artifacts.passes {
            let entry = self
                .passes
                .entry(pass.name)
                .or_default();
            entry.total_duration += pass.duration;
            entry.runs += 1;
            if pass.modified {
                entry.modified_runs += 1;
            }
        }
    }

    fn print_summary(&self) {
        if self.files == 0 {
            return;
        }

        fn average(duration: Duration, count: usize) -> Duration {
            duration.checked_div(count as u32).unwrap_or(Duration::ZERO)
        }

        println!(
            "\naggregate metrics ({} file{}):",
            self.files,
            if self.files == 1 { "" } else { "s" }
        );

        println!(
            "  - Front-end total: {:?} (avg {:?})",
            self.front_total,
            average(self.front_total, self.files)
        );
        println!(
            "  - Lexing total:    {:?} (avg {:?})",
            self.lexing_total,
            average(self.lexing_total, self.files)
        );
        println!(
            "  - Parsing total:   {:?} (avg {:?})",
            self.parsing_total,
            average(self.parsing_total, self.files)
        );
        println!(
            "  - Semantic total:  {:?} (avg {:?})",
            self.semantic_total,
            average(self.semantic_total, self.files)
        );
        println!(
            "  - Backend total:   {:?} (avg {:?})",
            self.backend_total,
            average(self.backend_total, self.files)
        );
        println!(
            "  - Lowering total:  {:?} (avg {:?})",
            self.lowering_total,
            average(self.lowering_total, self.files)
        );
        println!(
            "  - Codegen total:   {:?} (avg {:?})",
            self.codegen_total,
            average(self.codegen_total, self.files)
        );

        if !self.passes.is_empty() {
            println!("  - Optimization passes:");
            let mut entries: Vec<_> = self.passes.iter().collect();
            entries.sort_by_key(|entry| std::cmp::Reverse(entry.1.total_duration));
            for (name, data) in entries {
                println!(
                    "      - {:<24} {:?} total (runs: {}, modified: {})",
                    name, data.total_duration, data.runs, data.modified_runs
                );
            }
        }
    }
}

#[derive(Debug)]
struct FullPipelineArtifacts {
    pub(crate) ir_module: IRModule,
    passes: Vec<PassReport>,
    lowering_duration: Duration,
    codegen_duration: Duration,
}

/// Compiler-owned metadata consumed by native debug emitters.  Keeping this
/// beside the IR prevents the CLI from reconstructing functions or locals by
/// scanning source text (which was both lossy and capable of inventing PDB
/// records for symbols that did not exist in the object).
#[derive(Debug, Clone)]
pub struct NativeDebugFunction {
    pub name: String,
    pub locals: Vec<String>,
    pub local_offsets: Vec<Option<i64>>,
    pub local_locations: Vec<Vec<NativeValueLocationRange>>,
}

#[derive(Debug, Clone, Default)]
pub struct NativeDebugMetadata {
    pub functions: Vec<NativeDebugFunction>,
}

fn native_debug_metadata(module: &IRModule, executable: bool) -> NativeDebugMetadata {
    let mut functions = module
        .functions
        .iter()
        .map(|function| NativeDebugFunction {
            name: if executable && function.name == "main" {
                "spectra_user_main".to_string()
            } else {
                function.name.clone()
            },
            locals: function
                .locals
                .iter()
                .map(|local| local.name.clone())
                .collect(),
            local_offsets: vec![None; function.locals.len()],
            local_locations: vec![Vec::new(); function.locals.len()],
        })
        .collect::<Vec<_>>();
    functions.retain(|function| !function.name.is_empty());
    NativeDebugMetadata { functions }
}

struct FullPipelineBackend {
    codegen: Option<CodeGenerator>,
    /// When `true`, the "Execution completed" summary is suppressed so that
    /// only the Spectra program's own stdout/stderr is visible.
    quiet_execution: bool,
}

impl FullPipelineBackend {
    fn new() -> Self {
        Self {
            codegen: None,
            quiet_execution: false,
        }
    }
}

impl BackendDriver for FullPipelineBackend {
    type Artifacts = FullPipelineArtifacts;

    fn run(
        &mut self,
        ast: &spectra_compiler::ast::Module,
        options: &CompilationOptions,
    ) -> Result<Self::Artifacts, Vec<CompilerError>> {
        let mut lowering = ASTLowering::new();
        lowering.set_source_file(format!("{}.spectra", ast.name));
        let lowering_start = Instant::now();
        let mut ir_module = match lowering.lower_module(ast) {
            Ok(module) => module,
            Err(errors) => {
                return Err(errors
                    .into_iter()
                    .map(CompilerError::Midend)
                    .collect());
            }
        };
        let lowering_duration = lowering_start.elapsed();

        let mut pass_reports = Vec::new();

        let autodiff_start = Instant::now();
        let autodiff_steps = match spectra_midend::materialize_autodiff_steps(&mut ir_module) {
            Ok(count) => count,
            Err(message) => {
                return Err(vec![CompilerError::Midend(MidendError::new(message))]);
            }
        };
        pass_reports.push(PassReport {
            name: "Compiler-native Autodiff Steps",
            duration: autodiff_start.elapsed(),
            modified: autodiff_steps > 0,
        });

        let verification_start = Instant::now();
        if let Err(errors) = verify_module(&ir_module) {
            let ir_errors = errors
                .into_iter()
                .map(|msg| CompilerError::Midend(MidendError::new(msg)))
                .collect();
            return Err(ir_errors);
        }
        pass_reports.push(PassReport {
            name: "IR Verification (pre-opt)",
            duration: verification_start.elapsed(),
            modified: false,
        });

        let tensor_graph = TensorGraph::from_ir_module(&ir_module);
        let autodiff_graph = spectra_midend::AutodiffGraph::from_tensor_graph(&tensor_graph);
        if options.dump_ir && autodiff_graph.has_gradient_nodes() {
            println!("=== Compiler-native Autodiff IR ===");
            println!("{}", autodiff_graph.stable_dump());
        }
        if tensor_graph
            .functions
            .iter()
            .any(|function| !function.nodes.is_empty())
        {
            let tensor_start = Instant::now();
            let tensor_backend = if tensor_graph.functions.iter().any(|function| {
                function
                    .nodes
                    .iter()
                    .any(|node| node.output.device == TensorDevice::Wgpu)
            }) {
                TensorDevice::Wgpu
            } else {
                TensorDevice::Cpu
            };
            let tensor_backend_name = match &tensor_backend {
                TensorDevice::Cpu => "CPU",
                TensorDevice::Wgpu => "WGPU",
                TensorDevice::Reserved(_) => "RESERVED",
                TensorDevice::Unknown => "UNKNOWN",
            };
            match tensor_graph.lower_for_backend(tensor_backend) {
                Ok(legalized) => {
                    if options.dump_ir {
                        println!("=== Tensor IR ({} legalization) ===", tensor_backend_name);
                        println!("{}", legalized.graph.stable_dump());
                        println!("tensor_ir_report {:?}", legalized.report);
                    }
                }
                Err(errors) => {
                    let ir_errors = errors
                        .into_iter()
                        .map(|error| {
                            CompilerError::Midend(MidendError::new(format!(
                                "{}: tensor IR validation failed in function '{}' node {:?}: {}",
                                error.kind.diagnostic_code(),
                                error.function,
                                error.node,
                                error.message
                            )))
                        })
                        .collect();
                    return Err(ir_errors);
                }
            }
            pass_reports.push(PassReport {
                name: "Tensor IR Legalization (CPU)",
                duration: tensor_start.elapsed(),
                modified: true,
            });
        }

        if options.dump_ir {
            println!("=== IR (before optimization) ===");
            println!("{}", format_module(&ir_module));
            println!();
        }

        if options.optimize {
            let mut fusion = ConcurrentSpawnJoinFusion::new();
            let pass_start = Instant::now();
            let modified = fusion.run(&mut ir_module);
            pass_reports.push(PassReport {
                name: "Concurrent Spawn/Join Fusion",
                duration: pass_start.elapsed(),
                modified,
            });

            if options.opt_level >= 1 {
                let mut cf = ConstantFolding::new();
                let pass_start = Instant::now();
                let modified = cf.run(&mut ir_module);
                pass_reports.push(PassReport {
                    name: "Constant Folding",
                    duration: pass_start.elapsed(),
                    modified,
                });
            }

            if options.opt_level >= 2 {
                let mut inline = FunctionInlining::new();
                let pass_start = Instant::now();
                let modified = inline.run(&mut ir_module);
                pass_reports.push(PassReport {
                    name: "Function Inlining",
                    duration: pass_start.elapsed(),
                    modified,
                });
            }

            if options.opt_level >= 2 {
                let mut dce = DeadCodeElimination::new();
                let pass_start = Instant::now();
                let modified = dce.run(&mut ir_module);
                pass_reports.push(PassReport {
                    name: "Dead Code Elimination",
                    duration: pass_start.elapsed(),
                    modified,
                });
            }
        }

        let mut loop_check = LoopStructureValidation::new();
        let validation_start = Instant::now();
        loop_check.run(&mut ir_module);

        if loop_check.has_errors() {
            let errors: Vec<CompilerError> = loop_check
                .take_errors()
                .into_iter()
                .map(|err| {
                    CompilerError::Midend(MidendError::new(format!(
                        "Loop validation failed in function '{}' at block {} ('{}'): {}",
                        err.function, err.header_block, err.header_label, err.message
                    )))
                })
                .collect();
            return Err(errors);
        }

        pass_reports.push(PassReport {
            name: "Loop Structure Validation",
            duration: validation_start.elapsed(),
            modified: false,
        });

        let verify_after_start = Instant::now();
        if let Err(errors) = verify_module(&ir_module) {
            let ir_errors = errors
                .into_iter()
                .map(|msg| CompilerError::Midend(MidendError::new(msg)))
                .collect();
            return Err(ir_errors);
        }
        pass_reports.push(PassReport {
            name: "IR Verification (post-opt)",
            duration: verify_after_start.elapsed(),
            modified: false,
        });

        if options.dump_ir {
            println!("=== IR (after optimization) ===");
            println!("{}", format_module(&ir_module));
            println!();
        }

        // Reuse the same CodeGenerator (and its underlying JITModule) across all
        // modules in a project build.  This keeps every previously compiled function
        // in the JIT's function_map so that cross-module calls (e.g. main_app
        // calling square() from mathutils) can be resolved correctly.
        let codegen = self.codegen.get_or_insert_with(CodeGenerator::new);
        let codegen_start = Instant::now();
        let codegen_result = codegen.generate_module(&ir_module);
        let codegen_duration = codegen_start.elapsed();

        if let Err(error) = codegen_result {
            return Err(vec![CompilerError::Backend(BackendError::new(
                error.to_string(),
            ))]);
        }
        emit_r3105_hostcall_stats("jit", codegen.hostcall_batch_stats());

        Ok(FullPipelineArtifacts {
            ir_module,
            passes: pass_reports,
            lowering_duration,
            codegen_duration,
        })
    }

    fn execute(
        &mut self,
        artifacts: &Self::Artifacts,
        options: &CompilationOptions,
    ) -> Result<(), Vec<CompilerError>> {
        let codegen = match self.codegen.as_mut() {
            Some(codegen) => codegen,
            None => {
                return Err(vec![CompilerError::Backend(BackendError::new(
                    "JIT execution requested but no code generator is available".to_string(),
                ))]);
            }
        };

        if !artifacts
            .ir_module
            .functions
            .iter()
            .any(|func| func.name == "main")
        {
            // Library modules have no entry point — skip JIT execution silently.
            // The caller is responsible for ensuring that at least one module in
            // the project defines `main`; see execute_plan_with_options().
            return Ok(());
        }

        let _runtime_state = spectra_runtime::initialize();
        // Ensure package host calls are registered before bridging into JITed code.
        spectra_runtime::register_standard_library();
        spectra_api::register();
        let execution_start = Instant::now();

        let return_value = unsafe { codegen.execute_entry_point("main", &artifacts.ir_module) };
        let execution_duration = execution_start.elapsed();

        // Ensure manual allocations do not leak across invocation boundaries.
        spectra_runtime::ffi::spectra_rt_manual_clear();

        let return_value = return_value
            .map_err(|err| vec![CompilerError::Backend(BackendError::new(err.to_string()))])?;

        // Store the program's exit code so the CLI can propagate it.
        let exit_code = return_value.map(|v| v as i32).unwrap_or(0);
        LAST_EXEC_EXIT.with(|cell| cell.set(Some(exit_code)));

        // Print the execution summary only when timing data was requested.
        if !self.quiet_execution || options.collect_metrics {
            if let Some(exit) = return_value {
                println!("    Finished in {:?} (exit: {})", execution_duration, exit);
            } else {
                println!("    Finished in {:?}", execution_duration);
            }
        }

        Ok(())
    }
}
