fn quick_fix_for_diagnostic(
    uri: &Url,
    document: &DocumentState,
    diagnostic: &Diagnostic,
) -> Option<CodeAction> {
    let code = match &diagnostic.code {
        Some(NumberOrString::String(code)) => code.as_str(),
        _ => "",
    };

    // Lint-specific fixes
    match code {
        "lint(unused-binding)" => return unused_binding_quick_fix(uri, &document.text, diagnostic),
        "lint(shadowing)" => return shadowing_quick_fix(uri, document, diagnostic),
        "lint(unreachable-code)" => return unreachable_code_quick_fix(uri, diagnostic),
        _ => {}
    }

    // Semantic error fixes routed by code (E001–E009).
    match code {
        // E002: Variable already declared — rename the new binding.
        "E002" => return duplicate_binding_quick_fix(uri, &document.text, diagnostic),
        // E005: Return statement missing value — insert a default literal.
        "E005" => {
            return missing_return_value_quick_fix(
                uri,
                &document.text,
                diagnostic,
                &diagnostic.message,
            )
        }
        // E001: Undefined variable — prefix with `_` to suppress or declare it.
        // E003: Type mismatch in assignment — fall through to hint-based fix.
        // E004: Return type mismatch — fall through to hint-based fix.
        // Others: fall through to pattern-matching fallbacks.
        _ => {}
    }

    // Fallback: pattern-match on message strings for errors without a code.
    semantic_error_quick_fix(uri, document, diagnostic)
        .or_else(|| hint_based_quick_fix(uri, &document.text, diagnostic))
}

/// Tenta gerar um quick fix para erros semânticos comuns, reconhecendo padrões
/// de mensagem produzidos pelo compilador.
fn semantic_error_quick_fix(
    uri: &Url,
    document: &DocumentState,
    diagnostic: &Diagnostic,
) -> Option<CodeAction> {
    let msg = diagnostic.message.as_str();

    // "Variable 'x' is already declared in this scope"
    // → renomear a nova binding para um nome único
    if msg.contains("is already declared in this scope") {
        return duplicate_binding_quick_fix(uri, &document.text, diagnostic);
    }

    // "Return statement missing value of type ..."
    // → inserir um valor padrão compatível com o tipo mencionado
    if msg.starts_with("Return statement missing value of type") {
        return missing_return_value_quick_fix(uri, &document.text, diagnostic, msg);
    }

    None
}

fn duplicate_binding_quick_fix(
    uri: &Url,
    text: &str,
    diagnostic: &Diagnostic,
) -> Option<CodeAction> {
    let binding_name = extract_quoted_name(&diagnostic.message)?;
    let replacement = unique_identifier_name(text, &binding_name);
    let binding_range = find_name_range_in_range(text, diagnostic.range, &binding_name)?;

    Some(quick_fix_action(
        uri,
        diagnostic,
        format!("Renomear '{}' para '{}'", binding_name, replacement),
        vec![TextEdit {
            range: binding_range,
            new_text: replacement,
        }],
    ))
}

fn missing_return_value_quick_fix(
    uri: &Url,
    text: &str,
    diagnostic: &Diagnostic,
    message: &str,
) -> Option<CodeAction> {
    // Extrair o tipo da mensagem. Formato: 'Return statement missing value of type Int'
    let type_str = message
        .strip_prefix("Return statement missing value of type")
        .map(|s| s.trim())
        // Remover delimitadores de debug como `Int` ou `{...}`
        .map(|s| s.trim_matches(|c: char| c == '`' || c == '"'))
        .unwrap_or("");

    let default_value = match type_str.to_lowercase().as_str() {
        "int" => "0",
        "bool" => "false",
        "string" => "\"\"",
        "float" => "0.0",
        "char" => "'\\0'",
        _ => return None,
    };

    // Localizar a posição do `return` sem valor — o range aponta para o `return` keyword.
    // Inserimos o valor entre `return` e `;`.
    let start_offset = position_to_offset(text, diagnostic.range.start);
    let end_offset = position_to_offset(text, diagnostic.range.end);
    if start_offset >= text.len() || end_offset > text.len() {
        return None;
    }

    let slice = &text[start_offset..end_offset];
    // Encontra o ponto de inserção: logo após 'return', antes do ';' ou final do range
    let insert_relative = if let Some(semi) = slice.find(';') {
        semi
    } else {
        slice.len()
    };
    let insert_offset = start_offset + insert_relative;
    let insert_pos = offset_to_position(text, insert_offset);
    let insert_range = Range::new(insert_pos, insert_pos);

    Some(quick_fix_action(
        uri,
        diagnostic,
        format!("Inserir valor de retorno padrão ({})", default_value),
        vec![TextEdit {
            range: insert_range,
            new_text: format!(" {}", default_value),
        }],
    ))
}

fn unused_binding_quick_fix(uri: &Url, text: &str, diagnostic: &Diagnostic) -> Option<CodeAction> {
    let binding_name = extract_quoted_name(&diagnostic.message)?;
    if binding_name.starts_with('_') {
        return None;
    }

    let binding_range = find_name_range_in_range(text, diagnostic.range, &binding_name)?;
    let edit = TextEdit {
        range: binding_range,
        new_text: format!("_{}", binding_name),
    };

    Some(quick_fix_action(
        uri,
        diagnostic,
        format!("Prefixar '{}' com _", binding_name),
        vec![edit],
    ))
}

fn shadowing_quick_fix(
    uri: &Url,
    document: &DocumentState,
    diagnostic: &Diagnostic,
) -> Option<CodeAction> {
    let binding_name = extract_quoted_name(&diagnostic.message)?;
    let replacement = unique_identifier_name(&document.text, &binding_name);
    let definition_range =
        find_name_range_in_range(&document.text, diagnostic.range, &binding_name)?;
    let definition_span = range_to_span(&document.text, definition_range);

    let mut edits = Vec::new();
    for (span, info) in &document.analysis.symbols {
        if info.def_span != Some(definition_span) {
            continue;
        }
        edits.push(TextEdit {
            range: span_to_range(*span),
            new_text: replacement.clone(),
        });
    }

    if edits.is_empty() {
        edits.push(TextEdit {
            range: definition_range,
            new_text: replacement.clone(),
        });
    }

    Some(quick_fix_action(
        uri,
        diagnostic,
        format!("Renomear '{}' para '{}'", binding_name, replacement),
        edits,
    ))
}

fn unreachable_code_quick_fix(uri: &Url, diagnostic: &Diagnostic) -> Option<CodeAction> {
    Some(quick_fix_action(
        uri,
        diagnostic,
        "Remover código inalcançável".to_string(),
        vec![TextEdit {
            range: diagnostic.range,
            new_text: String::new(),
        }],
    ))
}

fn hint_based_quick_fix(uri: &Url, text: &str, diagnostic: &Diagnostic) -> Option<CodeAction> {
    let hints = diagnostic
        .related_information
        .as_ref()?
        .iter()
        .map(|info| info.message.as_str())
        .collect::<Vec<_>>();

    if let Some(hint) = hints.into_iter().next() {
        let inserted = if hint.contains("matching \" character") {
            Some("\"")
        } else if hint.contains("matching ' character") {
            Some("'")
        } else {
            None
        }?;

        let insert_offset = position_to_offset(text, diagnostic.range.end);
        let insert_range = span_to_range(span_from_offsets(text, insert_offset, insert_offset));
        return Some(quick_fix_action(
            uri,
            diagnostic,
            format!("Aplicar hint: inserir {}", inserted),
            vec![TextEdit {
                range: insert_range,
                new_text: inserted.to_string(),
            }],
        ));
    }

    None
}

fn quick_fix_action(
    uri: &Url,
    diagnostic: &Diagnostic,
    title: String,
    edits: Vec<TextEdit>,
) -> CodeAction {
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);

    CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diagnostic.clone()]),
        is_preferred: Some(true),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        ..Default::default()
    }
}

fn extract_quoted_name(message: &str) -> Option<String> {
    let start = message.find('\'')?;
    let end = message[start + 1..].find('\'')? + start + 1;
    Some(message[start + 1..end].to_string())
}

fn unique_identifier_name(text: &str, base: &str) -> String {
    let mut index = 1usize;
    loop {
        let candidate = format!("{}_{}", base, index);
        if !contains_identifier(text, &candidate) {
            return candidate;
        }
        index += 1;
    }
}

fn contains_identifier(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let end = start + name.len();
        let after = if end < text.len() {
            text[end..].chars().next()
        } else {
            None
        };
        !before.map(is_identifier_char).unwrap_or(false)
            && !after.map(is_identifier_char).unwrap_or(false)
    })
}

fn find_name_range_in_range(text: &str, range: Range, name: &str) -> Option<Range> {
    let start_offset = position_to_offset(text, range.start);
    let end_offset = position_to_offset(text, range.end);
    if start_offset >= end_offset || end_offset > text.len() {
        return None;
    }

    let slice = &text[start_offset..end_offset];
    let relative = slice.find(name)?;
    let before = if relative == 0 {
        None
    } else {
        slice[..relative].chars().next_back()
    };
    let after_index = relative + name.len();
    let after = if after_index >= slice.len() {
        None
    } else {
        slice[after_index..].chars().next()
    };
    if before.map(is_identifier_char).unwrap_or(false)
        || after.map(is_identifier_char).unwrap_or(false)
    {
        return None;
    }

    let absolute_start = start_offset + relative;
    let absolute_end = absolute_start + name.len();
    Some(span_to_range(span_from_offsets(
        text,
        absolute_start,
        absolute_end,
    )))
}

fn range_to_span(text: &str, range: Range) -> Span {
    let start = position_to_offset(text, range.start);
    let end = position_to_offset(text, range.end);
    span_from_offsets(text, start, end)
}

