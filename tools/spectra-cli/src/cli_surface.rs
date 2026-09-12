// `spectralang surface --json` — machine-readable public surface of a project.
//
// The snapshot itself is derived by the compiler
// (`spectra_compiler::semantic::surface`); this module owns the CLI contract:
// flag parsing, project compilation, deterministic JSON emission and the
// token-budget trimming ladder.

use spectra_compiler::semantic::surface::SurfaceSnapshot;

/// Options for `surface --json`.
#[derive(Debug)]
struct SurfaceOptions {
    /// Project root (file or directory) to inspect.
    root: PathBuf,
    /// Optional token budget; when set, output is trimmed deterministically.
    tokens: Option<usize>,
    /// Optional package-name filter.
    package: Option<String>,
    /// Include stdlib/builtin modules in the snapshot.
    include_builtins: bool,
}

#[derive(Serialize)]
struct SurfaceReportJson {
    schema: &'static str,
    success: bool,
    package: Option<String>,
    include_builtins: bool,
    estimated_tokens: usize,
    trimmed: SurfaceTrimJson,
    modules: Vec<SurfaceModuleJson>,
}

#[derive(Serialize, Default)]
struct SurfaceTrimJson {
    applied: Vec<String>,
    functions_dropped: usize,
    types_dropped: usize,
    traits_dropped: usize,
    modules_dropped: usize,
}

#[derive(Serialize)]
struct SurfaceModuleJson {
    path: String,
    package: Option<String>,
    is_builtin: bool,
    functions: Vec<SurfaceFunctionJson>,
    types: Vec<SurfaceTypeJson>,
    traits: Vec<SurfaceTraitJson>,
}

#[derive(Serialize)]
struct SurfaceFunctionJson {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    signature: String,
    is_async: bool,
    visibility: String,
    kind: String,
}

#[derive(Serialize)]
struct SurfaceTypeJson {
    name: String,
    members: Vec<String>,
    is_enum: bool,
    visibility: String,
}

#[derive(Serialize)]
struct SurfaceTraitJson {
    name: String,
    methods: Vec<String>,
    visibility: String,
}

#[derive(Serialize)]
struct SurfaceFailureJson {
    schema: &'static str,
    success: bool,
    error: String,
    diagnostics: Vec<SurfaceDiagnosticJson>,
}

#[derive(Serialize)]
struct SurfaceDiagnosticJson {
    file: String,
    message: String,
    code: Option<String>,
}

const SURFACE_SCHEMA: &str = "spectralang.surface.v1";

fn parse_surface_invocation<I>(args: &mut std::iter::Peekable<I>) -> CliResult<SurfaceOptions>
where
    I: Iterator<Item = String>,
{
    let mut root: Option<PathBuf> = None;
    let mut json = false;
    let mut tokens: Option<usize> = None;
    let mut package: Option<String> = None;
    let mut include_builtins = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--tokens" => {
                let value = args
                    .next()
                    .ok_or_else(|| usage_error("Missing number after --tokens."))?;
                tokens = Some(value.parse::<usize>().map_err(|_| {
                    usage_error("--tokens must be a non-negative integer.")
                })?);
            }
            "--package" => {
                package = Some(
                    args.next()
                        .ok_or_else(|| usage_error("Missing name after --package."))?,
                );
            }
            "--include-builtins" => include_builtins = true,
            "--help" | "-h" => {
                return Err(usage_error(&format!(
                    "Use '{} help surface' for surface command help.",
                    program_name()
                )));
            }
            other if other.starts_with('-') => {
                return Err(usage_error(&format!("Unknown surface option '{other}'.")));
            }
            other => {
                if root.is_some() {
                    return Err(usage_error("surface accepts at most one project path."));
                }
                root = Some(PathBuf::from(other));
            }
        }
    }

    if !json {
        return Err(usage_error("surface requires --json."));
    }

    Ok(SurfaceOptions {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        tokens,
        package,
        include_builtins,
    })
}

/// Approximate token count used for budgeting: four characters per token.
///
/// The estimate is intentionally simple and deterministic; the exact value is
/// reported back in the payload so callers can calibrate against their model.
fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

fn surface_report_from_snapshot(
    snapshot: &SurfaceSnapshot,
    options: &SurfaceOptions,
) -> SurfaceReportJson {
    let modules = snapshot
        .modules
        .iter()
        .map(|module| SurfaceModuleJson {
            path: module.path.clone(),
            package: module.package.clone(),
            is_builtin: module.is_builtin,
            functions: module
                .functions
                .iter()
                .map(|function| SurfaceFunctionJson {
                    name: function.name.clone(),
                    owner: function.owner.clone(),
                    signature: function.signature.clone(),
                    is_async: function.is_async,
                    visibility: function.visibility.clone(),
                    kind: function.kind.clone(),
                })
                .collect(),
            types: module
                .types
                .iter()
                .map(|ty| SurfaceTypeJson {
                    name: ty.name.clone(),
                    members: ty.members.clone(),
                    is_enum: ty.is_enum,
                    visibility: ty.visibility.clone(),
                })
                .collect(),
            traits: module
                .traits
                .iter()
                .map(|declared| SurfaceTraitJson {
                    name: declared.name.clone(),
                    methods: declared.methods.clone(),
                    visibility: declared.visibility.clone(),
                })
                .collect(),
        })
        .collect();

    SurfaceReportJson {
        schema: SURFACE_SCHEMA,
        success: true,
        package: options.package.clone(),
        include_builtins: options.include_builtins,
        estimated_tokens: 0,
        trimmed: SurfaceTrimJson::default(),
        modules,
    }
}

fn serialize_surface_report(report: &SurfaceReportJson) -> CliResult<String> {
    serde_json::to_string(report)
        .map_err(|error| CliError::io(format!("Failed to serialize surface report: {error}")))
}

/// Trim the report deterministically until it fits the budget.
///
/// Ladder order (each step is recorded in `trimmed.applied`):
/// 1. drop type member lists, 2. drop traits, 3. drop types no function
/// signature references, 4. drop functions from the tail of each module,
/// 5. drop modules from the tail.
fn apply_surface_token_budget(report: &mut SurfaceReportJson, budget: usize) -> CliResult<()> {
    let measure = |report: &SurfaceReportJson| -> CliResult<usize> {
        Ok(estimate_tokens(&serialize_surface_report(report)?))
    };

    if measure(report)? <= budget {
        return Ok(());
    }

    for module in &mut report.modules {
        for ty in &mut module.types {
            ty.members.clear();
        }
    }
    report.trimmed.applied.push("type_members".to_string());
    if measure(report)? <= budget {
        return Ok(());
    }

    let traits_dropped: usize = report.modules.iter().map(|module| module.traits.len()).sum();
    for module in &mut report.modules {
        module.traits.clear();
    }
    report.trimmed.traits_dropped = traits_dropped;
    report.trimmed.applied.push("traits".to_string());
    if measure(report)? <= budget {
        return Ok(());
    }

    for module in &mut report.modules {
        let signatures: Vec<&str> = module.functions.iter().map(|f| f.signature.as_str()).collect();
        let before = module.types.len();
        module
            .types
            .retain(|ty| signatures.iter().any(|signature| signature.contains(&ty.name)));
        report.trimmed.types_dropped += before - module.types.len();
    }
    report.trimmed.applied.push("unreferenced_types".to_string());
    if measure(report)? <= budget {
        return Ok(());
    }

    loop {
        if measure(report)? <= budget {
            break;
        }
        let mut dropped_this_round = 0usize;
        for module in &mut report.modules {
            // Drop ten percent of the tail (at least one) per round so large
            // projects converge without one serialization per function.
            let total = module.functions.len();
            if total == 0 {
                continue;
            }
            let step = (total / 10).max(1);
            let new_len = total.saturating_sub(step);
            module.functions.truncate(new_len);
            dropped_this_round += total - new_len;
        }
        report.trimmed.functions_dropped += dropped_this_round;
        if dropped_this_round == 0 {
            break;
        }
    }
    report.trimmed.applied.push("function_tail".to_string());
    if measure(report)? <= budget {
        return Ok(());
    }

    loop {
        if measure(report)? <= budget {
            break;
        }
        if report.modules.pop().is_none() {
            break;
        }
        report.trimmed.modules_dropped += 1;
    }
    report.trimmed.applied.push("module_tail".to_string());
    Ok(())
}

fn emit_surface_value<T: Serialize>(value: &T) -> CliResult<()> {
    let mut stdout = io::stdout();
    serde_json::to_writer(&mut stdout, value)
        .map_err(|error| CliError::io(format!("Failed to serialize surface output: {error}")))?;
    stdout
        .write_all(b"\n")
        .map_err(|error| CliError::io(format!("Failed to write surface output: {error}")))?;
    Ok(())
}

fn emit_surface_failure(error: &str, diagnostics: Vec<SurfaceDiagnosticJson>) -> CliResult<()> {
    emit_surface_value(&SurfaceFailureJson {
        schema: SURFACE_SCHEMA,
        success: false,
        error: error.to_string(),
        diagnostics,
    })
}

fn surface_error_code(error: &CompilerError) -> Option<String> {
    match error {
        CompilerError::Lexical(lex) => lex.code.clone(),
        CompilerError::Parse(parse) => parse.code.clone(),
        CompilerError::Semantic(semantic) => semantic.code.clone(),
        CompilerError::Midend(_) | CompilerError::Backend(_) => None,
    }
}

fn execute_surface(options: SurfaceOptions) -> CliResult<()> {
    if !options.root.exists() {
        let message = format!("Project path '{}' does not exist.", options.root.display());
        emit_surface_failure(&message, Vec::new())?;
        return Err(usage_error(&message));
    }

    let plan = match ProjectPlan::build(vec![options.root.clone()]) {
        Ok(plan) => plan,
        Err(error) => {
            let message = format!("{error}");
            emit_surface_failure(&message, Vec::new())?;
            return Err(CliError::compilation(message));
        }
    };

    if plan.modules().is_empty() {
        let message = "No Spectra modules were found in the project.".to_string();
        emit_surface_failure(&message, Vec::new())?;
        return Err(CliError::compilation(message));
    }

    let mut compile_options = CompilationOptions::default();
    compile_options.run_jit = false;
    let mut compiler = SpectraCompiler::new(compile_options);
    compiler.set_emit_internal_metrics(false);
    compiler.set_emit_output(false);

    let mut diagnostics: Vec<SurfaceDiagnosticJson> = Vec::new();
    for module in plan.modules() {
        let path = module.path.clone();
        let display_path = path_to_string(&path);
        let source = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) => {
                let message = format!("Failed to read '{}': {error}", path.display());
                emit_surface_failure(&message, Vec::new())?;
                return Err(CliError::io(message));
            }
        };
        compiler.set_current_package_name(module.package_name.clone());
        if let Err(errors) = compiler.compile_for_diagnostics(&source, &display_path) {
            for error in errors {
                diagnostics.push(SurfaceDiagnosticJson {
                    file: display_path.clone(),
                    message: error.to_string(),
                    code: surface_error_code(&error),
                });
            }
        }
    }

    if !diagnostics.is_empty() {
        let message = format!(
            "surface failed: {} diagnostic(s) across the project.",
            diagnostics.len()
        );
        emit_surface_failure(&message, diagnostics)?;
        return Err(CliError::compilation(message));
    }

    let registry = compiler.registry();
    let snapshot = {
        let guard = registry
            .read()
            .map_err(|_| CliError::compilation("Module registry lock was poisoned."))?;
        spectra_compiler::semantic::surface::snapshot_from_registry(
            &guard,
            options.include_builtins,
        )
    };

    let mut report = surface_report_from_snapshot(&snapshot, &options);

    if let Some(package) = &options.package {
        report
            .modules
            .retain(|module| module.package.as_deref() == Some(package.as_str()));
        if report.modules.is_empty() {
            let message = format!("No modules found for package '{package}'.");
            emit_surface_failure(&message, Vec::new())?;
            return Err(CliError::compilation(message));
        }
    }

    if let Some(budget) = options.tokens {
        apply_surface_token_budget(&mut report, budget)?;
    }
    report.estimated_tokens = estimate_tokens(&serialize_surface_report(&report)?);

    emit_surface_value(&report)
}
