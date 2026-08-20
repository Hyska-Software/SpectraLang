fn parse_package_invocation<I>(args: &mut std::iter::Peekable<I>) -> CliResult<PackageInvocation>
where
    I: Iterator<Item = String>,
{
    let subcommand = args
        .next()
        .ok_or_else(|| usage_error("No package subcommand supplied."))?;
    let mut root = PathBuf::from(".");
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut path: Option<PathBuf> = None;
    let mut registry: Option<PathBuf> = None;
    let mut git: Option<String> = None;
    let mut tag: Option<String> = None;
    let mut rev: Option<String> = None;
    let mut branch: Option<String> = None;
    let mut catalog: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut offline = false;
    let mut locked = false;
    let mut extra_positionals: Vec<String> = Vec::new();
    let mut test_filter: Option<String> = None;
    let mut test_list = false;
    let mut test_json = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                let value = args
                    .next()
                    .ok_or_else(|| usage_error("Missing path after '--root'."))?;
                root = PathBuf::from(value);
            }
            "--version" => {
                version = Some(
                    args.next()
                        .ok_or_else(|| usage_error("Missing value after '--version'."))?,
                );
            }
            "--path" => {
                path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        usage_error("Missing path after '--path'.")
                    })?));
            }
            "--registry" => {
                registry =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        usage_error("Missing path after '--registry'.")
                    })?));
            }
            "--git" => {
                git = Some(
                    args.next()
                        .ok_or_else(|| usage_error("Missing URL after '--git'."))?,
                );
            }
            "--tag" => {
                tag = Some(
                    args.next()
                        .ok_or_else(|| usage_error("Missing value after '--tag'."))?,
                );
            }
            "--rev" => {
                rev = Some(
                    args.next()
                        .ok_or_else(|| usage_error("Missing value after '--rev'."))?,
                );
            }
            "--branch" => {
                branch = Some(
                    args.next()
                        .ok_or_else(|| usage_error("Missing value after '--branch'."))?,
                );
            }
            "--catalog" => {
                catalog =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        usage_error("Missing path after '--catalog'.")
                    })?));
            }
            "--out" => {
                out = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| usage_error("Missing path after '--out'."))?,
                ));
            }
            "--offline" => {
                offline = true;
            }
            "--locked" => {
                locked = true;
            }
            "--filter" if subcommand == "test" => {
                test_filter = Some(
                    args.next()
                        .ok_or_else(|| usage_error("Missing value after '--filter'."))?,
                );
            }
            "--list" if subcommand == "test" => {
                test_list = true;
            }
            "--json" if subcommand == "test" => {
                test_json = true;
            }
            flag if flag.starts_with('-') => {
                return Err(usage_error(&format!("Unknown package option: {}", flag)));
            }
            value => {
                if subcommand == "catalog" && name.is_some() {
                    extra_positionals.push(value.to_string());
                    continue;
                }
                if name.is_some() {
                    return Err(usage_error(
                        "Multiple package names supplied. Provide exactly one name.",
                    ));
                }
                name = Some(value.to_string());
            }
        }
    }

    let command = match subcommand.as_str() {
        "lock" => PackageCommand::Lock,
        "build" => PackageCommand::Build,
        "check" => PackageCommand::Check,
        "run" => PackageCommand::Run,
        "test" => PackageCommand::Test(package::PackageTestOptions {
            filter: test_filter,
            list: test_list,
            json: test_json,
        }),
        "bench" => PackageCommand::Bench,
        "doc" => PackageCommand::Doc,
        "update" => PackageCommand::Update,
        "fetch" => PackageCommand::Fetch { offline },
        "search" => PackageCommand::Search {
            query: name.ok_or_else(|| usage_error("package search requires a query."))?,
            catalog,
        },
        "info" => PackageCommand::Info {
            name: name.ok_or_else(|| usage_error("package info requires a package name."))?,
            catalog,
        },
        "versions" => PackageCommand::Versions {
            name: name.ok_or_else(|| usage_error("package versions requires a package name."))?,
            catalog,
        },
        "tree" => PackageCommand::Tree,
        "add" => PackageCommand::Add {
            name: name.ok_or_else(|| usage_error("package add requires a package name."))?,
            version,
            path,
            registry,
            git,
            tag,
            rev,
            branch,
            catalog,
        },
        "register" => PackageCommand::Register {
            git: git.ok_or_else(|| usage_error("package register requires --git <url>."))?,
            tag,
            rev,
            branch,
            catalog: catalog
                .ok_or_else(|| usage_error("package register requires --catalog <path>."))?,
        },
        "publish-metadata" => PackageCommand::PublishMetadata {
            out: out
                .ok_or_else(|| usage_error("package publish-metadata requires --out <path>."))?,
            git,
            tag,
            rev,
            branch,
        },
        "catalog" => {
            match name.as_deref() {
                Some("add") => PackageCommand::Catalog(package::CatalogCommand::Add {
                    name: extra_positionals.first().cloned().ok_or_else(|| {
                        usage_error("package catalog add requires a catalog name.")
                    })?,
                    source: extra_positionals.get(1).cloned().ok_or_else(|| {
                        usage_error("package catalog add requires a source path.")
                    })?,
                }),
                Some("list") | None => PackageCommand::Catalog(package::CatalogCommand::List),
                Some("sync") => PackageCommand::Catalog(package::CatalogCommand::Sync {
                    offline,
                    locked,
                }),
                Some("remove") => PackageCommand::Catalog(package::CatalogCommand::Remove {
                    name: extra_positionals.first().cloned().ok_or_else(|| {
                        usage_error("package catalog remove requires a catalog name.")
                    })?,
                }),
                Some(other) => {
                    return Err(usage_error(&format!(
                        "Unknown package catalog action '{}'.",
                        other
                    )));
                }
            }
        }
        "publish" => PackageCommand::Publish {
            registry: registry
                .ok_or_else(|| usage_error("package publish requires --registry <path>."))?,
        },
        other => {
            return Err(usage_error(&format!(
                "Unknown package subcommand '{}'.",
                other
            )));
        }
    };

    Ok(PackageInvocation {
        root,
        command,
        offline,
        locked,
    })
}

fn parse_format_invocation<I>(args: &mut std::iter::Peekable<I>) -> CliResult<FormatOptions>
where
    I: Iterator<Item = String>,
{
    let mut entries = Vec::new();
    let mut check = false;
    let mut use_stdin = false;
    let mut write_stdout = false;
    let mut stats = false;
    let mut explain = ExplainMode::None;
    let mut config_path: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" => {
                for value in args {
                    entries.push(PathBuf::from(value));
                }
                break;
            }
            "--check" => check = true,
            "--stdin" => use_stdin = true,
            "--stdout" => write_stdout = true,
            "--explain" => {
                if explain != ExplainMode::None {
                    return Err(usage_error(
                        "Multiple --explain options provided. Specify it at most once.",
                    ));
                }
                explain = ExplainMode::Text;
                check = true;
            }
            flag if flag.starts_with("--explain=") => {
                if explain != ExplainMode::None {
                    return Err(usage_error(
                        "Multiple --explain options provided. Specify it at most once.",
                    ));
                }
                let value = &flag[10..];
                explain = match value {
                    "text" => ExplainMode::Text,
                    "json" => ExplainMode::Json,
                    other => {
                        return Err(usage_error(&format!(
                            "Unknown --explain mode '{}'. Use 'text' or 'json'.",
                            other
                        )))
                    }
                };
                check = true;
            }
            "--config" => {
                if config_path.is_some() {
                    return Err(usage_error(
                        "Multiple --config options provided. Supply at most one configuration path.",
                    ));
                }
                if let Some(value) = args.next() {
                    config_path = Some(PathBuf::from(value));
                } else {
                    return Err(usage_error("Missing path argument after '--config'."));
                }
            }
            "--stats" => {
                stats = true;
            }
            flag if flag.starts_with('-') => {
                return Err(usage_error(&format!("Unknown option: {}", flag)));
            }
            _ => entries.push(PathBuf::from(arg)),
        }
    }

    if use_stdin && !entries.is_empty() {
        return Err(usage_error(
            "--stdin cannot be combined with explicit file or directory paths.",
        ));
    }

    if !use_stdin && entries.is_empty() {
        return Err(usage_error(
            "No source files or directories were provided for formatting.",
        ));
    }

    Ok(FormatOptions {
        entries,
        check,
        use_stdin,
        write_stdout,
        explain,
        stats,
        config_path,
    })
}

