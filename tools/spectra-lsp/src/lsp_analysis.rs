/// Standalone analysis function — used both by `analyze_and_store` and the
/// debounce task spawned in `did_change`.  Takes cloneable handles so it can
/// be called from inside `tokio::spawn`.
async fn do_analyze_and_store(
    client: &Client,
    state: &BackendState,
    uri: Url,
    text: String,
    include_lints: bool,
) {
    let filename = uri
        .to_file_path()
        .ok()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| uri.to_string());

    let options = CompilationOptions {
        optimize: false,
        lint: if include_lints {
            spectra_compiler::LintOptions::all()
        } else {
            spectra_compiler::LintOptions::disabled()
        },
        ..CompilationOptions::default()
    };

    let analysis = analyze_document(&text, &filename, &options, None);
    let diagnostics = analysis_to_diagnostics(&uri, &analysis);
    client
        .publish_diagnostics(uri.clone(), diagnostics, None)
        .await;

    // Update workspace symbol cache.
    let symbols = analysis
        .module
        .as_ref()
        .map(|module| workspace_symbol_entries_for_module(&text, module))
        .unwrap_or_default();
    let references = reference_entries_for_analysis(&uri, &analysis);
    state.workspace_cache.write().await.insert(
        uri.clone(),
        WorkspaceCacheEntry {
            symbols,
            references,
            modified: cache_modified_time_for_uri(&uri),
        },
    );

    state
        .documents
        .write()
        .await
        .insert(uri, DocumentState { text, analysis });
}

fn analysis_to_diagnostics(uri: &Url, analysis: &DocumentAnalysis) -> Vec<Diagnostic> {
    let mut diagnostics: Vec<Diagnostic> = analysis
        .diagnostics
        .iter()
        .map(|error| compiler_error_to_diagnostic(uri, error))
        .collect();
    diagnostics.extend(
        analysis
            .warnings
            .iter()
            .map(|warning| lint_to_diagnostic(uri, warning)),
    );
    diagnostics
}

fn compiler_error_to_diagnostic(uri: &Url, error: &CompilerError) -> Diagnostic {
    match error {
        CompilerError::Lexical(error) => {
            let mut d = span_diagnostic(
                uri,
                error.span,
                &error.message,
                DiagnosticSeverity::ERROR,
                Some("lexical".to_string()),
                error.context.as_deref(),
                error.hint.as_deref(),
            );
            if let Some(code) = &error.code {
                d.code = Some(NumberOrString::String(code.clone()));
            }
            d
        }
        CompilerError::Parse(error) => {
            let mut d = span_diagnostic(
                uri,
                error.span,
                &error.message,
                DiagnosticSeverity::ERROR,
                Some("parse".to_string()),
                error.context.as_deref(),
                error.hint.as_deref(),
            );
            if let Some(code) = &error.code {
                d.code = Some(NumberOrString::String(code.clone()));
            }
            d
        }
        CompilerError::Semantic(error) => {
            let mut d = span_diagnostic(
                uri,
                error.span,
                &error.message,
                DiagnosticSeverity::ERROR,
                Some("semantic".to_string()),
                error.context.as_deref(),
                error.hint.as_deref(),
            );
            if let Some(code) = &error.code {
                d.code = Some(NumberOrString::String(code.clone()));
            }
            d
        }
        CompilerError::Midend(error) => Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("spectra/midend".to_string()),
            message: error.message.clone(),
            ..Default::default()
        },
        CompilerError::Backend(error) => Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("spectra/backend".to_string()),
            message: error.message.clone(),
            ..Default::default()
        },
    }
}

fn lint_to_diagnostic(uri: &Url, diagnostic: &LintDiagnostic) -> Diagnostic {
    let mut result = Diagnostic {
        range: span_to_range(diagnostic.span),
        severity: Some(DiagnosticSeverity::WARNING),
        source: Some("spectra/lint".to_string()),
        code: Some(NumberOrString::String(format!(
            "lint({})",
            diagnostic.rule.code()
        ))),
        message: diagnostic.message.clone(),
        ..Default::default()
    };

    if diagnostic.note.is_some() || diagnostic.secondary_span.is_some() {
        let mut related_information = Vec::new();
        if let Some(note) = &diagnostic.note {
            related_information.push(DiagnosticRelatedInformation {
                location: Location {
                    uri: uri.clone(),
                    range: span_to_range(diagnostic.span),
                },
                message: note.clone(),
            });
        }
        if let Some(secondary_span) = diagnostic.secondary_span {
            related_information.push(DiagnosticRelatedInformation {
                location: Location {
                    uri: uri.clone(),
                    range: span_to_range(secondary_span),
                },
                message: "related location".to_string(),
            });
        }
        result.related_information = Some(related_information);
    }

    result
}

fn span_diagnostic(
    uri: &Url,
    span: Span,
    message: &str,
    severity: DiagnosticSeverity,
    phase: Option<String>,
    context: Option<&str>,
    hint: Option<&str>,
) -> Diagnostic {
    let mut related_information = Vec::new();
    if let Some(context) = context {
        related_information.push(DiagnosticRelatedInformation {
            location: Location {
                uri: uri.clone(),
                range: span_to_range(span),
            },
            message: context.to_string(),
        });
    }
    if let Some(hint) = hint {
        related_information.push(DiagnosticRelatedInformation {
            location: Location {
                uri: uri.clone(),
                range: span_to_range(span),
            },
            message: hint.to_string(),
        });
    }

    Diagnostic {
        range: span_to_range(span),
        severity: Some(severity),
        source: Some(
            phase
                .map(|phase| format!("spectra/{}", phase))
                .unwrap_or_else(|| "spectra".to_string()),
        ),
        message: message.to_string(),
        related_information: if related_information.is_empty() {
            None
        } else {
            Some(related_information)
        },
        ..Default::default()
    }
}

fn span_to_range(span: Span) -> Range {
    Range::new(
        Position::new(
            span.start_location.line.saturating_sub(1) as u32,
            span.start_location.column.saturating_sub(1) as u32,
        ),
        Position::new(
            span.end_location.line.saturating_sub(1) as u32,
            span.end_location.column.saturating_sub(1) as u32,
        ),
    )
}

fn full_document_range(text: &str) -> Range {
    let mut line = 0u32;
    let mut col = 0u32;
    for ch in text.chars() {
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    Range::new(Position::new(0, 0), Position::new(line, col))
}

fn collect_spectra_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if matches!(name, ".git" | "node_modules" | "target" | ".vscode") {
                continue;
            }
            collect_spectra_files(&path, files);
        } else if path.extension().and_then(|value| value.to_str()) == Some("spectra") {
            files.push(path);
        }
    }
}

