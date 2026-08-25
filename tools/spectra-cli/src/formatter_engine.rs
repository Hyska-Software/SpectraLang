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
    let mut anchor_indent: usize = 0;
    let mut pending_open: i32 = 0;
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

        let normalized = normalize_spacing(trimmed_leading);
        let delta = net_round_bracket_delta(&normalized);
        // Operator continuations only apply at bracket depth zero; inside a
        // multi-line delimited group the paren machinery already indents.
        let continuation =
            pending_open <= 0 && is_wrapped_continuation_line(&normalized);
        let indent_for_line = if continuation {
            anchor_indent + 1
        } else {
            indent_level.saturating_sub(dedent)
        };
        lines.push(FormattedLine::new(indent_for_line, normalized));

        pending_open = (pending_open + delta).max(0);

        let (opens, closes) = count_brace_transitions(trimmed_leading, dedent);
        let level_base = if continuation { anchor_indent } else { indent_for_line };
        indent_level = level_base + opens;
        indent_level = indent_level.saturating_sub(closes);
        if !continuation {
            anchor_indent = indent_for_line;
        }
    }

    finalize_output(lines, config)
}

fn finalize_output(mut lines: Vec<FormattedLine>, config: &FormatterConfig) -> String {
    align_let_bindings(&mut lines, config);
    if config.max_line_length > 0 {
        wrap_long_lines(&mut lines, config);
        // Re-align: groups whose members were split by wrapping must settle
        // on padding that depends only on the surviving neighbours, keeping
        // fmt(fmt(x)) == fmt(x).
        align_let_bindings(&mut lines, config);
    }

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
            if lines[cursor].content.trim_end().ends_with('(') {
                // Wrapped call/array heads keep their own padding.
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

const WRAP_BINARY_PAIR_OPS: &[&str] = &["==", "!=", "<=", ">=", "&&", "||"];
const WRAP_BINARY_SINGLE_OPS: &[char] = &['+', '-', '*', '/', '%'];
const WRAP_ASSIGNMENT_NEEDLES: &[&str] =
    &[" = ", " += ", " -= ", " *= ", " /= ", " %= "];
const WRAP_DECL_PREFIXES: &[&str] = &["let ", "var ", "const "];

/// True when a line begins with a binary operator, i.e. it is the
/// continuation of an expression split across lines by [`wrap_long_lines`].
/// Continuations render one indent level below their anchor statement.
fn is_wrapped_continuation_line(content: &str) -> bool {
    let trimmed = content.trim_start();
    if trimmed.starts_with("//") {
        return false;
    }
    if WRAP_BINARY_PAIR_OPS.iter().any(|op| trimmed.starts_with(op)) {
        return true;
    }
    matches!(
        trimmed.chars().next(),
        Some(ch) if WRAP_BINARY_SINGLE_OPS.contains(&ch)
    )
}

/// Net count of `(`/`[` opened by this line (outside strings, char literals
/// and comments). Positive means a delimited group continues on the next
/// line, where operator-continuation indentation must not apply.
fn net_round_bracket_delta(line: &str) -> i32 {
    let mut delta = 0i32;
    let mut in_string = false;
    let mut in_char = false;
    let mut escape = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_string {
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
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '\'' {
                in_char = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '\'' => in_char = true,
            '/' if matches!(chars.peek(), Some('/')) => break,
            '(' | '[' => delta += 1,
            ')' | ']' => delta -= 1,
            _ => {}
        }
    }
    delta
}

/// One significant character of a normalized line: its byte index, the char,
/// the delimiter depth (over `()[]{}`) before/after it, and the generic angle
/// depth. String/char literal interiors are skipped; comments yield `None`
/// because a commented line has no safe wrap points. Angle depth tracks
/// `<...>` only when tightly attached to an identifier (generics), never for
/// spaced comparison operators.
type SignificantChar = (usize, char, usize, usize);

fn significant_chars(content: &str) -> Option<Vec<SignificantChar>> {
    let mut result: Vec<SignificantChar> = Vec::new();
    let mut depth = 0usize;
    let mut angle = 0usize;
    let mut in_string = false;
    let mut in_char = false;
    let mut escape = false;
    let mut previous: Option<char> = None;
    let mut chars = content.char_indices().peekable();

    while let Some((index, ch)) = chars.next() {
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            previous = Some(ch);
            continue;
        }
        if in_char {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '\'' {
                in_char = false;
            }
            previous = Some(ch);
            continue;
        }

        match ch {
            '"' => in_string = true,
            '\'' => in_char = true,
            '/' if matches!(chars.peek(), Some((_, '/'))) => return None,
            '(' | '[' | '{' => {
                result.push((index, ch, depth, angle));
                depth += 1;
            }
            ')' | ']' | '}' => {
                depth = depth.saturating_sub(1);
                result.push((index, ch, depth, angle));
            }
            '<' => {
                if generic_angle_context(previous) {
                    angle += 1;
                }
                result.push((index, ch, depth, angle));
            }
            '>' => {
                if angle > 0
                    && generic_angle_context(previous)
                    && !matches!(chars.peek(), Some((_, '=')))
                {
                    angle -= 1;
                }
                result.push((index, ch, depth, angle));
            }
            _ => result.push((index, ch, depth, angle)),
        }
        previous = Some(ch);
    }

    Some(result)
}

fn generic_angle_context(previous: Option<char>) -> bool {
    matches!(
        previous,
        Some(prev) if prev.is_alphanumeric() || prev == '_' || prev == '>'
    )
}

fn collapses_to(left: &str, right: &str) -> bool {
    left.chars()
        .filter(|ch| !ch.is_whitespace())
        .eq(right.chars().filter(|ch| !ch.is_whitespace()))
}

fn is_identifier_path(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == ':' || ch == '.')
        && text.chars().next().is_some_and(|ch| ch.is_alphabetic() || ch == '_')
}

struct DelimitedSplit {
    opener: char,
    head: String,
    items: Vec<String>,
    closer: char,
}

/// Splits a call/tuple/array literal whose outermost delimiter group ends the
/// line. Break points are top-level commas only (outside nested delimiters,
/// generics and lambda pipes); without at least one there is no safe break
/// point and the line stays intact.
fn try_wrap_delimited(content: &str, opener: char, closer: char) -> Option<DelimitedSplit> {
    let sigs = significant_chars(content)?;
    let open_position = sigs
        .iter()
        .position(|&(_, ch, depth, _)| ch == opener && depth == 0)?;
    let close_position = sigs[open_position..]
        .iter()
        .position(|&(_, ch, depth, _)| ch == closer && depth == 0)?
        + open_position;
    // The group must close exactly at end of line, otherwise the tail after
    // `)` / `]` belongs to a wider expression we must not touch.
    if close_position + 1 != sigs.len() || close_position == open_position + 1 {
        return None;
    }

    let open_byte = sigs[open_position].0;
    let close_byte = sigs[close_position].0;
    let inner = &sigs[open_position + 1..close_position];
    // Never break around lambda parameter lists (`|a, b| ...`).
    if inner.iter().any(|&(_, ch, _, _)| ch == '|') {
        return None;
    }
    let mut comma_bytes = Vec::new();
    for &(index, ch, depth, angle) in inner {
        if ch == ',' && depth == 1 && angle == 0 {
            comma_bytes.push(index);
        }
    }
    if comma_bytes.is_empty() {
        return None;
    }

    let mut items = Vec::with_capacity(comma_bytes.len() + 1);
    let mut cursor = open_byte + opener.len_utf8();
    for &comma in &comma_bytes {
        let item = content[cursor..comma].trim();
        if item.is_empty() {
            return None;
        }
        items.push(item.to_string());
        cursor = comma + ','.len_utf8();
    }
    let last = content[cursor..close_byte].trim();
    if last.is_empty() {
        return None;
    }
    items.push(last.to_string());

    let head = content[..open_byte].trim_end().to_string();
    let mut rejoined = String::with_capacity(content.len());
    rejoined.push_str(&head);
    rejoined.push(opener);
    rejoined.push_str(&items.join(","));
    rejoined.push(closer);
    // Safety net against scanner drift: rejoining the pieces must reproduce
    // the original token stream modulo whitespace. The trailing comma we
    // emit for calls/tuples is an intentional addition, so it stays out of
    // the comparison.
    if !collapses_to(&rejoined, content) {
        return None;
    }

    Some(DelimitedSplit {
        opener,
        head,
        items,
        closer,
    })
}

/// Splits a long binary expression on assignment / return statements before
/// each top-level operator. Only operators with whitespace on both sides are
/// considered, so unary `-` / `!` (tight after normalization) never split.
fn try_wrap_binary_expression(content: &str) -> Option<(String, Vec<String>)> {
    let sigs = significant_chars(content)?;
    let leading = content.len() - content.trim_start().len();
    let trimmed = content.trim_start();

    let mut assignment: Option<(usize, usize)> = None;
    if let Some(rest) = trimmed.strip_prefix("return ") {
        if rest.trim().is_empty() {
            return None;
        }
        assignment = Some((leading + "return".len(), leading + "return ".len()));
    } else {
        let declaration = WRAP_DECL_PREFIXES.iter().any(|p| trimmed.starts_with(p));
        for needle in WRAP_ASSIGNMENT_NEEDLES {
            if let Some(position) = content.find(needle) {
                // needle = " <op> " : position is its leading space, the
                // operator char sits right before the trailing space.
                let eq_byte = position + needle.len() - 2;
                let anchored = sigs.iter().any(
                    |&(index, ch, depth, angle)| {
                        index == eq_byte && ch == '=' && depth == 0 && angle == 0
                    },
                );
                if anchored
                    && assignment
                        .map_or(true, |(best, _)| eq_byte < best)
                {
                    assignment = Some((eq_byte, position + needle.len()));
                }
            }
        }
        let (eq_byte, _) = assignment?;
        if !declaration && !is_identifier_path(content[..eq_byte].trim()) {
            return None;
        }
    }
    let (_, rhs_start) = assignment?;

    let mut ops: Vec<(usize, usize)> = Vec::new();
    for &(index, _, depth, angle) in &sigs {
        if index < rhs_start || depth != 0 || angle != 0 {
            continue;
        }
        let rest = &content[index..];
        let length = WRAP_BINARY_PAIR_OPS
            .iter()
            .find_map(|op| rest.starts_with(op).then(|| op.len()))
            .or_else(|| {
                rest.chars().next().and_then(|ch| {
                    WRAP_BINARY_SINGLE_OPS
                        .contains(&ch)
                        .then(|| ch.len_utf8())
                })
            });
        if let Some(length) = length {
            let prev_raw = content[..index].chars().next_back();
            let next_raw = content[index + length..].chars().next();
            if prev_raw.is_some_and(|ch| ch.is_whitespace())
                && next_raw.is_some_and(|ch| ch.is_whitespace())
            {
                ops.push((index, index + length));
            }
        }
    }
    if ops.is_empty() {
        return None;
    }

    let (eq_byte, _) = assignment.unwrap();
    let mut operands = Vec::with_capacity(ops.len() + 1);
    let mut cursor = rhs_start;
    for &(start, end) in &ops {
        let operand = content[cursor..start].trim();
        if operand.is_empty() {
            return None;
        }
        operands.push(operand.to_string());
        cursor = end;
    }
    let tail = content[cursor..].trim();
    if tail.is_empty() {
        return None;
    }
    operands.push(tail.to_string());

    let lhs = content[..eq_byte].trim_end();
    let assign_op = content[eq_byte..rhs_start].trim();
    let head = if lhs == "return" {
        format!("return {}", operands[0])
    } else {
        format!("{} {} {}", lhs, assign_op, operands[0])
    };
    let continuations: Vec<String> = ops
        .iter()
        .enumerate()
        .map(|(position, &(start, end))| {
            format!("{} {}", &content[start..end], operands[position + 1])
        })
        .collect();

    let mut rejoined = String::with_capacity(content.len());
    rejoined.push_str(&head);
    for continuation in &continuations {
        rejoined.push_str(continuation);
    }
    if !collapses_to(&rejoined, content) {
        return None;
    }

    Some((head, continuations))
}

/// Attempts the three wrap forms for an over-long line, in priority order:
/// call/tuple argument list, array literal, binary expression. Returns the
/// replacement lines or `None` when no safe break point exists.
fn try_split_long_line(line: &FormattedLine) -> Option<Vec<FormattedLine>> {
    let content = line.content.trim();
    if content.starts_with("//") {
        return None;
    }
    if significant_chars(content).is_none() {
        return None;
    }

    if let Some(split) = try_wrap_delimited(content, '(', ')') {
        return Some(render_delimited_split(line, &split, true));
    }
    // Array literals: the parser rejects trailing commas inside `[...]`, so
    // the closing bracket keeps the last element on its own line instead.
    if let Some(split) = try_wrap_delimited(content, '[', ']') {
        return Some(render_delimited_split(line, &split, false));
    }
    let (head, continuations) = try_wrap_binary_expression(content)?;
    Some(render_binary_split(line, &head, &continuations))
}

fn render_delimited_split(
    line: &FormattedLine,
    split: &DelimitedSplit,
    trailing_comma: bool,
) -> Vec<FormattedLine> {
    let mut head = split.head.clone();
    head.push(split.opener);
    let mut result = vec![FormattedLine::new(
        line.indent_level,
        normalize_spacing(&head),
    )];
    let last = split.items.len().saturating_sub(1);
    for (index, item) in split.items.iter().enumerate() {
        let mut content = item.clone();
        if trailing_comma || index != last {
            content.push(',');
        }
        result.push(FormattedLine::new(
            line.indent_level + 1,
            normalize_spacing(&content),
        ));
    }
    result.push(FormattedLine::new(line.indent_level, split.closer.to_string()));
    result
}

fn render_binary_split(
    line: &FormattedLine,
    head: &str,
    continuations: &[String],
) -> Vec<FormattedLine> {
    let mut result = vec![FormattedLine::new(
        line.indent_level,
        normalize_spacing(head),
    )];
    for continuation in continuations {
        result.push(FormattedLine::new(
            line.indent_level + 1,
            normalize_spacing(continuation),
        ));
    }
    result
}

/// Replaces over-long lines with their wrapped form. Gate: the rendered line
/// must exceed `max_line_length` AND a safe break point must exist.
fn wrap_long_lines(lines: &mut Vec<FormattedLine>, config: &FormatterConfig) {
    let mut wrapped = Vec::with_capacity(lines.len());
    for line in lines.drain(..) {
        let exceeds = !line.is_blank
            && line.indent_level * config.indent_width
                + line.content.chars().count()
                > config.max_line_length;
        if !exceeds {
            wrapped.push(line);
            continue;
        }
        match try_split_long_line(&line) {
            Some(replacement) => wrapped.extend(replacement),
            None => wrapped.push(line),
        }
    }
    *lines = wrapped;
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

