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

fn embedded_stdlib_modules() -> &'static [(
    spectra_compiler::EmbeddedStdlibSource,
    spectra_compiler::ast::Module,
)] {
    static MODULES: OnceLock<
        Vec<(
            spectra_compiler::EmbeddedStdlibSource,
            spectra_compiler::ast::Module,
        )>,
    > = OnceLock::new();
    MODULES.get_or_init(|| {
        spectra_compiler::embedded_stdlib_sources()
            .iter()
            .filter_map(|source| {
                let tokens = spectra_compiler::Lexer::new(source.source)
                    .tokenize()
                    .ok()?;
                let module = spectra_compiler::Parser::new(tokens).parse().ok()?;
                (module.name == source.module).then_some((*source, module))
            })
            .collect()
    })
}

fn embedded_stdlib_completion_items() -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for (source, module) in embedded_stdlib_modules() {
        items.push(CompletionItem {
            label: source.module.to_string(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some(format!("bundled Spectra module ({})", source.display_path)),
            ..Default::default()
        });
        for function in module.items.iter().filter_map(|item| match item {
            spectra_compiler::ast::Item::Function(function)
                if function.visibility == spectra_compiler::ast::Visibility::Public =>
            {
                Some(function)
            }
            _ => None,
        }) {
            items.push(CompletionItem {
                label: format!("{}.{}", source.module, function.name),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some(format_function_signature(function)),
                documentation: Some(Documentation::String(format!(
                    "Implemented in `{}`",
                    source.display_path
                ))),
                ..Default::default()
            });
        }
    }
    items
}

fn embedded_stdlib_function(
    module_name: &str,
    function_name: &str,
) -> Option<(
    spectra_compiler::EmbeddedStdlibSource,
    spectra_compiler::ast::Function,
)> {
    let (source, module) = embedded_stdlib_modules()
        .iter()
        .find(|(source, _)| source.module == module_name)?;
    let function = module.items.iter().find_map(|item| match item {
        spectra_compiler::ast::Item::Function(function)
            if function.name == function_name
                && function.visibility == spectra_compiler::ast::Visibility::Public =>
        {
            Some(function.clone())
        }
        _ => None,
    })?;
    Some((*source, function))
}

fn imported_embedded_stdlib_function_at(
    document: &DocumentState,
    position: Position,
) -> Option<(
    spectra_compiler::EmbeddedStdlibSource,
    spectra_compiler::ast::Function,
)> {
    let module = document.analysis.module.as_ref()?;
    let identifier = identifier_at_position(&document.text, position)?;
    let mut identifier_offset =
        position_to_offset(&document.text, position).min(document.text.len());
    let bytes = document.text.as_bytes();
    while identifier_offset > 0 && is_identifier_byte(bytes[identifier_offset - 1]) {
        identifier_offset -= 1;
    }
    let prefix = document.text.get(..identifier_offset)?.trim_end();

    for import in module.items.iter().filter_map(|item| match item {
        spectra_compiler::ast::Item::Import(import) => Some(import),
        _ => None,
    }) {
        let module_name = import.path.join(".");
        if import.names.is_some() {
            let imported_name = import.names.as_ref()?.iter().find_map(|name| {
                (name.alias.as_deref().unwrap_or(&name.name) == identifier)
                    .then_some(name.name.as_str())
            });
            if let Some(imported_name) = imported_name {
                if let Some(function) = embedded_stdlib_function(&module_name, imported_name) {
                    return Some(function);
                }
            }
            continue;
        }

        let leaf = import.path.last().map(String::as_str).unwrap_or("");
        let binding = import.alias.as_deref().unwrap_or(leaf);
        let qualified_prefixes = [format!("{binding}."), format!("{module_name}.")];
        if qualified_prefixes
            .iter()
            .any(|qualified| prefix.ends_with(qualified))
        {
            if let Some(function) = embedded_stdlib_function(&module_name, &identifier) {
                return Some(function);
            }
        }
    }
    None
}

fn embedded_stdlib_definition_location_at(
    document: &DocumentState,
    position: Position,
) -> Option<Location> {
    let (source, function) = imported_embedded_stdlib_function_at(document, position)?;
    let path = embedded_stdlib_source_path(source)?;
    let uri = Url::from_file_path(path).ok()?;
    Some(Location {
        uri,
        range: span_to_range(function.span),
    })
}

fn embedded_stdlib_source_path(source: spectra_compiler::EmbeddedStdlibSource) -> Option<PathBuf> {
    let development_path = PathBuf::from(source.path);
    if development_path.is_file() {
        return Some(development_path);
    }

    let cache_root = std::env::temp_dir()
        .join("spectralang")
        .join("stdlib")
        .join(spectra_compiler::embedded_stdlib_bundle_id());
    materialize_embedded_stdlib_source_at(source, &cache_root)
}

fn materialize_embedded_stdlib_source_at(
    source: spectra_compiler::EmbeddedStdlibSource,
    cache_root: &std::path::Path,
) -> Option<PathBuf> {
    let relative_path = std::path::Path::new(source.display_path);
    if relative_path
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }
    let path = cache_root.join(relative_path);
    let parent = path.parent()?;
    std::fs::create_dir_all(parent).ok()?;
    let existing = std::fs::read_to_string(&path).ok();
    if existing.as_deref() != Some(source.source) {
        std::fs::write(&path, source.source).ok()?;
    }
    Some(path)
}

fn embedded_stdlib_hover_at(document: &DocumentState, position: Position) -> Option<Hover> {
    let (source, function) = imported_embedded_stdlib_function_at(document, position)?;
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!(
                "```spectra\npublic {}\n```\n\nMódulo: `{}`\n\nFonte: `{}`",
                format_function_signature(&function),
                source.module,
                source.display_path
            ),
        }),
        range: Some(span_to_range(function.span)),
    })
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

/// Frequent-pattern completion snippets. Only the items built here carry
/// `InsertTextFormat::SNIPPET` (2); plain keyword/std-api/symbol completions
/// stay plaintext.
///
/// Documented list (bodies use the canonical surface):
/// - `func`        declaration template with `$1`/`$2` params and returns
/// - `record`      record declaration with name/field placeholders
/// - `enum`        enum declaration with variant placeholder
/// - `match`       match expression using canonical `when .. then` /
///   `otherwise` arms
/// - `if let`      if-let statement with pattern and scrutinee placeholders
/// - `for`         for-in loop over a collection
/// - `async block` async block expression
///
/// There is deliberately no `test` snippet: the language surface has no test
/// item or keyword (`KEYWORDS` / `ast::Item` have none), so any body would be
/// invented syntax.
fn snippet_completion_items() -> Vec<CompletionItem> {
    fn snippet(label: &str, detail: &str, body: &str) -> CompletionItem {
        CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some(detail.to_string()),
            insert_text: Some(body.to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        }
    }

    vec![
        snippet(
            "func",
            "function template",
            "func ${1:name}(${2:param}: int) returns ${3:int} {\n    $0\n}",
        ),
        snippet(
            "record",
            "record template",
            "record ${1:Name} {\n    ${2:field}: int\n    $0\n}",
        ),
        snippet(
            "enum",
            "enum template",
            "enum ${1:Name} {\n    ${2:Variant},\n    $0\n}",
        ),
        snippet(
            "match",
            "match with when/otherwise arms",
            "match ${1:value} {\n    when ${2:pattern} then ${3:expr}\n    otherwise ${4:expr}\n    $0\n}",
        ),
        snippet(
            "if let",
            "if-let statement",
            // Constructor patterns need the qualified `Enum::Variant(data)`
            // form, and the scrutinee placeholder uses a call shape because
            // `identifier { ... }` is ambiguous with a struct literal while
            // the body is still empty.
            "if let ${1:Enum::Variant(value)} = ${2:compute()} {\n    $0\n}",
        ),
        snippet(
            "for",
            "for-in loop",
            // Call-shaped iterable: `identifier { ... }` would be ambiguous
            // with a struct literal while the loop body is still empty.
            "for ${1:item} in ${2:items()} {\n    $0\n}",
        ),
        snippet("async block", "async block expression", "async {\n    $0\n}"),
    ]
}
