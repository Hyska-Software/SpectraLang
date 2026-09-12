fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::NAMESPACE,
            SemanticTokenType::TYPE,
            SemanticTokenType::ENUM,
            SemanticTokenType::INTERFACE,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::METHOD,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::PARAMETER,
            SemanticTokenType::PROPERTY,
            SemanticTokenType::ENUM_MEMBER,
        ],
        token_modifiers: Vec::new(),
    }
}

fn find_call_site(
    module: &spectra_compiler::ast::Module,
    line: usize,
    column: usize,
) -> Option<CallSite> {
    let mut best = None;

    for item in &module.items {
        match item {
            spectra_compiler::ast::Item::Function(function) => {
                find_call_site_in_block(&function.body, line, column, &mut best);
            }
            spectra_compiler::ast::Item::Impl(impl_block) => {
                for method in &impl_block.methods {
                    find_call_site_in_block(&method.body, line, column, &mut best);
                }
            }
            spectra_compiler::ast::Item::Trait(trait_def) => {
                for method in &trait_def.methods {
                    if let Some(body) = &method.body {
                        find_call_site_in_block(body, line, column, &mut best);
                    }
                }
            }
            spectra_compiler::ast::Item::TraitImpl(trait_impl) => {
                for method in &trait_impl.methods {
                    find_call_site_in_block(&method.body, line, column, &mut best);
                }
            }
            spectra_compiler::ast::Item::Import(_)
            | spectra_compiler::ast::Item::Struct(_)
            | spectra_compiler::ast::Item::Enum(_) => {}
            spectra_compiler::ast::Item::TypeAlias(_)
            | spectra_compiler::ast::Item::Const(_)
            | spectra_compiler::ast::Item::Static(_) => {}
        }
    }

    best
}

fn find_call_site_in_block(
    block: &spectra_compiler::ast::Block,
    line: usize,
    column: usize,
    best: &mut Option<CallSite>,
) {
    for statement in &block.statements {
        find_call_site_in_statement(statement, line, column, best);
    }
}

fn find_call_site_in_statement(
    statement: &spectra_compiler::ast::Statement,
    line: usize,
    column: usize,
    best: &mut Option<CallSite>,
) {
    match &statement.kind {
        spectra_compiler::ast::StatementKind::Let(let_stmt) => {
            if let Some(value) = &let_stmt.value {
                find_call_site_in_expression(value, line, column, best);
            }
        }
        spectra_compiler::ast::StatementKind::Assignment(assign_stmt) => {
            find_call_site_in_expression(&assign_stmt.value, line, column, best);
            match &assign_stmt.target {
                spectra_compiler::ast::LValue::IndexAccess { array, index } => {
                    find_call_site_in_expression(array, line, column, best);
                    find_call_site_in_expression(index, line, column, best);
                }
                spectra_compiler::ast::LValue::FieldAccess { object, .. } => {
                    find_call_site_in_expression(object, line, column, best);
                }
                spectra_compiler::ast::LValue::Identifier(_) => {}
            }
        }
        spectra_compiler::ast::StatementKind::Return(ret_stmt) => {
            if let Some(value) = &ret_stmt.value {
                find_call_site_in_expression(value, line, column, best);
            }
        }
        spectra_compiler::ast::StatementKind::Expression(expr) => {
            find_call_site_in_expression(expr, line, column, best);
        }
        spectra_compiler::ast::StatementKind::While(loop_stmt) => {
            find_call_site_in_expression(&loop_stmt.condition, line, column, best);
            find_call_site_in_block(&loop_stmt.body, line, column, best);
        }
        spectra_compiler::ast::StatementKind::DoWhile(loop_stmt) => {
            find_call_site_in_block(&loop_stmt.body, line, column, best);
            find_call_site_in_expression(&loop_stmt.condition, line, column, best);
        }
        spectra_compiler::ast::StatementKind::For(loop_stmt) => {
            find_call_site_in_expression(&loop_stmt.iterable, line, column, best);
            find_call_site_in_block(&loop_stmt.body, line, column, best);
        }
        spectra_compiler::ast::StatementKind::Loop(loop_stmt) => {
            find_call_site_in_block(&loop_stmt.body, line, column, best);
        }
        spectra_compiler::ast::StatementKind::Switch(switch_stmt) => {
            find_call_site_in_expression(&switch_stmt.value, line, column, best);
            for case in &switch_stmt.cases {
                find_call_site_in_expression(&case.pattern, line, column, best);
                find_call_site_in_block(&case.body, line, column, best);
            }
            if let Some(default) = &switch_stmt.default {
                find_call_site_in_block(default, line, column, best);
            }
        }
        spectra_compiler::ast::StatementKind::IfLet(if_let_stmt) => {
            find_call_site_in_expression(&if_let_stmt.value, line, column, best);
            find_call_site_in_block(&if_let_stmt.then_block, line, column, best);
            if let Some(else_block) = &if_let_stmt.else_block {
                find_call_site_in_block(else_block, line, column, best);
            }
        }
        spectra_compiler::ast::StatementKind::WhileLet(while_let_stmt) => {
            find_call_site_in_expression(&while_let_stmt.value, line, column, best);
            find_call_site_in_block(&while_let_stmt.body, line, column, best);
        }
        spectra_compiler::ast::StatementKind::Break
        | spectra_compiler::ast::StatementKind::Continue => {}
    }
}

fn find_call_site_in_expression(
    expr: &spectra_compiler::ast::Expression,
    line: usize,
    column: usize,
    best: &mut Option<CallSite>,
) {
    if !span_contains(expr.span, line, column) {
        return;
    }

    match &expr.kind {
        spectra_compiler::ast::ExpressionKind::Call { callee, arguments } => {
            record_call_site(
                CallSite {
                    span: expr.span,
                    search_start: callee.span.end,
                    kind: CallKind::Function {
                        callee_span: callee.span,
                    },
                },
                best,
            );
            find_call_site_in_expression(callee, line, column, best);
            for arg in arguments {
                find_call_site_in_expression(arg, line, column, best);
            }
        }
        spectra_compiler::ast::ExpressionKind::MethodCall {
            object, arguments, ..
        } => {
            record_call_site(
                CallSite {
                    span: expr.span,
                    search_start: object.span.end,
                    kind: CallKind::Method {
                        call_span: expr.span,
                    },
                },
                best,
            );
            find_call_site_in_expression(object, line, column, best);
            for arg in arguments {
                find_call_site_in_expression(arg, line, column, best);
            }
        }
        spectra_compiler::ast::ExpressionKind::Binary { left, right, .. } => {
            find_call_site_in_expression(left, line, column, best);
            find_call_site_in_expression(right, line, column, best);
        }
        spectra_compiler::ast::ExpressionKind::Unary { operand, .. }
        | spectra_compiler::ast::ExpressionKind::Try(operand)
        | spectra_compiler::ast::ExpressionKind::Await(operand)
        | spectra_compiler::ast::ExpressionKind::Grouping(operand) => {
            find_call_site_in_expression(operand, line, column, best);
        }
        spectra_compiler::ast::ExpressionKind::Range { start, end, .. }
        | spectra_compiler::ast::ExpressionKind::IndexAccess {
            array: start,
            index: end,
        } => {
            find_call_site_in_expression(start, line, column, best);
            find_call_site_in_expression(end, line, column, best);
        }
        spectra_compiler::ast::ExpressionKind::If {
            condition,
            then_block,
            elif_blocks,
            else_block,
        } => {
            find_call_site_in_expression(condition, line, column, best);
            find_call_site_in_block(then_block, line, column, best);
            for (elif_expr, elif_block) in elif_blocks {
                find_call_site_in_expression(elif_expr, line, column, best);
                find_call_site_in_block(elif_block, line, column, best);
            }
            if let Some(else_block) = else_block {
                find_call_site_in_block(else_block, line, column, best);
            }
        }
        spectra_compiler::ast::ExpressionKind::Unless {
            condition,
            then_block,
            else_block,
        } => {
            find_call_site_in_expression(condition, line, column, best);
            find_call_site_in_block(then_block, line, column, best);
            if let Some(else_block) = else_block {
                find_call_site_in_block(else_block, line, column, best);
            }
        }
        spectra_compiler::ast::ExpressionKind::ArrayLiteral { elements }
        | spectra_compiler::ast::ExpressionKind::TupleLiteral { elements } => {
            for element in elements {
                find_call_site_in_expression(element, line, column, best);
            }
        }
        spectra_compiler::ast::ExpressionKind::StructLiteral { fields, .. } => {
            for (_, value) in fields {
                find_call_site_in_expression(value, line, column, best);
            }
        }
        spectra_compiler::ast::ExpressionKind::FieldAccess { object, .. } => {
            find_call_site_in_expression(object, line, column, best);
        }
        spectra_compiler::ast::ExpressionKind::EnumVariant {
            data, struct_data, ..
        } => {
            if let Some(data) = data {
                for value in data {
                    find_call_site_in_expression(value, line, column, best);
                }
            }
            if let Some(struct_data) = struct_data {
                for (_, value) in struct_data {
                    find_call_site_in_expression(value, line, column, best);
                }
            }
        }
        spectra_compiler::ast::ExpressionKind::Match { scrutinee, arms } => {
            find_call_site_in_expression(scrutinee, line, column, best);
            for arm in arms {
                find_call_site_in_expression(&arm.body, line, column, best);
            }
        }
        spectra_compiler::ast::ExpressionKind::Lambda { body, .. } => {
            find_call_site_in_expression(body, line, column, best);
        }
        spectra_compiler::ast::ExpressionKind::Block(block) => {
            find_call_site_in_block(block, line, column, best);
        }
        spectra_compiler::ast::ExpressionKind::DifferentiableBlock(block) => {
            find_call_site_in_block(block, line, column, best);
        }
        spectra_compiler::ast::ExpressionKind::AsyncBlock(block) => {
            find_call_site_in_block(block, line, column, best);
        }
        spectra_compiler::ast::ExpressionKind::Cast { expr, .. } => {
            find_call_site_in_expression(expr, line, column, best);
        }
        spectra_compiler::ast::ExpressionKind::Identifier(_)
        | spectra_compiler::ast::ExpressionKind::NumberLiteral(_)
        | spectra_compiler::ast::ExpressionKind::StringLiteral(_)
        | spectra_compiler::ast::ExpressionKind::BoolLiteral(_)
        | spectra_compiler::ast::ExpressionKind::CharLiteral(_)
        | spectra_compiler::ast::ExpressionKind::FString(_)
        | spectra_compiler::ast::ExpressionKind::TupleAccess { .. } => {}
    }
}

fn record_call_site(candidate: CallSite, best: &mut Option<CallSite>) {
    match best {
        Some(current) if span_len(current.span) <= span_len(candidate.span) => {}
        _ => *best = Some(candidate),
    }
}

fn span_contains(span: Span, line: usize, column: usize) -> bool {
    let starts_before = line > span.start_location.line
        || (line == span.start_location.line && column >= span.start_location.column);
    let ends_after = line < span.end_location.line
        || (line == span.end_location.line && column < span.end_location.column);
    starts_before && ends_after
}

fn span_len(span: Span) -> usize {
    span.end.saturating_sub(span.start)
}

fn signature_label_for_call(document: &DocumentState, call_site: &CallSite) -> Option<String> {
    match call_site.kind {
        CallKind::Function { callee_span } => document
            .analysis
            .symbol_at(
                callee_span.start_location.line,
                callee_span.start_location.column,
            )
            .and_then(|symbol| symbol.definition.map(|definition| definition.label)),
        CallKind::Method { call_span } => document
            .analysis
            .symbols
            .get(&call_span)
            .and_then(|info| info.def_span)
            .and_then(|definition_span| document.analysis.definitions.get(&definition_span))
            .map(|definition| definition.label.clone()),
    }
}

fn split_signature_parameters(label: &str) -> Vec<String> {
    let Some(open_idx) = label.find('(') else {
        return Vec::new();
    };
    let Some(close_idx) = label.rfind(')') else {
        return Vec::new();
    };
    if close_idx <= open_idx + 1 {
        return Vec::new();
    }

    split_top_level(&label[open_idx + 1..close_idx], ',')
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

fn active_parameter_index(text: &str, call_site: &CallSite, position: Position) -> usize {
    let cursor_offset = position_to_offset(text, position);
    let Some(open_paren_offset) =
        find_open_paren_offset(text, call_site.search_start, call_site.span.end)
    else {
        return 0;
    };
    if cursor_offset <= open_paren_offset + 1 {
        return 0;
    }

    let scan_end = cursor_offset.min(call_site.span.end).min(text.len());
    let scan_start = (open_paren_offset + 1).min(scan_end);
    let snippet = &text[scan_start..scan_end];
    split_top_level(snippet, ',').len().saturating_sub(1)
}

fn split_top_level(input: &str, separator: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut in_string = false;
    let mut in_char = false;
    let mut escape = false;

    for (idx, ch) in input.char_indices() {
        if escape {
            escape = false;
            continue;
        }

        match ch {
            '\\' if in_string || in_char => {
                escape = true;
            }
            '"' if !in_char => in_string = !in_string,
            '\'' if !in_string => in_char = !in_char,
            '(' if !in_string && !in_char => paren_depth += 1,
            ')' if !in_string && !in_char => paren_depth = paren_depth.saturating_sub(1),
            '[' if !in_string && !in_char => bracket_depth += 1,
            ']' if !in_string && !in_char => bracket_depth = bracket_depth.saturating_sub(1),
            '{' if !in_string && !in_char => brace_depth += 1,
            '}' if !in_string && !in_char => brace_depth = brace_depth.saturating_sub(1),
            '<' if !in_string && !in_char => angle_depth += 1,
            '>' if !in_string && !in_char => angle_depth = angle_depth.saturating_sub(1),
            _ if ch == separator
                && !in_string
                && !in_char
                && paren_depth == 0
                && bracket_depth == 0
                && angle_depth == 0
                && brace_depth == 0 =>
            {
                parts.push(input[start..idx].to_string());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    parts.push(input[start..].to_string());
    parts
}

fn find_open_paren_offset(text: &str, start: usize, end: usize) -> Option<usize> {
    let upper = end.min(text.len());
    let lower = start.min(upper);
    text[lower..upper]
        .char_indices()
        .find_map(|(idx, ch)| (ch == '(').then_some(lower + idx))
}

fn position_to_offset(text: &str, position: Position) -> usize {
    let mut line = 0u32;
    let mut character = 0u32;

    for (idx, ch) in text.char_indices() {
        if line == position.line && character == position.character {
            return idx;
        }

        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }

    text.len()
}

fn offset_to_position(text: &str, target_offset: usize) -> Position {
    let mut line = 0u32;
    let mut character = 0u32;

    for (idx, ch) in text.char_indices() {
        if idx >= target_offset {
            return Position::new(line, character);
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }

    // offset beyond end of text → clamp to last position
    Position::new(line, character)
}

