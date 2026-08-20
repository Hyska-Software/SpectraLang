fn semantic_tokens_for_document(
    text: &str,
    module: &spectra_compiler::ast::Module,
    analysis: &DocumentAnalysis,
) -> Vec<SemanticToken> {
    let (declaration_tokens, declaration_kinds) = semantic_declaration_tokens(text, module);
    let mut all_tokens = declaration_tokens;

    for (span, info) in &analysis.symbols {
        let token_type = info
            .def_span
            .and_then(|definition_span| declaration_kinds.get(&definition_span).copied())
            .unwrap_or({
                if info.is_local {
                    TOKEN_VARIABLE
                } else {
                    TOKEN_FUNCTION
                }
            });

        all_tokens.push((*span, token_type));
    }

    all_tokens.sort_by_key(|(span, _)| {
        (
            span.start_location.line,
            span.start_location.column,
            span.end,
        )
    });
    all_tokens.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);

    encode_semantic_tokens(all_tokens)
}

fn semantic_declaration_tokens(
    text: &str,
    module: &spectra_compiler::ast::Module,
) -> (Vec<(Span, u32)>, HashMap<Span, u32>) {
    let mut tokens = Vec::new();
    let mut kinds = HashMap::new();

    for item in &module.items {
        match item {
            spectra_compiler::ast::Item::Import(import) => {
                for span in ordered_name_subspans(text, import.span, &import.path) {
                    tokens.push((span, TOKEN_NAMESPACE));
                }
                if let Some(names) = &import.names {
                    for imported_name in names {
                        let token_type =
                            imported_symbol_token_type(module, &imported_name.name);
                        if let Some(span) = named_subspan(text, import.span, &imported_name.name) {
                            tokens.push((span, token_type));
                        }
                        if let Some(alias) = &imported_name.alias {
                            if let Some(span) = named_subspan(text, imported_name.span, alias) {
                                tokens.push((span, token_type));
                            }
                        }
                    }
                }
                if let Some(alias) = &import.alias {
                    if let Some(span) = named_subspan(text, import.span, alias) {
                        tokens.push((span, TOKEN_NAMESPACE));
                    }
                }
                kinds.insert(import.span, TOKEN_NAMESPACE);
            }
            spectra_compiler::ast::Item::Function(function) => {
                if let Some(span) = named_subspan(text, function.span, &function.name) {
                    tokens.push((span, TOKEN_FUNCTION));
                }
                kinds.insert(function.span, TOKEN_FUNCTION);
                for type_param in &function.type_params {
                    if let Some(span) = named_subspan(text, type_param.span, &type_param.name) {
                        tokens.push((span, TOKEN_TYPE));
                    }
                    for bound in &type_param.bounds {
                        if let Some(span) = named_subspan(text, type_param.span, bound) {
                            tokens.push((span, TOKEN_INTERFACE));
                        }
                    }
                }
                for param in &function.params {
                    if let Some(span) = named_subspan(text, param.span, &param.name) {
                        tokens.push((span, TOKEN_PARAMETER));
                    }
                    kinds.insert(param.span, TOKEN_PARAMETER);
                    if let Some(ty) = &param.ty {
                        collect_type_annotation_tokens(text, ty, TOKEN_TYPE, &mut tokens);
                    }
                }
                if let Some(return_type) = &function.return_type {
                    collect_type_annotation_tokens(text, return_type, TOKEN_TYPE, &mut tokens);
                }
            }
            spectra_compiler::ast::Item::Struct(struct_def) => {
                if let Some(span) = named_subspan(text, struct_def.span, &struct_def.name) {
                    tokens.push((span, TOKEN_TYPE));
                }
                kinds.insert(struct_def.span, TOKEN_TYPE);
                for type_param in &struct_def.type_params {
                    if let Some(span) = named_subspan(text, type_param.span, &type_param.name) {
                        tokens.push((span, TOKEN_TYPE));
                    }
                }
                for field in &struct_def.fields {
                    if let Some(span) = named_subspan(text, field.span, &field.name) {
                        tokens.push((span, TOKEN_PROPERTY));
                    }
                    kinds.insert(field.span, TOKEN_PROPERTY);
                    collect_type_annotation_tokens(text, &field.ty, TOKEN_TYPE, &mut tokens);
                }
            }
            spectra_compiler::ast::Item::Enum(enum_def) => {
                if let Some(span) = named_subspan(text, enum_def.span, &enum_def.name) {
                    tokens.push((span, TOKEN_ENUM));
                }
                kinds.insert(enum_def.span, TOKEN_ENUM);
                for type_param in &enum_def.type_params {
                    if let Some(span) = named_subspan(text, type_param.span, &type_param.name) {
                        tokens.push((span, TOKEN_TYPE));
                    }
                }
                for variant in &enum_def.variants {
                    if let Some(span) = named_subspan(text, variant.span, &variant.name) {
                        tokens.push((span, TOKEN_ENUM_MEMBER));
                    }
                    kinds.insert(variant.span, TOKEN_ENUM_MEMBER);
                    if let Some(data) = &variant.data {
                        for ty in data {
                            collect_type_annotation_tokens(text, ty, TOKEN_TYPE, &mut tokens);
                        }
                    }
                    if let Some(fields) = &variant.struct_data {
                        for (_, ty) in fields {
                            collect_type_annotation_tokens(text, ty, TOKEN_TYPE, &mut tokens);
                        }
                    }
                }
            }
            spectra_compiler::ast::Item::Impl(impl_block) => {
                if let Some(span) = named_subspan(text, impl_block.span, &impl_block.type_name) {
                    tokens.push((span, TOKEN_TYPE));
                }
                if let Some(trait_name) = &impl_block.trait_name {
                    if let Some(span) = named_subspan(text, impl_block.span, trait_name) {
                        tokens.push((span, TOKEN_INTERFACE));
                    }
                }
                for method in &impl_block.methods {
                    if let Some(span) = named_subspan(text, method.span, &method.name) {
                        tokens.push((span, TOKEN_METHOD));
                    }
                    kinds.insert(method.span, TOKEN_METHOD);
                    for param in &method.params {
                        if let Some(span) = named_subspan(text, param.span, &param.name) {
                            tokens.push((span, TOKEN_PARAMETER));
                        }
                        kinds.insert(param.span, TOKEN_PARAMETER);
                        if let Some(ty) = &param.type_annotation {
                            collect_type_annotation_tokens(text, ty, TOKEN_TYPE, &mut tokens);
                        }
                    }
                    if let Some(return_type) = &method.return_type {
                        collect_type_annotation_tokens(text, return_type, TOKEN_TYPE, &mut tokens);
                    }
                }
            }
            spectra_compiler::ast::Item::Trait(trait_def) => {
                if let Some(span) = named_subspan(text, trait_def.span, &trait_def.name) {
                    tokens.push((span, TOKEN_INTERFACE));
                }
                kinds.insert(trait_def.span, TOKEN_INTERFACE);
                for parent_trait in &trait_def.parent_traits {
                    if let Some(span) = named_subspan(text, trait_def.span, parent_trait) {
                        tokens.push((span, TOKEN_INTERFACE));
                    }
                }
                for method in &trait_def.methods {
                    if let Some(span) = named_subspan(text, method.span, &method.name) {
                        tokens.push((span, TOKEN_METHOD));
                    }
                    kinds.insert(method.span, TOKEN_METHOD);
                    for param in &method.params {
                        if let Some(span) = named_subspan(text, param.span, &param.name) {
                            tokens.push((span, TOKEN_PARAMETER));
                        }
                        kinds.insert(param.span, TOKEN_PARAMETER);
                        if let Some(ty) = &param.type_annotation {
                            collect_type_annotation_tokens(text, ty, TOKEN_TYPE, &mut tokens);
                        }
                    }
                    if let Some(return_type) = &method.return_type {
                        collect_type_annotation_tokens(text, return_type, TOKEN_TYPE, &mut tokens);
                    }
                }
            }
            spectra_compiler::ast::Item::TypeAlias(_)
            | spectra_compiler::ast::Item::Const(_)
            | spectra_compiler::ast::Item::Static(_) => {}
            spectra_compiler::ast::Item::TraitImpl(trait_impl) => {
                if let Some(span) = named_subspan(text, trait_impl.span, &trait_impl.trait_name) {
                    tokens.push((span, TOKEN_INTERFACE));
                }
                if let Some(span) = named_subspan(text, trait_impl.span, &trait_impl.type_name) {
                    tokens.push((span, TOKEN_TYPE));
                }
                for method in &trait_impl.methods {
                    if let Some(span) = named_subspan(text, method.span, &method.name) {
                        tokens.push((span, TOKEN_METHOD));
                    }
                    kinds.insert(method.span, TOKEN_METHOD);
                    for param in &method.params {
                        if let Some(span) = named_subspan(text, param.span, &param.name) {
                            tokens.push((span, TOKEN_PARAMETER));
                        }
                        kinds.insert(param.span, TOKEN_PARAMETER);
                        if let Some(ty) = &param.type_annotation {
                            collect_type_annotation_tokens(text, ty, TOKEN_TYPE, &mut tokens);
                        }
                    }
                    if let Some(return_type) = &method.return_type {
                        collect_type_annotation_tokens(text, return_type, TOKEN_TYPE, &mut tokens);
                    }
                }
            }
        }
    }

    (tokens, kinds)
}

fn imported_symbol_token_type(module: &spectra_compiler::ast::Module, imported_name: &str) -> u32 {
    if module
        .imported_function_return_types
        .iter()
        .any(|(name, _)| name == imported_name)
    {
        TOKEN_FUNCTION
    } else if is_type_like_name(imported_name) {
        TOKEN_TYPE
    } else {
        TOKEN_NAMESPACE
    }
}

fn ordered_name_subspans(text: &str, container: Span, names: &[String]) -> Vec<Span> {
    if container.end > text.len() || container.start > container.end {
        return Vec::new();
    }

    let slice = &text[container.start..container.end];
    let mut spans = Vec::new();
    let mut search_from = 0usize;

    for name in names {
        if let Some(relative) = slice[search_from..].find(name) {
            let start = container.start + search_from + relative;
            let end = start + name.len();
            spans.push(span_from_offsets(text, start, end));
            search_from = end.saturating_sub(container.start);
        }
    }

    spans
}

fn collect_type_annotation_tokens(
    text: &str,
    ty: &spectra_compiler::ast::TypeAnnotation,
    final_kind: u32,
    tokens: &mut Vec<(Span, u32)>,
) {
    match &ty.kind {
        spectra_compiler::ast::TypeAnnotationKind::Simple { segments } => {
            let spans = ordered_name_subspans(text, ty.span, segments);
            for (index, span) in spans.into_iter().enumerate() {
                let kind = if index + 1 == segments.len() {
                    final_kind
                } else {
                    TOKEN_NAMESPACE
                };
                tokens.push((span, kind));
            }
        }
        spectra_compiler::ast::TypeAnnotationKind::Tuple { elements } => {
            for element in elements {
                collect_type_annotation_tokens(text, element, TOKEN_TYPE, tokens);
            }
        }
        spectra_compiler::ast::TypeAnnotationKind::Function {
            params,
            return_type,
        } => {
            for param in params {
                collect_type_annotation_tokens(text, param, TOKEN_TYPE, tokens);
            }
            collect_type_annotation_tokens(text, return_type, TOKEN_TYPE, tokens);
        }
        spectra_compiler::ast::TypeAnnotationKind::Generic { name: _, type_args } => {
            for arg in type_args {
                collect_type_annotation_tokens(text, arg, TOKEN_TYPE, tokens);
            }
        }
        spectra_compiler::ast::TypeAnnotationKind::DynTrait { .. } => {}
    }
}

fn is_type_like_name(name: &str) -> bool {
    name.chars()
        .next()
        .map(|ch| ch.is_ascii_uppercase())
        .unwrap_or(false)
}

fn encode_semantic_tokens(tokens: Vec<(Span, u32)>) -> Vec<SemanticToken> {
    let mut encoded = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for (span, token_type) in tokens {
        if span.start_location.line != span.end_location.line {
            continue;
        }

        let line = span.start_location.line.saturating_sub(1) as u32;
        let start = span.start_location.column.saturating_sub(1) as u32;
        let length = span
            .end_location
            .column
            .saturating_sub(span.start_location.column) as u32;
        if length == 0 {
            continue;
        }

        let delta_line = line.saturating_sub(prev_line);
        let delta_start = if delta_line == 0 {
            start.saturating_sub(prev_start)
        } else {
            start
        };

        encoded.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type,
            token_modifiers_bitset: 0,
        });

        prev_line = line;
        prev_start = start;
    }

    encoded
}

fn named_subspan(text: &str, container: Span, name: &str) -> Option<Span> {
    if name.is_empty() || container.end > text.len() || container.start > container.end {
        return None;
    }

    let slice = &text[container.start..container.end];
    let mut search_from = 0usize;
    while let Some(relative) = slice[search_from..].find(name) {
        let relative_start = search_from + relative;
        let relative_end = relative_start + name.len();

        let before = slice[..relative_start].chars().next_back();
        let after = slice[relative_end..].chars().next();
        let boundary_before = before.map(is_identifier_char).unwrap_or(false);
        let boundary_after = after.map(is_identifier_char).unwrap_or(false);
        if !boundary_before && !boundary_after {
            let start = container.start + relative_start;
            let end = container.start + relative_end;
            return Some(span_from_offsets(text, start, end));
        }

        search_from = relative_end;
    }

    None
}

fn is_identifier_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn span_from_offsets(text: &str, start: usize, end: usize) -> Span {
    let start_location = offset_to_location(text, start);
    let end_location = offset_to_location(text, end);
    Span {
        start,
        end,
        start_location,
        end_location,
    }
}

fn offset_to_location(text: &str, offset: usize) -> spectra_compiler::span::Location {
    let mut line = 1usize;
    let mut column = 1usize;

    for (idx, ch) in text.char_indices() {
        if idx >= offset {
            break;
        }

        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }

    spectra_compiler::span::Location { line, column }
}

