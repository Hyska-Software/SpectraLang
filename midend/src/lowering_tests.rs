use super::*;

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
    fn r2103_async_await_lowers_to_lazy_coroutine() {
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
        assert!(pretty.contains("fn ready__poll("));
        assert!(pretty.contains("fn add_one__poll("));
        assert!(pretty.contains("coroutine.create"));
        assert!(pretty.contains("poll.child"));
        assert!(pretty.contains("coroutine.suspend"));
        assert!(!pretty.contains("spectra.async.task.wait"));
        assert!(!pretty.contains("async.ready"));
    }

    #[test]
    fn r2103_async_early_return_completes_each_poll_exit() {
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
        assert!(pretty.contains("fn choose(bool flag) -> Task<int>"));
        assert!(pretty.contains("fn choose__poll("));
        assert!(pretty.matches("coroutine.complete").count() >= 2, "{pretty}");
    }


    #[test]
    fn array_for_loop_lowers_to_direct_index_loop_without_host_calls() {
        let ir = lower_source(
            r#"
            module array_index_loop

            func total()  returns  int {
                let values = [3, 1, 4, 1, 5, 9, 2, 6]
                let sum = 0
                for value in values {
                    sum = sum + value
                }
                return sum
            }
            "#,
        );

        let pretty = crate::ir::pretty::format_module(&ir);
        // The array arm must not go through the collections iterator ABI.
        assert!(!pretty.contains("spectra.std.collections.iterator_from_values"));
        assert!(!pretty.contains("spectra.std.collections.iterator_remaining"));
        assert!(!pretty.contains("spectra.std.collections.iterator_next"));
        assert!(!pretty.contains("spectra.std.collections.list_iter"));
        // The loop is emitted as explicit cond/body/latch blocks with a GEP load.
        assert!(pretty.contains("array.cond"), "{pretty}");
        assert!(pretty.contains("array.body"), "{pretty}");
        assert!(pretty.contains("array.latch"), "{pretty}");
        assert!(pretty.contains("array.exit"), "{pretty}");
        assert!(pretty.contains("= gep "), "{pretty}");
    }

    #[test]
    fn unsized_array_parameter_iteration_is_rejected_with_a_clear_error() {
        let tokens = Lexer::new(
            r#"
            module unsized_param_iteration

            func total(values: array<int>)  returns  int {
                let sum = 0
                for value in values {
                    sum = sum + value
                }
                return sum
            }
            "#,
        )
        .tokenize()
        .expect("lexing should pass");
        let mut module = Parser::new(tokens, HashSet::new())
            .parse()
            .expect("parsing should pass");
        analyze_modules(&mut [&mut module]).expect("semantic analysis should pass");
        let errors = ASTLowering::new()
            .lower_module(&module)
            .expect_err("unsized array parameters cannot be iterated");
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("without a static length")),
            "{errors:?}"
        );
    }

    #[test]
    fn empty_array_binding_lowers_without_a_loop() {
        let ir = lower_source(
            r#"
            module empty_array_iteration

            func probe()  returns  int {
                let values: array<int> = []
                let touched = false
                for value in values {
                    touched = true
                }
                if touched == true {
                    return 1
                }
                return 0
            }
            "#,
        );

        let pretty = crate::ir::pretty::format_module(&ir);
        assert!(!pretty.contains("array.cond"), "{pretty}");
        assert!(!pretty.contains("iterator_from_values"), "{pretty}");
    }
}
