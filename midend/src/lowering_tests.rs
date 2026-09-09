use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use spectra_compiler::{analyze_modules, Lexer, Parser};
    

    fn lower_source(source: &str) -> IRModule {
        let tokens = Lexer::new(source).tokenize().expect("lexing should pass");
        let mut module = Parser::new(tokens)
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
        let mut module = Parser::new(tokens)
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
    /// R-2112 regression: `shift_body_values` must move every operand id when a
    /// coroutine body is renumbered for the poll prologue. Missed fields (seen
    /// with `GetElementPtr.index` and `CallIndirect.fn_ptr`) leave stale ids that
    /// still resolve — to whatever value now owns the old number — so async
    /// trait-object dispatch silently computed `base + 2 * input`. A mere
    /// definedness check cannot catch the aliasing; instead assert the producer
    /// shape: a vtable index is always an integer constant and an indirect
    /// callee always comes from a vtable slot (or a direct function address).
    /// Any future missed shift field that feeds these positions fails loudly.
    #[test]
    fn r2112_async_dyn_dispatch_shift_keeps_every_operand_defined() {
        let ir = lower_source(
            r#"
            module r2112_async_dyn_shift

            trait Worker {
                async func run(&self, input: int) returns int
            }

            record SendWorker {
                base: int
            }

            impl Worker for SendWorker {
                async func run(&self, input: int) returns int {
                    return self.base + input
                }
            }

            async func drive(worker: dyn Worker + Send, input: int) returns int {
                let result = await worker.run(input)
                return result
            }
            "#,
        );

        let pretty = crate::ir::pretty::format_module(&ir);
        assert!(pretty.contains("call_indirect"), "{pretty}");
        assert!(pretty.contains("vtable_slot"), "{pretty}");

        let mut producers: std::collections::HashMap<usize, &'static str> = std::collections::HashMap::new();
        for function in &ir.functions {
            for block in &function.blocks {
                for instruction in &block.instructions {
                    let produced = match &instruction.kind {
                        crate::ir::InstructionKind::ConstInt { result, .. }
                        | crate::ir::InstructionKind::ConstIntTyped { result, .. } => {
                            Some((result.id, "const"))
                        }
                        crate::ir::InstructionKind::LoadVtableSlot { result, .. } => {
                            Some((result.id, "vtable_slot"))
                        }
                        crate::ir::InstructionKind::FuncAddr { result, .. } => {
                            Some((result.id, "func_addr"))
                        }
                        _ => None,
                    };
                    if let Some((id, opcode)) = produced {
                        producers.insert(id, opcode);
                    }
                }
            }
        }
        for function in &ir.functions {
            if !(function.name.ends_with("__poll") || function.name.ends_with("__drop")) {
                continue;
            }
            for block in &function.blocks {
                for instruction in &block.instructions {
                    match &instruction.kind {
                        crate::ir::InstructionKind::GetElementPtr { index, .. } => {
                            assert_eq!(
                                producers.get(&index.id),
                                Some(&"const"),
                                "vtable index %v{} is not an integer constant in {}",
                                index.id,
                                function.name,
                            );
                        }
                        crate::ir::InstructionKind::CallIndirect { fn_ptr, .. } => {
                            let producer = producers.get(&fn_ptr.id).copied().unwrap_or("undefined");
                            assert!(
                                producer == "vtable_slot" || producer == "func_addr",
                                "indirect callee %v{} comes from {producer}, not a vtable slot, in {}",
                                fn_ptr.id,
                                function.name,
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    #[test]
    fn array_ir_type_round_trips_to_array_annotation() {
        // Generic inference records annotations via `ir_type_to_annotation`;
        // an `Array<int>` actual must stay an array so the callee
        // specializes on the array instead of the bare element type.
        let lowering = ASTLowering::new();
        let annotation = lowering.ir_type_to_annotation(&crate::ir::Type::Array {
            element_type: Box::new(crate::ir::Type::Int),
            size: 0,
        });
        match &annotation.kind {
            spectra_compiler::ast::TypeAnnotationKind::Generic { name, type_args } => {
                assert_eq!(name, "array");
                assert_eq!(type_args.len(), 1);
            }
            other => panic!("array IR type must round-trip to an array annotation, got {other:?}"),
        }
    }
    #[test]
    fn vtable_order_and_signatures_agree_for_every_trait() {
        // `lower_dyn_method_call` resolves the signature before the slot and
        // never guesses slot 0. That is sound only while every ordered
        // method has a signature and vice versa; lock the agreement here,
        // including through trait inheritance.
        let source = r#"
            module vtable_agreement
            trait Printable {
                func render(&self) returns int
            }
            trait Debug: Printable {
                func debug(&self) returns int
            }
            record Point {
                x: int,
                y: int,
            }
            impl Debug for Point {
                func render(&self) returns int {
                    return self.x + self.y
                }
                func debug(&self) returns int {
                    return self.x * self.y
                }
            }
            public func main() returns int {
                let p = Point { x: 10, y: 20 }
                let d = p as dyn Debug
                return d.debug() + d.render()
            }
            "#;
        let tokens = Lexer::new(source).tokenize().expect("lexing should pass");
        let mut module = Parser::new(tokens)
            .parse()
            .expect("parsing should pass");
        analyze_modules(&mut [&mut module]).expect("semantic analysis should pass");
        let mut lowering = ASTLowering::new();
        lowering
            .lower_module(&module)
            .expect("lowering should pass");
        for (trait_name, order) in &lowering.trait_method_order {
            let signatures = lowering
                .trait_method_signatures
                .get(trait_name)
                .unwrap_or_else(|| panic!("trait '{trait_name}' has order but no signatures"));
            for method in order {
                assert!(
                    signatures.contains_key(method),
                    "trait '{trait_name}' orders method '{method}' without a signature"
                );
            }
        }
        for (trait_name, signatures) in &lowering.trait_method_signatures {
            let order = lowering.trait_method_order.get(trait_name).unwrap_or_else(|| {
                panic!("trait '{trait_name}' has signatures but no order")
            });
            for method in signatures.keys() {
                assert!(
                    order.iter().any(|ordered| ordered == method),
                    "trait '{trait_name}' signs method '{method}' without an order slot"
                );
            }
        }
    }
    #[test]
    fn mixed_layout_field_offsets_cover_every_field() {
        // Mixed int/char/bool/float/string fields have distinct padded
        // offsets (char is 4 bytes, bool is 1); every construction and field
        // read must resolve a real layout offset instead of guessing.
        let ir = lower_source(
            r#"
            module layout_offsets
            record Mixed {
                size: int,
                code: char,
                flag: bool,
                score: float,
                name: string,
            }
            func make() returns Mixed {
                Mixed { size: 1, code: 'a', flag: true, score: 2.5, name: "x" }
            }
            public func main() returns int {
                let m = make()
                if m.size != 1 { return 1 }
                if m.code != 'a' { return 2 }
                if make().score != 2.5 { return 3 }
                return 0
            }
            "#,
        );
        let pretty = crate::ir::pretty::format_module(&ir);
        // Five constructed fields plus three reads: construction, the
        // identifier field path, and the general expression field path.
        assert!(pretty.matches("field_ptr").count() >= 8);
    }
}
