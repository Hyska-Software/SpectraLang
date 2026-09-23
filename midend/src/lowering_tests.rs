use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use spectra_compiler::{analyze_modules, Lexer, Parser};

    fn lower_source(source: &str) -> IRModule {
        let tokens = Lexer::new(source).tokenize().expect("lexing should pass");
        let mut module = Parser::new(tokens).parse().expect("parsing should pass");
        analyze_modules(&mut [&mut module]).expect("semantic analysis should pass");
        ASTLowering::new()
            .lower_module(&module)
            .expect("lowering should pass")
    }

    #[test]
    fn generated_lambda_names_include_module_identity() {
        let first = lower_source(
            r#"
            module alpha_lambda

            from std.collections import List
            import std.collections as collections

            func apply() returns int {
                let values: List<int> = collections.list_new()
                collections.list_push(values, 1)
                let mapped = collections.list_map(values, |value: int| value + 1)
                return collections.list_len(mapped)
            }
            "#,
        );
        let second = lower_source(
            r#"
            module beta_lambda

            from std.collections import List
            import std.collections as collections

            func apply() returns int {
                let values: List<int> = collections.list_new()
                collections.list_push(values, 1)
                let mapped = collections.list_map(values, |value: int| value + 1)
                return collections.list_len(mapped)
            }
            "#,
        );

        let first_text = crate::ir::pretty::format_module(&first);
        let second_text = crate::ir::pretty::format_module(&second);
        assert!(first_text.contains("__lambda_alpha_lambda_0"), "{first_text}");
        assert!(second_text.contains("__lambda_beta_lambda_0"), "{second_text}");
        assert!(!first_text.contains("__lambda_beta_lambda_0"));
        assert!(!second_text.contains("__lambda_alpha_lambda_0"));
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
        assert!(
            pretty.matches("coroutine.complete").count() >= 2,
            "{pretty}"
        );
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
    fn range_for_loop_lowers_to_direct_induction_loop_without_iterator_hosts() {
        let ir = lower_source(
            r#"
            module range_induction_loop

            func total() returns int {
                let sum = 0
                for value in 1 ..= 4 {
                    sum = sum + value
                }
                return sum
            }
            "#,
        );

        let pretty = crate::ir::pretty::format_module(&ir);
        assert!(!pretty.contains("spectra.std.range.iter"), "{pretty}");
        assert!(!pretty.contains("spectra.std.collections.iterator_remaining"), "{pretty}");
        assert!(!pretty.contains("spectra.std.collections.iterator_next"), "{pretty}");
        assert!(pretty.contains("range.cond"), "{pretty}");
        assert!(pretty.contains("range.body"), "{pretty}");
        assert!(pretty.contains("range.latch"), "{pretty}");
        assert!(pretty.contains("range.exit"), "{pretty}");
    }

    #[test]
    fn range_binding_uses_direct_loop_until_reassignment() {
        let ir = lower_source(
            r#"
            module range_binding_loop

            func total() returns int {
                let range = 1 ..= 3
                let sum = 0
                for value in range {
                    sum = sum + value
                }
                return sum
            }

            func reassigned() returns int {
                let range = 1 ..= 3
                range = 5 ..= 6
                let sum = 0
                for value in range {
                    sum = sum + value
                }
                return sum
            }
            "#,
        );

        let pretty = crate::ir::pretty::format_module(&ir);
        let total = pretty
            .split("fn total()")
            .nth(1)
            .and_then(|body| body.split("fn reassigned()").next())
            .expect("total function should be present");
        assert!(!total.contains("spectra.std.range.iter"), "{total}");
        assert!(!total.contains("spectra.std.collections.iterator_remaining"), "{total}");

        let reassigned = pretty
            .split("fn reassigned()")
            .nth(1)
            .expect("reassigned function should be present");
        assert!(reassigned.contains("spectra.std.range.iter"), "{reassigned}");
    }

    #[test]
    fn collection_for_loop_caches_snapshot_length_in_a_local_counter() {
        let ir = lower_source(
            r#"
            module collection_snapshot_counter

            import std.collections as collections

            func total() returns int {
                let values = collections.list_new()
                collections.list_push(values, 2)
                collections.list_push(values, 3)
                let sum = 0
                for value in values {
                    sum = sum + value
                }
                collections.list_free(values)
                return sum
            }
            "#,
        );

        let pretty = crate::ir::pretty::format_module(&ir);
        let remaining = pretty
            .find("spectra.std.collections.iterator_remaining")
            .expect("collection loop should initialize its snapshot counter");
        let header = pretty
            .find("iterator.header:")
            .expect("collection loop should have an iterator header");
        assert!(remaining < header, "{pretty}");
        assert!(pretty.contains("iterator.latch"), "{pretty}");
        assert!(pretty.contains("spectra.std.collections.iterator_next_unchecked"), "{pretty}");
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
        let mut module = Parser::new(tokens).parse().expect("parsing should pass");
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
    fn unsized_array_parameter_indexing_checks_the_hidden_length() {
        // The definition appends one hidden `Int` length parameter after the
        // public ones, indexing the parameter gets a dynamic bound naming that
        // value, and the call site appends the length it knows. Before this,
        // an out-of-bounds index through a `[T]` parameter corrupted memory
        // silently because the static bound was 0 ("unknown").
        let ir = lower_source(
            r#"
            module parameter_bounds

            func read_at(values: [int], index: int) returns int {
                return values[index]
            }

            func main() returns int {
                return read_at([10, 20, 30], 1)
            }
            "#,
        );

        let read_at = ir
            .functions
            .iter()
            .find(|function| function.name == "read_at")
            .expect("read_at must be lowered");
        assert_eq!(
            read_at.params.len(),
            3,
            "public parameters plus one hidden length"
        );
        assert_eq!(read_at.params[2].name, "__len_values");

        let main = ir
            .functions
            .iter()
            .find(|function| function.name == "main")
            .expect("main must be lowered");
        let call_args = main
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find_map(|instruction| match &instruction.kind {
                crate::ir::InstructionKind::Call { args, .. } => Some(args.clone()),
                _ => None,
            })
            .expect("main must call read_at");
        assert_eq!(
            call_args.len(),
            3,
            "the call appends the array literal length"
        );

        let pretty = crate::ir::pretty::format_module(&ir);
        assert!(pretty.contains("bound dynamic"), "{pretty}");
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

        let mut producers: std::collections::HashMap<usize, &'static str> =
            std::collections::HashMap::new();
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
                            let producer =
                                producers.get(&fn_ptr.id).copied().unwrap_or("undefined");
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
        let mut module = Parser::new(tokens).parse().expect("parsing should pass");
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
            let order = lowering
                .trait_method_order
                .get(trait_name)
                .unwrap_or_else(|| panic!("trait '{trait_name}' has signatures but no order"));
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

    /// Qualified and aliased print calls (`io.println(x)`, `std.io.println(x)`)
    /// parse as method calls, so they lower through the method-call path. Both
    /// lowering paths must satisfy the runtime contract: one (type_tag, value)
    /// pair per printed value, with the tag as an integer constant.
    #[test]
    fn qualified_io_print_pairs_type_tags_with_arguments() {
        let ir = lower_source(
            r#"
            module qualified_io_print_test

            import std.io as io
            from std.io import println

            func emit()  returns  int {
                io.println("text")
                io.println(7)
                io.println(true)
                std.io.println(2.5)
                println("direct")
                return 0
            }
            "#,
        );

        let mut tags: Vec<i64> = Vec::new();
        for function in &ir.functions {
            if function.name != "emit" {
                continue;
            }
            let mut constants: std::collections::HashMap<usize, i64> =
                std::collections::HashMap::new();
            for block in &function.blocks {
                for instruction in &block.instructions {
                    if let crate::ir::InstructionKind::ConstInt { result, value } =
                        &instruction.kind
                    {
                        constants.insert(result.id, *value);
                    }
                }
            }
            for block in &function.blocks {
                for instruction in &block.instructions {
                    let crate::ir::InstructionKind::HostCall { host, args, .. } = &instruction.kind
                    else {
                        continue;
                    };
                    if host != "spectra.std.io.println" {
                        continue;
                    }
                    assert_eq!(
                        args.len(),
                        2,
                        "print arguments must be (type_tag, value) pairs, got {args:?}",
                    );
                    let tag = constants
                        .get(&args[0].id)
                        .copied()
                        .expect("print type tag must be an integer constant");
                    tags.push(tag);
                }
            }
        }
        assert_eq!(
            tags,
            vec![1, 0, 2, 3, 1],
            "expected string/int/bool/float/string tags",
        );
    }

    #[test]
    fn scalar_map_calls_use_allocation_table_free_fast_variants() {
        let ir = lower_source(
            r#"
            module scalar_map_fast_variants

            from std.collections import Map
            import std.collections as collections
            import std.option as option

            public func main() returns int {
                let numbers: Map<int, int> = collections.map_new()
                collections.map_set(numbers, 1, 10)
                let present = collections.map_contains(numbers, 1)
                let value = collections.map_get(numbers, 1)
                let removed = collections.map_remove(numbers, 1)
                if present and option.is_some(value) and option.is_some(removed) {
                    return 0
                }
                return 1
            }
            "#,
        );

        let pretty = crate::ir::pretty::format_module(&ir);
        assert!(pretty.contains("spectra.compiler.collections.map_set_scalar"));
        assert!(pretty.contains("spectra.compiler.collections.map_contains_scalar"));
        assert!(pretty.contains("spectra.compiler.collections.map_get_scalar"));
        assert!(pretty.contains("spectra.compiler.collections.map_remove_scalar"));
        assert!(!pretty.contains("spectra.std.collections.map_set"));
        assert!(!pretty.contains("spectra.std.collections.map_contains"));

        let string_ir = lower_source(
            r#"
            module string_map_keeps_general_path

            from std.collections import Map
            import std.collections as collections

            public func main() returns int {
                let labels: Map<string, int> = collections.map_new()
                collections.map_set(labels, "key", 7)
                return 0
            }
            "#,
        );
        let string_pretty = crate::ir::pretty::format_module(&string_ir);
        assert!(string_pretty.contains("spectra.std.collections.map_set"));
        assert!(!string_pretty.contains("spectra.compiler.collections.map_set_scalar"));
    }

    /// Exact-width unsigned operands must select the unsigned machine
    /// signedness on all six signedness-sensitive ops (`Lt`/`Le`/`Gt`/`Ge`/
    /// `Div`/`Rem`), while signed exact ints and plain `int` keep the signed
    /// form. Equality is bit-wise and carries no flag.
    #[test]
    fn unsigned_exact_int_operands_set_the_unsigned_flag() {
        let ir = lower_source(
            r#"
            module unsigned_flag_shapes

            public func main() returns int {
                let hi: u8 = 200
                let lo: u8 = 10
                let dividend: u8 = 200
                let divisor: u8 = 2
                if not (hi > lo) { return 1 }
                if not (hi >= lo) { return 2 }
                if lo < hi { return 3 }
                if lo <= hi { return 4 }
                if (dividend / divisor) as int != 100 { return 5 }
                if (dividend % divisor) as int != 0 { return 6 }
                return 0
            }
            "#,
        );

        let mut relational_ops = 0;
        for function in &ir.functions {
            for block in &function.blocks {
                for instruction in &block.instructions {
                    match &instruction.kind {
                        crate::ir::InstructionKind::Lt { unsigned, .. }
                        | crate::ir::InstructionKind::Le { unsigned, .. }
                        | crate::ir::InstructionKind::Gt { unsigned, .. }
                        | crate::ir::InstructionKind::Ge { unsigned, .. }
                        | crate::ir::InstructionKind::Div { unsigned, .. }
                        | crate::ir::InstructionKind::Rem { unsigned, .. } => {
                            relational_ops += 1;
                            assert!(
                                *unsigned,
                                "unsigned exact-int operand in {} must lower with unsigned=true",
                                function.name
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(
            relational_ops >= 6,
            "expected at least six signedness-sensitive ops, got {relational_ops}"
        );
    }

    /// Signed exact ints and plain `int` must keep the signed flag so the
    /// backend emits `sdiv`/`srem` and signed `icmp` conditions for them.
    #[test]
    fn signed_operands_keep_the_signed_flag() {
        let ir = lower_source(
            r#"
            module signed_flag_shapes

            public func main() returns int {
                let wide: i8 = -5
                let other: i8 = 3
                let plain_a = 10
                let plain_b = 3
                if not (wide < other) { return 1 }
                if (wide / other) as int != -1 { return 2 }
                if plain_a / plain_b != 3 { return 3 }
                if plain_a % plain_b != 1 { return 4 }
                return 0
            }
            "#,
        );

        let mut signedness_ops = 0;
        for function in &ir.functions {
            for block in &function.blocks {
                for instruction in &block.instructions {
                    match &instruction.kind {
                        crate::ir::InstructionKind::Lt { unsigned, .. }
                        | crate::ir::InstructionKind::Div { unsigned, .. }
                        | crate::ir::InstructionKind::Rem { unsigned, .. } => {
                            signedness_ops += 1;
                            assert!(
                                !*unsigned,
                                "signed operand in {} must lower with unsigned=false",
                                function.name
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(signedness_ops >= 4, "got {signedness_ops} ops");
    }

    /// Mixed-width exact ints (`u8 < u16`, `u8 + u16`) used to reach the
    /// backend as mismatched Cranelift types and crashed the verifier.
    /// Lowering must coerce both operands to the unification type (signed
    /// 64-bit, mirroring semantic `numeric_result_type`) with a `Cast` per
    /// operand, and the relational flag still comes from the original
    /// unsigned operand types.
    #[test]
    fn mixed_width_exact_int_operands_are_coerced_to_a_common_type() {
        let ir = lower_source(
            r#"
            module mixed_width_coercion

            public func main() returns int {
                let small: u8 = 200
                let wide: u16 = 300
                if not (small < wide) { return 1 }
                let sum = small + wide
                if sum as int != 500 { return 2 }
                return 0
            }
            "#,
        );

        let main = ir
            .functions
            .iter()
            .find(|function| function.name == "main")
            .expect("main must be lowered");

        let unify_ty = crate::ir::Type::ExactInt {
            signed: true,
            width: crate::ir::IntWidth::I64,
        };

        let mut casts_to_unify = 0;
        let mut mixed_lt: Option<(crate::ir::Value, crate::ir::Value)> = None;
        for block in &main.blocks {
            for instruction in &block.instructions {
                match &instruction.kind {
                    crate::ir::InstructionKind::Cast { to_ty, .. } if *to_ty == unify_ty => {
                        casts_to_unify += 1;
                    }
                    crate::ir::InstructionKind::Lt {
                        lhs, rhs, unsigned, ..
                    } => {
                        assert!(*unsigned, "u8 vs u16 must stay unsigned after coercion");
                        mixed_lt = Some((*lhs, *rhs));
                    }
                    _ => {}
                }
            }
        }
        assert!(
            casts_to_unify >= 3,
            "both comparison operands and the add operands must be cast to the common type, \
             got {casts_to_unify} casts"
        );

        let (lt_lhs, lt_rhs) = mixed_lt.expect("the mixed comparison must be lowered");
        let is_cast_result = |value: crate::ir::Value| {
            main.blocks
                .iter()
                .flat_map(|block| block.instructions.iter())
                .any(|instruction| {
                    matches!(
                        &instruction.kind,
                        crate::ir::InstructionKind::Cast { result, .. } if result.id == value.id
                    )
                })
        };
        assert!(
            is_cast_result(lt_lhs),
            "the lhs of the mixed comparison must be a cast"
        );
        assert!(
            is_cast_result(lt_rhs),
            "the rhs of the mixed comparison must be a cast"
        );
    }

    /// The zero operand of a unary negation must be typed exactly like the
    /// operand's *lowered* form: number literals take their type from
    /// `current_expected_annotation` (not the semantic span fact).
    /// `let x: i8 = -5` therefore needs an i8 zero, while `(-120) as i8`
    /// (cast outside the annotation's reach) lowers the literal untyped and
    /// needs the historical i64 zero. Both used to mismatch the other way
    /// and crash the Cranelift verifier.
    #[test]
    fn unary_negate_zero_matches_the_literals_lowered_type() {
        // Shape 1: annotation active over the literal -> typed i8 pair.
        let ir = lower_source(
            r#"
            module negate_typed_literal
            public func main() returns int {
                let x: i8 = -5
                return x as int
            }
            "#,
        );
        let mut saw_typed_sub = false;
        for function in &ir.functions {
            for block in &function.blocks {
                let instructions = &block.instructions;
                for (index, instruction) in instructions.iter().enumerate() {
                    if !matches!(
                        &instruction.kind,
                        crate::ir::InstructionKind::Sub { .. }
                    ) {
                        continue;
                    }
                    // Find the preceding constants feeding this Sub.
                    let (lhs_id, rhs_id) = match &instruction.kind {
                        crate::ir::InstructionKind::Sub { lhs, rhs, .. } => (lhs.id, rhs.id),
                        _ => unreachable!(),
                    };
                    let producer = |id: usize| {
                        instructions[..index].iter().rev().find_map(|instr| match &instr.kind {
                            crate::ir::InstructionKind::ConstIntTyped { result, ty, .. }
                                if result.id == id =>
                            {
                                Some(Some(ty.clone()))
                            }
                            crate::ir::InstructionKind::ConstInt { result, .. }
                                if result.id == id =>
                            {
                                Some(None)
                            }
                            _ => None,
                        })
                    };
                    if let (Some(Some(lhs_ty)), Some(Some(rhs_ty))) =
                        (producer(lhs_id), producer(rhs_id))
                    {
                        assert_eq!(lhs_ty, rhs_ty, "both negate operands must be typed alike");
                        assert!(
                            matches!(lhs_ty, IRType::ExactInt { .. }),
                            "annotated literal negation must use a typed zero, got {lhs_ty:?}"
                        );
                        saw_typed_sub = true;
                    }
                }
            }
        }
        assert!(
            saw_typed_sub,
            "the annotated `-5` must lower as a typed-constant Sub"
        );

        // Shape 2: no annotation reaches the literal when it sits behind a
        // cast feeding a call argument (`sink((-120) as i8)`), so both
        // negate operands are the historical untyped i64 constants. (A
        // fully-constant `let` chain would be const-evaluated instead, which
        // is why this shape goes through an opaque call.)
        let ir = lower_source(
            r#"
            module negate_cast_literal
            func sink(value: i8) returns int {
                return value as int
            }
            public func main() returns int {
                return sink(-120 as i8)
            }
            "#,
        );
        let mut saw_untyped_sub = false;
        for function in &ir.functions {
            for block in &function.blocks {
                let instructions = &block.instructions;
                for (index, instruction) in instructions.iter().enumerate() {
                    let crate::ir::InstructionKind::Sub { lhs, rhs, .. } = &instruction.kind
                    else {
                        continue;
                    };
                    let producer = |id: usize| {
                        instructions[..index].iter().rev().any(|instr| {
                            matches!(
                                &instr.kind,
                                crate::ir::InstructionKind::ConstInt { result, .. } if result.id == id
                            )
                        })
                    };
                    if producer(lhs.id) && producer(rhs.id) {
                        saw_untyped_sub = true;
                    }
                }
            }
        }
        assert!(
            saw_untyped_sub,
            "an unannotated literal negation must lower as an untyped i64 Sub, got:\n{}",
            crate::ir::pretty::format_module(&ir)
        );
    }

    /// The per-category lowering helpers used to `unreachable!`-panic on an
    /// out-of-category AST node. They must record a `MidendError` (through
    /// `ASTLowering::error`/`invalid_value`) and hand back the internal
    /// poison value instead, so a compiler ICE is impossible from this path.
    #[test]
    fn out_of_category_expression_records_an_error_instead_of_panicking() {
        use spectra_compiler::ast::{BinaryOperator, Expression, ExpressionKind};

        let identifier = |name: &str| Expression {
            span: spectra_compiler::Span::new(
                0,
                1,
                spectra_compiler::Location::new(1, 1),
                spectra_compiler::Location::new(1, 2),
            ),
            kind: ExpressionKind::Identifier(name.to_string()),
        };
        let binary = Expression {
            span: spectra_compiler::Span::new(
                0,
                3,
                spectra_compiler::Location::new(1, 1),
                spectra_compiler::Location::new(1, 4),
            ),
            kind: ExpressionKind::Binary {
                left: Box::new(identifier("a")),
                operator: BinaryOperator::Add,
                right: Box::new(identifier("b")),
            },
        };

        let mut lowering = ASTLowering::new();
        let mut function = IRFunction::new("probe", Vec::new(), IRType::Int);
        let value = lowering.lower_expression_literals(&binary, &mut function);

        assert_eq!(
            value.id,
            crate::ir::Value::INVALID_ID,
            "an out-of-category node must produce the internal poison value"
        );
        assert!(
            lowering
                .errors
                .iter()
                .any(|error| error.message.contains("non-literal expression")),
            "a MidendError must be recorded instead of panicking: {:?}",
            lowering.errors
        );
    }
}
