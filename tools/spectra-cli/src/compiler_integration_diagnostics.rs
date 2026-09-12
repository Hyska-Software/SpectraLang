/// ANSI color codes for terminal output. Automatically disabled when NO_COLOR is set.
struct Colors {
    error_label: &'static str,
    warning_label: &'static str,
    arrow: &'static str,
    gutter: &'static str,
    caret_error: &'static str,
    caret_warning: &'static str,
    note_label: &'static str,
    help_label: &'static str,
    bold: &'static str,
    reset: &'static str,
}

const COLORS_ON: Colors = Colors {
    error_label: "\x1b[1;31m",
    warning_label: "\x1b[1;33m",
    arrow: "\x1b[36m",
    gutter: "\x1b[34m",
    caret_error: "\x1b[1;31m",
    caret_warning: "\x1b[1;33m",
    note_label: "\x1b[1;34m",
    help_label: "\x1b[1;32m",
    bold: "\x1b[1m",
    reset: "\x1b[0m",
};

const COLORS_OFF: Colors = Colors {
    error_label: "",
    warning_label: "",
    arrow: "",
    gutter: "",
    caret_error: "",
    caret_warning: "",
    note_label: "",
    help_label: "",
    bold: "",
    reset: "",
};

fn get_colors() -> &'static Colors {
    use std::io::IsTerminal as _;
    if std::env::var("NO_COLOR").is_ok()
        || std::env::var("TERM").as_deref() == Ok("dumb")
        || !std::io::stderr().is_terminal()
    {
        &COLORS_OFF
    } else {
        &COLORS_ON
    }
}

fn render_errors(errors: &[CompilerError], source: &str, filename: &str, stage: &str) -> String {
    if errors.is_empty() {
        return format!("{} failed with no diagnostics.", capitalize(stage));
    }

    let c = get_colors();
    let mut output = String::new();
    for (idx, error) in errors.iter().enumerate() {
        if idx > 0 {
            output.push('\n');
        }
        output.push_str(&render_error(error, source, filename));
    }

    // Summary line — mirrors rustc: "error: aborting due to N previous error(s)"
    let n = errors.len();
    output.push('\n');
    let _ = writeln!(
        &mut output,
        "{}error{}: aborting due to {} previous error{}",
        c.error_label,
        c.reset,
        n,
        if n == 1 { "" } else { "s" },
    );

    output
}

/// Subtract synthetic header lines from diagnostic spans (D6).
///
/// When the CLI prepends `module <name>\n` to a headerless file, every span
/// produced while compiling points one line past the on-disk location.
/// Shifting the span lines back (and rendering against the on-disk source)
/// restores the true location. Lines floor at 1: the synthetic header itself
/// is always valid, so a shifted span can never legitimately land on it.
pub(crate) fn shift_span_lines(span: &mut Span, shift: usize) {
    if shift == 0 {
        return;
    }
    span.start_location.line = span.start_location.line.saturating_sub(shift).max(1);
    span.end_location.line = span.end_location.line.saturating_sub(shift).max(1);
}

/// Shift every spanned compilation error; spanless midend/backend errors pass
/// through untouched.
fn shift_compilation_error_lines(errors: &mut [CompilerError], shift: usize) {
    if shift == 0 {
        return;
    }
    for error in errors {
        match error {
            CompilerError::Lexical(inner) => shift_span_lines(&mut inner.span, shift),
            CompilerError::Parse(inner) => shift_span_lines(&mut inner.span, shift),
            CompilerError::Semantic(inner) => shift_span_lines(&mut inner.span, shift),
            CompilerError::Midend(_) | CompilerError::Backend(_) => {}
        }
    }
}

/// Shift lint warning spans (primary and secondary) the same way.
fn shift_lint_lines(warnings: &mut [LintDiagnostic], shift: usize) {
    if shift == 0 {
        return;
    }
    for warning in warnings {
        shift_span_lines(&mut warning.span, shift);
        if let Some(secondary) = warning.secondary_span.as_mut() {
            shift_span_lines(secondary, shift);
        }
    }
}

enum DiagnosticSeverity {
    Error,
    Warning,
}

impl DiagnosticSeverity {
    fn as_str(&self) -> &'static str {
        match self {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
        }
    }

    fn color<'a>(&self, c: &'a Colors) -> &'a str {
        match self {
            DiagnosticSeverity::Error => c.error_label,
            DiagnosticSeverity::Warning => c.warning_label,
        }
    }

    fn caret_color<'a>(&self, c: &'a Colors) -> &'a str {
        match self {
            DiagnosticSeverity::Error => c.caret_error,
            DiagnosticSeverity::Warning => c.caret_warning,
        }
    }
}

fn render_error(error: &CompilerError, source: &str, filename: &str) -> String {
    match error {
        CompilerError::Lexical(e) => render_span_diagnostic(
            "syntax",
            DiagnosticSeverity::Error,
            &e.message,
            &e.span,
            e.hint.as_deref(),
            e.context.as_deref(),
            source,
            filename,
        ),
        CompilerError::Parse(e) => render_span_diagnostic(
            "syntax",
            DiagnosticSeverity::Error,
            &e.message,
            &e.span,
            e.hint.as_deref(),
            e.context.as_deref(),
            source,
            filename,
        ),
        CompilerError::Semantic(e) => render_span_diagnostic(
            "semantic",
            DiagnosticSeverity::Error,
            &e.message,
            &e.span,
            e.hint.as_deref(),
            e.context.as_deref(),
            source,
            filename,
        ),
        CompilerError::Midend(e) => {
            let c = get_colors();
            format!(
                "{}error[internal]{}: {}{}{}\n",
                c.error_label, c.reset, c.bold, e.message, c.reset
            )
        }
        CompilerError::Backend(e) => {
            let c = get_colors();
            format!(
                "{}error[codegen]{}: {}{}{}\n",
                c.error_label, c.reset, c.bold, e.message, c.reset
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_span_diagnostic(
    phase: &str,
    severity: DiagnosticSeverity,
    message: &str,
    span: &Span,
    hint: Option<&str>,
    context: Option<&str>,
    source: &str,
    filename: &str,
) -> String {
    let c = get_colors();
    let mut buf = String::new();

    // Header: error[phase]: message
    let sev_color = severity.color(c);
    let _ = writeln!(
        &mut buf,
        "{}{}[{}]{}: {}{}{}",
        sev_color,
        severity.as_str(),
        phase,
        c.reset,
        c.bold,
        message,
        c.reset
    );

    // Location: --> filename:line:col
    let _ = writeln!(
        &mut buf,
        "  {}-->{} {}:{}:{}",
        c.arrow, c.reset, filename, span.start_location.line, span.start_location.column
    );

    if let Some(raw_line) = get_source_line(source, span.start_location.line) {
        let line_text = raw_line.trim_end_matches('\r');
        let gutter_width = span.start_location.line.to_string().len();
        let pipe = format!("{}|{}", c.gutter, c.reset);

        // Empty gutter line
        let _ = writeln!(
            &mut buf,
            "  {}{:>width$} {}",
            c.gutter,
            "",
            c.reset,
            width = gutter_width
        );

        // Source line
        let _ = writeln!(
            &mut buf,
            "  {}{:>width$}{} {} {}",
            c.gutter,
            span.start_location.line,
            c.reset,
            pipe,
            line_text,
            width = gutter_width
        );

        // Caret line
        if let Some(marker_line) = build_highlight_line(span, line_text) {
            let caret_color = severity.caret_color(c);
            let _ = writeln!(
                &mut buf,
                "  {}{:>width$} {} {}{}{}",
                c.gutter,
                "",
                pipe,
                caret_color,
                marker_line,
                c.reset,
                width = gutter_width
            );
        }
    }

    if span.start_location.line != span.end_location.line {
        let _ = writeln!(
            &mut buf,
            "  {}= note:{} spans lines {}–{}",
            c.note_label, c.reset, span.start_location.line, span.end_location.line
        );
    }

    if let Some(context) = context {
        let _ = writeln!(&mut buf, "  {}= note:{} {}", c.note_label, c.reset, context);
    }

    if let Some(hint) = hint {
        let _ = writeln!(&mut buf, "  {}= help:{} {}", c.help_label, c.reset, hint);
    }

    buf
}

fn render_lint_warning(diagnostic: &LintDiagnostic, filename: &str, source: &str) -> String {
    let mut context = diagnostic.note.clone().unwrap_or_default();

    if let Some(secondary) = diagnostic.secondary_span {
        let related = format!(
            "related location: {}:{}:{}",
            filename, secondary.start_location.line, secondary.start_location.column
        );

        if context.is_empty() {
            context = related;
        } else {
            context.push_str("; ");
            context.push_str(&related);
        }
    }

    let context_owned = if context.is_empty() {
        None
    } else {
        Some(context)
    };
    let context_ref = context_owned.as_deref();

    render_span_diagnostic(
        &format!("lint({})", diagnostic.rule.code()),
        DiagnosticSeverity::Warning,
        &diagnostic.message,
        &diagnostic.span,
        None,
        context_ref,
        source,
        filename,
    )
}

fn get_source_line(source: &str, line_number: usize) -> Option<&str> {
    if line_number == 0 {
        return None;
    }

    source.lines().nth(line_number.saturating_sub(1))
}

fn build_highlight_line(span: &Span, line_text: &str) -> Option<String> {
    if line_text.is_empty() {
        return None;
    }

    let total_chars = line_text.chars().count();
    let start_column = span.start_location.column.max(1);
    let mut start_index = start_column.saturating_sub(1);
    if start_index > total_chars {
        start_index = total_chars;
    }

    let end_column = if span.start_location.line == span.end_location.line {
        span.end_location.column.max(start_column)
    } else {
        total_chars + 1
    };

    let mut end_index = end_column.saturating_sub(1);
    if end_index < start_index {
        end_index = start_index;
    }
    if end_index > total_chars {
        end_index = total_chars;
    }

    let span_width = end_index.saturating_sub(start_index);
    let highlight_len = if span_width == 0 { 1 } else { span_width + 1 };

    let mut marker = String::new();
    for _ in 0..start_index {
        marker.push(' ');
    }
    for _ in 0..highlight_len {
        marker.push('^');
    }

    Some(marker)
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => {
            let mut result = first.to_uppercase().collect::<String>();
            result.push_str(chars.as_str());
            result
        }
        None => String::new(),
    }
}

impl Default for SpectraCompiler {
    fn default() -> Self {
        Self::new(CompilationOptions::default())
    }
}
