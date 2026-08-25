fn analyze_cache_document(text: &str, filename: &str) -> DocumentAnalysis {
    let options = CompilationOptions {
        optimize: false,
        lint: spectra_compiler::LintOptions::disabled(),
        ..CompilationOptions::default()
    };
    analyze_document(text, filename, &options, None)
}

fn cache_modified_time_for_uri(uri: &Url) -> Option<SystemTime> {
    uri.to_file_path()
        .ok()
        .and_then(|path| fs::metadata(path).ok())
        .and_then(|meta| meta.modified().ok())
}

fn location_key(location: &Location) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        location.uri,
        location.range.start.line,
        location.range.start.character,
        location.range.end.line,
        location.range.end.character
    )
}

fn reference_key_for_resolved_symbol(
    document: &DocumentState,
    symbol: &spectra_compiler::ResolvedSymbol,
) -> Option<String> {
    symbol
        .definition
        .as_ref()
        .map(|definition| definition_key(&definition.label))
        .or_else(|| {
            document
                .analysis
                .definitions
                .get(&symbol.span)
                .map(|definition| definition_key(&definition.label))
        })
}

fn definition_key(label: &str) -> String {
    let trimmed = label.trim();
    if let Some(rest) = trimmed.strip_prefix("async func ") {
        return rest.split('(').next().unwrap_or(rest).trim().to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("func ") {
        return rest.split('(').next().unwrap_or(rest).trim().to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("record ") {
        return rest.trim().to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("enum ") {
        return rest.trim().to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("trait ") {
        return rest.trim().to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("variant ") {
        return rest.trim().to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("field ") {
        return rest.split(':').next().unwrap_or(rest).trim().to_string();
    }
    trimmed.to_string()
}

fn reference_entries_for_analysis(uri: &Url, analysis: &DocumentAnalysis) -> Vec<CachedReference> {
    let mut references = Vec::new();

    for (span, info) in &analysis.symbols {
        if info.is_local {
            continue;
        }
        let Some(definition_span) = info.def_span else {
            continue;
        };
        let Some(definition) = analysis.definitions.get(&definition_span) else {
            continue;
        };

        references.push(CachedReference {
            key: definition_key(&definition.label),
            location: Location {
                uri: uri.clone(),
                range: span_to_range(*span),
            },
        });
    }

    references
}

fn rename_edits_for_document(
    document: &DocumentState,
    symbol: &spectra_compiler::ResolvedSymbol,
    current_name: &str,
    new_name: &str,
) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    let mut seen = HashSet::new();
    if let Some(definition_span) = symbol.info.def_span {
        for (span, info) in &document.analysis.symbols {
            if info.def_span != Some(definition_span) {
                continue;
            }
            if text_for_span(&document.text, *span) != current_name {
                continue;
            }
            push_unique_rename_edit(&mut edits, &mut seen, span_to_range(*span), new_name);
        }

        if let Some(definition_range) =
            find_name_range_in_span(&document.text, definition_span, current_name)
        {
            push_unique_rename_edit(&mut edits, &mut seen, definition_range, new_name);
        }
    }

    if edits.is_empty() {
        for range in lexical_rename_ranges(&document.text, symbol.span, current_name) {
            push_unique_rename_edit(&mut edits, &mut seen, range, new_name);
        }
    }

    edits.sort_by_key(|edit| {
        (
            edit.range.start.line,
            edit.range.start.character,
            edit.range.end.line,
            edit.range.end.character,
        )
    });
    edits
}

fn lexical_rename_ranges(text: &str, cursor_span: Span, current_name: &str) -> Vec<Range> {
    let (scope_start, scope_end) =
        enclosing_brace_scope(text, cursor_span.start).unwrap_or((0usize, text.len()));
    identifier_occurrence_ranges(text, scope_start, scope_end, current_name)
}

fn enclosing_brace_scope(text: &str, cursor_offset: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut open = None;
    let mut index = cursor_offset.min(bytes.len());
    while index > 0 {
        index -= 1;
        match bytes[index] {
            b'}' => depth += 1,
            b'{' if depth == 0 => {
                open = Some(index + 1);
                break;
            }
            b'{' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    let open = open?;
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().enumerate().skip(open) {
        match byte {
            b'{' => depth += 1,
            b'}' if depth == 0 => return Some((open, index)),
            b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Some((open, bytes.len()))
}

fn identifier_occurrence_ranges(
    text: &str,
    scope_start: usize,
    scope_end: usize,
    name: &str,
) -> Vec<Range> {
    if name.is_empty() || scope_start > scope_end || scope_end > text.len() {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let slice = &text[scope_start..scope_end];
    let mut search_from = 0usize;
    while let Some(relative) = slice[search_from..].find(name) {
        let relative_start = search_from + relative;
        let relative_end = relative_start + name.len();
        let before = slice[..relative_start].chars().next_back();
        let after = slice[relative_end..].chars().next();
        if !before.map(is_identifier_char).unwrap_or(false)
            && !after.map(is_identifier_char).unwrap_or(false)
        {
            let absolute_start = scope_start + relative_start;
            let absolute_end = scope_start + relative_end;
            ranges.push(span_to_range(span_from_offsets(
                text,
                absolute_start,
                absolute_end,
            )));
        }
        search_from = relative_end;
    }
    ranges
}

fn identifier_at_position(text: &str, position: Position) -> Option<String> {
    let offset = position_to_offset(text, position).min(text.len());
    let bytes = text.as_bytes();
    let mut start = offset;
    while start > 0 && is_identifier_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len() && is_identifier_byte(bytes[end]) {
        end += 1;
    }
    (start < end).then(|| text[start..end].to_string())
}

fn is_identifier_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn push_unique_rename_edit(
    edits: &mut Vec<TextEdit>,
    seen: &mut HashSet<String>,
    range: Range,
    new_name: &str,
) {
    let key = format!(
        "{}:{}:{}:{}",
        range.start.line, range.start.character, range.end.line, range.end.character
    );
    if seen.insert(key) {
        edits.push(TextEdit {
            range,
            new_text: new_name.to_string(),
        });
    }
}

fn text_for_span(text: &str, span: Span) -> String {
    if span.start <= span.end && span.end <= text.len() {
        text[span.start..span.end].to_string()
    } else {
        String::new()
    }
}

fn is_valid_rename_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        && !KEYWORDS.contains(&value)
}

fn find_name_range_in_span(text: &str, span: Span, name: &str) -> Option<Range> {
    if name.is_empty() || span.end > text.len() || span.start > span.end {
        return None;
    }
    let slice = &text[span.start..span.end];
    let mut search_from = 0usize;
    while let Some(relative) = slice[search_from..].find(name) {
        let relative_start = search_from + relative;
        let relative_end = relative_start + name.len();
        let before = slice[..relative_start].chars().next_back();
        let after = slice[relative_end..].chars().next();
        if !before.map(is_identifier_char).unwrap_or(false)
            && !after.map(is_identifier_char).unwrap_or(false)
        {
            let absolute_start = span.start + relative_start;
            let absolute_end = span.start + relative_end;
            return Some(span_to_range(span_from_offsets(
                text,
                absolute_start,
                absolute_end,
            )));
        }
        search_from = relative_end;
    }
    None
}

fn workspace_symbol_entries_for_module(
    text: &str,
    module: &spectra_compiler::ast::Module,
) -> Vec<CachedWorkspaceSymbol> {
    let mut symbols = Vec::new();

    for item in &module.items {
        collect_workspace_symbol_entries(text, item, &mut symbols);
    }

    symbols
}

fn collect_workspace_symbol_entries(
    text: &str,
    item: &spectra_compiler::ast::Item,
    output: &mut Vec<CachedWorkspaceSymbol>,
) {
    match item {
        spectra_compiler::ast::Item::Import(import) => {
            output.push(CachedWorkspaceSymbol {
                name: import
                    .alias
                    .clone()
                    .unwrap_or_else(|| import.path.join("::")),
                detail: Some("import".to_string()),
                kind: SymbolKind::MODULE,
                span: import.span,
                container_name: None,
            });
        }
        spectra_compiler::ast::Item::Function(function) => {
            if let Some(name_span) = named_subspan(text, function.span, &function.name) {
                output.push(CachedWorkspaceSymbol {
                    name: function.name.clone(),
                    detail: Some(format_function_signature(function)),
                    kind: SymbolKind::FUNCTION,
                    span: name_span,
                    container_name: None,
                });
            }
            for param in &function.params {
                output.push(CachedWorkspaceSymbol {
                    name: param.name.clone(),
                    detail: param.ty.as_ref().map(format_type_annotation),
                    kind: SymbolKind::VARIABLE,
                    span: param.span,
                    container_name: Some(function.name.clone()),
                });
            }
        }
        spectra_compiler::ast::Item::Struct(struct_def) => {
            if let Some(name_span) = named_subspan(text, struct_def.span, &struct_def.name) {
                output.push(CachedWorkspaceSymbol {
                    name: struct_def.name.clone(),
                    detail: Some("record".to_string()),
                    kind: SymbolKind::STRUCT,
                    span: name_span,
                    container_name: None,
                });
            }
            for field in &struct_def.fields {
                output.push(CachedWorkspaceSymbol {
                    name: field.name.clone(),
                    detail: Some(format_type_annotation(&field.ty)),
                    kind: SymbolKind::FIELD,
                    span: field.span,
                    container_name: Some(struct_def.name.clone()),
                });
            }
        }
        spectra_compiler::ast::Item::Enum(enum_def) => {
            if let Some(name_span) = named_subspan(text, enum_def.span, &enum_def.name) {
                output.push(CachedWorkspaceSymbol {
                    name: enum_def.name.clone(),
                    detail: Some("enum".to_string()),
                    kind: SymbolKind::ENUM,
                    span: name_span,
                    container_name: None,
                });
            }
            for variant in &enum_def.variants {
                output.push(CachedWorkspaceSymbol {
                    name: variant.name.clone(),
                    detail: Some(format_variant_signature(enum_def, variant)),
                    kind: SymbolKind::ENUM_MEMBER,
                    span: variant.span,
                    container_name: Some(enum_def.name.clone()),
                });
            }
        }
        spectra_compiler::ast::Item::Impl(impl_block) => {
            for method in &impl_block.methods {
                if let Some(name_span) = named_subspan(text, method.span, &method.name) {
                    output.push(CachedWorkspaceSymbol {
                        name: method.name.clone(),
                        detail: Some(format_method_signature(&impl_block.type_name, method)),
                        kind: SymbolKind::METHOD,
                        span: name_span,
                        container_name: Some(impl_block.type_name.clone()),
                    });
                }
            }
        }
        spectra_compiler::ast::Item::Trait(trait_def) => {
            if let Some(name_span) = named_subspan(text, trait_def.span, &trait_def.name) {
                output.push(CachedWorkspaceSymbol {
                    name: trait_def.name.clone(),
                    detail: Some("trait".to_string()),
                    kind: SymbolKind::INTERFACE,
                    span: name_span,
                    container_name: None,
                });
            }
            for method in &trait_def.methods {
                if let Some(name_span) = named_subspan(text, method.span, &method.name) {
                    output.push(CachedWorkspaceSymbol {
                        name: method.name.clone(),
                        detail: Some(format_trait_method_signature(method)),
                        kind: SymbolKind::METHOD,
                        span: name_span,
                        container_name: Some(trait_def.name.clone()),
                    });
                }
            }
        }
        spectra_compiler::ast::Item::TraitImpl(trait_impl) => {
            for method in &trait_impl.methods {
                if let Some(name_span) = named_subspan(text, method.span, &method.name) {
                    output.push(CachedWorkspaceSymbol {
                        name: method.name.clone(),
                        detail: Some(format_method_signature(&trait_impl.type_name, method)),
                        kind: SymbolKind::METHOD,
                        span: name_span,
                        container_name: Some(trait_impl.type_name.clone()),
                    });
                }
            }
        }
        spectra_compiler::ast::Item::TypeAlias(_)
        | spectra_compiler::ast::Item::Const(_)
        | spectra_compiler::ast::Item::Static(_) => {}
    }
}

#[allow(deprecated)]
fn workspace_symbol_information(uri: &Url, symbol: &CachedWorkspaceSymbol) -> SymbolInformation {
    SymbolInformation {
        name: symbol.name.clone(),
        kind: symbol.kind,
        tags: None,
        deprecated: None,
        location: Location {
            uri: uri.clone(),
            range: span_to_range(symbol.span),
        },
        container_name: symbol.container_name.clone(),
    }
}

fn workspace_definition_locations(
    cache: &HashMap<Url, WorkspaceCacheEntry>,
    identifier: &str,
) -> Vec<Location> {
    let mut locations = Vec::new();
    let mut seen = HashSet::new();
    for (uri, entry) in cache {
        for symbol in &entry.symbols {
            if symbol.name != identifier {
                continue;
            }
            let location = Location {
                uri: uri.clone(),
                range: span_to_range(symbol.span),
            };
            if seen.insert(location_key(&location)) {
                locations.push(location);
            }
        }
    }
    locations.sort_by_key(|location| {
        (
            location.uri.to_string(),
            location.range.start.line,
            location.range.start.character,
        )
    });
    locations
}


fn spectra_file_operation_registration() -> FileOperationRegistrationOptions {
    FileOperationRegistrationOptions {
        filters: vec![FileOperationFilter {
            scheme: Some("file".to_string()),
            pattern: FileOperationPattern {
                glob: "**/*.spectra".to_string(),
                matches: None,
                options: None,
            },
        }],
    }
}

/// Module name the renamed file declares. Prefers the document's own `module`
/// declaration (open documents are analyzed); falls back to the file stem so a
/// closed-but-cached rename still works when module and stem agree.
fn declared_or_stem_module(documents: &HashMap<Url, DocumentState>, uri: &Url) -> Option<String> {
    if let Some(document) = documents.get(uri) {
        if let Some(module) = &document.analysis.module {
            return Some(module.name.clone());
        }
    }
    uri_file_stem(uri)
}

fn uri_file_stem(uri: &Url) -> Option<String> {
    uri.to_file_path()
        .ok()?
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
}

/// willRenameFiles support: for every *other* open document whose imports
/// reference the renamed module, produce edits that rewrite the import path
/// segment to the new name. Only the dotted path inside each matching import
/// statement is touched; aliases and named-import lists are preserved.
fn will_rename_import_edits(
    documents: &HashMap<Url, DocumentState>,
    files: &[FileRename],
) -> HashMap<Url, Vec<TextEdit>> {
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

    for file in files {
        let Ok(old_uri) = Url::parse(&file.old_uri) else {
            continue;
        };
        let Ok(new_uri) = Url::parse(&file.new_uri) else {
            continue;
        };
        let Some(old_module) = declared_or_stem_module(documents, &old_uri) else {
            continue;
        };
        let Some(new_module) = uri_file_stem(&new_uri) else {
            continue;
        };
        if old_module == new_module {
            continue;
        }

        for (uri, document) in documents {
            if *uri == old_uri || *uri == new_uri {
                continue;
            }
            let edits = import_update_edits_for_document(document, &old_module, &new_module);
            if !edits.is_empty() {
                changes.insert(uri.clone(), edits);
            }
        }
    }

    changes
}


fn import_update_edits_for_document(
    document: &DocumentState,
    old_module: &str,
    new_module: &str,
) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    let mut seen = HashSet::new();
    let Some(module) = &document.analysis.module else {
        return edits;
    };

    for item in &module.items {
        let spectra_compiler::ast::Item::Import(import) = item else {
            continue;
        };
        if !import.path.iter().any(|segment| segment == old_module) {
            continue;
        }

        // The import path is spelled dotted in source (`import util` /
        // `from std.util import x`). Locate that exact text within the
        // statement span so only the path is replaced.
        let dotted = import.path.join(".");
        let span_start = import.span.start.min(document.text.len());
        let span_end = import.span.end.min(document.text.len());
        if span_start >= span_end {
            continue;
        }
        let Some(relative) = document.text[span_start..span_end].find(&dotted) else {
            continue;
        };

        let start_offset = span_start + relative;
        let end_offset = start_offset + dotted.len();
        let key = format!("{}:{}", start_offset, end_offset);
        if !seen.insert(key) {
            continue;
        }

        let updated_path = import
            .path
            .iter()
            .map(|segment| {
                if segment == old_module {
                    new_module.to_string()
                } else {
                    segment.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(".");

        edits.push(TextEdit {
            range: Range {
                start: offset_to_position(&document.text, start_offset),
                end: offset_to_position(&document.text, end_offset),
            },
            new_text: updated_path,
        });
    }

    edits.sort_by_key(|edit| edit.range.start);
    edits
}
