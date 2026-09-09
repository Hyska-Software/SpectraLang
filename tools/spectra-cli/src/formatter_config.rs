use crate::{CliError, CliResult};
use diff::{lines, Result as DiffResult};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use toml::Value;

const DEFAULT_INDENT_WIDTH: usize = 4;
const DEFAULT_MAX_LINE_LENGTH: usize = 100;
const MAX_SUPPORTED_INDENT: usize = 12;
const MIN_LINE_LENGTH: usize = 40;

#[derive(Debug, Clone)]
pub(crate) struct FormatterConfig {
    indent_width: usize,
    max_line_length: usize,
}

impl Default for FormatterConfig {
    fn default() -> Self {
        Self {
            indent_width: DEFAULT_INDENT_WIDTH,
            max_line_length: DEFAULT_MAX_LINE_LENGTH,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExplainMode {
    None,
    Text,
    Json,
}

#[derive(Debug)]
pub(crate) struct FormatOptions {
    pub entries: Vec<PathBuf>,
    pub check: bool,
    pub use_stdin: bool,
    pub write_stdout: bool,
    pub explain: ExplainMode,
    pub stats: bool,
    pub config_path: Option<PathBuf>,
}

struct FormatterConfigResolver {
    override_config: Option<FormatterConfig>,
    directory_cache: HashMap<PathBuf, FormatterConfig>,
    manifest_cache: HashMap<PathBuf, Option<PathBuf>>,
    parsed_configs: HashMap<PathBuf, FormatterConfig>,
    default: FormatterConfig,
    stats: ConfigStats,
}

#[derive(Debug, Clone, Default, Serialize)]
struct ConfigStats {
    cache_lookups: usize,
    cache_hits: usize,
    cache_misses: usize,
}

impl ConfigStats {
    fn record_hit(&mut self) {
        self.cache_lookups += 1;
        self.cache_hits += 1;
    }

    fn record_miss(&mut self) {
        self.cache_lookups += 1;
        self.cache_misses += 1;
    }
}

#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
struct FormatterRunStats {
    processed: usize,
    changed: usize,
    updated: usize,
    unchanged: usize,
    mode: FormatterMode,
    config_cache_lookups: usize,
    config_cache_hits: usize,
    config_cache_misses: usize,
}

impl FormatterRunStats {
    fn new(processed: usize, changed: usize, is_check: bool, config_stats: &ConfigStats) -> Self {
        let unchanged = processed.saturating_sub(changed);
        let updated = if is_check { 0 } else { changed };
        Self {
            processed,
            changed,
            updated,
            unchanged,
            mode: if is_check {
                FormatterMode::Check
            } else {
                FormatterMode::Write
            },
            config_cache_lookups: config_stats.cache_lookups,
            config_cache_hits: config_stats.cache_hits,
            config_cache_misses: config_stats.cache_misses,
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum FormatterMode {
    Check,
    Write,
}

impl FormatterConfigResolver {
    fn new(options: &FormatOptions) -> CliResult<Self> {
        if let Some(path) = &options.config_path {
            let canonical = fs::canonicalize(path).map_err(|error| {
                CliError::io(format!(
                    "Failed to resolve configuration path '{}': {}",
                    path.display(),
                    error
                ))
            })?;

            if !canonical.is_file() {
                return Err(CliError::usage(format!(
                    "Configuration override '{}' is not a file.",
                    canonical.display()
                )));
            }

            let config = parse_formatter_config(&canonical)?;

            return Ok(Self {
                override_config: Some(config),
                directory_cache: HashMap::new(),
                manifest_cache: HashMap::new(),
                parsed_configs: HashMap::new(),
                default: FormatterConfig::default(),
                stats: ConfigStats::default(),
            });
        }

        Ok(Self {
            override_config: None,
            directory_cache: HashMap::new(),
            manifest_cache: HashMap::new(),
            parsed_configs: HashMap::new(),
            default: FormatterConfig::default(),
            stats: ConfigStats::default(),
        })
    }

    fn config_for_stdin(&mut self) -> CliResult<FormatterConfig> {
        if let Some(config) = &self.override_config {
            return Ok(config.clone());
        }

        let cwd = env::current_dir().map_err(|error| {
            CliError::io(format!("Failed to determine current directory: {}", error))
        })?;
        let canonical = fs::canonicalize(&cwd).map_err(|error| {
            CliError::io(format!(
                "Failed to resolve current directory '{}': {}",
                cwd.display(),
                error
            ))
        })?;

        self.config_for_directory(&canonical)
    }

    fn config_for_path(&mut self, path: &Path) -> CliResult<FormatterConfig> {
        if let Some(config) = &self.override_config {
            return Ok(config.clone());
        }

        let canonical = fs::canonicalize(path).map_err(|error| {
            CliError::io(format!(
                "Failed to resolve path '{}': {}",
                path.display(),
                error
            ))
        })?;

        let directory = canonical
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        self.config_for_directory(&directory)
    }

    fn config_for_directory(&mut self, directory: &Path) -> CliResult<FormatterConfig> {
        if let Some(config) = self.directory_cache.get(directory) {
            self.stats.record_hit();
            return Ok(config.clone());
        }

        self.stats.record_miss();

        let manifest = self.manifest_for_directory(directory)?;

        let config = if let Some(manifest_path) = manifest {
            if let Some(existing) = self.parsed_configs.get(&manifest_path) {
                existing.clone()
            } else {
                let parsed = parse_formatter_config(&manifest_path)?;
                self.parsed_configs
                    .insert(manifest_path.clone(), parsed.clone());
                parsed
            }
        } else {
            self.default.clone()
        };

        self.directory_cache
            .insert(directory.to_path_buf(), config.clone());

        Ok(config)
    }

    fn stats(&self) -> ConfigStats {
        self.stats.clone()
    }

    fn manifest_for_directory(&mut self, directory: &Path) -> CliResult<Option<PathBuf>> {
        if let Some(cached) = self.manifest_cache.get(directory) {
            return Ok(cached.clone());
        }

        let mut current = Some(directory.to_path_buf());
        let mut visited = Vec::new();
        let mut result = None;

        while let Some(dir) = current {
            if let Some(cached) = self.manifest_cache.get(&dir) {
                result = cached.clone();
                break;
            }

            visited.push(dir.clone());
            let candidate = dir.join("spectra.toml");
            if candidate.is_file() {
                let canonical = fs::canonicalize(&candidate).map_err(|error| {
                    CliError::io(format!(
                        "Failed to resolve configuration '{}': {}",
                        candidate.display(),
                        error
                    ))
                })?;
                result = Some(canonical);
                break;
            }

            current = dir.parent().map(Path::to_path_buf);
        }

        for dir in visited {
            self.manifest_cache.insert(dir, result.clone());
        }

        Ok(result)
    }
}

pub(crate) fn run(options: FormatOptions) -> CliResult<()> {
    if options.explain != ExplainMode::None && options.use_stdin {
        return Err(CliError::usage(
            "--explain cannot be used with --stdin. Provide files or directories instead.",
        ));
    }

    if options.explain != ExplainMode::None && options.write_stdout {
        return Err(CliError::usage(
            "--explain cannot be combined with --stdout. Use regular formatting or --check.",
        ));
    }

    let mut config_resolver = FormatterConfigResolver::new(&options)?;

    if options.use_stdin {
        let config = config_resolver.config_for_stdin()?;
        let mut input = String::new();
        io::stdin()
            .read_to_string(&mut input)
            .map_err(|error| CliError::io(format!("Failed to read standard input: {}", error)))?;

        let output = format_preserving_endings(&input, &config).map_err(|error| {
            CliError::compilation(format!("Failed to format standard input: {}", error.message))
        })?;
        let changed = output != input;
        let run_stats = {
            let config_stats = config_resolver.stats();
            FormatterRunStats::new(1, usize::from(changed), options.check, &config_stats)
        };

        if options.check {
            if changed {
                maybe_emit_stats(&options, &run_stats);
                return Err(CliError::compilation(
                    "Standard input is not properly formatted.\nRun 'spectra fmt --stdin' to rewrite the stream.",
                ));
            }

            maybe_emit_stats(&options, &run_stats);
            return Ok(());
        }

        print!("{}", output);
        maybe_emit_stats(&options, &run_stats);
        return Ok(());
    }

    if options.entries.is_empty() {
        return Err(CliError::usage(
            "No source files or directories were provided for formatting.",
        ));
    }

    let sources = discover_sources(&options.entries)?;
    if sources.is_empty() {
        return Err(CliError::usage(
            "No Spectra source files found in the provided paths.",
        ));
    }

    if options.write_stdout && sources.len() != 1 {
        return Err(CliError::usage(
            "--stdout requires exactly one input file when formatting from disk.",
        ));
    }

    if options.write_stdout {
        let path = &sources[0];
        let config = config_resolver.config_for_path(path)?;
        let original = fs::read_to_string(path).map_err(|error| {
            CliError::io(format!(
                "Failed to read source '{}' for formatting: {}",
                path.display(),
                error
            ))
        })?;

        let output = format_preserving_endings(&original, &config).map_err(|error| {
            CliError::compilation(format!(
                "Failed to format '{}': {}",
                path.display(),
                error.message
            ))
        })?;
        let changed = output != original;
        let run_stats = {
            let config_stats = config_resolver.stats();
            FormatterRunStats::new(1, usize::from(changed), options.check, &config_stats)
        };

        if options.check {
            if changed {
                maybe_emit_stats(&options, &run_stats);
                return Err(CliError::compilation(format!(
                    "File '{}' is not properly formatted.\nRun 'spectra fmt {}' to produce the formatted output.",
                    path.display(),
                    path.display()
                )));
            }
            maybe_emit_stats(&options, &run_stats);
            return Ok(());
        }

        print!("{}", output);
        maybe_emit_stats(&options, &run_stats);
        return Ok(());
    }

    let mut changed = Vec::new();
    let mut text_diffs = if options.explain == ExplainMode::Text {
        Some(Vec::new())
    } else {
        None
    };
    let mut json_diffs = if options.explain == ExplainMode::Json {
        Some(Vec::new())
    } else {
        None
    };
    let mut processed = 0usize;

    for path in &sources {
        processed += 1;
        let config = config_resolver.config_for_path(path)?;
        let original = fs::read_to_string(path).map_err(|error| {
            CliError::io(format!(
                "Failed to read source '{}' for formatting: {}",
                path.display(),
                error
            ))
        })?;

        let output = format_preserving_endings(&original, &config).map_err(|error| {
            CliError::compilation(format!(
                "Failed to format '{}': {}",
                path.display(),
                error.message
            ))
        })?;

        if output != original {
            changed.push(path.clone());
            if let Some(diffs) = text_diffs.as_mut() {
                diffs.push(render_diff(path, &original, &output));
            }
            if let Some(diffs) = json_diffs.as_mut() {
                diffs.push(render_json_diff(path, &original, &output));
            }
            if !options.check {
                write_formatted(path, &output)?;
            }
        }
    }

    let run_stats = {
        let config_stats = config_resolver.stats();
        FormatterRunStats::new(processed, changed.len(), options.check, &config_stats)
    };

    match options.explain {
        ExplainMode::Text => {
            let diffs = text_diffs.unwrap_or_default();
            if changed.is_empty() {
                println!(
                    "No formatting changes detected across {} file{}.",
                    processed,
                    if processed == 1 { "" } else { "s" }
                );
                maybe_emit_stats(&options, &run_stats);
                return Ok(());
            }

            for diff in diffs {
                println!("{}", diff);
            }

            maybe_emit_stats(&options, &run_stats);
            return Err(CliError::compilation(format!(
                "Formatting differences detected in {} file{}.",
                changed.len(),
                if changed.len() == 1 { "" } else { "s" }
            )));
        }
        ExplainMode::Json => {
            let diffs = json_diffs.unwrap_or_default();
            let payload = JsonExplainPayload {
                summary: JsonSummary::from_stats(&run_stats),
                files: diffs,
            };

            let serialized = serde_json::to_string_pretty(&payload).map_err(|error| {
                CliError::io(format!("Failed to serialize explain payload: {}", error))
            })?;

            println!("{}", serialized);

            if changed.is_empty() {
                maybe_emit_stats(&options, &run_stats);
                return Ok(());
            }

            maybe_emit_stats(&options, &run_stats);
            return Err(CliError::compilation(format!(
                "Formatting differences detected in {} file{}.",
                changed.len(),
                if changed.len() == 1 { "" } else { "s" }
            )));
        }
        ExplainMode::None => {}
    }

    if options.check && !changed.is_empty() {
        let mut message =
            String::from("Formatting check failed. The following files need formatting:\n");
        for path in &changed {
            message.push_str("  - ");
            message.push_str(&path.display().to_string());
            message.push('\n');
        }
        message.push_str("Run 'spectra fmt' to apply the required changes.");
        maybe_emit_stats(&options, &run_stats);
        return Err(CliError::compilation(message));
    }

    if options.check {
        println!(
            "Checked {} file{} ({} clean).",
            processed,
            if processed == 1 { "" } else { "s" },
            processed - changed.len()
        );
        maybe_emit_stats(&options, &run_stats);
    } else if changed.is_empty() {
        println!(
            "Formatted {} file{} (no changes needed).",
            processed,
            if processed == 1 { "" } else { "s" }
        );
        maybe_emit_stats(&options, &run_stats);
    } else {
        println!(
            "Formatted {} file{} ({} updated, {} already formatted).",
            processed,
            if processed == 1 { "" } else { "s" },
            changed.len(),
            processed - changed.len()
        );
        maybe_emit_stats(&options, &run_stats);
    }

    Ok(())
}
