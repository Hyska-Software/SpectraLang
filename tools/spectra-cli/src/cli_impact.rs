// `spectralang impact --json <symbol>` — "what breaks if I change this".
//
// Answers from the SIR call graph (`spectra_midend::callgraph`), not from text
// search, and reports unresolved dynamic sites instead of pretending the
// answer is complete.

use spectra_midend::callgraph::ImpactIndex;
use spectra_midend::ir::Module as IrModule;

/// Options for `impact`.
#[derive(Debug)]
struct ImpactOptions {
    symbol: String,
    root: PathBuf,
    json: bool,
}

#[derive(Serialize)]
struct ImpactReportJson {
    schema: &'static str,
    success: bool,
    symbol: String,
    affected_functions: Vec<String>,
    affected_types: Vec<String>,
    affected_schemas: Vec<String>,
    dynamic_dispatch: bool,
    unresolved: Vec<String>,
    fixtures: Vec<String>,
    eval_cases: Vec<String>,
    modules_scanned: Vec<String>,
}

#[derive(Serialize)]
struct ImpactFailureJson {
    schema: &'static str,
    success: bool,
    symbol: String,
    error: String,
    near_matches: Vec<String>,
}

const IMPACT_SCHEMA: &str = "spectralang.impact.v1";

fn parse_impact_invocation<I>(args: &mut std::iter::Peekable<I>) -> CliResult<ImpactOptions>
where
    I: Iterator<Item = String>,
{
    let mut symbol: Option<String> = None;
    let mut root: Option<PathBuf> = None;
    let mut json = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--help" | "-h" => {
                return Err(usage_error(&format!(
                    "Use '{} help impact' for impact command help.",
                    program_name()
                )));
            }
            other if other.starts_with('-') => {
                return Err(usage_error(&format!("Unknown impact option '{other}'.")));
            }
            other => {
                if symbol.is_none() {
                    symbol = Some(other.to_string());
                } else if root.is_none() {
                    root = Some(PathBuf::from(other));
                } else {
                    return Err(usage_error(
                        "impact accepts a symbol and at most one project path.",
                    ));
                }
            }
        }
    }

    if !json {
        return Err(usage_error("impact requires --json."));
    }
    let Some(symbol) = symbol else {
        return Err(usage_error("impact requires a symbol (Module::function or Type.field)."));
    };

    Ok(ImpactOptions {
        symbol,
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        json,
    })
}

#[derive(Debug)]
struct ResolvedSymbol {
    /// Module qualifier from `Module::symbol`, when present.
    module: Option<String>,
    /// Field symbol (`Type.field`) when the query names a field.
    field: Option<String>,
    /// Owning type for field and method queries.
    owner_type: Option<String>,
    /// Caller-lookup candidates in priority order.
    call_candidates: Vec<String>,
}

fn resolve_impact_symbol(symbol: &str) -> ResolvedSymbol {
    let (module, remainder) = match symbol.split_once("::") {
        Some((module, remainder)) => (Some(module.to_string()), remainder),
        None => (None, symbol),
    };

    match remainder.split_once('.') {
        Some((owner, member)) => ResolvedSymbol {
            module,
            field: Some(format!("{owner}.{member}")),
            owner_type: Some(owner.to_string()),
            // Methods mangle as `Type_method`; keep the bare member as a
            // fallback for free functions queried with a stray qualifier.
            call_candidates: vec![format!("{owner}_{member}"), member.to_string()],
        },
        None => ResolvedSymbol {
            module,
            field: None,
            owner_type: None,
            call_candidates: vec![remainder.to_string()],
        },
    }
}

/// Catalog fixtures whose entry name matches the queried symbol.
///
/// The generated catalog is machine-written and regular, so a block scan keeps
/// this command dependency-free (no TOML parser in the CLI).
fn impact_fixtures(root: &Path, symbol: &str) -> Vec<String> {
    let catalog = root.join("packages/spectra-contract/catalog/stdlib.toml");
    let Ok(text) = fs::read_to_string(&catalog) else {
        return Vec::new();
    };

    let mut fixtures: Vec<String> = Vec::new();
    let mut current_path = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "[[entry]]" || trimmed == "[[entries]]" {
            current_path.clear();
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("path = ") {
            current_path = value.trim_matches('"').to_string();
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("fixture = ") {
            let fixture = value.trim_matches('"').to_string();
            if fixture.is_empty() {
                continue;
            }
            let last_segment = current_path.rsplit('.').next().unwrap_or("");
            let full_match = current_path == symbol;
            let name_match = last_segment == symbol;
            if (full_match || name_match) && !fixtures.contains(&fixture) {
                fixtures.push(fixture);
            }
        }
    }
    fixtures.sort();
    fixtures.truncate(20);
    fixtures
}

/// Eval cases that mention the symbol, when an eval directory exists.
fn impact_eval_cases(root: &Path, symbol: &str) -> Vec<String> {
    let mut cases: Vec<String> = Vec::new();
    let mut directories = vec![root.join("examples/agent/evals"), root.join("evals")];
    directories.retain(|directory| directory.is_dir());

    for directory in directories {
        let mut stack = vec![directory];
        while let Some(current) = stack.pop() {
            let Ok(entries) = fs::read_dir(&current) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(text) = fs::read_to_string(&path) {
                    if text.contains(symbol) {
                        let display = path
                            .strip_prefix(root)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .replace('\\', "/");
                        if !cases.contains(&display) {
                            cases.push(display);
                        }
                    }
                }
            }
        }
    }
    cases.sort();
    cases.truncate(20);
    cases
}

fn emit_impact_json<T: Serialize>(value: &T) -> CliResult<()> {
    let mut stdout = io::stdout();
    serde_json::to_writer(&mut stdout, value)
        .map_err(|error| CliError::io(format!("Failed to serialize impact output: {error}")))?;
    stdout
        .write_all(b"\n")
        .map_err(|error| CliError::io(format!("Failed to write impact output: {error}")))?;
    Ok(())
}

fn impact_symbol_exists(modules: &[(String, IrModule)], candidates: &[String]) -> bool {
    modules.iter().any(|(_, module)| {
        candidates.iter().any(|candidate| {
            module.functions.iter().any(|function| &function.name == candidate)
                || module
                    .external_functions
                    .iter()
                    .any(|function| &function.name == candidate)
        })
    })
}

/// True when a nominal type name appears anywhere in the module's IR types.
fn impact_type_exists(module: &IrModule, type_name: &str) -> bool {
    module.functions.iter().any(|function| {
        ir_type_mentions(&function.return_type, type_name)
            || function
                .params
                .iter()
                .any(|param| ir_type_mentions(&param.ty, type_name))
            || function
                .locals
                .iter()
                .any(|local| ir_type_mentions(&local.ty, type_name))
    })
}

fn ir_type_mentions(type_: &spectra_midend::ir::Type, type_name: &str) -> bool {
    use spectra_midend::ir::Type as IrType;
    match type_ {
        IrType::Struct { name, .. } => name == type_name,
        IrType::Enum { name, .. } => name == type_name,
        IrType::DynTrait { trait_name, .. } => trait_name == type_name,
        IrType::Array { element_type, .. } => ir_type_mentions(element_type, type_name),
        IrType::Pointer(inner) => ir_type_mentions(inner, type_name),
        IrType::Tuple { elements } => {
            elements.iter().any(|element| ir_type_mentions(element, type_name))
        }
        IrType::Generic {
            name,
            args,
            representation,
        } => {
            name == type_name
                || args.iter().any(|arg| ir_type_mentions(arg, type_name))
                || ir_type_mentions(representation, type_name)
        }
        IrType::Task { output } => ir_type_mentions(output, type_name),
        IrType::Function {
            params,
            return_type,
        } => {
            params.iter().any(|param| ir_type_mentions(param, type_name))
                || ir_type_mentions(return_type, type_name)
        }
        _ => false,
    }
}

fn impact_known_symbols(modules: &[(String, IrModule)]) -> Vec<String> {
    let mut symbols: Vec<String> = Vec::new();
    for (_, module) in modules {
        for function in &module.functions {
            symbols.push(function.name.clone());
        }
    }
    symbols.sort();
    symbols.dedup();
    symbols
}

fn impact_near_matches(known: &[String], query: &str) -> Vec<String> {
    let mut scored: Vec<(usize, String)> = known
        .iter()
        .filter_map(|candidate| {
            let distance = impact_levenshtein(query, candidate);
            if distance <= 4 {
                Some((distance, candidate.clone()))
            } else {
                None
            }
        })
        .collect();
    scored.sort();
    scored.into_iter().map(|(_, name)| name).take(8).collect()
}

fn impact_levenshtein(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0usize; right.len() + 1];
    for (row, left_char) in left.iter().enumerate() {
        current[0] = row + 1;
        for (column, right_char) in right.iter().enumerate() {
            let cost = if left_char == right_char { 0 } else { 1 };
            current[column + 1] = (previous[column + 1] + 1)
                .min(current[column] + 1)
                .min(previous[column] + cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

fn execute_impact(options: ImpactOptions) -> CliResult<()> {
    if !options.root.exists() {
        let message = format!("Project path '{}' does not exist.", options.root.display());
        emit_impact_json(&ImpactFailureJson {
            schema: IMPACT_SCHEMA,
            success: false,
            symbol: options.symbol.clone(),
            error: message.clone(),
            near_matches: Vec::new(),
        })?;
        return Err(usage_error(&message));
    }

    let plan = match ProjectPlan::build(vec![options.root.clone()]) {
        Ok(plan) => plan,
        Err(error) => {
            let message = format!("{error}");
            emit_impact_json(&ImpactFailureJson {
                schema: IMPACT_SCHEMA,
                success: false,
                symbol: options.symbol.clone(),
                error: message.clone(),
                near_matches: Vec::new(),
            })?;
            return Err(CliError::compilation(message));
        }
    };

    if plan.modules().is_empty() {
        let message = "No Spectra modules were found in the project.".to_string();
        emit_impact_json(&ImpactFailureJson {
            schema: IMPACT_SCHEMA,
            success: false,
            symbol: options.symbol.clone(),
            error: message.clone(),
            near_matches: Vec::new(),
        })?;
        return Err(CliError::compilation(message));
    }

    let mut compile_options = CompilationOptions::default();
    compile_options.run_jit = false;
    // Impact analysis reads the call graph as written. Running the optimizer
    // first would inline callees into their callers and erase the very edges
    // the index reports, so this compile stays unoptimized on purpose.
    compile_options.optimize = false;
    let mut compiler = SpectraCompiler::new(compile_options);
    compiler.set_emit_internal_metrics(false);
    compiler.set_emit_output(false);

    let mut modules: Vec<(String, IrModule)> = Vec::new();
    for module in plan.modules() {
        let path = module.path.clone();
        let source = fs::read_to_string(&path)
            .map_err(|error| CliError::io(format!("Failed to read '{}': {error}", path.display())))?;
        compiler.set_current_package_name(module.package_name.clone());
        match compiler.compile_module_ir(&source, &path_to_string(&path)) {
            Ok(ir_module) => modules.push((module.name.clone(), ir_module)),
            Err(errors) => {
                let message = format!(
                    "impact failed to compile '{}' ({} diagnostic(s)).",
                    path.display(),
                    errors.len()
                );
                emit_impact_json(&ImpactFailureJson {
                    schema: IMPACT_SCHEMA,
                    success: false,
                    symbol: options.symbol.clone(),
                    error: message.clone(),
                    near_matches: Vec::new(),
                })?;
                return Err(CliError::compilation(message));
            }
        }
    }

    let resolved = resolve_impact_symbol(&options.symbol);
    let scoped: Vec<&(String, IrModule)> = modules
        .iter()
        .filter(|(name, _)| {
            resolved
                .module
                .as_deref()
                .is_none_or(|wanted| wanted == name)
        })
        .collect();

    let mut affected_functions: Vec<String> = Vec::new();
    let mut field_users: Vec<String> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();

    for (module_name, ir_module) in &scoped {
        let index = ImpactIndex::build(ir_module);
        if let Some(field) = &resolved.field {
            field_users.extend(
                index
                    .field_users_of(field)
                    .into_iter()
                    .map(|function| format!("{module_name}::{function}")),
            );
        }
        for candidate in &resolved.call_candidates {
            affected_functions.extend(
                index
                    .callers_of(candidate)
                    .into_iter()
                    .map(|function| format!("{module_name}::{function}")),
            );
        }
        unresolved.extend(
            index
                .unresolved_dynamic()
                .into_iter()
                .map(|function| format!("{module_name}::{function}")),
        );
    }

    // Field queries exist when the field is used somewhere or the declaring
    // type is present in the IR; function queries exist when the symbol is a
    // declared function (an uncalled function is still a real symbol).
    let symbol_exists = match &resolved.field {
        Some(_) => {
            !field_users.is_empty()
                || resolved.owner_type.as_deref().is_some_and(|owner| {
                    scoped
                        .iter()
                        .any(|(_, module)| impact_type_exists(module, owner))
                })
        }
        None => impact_symbol_exists(&modules, &resolved.call_candidates),
    };

    if !symbol_exists {
        let known = impact_known_symbols(&modules);
        let near_matches = impact_near_matches(&known, &resolved.call_candidates[0]);
        let message = format!("Symbol '{}' was not found in the project IR.", options.symbol);
        emit_impact_json(&ImpactFailureJson {
            schema: IMPACT_SCHEMA,
            success: false,
            symbol: options.symbol.clone(),
            error: message.clone(),
            near_matches,
        })?;
        return Err(CliError::compilation(message));
    }

    let field_semantics = resolved.field.is_some() && !field_users.is_empty();
    let mut affected_types: Vec<String> = Vec::new();
    let mut affected_schemas: Vec<String> = Vec::new();
    if field_semantics {
        if let Some(owner) = &resolved.owner_type {
            affected_types.push(owner.clone());
            affected_schemas.push(owner.clone());
        }
    } else if resolved.owner_type.is_some() {
        if let Some(owner) = &resolved.owner_type {
            affected_types.push(owner.clone());
        }
    }

    let functions = if field_semantics {
        field_users
    } else {
        affected_functions
    };
    let mut affected_functions = functions;
    affected_functions.sort();
    affected_functions.dedup();
    affected_types.sort();
    affected_types.dedup();
    affected_schemas.sort();
    affected_schemas.dedup();
    unresolved.sort();
    unresolved.dedup();

    let modules_scanned: Vec<String> = scoped.iter().map(|(name, _)| name.clone()).collect();
    let dynamic_dispatch = !unresolved.is_empty();

    let report = ImpactReportJson {
        schema: IMPACT_SCHEMA,
        success: true,
        symbol: options.symbol.clone(),
        affected_functions,
        affected_types,
        affected_schemas,
        dynamic_dispatch,
        unresolved,
        fixtures: impact_fixtures(&options.root, &resolved.call_candidates[0]),
        eval_cases: impact_eval_cases(&options.root, &resolved.call_candidates[0]),
        modules_scanned,
    };

    if options.json {
        return emit_impact_json(&report);
    }
    Ok(())
}
