use spectra_compiler::{
    analyze_modules, CompilationOptions, CompilationPipeline, CompilerError, Lexer, Parser,
};

fn parse_module(source: &str) -> spectra_compiler::Module {
    let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");
    Parser::new(tokens).parse().expect("parser should succeed")
}

#[test]
fn frontend_and_semantic_accepts_valid_program() {
    let source = r#"
        module smoke

        import std.io

        func add(lhs: int, rhs: int)  returns  int {
            return lhs + rhs
        }

        public func main()  returns  int {
            let total = add(20, 22)
            println(total)
            return total
        }
    "#;

    let mut module = parse_module(source);
    let mut modules = vec![&mut module];

    let result = analyze_modules(modules.as_mut_slice());
    assert!(
        result.is_ok(),
        "semantic analysis should succeed: {result:?}"
    );
}

#[test]
fn type_aliases_are_available_to_signatures_and_bodies() {
    let source = r#"
        module aliases

        type Pair = (int, string)
        const BASE: int = 40 + 2

        func make_pair() returns Pair {
            (BASE, "spectra")
        }

        public func main() returns int {
            let pair: Pair = make_pair()
            if pair.0 != 42 {
                return 1
            }
            if pair.1 != "spectra" {
                return 2
            }
            return 0
        }
    "#;

    let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
    pipeline
        .compile(source, "type_aliases.spectra")
        .expect("type aliases should resolve before function analysis and lowering");
}

#[test]
fn pipeline_reports_coded_semantic_error() {
    let source = r#"
        module smoke

        public func main()  returns  int {
            return missing_symbol
        }
    "#;

    let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
    let errors = pipeline
        .compile(source, "missing_symbol.spectra")
        .expect_err("compilation should fail");

    assert!(errors.iter().any(|error| matches!(
        error,
        CompilerError::Semantic(semantic) if semantic.code.as_deref() == Some("E001")
    )));
}

#[test]
fn trait_bound_violation_is_semantic_not_midend() {
    let source = r#"
        module smoke

        trait Score {
            func score(&self)  returns  int
        }

        record Plain {
            value: int,
        }

        func evaluate<T: Score>(item: T)  returns  int {
            return item.score()
        }

        public func main()  returns  int {
            let plain = Plain { value: 1 }
            return evaluate(plain)
        }
    "#;

    let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
    let errors = pipeline
        .compile(source, "trait_bound.spectra")
        .expect_err("compilation should fail before midend");

    assert!(errors
        .iter()
        .all(|error| !matches!(error, CompilerError::Midend(_))));
    assert_eq!(
        errors.len(),
        1,
        "expected no cascading diagnostics: {errors:?}"
    );
    assert!(matches!(
        &errors[0],
        CompilerError::Semantic(semantic)
            if semantic.code.as_deref() == Some("E010")
                && semantic.message.contains("Plain")
                && semantic.message.contains("T: Score")
    ));
}

#[test]
fn trait_bound_satisfaction_is_not_item_order_dependent() {
    let source = r#"
        module smoke

        trait Score {
            func score(&self)  returns  int
        }

        record Ranked {
            value: int,
        }

        func evaluate<T: Score>(item: T)  returns  int {
            return item.score()
        }

        public func main()  returns  int {
            let ranked = Ranked { value: 7 }
            return evaluate(ranked)
        }

        impl Score for Ranked {
            func score(&self)  returns  int {
                return self.value
            }
        }
    "#;

    let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
    pipeline
        .compile(source, "trait_bound_order.spectra")
        .expect("trait impl declared later should satisfy the generic bound");
}

#[test]
fn unknown_import_alias_member_reports_candidates() {
    let source = r#"
        module smoke

        import std.math as math

        public func main()  returns  int {
            return math.not_a_function(1)
        }
    "#;

    let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
    let errors = pipeline
        .compile(source, "unknown_alias_member.spectra")
        .expect_err("compilation should fail semantically");

    assert!(errors
        .iter()
        .all(|error| !matches!(error, CompilerError::Midend(_))));
    assert_eq!(
        errors.len(),
        1,
        "expected no cascading diagnostics: {errors:?}"
    );
    assert!(matches!(
        &errors[0],
        CompilerError::Semantic(semantic)
            if semantic.code.as_deref() == Some("E011")
                && semantic.message.contains("math")
                && semantic.message.contains("not_a_function")
                && semantic.hint.as_deref().unwrap_or("").contains("sqrt_f")
    ));
}

#[test]
fn std_api_surface_resolves_qualified_and_aliased_calls() {
    let source = r#"
        module api_surface

        import std.api.http as http
        import std.api.json as json
        import std.api.tls as tls

        public func main()  returns  int {
            let request = http.request_new(1)
            let method = http.request_method(request)
            let method_name = std.api.http.method_name(method)
            let ok = json.validate("{\"ok\": true}")
            let tls_config = tls.client_config()
            let tls_mode = tls.config_mode(tls_config)
            let status_class = std.api.http.status_class(200)
            return status_class + tls_mode
        }
    "#;

    let mut module = parse_module(source);
    let mut modules = vec![&mut module];

    let result = analyze_modules(modules.as_mut_slice());
    assert!(
        result.is_ok(),
        "std.api semantic surface should resolve without missing-module diagnostics: {result:?}"
    );
}

#[test]
fn generic_return_type_parameter_matches_declared_type_parameter() {
    let source = r#"
        module smoke

        func identity<T>(value: T)  returns  T {
            return value
        }

        public func main()  returns  int {
            return identity(42)
        }
    "#;

    let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
    pipeline
        .compile(source, "generic_identity.spectra")
        .expect("generic function returning its declared type parameter should compile");
}

#[test]
fn generic_return_type_parameter_cannot_satisfy_concrete_return() {
    let source = r#"
        module smoke

        func bad<T>(value: T)  returns  string {
            return value
        }

        public func main()  returns  int {
            let x = bad(1)
            return 0
        }
    "#;

    let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
    let errors = pipeline
        .compile(source, "generic_bad_return.spectra")
        .expect_err("generic return mismatch should fail in semantic analysis");

    assert!(errors
        .iter()
        .all(|error| !matches!(error, CompilerError::Backend(_) | CompilerError::Midend(_))));
    assert_eq!(
        errors.len(),
        1,
        "expected no backend or cascading diagnostics: {errors:?}"
    );
    assert!(matches!(
        &errors[0],
        CompilerError::Semantic(semantic)
            if semantic.code.as_deref() == Some("E004")
                && semantic.message.contains("expected string")
                && semantic.message.contains("found T")
    ));
}

#[test]
fn json_derived_static_error_field_keeps_string_type_without_annotation() {
    let source = r#"
        module json_type_flow

        #[derive(Serialize, Deserialize)]
        record Profile {
            id: int,
            name: string,
        }

        public func main() returns int {
            let field = Profile::json_error_field("{\"id\":7,\"name\":\"Ada\"}")
            if field != "" {
                return 1
            }
            return 0
        }
    "#;

    let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
    pipeline
        .compile(source, "json_derived_static_error_field.spectra")
        .expect("derived JSON static method should infer string through the full pipeline");
}

#[test]
fn analyze_modules_rejects_mutual_import_cycle_with_e028() {
    let source_a = r#"
        module cycle_a

        import cycle_b

        public func from_a() returns int {
            return 1
        }
    "#;
    let source_b = r#"
        module cycle_b

        import cycle_a

        public func from_b() returns int {
            return 2
        }
    "#;

    let mut module_a = parse_module(source_a);
    let mut module_b = parse_module(source_b);
    let mut modules = vec![&mut module_a, &mut module_b];

    let errors = analyze_modules(modules.as_mut_slice())
        .expect_err("mutual imports must be rejected as a circular import");
    assert_eq!(
        errors.len(),
        1,
        "expected a single non-cascading circular-import diagnostic: {errors:?}"
    );
    assert!(
        matches!(
            &errors[0],
            error
                if error.code.as_deref() == Some("E028")
                    && error.message.contains("cycle_a -> cycle_b -> cycle_a")
        ),
        "expected coded E028 listing the full cycle: {errors:?}"
    );
}

#[test]
fn analyze_modules_reports_self_import_as_circular_without_processing() {
    let source = r#"
        module loopback

        import loopback

        public func main() returns int {
            return 0
        }
    "#;

    let mut module = parse_module(source);
    let mut modules = vec![&mut module];

    let errors = analyze_modules(modules.as_mut_slice())
        .expect_err("a self-import must be rejected as a circular import");
    assert!(
        matches!(
            &errors[0],
            error
                if error.code.as_deref() == Some("E028")
                    && error.message.contains("loopback -> loopback")
        ),
        "expected coded E028 for the self-import: {errors:?}"
    );
}
