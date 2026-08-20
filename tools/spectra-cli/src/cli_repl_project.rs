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

    let session = ReplSession::new(base_options, autorun, show_pipeline_summary, verbose);

    if !preload.is_empty() {
        if let Err(error) = session.compile_entries(preload, session.default_command(), true) {
            log_error(&error.message);
        }
    }

    session.run()
}

struct ReplSession {
    base_options: CompilationOptions,
    autorun: bool,
    show_pipeline_summary: bool,
    verbose: bool,
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

    fn run(&self) -> CliResult<()> {
        println!("SpectraLang REPL (type ':help' for commands)");

        let stdin = io::stdin();

        loop {
            print!("spectra> ");
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
            if trimmed.is_empty() {
                continue;
            }

            if trimmed.starts_with(':') {
                if !self.handle_command(trimmed)? {
                    break;
                }
                continue;
            }

            let entries: Vec<PathBuf> = trimmed.split_whitespace().map(PathBuf::from).collect();

            if let Err(error) = self.compile_entries(entries, self.default_command(), true) {
                log_error(&error.message);
            }
        }

        Ok(())
    }

    fn handle_command(&self, input: &str) -> CliResult<bool> {
        let command = input[1..].trim();
        if command.is_empty() {
            print_repl_help();
            return Ok(true);
        }

        let mut parts = command.split_whitespace();
        let keyword = parts.next().unwrap();
        let args: Vec<PathBuf> = parts.map(PathBuf::from).collect();

        match keyword {
            "help" | "h" => {
                print_repl_help();
                Ok(true)
            }
            "quit" | "q" | "exit" => Ok(false),
            "load" | "l" => {
                if args.is_empty() {
                    println!("Usage: :load <paths>...");
                    return Ok(true);
                }
                if let Err(error) = self.compile_entries(args, BuildCommand::Compile, true) {
                    log_error(&error.message);
                }
                Ok(true)
            }
            "run" => {
                if args.is_empty() {
                    println!("Usage: :run <paths>...");
                    return Ok(true);
                }
                if let Err(error) = self.compile_entries(args, BuildCommand::Run, true) {
                    log_error(&error.message);
                }
                Ok(true)
            }
            "check" => {
                if args.is_empty() {
                    println!("Usage: :check <paths>...");
                    return Ok(true);
                }
                if let Err(error) = self.compile_entries(args, BuildCommand::Check, true) {
                    log_error(&error.message);
                }
                Ok(true)
            }
            "compile" | "build" => {
                if args.is_empty() {
                    println!("Usage: :compile <paths>...");
                    return Ok(true);
                }
                if let Err(error) = self.compile_entries(args, BuildCommand::Compile, true) {
                    log_error(&error.message);
                }
                Ok(true)
            }
            unknown => {
                println!(
                    "Unknown REPL command ':{}'. Type ':help' for assistance.",
                    unknown
                );
                Ok(true)
            }
        }
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

