#[cfg(test)]
mod tests {
    use super::*;
    use spectra_compiler::lint::LintOptions;
    use std::collections::HashSet;

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
        let result = compiler.compile(source, "test.spectra");

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
            experimental_features: HashSet::new(),
            lint: LintOptions::default(),
        };

        let mut compiler = SpectraCompiler::new(options);
        let result = compiler.compile(source, "test.spectra");

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
        let result = compiler.compile(source, "test.spectra");

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
        let result = compiler.compile(source, "test.spectra");

        assert!(result.is_ok());
    }
}
