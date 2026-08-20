#[cfg(test)]
mod tests {
    use super::*;
    use spectra_compiler::{analyze_modules, Lexer, Parser};
    use std::collections::HashSet;

    fn lower_source(source: &str) -> IRModule {
        let tokens = Lexer::new(source).tokenize().expect("lexing should pass");
        let mut module = Parser::new(tokens, HashSet::new())
            .parse()
            .expect("parsing should pass");
        analyze_modules(&mut [&mut module]).expect("semantic analysis should pass");
        ASTLowering::new()
            .lower_module(&module)
            .expect("lowering should pass")
    }

    #[test]
    fn r2103_async_await_lowers_to_task_and_suspend_resume_markers() {
        let ir = lower_source(
            r#"
            module r2103_async_await

            async func ready()  returns  int {
                return 41
            }

            async func add_one()  returns  int {
                let value = await ready()
                return value + 1
            }
            "#,
        );

        let pretty = crate::ir::pretty::format_module(&ir);
        assert!(pretty.contains("fn ready() -> Task<int>"));
        assert!(pretty.contains("fn add_one() -> Task<int>"));
        assert!(pretty.contains("async.suspend"));
        assert!(pretty.contains("async.resume"));
        assert!(pretty.contains("async.ready"));
        assert!(pretty.contains("spectra.async.task.ready"));
        assert!(pretty.contains("spectra.async.task.poll"));
        assert!(!pretty.contains("spectra.async.task.block_on"));
        assert!(pretty.contains("spectra.async.task.result"));
    }

    #[test]
    fn r2103_async_early_return_lowers_every_exit_to_ready_task() {
        let ir = lower_source(
            r#"
            module r2103_async_early_return

            async func choose(flag: bool)  returns  int {
                if flag {
                    return 7
                }
                return 9
            }
            "#,
        );

        let pretty = crate::ir::pretty::format_module(&ir);
        let ready_markers = pretty.matches("async.ready").count();
        assert!(pretty.contains("fn choose(bool flag) -> Task<int>"));
        assert!(ready_markers >= 2, "{pretty}");
    }
}
