#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{format_source, FormatterConfig};

    #[test]
    fn formats_basic_block_structure() {
        let input =
            "module demo\n\nfunc main(){\nlet value=1\nif value>0 {\nprintln(value)\n}\n}\n";
        let expected =
            "module demo\n\nfunc main() {\n    let value = 1\n    if value > 0 {\n        println(value)\n    }\n}\n";
        assert_eq!(format_source(input, &FormatterConfig::default()), expected);
    }

    #[test]
    fn preserves_else_alignment() {
        let input = "func check(){\nif cond {\nprintf(\"yes\")\n}else{\nprintf(\"no\")\n}\n}\n";
        let expected =
            "func check() {\n    if cond {\n        printf(\"yes\")\n    } else {\n        printf(\"no\")\n    }\n}\n";
        assert_eq!(format_source(input, &FormatterConfig::default()), expected);
    }

    #[test]
    fn inserts_spaces_around_binary_operators() {
        let input = "func math(){\nlet sum=left+right*factor-3\nlet cmp=a==b||a!=c&&d>=e\n}\n";
        let expected =
            "func math() {\n    let sum = left + right * factor - 3\n    let cmp = a == b || a != c && d >= e\n}\n";
        assert_eq!(format_source(input, &FormatterConfig::default()), expected);
    }

    #[test]
    fn aligns_consecutive_let_bindings() {
        let input =
            "func demo(){\nlet short=1\nlet much_longer_name=2\nlet mid=short+much_longer_name\n}\n";
        let expected =
            "func demo() {\n    let short            = 1\n    let much_longer_name = 2\n    let mid              = short + much_longer_name\n}\n";
        assert_eq!(format_source(input, &FormatterConfig::default()), expected);
    }

    #[test]
    fn respects_custom_indent_width() {
        let config = FormatterConfig {
            indent_width: 2,
            ..FormatterConfig::default()
        };
        let input = "func main(){\nif cond {\nprintln(\"hi\")\n}\n}\n";
        let expected = "func main() {\n  if cond {\n    println(\"hi\")\n  }\n}\n";
        assert_eq!(format_source(input, &config), expected);
    }

    #[test]
    fn skips_alignment_when_line_would_exceed_limit() {
        let config = FormatterConfig {
            max_line_length: 40,
            ..FormatterConfig::default()
        };
        let input =
            "func wide(){\nlet short=call()\nlet very_very_long_identifier=call_with_many_arguments()\n}\n";
        let expected =
            "func wide() {\n    let short = call()\n    let very_very_long_identifier = call_with_many_arguments()\n}\n";
        assert_eq!(format_source(input, &config), expected);
    }

    #[test]
    fn keeps_else_if_spacing() {
        let input =
            "func flag(){\nif ready {\nreturn\n}else if pending {\nreturn\n}else{\nreturn\n}\n}\n";
        let expected =
            "func flag() {\n    if ready {\n        return\n    } else if pending {\n        return\n    } else {\n        return\n    }\n}\n";
        assert_eq!(format_source(input, &FormatterConfig::default()), expected);
    }

    #[test]
    fn keeps_canonical_generics_paths_and_numeric_unary_operators_compact() {
        let input = "module demo\n\nfunc eval(value: Option < int >) returns int {\nif value == Option:: Some {\nreturn -1\n}\nreturn 0\n}\n";
        let expected = "module demo\n\nfunc eval(value: Option<int>) returns int {\n    if value == Option::Some {\n        return -1\n    }\n    return 0\n}\n";
        assert_eq!(format_source(input, &FormatterConfig::default()), expected);
        assert!(super::ends_with_expression_keyword("when 1 then"));
        assert_eq!(super::normalize_spacing("when 1 then - 1"), "when 1 then -1");
    }

    #[test]
    fn preserves_double_colon_compact() {
        let input = "func main(){\nlet value=Namespace::member()\nreturn value\n}\n";
        let expected = "func main() {\n    let value = Namespace::member()\n    return value\n}\n";
        assert_eq!(format_source(input, &FormatterConfig::default()), expected);
    }

    #[test]
    fn keeps_unary_minus_tight() {
        let input =
            "func eval(flag: bool){\nif flag {\nreturn -value\n}else{\nreturn !flag\n}\n}\n";
        let expected =
            "func eval(flag: bool) {\n    if flag {\n        return -value\n    } else {\n        return !flag\n    }\n}\n";
        assert_eq!(format_source(input, &FormatterConfig::default()), expected);
    }

    #[test]
    fn formats_doc_comments_adjacent_to_function() {
        let input = "///short\nfunc demo(){\nreturn 42\n}\n";
        let expected = "/// short\nfunc demo() {\n    return 42\n}\n";
        assert_eq!(format_source(input, &FormatterConfig::default()), expected);
    }

    #[test]
    fn normalizes_simple_match_arms() {
        let input = "func classify(value){\nmatch value {\nwhen Alpha then 1\nwhen Beta then 2\nwhen Gamma then 3\n}\n}\n";
        let expected = "func classify(value) {\n    match value {\n        when Alpha then 1\n        when Beta then 2\n        when Gamma then 3\n    }\n}\n";
        assert_eq!(format_source(input, &FormatterConfig::default()), expected);
    }

    #[test]
    fn render_diff_reports_changes() {
        let original = "func demo() {\n    return 0\n}\n";
        let formatted = "func demo() {\n    return 1\n}\n";
        let diff = super::render_diff(Path::new("demo.spectra"), original, formatted);
        assert!(diff.contains("diff --spectra demo.spectra"));
        assert!(diff.contains("--- original"));
        assert!(diff.contains("+++ formatted"));
        assert!(diff.contains("-    return 0"));
        assert!(diff.contains("+    return 1"));
    }

    #[test]
    fn render_json_diff_reports_operations() {
        let original = "func demo() {\n    return 0\n}\n";
        let formatted = "func demo() {\n    return 1\n}\n";
        let file_diff = super::render_json_diff(Path::new("demo.spectra"), original, formatted);
        assert_eq!(file_diff.path, "demo.spectra");
        assert!(file_diff
            .operations
            .iter()
            .any(|op| matches!(op.op, super::JsonOpKind::Remove) && op.text.contains("return 0")));
        assert!(file_diff
            .operations
            .iter()
            .any(|op| matches!(op.op, super::JsonOpKind::Insert) && op.text.contains("return 1")));
    }

    #[test]
    fn formatter_run_stats_check_mode_reports_zero_updates() {
        let mut config_stats = super::ConfigStats::default();
        config_stats.record_miss();
        let stats = super::FormatterRunStats::new(2, 1, true, &config_stats);
        assert_eq!(stats.processed, 2);
        assert_eq!(stats.changed, 1);
        assert_eq!(stats.updated, 0);
        assert_eq!(stats.unchanged, 1);
        assert_eq!(stats.mode, super::FormatterMode::Check);
        assert_eq!(stats.config_cache_lookups, 1);
        assert_eq!(stats.config_cache_hits, 0);
        assert_eq!(stats.config_cache_misses, 1);
    }

    #[test]
    fn json_summary_reflects_run_stats() {
        let mut config_stats = super::ConfigStats::default();
        config_stats.record_hit();
        let stats = super::FormatterRunStats::new(3, 2, false, &config_stats);
        let summary = super::JsonSummary::from_stats(&stats);
        assert_eq!(summary.processed, 3);
        assert_eq!(summary.changed, 2);
        assert_eq!(summary.updated, 2);
        assert_eq!(summary.unchanged, 1);
        assert_eq!(summary.mode, super::FormatterMode::Write);
        assert_eq!(stats.config_cache_hits, 1);
        assert_eq!(summary.config_cache_lookups, 1);
        assert_eq!(summary.config_cache_hits, 1);
        assert_eq!(summary.config_cache_misses, 0);
    }


    fn assert_idempotent(input: &str, config: &FormatterConfig) -> String {
        let once = format_source(input, config);
        let twice = format_source(&once, config);
        assert_eq!(once, twice, "formatter is not idempotent");
        once
    }

    #[test]
    fn wraps_long_call_arguments_one_per_line() {
        let input = "func demo() {\n    let values = collect_measurements(alpha_measurement_value, beta_measurement_value, gamma_measurement_value)\n}\n";
        let output = assert_idempotent(input, &FormatterConfig::default());
        assert!(
            output.contains("let values = collect_measurements("),
            "head must keep the callee prefix:\n{output}"
        );
        for argument in [
            "alpha_measurement_value,", "beta_measurement_value,", "gamma_measurement_value,",
        ] {
            assert!(
                output.lines().any(|line| line.trim() == argument),
                "expected one-arg-per-line with trailing comma:\n{output}"
            );
        }
        // Closing paren back at the statement's base indentation.
        assert!(
            output.lines().any(|line| line == "    )"),
            "closing paren must sit at base indent:\n{output}"
        );
    }

    #[test]
    fn wraps_long_array_literal_one_per_line_with_trailing_comma() {
        let input = "func demo() {\n    let matrix = [first_row_of_the_largest_matrix, second_row_of_largest_matrix, third_row_of_largest]\n}\n";
        let output = assert_idempotent(input, &FormatterConfig::default());
        assert!(output.contains("let matrix = ["), "array head:\n{output}");
        let lines: Vec<&str> = output.lines().collect();
        assert!(
            lines.contains(&"        first_row_of_the_largest_matrix,")
                && lines.contains(&"        second_row_of_largest_matrix,"),
            "intermediate elements keep their commas:\n{output}"
        );
        assert!(
            lines.contains(&"        third_row_of_largest,"),
            "last element with trailing comma:\n{output}"
        );
        assert!(
            lines.contains(&"    ]"),
            "closing bracket at base indent:\n{output}"
        );
    }

    #[test]
    fn wraps_long_binary_expression_in_return() {
        let input = "func demo() -> int {\n    return left_hand_side_operand_value * right_hand_side_operand_value + adjustment_constant_offset_value\n}\n";
        let output = assert_idempotent(input, &FormatterConfig::default());
        let lines: Vec<&str> = output.lines().collect();
        assert!(
            lines.contains(&"    return left_hand_side_operand_value"),
            "return head:\n{output}"
        );
        assert!(lines.contains(&"        * right_hand_side_operand_value"));
        assert!(
            output.lines().any(|line| line.trim_start().starts_with("+ adjustment_constant_offset_value")),
            "return continuation:\n{output}"
        );
    }
    #[test]
    fn wraps_long_binary_expression_before_operators_in_let() {
        let input = "func demo() {\n    let grand_total_value = first_component_value + second_component_value + third_component_value_sum\n}\n";
        let output = assert_idempotent(input, &FormatterConfig::default());
        assert!(
            output.contains("let grand_total_value = first_component_value"),
            "head keeps lhs:\n{output}"
        );
        assert!(
            output.lines().any(|line| line.trim_start().starts_with("+ second_component_value")),
            "break before operator:\n{output}"
        );
        assert!(
            output.lines().any(|line| line.starts_with("        + third_component_value_sum")),
            "continuations sit one level deeper than the statement:\n{output}"
        );
    }

    #[test]
    fn does_not_wrap_short_lines_or_lines_without_safe_breaks() {
        let config = FormatterConfig::default();
        let short = "func demo() {\n    let values = collect(alpha, beta)\n}\n";
        assert_eq!(format_source(short, &config), short);

        // Single argument (no top-level comma): no safe break point even
        // though the rendered line exceeds the limit.
        let single = "func demo() {\n    let text = render_with_a_single_argument(one_enormously_long_argument_name_exceeding_one_hundred_chars_total)\n}\n";
        assert_eq!(format_source(single, &config), single);

        let idempotent_single = assert_idempotent(single, &config);
        assert_eq!(idempotent_single, single);
    }

    #[test]
    fn never_breaks_inside_string_literals() {
        let config = FormatterConfig::default();
        let input = "func demo() {\n    let banner = render_label(\"call(a, b) and also [x, y] stay completely glued\", trailing_argument)\n}\n";
        let output = format_source(input, &config);
        assert!(
            output.contains("\"call(a, b) and also [x, y] stay completely glued\""),
            "string contents must survive intact:\n{output}"
        );
        assert_eq!(format_source(&output, &config), output);
    }

    #[test]
    fn wrapped_output_survives_full_reformat_with_surroundings() {
        let input = concat!(
            "module demo\n\n",
            "func process(flag: bool) {\n",
            "    let values = collect_measurements(alpha_value, beta_value, gamma_value, delta_value)\n",
            "    let total = first_component_value + second_component_value + third_component_value\n",
            "    if flag {\n",
            "        return total\n",
            "    }\n",
            "    return 0\n",
            "}\n"
        );
        assert_idempotent(input, &FormatterConfig::default());
    }

    #[test]
    fn formatting_is_idempotent_over_validation_corpus() {
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/validation");
        if !corpus.is_dir() {
            eprintln!("tests/validation not found; skipping corpus idempotency check");
            return;
        }

        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&corpus)
            .expect("read validation corpus")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "spectra"))
            .collect();
        files.sort();

        let mut checked = 0usize;
        let mut failures = Vec::new();
        for path in &files {
            let Ok(original) = std::fs::read_to_string(path) else {
                continue;
            };
            let once = format_source(&original, &FormatterConfig::default());
            let twice = format_source(&once, &FormatterConfig::default());
            checked += 1;
            if once != twice {
                failures.push(
                    path.file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string()),
                );
            }
        }

        eprintln!("idempotency validated over {checked} validation files");
        assert!(!failures.is_empty() || checked > 0);
        assert!(
            failures.is_empty(),
            "non-idempotent files ({}/{}):\n{}",
            failures.len(),
            checked,
            failures.join("\n")
        );
    }
}
