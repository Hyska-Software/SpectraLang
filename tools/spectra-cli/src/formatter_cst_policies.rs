    fn apply_ast_policies(
        lines: &mut [FormattedLine],
        module: &Module,
        tokens: &[Token],
        source: &str,
    ) {
        let line_offsets = compute_line_offsets(source);
        ensure_doc_comment_alignment(lines, module, &line_offsets);
        enforce_match_arm_formatting(lines, module, tokens, source, &line_offsets);
    }

    fn ensure_doc_comment_alignment(
        lines: &mut [FormattedLine],
        module: &Module,
        line_offsets: &[usize],
    ) {
        for item in &module.items {
            adjust_doc_comments_for_span(item_span(item), lines, line_offsets);
            match item {
                Item::Function(func) => {
                    collect_nested_doc_comment_targets_function(func, lines, line_offsets);
                }
                Item::Struct(node) => adjust_doc_comments_for_span(&node.span, lines, line_offsets),
                Item::Enum(node) => adjust_doc_comments_for_span(&node.span, lines, line_offsets),
                Item::Import(node) => adjust_doc_comments_for_span(&node.span, lines, line_offsets),
                Item::Impl(impl_block) => {
                    collect_nested_doc_comment_targets_impl(impl_block, lines, line_offsets);
                }
                Item::Trait(trait_decl) => {
                    collect_nested_doc_comment_targets_trait(trait_decl, lines, line_offsets);
                }
                Item::TraitImpl(trait_impl) => {
                    collect_nested_doc_comment_targets_trait_impl(trait_impl, lines, line_offsets);
                }
                Item::TypeAlias(_) | Item::Const(_) | Item::Static(_) => {}
            }
        }
    }

    fn enforce_match_arm_formatting(
        lines: &mut [FormattedLine],
        module: &Module,
        tokens: &[Token],
        source: &str,
        line_offsets: &[usize],
    ) {
        let mut spans = Vec::new();
        collect_match_spans_module(module, &mut spans);

        for span in spans {
            rewrite_simple_match_arms(lines, tokens, source, &span, line_offsets);
        }
    }

    fn compute_line_offsets(source: &str) -> Vec<usize> {
        let mut offsets = Vec::new();
        offsets.push(0);
        let mut acc = 0usize;
        for line in source.split_inclusive('\n') {
            acc += line.len();
            offsets.push(acc);
        }
        offsets
    }

    fn line_index_from_offset(offset: usize, offsets: &[usize]) -> Option<usize> {
        match offsets.binary_search(&offset) {
            Ok(index) => Some(index),
            Err(index) => Some(index.saturating_sub(1)),
        }
    }

    fn adjust_doc_comments_for_span(
        span: &Span,
        lines: &mut [FormattedLine],
        line_offsets: &[usize],
    ) {
        if let Some(start_line) = line_index_from_offset(span.start, line_offsets) {
            if start_line == 0 || start_line >= lines.len() {
                return;
            }
            let target_indent = lines
                .get(start_line)
                .map(|line| line.indent_level)
                .unwrap_or(0);

            let mut cursor = start_line;
            let mut doc_indices = Vec::new();
            while cursor > 0 {
                let prev_index = cursor - 1;
                if prev_index >= lines.len() {
                    break;
                }
                if lines[prev_index].is_blank {
                    if doc_indices.is_empty() {
                        cursor = prev_index;
                        continue;
                    }
                    break;
                }
                if lines[prev_index].content.trim_start().starts_with("///") {
                    doc_indices.push(prev_index);
                    cursor = prev_index;
                } else {
                    break;
                }
            }

            for index in doc_indices {
                if let Some(line) = lines.get_mut(index) {
                    line.indent_level = target_indent;
                    line.content = sanitize_doc_comment(&line.content);
                }
            }
        }
    }

    fn collect_nested_doc_comment_targets_function(
        function: &Function,
        lines: &mut [FormattedLine],
        line_offsets: &[usize],
    ) {
        adjust_doc_comments_for_span(&function.span, lines, line_offsets);
        collect_doc_comments_from_block(&function.body, lines, line_offsets);
    }

    fn collect_nested_doc_comment_targets_impl(
        impl_block: &ImplBlock,
        lines: &mut [FormattedLine],
        line_offsets: &[usize],
    ) {
        adjust_doc_comments_for_span(&impl_block.span, lines, line_offsets);
        for method in &impl_block.methods {
            collect_doc_comments_from_method(method, lines, line_offsets);
        }
    }

    fn collect_nested_doc_comment_targets_trait(
        trait_decl: &TraitDeclaration,
        lines: &mut [FormattedLine],
        line_offsets: &[usize],
    ) {
        adjust_doc_comments_for_span(&trait_decl.span, lines, line_offsets);
        for method in &trait_decl.methods {
            adjust_doc_comments_for_span(&method.span, lines, line_offsets);
        }
    }

    fn collect_nested_doc_comment_targets_trait_impl(
        trait_impl: &TraitImpl,
        lines: &mut [FormattedLine],
        line_offsets: &[usize],
    ) {
        adjust_doc_comments_for_span(&trait_impl.span, lines, line_offsets);
        for method in &trait_impl.methods {
            collect_doc_comments_from_method(method, lines, line_offsets);
        }
    }

    fn collect_doc_comments_from_method(
        method: &Method,
        lines: &mut [FormattedLine],
        line_offsets: &[usize],
    ) {
        adjust_doc_comments_for_span(&method.span, lines, line_offsets);
        collect_doc_comments_from_block(&method.body, lines, line_offsets);
    }

    fn collect_doc_comments_from_block(
        block: &Block,
        lines: &mut [FormattedLine],
        line_offsets: &[usize],
    ) {
        for statement in &block.statements {
            adjust_doc_comments_for_span(&statement.span, lines, line_offsets);
            collect_doc_comments_from_statement(statement, lines, line_offsets);
        }
    }

    fn collect_doc_comments_from_statement(
        statement: &Statement,
        lines: &mut [FormattedLine],
        line_offsets: &[usize],
    ) {
        match &statement.kind {
            StatementKind::Expression(expr) => {
                collect_doc_comments_from_expression(expr, lines, line_offsets)
            }
            StatementKind::Let(let_stmt) => {
                if let Some(value) = &let_stmt.value {
                    collect_doc_comments_from_expression(value, lines, line_offsets);
                }
            }
            StatementKind::Assignment(assign) => {
                collect_doc_comments_from_expression(&assign.value, lines, line_offsets);
            }
            StatementKind::Return(ret) => {
                if let Some(expr) = &ret.value {
                    collect_doc_comments_from_expression(expr, lines, line_offsets);
                }
            }
            StatementKind::While(while_loop) => {
                collect_doc_comments_from_expression(&while_loop.condition, lines, line_offsets);
                collect_doc_comments_from_block(&while_loop.body, lines, line_offsets);
            }
            StatementKind::DoWhile(do_while) => {
                collect_doc_comments_from_block(&do_while.body, lines, line_offsets);
                collect_doc_comments_from_expression(&do_while.condition, lines, line_offsets);
            }
            StatementKind::For(for_loop) => {
                collect_doc_comments_from_expression(&for_loop.iterable, lines, line_offsets);
                collect_doc_comments_from_block(&for_loop.body, lines, line_offsets);
            }
            StatementKind::Loop(loop_stmt) => {
                collect_doc_comments_from_block(&loop_stmt.body, lines, line_offsets);
            }
            StatementKind::Switch(switch_stmt) => {
                collect_doc_comments_from_expression(&switch_stmt.value, lines, line_offsets);
                for case in &switch_stmt.cases {
                    collect_doc_comments_from_block(&case.body, lines, line_offsets);
                }
                if let Some(default) = &switch_stmt.default {
                    collect_doc_comments_from_block(default, lines, line_offsets);
                }
            }
            StatementKind::Break | StatementKind::Continue => {}
            StatementKind::IfLet(stmt) => {
                collect_doc_comments_from_expression(&stmt.value, lines, line_offsets);
                collect_doc_comments_from_block(&stmt.then_block, lines, line_offsets);
                if let Some(else_b) = &stmt.else_block {
                    collect_doc_comments_from_block(else_b, lines, line_offsets);
                }
            }
            StatementKind::WhileLet(stmt) => {
                collect_doc_comments_from_expression(&stmt.value, lines, line_offsets);
                collect_doc_comments_from_block(&stmt.body, lines, line_offsets);
            }
        }
    }

    fn collect_doc_comments_from_expression(
        expression: &Expression,
        lines: &mut [FormattedLine],
        line_offsets: &[usize],
    ) {
        adjust_doc_comments_for_span(&expression.span, lines, line_offsets);
        match &expression.kind {
            ExpressionKind::Binary { left, right, .. } => {
                collect_doc_comments_from_expression(left, lines, line_offsets);
                collect_doc_comments_from_expression(right, lines, line_offsets);
            }
            ExpressionKind::Unary { operand, .. } | ExpressionKind::Await(operand) => {
                collect_doc_comments_from_expression(operand, lines, line_offsets);
            }
            ExpressionKind::Call { callee, arguments } => {
                collect_doc_comments_from_expression(callee, lines, line_offsets);
                for arg in arguments {
                    collect_doc_comments_from_expression(arg, lines, line_offsets);
                }
            }
            ExpressionKind::If {
                condition,
                then_block,
                elif_blocks,
                else_block,
            } => {
                collect_doc_comments_from_expression(condition, lines, line_offsets);
                collect_doc_comments_from_block(then_block, lines, line_offsets);
                for (expr, block) in elif_blocks {
                    collect_doc_comments_from_expression(expr, lines, line_offsets);
                    collect_doc_comments_from_block(block, lines, line_offsets);
                }
                if let Some(block) = else_block {
                    collect_doc_comments_from_block(block, lines, line_offsets);
                }
            }
            ExpressionKind::Unless {
                condition,
                then_block,
                else_block,
            } => {
                collect_doc_comments_from_expression(condition, lines, line_offsets);
                collect_doc_comments_from_block(then_block, lines, line_offsets);
                if let Some(block) = else_block {
                    collect_doc_comments_from_block(block, lines, line_offsets);
                }
            }
            ExpressionKind::ArrayLiteral { elements }
            | ExpressionKind::TupleLiteral { elements } => {
                for element in elements {
                    collect_doc_comments_from_expression(element, lines, line_offsets);
                }
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                for (_, expr) in fields {
                    collect_doc_comments_from_expression(expr, lines, line_offsets);
                }
            }
            ExpressionKind::EnumVariant {
                data, struct_data, ..
            } => {
                if let Some(elements) = data {
                    for expr in elements {
                        collect_doc_comments_from_expression(expr, lines, line_offsets);
                    }
                }
                if let Some(fields) = struct_data {
                    for (_, expr) in fields {
                        collect_doc_comments_from_expression(expr, lines, line_offsets);
                    }
                }
            }
            ExpressionKind::Match { scrutinee, arms } => {
                collect_doc_comments_from_expression(scrutinee, lines, line_offsets);
                for arm in arms {
                    collect_doc_comments_from_expression(&arm.body, lines, line_offsets);
                }
            }
            ExpressionKind::MethodCall {
                object, arguments, ..
            } => {
                collect_doc_comments_from_expression(object, lines, line_offsets);
                for arg in arguments {
                    collect_doc_comments_from_expression(arg, lines, line_offsets);
                }
            }
            ExpressionKind::Grouping(expr)
            | ExpressionKind::IndexAccess { array: expr, .. }
            | ExpressionKind::FieldAccess { object: expr, .. }
            | ExpressionKind::TupleAccess { tuple: expr, .. } => {
                collect_doc_comments_from_expression(expr, lines, line_offsets);
            }
            ExpressionKind::NumberLiteral(_)
            | ExpressionKind::StringLiteral(_)
            | ExpressionKind::BoolLiteral(_)
            | ExpressionKind::Identifier(_) => {}
            ExpressionKind::CharLiteral(_) => {}
            ExpressionKind::FString(parts) => {
                for part in parts {
                    if let spectra_compiler::ast::FStringPart::Interpolated(expr) = part {
                        collect_doc_comments_from_expression(expr, lines, line_offsets);
                    }
                }
            }
            ExpressionKind::Lambda { body, .. } => {
                collect_doc_comments_from_expression(body, lines, line_offsets);
            }
            ExpressionKind::Try(inner) => {
                collect_doc_comments_from_expression(inner, lines, line_offsets);
            }
            ExpressionKind::Range { start, end, .. } => {
                collect_doc_comments_from_expression(start, lines, line_offsets);
                collect_doc_comments_from_expression(end, lines, line_offsets);
            }
            ExpressionKind::Cast { expr, .. } => {
                collect_doc_comments_from_expression(expr, lines, line_offsets);
            }
            ExpressionKind::Block(block) => {
                for stmt in &block.statements {
                    collect_doc_comments_from_statement(stmt, lines, line_offsets);
                }
            }
            ExpressionKind::DifferentiableBlock(block) | ExpressionKind::AsyncBlock(block) => {
                for stmt in &block.statements {
                    collect_doc_comments_from_statement(stmt, lines, line_offsets);
                }
            }
        }
    }

    fn collect_match_spans_module(module: &Module, spans: &mut Vec<Span>) {
        for item in &module.items {
            match item {
                Item::Function(function) => collect_match_spans_block(&function.body, spans),
                Item::Impl(impl_block) => collect_match_spans_impl(impl_block, spans),
                Item::Trait(trait_decl) => collect_match_spans_trait(trait_decl, spans),
                Item::TraitImpl(trait_impl) => collect_match_spans_trait_impl(trait_impl, spans),
                Item::Struct(_) | Item::Enum(_) | Item::Import(_) => {}
                Item::TypeAlias(_) | Item::Const(_) | Item::Static(_) => {}
            }
        }
    }

    fn collect_match_spans_impl(impl_block: &ImplBlock, spans: &mut Vec<Span>) {
        for method in &impl_block.methods {
            collect_match_spans_block(&method.body, spans);
        }
    }

    fn collect_match_spans_trait(trait_decl: &TraitDeclaration, spans: &mut Vec<Span>) {
        for method in &trait_decl.methods {
            if let Some(body) = &method.body {
                collect_match_spans_block(body, spans);
            }
        }
    }

    fn collect_match_spans_trait_impl(trait_impl: &TraitImpl, spans: &mut Vec<Span>) {
        for method in &trait_impl.methods {
            collect_match_spans_block(&method.body, spans);
        }
    }

    fn collect_match_spans_block(block: &Block, spans: &mut Vec<Span>) {
        for statement in &block.statements {
            collect_match_spans_statement(statement, spans);
        }
    }

    fn collect_match_spans_statement(statement: &Statement, spans: &mut Vec<Span>) {
        match &statement.kind {
            StatementKind::Expression(expr) => collect_match_spans_expression(expr, spans),
            StatementKind::Let(let_stmt) => {
                if let Some(value) = &let_stmt.value {
                    collect_match_spans_expression(value, spans);
                }
            }
            StatementKind::Assignment(assign) => {
                collect_match_spans_expression(&assign.value, spans);
            }
            StatementKind::Return(ret) => {
                if let Some(value) = &ret.value {
                    collect_match_spans_expression(value, spans);
                }
            }
            StatementKind::While(while_loop) => {
                collect_match_spans_expression(&while_loop.condition, spans);
                collect_match_spans_block(&while_loop.body, spans);
            }
            StatementKind::DoWhile(do_while) => {
                collect_match_spans_block(&do_while.body, spans);
                collect_match_spans_expression(&do_while.condition, spans);
            }
            StatementKind::For(for_loop) => {
                collect_match_spans_expression(&for_loop.iterable, spans);
                collect_match_spans_block(&for_loop.body, spans);
            }
            StatementKind::Loop(loop_stmt) => collect_match_spans_block(&loop_stmt.body, spans),
            StatementKind::Switch(switch_stmt) => {
                collect_match_spans_expression(&switch_stmt.value, spans);
                for case in &switch_stmt.cases {
                    collect_match_spans_block(&case.body, spans);
                }
                if let Some(default) = &switch_stmt.default {
                    collect_match_spans_block(default, spans);
                }
            }
            StatementKind::Break | StatementKind::Continue => {}
            StatementKind::IfLet(stmt) => {
                collect_match_spans_expression(&stmt.value, spans);
                collect_match_spans_block(&stmt.then_block, spans);
                if let Some(else_b) = &stmt.else_block {
                    collect_match_spans_block(else_b, spans);
                }
            }
            StatementKind::WhileLet(stmt) => {
                collect_match_spans_expression(&stmt.value, spans);
                collect_match_spans_block(&stmt.body, spans);
            }
        }
    }

    fn collect_match_spans_expression(expr: &Expression, spans: &mut Vec<Span>) {
        match &expr.kind {
            ExpressionKind::Binary { left, right, .. } => {
                collect_match_spans_expression(left, spans);
                collect_match_spans_expression(right, spans);
            }
            ExpressionKind::Unary { operand, .. } | ExpressionKind::Await(operand) => {
                collect_match_spans_expression(operand, spans)
            }
            ExpressionKind::Call { callee, arguments } => {
                collect_match_spans_expression(callee, spans);
                for arg in arguments {
                    collect_match_spans_expression(arg, spans);
                }
            }
            ExpressionKind::If {
                condition,
                then_block,
                elif_blocks,
                else_block,
            } => {
                collect_match_spans_expression(condition, spans);
                collect_match_spans_block(then_block, spans);
                for (expr, block) in elif_blocks {
                    collect_match_spans_expression(expr, spans);
                    collect_match_spans_block(block, spans);
                }
                if let Some(block) = else_block {
                    collect_match_spans_block(block, spans);
                }
            }
            ExpressionKind::Unless {
                condition,
                then_block,
                else_block,
            } => {
                collect_match_spans_expression(condition, spans);
                collect_match_spans_block(then_block, spans);
                if let Some(block) = else_block {
                    collect_match_spans_block(block, spans);
                }
            }
            ExpressionKind::ArrayLiteral { elements }
            | ExpressionKind::TupleLiteral { elements } => {
                for element in elements {
                    collect_match_spans_expression(element, spans);
                }
            }
            ExpressionKind::StructLiteral { fields, .. } => {
                for (_, expr) in fields {
                    collect_match_spans_expression(expr, spans);
                }
            }
            ExpressionKind::EnumVariant {
                data, struct_data, ..
            } => {
                if let Some(elements) = data {
                    for expr in elements {
                        collect_match_spans_expression(expr, spans);
                    }
                }
                if let Some(fields) = struct_data {
                    for (_, expr) in fields {
                        collect_match_spans_expression(expr, spans);
                    }
                }
            }
            ExpressionKind::Match { scrutinee, arms } => {
                spans.push(expr.span);
                collect_match_spans_expression(scrutinee, spans);
                for arm in arms {
                    collect_match_spans_expression(&arm.body, spans);
                }
            }
            ExpressionKind::MethodCall {
                object, arguments, ..
            } => {
                collect_match_spans_expression(object, spans);
                for arg in arguments {
                    collect_match_spans_expression(arg, spans);
                }
            }
            ExpressionKind::Grouping(inner)
            | ExpressionKind::IndexAccess { array: inner, .. }
            | ExpressionKind::FieldAccess { object: inner, .. }
            | ExpressionKind::TupleAccess { tuple: inner, .. } => {
                collect_match_spans_expression(inner, spans);
            }
            ExpressionKind::NumberLiteral(_)
            | ExpressionKind::StringLiteral(_)
            | ExpressionKind::BoolLiteral(_)
            | ExpressionKind::Identifier(_) => {}
            ExpressionKind::CharLiteral(_) => {}
            ExpressionKind::FString(parts) => {
                for part in parts {
                    if let spectra_compiler::ast::FStringPart::Interpolated(expr) = part {
                        collect_match_spans_expression(expr, spans);
                    }
                }
            }
            ExpressionKind::Lambda { body, .. } => {
                collect_match_spans_expression(body, spans);
            }
            ExpressionKind::Try(inner) => {
                collect_match_spans_expression(inner, spans);
            }
            ExpressionKind::Range { start, end, .. } => {
                collect_match_spans_expression(start, spans);
                collect_match_spans_expression(end, spans);
            }
            ExpressionKind::Cast { expr: inner, .. } => {
                collect_match_spans_expression(inner, spans);
            }
            ExpressionKind::Block(block) => {
                collect_match_spans_block(block, spans);
            }
            ExpressionKind::DifferentiableBlock(block) | ExpressionKind::AsyncBlock(block) => {
                collect_match_spans_block(block, spans);
            }
        }
    }

    fn rewrite_simple_match_arms(
        lines: &mut [FormattedLine],
        tokens: &[Token],
        source: &str,
        span: &Span,
        line_offsets: &[usize],
    ) {
        let (start_index, end_index) = match token_range_for_span(tokens, span) {
            Some(range) => range,
            None => return,
        };

        let mut brace_depth = 0usize;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut in_match_body = false;

        let mut current_arm_start = None;
        let mut current_arm: Option<(Range<usize>, usize)> = None;
        let mut last_token_end = span.start;

        for token in &tokens[start_index..=end_index] {
            match &token.kind {
                TokenKind::Symbol('{') => {
                    if in_match_body {
                        brace_depth += 1;
                    } else {
                        in_match_body = true;
                        brace_depth = 1;
                        current_arm_start = Some(token.span.end);
                    }
                }
                TokenKind::Symbol('}') => {
                    brace_depth = brace_depth.saturating_sub(1);
                    if in_match_body && brace_depth == 0 {
                        if let Some((pattern_range, arrow_end)) = current_arm.take() {
                            finalize_arm(
                                lines,
                                source,
                                &pattern_range,
                                arrow_end,
                                token.span.start,
                                false,
                                line_offsets,
                            );
                        }
                        break;
                    }
                }
                TokenKind::Symbol('(') => paren_depth += 1,
                TokenKind::Symbol(')') => {
                    paren_depth = paren_depth.saturating_sub(1);
                }
                TokenKind::Symbol('[') => bracket_depth += 1,
                TokenKind::Symbol(']') => {
                    bracket_depth = bracket_depth.saturating_sub(1);
                }
                TokenKind::Operator(Operator::FatArrow)
                    if in_match_body
                        && brace_depth == 1
                        && paren_depth == 0
                        && bracket_depth == 0 =>
                {
                    let start = current_arm_start.unwrap_or(last_token_end);
                    let pattern_range = Range {
                        start,
                        end: token.span.start,
                    };
                    current_arm = Some((pattern_range, token.span.end));
                }
                TokenKind::Symbol(',')
                    if in_match_body
                        && brace_depth == 1
                        && paren_depth == 0
                        && bracket_depth == 0 =>
                {
                    if let Some((pattern_range, arrow_end)) = current_arm.take() {
                        finalize_arm(
                            lines,
                            source,
                            &pattern_range,
                            arrow_end,
                            token.span.start,
                            true,
                            line_offsets,
                        );
                        current_arm_start = Some(token.span.end);
                    }
                }
                _ => {}
            }

            last_token_end = token.span.end;
        }
    }

    fn token_range_for_span(tokens: &[Token], span: &Span) -> Option<(usize, usize)> {
        let start_index = tokens
            .iter()
            .position(|token| token.span.end > span.start)?;
        let mut end_index = start_index;
        while end_index < tokens.len() && tokens[end_index].span.start < span.end {
            end_index += 1;
        }
        if end_index == 0 {
            return None;
        }
        Some((start_index, end_index.saturating_sub(1)))
    }

    fn finalize_arm(
        lines: &mut [FormattedLine],
        source: &str,
        pattern_range: &Range<usize>,
        arrow_end: usize,
        body_end_start: usize,
        had_comma: bool,
        line_offsets: &[usize],
    ) {
        let pattern_range = trim_range(source, pattern_range.clone());
        if pattern_range.start >= pattern_range.end {
            return;
        }
        let body_range = trim_range(
            source,
            Range {
                start: arrow_end,
                end: body_end_start,
            },
        );
        if body_range.start >= body_range.end {
            return;
        }

        let pattern_text = &source[pattern_range.clone()];
        let body_text = &source[body_range.clone()];
        if pattern_text.contains('\n') || body_text.contains('\n') {
            return;
        }

        let arrow_line = match line_index_from_offset(pattern_range.end, line_offsets) {
            Some(index) if index < lines.len() => index,
            _ => return,
        };

        let mut rebuilt = String::new();
        rebuilt.push_str(pattern_text.trim());
        rebuilt.push_str(" => ");
        rebuilt.push_str(body_text.trim());
        if had_comma && !rebuilt.trim_end().ends_with(',') {
            rebuilt.push(',');
        } else if !had_comma && rebuilt.trim_end().ends_with(',') {
            let mut trimmed = rebuilt
                .trim_end_matches(|ch: char| ch == ',' || ch.is_whitespace())
                .to_string();
            if pattern_text.trim().ends_with(',') {
                trimmed.push(',');
            }
            rebuilt = trimmed;
        }

        if let Some(target) = lines.get_mut(arrow_line) {
            if !target.content.contains("=>") {
                return;
            }
            target.content = rebuilt;
        }
    }

    fn trim_range(source: &str, range: Range<usize>) -> Range<usize> {
        if range.start >= range.end {
            return range;
        }
        let slice = &source[range.clone()];
        let trimmed_start = slice.trim_start();
        let leading = slice.len().saturating_sub(trimmed_start.len());
        let trimmed = trimmed_start.trim_end();
        let trailing = trimmed_start.len().saturating_sub(trimmed.len());
        Range {
            start: range.start + leading,
            end: range.end.saturating_sub(trailing),
        }
    }

    fn item_span(item: &Item) -> &Span {
        match item {
            Item::Import(Import { span, .. }) => span,
            Item::Function(Function { span, .. }) => span,
            Item::Struct(Struct { span, .. }) => span,
            Item::Enum(Enum { span, .. }) => span,
            Item::Impl(ImplBlock { span, .. }) => span,
            Item::Trait(TraitDeclaration { span, .. }) => span,
            Item::TraitImpl(TraitImpl { span, .. }) => span,
            Item::TypeAlias(ta) => &ta.span,
            Item::Const(c) => &c.span,
            Item::Static(s) => &s.span,
        }
    }
