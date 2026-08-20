fn format_source(input: &str, config: &FormatterConfig) -> String {
    match cst::format_with_cst(input, config) {
        Ok(formatted) => formatted,
        Err(_) => legacy_format_source(input, config),
    }
}

#[derive(Debug, Clone)]
struct FormattedLine {
    indent_level: usize,
    content: String,
    is_blank: bool,
}

impl FormattedLine {
    fn blank() -> Self {
        Self {
            indent_level: 0,
            content: String::new(),
            is_blank: true,
        }
    }

    fn new(indent_level: usize, content: String) -> Self {
        Self {
            indent_level,
            content,
            is_blank: false,
        }
    }
}

fn legacy_format_source(input: &str, config: &FormatterConfig) -> String {
    let mut indent_level: usize = 0;
    let mut lines = Vec::new();

    for line in input.split('\n') {
        let trimmed_trailing =
            line.trim_end_matches([' ', '\t', '\r']);
        let trimmed_leading = trimmed_trailing.trim_start();

        if trimmed_leading.is_empty() {
            lines.push(FormattedLine::blank());
            continue;
        }

        let mut dedent = count_leading_closing_braces(trimmed_leading);
        if dedent > indent_level {
            dedent = indent_level;
        }

        let indent_for_line = indent_level.saturating_sub(dedent);
        let normalized = normalize_spacing(trimmed_leading);
        lines.push(FormattedLine::new(indent_for_line, normalized));

        let (opens, closes) = count_brace_transitions(trimmed_leading, dedent);
        indent_level = indent_for_line + opens;
        indent_level = indent_level.saturating_sub(closes);
    }

    finalize_output(lines, config)
}

fn finalize_output(mut lines: Vec<FormattedLine>, config: &FormatterConfig) -> String {
    align_let_bindings(&mut lines, config);

    let mut output_lines = Vec::new();
    let mut blank_streak = 0usize;

    for line in lines {
        if line.is_blank {
            blank_streak += 1;
            if blank_streak > 1 {
                continue;
            }
            output_lines.push(String::new());
            continue;
        }

        blank_streak = 0;
        let mut buffer = String::new();
        buffer.extend(std::iter::repeat_n(' ', line.indent_level * config.indent_width));
        buffer.push_str(&line.content);
        output_lines.push(buffer);
    }

    while matches!(output_lines.last(), Some(value) if value.is_empty()) {
        output_lines.pop();
    }

    if output_lines.is_empty() {
        String::new()
    } else {
        output_lines.join("\n") + "\n"
    }
}

/// Returns true if `result` (trimmed) ends with a keyword that syntactically
/// expects an expression to follow (e.g. `return`, `else`, `if`).
/// Used to detect unary operators like `-` and `!` after these keywords.
fn ends_with_expression_keyword(result: &str) -> bool {
    let trimmed = result.trim_end();
    const KEYWORDS: &[&str] = &[
        "return", "else", "if", "while", "for", "in", "match", "when", "then", "not",
    ];
    KEYWORDS.iter().any(|&kw| {
        if let Some(rest) = trimmed.strip_suffix(kw) {
            rest.is_empty() || !rest.ends_with(|c: char| c.is_alphanumeric() || c == '_')
        } else {
            false
        }
    })
}

fn normalize_spacing(content: &str) -> String {
    let mut result = String::new();
    let mut chars = content.chars().peekable();
    let mut in_string = false;
    let mut in_char = false;
    let mut escape = false;
    let mut pending_space = false;
    let mut generic_depth = 0usize;
    let mut suppress_space_after_unary = false;

    while let Some(ch) = chars.next() {
        if in_string {
            result.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if in_char {
            result.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '\'' {
                in_char = false;
            }
            continue;
        }

        if !ch.is_whitespace() {
            suppress_space_after_unary = false;
        }

        match ch {
            '"' => {
                push_pending_space(&mut result, &mut pending_space);
                result.push('"');
                in_string = true;
            }
            '\'' => {
                push_pending_space(&mut result, &mut pending_space);
                result.push('\'');
                in_char = true;
            }
            ch if ch.is_whitespace() => {
                if !result.ends_with("::")
                    && previous_non_space(&result) != Some('<')
                    && !suppress_space_after_unary
                {
                    pending_space = true;
                }
            }
            ':' => {
                while result.ends_with(' ') {
                    result.pop();
                }
                if matches!(chars.peek(), Some(':')) {
                    result.push(':');
                    result.push(':');
                    chars.next();
                    pending_space = false;
                } else {
                    result.push(':');
                    pending_space = true;
                }
            }
            ',' | ';' => {
                while result.ends_with(' ') {
                    result.pop();
                }
                result.push(ch);
                pending_space = true;
            }
            ')' | ']' => {
                while result.ends_with(' ') {
                    result.pop();
                }
                result.push(ch);
                pending_space = true;
            }
            '}' => {
                // Keep the delimiter padding produced for inline record
                // literals (`Point { x: 1 }`).  Block closing braces are
                // normally on their own line and therefore are unaffected.
                push_pending_space(&mut result, &mut pending_space);
                result.push(ch);
                pending_space = true;
            }
            '(' | '[' | '{' => {
                // Force a space before '{' when it immediately follows an identifier or
                // keyword (e.g. `else{` → `else {`, `if cond{` → `if cond {`).
                if ch == '{'
                    && matches!(result.chars().last(), Some(c) if c.is_alphanumeric() || c == '_') {
                        pending_space = true;
                    }
                push_pending_space(&mut result, &mut pending_space);
                result.push(ch);
            }
            '/' => {
                if matches!(chars.peek(), Some('/')) {
                    if !result.is_empty() && !result.ends_with(' ') {
                        result.push(' ');
                    }
                    result.push('/');
                    result.push('/');
                    chars.next();
                    // Detect doc comment (`///`) and ensure a space after the marker.
                    if matches!(chars.peek(), Some('/')) {
                        chars.next();
                        result.push('/');
                        if !matches!(chars.peek(), Some(' ') | None) {
                            result.push(' ');
                        }
                    }
                    for next in chars.by_ref() {
                        result.push(next);
                    }
                    break;
                }
                push_pending_space(&mut result, &mut pending_space);
                result.push('/');
            }
            '.' if matches!(chars.peek(), Some('.')) => {
                push_pending_space(&mut result, &mut pending_space);
                result.push('.');
                result.push('.');
                chars.next();
                if matches!(chars.peek(), Some('=')) {
                    result.push('=');
                    chars.next();
                }
                pending_space = true;
            }
            '<' if is_generic_open_context(&result, &chars) => {
                while result.ends_with(' ') {
                    result.pop();
                }
                result.push('<');
                pending_space = false;
                generic_depth += 1;
            }
            '>' if generic_depth > 0 && !matches!(chars.peek(), Some('=')) => {
                while result.ends_with(' ') {
                    result.pop();
                }
                result.push('>');
                pending_space = false;
                generic_depth = generic_depth.saturating_sub(1);
            }
            _ => {
                if let Some(op) = read_operator(ch, &mut chars) {
                    let prev = previous_non_space(&result);
                    let after_unary_context = matches!(
                        prev,
                        None
                            | Some(
                                '(' | '[' | '{' | '=' | ',' | ':' | '+' | '-' | '*' | '/'
                                    | '%' | '<' | '>' | '&' | '|'
                            )
                    ) || ends_with_expression_keyword(&result);
                    let is_unary_minus = op == "-" && after_unary_context;
                    let is_unary_not = op == "!" && after_unary_context;

                    if is_unary_minus || is_unary_not {
                        push_pending_space(&mut result, &mut pending_space);
                        result.push_str(&op);
                        suppress_space_after_unary = true;
                    } else {
                        if !result.is_empty() && !result.ends_with(' ') {
                            result.push(' ');
                        }
                        result.push_str(&op);
                        pending_space = true;
                    }
                    continue;
                }

                push_pending_space(&mut result, &mut pending_space);
                result.push(ch);
            }
        }
    }

    result.trim_end().to_string()
}

fn is_generic_open_context(
    result: &str,
    chars: &std::iter::Peekable<std::str::Chars<'_>>,
) -> bool {
    let previous = result
        .trim_end()
        .rsplit(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .next()
        .filter(|name| !name.is_empty());
    let next = peek_identifier(chars);

    let Some(previous) = previous else {
        return false;
    };
    let Some(next) = next else {
        return false;
    };

    (is_type_like_name(previous) || is_type_like_name(&next))
        && previous
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
}

fn peek_identifier(chars: &std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    let mut lookahead = chars.clone();
    while matches!(lookahead.peek(), Some(ch) if ch.is_whitespace()) {
        lookahead.next();
    }

    let mut value = String::new();
    while matches!(lookahead.peek(), Some(ch) if ch.is_ascii_alphanumeric() || *ch == '_') {
        if let Some(ch) = lookahead.next() {
            value.push(ch);
        }
    }
    (!value.is_empty()).then_some(value)
}

fn is_type_like_name(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "byte"
            | "char"
            | "float"
            | "int"
            | "string"
            | "unit"
            | "void"
            | "dynamic_dim"
            | "rank1"
            | "rank2"
            | "rank3"
            | "rank4"
            | "row_major"
            | "col_major"
            | "cpu"
            | "wgpu"
    ) || name
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn read_operator(
    first: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Option<String> {
    let mut op = String::new();
    op.push(first);

    match first {
        '=' => match chars.peek() {
            Some('=') => {
                chars.next();
                op.push('=');
                Some(op)
            }
            Some('>') => {
                chars.next();
                op.push('>');
                Some(op)
            }
            _ => Some(op),
        },
        '!' => match chars.peek() {
            Some('=') => {
                chars.next();
                op.push('=');
                Some(op)
            }
            _ => Some(op),
        },
        '<' => match chars.peek() {
            Some('=') => {
                chars.next();
                op.push('=');
                Some(op)
            }
            _ => Some(op),
        },
        '>' => match chars.peek() {
            Some('=') => {
                chars.next();
                op.push('=');
                Some(op)
            }
            _ => Some(op),
        },
        '&' => match chars.peek() {
            Some('&') => {
                chars.next();
                op.push('&');
                Some(op)
            }
            _ => Some(op),
        },
        '|' => match chars.peek() {
            Some('|') => {
                chars.next();
                op.push('|');
                Some(op)
            }
            _ => Some(op),
        },
        '+' | '*' | '/' | '%' => Some(op),
        '-' => match chars.peek() {
            Some('>') => {
                chars.next();
                op.push('>');
                Some(op)
            }
            Some('=') => {
                chars.next();
                op.push('=');
                Some(op)
            }
            _ => Some(op),
        },
        _ => None,
    }
}

fn previous_non_space(result: &str) -> Option<char> {
    result.chars().rev().find(|ch| !ch.is_whitespace())
}

fn push_pending_space(result: &mut String, pending: &mut bool) {
    if *pending && !result.is_empty() && !result.ends_with(' ') {
        result.push(' ');
    }
    *pending = false;
}

fn align_let_bindings(lines: &mut [FormattedLine], config: &FormatterConfig) {
    let mut index = 0usize;

    while index < lines.len() {
        if lines[index].is_blank {
            index += 1;
            continue;
        }

        let indent = lines[index].indent_level;
        if !lines[index].content.starts_with("let ") {
            index += 1;
            continue;
        }

        let mut group = Vec::new();
        let mut cursor = index;

        while cursor < lines.len() {
            if lines[cursor].is_blank || lines[cursor].indent_level != indent {
                break;
            }
            if !lines[cursor].content.starts_with("let ") {
                break;
            }
            if let Some(split) = lines[cursor].content.split_once(" = ") {
                group.push((cursor, split.0.to_string(), split.1.to_string()));
                cursor += 1;
            } else {
                break;
            }
        }

        if group.len() < 2 {
            index += 1;
            continue;
        }

        let max_binding = group
            .iter()
            .map(|(_, before, _)| before.len())
            .max()
            .unwrap_or(0);
        let target_column = max_binding + 1;

        let exceeds_limit = group.iter().any(|(_, _, after)| {
            let line_length = indent * config.indent_width + target_column + 2 + after.len();
            line_length > config.max_line_length
        });

        if exceeds_limit {
            index = cursor;
            continue;
        }

        for (line_index, before, after) in group {
            let padding = target_column.saturating_sub(before.len());
            let mut rebuilt = before;
            rebuilt.extend(std::iter::repeat_n(' ', padding));
            rebuilt.push_str("= ");
            rebuilt.push_str(&after);
            lines[line_index].content = rebuilt;
        }

        index = cursor;
    }
}

fn count_leading_closing_braces(line: &str) -> usize {
    line.chars()
        .take_while(|ch| matches!(ch, '}' | ']' | ')'))
        .count()
}

fn count_brace_transitions(line: &str, mut skip_closing: usize) -> (usize, usize) {
    let mut opens = 0usize;
    let mut closes = 0usize;
    let mut chars = line.chars().peekable();
    let mut in_string = false;
    let mut in_char = false;
    let mut escape = false;

    while let Some(ch) = chars.next() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            match ch {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        if in_char {
            if escape {
                escape = false;
                continue;
            }
            match ch {
                '\\' => escape = true,
                '\'' => in_char = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '\'' => in_char = true,
            '/' => {
                if matches!(chars.peek(), Some('/')) {
                    break;
                }
            }
            '{' | '(' | '[' => opens += 1,
            '}' | ')' | ']' => {
                if skip_closing > 0 {
                    skip_closing -= 1;
                } else {
                    closes += 1;
                }
            }
            _ => {}
        }
    }

    (opens, closes)
}

