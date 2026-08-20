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
}
