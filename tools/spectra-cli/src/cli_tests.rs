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
}
