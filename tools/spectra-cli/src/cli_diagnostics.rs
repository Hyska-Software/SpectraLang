fn execute_structured_diagnostics(
    entries: Vec<PathBuf>,
    mut options: CompilationOptions,
    sarif_output: bool,
) -> CliResult<()> {
    if entries.is_empty() {
        return Err(CliError::usage(
            "No Spectra source files were provided for linting.",
        ));
    }

    options.run_jit = false;
    run_structured_diagnostics(entries, options, sarif_output)
}

fn execute_repl_json(mut options: CompilationOptions, preload: Vec<PathBuf>) -> CliResult<()> {
    if preload.is_empty() {
        return Err(CliError::usage(
            "Provide one or more paths when using 'spectra repl --json'.",
        ));
    }

    configure_lint_options(&mut options, &preload, true, &[], &[])?;
    options.run_jit = false;
    run_structured_diagnostics(preload, options, false)
}

fn run_structured_diagnostics(
    entries: Vec<PathBuf>,
    options: CompilationOptions,
    sarif_output: bool,
) -> CliResult<()> {
    let plan = match ProjectPlan::build(entries.clone()) {
        Ok(plan) => plan,
        Err(error) => {
            let path = entries.first()
                .cloned()
                .unwrap_or_else(|| PathBuf::from("."));

            let report = JsonDiagnosticReport {
                version: 1,
                success: false,
                files: vec![JsonFileDiagnostics {
                    path: path_to_string(&path),
                    diagnostics: vec![generic_error_diagnostic(format!("{}", error), Some("cli"))],
                }],
            };

            emit_diagnostic_report(&report, true, sarif_output)?;
            return Ok(());
        }
    };

    if plan.modules().is_empty() {
        let report = JsonDiagnosticReport {
            version: 1,
            success: true,
            files: Vec::new(),
        };
        emit_diagnostic_report(&report, false, sarif_output)?;
        return Ok(());
    }

    let mut compiler = SpectraCompiler::new(options);
    compiler.set_emit_internal_metrics(false);
    compiler.set_emit_output(false);

    let mut files: BTreeMap<PathBuf, Vec<JsonDiagnostic>> = BTreeMap::new();
    let mut has_errors = false;

    for module in plan.modules() {
        let path = module.path.clone();
        let display_path = path_to_string(&path);
        let diagnostics = files.entry(path.clone()).or_default();

        let source = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) => {
                diagnostics.push(io_error_diagnostic(&error));
                has_errors = true;
                continue;
            }
        };

        match compiler.compile_for_diagnostics(&source, &display_path) {
            Ok(warnings) => {
                for warning in warnings {
                    diagnostics.push(convert_lint_diagnostic(warning));
                }
            }
            Err(errors) => {
                has_errors = true;
                for error in errors {
                    diagnostics.push(convert_compiler_error(error));
                }
            }
        }
    }

    let files: Vec<JsonFileDiagnostics> = files
        .into_iter()
        .map(|(path, diagnostics)| JsonFileDiagnostics {
            path: path_to_string(&path),
            diagnostics,
        })
        .collect();

    let report = JsonDiagnosticReport {
        version: 1,
        success: !has_errors,
        files,
    };

    emit_diagnostic_report(&report, has_errors, sarif_output)
}

fn emit_diagnostic_report(
    report: &JsonDiagnosticReport,
    has_errors: bool,
    sarif_output: bool,
) -> CliResult<()> {
    if sarif_output {
        emit_sarif_report(report, has_errors)
    } else {
        emit_json_report(report, has_errors)
    }
}

fn emit_json_report(report: &JsonDiagnosticReport, has_errors: bool) -> CliResult<()> {
    let mut stdout = io::stdout();
    serde_json::to_writer(&mut stdout, report).map_err(|error| {
        CliError::io(format!(
            "Failed to serialize diagnostics to JSON: {}",
            error
        ))
    })?;
    stdout
        .write_all(b"\n")
        .map_err(|error| CliError::io(format!("Failed to write diagnostics: {}", error)))?;
    stdout
        .flush()
        .map_err(|error| CliError::io(format!("Failed to flush diagnostics: {}", error)))?;

    if has_errors {
        process::exit(ExitCode::CompilationFailed.as_i32());
    }

    Ok(())
}

fn emit_sarif_report(report: &JsonDiagnosticReport, has_errors: bool) -> CliResult<()> {
    let mut rules: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    let mut results = Vec::new();

    for file in &report.files {
        for diagnostic in &file.diagnostics {
            let rule_id = diagnostic
                .code
                .clone()
                .or_else(|| diagnostic.phase.clone())
                .unwrap_or_else(|| "diagnostic".to_string());
            rules.entry(rule_id.clone()).or_insert_with(|| {
                json!({
                    "id": rule_id,
                    "name": diagnostic.phase.clone().unwrap_or_else(|| "diagnostic".to_string()),
                    "shortDescription": { "text": diagnostic.message },
                    "help": { "text": diagnostic.hint.clone().unwrap_or_else(|| diagnostic.message.clone()) }
                })
            });

            let mut physical_location = json!({
                "artifactLocation": { "uri": file.path }
            });
            // Spanless diagnostics carry no region: pinning file-start
            // coordinates would invent a location the error never had.
            if let Some(range) = &diagnostic.range {
                physical_location["region"] = json!({
                    "startLine": range.start.line,
                    "startColumn": range.start.column,
                    "endLine": range.end.line,
                    "endColumn": range.end.column
                });
            }
            let mut result = json!({
                "ruleId": rule_id,
                "level": diagnostic.severity.sarif_level(),
                "message": { "text": diagnostic.message },
                "locations": [
                    {
                        "physicalLocation": physical_location
                    }
                ]
            });

            if let Some(hint) = &diagnostic.hint {
                result["properties"] = json!({ "hint": hint });
            }

            if !diagnostic.related.is_empty() {
                result["relatedLocations"] = json!(diagnostic
                    .related
                    .iter()
                    .enumerate()
                    .map(|(index, related)| {
                        let mut item = json!({
                            "id": index + 1,
                            "message": { "text": related.message }
                        });
                        if let Some(range) = &related.range {
                            item["physicalLocation"] = json!({
                                "artifactLocation": { "uri": file.path },
                                "region": {
                                    "startLine": range.start.line,
                                    "startColumn": range.start.column,
                                    "endLine": range.end.line,
                                    "endColumn": range.end.column
                                }
                            });
                        }
                        item
                    })
                    .collect::<Vec<_>>());
            }

            results.push(result);
        }
    }

    let report = json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [
            {
                "tool": {
                    "driver": {
                        "name": "SpectraLang",
                        "semanticVersion": env!("CARGO_PKG_VERSION"),
                        "informationUri": "https://github.com/spectralang/spectralang",
                        "rules": rules.into_values().collect::<Vec<_>>()
                    }
                },
                "results": results
            }
        ]
    });

    let mut stdout = io::stdout();
    serde_json::to_writer_pretty(&mut stdout, &report).map_err(|error| {
        CliError::io(format!(
            "Failed to serialize diagnostics to SARIF: {}",
            error
        ))
    })?;
    stdout
        .write_all(b"\n")
        .map_err(|error| CliError::io(format!("Failed to write diagnostics: {}", error)))?;
    stdout
        .flush()
        .map_err(|error| CliError::io(format!("Failed to flush diagnostics: {}", error)))?;

    if has_errors {
        process::exit(ExitCode::CompilationFailed.as_i32());
    }

    Ok(())
}

fn convert_lint_diagnostic(diagnostic: LintDiagnostic) -> JsonDiagnostic {
    let LintDiagnostic {
        rule,
        message,
        span,
        note,
        secondary_span,
    } = diagnostic;

    let mut related = Vec::new();
    if let Some(secondary) = secondary_span {
        related.push(JsonRelated {
            message: "related location".to_string(),
            range: Some(span_to_range(&secondary)),
        });
    }

    JsonDiagnostic {
        severity: JsonSeverity::Warning,
        code: Some(format!("lint({})", rule.code())),
        message,
        phase: Some("lint".to_string()),
        hint: note,
        range: Some(span_to_range(&span)),
        related,
    }
}

fn convert_compiler_error(error: CompilerError) -> JsonDiagnostic {
    match error {
        CompilerError::Lexical(e) => {
            span_error_to_json("lexical", e.code, e.message, e.span, e.context, e.hint)
        }
        CompilerError::Parse(e) => {
            span_error_to_json("parse", e.code, e.message, e.span, e.context, e.hint)
        }
        CompilerError::Semantic(e) => {
            span_error_to_json("semantic", e.code, e.message, e.span, e.context, e.hint)
        }
        CompilerError::Midend(e) => {
            generic_error_diagnostic(format!("midend error: {}", e.message), Some("midend"))
        }
        CompilerError::Backend(e) => {
            generic_error_diagnostic(format!("backend error: {}", e.message), Some("backend"))
        }
    }
}

fn span_error_to_json(
    phase: &'static str,
    code: Option<String>,
    message: String,
    span: Span,
    context: Option<String>,
    hint: Option<String>,
) -> JsonDiagnostic {
    let mut related = Vec::new();
    if let Some(context) = context {
        related.push(JsonRelated {
            message: context,
            range: None,
        });
    }

    JsonDiagnostic {
        severity: JsonSeverity::Error,
        code: Some(code.unwrap_or_else(|| phase.to_string())),
        message,
        phase: Some(phase.to_string()),
        hint,
        range: Some(span_to_range(&span)),
        related,
    }
}

fn io_error_diagnostic(error: &io::Error) -> JsonDiagnostic {
    generic_error_diagnostic(format!("I/O error: {}", error), Some("io"))
}

fn generic_error_diagnostic(message: String, phase: Option<&str>) -> JsonDiagnostic {
    // Spanless errors (midend, backend, I/O, CLI) carry no location: the
    // range stays null instead of pinning a fabricated 1:1 file-start
    // position the way `default_range` used to.
    JsonDiagnostic {
        severity: JsonSeverity::Error,
        code: phase.map(|value| value.to_string()),
        message,
        phase: phase.map(|value| value.to_string()),
        hint: None,
        range: None,
        related: Vec::new(),
    }
}

fn span_to_range(span: &Span) -> JsonRange {
    JsonRange {
        start: JsonPosition {
            line: span.start_location.line,
            column: span.start_location.column,
        },
        end: JsonPosition {
            line: span.end_location.line,
            column: span.end_location.column,
        },
    }
}

fn path_to_string(path: &Path) -> String {
    fs::canonicalize(path)
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

#[derive(Serialize)]
struct JsonDiagnosticReport {
    version: u8,
    success: bool,
    files: Vec<JsonFileDiagnostics>,
}

#[derive(Serialize)]
struct JsonFileDiagnostics {
    path: String,
    diagnostics: Vec<JsonDiagnostic>,
}

#[derive(Serialize)]
struct JsonDiagnostic {
    severity: JsonSeverity,
    code: Option<String>,
    message: String,
    phase: Option<String>,
    hint: Option<String>,
    range: Option<JsonRange>,
    related: Vec<JsonRelated>,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum JsonSeverity {
    Error,
    Warning,
}

impl JsonSeverity {
    fn sarif_level(&self) -> &'static str {
        match self {
            JsonSeverity::Error => "error",
            JsonSeverity::Warning => "warning",
        }
    }
}

#[derive(Serialize)]
struct JsonRange {
    start: JsonPosition,
    end: JsonPosition,
}

#[derive(Serialize)]
struct JsonPosition {
    line: usize,
    column: usize,
}

#[derive(Serialize)]
struct JsonRelated {
    message: String,
    range: Option<JsonRange>,
}

fn derive_project_identifiers(path: &Path) -> (String, String) {
    let raw_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("spectra_app");

    let project_name = sanitize_project_name(raw_name);
    let module_name = sanitize_module_name(&project_name);

    (project_name, module_name)
}

fn sanitize_project_name(raw: &str) -> String {
    let mut result = String::new();

    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '_' | '-' | ' ')
            && !result.ends_with('_') && !result.is_empty() {
                result.push('_');
            }
    }

    let trimmed = result.trim_matches('_');
    if trimmed.is_empty() {
        "spectra_app".to_string()
    } else {
        trimmed.to_string()
    }
}

fn sanitize_module_name(project_name: &str) -> String {
    let mut result = String::new();

    for ch in project_name.chars() {
        if result.is_empty() {
            if ch.is_ascii_alphabetic() {
                result.push(ch);
            } else if ch.is_ascii_digit() {
                result.push('m');
                result.push(ch);
            }
        } else if ch.is_ascii_alphanumeric() || ch == '_' {
            result.push(ch);
        }
    }

    if result.is_empty() {
        "app".to_string()
    } else {
        result
    }
}

fn is_directory_empty(path: &Path) -> Result<bool, io::Error> {
    let mut entries = fs::read_dir(path)?;
    Ok(entries.next().transpose()?.is_none())
}


#[cfg(test)]
mod diagnostics_tests {
    use super::*;

    #[test]
    fn spanless_errors_render_without_location() {
        let diagnostic = generic_error_diagnostic("midend error: boom".to_string(), Some("midend"));
        let json = serde_json::to_value(&diagnostic).expect("diagnostic serializes");
        assert_eq!(json.get("range"), Some(&serde_json::Value::Null));
        let text = serde_json::to_string(&json).expect("diagnostic renders");
        assert!(
            !text.contains("\"line\":1") && !text.contains("1:1"),
            "spanless error must not pin file-start coordinates, got: {text}"
        );
    }

    #[test]
    fn spanned_errors_keep_their_range() {
        let diagnostic = span_error_to_json(
            "syntax",
            None,
            "boom".to_string(),
            spectra_compiler::span::Span::new(
                0,
                1,
                spectra_compiler::span::Location::new(3, 7),
                spectra_compiler::span::Location::new(3, 12),
            ),
            None,
            None,
        );
        let json = serde_json::to_value(&diagnostic).expect("diagnostic serializes");
        let range = json.get("range").expect("spanned error keeps a range");
        assert_eq!(range.get("start").and_then(|start| start.get("line")), Some(&serde_json::json!(3)));
    }
}
