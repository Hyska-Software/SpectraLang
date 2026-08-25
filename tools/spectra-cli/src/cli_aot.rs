fn find_main_location(plan: &ProjectPlan) -> Option<(PathBuf, usize, usize)> {
    for module in plan.modules() {
        let source = fs::read_to_string(&module.path).ok()?;
        if let Some((line_index, column_index, _line_text)) = find_main_location_in_source(&source)
        {
            return Some((module.path.clone(), line_index + 1, column_index + 1));
        }
    }
    None
}

#[derive(Clone, Copy)]
enum AotArtifactKind {
    Object,
    Executable,
}

impl AotArtifactKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Object => "object",
            Self::Executable => "executable",
        }
    }
}

#[derive(Serialize)]
struct AotDebugMap<'a> {
    schema: &'a str,
    schema_version: u32,
    artifact: AotDebugArtifact,
    source: AotDebugSource,
    entrypoint: Option<AotDebugEntrypoint>,
    native_debuggers: &'a [&'a str],
    strategy: &'a str,
    native_format: &'a str,
    native_artifact: Option<String>,
}

#[derive(Serialize)]
struct AotDebugArtifact {
    kind: String,
    path: String,
}

#[derive(Serialize)]
struct AotDebugSource {
    path: String,
}

#[derive(Serialize)]
struct AotDebugEntrypoint {
    function: String,
    exported_symbol: String,
    source_line: usize,
    source_column: usize,
    source_text: String,
}

fn write_aot_debug_map(
    source_path: &Path,
    artifact_path: &Path,
    source: &str,
    artifact_kind: AotArtifactKind,
    native_debuggers: &'static [&'static str],
    native_debug: bool,
) -> CliResult<PathBuf> {
    let entrypoint =
        if let Some((line_index, column_index, line_text)) = find_main_location_in_source(source) {
            Some(AotDebugEntrypoint {
                function: "main".to_string(),
                exported_symbol: match artifact_kind {
                    AotArtifactKind::Object => "main",
                    AotArtifactKind::Executable => "spectra_user_main",
                }
                .to_string(),
                source_line: line_index + 1,
                source_column: column_index + 1,
                source_text: line_text.trim().to_string(),
            })
        } else {
            if matches!(artifact_kind, AotArtifactKind::Executable) {
                return Err(CliError::compilation(
                "cannot emit executable AOT debug map because no 'func main' entry point was found",
            ));
            }
            None
        };

    let debug_map_path = debug_map_path_for_artifact(artifact_path);
    let map = AotDebugMap {
        schema: "spectra-aot-debug-map",
        schema_version: AOT_DEBUG_MAP_SCHEMA_VERSION,
        artifact: AotDebugArtifact {
            kind: artifact_kind.as_str().to_string(),
            path: display_path(artifact_path),
        },
        source: AotDebugSource {
            path: display_path(source_path),
        },
        entrypoint,
        native_debuggers,
        strategy: if native_debug {
            "Native debug records are embedded in the object; executable links request a real PDB/DWARF artifact. This sidecar supplements, but does not replace, native debug information."
        } else {
            "Native debug emission was explicitly disabled; this sidecar contains source metadata only."
        },
        native_format: if native_debug {
            if cfg!(windows) { "codeview/pdb" } else { "dwarf" }
        } else {
            "none"
        },
        native_artifact: if native_debug && matches!(artifact_kind, AotArtifactKind::Executable) {
            Some(display_path(&artifact_path.with_extension("pdb")))
        } else {
            None
        },
    };
    let text = serde_json::to_string_pretty(&map)
        .map_err(|e| CliError::io(format!("Cannot serialize AOT debug map: {}", e)))?;
    fs::write(&debug_map_path, format!("{}\n", text)).map_err(|e| {
        CliError::io(format!(
            "Cannot write AOT debug map '{}': {}",
            debug_map_path.display(),
            e
        ))
    })?;
    Ok(debug_map_path)
}

fn debug_map_path_for_artifact(artifact_path: &Path) -> PathBuf {
    let mut debug_map_path = artifact_path.as_os_str().to_os_string();
    debug_map_path.push(".spectra-debug.json");
    PathBuf::from(debug_map_path)
}

/// Add compiler-owned CodeView records to a COFF object.  The backend owns
/// both the records and the COFF container rewrite; the sidecar is not used
/// to synthesize native debug information.
fn attach_native_codeview(
    object_path: &Path,
    source_path: &Path,
    debug_metadata: &NativeDebugMetadata,
) -> CliResult<()> {
    let functions = debug_metadata.functions.iter().map(|function| function.name.clone()).collect::<Vec<_>>();
    let object_bytes = fs::read(object_path)
        .map_err(|e| CliError::io(format!("Cannot read object for debug ranges: {}", e)))?;
    let mut ranged_functions = spectra_backend::debug::coff_function_ranges(&object_bytes);
    // Only symbols present in the object are eligible for native debug
    // records.  This prevents a sidecar/source name from becoming a fake PDB
    // procedure.  The wrapper is present only for executable objects.
    ranged_functions.retain(|function| functions.iter().any(|name| name == &function.name));
    if ranged_functions.is_empty() {
        return Err(CliError::compilation(
            "--debug-info=native found no user function symbols in the COFF object",
        ));
    }
    for function in &mut ranged_functions {
        function.locals = debug_metadata.functions.iter()
            .find(|metadata| metadata.name == function.name)
            .map(|metadata| metadata.locals.clone())
            .unwrap_or_default();
        function.local_offsets = debug_metadata.functions.iter()
            .find(|metadata| metadata.name == function.name)
            .map(|metadata| metadata.local_offsets.clone())
            .unwrap_or_default();
        function.local_locations = debug_metadata.functions.iter()
            .find(|metadata| metadata.name == function.name)
            .map(|metadata| metadata.local_locations.clone())
            .unwrap_or_default();
        function.line_rows = debug_metadata.functions.iter()
            .find(|metadata| metadata.name == function.name)
            .map(|metadata| metadata.line_rows.clone())
            .unwrap_or_default();
    }
    let records = spectra_backend::debug::codeview_section_with_ranges(
        &source_path.to_string_lossy(),
        &ranged_functions,
        &fs::read_to_string(source_path).map_err(|e| CliError::io(format!("Cannot read source for debug lines: {e}")))?,
    );
    let rewritten = spectra_backend::debug::append_coff_section(
        &object_bytes,
        ".debug$S",
        &records,
        0x4230_0040,
    )
    .map_err(CliError::compilation)?;
    fs::write(object_path, rewritten)
        .map_err(|e| CliError::io(format!("Cannot write CodeView-enabled object: {}", e)))
}

fn attach_native_debug(object_path: &Path, source_path: &Path, debug_metadata: &NativeDebugMetadata) -> CliResult<()> {
    if cfg!(windows) {
        attach_native_codeview(object_path, source_path, debug_metadata)
    } else {
        attach_native_dwarf(object_path, source_path, debug_metadata)
    }
}

fn attach_native_dwarf(object_path: &Path, source_path: &Path, debug_metadata: &NativeDebugMetadata) -> CliResult<()> {
    let object_bytes = fs::read(object_path)
        .map_err(|e| CliError::io(format!("Cannot read object for DWARF attachment: {e}")))?;
    let source_functions = debug_metadata.functions.iter().map(|function| function.name.clone()).collect::<Vec<_>>();
    let mut functions = spectra_backend::debug::native_function_ranges(&object_bytes);
    functions.retain(|function| source_functions.iter().any(|name| name == &function.name));
    if functions.is_empty() {
        return Err(CliError::compilation(
            "--debug-info=native found no user function symbols in the Unix object",
        ));
    }
    for function in &mut functions {
        function.locals = debug_metadata.functions.iter()
            .find(|metadata| metadata.name == function.name)
            .map(|metadata| metadata.locals.clone())
            .unwrap_or_default();
        function.local_offsets = debug_metadata.functions.iter()
            .find(|metadata| metadata.name == function.name)
            .map(|metadata| metadata.local_offsets.clone())
            .unwrap_or_default();
        function.local_locations = debug_metadata.functions.iter()
            .find(|metadata| metadata.name == function.name)
            .map(|metadata| metadata.local_locations.clone())
            .unwrap_or_default();
        function.line_rows = debug_metadata.functions.iter()
            .find(|metadata| metadata.name == function.name)
            .map(|metadata| metadata.line_rows.clone())
            .unwrap_or_default();
    }
    let source = fs::read_to_string(source_path)
        .map_err(|e| CliError::io(format!("Cannot read source for DWARF lines: {e}")))?;
    let sections = spectra_backend::dwarf::sections_for_functions(
        &source_path.to_string_lossy(),
        &source,
        &functions,
    )
    .map_err(CliError::compilation)?;
    let objcopy = find_tool_on_path("llvm-objcopy").ok_or_else(|| {
        CliError::compilation(
            "--debug-info=native on Unix requires llvm-objcopy as a COFF/ELF container editor",
        )
    })?;
    let mut input = object_path.to_path_buf();
    for (index, (name, data)) in sections.iter().enumerate() {
        let section_path = object_path.with_extension(format!("spectra-dwarf-{index}"));
        let output_path = object_path.with_extension(format!("spectra-dwarf-out-{index}"));
        fs::write(&section_path, data)
            .map_err(|e| CliError::io(format!("Cannot write DWARF section {name}: {e}")))?;
        let status = std::process::Command::new(&objcopy)
            .args(["--add-section", &format!("{name}={}", section_path.display())])
            .arg(&input)
            .arg(&output_path)
            .status()
            .map_err(|e| CliError::compilation(format!("Cannot execute llvm-objcopy: {e}")))?;
        let _ = fs::remove_file(&section_path);
        if !status.success() {
            let _ = fs::remove_file(&output_path);
            return Err(CliError::compilation(format!("llvm-objcopy failed to attach {name}")));
        }
        if input != object_path {
            let _ = fs::remove_file(&input);
        }
        input = output_path;
    }
    fs::rename(&input, object_path).map_err(|e| {
        let _ = fs::remove_file(&input);
        CliError::io(format!("Cannot replace object with DWARF-enabled object: {e}"))
    })
}

fn find_tool_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for directory in env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() { return Some(candidate); }
        let candidate = directory.join(format!("{name}.exe"));
        if candidate.is_file() { return Some(candidate); }
    }
    None
}

fn find_main_location_in_source(source: &str) -> Option<(usize, usize, &str)> {
    for (line_index, line) in source.lines().enumerate() {
        let Some(column_index) = line.find("func main") else {
            continue;
        };
        return Some((line_index, column_index, line));
    }
    None
}

fn display_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

fn print_verbose_configuration(kind: BuildCommand, options: &CompilationOptions) {
    println!("  - Command: {}", kind.name());
    println!(
        "  - Optimization level: O{} ({})",
        options.opt_level,
        if options.optimize {
            "optimizations on"
        } else {
            "optimizations off"
        }
    );
    println!(
        "  - Dump AST: {}",
        if options.dump_ast { "yes" } else { "no" }
    );
    println!(
        "  - Dump IR: {}",
        if options.dump_ir { "yes" } else { "no" }
    );
    println!(
        "  - Collect metrics: {}",
        if options.collect_metrics { "yes" } else { "no" }
    );
    println!(
        "  - Run JIT after build: {}",
        if options.run_jit { "yes" } else { "no" }
    );

    if options.lint.enabled.is_empty() {
        println!("  - Linting: disabled");
    } else {
        let mut denied: Vec<_> = options.lint.deny.iter().map(|rule| rule.code()).collect();
        denied.sort();
        let denied_display = if denied.is_empty() {
            "none".to_string()
        } else {
            denied.join(", ")
        };
        println!("  - Linting: enabled (denied rules: {})", denied_display);
    }

    let mut features: Vec<_> = options.experimental_features.iter().collect();
    features.sort();
    if features.is_empty() {
        println!("  - Experimental features: (none)");
    } else {
        println!(
            "  - Experimental features: {}",
            features
                .into_iter()
                .map(|feature| feature.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

