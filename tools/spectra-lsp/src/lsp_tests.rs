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

    /// Expands LSP snippet markers so snippet bodies can be checked against
    /// the real parser: `${N:default}` keeps its default text, `$N` and `$0`
    /// vanish.
    fn expand_snippet(body: &str) -> String {
        let mut expanded = String::with_capacity(body.len());
        let mut chars = body.chars().peekable();
        while let Some(current) = chars.next() {
            if current != '$' {
                expanded.push(current);
                continue;
            }
            match chars.peek() {
                Some('{') => {
                    chars.next();
                    let mut inner = String::new();
                    for nested in chars.by_ref() {
                        if nested == '}' {
                            break;
                        }
                        inner.push(nested);
                    }
                    // `N:default` -> default; bare `N` -> nothing.
                    match inner.split_once(':') {
                        Some((_, default)) => expanded.push_str(default),
                        None => {}
                    }
                }
                _ => {
                    // bare tabstop like `$0` or `$1`: drop it.
                    if matches!(chars.peek(), Some(digit) if digit.is_ascii_digit()) {
                        chars.next();
                    } else {
                        expanded.push('$');
                    }
                }
            }
        }
        expanded
    }

    fn placeholder_numbers(body: &str) -> Vec<u32> {
        let mut numbers = Vec::new();
        let bytes = body.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'$' && index + 1 < bytes.len() && bytes[index + 1] == b'{' {
                let mut cursor = index + 2;
                let mut number = 0u32;
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    number = number * 10 + u32::from(bytes[cursor] - b'0');
                    cursor += 1;
                }
                numbers.push(number);
                index = cursor;
            } else {
                index += 1;
            }
        }
        numbers
    }

    #[test]
    fn snippet_completions_use_snippet_format_with_ordered_placeholders() {
        let items = snippet_completion_items();
        assert!(items.len() >= 7, "expected the documented snippet set");

        for item in &items {
            assert_eq!(
                item.insert_text_format,
                Some(InsertTextFormat::SNIPPET),
                "snippet '{}' must declare InsertTextFormat=2",
                item.label
            );
            assert_eq!(item.kind, Some(CompletionItemKind::SNIPPET));
            let body = item.insert_text.as_deref().unwrap_or_default();
            assert!(body.contains("$0"), "snippet '{}' lacks a final tabstop", item.label);
            let numbers = placeholder_numbers(body);
            if numbers.is_empty() {
                // Tabstop-only snippet (e.g. `async block`); nothing to order.
                continue;
            }
            let mut sorted = numbers.clone();
            sorted.sort_unstable();
            assert_eq!(
                (numbers.first().copied(), sorted.first().copied()),
                (Some(1), Some(1)),
                "snippet '{}' must start at ${{1:..}}",
                item.label
            );
        }

        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        for expected in ["func", "record", "enum", "match", "if let", "for"] {
            assert!(labels.contains(&expected), "missing snippet '{expected}'");
        }

        // Plain keyword completions stay plaintext (no InsertTextFormat).
        let keyword_item = CompletionItem {
            label: "func".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..Default::default()
        };
        assert_eq!(keyword_item.insert_text_format, None);
    }

    #[test]
    fn snippet_bodies_expand_to_parseable_spectra() {
        for item in snippet_completion_items() {
            let body = item.insert_text.as_deref().expect("snippet text");
            let expanded = expand_snippet(body);
            // Declaration snippets (record/enum) live at module level;
            // statement/expression snippets (match/if let/for/async block)
            // live inside a function body. Either context must parse.
            let top_level = format!("module snippet_check\n\n{}\n", expanded);
            let in_function = format!(
                "module snippet_check\n\npublic func main() returns int {{\n{}\n    return 0\n}}\n",
                expanded
            );
            let top = analyzed_document(&top_level).analysis.module.is_some();
            let inner = analyzed_document(&in_function).analysis.module.is_some();
            assert!(
                top || inner,
                "snippet '{}' does not parse in any context.\nexpanded:\n{}",
                item.label,
                expanded
            );
        }
    }


    #[test]
    fn will_rename_updates_imports_in_open_documents() {
        let consumer_source = "module consumer\n\nfrom util import helper\n";
        let namespace_source = "module namespace_user\n\nimport util as u\n";
        let renamed_source = "module util\n\npublic func helper() returns unit {\n}\n";

        let old_uri = Url::from_file_path("D:/ws/util.spectra").expect("old uri");
        let new_uri = Url::from_file_path("D:/ws/utils.spectra").expect("new uri");
        let consumer_uri = Url::from_file_path("D:/ws/consumer.spectra").expect("consumer uri");
        let namespace_uri =
            Url::from_file_path("D:/ws/namespace_user.spectra").expect("namespace uri");

        let mut documents = HashMap::new();
        documents.insert(consumer_uri.clone(), analyzed_document(consumer_source));
        documents.insert(namespace_uri.clone(), analyzed_document(namespace_source));
        documents.insert(old_uri.clone(), analyzed_document(renamed_source));

        let files = vec![FileRename {
            old_uri: old_uri.to_string(),
            new_uri: new_uri.to_string(),
        }];
        let changes = will_rename_import_edits(&documents, &files);

        let consumer_edits = changes
            .get(&consumer_uri)
            .expect("consumer document must receive import edits");
        assert_eq!(consumer_edits.len(), 1);
        assert_eq!(consumer_edits[0].new_text, "utils");
        assert_eq!(
            apply_text_edits(consumer_source, consumer_edits),
            "module consumer\n\nfrom utils import helper\n"
        );

        let namespace_edits = changes
            .get(&namespace_uri)
            .expect("aliased import must be updated too");
        assert_eq!(namespace_edits[0].new_text, "utils");
        assert_eq!(
            apply_text_edits(namespace_source, namespace_edits),
            "module namespace_user\n\nimport utils as u\n"
        );

        // The renamed document itself never receives edits.
        assert!(!changes.contains_key(&old_uri) && !changes.contains_key(&new_uri));
    }

    #[test]
    fn will_rename_without_module_change_yields_no_edits() {
        let source = "module util\n\npublic func helper() returns unit {\n}\n";
        let old_uri = Url::from_file_path("D:/ws/util.spectra").expect("old uri");
        let new_uri = Url::from_file_path("D:/ws/util_v2.spectra").expect("new uri");
        let mut documents = HashMap::new();
        documents.insert(old_uri.clone(), analyzed_document(source));

        let files = vec![FileRename {
            old_uri: old_uri.to_string(),
            new_uri: new_uri.to_string(),
        }];
        assert!(will_rename_import_edits(&documents, &files).is_empty());
    }

    #[test]
    fn cli_default_resolves_spectralang_binary() {
        assert_eq!(
            ServerConfig::default().cli_path,
            "spectralang",
            "server-side CLI default must target the real binary name"
        );
    }
}
