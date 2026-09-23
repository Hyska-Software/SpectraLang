#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn exit_code_values_are_stable() {
        assert_eq!(ExitCode::Success.as_i32(), 0);
        assert_eq!(ExitCode::Usage.as_i32(), 64);
        assert_eq!(ExitCode::CompilationFailed.as_i32(), 65);
        assert_eq!(ExitCode::IoError.as_i32(), 74);
    }

    #[test]
    fn usage_error_includes_help_hint() {
        let error = usage_error("Missing source");
        assert_eq!(error.code.as_i32(), ExitCode::Usage.as_i32());
        assert!(error.message.contains("Missing source"));
        assert!(error
            .message
            .contains("Use 'spectralang --help' for usage information."));
    }

    #[test]
    fn program_name_uses_canonical_fallback_until_run_initializes_it() {
        assert_eq!(program_name(), "spectralang");
    }

    #[test]
    fn program_name_derives_from_arg0() {
        assert_eq!(program_name_from_arg0(None), "spectralang");
        assert_eq!(program_name_from_arg0(Some(OsStr::new(""))), "spectralang");
        assert_eq!(program_name_from_arg0(Some(OsStr::new("spc"))), "spc");
        assert_eq!(program_name_from_arg0(Some(OsStr::new("spc.exe"))), "spc");
        assert_eq!(
            program_name_from_arg0(Some(OsStr::new("target/debug/spc.exe"))),
            "spc"
        );
        assert_eq!(
            program_name_from_arg0(Some(OsStr::new("bin/spectralang"))),
            "spectralang"
        );
    }

    #[test]
    fn cli_error_builders_assign_codes() {
        let compilation = CliError::compilation("failed");
        assert_eq!(
            compilation.code.as_i32(),
            ExitCode::CompilationFailed.as_i32()
        );

        let io = CliError::io("io issue");
        assert_eq!(io.code.as_i32(), ExitCode::IoError.as_i32());
    }

    #[test]
    fn json_is_allowed_for_check() {
        let mut args = vec![
            "--json".to_string(),
            "../../tests/validation/60_pattern_control_surface.spectra".to_string(),
        ]
        .into_iter()
        .peekable();

        let invocation = parse_compilation_invocation(&mut args, BuildCommand::Check, false)
            .expect("check --json should parse");

        assert!(invocation.json_output);
        assert!(!invocation.sarif_output);
    }

    #[test]
    fn sarif_is_allowed_for_check() {
        let mut args = vec![
            "--sarif".to_string(),
            "../../tests/validation/60_pattern_control_surface.spectra".to_string(),
        ]
        .into_iter()
        .peekable();

        let invocation = parse_compilation_invocation(&mut args, BuildCommand::Check, false)
            .expect("check --sarif should parse");

        assert!(invocation.sarif_output);
        assert!(!invocation.json_output);
    }

    #[test]
    fn json_is_rejected_for_run() {
        let mut args = vec![
            "--json".to_string(),
            "../../tests/validation/60_pattern_control_surface.spectra".to_string(),
        ]
        .into_iter()
        .peekable();

        let error = parse_compilation_invocation(&mut args, BuildCommand::Run, false).unwrap_err();

        assert!(error.message.contains("--json"));
        assert_eq!(error.code.as_i32(), ExitCode::Usage.as_i32());
    }

    #[test]
    fn json_and_sarif_are_mutually_exclusive() {
        let mut args = vec![
            "--json".to_string(),
            "--sarif".to_string(),
            "../../tests/validation/60_pattern_control_surface.spectra".to_string(),
        ]
        .into_iter()
        .peekable();

        let error = parse_compilation_invocation(&mut args, BuildCommand::Check, false)
            .expect_err("json and sarif together should fail");

        assert!(error.message.contains("--json"));
        assert!(error.message.contains("--sarif"));
        assert_eq!(error.code.as_i32(), ExitCode::Usage.as_i32());
    }

    // --- `--run` rejection matrix (only `run` and `compile` execute) ---

    const ENTRY_FIXTURE: &str = "../../tests/validation/60_pattern_control_surface.spectra";

    fn parse_args(args: &[&str], command: BuildCommand) -> CliResult<CliInvocation> {
        let mut iter = args.iter().map(|value| value.to_string()).peekable();
        parse_compilation_invocation(&mut iter, command, command == BuildCommand::Lint)
    }

    #[test]
    fn run_flag_is_rejected_for_commands_that_never_execute() {
        for command in [BuildCommand::Check, BuildCommand::Lint, BuildCommand::Bench] {
            let error = parse_args(&["--run", ENTRY_FIXTURE], command)
                .expect_err(&format!("{:?} must reject --run", command));
            assert!(
                error.message.contains("--run"),
                "{:?} rejection must name the flag: {}",
                command,
                error.message
            );
            assert_eq!(
                error.code.as_i32(),
                ExitCode::Usage.as_i32(),
                "{:?}",
                command
            );
        }
    }

    #[test]
    fn run_flag_short_form_is_rejected_for_lint_too() {
        let error =
            parse_args(&["-r", ENTRY_FIXTURE], BuildCommand::Lint).expect_err("-r == --run");
        assert!(error.message.contains("--run"));
        assert_eq!(error.code.as_i32(), ExitCode::Usage.as_i32());
    }

    #[test]
    fn run_flag_is_accepted_for_run_and_compile() {
        for command in [BuildCommand::Run, BuildCommand::Compile] {
            let invocation = parse_args(&["--run", ENTRY_FIXTURE], command)
                .unwrap_or_else(|error| panic!("{:?} must accept --run: {}", command, error.message));
            assert!(invocation.options.run_jit, "{:?}", command);
        }
    }

    // --- Emit-flag combinations (documented matrix: compile only) ---

    #[test]
    fn emit_object_and_emit_exe_are_mutually_exclusive() {
        let error = parse_args(
            &[
                "--emit-object",
                "out.o",
                "--emit-exe",
                "out.exe",
                ENTRY_FIXTURE,
            ],
            BuildCommand::Compile,
        )
        .expect_err("both emit flags together must fail");

        assert!(error.message.contains("--emit-object"), "{}", error.message);
        assert!(error.message.contains("--emit-exe"), "{}", error.message);
        assert_eq!(error.code.as_i32(), ExitCode::Usage.as_i32());
    }

    #[test]
    fn emit_flags_are_rejected_outside_compile() {
        for command in [
            BuildCommand::Run,
            BuildCommand::Check,
            BuildCommand::Lint,
            BuildCommand::Bench,
        ] {
            for args in [
                ["--emit-object", "out.o", ENTRY_FIXTURE],
                ["--emit-exe", "out.exe", ENTRY_FIXTURE],
            ] {
                let error = parse_args(&args, command)
                    .expect_err(&format!("{:?} must reject {}", command, args[0]));
                assert!(
                    error
                        .message
                        .contains("only supported with the 'compile' command"),
                    "{:?} {}: {}",
                    command,
                    args[0],
                    error.message
                );
                assert_eq!(
                    error.code.as_i32(),
                    ExitCode::Usage.as_i32(),
                    "{:?} {}",
                    command,
                    args[0]
                );
            }
        }
    }

    #[test]
    fn emit_flags_are_accepted_for_compile_individually() {
        let object = parse_args(&["--emit-object", "out.o", ENTRY_FIXTURE], BuildCommand::Compile)
            .expect("compile --emit-object must parse");
        assert!(object.emit_object.is_some());
        assert!(object.emit_exe.is_none());

        let exe = parse_args(&["--emit-exe", "out.exe", ENTRY_FIXTURE], BuildCommand::Compile)
            .expect("compile --emit-exe must parse");
        assert!(exe.emit_exe.is_some());
        assert!(exe.emit_object.is_none());
    }

    #[test]
    fn structured_output_flags_cannot_combine_with_emit_flags() {
        let json = parse_args(
            &["--json", "--emit-object", "out.o", ENTRY_FIXTURE],
            BuildCommand::Compile,
        )
        .expect_err("--json with --emit-object must fail");
        assert!(json.message.contains("--json"), "{}", json.message);
        assert_eq!(json.code.as_i32(), ExitCode::Usage.as_i32());

        let sarif = parse_args(
            &["--sarif", "--emit-exe", "out.exe", ENTRY_FIXTURE],
            BuildCommand::Compile,
        )
        .expect_err("--sarif with --emit-exe must fail");
        assert!(sarif.message.contains("--sarif"), "{}", sarif.message);
        assert_eq!(sarif.code.as_i32(), ExitCode::Usage.as_i32());
    }

    // --- Program exit-code threading (item 1/3) ---
    //
    // These run the real JIT in-process. The assertions below only execute at
    // all because a non-zero program status now travels out as a structured
    // `CliError` instead of calling `std::process::exit` mid-layer (which
    // used to kill the REPL and any other in-process caller).

    #[test]
    fn program_exit_status_propagates_in_process_for_run_and_compile_run() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/cli/runtime_nonzero.spectra");

        // `run`: the status leaves through CliError.
        let run_error = execute_plan_with_sources(
            BuildCommand::Run,
            CompilationOptions {
                run_jit: true,
                ..CompilationOptions::default()
            },
            vec![ProjectSourceEntry::plain(fixture.clone())],
            None,
            false,
            false,
            false,
            false,
            None,
        )
        .expect_err("a program returning 7 must surface as an error carrying the status");
        assert_eq!(
            run_error.outcome,
            Some(CliFailureOutcome::ProgramExit(7)),
            "run must propagate the program's own status"
        );

        // `compile --run`: identical propagation regardless of command kind.
        let compile_error = execute_plan_with_sources(
            BuildCommand::Compile,
            CompilationOptions {
                run_jit: true,
                ..CompilationOptions::default()
            },
            vec![ProjectSourceEntry::plain(fixture.clone())],
            None,
            false,
            false,
            false,
            false,
            None,
        )
        .expect_err("compile --run must propagate the program status too");
        assert_eq!(
            compile_error.outcome,
            Some(CliFailureOutcome::ProgramExit(7)),
            "compile --run must propagate the program's own status"
        );

        // A program exiting 0 must NOT surface as an error.
        let clean =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/cli/lint_clean.spectra");
        execute_plan_with_sources(
            BuildCommand::Run,
            CompilationOptions {
                run_jit: true,
                ..CompilationOptions::default()
            },
            vec![ProjectSourceEntry::plain(clean)],
            None,
            false,
            false,
            false,
            false,
            None,
        )
        .expect("a program exiting 0 must keep reporting success");
    }
}
