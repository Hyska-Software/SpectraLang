fn api_call_hover(document: &DocumentState, call_site: &CallSite) -> Option<Hover> {
    let call_expr = expression_at_span(document.analysis.module.as_ref()?, call_site.span)?;
    let path = match &call_expr.kind {
        spectra_compiler::ast::ExpressionKind::Call { callee, .. } => expression_path(callee)?,
        spectra_compiler::ast::ExpressionKind::MethodCall {
            object,
            method_name,
            ..
        } => format!("{}.{}", expression_path(object)?, method_name),
        _ => return None,
    };
    if !path.starts_with("std.api.") {
        return None;
    }
    let signature = spectra_compiler::semantic::builtin_modules::STD_API_PUBLIC_FUNCTIONS
        .iter()
        .find_map(|(name, signature)| (*name == path).then_some(*signature))?;
    let mut value = format!(
        "```spectra\n{}: {}\n```\n\nModule: `{}`\n\nReturn: `{}`",
        path,
        signature,
        path.rsplit_once('.')
            .map(|(module, _)| module)
            .unwrap_or("std.api"),
        signature_return_type(signature).unwrap_or("unknown")
    );
    if let Some(route) = api_route_from_expression(call_expr) {
        value.push_str(&format!(
            "\n\nRoute: `{}`\n\nMethod: `{}`\n\nPath: `{}`",
            route.label(),
            route.method,
            route.path
        ));
    }
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(span_to_range(call_site.span)),
    })
}

fn signature_return_type(signature: &str) -> Option<&str> {
    signature
        .rsplit_once(" returns ")
        .map(|(_, ty)| ty.trim())
}

fn expression_at_span(
    module: &spectra_compiler::ast::Module,
    target: Span,
) -> Option<&spectra_compiler::ast::Expression> {
    for item in &module.items {
        let found = match item {
            spectra_compiler::ast::Item::Function(function) => {
                expression_at_span_in_block(&function.body, target)
            }
            spectra_compiler::ast::Item::Impl(impl_block) => impl_block
                .methods
                .iter()
                .find_map(|method| expression_at_span_in_block(&method.body, target)),
            spectra_compiler::ast::Item::Trait(trait_def) => {
                trait_def.methods.iter().find_map(|method| {
                    method
                        .body
                        .as_ref()
                        .and_then(|body| expression_at_span_in_block(body, target))
                })
            }
            spectra_compiler::ast::Item::TraitImpl(trait_impl) => trait_impl
                .methods
                .iter()
                .find_map(|method| expression_at_span_in_block(&method.body, target)),
            spectra_compiler::ast::Item::Import(_)
            | spectra_compiler::ast::Item::Struct(_)
            | spectra_compiler::ast::Item::Enum(_)
            | spectra_compiler::ast::Item::TypeAlias(_)
            | spectra_compiler::ast::Item::Const(_)
            | spectra_compiler::ast::Item::Static(_) => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

fn expression_at_span_in_block(
    block: &spectra_compiler::ast::Block,
    target: Span,
) -> Option<&spectra_compiler::ast::Expression> {
    block
        .statements
        .iter()
        .find_map(|statement| expression_at_span_in_statement(statement, target))
}

fn expression_at_span_in_statement(
    statement: &spectra_compiler::ast::Statement,
    target: Span,
) -> Option<&spectra_compiler::ast::Expression> {
    match &statement.kind {
        spectra_compiler::ast::StatementKind::Let(let_stmt) => let_stmt
            .value
            .as_ref()
            .and_then(|value| expression_at_span_in_expression(value, target)),
        spectra_compiler::ast::StatementKind::Assignment(assign_stmt) => {
            expression_at_span_in_expression(&assign_stmt.value, target).or_else(|| {
                match &assign_stmt.target {
                    spectra_compiler::ast::LValue::IndexAccess { array, index } => {
                        expression_at_span_in_expression(array, target)
                            .or_else(|| expression_at_span_in_expression(index, target))
                    }
                    spectra_compiler::ast::LValue::FieldAccess { object, .. } => {
                        expression_at_span_in_expression(object, target)
                    }
                    spectra_compiler::ast::LValue::Identifier(_) => None,
                }
            })
        }
        spectra_compiler::ast::StatementKind::Return(ret_stmt) => ret_stmt
            .value
            .as_ref()
            .and_then(|value| expression_at_span_in_expression(value, target)),
        spectra_compiler::ast::StatementKind::Expression(expr) => {
            expression_at_span_in_expression(expr, target)
        }
        spectra_compiler::ast::StatementKind::While(loop_stmt) => {
            expression_at_span_in_expression(&loop_stmt.condition, target)
                .or_else(|| expression_at_span_in_block(&loop_stmt.body, target))
        }
        spectra_compiler::ast::StatementKind::DoWhile(loop_stmt) => {
            expression_at_span_in_block(&loop_stmt.body, target)
                .or_else(|| expression_at_span_in_expression(&loop_stmt.condition, target))
        }
        spectra_compiler::ast::StatementKind::For(loop_stmt) => {
            expression_at_span_in_expression(&loop_stmt.iterable, target)
                .or_else(|| expression_at_span_in_block(&loop_stmt.body, target))
        }
        spectra_compiler::ast::StatementKind::Loop(loop_stmt) => {
            expression_at_span_in_block(&loop_stmt.body, target)
        }
        spectra_compiler::ast::StatementKind::Switch(switch_stmt) => {
            expression_at_span_in_expression(&switch_stmt.value, target)
                .or_else(|| {
                    switch_stmt.cases.iter().find_map(|case| {
                        expression_at_span_in_expression(&case.pattern, target)
                            .or_else(|| expression_at_span_in_block(&case.body, target))
                    })
                })
                .or_else(|| {
                    switch_stmt
                        .default
                        .as_ref()
                        .and_then(|block| expression_at_span_in_block(block, target))
                })
        }
        spectra_compiler::ast::StatementKind::IfLet(if_let_stmt) => {
            expression_at_span_in_expression(&if_let_stmt.value, target)
                .or_else(|| expression_at_span_in_block(&if_let_stmt.then_block, target))
                .or_else(|| {
                    if_let_stmt
                        .else_block
                        .as_ref()
                        .and_then(|block| expression_at_span_in_block(block, target))
                })
        }
        spectra_compiler::ast::StatementKind::WhileLet(while_let_stmt) => {
            expression_at_span_in_expression(&while_let_stmt.value, target)
                .or_else(|| expression_at_span_in_block(&while_let_stmt.body, target))
        }
        spectra_compiler::ast::StatementKind::Break
        | spectra_compiler::ast::StatementKind::Continue => None,
    }
}

fn expression_at_span_in_expression(
    expr: &spectra_compiler::ast::Expression,
    target: Span,
) -> Option<&spectra_compiler::ast::Expression> {
    if expr.span == target {
        return Some(expr);
    }

    match &expr.kind {
        spectra_compiler::ast::ExpressionKind::Call { callee, arguments } => {
            expression_at_span_in_expression(callee, target).or_else(|| {
                arguments
                    .iter()
                    .find_map(|arg| expression_at_span_in_expression(arg, target))
            })
        }
        spectra_compiler::ast::ExpressionKind::MethodCall {
            object, arguments, ..
        } => expression_at_span_in_expression(object, target).or_else(|| {
            arguments
                .iter()
                .find_map(|arg| expression_at_span_in_expression(arg, target))
        }),
        spectra_compiler::ast::ExpressionKind::Binary { left, right, .. } => {
            expression_at_span_in_expression(left, target)
                .or_else(|| expression_at_span_in_expression(right, target))
        }
        spectra_compiler::ast::ExpressionKind::Unary { operand, .. }
        | spectra_compiler::ast::ExpressionKind::Try(operand)
        | spectra_compiler::ast::ExpressionKind::Await(operand)
        | spectra_compiler::ast::ExpressionKind::Grouping(operand) => {
            expression_at_span_in_expression(operand, target)
        }
        spectra_compiler::ast::ExpressionKind::Range { start, end, .. }
        | spectra_compiler::ast::ExpressionKind::IndexAccess {
            array: start,
            index: end,
        } => expression_at_span_in_expression(start, target)
            .or_else(|| expression_at_span_in_expression(end, target)),
        spectra_compiler::ast::ExpressionKind::If {
            condition,
            then_block,
            elif_blocks,
            else_block,
        } => expression_at_span_in_expression(condition, target)
            .or_else(|| expression_at_span_in_block(then_block, target))
            .or_else(|| {
                elif_blocks.iter().find_map(|(elif_expr, elif_block)| {
                    expression_at_span_in_expression(elif_expr, target)
                        .or_else(|| expression_at_span_in_block(elif_block, target))
                })
            })
            .or_else(|| {
                else_block
                    .as_ref()
                    .and_then(|block| expression_at_span_in_block(block, target))
            }),
        spectra_compiler::ast::ExpressionKind::Unless {
            condition,
            then_block,
            else_block,
        } => expression_at_span_in_expression(condition, target)
            .or_else(|| expression_at_span_in_block(then_block, target))
            .or_else(|| {
                else_block
                    .as_ref()
                    .and_then(|block| expression_at_span_in_block(block, target))
            }),
        spectra_compiler::ast::ExpressionKind::ArrayLiteral { elements }
        | spectra_compiler::ast::ExpressionKind::TupleLiteral { elements } => elements
            .iter()
            .find_map(|element| expression_at_span_in_expression(element, target)),
        spectra_compiler::ast::ExpressionKind::StructLiteral { fields, .. } => fields
            .iter()
            .find_map(|(_, value)| expression_at_span_in_expression(value, target)),
        spectra_compiler::ast::ExpressionKind::FieldAccess { object, .. } => {
            expression_at_span_in_expression(object, target)
        }
        spectra_compiler::ast::ExpressionKind::EnumVariant {
            data, struct_data, ..
        } => data
            .as_ref()
            .and_then(|values| {
                values
                    .iter()
                    .find_map(|value| expression_at_span_in_expression(value, target))
            })
            .or_else(|| {
                struct_data.as_ref().and_then(|values| {
                    values
                        .iter()
                        .find_map(|(_, value)| expression_at_span_in_expression(value, target))
                })
            }),
        spectra_compiler::ast::ExpressionKind::Match { scrutinee, arms } => {
            expression_at_span_in_expression(scrutinee, target).or_else(|| {
                arms.iter()
                    .find_map(|arm| expression_at_span_in_expression(&arm.body, target))
            })
        }
        spectra_compiler::ast::ExpressionKind::Lambda { body, .. } => {
            expression_at_span_in_expression(body, target)
        }
        spectra_compiler::ast::ExpressionKind::Block(block)
        | spectra_compiler::ast::ExpressionKind::DifferentiableBlock(block)
        | spectra_compiler::ast::ExpressionKind::AsyncBlock(block) => {
            expression_at_span_in_block(block, target)
        }
        spectra_compiler::ast::ExpressionKind::Cast { expr, .. } => {
            expression_at_span_in_expression(expr, target)
        }
        spectra_compiler::ast::ExpressionKind::Identifier(_)
        | spectra_compiler::ast::ExpressionKind::NumberLiteral(_)
        | spectra_compiler::ast::ExpressionKind::StringLiteral(_)
        | spectra_compiler::ast::ExpressionKind::BoolLiteral(_)
        | spectra_compiler::ast::ExpressionKind::CharLiteral(_)
        | spectra_compiler::ast::ExpressionKind::FString(_)
        | spectra_compiler::ast::ExpressionKind::TupleAccess { .. } => None,
    }
}

