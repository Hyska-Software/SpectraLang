fn std_api_completion_items() -> Vec<CompletionItem> {
    let mut items = Vec::new();
    items.extend(
        spectra_compiler::semantic::builtin_modules::STD_API_MODULE_PATHS
            .iter()
            .map(|module| CompletionItem {
                label: (*module).to_string(),
                kind: Some(CompletionItemKind::MODULE),
                detail: Some("stable std.api module".to_string()),
                ..Default::default()
            }),
    );
    items.extend(
        spectra_compiler::semantic::builtin_modules::STD_API_PUBLIC_TYPES
            .iter()
            .map(|(label, detail)| CompletionItem {
                label: (*label).to_string(),
                kind: Some(CompletionItemKind::STRUCT),
                detail: Some((*detail).to_string()),
                ..Default::default()
            }),
    );
    items.extend(
        spectra_compiler::semantic::builtin_modules::STD_API_PUBLIC_FUNCTIONS
            .iter()
            .map(|(label, detail)| CompletionItem {
                label: (*label).to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some((*detail).to_string()),
                ..Default::default()
            }),
    );
    items
}

fn route_completion_items(module: &spectra_compiler::ast::Module) -> Vec<CompletionItem> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for route in collect_api_routes(module) {
        let label = route.label();
        if !seen.insert(label.clone()) {
            continue;
        }
        items.push(CompletionItem {
            label,
            kind: Some(CompletionItemKind::REFERENCE),
            detail: Some("spectra.api route".to_string()),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: route.markdown(),
            })),
            ..Default::default()
        });
    }
    items
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApiRouteInfo {
    helper: String,
    method: String,
    path: String,
}

impl ApiRouteInfo {
    fn label(&self) -> String {
        format!("{} {}", self.method, self.path)
    }

    fn markdown(&self) -> String {
        format!(
            "```spectra\nstd.api.routing.{}(...)\n```\n\nRoute: `{}`\n\nMethod: `{}`\n\nPath: `{}`",
            self.helper,
            self.label(),
            self.method,
            self.path
        )
    }
}

fn collect_api_routes(module: &spectra_compiler::ast::Module) -> Vec<ApiRouteInfo> {
    let mut routes = Vec::new();
    for item in &module.items {
        match item {
            spectra_compiler::ast::Item::Function(function) => {
                collect_api_routes_from_block(&function.body, &mut routes);
            }
            spectra_compiler::ast::Item::Impl(impl_block) => {
                for method in &impl_block.methods {
                    collect_api_routes_from_block(&method.body, &mut routes);
                }
            }
            spectra_compiler::ast::Item::Trait(trait_def) => {
                for method in &trait_def.methods {
                    if let Some(body) = &method.body {
                        collect_api_routes_from_block(body, &mut routes);
                    }
                }
            }
            spectra_compiler::ast::Item::TraitImpl(trait_impl) => {
                for method in &trait_impl.methods {
                    collect_api_routes_from_block(&method.body, &mut routes);
                }
            }
            spectra_compiler::ast::Item::Import(_)
            | spectra_compiler::ast::Item::Struct(_)
            | spectra_compiler::ast::Item::Enum(_)
            | spectra_compiler::ast::Item::TypeAlias(_)
            | spectra_compiler::ast::Item::Const(_)
            | spectra_compiler::ast::Item::Static(_) => {}
        }
    }
    routes
}

fn collect_api_routes_from_block(
    block: &spectra_compiler::ast::Block,
    routes: &mut Vec<ApiRouteInfo>,
) {
    for statement in &block.statements {
        collect_api_routes_from_statement(statement, routes);
    }
}

fn collect_api_routes_from_statement(
    statement: &spectra_compiler::ast::Statement,
    routes: &mut Vec<ApiRouteInfo>,
) {
    match &statement.kind {
        spectra_compiler::ast::StatementKind::Let(let_stmt) => {
            if let Some(value) = &let_stmt.value {
                collect_api_routes_from_expression(value, routes);
            }
        }
        spectra_compiler::ast::StatementKind::Assignment(assign_stmt) => {
            collect_api_routes_from_expression(&assign_stmt.value, routes);
            match &assign_stmt.target {
                spectra_compiler::ast::LValue::IndexAccess { array, index } => {
                    collect_api_routes_from_expression(array, routes);
                    collect_api_routes_from_expression(index, routes);
                }
                spectra_compiler::ast::LValue::FieldAccess { object, .. } => {
                    collect_api_routes_from_expression(object, routes);
                }
                spectra_compiler::ast::LValue::Identifier(_) => {}
            }
        }
        spectra_compiler::ast::StatementKind::Return(ret_stmt) => {
            if let Some(value) = &ret_stmt.value {
                collect_api_routes_from_expression(value, routes);
            }
        }
        spectra_compiler::ast::StatementKind::Expression(expr) => {
            collect_api_routes_from_expression(expr, routes);
        }
        spectra_compiler::ast::StatementKind::While(loop_stmt) => {
            collect_api_routes_from_expression(&loop_stmt.condition, routes);
            collect_api_routes_from_block(&loop_stmt.body, routes);
        }
        spectra_compiler::ast::StatementKind::DoWhile(loop_stmt) => {
            collect_api_routes_from_block(&loop_stmt.body, routes);
            collect_api_routes_from_expression(&loop_stmt.condition, routes);
        }
        spectra_compiler::ast::StatementKind::For(loop_stmt) => {
            collect_api_routes_from_expression(&loop_stmt.iterable, routes);
            collect_api_routes_from_block(&loop_stmt.body, routes);
        }
        spectra_compiler::ast::StatementKind::Loop(loop_stmt) => {
            collect_api_routes_from_block(&loop_stmt.body, routes);
        }
        spectra_compiler::ast::StatementKind::Switch(switch_stmt) => {
            collect_api_routes_from_expression(&switch_stmt.value, routes);
            for case in &switch_stmt.cases {
                collect_api_routes_from_expression(&case.pattern, routes);
                collect_api_routes_from_block(&case.body, routes);
            }
            if let Some(default) = &switch_stmt.default {
                collect_api_routes_from_block(default, routes);
            }
        }
        spectra_compiler::ast::StatementKind::IfLet(if_let_stmt) => {
            collect_api_routes_from_expression(&if_let_stmt.value, routes);
            collect_api_routes_from_block(&if_let_stmt.then_block, routes);
            if let Some(else_block) = &if_let_stmt.else_block {
                collect_api_routes_from_block(else_block, routes);
            }
        }
        spectra_compiler::ast::StatementKind::WhileLet(while_let_stmt) => {
            collect_api_routes_from_expression(&while_let_stmt.value, routes);
            collect_api_routes_from_block(&while_let_stmt.body, routes);
        }
        spectra_compiler::ast::StatementKind::Break
        | spectra_compiler::ast::StatementKind::Continue => {}
    }
}

fn collect_api_routes_from_expression(
    expr: &spectra_compiler::ast::Expression,
    routes: &mut Vec<ApiRouteInfo>,
) {
    if let Some(route) = api_route_from_expression(expr) {
        routes.push(route);
    }

    match &expr.kind {
        spectra_compiler::ast::ExpressionKind::Call { callee, arguments } => {
            collect_api_routes_from_expression(callee, routes);
            for arg in arguments {
                collect_api_routes_from_expression(arg, routes);
            }
        }
        spectra_compiler::ast::ExpressionKind::MethodCall {
            object, arguments, ..
        } => {
            collect_api_routes_from_expression(object, routes);
            for arg in arguments {
                collect_api_routes_from_expression(arg, routes);
            }
        }
        spectra_compiler::ast::ExpressionKind::Binary { left, right, .. } => {
            collect_api_routes_from_expression(left, routes);
            collect_api_routes_from_expression(right, routes);
        }
        spectra_compiler::ast::ExpressionKind::Unary { operand, .. }
        | spectra_compiler::ast::ExpressionKind::Try(operand)
        | spectra_compiler::ast::ExpressionKind::Await(operand)
        | spectra_compiler::ast::ExpressionKind::Grouping(operand) => {
            collect_api_routes_from_expression(operand, routes);
        }
        spectra_compiler::ast::ExpressionKind::Range { start, end, .. }
        | spectra_compiler::ast::ExpressionKind::IndexAccess {
            array: start,
            index: end,
        } => {
            collect_api_routes_from_expression(start, routes);
            collect_api_routes_from_expression(end, routes);
        }
        spectra_compiler::ast::ExpressionKind::If {
            condition,
            then_block,
            elif_blocks,
            else_block,
        } => {
            collect_api_routes_from_expression(condition, routes);
            collect_api_routes_from_block(then_block, routes);
            for (elif_expr, elif_block) in elif_blocks {
                collect_api_routes_from_expression(elif_expr, routes);
                collect_api_routes_from_block(elif_block, routes);
            }
            if let Some(else_block) = else_block {
                collect_api_routes_from_block(else_block, routes);
            }
        }
        spectra_compiler::ast::ExpressionKind::Unless {
            condition,
            then_block,
            else_block,
        } => {
            collect_api_routes_from_expression(condition, routes);
            collect_api_routes_from_block(then_block, routes);
            if let Some(else_block) = else_block {
                collect_api_routes_from_block(else_block, routes);
            }
        }
        spectra_compiler::ast::ExpressionKind::ArrayLiteral { elements }
        | spectra_compiler::ast::ExpressionKind::TupleLiteral { elements } => {
            for element in elements {
                collect_api_routes_from_expression(element, routes);
            }
        }
        spectra_compiler::ast::ExpressionKind::StructLiteral { fields, .. } => {
            for (_, value) in fields {
                collect_api_routes_from_expression(value, routes);
            }
        }
        spectra_compiler::ast::ExpressionKind::FieldAccess { object, .. } => {
            collect_api_routes_from_expression(object, routes);
        }
        spectra_compiler::ast::ExpressionKind::EnumVariant {
            data, struct_data, ..
        } => {
            if let Some(data) = data {
                for value in data {
                    collect_api_routes_from_expression(value, routes);
                }
            }
            if let Some(struct_data) = struct_data {
                for (_, value) in struct_data {
                    collect_api_routes_from_expression(value, routes);
                }
            }
        }
        spectra_compiler::ast::ExpressionKind::Match { scrutinee, arms } => {
            collect_api_routes_from_expression(scrutinee, routes);
            for arm in arms {
                collect_api_routes_from_expression(&arm.body, routes);
            }
        }
        spectra_compiler::ast::ExpressionKind::Lambda { body, .. } => {
            collect_api_routes_from_expression(body, routes);
        }
        spectra_compiler::ast::ExpressionKind::Block(block)
        | spectra_compiler::ast::ExpressionKind::DifferentiableBlock(block)
        | spectra_compiler::ast::ExpressionKind::AsyncBlock(block) => {
            collect_api_routes_from_block(block, routes);
        }
        spectra_compiler::ast::ExpressionKind::Cast { expr, .. } => {
            collect_api_routes_from_expression(expr, routes);
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

fn api_route_from_expression(expr: &spectra_compiler::ast::Expression) -> Option<ApiRouteInfo> {
    let (path, arguments) = match &expr.kind {
        spectra_compiler::ast::ExpressionKind::Call { callee, arguments } => {
            (expression_path(callee)?, arguments.as_slice())
        }
        spectra_compiler::ast::ExpressionKind::MethodCall {
            object,
            method_name,
            arguments,
            ..
        } => (
            format!("{}.{}", expression_path(object)?, method_name),
            arguments.as_slice(),
        ),
        _ => return None,
    };
    let helper = path.strip_prefix("std.api.routing.")?;
    let method = match helper {
        "get" | "post" | "put" | "patch" | "delete" | "options" => helper.to_ascii_uppercase(),
        "route_add" => route_add_method(arguments.get(1))?,
        _ => return None,
    };
    let path_arg_index = if helper == "route_add" { 2 } else { 1 };
    let route_path = string_literal_value(arguments.get(path_arg_index)?)?;
    Some(ApiRouteInfo {
        helper: helper.to_string(),
        method,
        path: route_path,
    })
}

fn route_add_method(expr: Option<&spectra_compiler::ast::Expression>) -> Option<String> {
    match expr?.kind {
        spectra_compiler::ast::ExpressionKind::NumberLiteral(ref value) => match value.as_str() {
            "1" => Some("GET".to_string()),
            "2" => Some("HEAD".to_string()),
            "3" => Some("POST".to_string()),
            "4" => Some("PUT".to_string()),
            "5" => Some("PATCH".to_string()),
            "6" => Some("DELETE".to_string()),
            "7" => Some("OPTIONS".to_string()),
            _ => Some(format!("METHOD {}", value)),
        },
        _ => Some("METHOD".to_string()),
    }
}

fn string_literal_value(expr: &spectra_compiler::ast::Expression) -> Option<String> {
    match &expr.kind {
        spectra_compiler::ast::ExpressionKind::StringLiteral(value) => Some(value.clone()),
        _ => None,
    }
}

fn expression_path(expr: &spectra_compiler::ast::Expression) -> Option<String> {
    match &expr.kind {
        spectra_compiler::ast::ExpressionKind::Identifier(name) => Some(name.clone()),
        spectra_compiler::ast::ExpressionKind::FieldAccess { object, field } => {
            Some(format!("{}.{}", expression_path(object)?, field))
        }
        _ => None,
    }
}

