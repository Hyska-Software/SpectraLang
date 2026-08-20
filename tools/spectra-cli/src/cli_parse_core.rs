fn run_cli() -> CliResult<()> {
    let action = parse_cli()?;
    execute_action(action)
}

fn execute_action(action: CliAction) -> CliResult<()> {
    match action {
        CliAction::Help(topic) => {
            match topic {
                HelpTopic::Global => print_global_help(),
                HelpTopic::Build(command) => print_build_help(command),
                HelpTopic::Repl => print_repl_help(),
                HelpTopic::NewProject => print_new_help(),
                HelpTopic::ReleaseInfo => print_release_info_help(),
                HelpTopic::Package => print_package_help(),
                HelpTopic::Db => print_db_help(),
                HelpTopic::Format => print_format_help(),
                HelpTopic::Lint => print_lint_help(),
            }
            Ok(())
        }
        CliAction::ListExperimental => {
            print_experimental_features();
            Ok(())
        }
        CliAction::Build { kind, invocation } => execute_build_command(kind, invocation),
        CliAction::Repl(options) => execute_repl(options),
        CliAction::NewProject(options) => execute_new_project(options),
        CliAction::ReleaseInfo(options) => execute_release_info(options),
        CliAction::Package(invocation) => execute_package_command(invocation),
        CliAction::Format(options) => execute_format(options),
        CliAction::Db(invocation) => execute_db_command(invocation),
    }
}

fn parse_cli() -> CliResult<CliAction> {
    let mut args = env::args().skip(1).peekable();

    if args.peek().is_none() {
        return Err(usage_error("No command or input files provided."));
    }

    match args.peek().map(|value| value.as_str()) {
        Some("--help") | Some("-h") => {
            args.next();
            return Ok(CliAction::Help(HelpTopic::Global));
        }
        Some("help") => {
            args.next();
            if let Some(target) = args.next() {
                return match target.as_str() {
                    "new" | "new-project" => Ok(CliAction::Help(HelpTopic::NewProject)),
                    "release-info" | "release" => Ok(CliAction::Help(HelpTopic::ReleaseInfo)),
                    "package" | "pkg" => Ok(CliAction::Help(HelpTopic::Package)),
                    "db" | "database" => Ok(CliAction::Help(HelpTopic::Db)),
                    "repl" => Ok(CliAction::Help(HelpTopic::Repl)),
                    "fmt" | "format" => Ok(CliAction::Help(HelpTopic::Format)),
                    "lint" => Ok(CliAction::Help(HelpTopic::Lint)),
                    "bench" => Ok(CliAction::Help(HelpTopic::Build(BuildCommand::Bench))),
                    other => {
                        if let Some(kind) = parse_build_command_name(other) {
                            Ok(CliAction::Help(HelpTopic::Build(kind)))
                        } else {
                            Err(usage_error(&format!("Unknown command '{}'.", other)))
                        }
                    }
                };
            } else {
                return Ok(CliAction::Help(HelpTopic::Global));
            }
        }
        Some("--list-experimental") => {
            args.next();
            if args.peek().is_some() {
                return Err(usage_error("--list-experimental must be used on its own."));
            }
            return Ok(CliAction::ListExperimental);
        }
        Some("repl") => {
            args.next();
            if let Some(flag) = args.peek() {
                if matches!(flag.as_str(), "--help" | "-h") {
                    args.next();
                    return Ok(CliAction::Help(HelpTopic::Repl));
                }
            }

            let options = parse_repl_invocation(&mut args)?;
            return Ok(CliAction::Repl(options));
        }
        Some("new") | Some("new-project") => {
            args.next();
            if let Some(flag) = args.peek() {
                if matches!(flag.as_str(), "--help" | "-h") {
                    args.next();
                    return Ok(CliAction::Help(HelpTopic::NewProject));
                }
            }

            let options = parse_new_project_invocation(&mut args)?;
            return Ok(CliAction::NewProject(options));
        }
        Some("release-info") | Some("release") => {
            args.next();
            if let Some(flag) = args.peek() {
                if matches!(flag.as_str(), "--help" | "-h") {
                    args.next();
                    return Ok(CliAction::Help(HelpTopic::ReleaseInfo));
                }
            }

            let options = parse_release_info_invocation(&mut args)?;
            return Ok(CliAction::ReleaseInfo(options));
        }
        Some("package") | Some("pkg") => {
            args.next();
            if let Some(flag) = args.peek() {
                if matches!(flag.as_str(), "--help" | "-h") {
                    args.next();
                    return Ok(CliAction::Help(HelpTopic::Package));
                }
            }

            let invocation = parse_package_invocation(&mut args)?;
            return Ok(CliAction::Package(invocation));
        }
        Some("db") => {
            args.next();
            if let Some(flag) = args.peek() {
                if matches!(flag.as_str(), "--help" | "-h") {
                    args.next();
                    return Ok(CliAction::Help(HelpTopic::Db));
                }
            }
            return Ok(CliAction::Db(parse_db_invocation(&mut args)?));
        }
        Some("fmt") | Some("format") => {
            args.next();
            if let Some(flag) = args.peek() {
                if matches!(flag.as_str(), "--help" | "-h") {
                    args.next();
                    return Ok(CliAction::Help(HelpTopic::Format));
                }
            }

            let options = parse_format_invocation(&mut args)?;
            return Ok(CliAction::Format(options));
        }
        Some("lint") => {
            args.next();
            if let Some(flag) = args.peek() {
                if matches!(flag.as_str(), "--help" | "-h") {
                    args.next();
                    return Ok(CliAction::Help(HelpTopic::Lint));
                }
            }

            let invocation = parse_compilation_invocation(&mut args, BuildCommand::Lint, true)?;
            return Ok(CliAction::Build {
                kind: BuildCommand::Lint,
                invocation,
            });
        }
        _ => {}
    }

    let mut command = BuildCommand::Compile;

    if let Some(value) = args.peek() {
        if !value.starts_with('-') {
            if let Some(kind) = parse_build_command_name(value) {
                command = kind;
                args.next();
            }
        }
    }

    if let Some(flag) = args.peek() {
        if matches!(flag.as_str(), "--help" | "-h") {
            args.next();
            return Ok(CliAction::Help(HelpTopic::Build(command)));
        }
    }

    let invocation =
        parse_compilation_invocation(&mut args, command, matches!(command, BuildCommand::Lint))?;

    Ok(CliAction::Build {
        kind: command,
        invocation,
    })
}

fn parse_build_command_name(value: &str) -> Option<BuildCommand> {
    match value {
        "compile" | "build" => Some(BuildCommand::Compile),
        "check" => Some(BuildCommand::Check),
        "run" => Some(BuildCommand::Run),
        "lint" => Some(BuildCommand::Lint),
        "bench" => Some(BuildCommand::Bench),
        _ => None,
    }
}

fn parse_db_invocation<I>(args: &mut std::iter::Peekable<I>) -> CliResult<DbInvocation>
where
    I: Iterator<Item = String>,
{
    let command = match args.next().as_deref() {
        Some("migrate") => DbCommand::Migrate,
        Some("rollback") => DbCommand::Rollback,
        Some("status") => DbCommand::Status,
        Some(other) => return Err(usage_error(&format!("Unknown db subcommand '{other}'."))),
        None => return Err(usage_error("Missing db subcommand. Use migrate, rollback, or status.")),
    };
    let mut database = None;
    let mut migrations_dir = None;
    let mut steps = 1;
    let mut json = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--database" => database = Some(PathBuf::from(args.next().ok_or_else(|| usage_error("Missing path after --database."))?)),
            "--migrations-dir" => migrations_dir = Some(PathBuf::from(args.next().ok_or_else(|| usage_error("Missing path after --migrations-dir."))?)),
            "--steps" => {
                let value = args.next().ok_or_else(|| usage_error("Missing number after --steps."))?;
                steps = value.parse::<usize>().map_err(|_| usage_error("--steps must be a non-negative integer."))?;
            }
            "--json" => json = true,
            "--help" | "-h" => return Err(usage_error("Use 'spectralang help db' for database command help.")),
            other => return Err(usage_error(&format!("Unknown db option '{other}'."))),
        }
    }
    if json && !matches!(command, DbCommand::Status) {
        return Err(usage_error("--json is currently supported only by 'db status'."));
    }
    Ok(DbInvocation {
        command,
        database: database.ok_or_else(|| usage_error("--database is required."))?,
        migrations_dir: migrations_dir.ok_or_else(|| usage_error("--migrations-dir is required."))?,
        steps,
        json,
    })
}

fn execute_db_command(invocation: DbInvocation) -> CliResult<()> {
    let connection = SqliteConnection::open(&invocation.database, std::time::Duration::from_secs(5))
        .map_err(|error| CliError::io(error.to_string()))?;
    let migrator = SqliteMigrator::from_directory(connection, &invocation.migrations_dir)
        .map_err(|error| CliError::compilation(error.to_string()))?;
    match invocation.command {
        DbCommand::Migrate => {
            let entries = migrator.migrate().map_err(|error| CliError::compilation(error.to_string()))?;
            for entry in entries {
                println!("applied {} {}", entry.version, entry.name);
            }
        }
        DbCommand::Rollback => {
            let entries = migrator.rollback(invocation.steps).map_err(|error| CliError::compilation(error.to_string()))?;
            for entry in entries {
                println!("rolled back {} {}", entry.version, entry.name);
            }
        }
        DbCommand::Status => {
            let status = migrator.status().map_err(|error| CliError::compilation(error.to_string()))?;
            if invocation.json {
                let value = json!({
                    "applied": status.applied.iter().map(|entry| json!({"version": entry.version, "name": entry.name, "checksum": entry.checksum, "applied_at": entry.applied_at})).collect::<Vec<_>>(),
                    "pending": status.pending.iter().map(|entry| json!({"version": entry.version, "name": entry.name, "checksum": entry.checksum})).collect::<Vec<_>>(),
                    "drift": status.drift.iter().map(|entry| json!({"version": entry.version, "reason": entry.reason})).collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string_pretty(&value).map_err(|error| CliError::io(error.to_string()))?);
            } else {
                println!("applied: {}", status.applied.len());
                println!("pending: {}", status.pending.len());
                for entry in status.drift { println!("drift {}: {}", entry.version, entry.reason); }
            }
        }
    }
    Ok(())
}

