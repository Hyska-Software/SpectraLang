fn execute_repl(options: ReplOptions) -> CliResult<()> {
    let ReplOptions {
        base_options,
        preload,
        autorun,
        show_pipeline_summary,
        verbose,
        json_output,
    } = options;

    if json_output {
        return execute_repl_json(base_options, preload);
    }

    let mut session = ReplSession::new(base_options, autorun, show_pipeline_summary, verbose);

    if !preload.is_empty() {
        if let Err(error) = session.compile_entries(preload, session.default_command(), true) {
            log_error(&error.message);
        }
    }

    session.run()
}

/// Interactive session shell over [`ReplBuffer`]. See repl_session.rs for the
/// persistence model: declarations accumulate and are validated as a whole
/// before commit; expressions are evaluated through a temporary entry point.
struct ReplSession {
    base_options: CompilationOptions,
    autorun: bool,
    show_pipeline_summary: bool,
    verbose: bool,
    buffer: ReplBuffer,
    scratch_path: PathBuf,
}

impl ReplSession {
    fn new(
        base_options: CompilationOptions,
        autorun: bool,
        show_pipeline_summary: bool,
        verbose: bool,
    ) -> Self {
        Self {
            base_options,
            autorun,
            show_pipeline_summary,
            verbose,
            buffer: ReplBuffer::new(),
            scratch_path: env::temp_dir().join(format!(
                "spectra-repl-session-{}.spectra",
                process::id()
            )),
        }
    }

    fn default_command(&self) -> BuildCommand {
        if self.autorun {
            BuildCommand::Run
        } else {
            BuildCommand::Compile
        }
    }

    fn compile_entries(
        &self,
        entries: Vec<PathBuf>,
        command: BuildCommand,
        print_success: bool,
    ) -> CliResult<()> {
        if entries.is_empty() {
            return Err(CliError::usage("Provide one or more paths to compile."));
        }

        let mut options = self.base_options.clone();
        match command {
            BuildCommand::Run => options.run_jit = true,
            BuildCommand::Check
            | BuildCommand::Lint
            | BuildCommand::Compile
            | BuildCommand::Bench => options.run_jit = false,
        }

        execute_plan_with_options(
            command,
            options,
            entries,
            None,
            self.show_pipeline_summary,
            false,
            print_success,
            self.verbose,
            None,
        )
    }

    fn run(&mut self) -> CliResult<()> {
        println!("SpectraLang session REPL");
        println!(
            "  declarations and 'let' bindings accumulate in module '{}' and are recompiled",
            REPL_MODULE_NAME
        );
        println!(
            "  as a whole; bare expressions evaluate immediately. Type ':help' for commands."
        );

        let stdin = io::stdin();
        let mut block_lines: Option<Vec<String>> = None;

        loop {
            let prompt = if block_lines.is_some() {
                "   ....> "
            } else {
                "spectra> "
            };
            print!("{}", prompt);
            io::stdout()
                .flush()
                .map_err(|error| CliError::io(format!("Failed to flush prompt: {}", error)))?;

            let mut line = String::new();
            let bytes = stdin
                .read_line(&mut line)
                .map_err(|error| CliError::io(format!("Failed to read input: {}", error)))?;

            if bytes == 0 {
                println!();
                break;
            }

            let trimmed = line.trim();

            if let Some(lines) = block_lines.as_mut() {
                if trimmed == "}:" {
                    let block = lines.join("\n");
                    block_lines = None;
                    self.process_input(&block);
                } else if !trimmed.is_empty() {
                    lines.push(trimmed.to_string());
                }
                continue;
            }

            if trimmed.is_empty() {
                continue;
            }

            if trimmed == ":{" {
                block_lines = Some(Vec::new());
                println!(
                    "  multi-line input; finish with '}}:' on its own line, processed as one unit"
                );
                continue;
            }

            if trimmed.starts_with(':') {
                if !self.handle_command(trimmed)? {
                    break;
                }
                continue;
            }

            self.process_input(trimmed);
        }

        let _ = fs::remove_file(&self.scratch_path);
        Ok(())
    }

    fn process_input(&mut self, text: &str) {
        match repl_classify_input(text) {
            ReplInput::Expression(expression) => {
                self.evaluate_expression(expression);
            }
            _ => match self.buffer.append_checked(text, &self.base_options) {
                Ok(AppendedEntry::Binding { name }) => {
                    // Echo the bound type like a minimal language REPL.
                    match self.buffer.infer_binding_type(&name, &self.base_options) {
                        Ok(inferred) => println!("  {} : {}", name, inferred),
                        Err(_) => println!("  bound '{}'", name),
                    }
                }
                Ok(AppendedEntry::Declaration) => println!("  appended to session buffer"),
                Err(diagnostics) => {
                    println!("  rejected; buffer unchanged:");
                    for diagnostic in &diagnostics {
                        println!("    {}", diagnostic);
                    }
                }
            },
        }
    }

    fn evaluate_expression(&self, expression: &str) {
        // Front-end gate first so broken expressions never reach the JIT.
        let snapshot = self.buffer.build_snapshot(SnapshotTail::Expression(expression));
        let diagnostics = snapshot.render_diagnostics(&self.base_options);
        if !diagnostics.is_empty() {
            for diagnostic in &diagnostics {
                println!("  {}", diagnostic);
            }
            return;
        }

        if let Err(error) = fs::write(&self.scratch_path, &snapshot.source) {
            log_error(&format!(
                "Failed to write REPL session scratch file: {}",
                error
            ));
            return;
        }

        if let Err(error) =
            self.compile_entries(vec![self.scratch_path.clone()], BuildCommand::Run, false)
        {
            log_error(&error.message);
        }
    }

    /// Returns `false` when the loop should exit (`:quit`).
    fn handle_command(&mut self, input: &str) -> CliResult<bool> {
        let command = input[1..].trim();
        if command.is_empty() {
            print_repl_help();
            return Ok(true);
        }

        let (keyword, rest) = match command.find(char::is_whitespace) {
            Some(index) => (&command[..index], command[index..].trim()),
            None => (command, ""),
        };

        match keyword {
            "help" | "h" => {
                print_repl_help();
            }
            "quit" | "q" | "exit" => return Ok(false),
            "reset" => {
                self.buffer.clear();
                println!("  session buffer cleared");
            }
            "buffer" | "b" => {
                println!("{}", self.buffer.summary());
            }
            "save" | "s" => {
                if rest.is_empty() {
                    println!("Usage: :save <path>");
                    return Ok(true);
                }
                match fs::write(rest, self.buffer.source()) {
                    Ok(()) => println!("     Saved session buffer to {}", rest),
                    Err(error) => println!("  failed to save '{}': {}", rest, error),
                }
            }
            "load" | "l" => {
                if rest.is_empty() {
                    println!("Usage: :load <path>   (merges the file into the session buffer)");
                    return Ok(true);
                }
                match fs::read_to_string(rest) {
                    Ok(contents) => {
                        match self.buffer.merge_checked(&contents, &self.base_options) {
                            Ok(()) => println!("  merged {} into the session buffer", rest),
                            Err(diagnostics) => {
                                println!("  merge rejected; buffer unchanged:");
                                for diagnostic in &diagnostics {
                                    println!("    {}", diagnostic);
                                }
                            }
                        }
                    }
                    Err(error) => println!("  failed to read '{}': {}", rest, error),
                }
            }
            "type" | "t" => {
                if rest.is_empty() {
                    println!("Usage: :type <expression>");
                    return Ok(true);
                }
                match repl_infer_expression_type(&self.buffer, rest, &self.base_options) {
                    Ok(inferred) => println!("  {}", inferred),
                    Err(diagnostics) => {
                        for diagnostic in &diagnostics {
                            println!("  {}", diagnostic);
                        }
                    }
                }
            }
            "run" | "check" | "compile" | "build" => {
                let entries: Vec<PathBuf> = rest.split_whitespace().map(PathBuf::from).collect();
                if entries.is_empty() {
                    println!("Usage: :{} <paths>...", keyword);
                    return Ok(true);
                }
                let command = match keyword {
                    "run" => BuildCommand::Run,
                    "check" => BuildCommand::Check,
                    _ => BuildCommand::Compile,
                };
                if let Err(error) = self.compile_entries(entries, command, true) {
                    log_error(&error.message);
                }
            }
            unknown => {
                println!(
                    "Unknown REPL command ':{}'. Type ':help' for assistance.",
                    unknown
                );
            }
        }

        Ok(true)
    }
}

fn execute_new_project(options: NewProjectOptions) -> CliResult<()> {
    create_new_project(options)
}

fn create_new_project(options: NewProjectOptions) -> CliResult<()> {
    let NewProjectOptions { path, force } = options;

    if path.exists() {
        if !path.is_dir() {
            return Err(CliError::io(format!(
                "Path '{}' exists and is not a directory.",
                path.display()
            )));
        }

        if !force
            && !is_directory_empty(&path).map_err(|error| {
                CliError::io(format!("Failed to inspect '{}': {}", path.display(), error))
            })?
        {
            return Err(CliError::usage(format!(
                "Directory '{}' already exists. Use '--force' to scaffold anyway.",
                path.display()
            )));
        }
    }

    fs::create_dir_all(path.join("src")).map_err(|error| {
        CliError::io(format!(
            "Failed to create project directories under '{}': {}",
            path.display(),
            error
        ))
    })?;

    let (project_name, module_name) = derive_project_identifiers(&path);
    let manifest_path = path.join("spectra.toml");
    let main_source_path = path.join("src").join("main.spectra");

    let manifest_contents = format!(
        "[project]\nname = \"{}\"\nversion = \"0.1.0\"\nentry = \"src/main.spectra\"\nsrc_dirs = [\"src\"]\n\n[release]\nchannel = \"nightly\"\ncompatibility = \"spectralang-0.1\"\n\n[dependencies]\n# Add your dependencies here\n",
        project_name
    );

    let main_source = format!(
        "// SpectraLang starter module\n// Generated by `spectra new`\n\nmodule {}\n\nfunc add(lhs: int, rhs: int) returns int {{\n    return lhs + rhs\n}}\n\npublic func main() returns int {{\n    let first = 21\n    let second = 21\n    let total = add(first, second)\n    return total\n}}\n",
        module_name
    );

    fs::write(&manifest_path, manifest_contents).map_err(|error| {
        CliError::io(format!(
            "Failed to write manifest '{}': {}",
            manifest_path.display(),
            error
        ))
    })?;

    fs::write(&main_source_path, main_source).map_err(|error| {
        CliError::io(format!(
            "Failed to write source file '{}': {}",
            main_source_path.display(),
            error
        ))
    })?;

    println!("     Created \"{}\" project", path.display());
    println!("       entry: {}", main_source_path.display());
    println!(
        "         run: spectra run \"{}\"",
        main_source_path.display()
    );

    Ok(())
}

