#[cfg(test)]
mod tests {
    use super::*;

    fn analyzed_document(source: &str) -> DocumentState {
        DocumentState {
            text: source.to_string(),
            analysis: analyze_document(
                source,
                "rename_test.spectra",
                &CompilationOptions::default(),
                None,
            ),
        }
    }

    fn apply_text_edits(source: &str, edits: &[TextEdit]) -> String {
        let mut ranges = edits
            .iter()
            .map(|edit| {
                (
                    position_to_offset(source, edit.range.start),
                    position_to_offset(source, edit.range.end),
                    edit.new_text.clone(),
                )
            })
            .collect::<Vec<_>>();
        ranges.sort_by_key(|range| std::cmp::Reverse(range.0));

        let mut result = source.to_string();
        for (start, end, replacement) in ranges {
            result.replace_range(start..end, &replacement);
        }
        result
    }

    #[test]
    fn rename_identifier_validation_rejects_keywords_and_invalid_names() {
        assert!(is_valid_rename_identifier("renamed_value"));
        assert!(is_valid_rename_identifier("_hidden1"));
        assert!(!is_valid_rename_identifier("1bad"));
        assert!(!is_valid_rename_identifier("two words"));
        assert!(!is_valid_rename_identifier("func"));
        assert!(!is_valid_rename_identifier("module"));
    }

    #[test]
    fn std_api_completion_items_cover_modules_types_and_functions() {
        let items = std_api_completion_items();
        assert!(items.iter().any(|item| {
            item.label == "std.api.http" && item.kind == Some(CompletionItemKind::MODULE)
        }));
        assert!(items.iter().any(|item| {
            item.label == "std.api.http.Request" && item.kind == Some(CompletionItemKind::STRUCT)
        }));
        assert!(items.iter().any(|item| {
            item.label == "std.api.routing.get"
                && item.kind == Some(CompletionItemKind::FUNCTION)
                && item.detail.as_deref().unwrap_or("").contains("func(Router")
        }));
        assert!(items.iter().any(|item| {
            item.label == "std.api.cors.middleware"
                && item.kind == Some(CompletionItemKind::FUNCTION)
        }));
        assert!(items.iter().any(|item| {
            item.label == "std.api.middleware.execute_async"
                && item.kind == Some(CompletionItemKind::FUNCTION)
        }));
    }

    #[test]
    fn keyword_completion_items_cover_current_language_surface() {
        for keyword in [
            "async", "await", "export", "from", "public", "record", "when", "then",
            "otherwise", "and", "or", "not", "yield", "goto",
        ] {
            assert!(KEYWORDS.contains(&keyword), "missing keyword {keyword}");
        }
    }

    #[test]
    fn route_completion_items_cover_detected_api_routes() {
        let source = r#"
module api_routes

public func build() {
    let router = std.api.routing.router()
    let users = std.api.routing.get(router, "/users")
    let created = std.api.routing.post(router, "/users")
}
"#;
        let document = analyzed_document(source);
        let module = document.analysis.module.as_ref().expect("parsed module");
        let items = route_completion_items(module);

        assert!(items.iter().any(|item| {
            item.label == "GET /users" && item.kind == Some(CompletionItemKind::REFERENCE)
        }));
        assert!(items.iter().any(|item| {
            item.label == "POST /users" && item.kind == Some(CompletionItemKind::REFERENCE)
        }));
    }

    #[test]
    fn api_route_hover_shows_signature_method_and_path() {
        let source = r#"
module api_routes

public func build() {
    let router = std.api.routing.router()
    let users = std.api.routing.get(router, "/users")
}
"#;
        let document = analyzed_document(source);
        let module = document.analysis.module.as_ref().expect("parsed module");
        let call_site = find_call_site(module, 6, 35).expect("route call site");
        let hover = api_call_hover(&document, &call_site).expect("api hover");
        let HoverContents::Markup(markup) = hover.contents else {
            panic!("expected markup hover");
        };

        assert!(markup.value.contains("std.api.routing.get"));
        assert!(markup.value.contains("Route: `GET /users`"));
        assert!(markup.value.contains("Return: `Route`"));
    }

    #[test]
    fn async_handler_definition_keys_support_workspace_references() {
        assert_eq!(
            definition_key("async func handle(request: Request) returns Response"),
            "handle"
        );
        assert_eq!(
            definition_key("async func Api::handle(&self, request: Request) returns Response"),
            "Api::handle"
        );
    }

    #[test]
    fn workspace_definition_locations_resolve_async_handler_across_files() {
        let handler_source = r#"
module handlers

public async func handle(request: std.api.http.Request)  returns  std.api.http.Response {
    return std.api.handler.json("{}")
}
"#;
        let handler_analysis = analyzed_document(handler_source);
        let handler_module = handler_analysis
            .analysis
            .module
            .as_ref()
            .expect("handler module");
        let handler_uri = Url::parse("file:///workspace/handlers.spectra").unwrap();
        let route_uri = Url::parse("file:///workspace/routes.spectra").unwrap();
        let mut cache = HashMap::new();
        cache.insert(
            handler_uri.clone(),
            WorkspaceCacheEntry {
                symbols: workspace_symbol_entries_for_module(handler_source, handler_module),
                references: Vec::new(),
                modified: None,
            },
        );
        cache.insert(
            route_uri,
            WorkspaceCacheEntry {
                symbols: Vec::new(),
                references: Vec::new(),
                modified: None,
            },
        );

        let locations = workspace_definition_locations(&cache, "handle");
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].uri, handler_uri);
        assert_eq!(locations[0].range.start.line, 3);
    }

    #[test]
    fn rename_edits_cover_local_definition_and_uses() {
        let source = "module rename_test\n\npublic func main() returns int {\n    let value = 1\n    let next = value + value\n    return next\n}\n";
        let document = analyzed_document(source);
        let symbol = document
            .analysis
            .symbol_at(5, 17)
            .expect("symbol at value use");
        let edits = rename_edits_for_document(&document, &symbol, "value", "renamed");
        let result = apply_text_edits(source, &edits);

        assert_eq!(edits.len(), 3);
        assert!(result.contains("let renamed = 1"));
        assert!(result.contains("let next = renamed + renamed"));
        assert!(!result.contains("value"));
    }

    #[test]
    fn rename_edits_do_not_touch_identifier_substrings() {
        let source = "module rename_test\n\npublic func main() returns int {\n    let value = 1\n    let value_extra = 2\n    return value + value_extra\n}\n";
        let document = analyzed_document(source);
        let symbol = document
            .analysis
            .symbol_at(6, 12)
            .expect("symbol at value use");
        let edits = rename_edits_for_document(&document, &symbol, "value", "renamed");
        let result = apply_text_edits(source, &edits);

        assert!(result.contains("let renamed = 1"));
        assert!(result.contains("let value_extra = 2"));
        assert!(result.contains("return renamed + value_extra"));
    }
}
