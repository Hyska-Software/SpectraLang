fn parse_formatter_config(path: &Path) -> CliResult<FormatterConfig> {
    let manifest = fs::read_to_string(path).map_err(|error| {
        CliError::io(format!(
            "Failed to read configuration '{}': {}",
            path.display(),
            error
        ))
    })?;

    let value: Value = manifest.parse().map_err(|error| {
        CliError::usage(format!("Failed to parse '{}': {}", path.display(), error))
    })?;

    let formatter = match value.get("formatter") {
        None => return Ok(FormatterConfig::default()),
        Some(Value::Table(table)) => table,
        Some(_) => {
            return Err(CliError::usage(format!(
                "Section [formatter] in '{}' must be a table.",
                path.display()
            )))
        }
    };

    let mut config = FormatterConfig::default();

    for (key, value) in formatter {
        match key.as_str() {
            "indent_width" => {
                config.indent_width =
                    parse_positive_usize(value, "indent_width", path, 1, MAX_SUPPORTED_INDENT)?;
            }
            "max_line_length" => {
                config.max_line_length = parse_positive_usize(
                    value,
                    "max_line_length",
                    path,
                    MIN_LINE_LENGTH,
                    usize::MAX,
                )?;
            }
            other => {
                return Err(CliError::usage(format!(
                    "Unknown formatter option '{}' in '{}'.",
                    other,
                    path.display()
                )));
            }
        }
    }

    Ok(config)
}

fn parse_positive_usize(
    value: &Value,
    key: &str,
    path: &Path,
    min: usize,
    max: usize,
) -> CliResult<usize> {
    if let Some(raw) = value.as_integer() {
        if raw < min as i64 || raw > max as i64 {
            return Err(CliError::usage(format!(
                "Value '{}' in '{}' must be between {} and {}.",
                key,
                path.display(),
                min,
                max
            )));
        }
        Ok(raw as usize)
    } else {
        Err(CliError::usage(format!(
            "Value '{}' in '{}' must be an integer.",
            key,
            path.display()
        )))
    }
}

fn format_preserving_endings(original: &str, config: &FormatterConfig) -> String {
    let normalized_input = if original.contains("\r\n") {
        original.replace("\r\n", "\n")
    } else {
        original.to_string()
    };

    let formatted = format_source(&normalized_input, config);

    if original.contains("\r\n") {
        formatted.replace('\n', "\r\n")
    } else {
        formatted
    }
}

fn discover_sources(entries: &[PathBuf]) -> CliResult<Vec<PathBuf>> {
    let mut seen = HashSet::new();
    let mut files = Vec::new();

    for entry in entries {
        let canonical = fs::canonicalize(entry).map_err(|error| {
            CliError::io(format!(
                "Failed to resolve path '{}': {}",
                entry.display(),
                error
            ))
        })?;
        let metadata = fs::symlink_metadata(&canonical).map_err(|error| {
            CliError::io(format!(
                "Failed to inspect '{}': {}",
                canonical.display(),
                error
            ))
        })?;

        if metadata.is_file() {
            if is_source_file(&canonical) {
                files.push(canonical);
            } else {
                return Err(CliError::usage(format!(
                    "Path '{}' is not a Spectra source file (expected .spectra or .spc).",
                    canonical.display()
                )));
            }
            continue;
        }

        if metadata.is_dir() {
            visit_path(&canonical, &mut seen, &mut files)?;
        }
    }

    files.sort();
    files.dedup();
    Ok(files)
}

fn visit_path(path: &Path, seen: &mut HashSet<PathBuf>, out: &mut Vec<PathBuf>) -> CliResult<()> {
    let canonical = fs::canonicalize(path).map_err(|error| {
        CliError::io(format!(
            "Failed to resolve path '{}' during traversal: {}",
            path.display(),
            error
        ))
    })?;

    if !seen.insert(canonical.clone()) {
        return Ok(());
    }

    let metadata = fs::symlink_metadata(&canonical).map_err(|error| {
        CliError::io(format!(
            "Failed to inspect '{}': {}",
            canonical.display(),
            error
        ))
    })?;

    if metadata.is_dir() {
        if should_skip_directory(&canonical) {
            return Ok(());
        }

        let read_dir = fs::read_dir(&canonical).map_err(|error| {
            CliError::io(format!(
                "Failed to enumerate directory '{}': {}",
                canonical.display(),
                error
            ))
        })?;

        for entry in read_dir {
            let entry = entry.map_err(|error| {
                CliError::io(format!(
                    "Failed to enumerate directory '{}': {}",
                    canonical.display(),
                    error
                ))
            })?;
            visit_path(&entry.path(), seen, out)?;
        }
        return Ok(());
    }

    if metadata.is_file() && is_source_file(&canonical) {
        out.push(canonical);
    }

    Ok(())
}

fn is_source_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("spectra") || ext.eq_ignore_ascii_case("spc"))
        .unwrap_or(false)
}

fn should_skip_directory(path: &Path) -> bool {
    match path.file_name().and_then(|value| value.to_str()) {
        Some(name) if name.starts_with('.') => true,
        Some("target" | "build" | "dist" | "out") => true,
        _ => false,
    }
}

fn write_formatted(path: &Path, contents: &str) -> CliResult<()> {
    fs::write(path, contents).map_err(|error| {
        CliError::io(format!(
            "Failed to write formatted output to '{}': {}",
            path.display(),
            error
        ))
    })
}

fn render_diff(path: &Path, original: &str, formatted: &str) -> String {
    let mut buffer = String::new();
    let _ = writeln!(buffer, "diff --spectra {}", path.display());
    let _ = writeln!(buffer, "--- original");
    let _ = writeln!(buffer, "+++ formatted");

    for change in lines(original, formatted) {
        match change {
            DiffResult::Left(line) => {
                let _ = writeln!(buffer, "-{}", line);
            }
            DiffResult::Right(line) => {
                let _ = writeln!(buffer, "+{}", line);
            }
            DiffResult::Both(line, _) => {
                let _ = writeln!(buffer, " {}", line);
            }
        }
    }

    buffer
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct JsonExplainPayload {
    summary: JsonSummary,
    files: Vec<JsonFileDiff>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct JsonSummary {
    processed: usize,
    changed: usize,
    updated: usize,
    unchanged: usize,
    mode: FormatterMode,
    config_cache_lookups: usize,
    config_cache_hits: usize,
    config_cache_misses: usize,
}

impl JsonSummary {
    fn from_stats(stats: &FormatterRunStats) -> Self {
        Self {
            processed: stats.processed,
            changed: stats.changed,
            updated: stats.updated,
            unchanged: stats.unchanged,
            mode: stats.mode,
            config_cache_lookups: stats.config_cache_lookups,
            config_cache_hits: stats.config_cache_hits,
            config_cache_misses: stats.config_cache_misses,
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct JsonFileDiff {
    path: String,
    operations: Vec<JsonDiffOp>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct JsonDiffOp {
    op: JsonOpKind,
    text: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum JsonOpKind {
    Equal,
    Insert,
    Remove,
}

fn render_json_diff(path: &Path, original: &str, formatted: &str) -> JsonFileDiff {
    let mut operations = Vec::new();

    for change in lines(original, formatted) {
        match change {
            DiffResult::Left(line) => operations.push(JsonDiffOp {
                op: JsonOpKind::Remove,
                text: line.to_string(),
            }),
            DiffResult::Right(line) => operations.push(JsonDiffOp {
                op: JsonOpKind::Insert,
                text: line.to_string(),
            }),
            DiffResult::Both(line, _) => operations.push(JsonDiffOp {
                op: JsonOpKind::Equal,
                text: line.to_string(),
            }),
        }
    }

    JsonFileDiff {
        path: path.display().to_string(),
        operations,
    }
}

fn maybe_emit_stats(options: &FormatOptions, stats: &FormatterRunStats) {
    if options.stats {
        emit_stats(stats);
    }
}

fn emit_stats(stats: &FormatterRunStats) {
    match serde_json::to_string_pretty(stats) {
        Ok(serialized) => println!("{}", serialized),
        Err(error) => eprintln!("Failed to serialize formatter stats: {}", error),
    }
}
