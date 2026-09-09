#[cfg(test)]
mod tests {
    use super::*;
    use spectra_compiler::lint::LintOptions;

    #[test]
    fn test_end_to_end_simple() {
        let source = r#"
            module test
            
            func add(a: int, b: int)  returns  int {
                return a + b
            }
            
            public func main() {
                let x = add(5, 3)
                return
            }
        "#;

        let mut compiler = SpectraCompiler::default();
        let result = compiler.compile_with_line_shift(source, "test.spectra", source, 0);

        assert!(result.is_ok());
    }

    #[test]
    fn test_end_to_end_with_optimization() {
        let source = r#"
            module test
            
            func compute()  returns  int {
                let x = 10 + 20
                let y = x * 2
                return y
            }
            
            public func main() {
                let result = compute()
                return
            }
        "#;

        let options = CompilationOptions {
            debug_info: spectra_compiler::DebugInfoMode::Native,
            optimize: true,
            opt_level: 2,
            dump_ir: false,
            dump_ast: false,
            run_jit: false,
            collect_metrics: false,
            lint: LintOptions::default(),
        };

        let mut compiler = SpectraCompiler::new(options);
        let result = compiler.compile_with_line_shift(source, "test.spectra", source, 0);

        assert!(result.is_ok());
    }

    #[test]
    fn test_end_to_end_control_flow() {
        let source = r#"
            module test
            
            func max(a: int, b: int)  returns  int {
                if a > b {
                    return a
                } else {
                    return b
                }
            }
            
            public func main() {
                let result = max(10, 20)
                return
            }
        "#;

        let mut compiler = SpectraCompiler::default();
        let result = compiler.compile_with_line_shift(source, "test.spectra", source, 0);

        assert!(result.is_ok());
    }

    #[test]
    fn test_end_to_end_loop() {
        let source = r#"
            module test
            
            func factorial(n: int)  returns  int {
                let result = 1
                let i = 1
                
                while i <= n {
                    result = result * i
                    i = i + 1
                }
                
                return result
            }
            
            public func main() {
                let result = factorial(5)
                return
            }
        "#;

        let mut compiler = SpectraCompiler::default();
        let result = compiler.compile_with_line_shift(source, "test.spectra", source, 0);

        assert!(result.is_ok());
    }
    #[test]
    fn test_line_shift_reports_disk_lines() {
        // The mismatch is on disk line 2; the synthetic `module` header moves
        // it to effective line 3. The shifted render must report line 2.
        let disk = "func f() returns int {\n    return \"oops\"\n}\n";
        let effective = format!("module test\n{disk}");
        let mut compiler = SpectraCompiler::default();
        let shifted = compiler
            .compile_with_line_shift(&effective, "shift.spectra", disk, 1)
            .expect_err("returning a string from an int function must fail");
        assert!(
            shifted.contains("shift.spectra:2:"),
            "expected disk line 2, got:\n{shifted}"
        );
        assert!(
            !shifted.contains("shift.spectra:3:"),
            "effective line must not leak, got:\n{shifted}"
        );
    }

    #[test]
    fn test_line_shift_zero_reports_compiled_lines() {
        // shift = 0 applies no adjustment: the compiled (effective) line 3
        // is reported as-is.
        let disk = "func f() returns int {\n    return \"oops\"\n}\n";
        let effective = format!("module test\n{disk}");
        let mut compiler = SpectraCompiler::default();
        let zero = compiler
            .compile_with_line_shift(&effective, "shift.spectra", &effective, 0)
            .expect_err("must fail");
        assert!(
            zero.contains("shift.spectra:3:"),
            "shift 0 must report the compiled line, got:\n{zero}"
        );
    }
}
