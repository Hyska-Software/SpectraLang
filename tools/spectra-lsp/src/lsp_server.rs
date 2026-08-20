#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        if let Some(folders) = params.workspace_folders {
            let mut workspace_folders = self.state.workspace_folders.write().await;
            *workspace_folders = folders
                .into_iter()
                .filter_map(|folder| folder.uri.to_file_path().ok())
                .collect();
        }

        if let Some(options) = params.initialization_options {
            self.update_config(options).await;
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: Some(vec![",".to_string()]),
                    ..Default::default()
                }),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
                    ..Default::default()
                }),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
                        ..Default::default()
                    },
                )),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: semantic_tokens_legend(),
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            ..Default::default()
                        },
                    ),
                ),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![
                        COMMAND_RUN_DIAGNOSTICS.to_string(),
                        COMMAND_LINT_WORKSPACE.to_string(),
                    ],
                    ..Default::default()
                }),
                inlay_hint_provider: Some(OneOf::Left(true)),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "spectra-lsp".to_string(),
                version: Some("0.1.0".to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "spectra-lsp inicializado")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        self.update_config(params.settings).await;
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        let mut folders = self.state.workspace_folders.write().await;
        let mut cache = self.state.workspace_cache.write().await;
        for removed in params.event.removed {
            if let Ok(path) = removed.uri.to_file_path() {
                folders.retain(|folder| folder != &path);
                cache.retain(|uri, _| {
                    uri.to_file_path()
                        .ok()
                        .map(|file| !file.starts_with(&path))
                        .unwrap_or(true)
                });
            }
        }
        for added in params.event.added {
            if let Ok(path) = added.uri.to_file_path() {
                if !folders.iter().any(|folder| folder == &path) {
                    folders.push(path);
                }
            }
        }
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.analyze_and_store(params.text_document.uri, params.text_document.text, false)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            let uri = params.text_document.uri;
            let text = change.text;

            // Abort any previously scheduled debounce task for this document.
            if let Some(old) = self.state.debounce_handles.lock().await.remove(&uri) {
                old.abort();
            }

            // Schedule a new analysis after a 300 ms typing pause.
            let client = self.client.clone();
            let state = Arc::clone(&self.state);
            let uri_debounce = uri.clone();
            let text_debounce = text;

            let join = tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                // Remove our own handle now that we are executing.
                state.debounce_handles.lock().await.remove(&uri_debounce);
                do_analyze_and_store(&client, &state, uri_debounce, text_debounce, false).await;
            });

            self.state
                .debounce_handles
                .lock()
                .await
                .insert(uri, join.abort_handle());
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let include_lints = self.state.config.read().await.lint_on_save;
        if let Some(text) = params.text {
            self.analyze_and_store(params.text_document.uri, text, include_lints)
                .await;
        } else {
            self.reanalyze_document(&params.text_document.uri, include_lints)
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.state
            .documents
            .write()
            .await
            .remove(&params.text_document.uri);
        self.refresh_cache_from_disk(&params.text_document.uri)
            .await;
        self.client
            .publish_diagnostics(params.text_document.uri, Vec::new(), None)
            .await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let text_position = params.text_document_position_params;
        let Some(document) = self
            .state
            .documents
            .read()
            .await
            .get(&text_position.text_document.uri)
            .cloned()
        else {
            return Ok(None);
        };

        let line = text_position.position.line as usize + 1;
        let column = text_position.position.character as usize + 1;

        if let Some(module) = &document.analysis.module {
            if let Some(call_site) = find_call_site(module, line, column) {
                if let Some(api_hover) = api_call_hover(&document, &call_site) {
                    return Ok(Some(api_hover));
                }
            }
        }

        let Some(symbol) = document.analysis.symbol_at(line, column) else {
            return Ok(None);
        };

        let definition_label = symbol
            .definition
            .as_ref()
            .map(|definition| definition.label.clone())
            .unwrap_or_else(|| spectra_compiler::language_service::type_to_string(&symbol.info.ty));
        let scope = if symbol.info.is_local {
            "local"
        } else {
            "global"
        };
        let type_label = spectra_compiler::language_service::type_to_string(&symbol.info.ty);
        let contents = HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!(
                "```spectra\n{}\n```\n\nTipo: `{}`\n\nEscopo: {}",
                definition_label, type_label, scope
            ),
        });

        Ok(Some(Hover {
            contents,
            range: Some(span_to_range(symbol.span)),
        }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let text_position = params.text_document_position_params;
        let Some(document) = self
            .state
            .documents
            .read()
            .await
            .get(&text_position.text_document.uri)
            .cloned()
        else {
            return Ok(None);
        };

        let line = text_position.position.line as usize + 1;
        let column = text_position.position.character as usize + 1;
        let symbol = document.analysis.symbol_at(line, column);
        if let Some(definition) = symbol.as_ref().and_then(|symbol| symbol.definition.clone()) {
            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                uri: text_position.text_document.uri,
                range: span_to_range(definition.span),
            })));
        }

        let Some(identifier) = identifier_at_position(&document.text, text_position.position)
        else {
            return Ok(None);
        };
        if !is_valid_rename_identifier(&identifier) {
            return Ok(None);
        }

        self.ensure_workspace_cache().await;
        let cache = self.state.workspace_cache.read().await;
        let mut locations = workspace_definition_locations(&cache, &identifier);
        locations.retain(|location| location.uri != text_position.text_document.uri);
        Ok(locations
            .into_iter()
            .next()
            .map(GotoDefinitionResponse::Scalar))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let Some(document) = self
            .state
            .documents
            .read()
            .await
            .get(&params.text_document.uri)
            .cloned()
        else {
            return Ok(None);
        };

        match self
            .format_document(&params.text_document.uri, &document.text)
            .await
        {
            Ok(Some(formatted)) if formatted != document.text => {
                let edit = TextEdit {
                    range: full_document_range(&document.text),
                    new_text: formatted,
                };
                Ok(Some(vec![edit]))
            }
            Ok(_) => Ok(None),
            Err(message) => {
                self.client.log_message(MessageType::ERROR, message).await;
                Ok(None)
            }
        }
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let text_position = params.text_document_position;
        let Some(document) = self
            .state
            .documents
            .read()
            .await
            .get(&text_position.text_document.uri)
            .cloned()
        else {
            return Ok(None);
        };

        let line = text_position.position.line as usize + 1;
        let column = text_position.position.character as usize + 1;
        let Some(symbol) = document.analysis.symbol_at(line, column) else {
            return Ok(None);
        };
        let Some(definition_span) = symbol.info.def_span else {
            return Ok(None);
        };

        let include_declaration = params.context.include_declaration;
        let declaration_range = span_to_range(definition_span);
        let current_uri = text_position.text_document.uri.clone();
        let mut locations = Vec::new();
        let mut seen = HashSet::new();

        for (span, info) in &document.analysis.symbols {
            if info.def_span != Some(definition_span) {
                continue;
            }

            if !include_declaration && *span == definition_span {
                continue;
            }

            let location = Location {
                uri: current_uri.clone(),
                range: span_to_range(*span),
            };
            if seen.insert(location_key(&location)) {
                locations.push(location);
            }
        }

        if include_declaration
            && !locations
                .iter()
                .any(|location| location.range == span_to_range(definition_span))
        {
            let location = Location {
                uri: current_uri.clone(),
                range: declaration_range,
            };
            if seen.insert(location_key(&location)) {
                locations.push(location);
            }
        }

        if !symbol.info.is_local {
            if let Some(reference_key) = reference_key_for_resolved_symbol(&document, &symbol) {
                self.ensure_workspace_cache().await;
                let cache = self.state.workspace_cache.read().await;
                for entry in cache.values() {
                    for reference in &entry.references {
                        if reference.key != reference_key {
                            continue;
                        }
                        if !include_declaration
                            && reference.location.uri == current_uri
                            && reference.location.range == declaration_range
                        {
                            continue;
                        }
                        if seen.insert(location_key(&reference.location)) {
                            locations.push(reference.location.clone());
                        }
                    }
                }
            }
        }

        Ok(Some(locations))
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let Some(document) = self
            .state
            .documents
            .read()
            .await
            .get(&params.text_document.uri)
            .cloned()
        else {
            return Ok(None);
        };

        let line = params.position.line as usize + 1;
        let column = params.position.character as usize + 1;
        let Some(symbol) = document.analysis.symbol_at(line, column) else {
            return Ok(None);
        };
        let current_name = identifier_at_position(&document.text, params.position)
            .unwrap_or_else(|| text_for_span(&document.text, symbol.span));
        if !is_valid_rename_identifier(&current_name) {
            return Ok(None);
        }

        Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
            range: span_to_range(symbol.span),
            placeholder: current_name,
        }))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        if !is_valid_rename_identifier(&params.new_name) {
            return Ok(None);
        }

        let text_position = params.text_document_position;
        let current_uri = text_position.text_document.uri;
        let Some(document) = self.state.documents.read().await.get(&current_uri).cloned() else {
            return Ok(None);
        };

        let line = text_position.position.line as usize + 1;
        let column = text_position.position.character as usize + 1;
        let Some(symbol) = document.analysis.symbol_at(line, column) else {
            return Ok(None);
        };
        let current_name = identifier_at_position(&document.text, text_position.position)
            .unwrap_or_else(|| text_for_span(&document.text, symbol.span));
        if !is_valid_rename_identifier(&current_name) {
            return Ok(None);
        }

        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
        let local_edits =
            rename_edits_for_document(&document, &symbol, &current_name, &params.new_name);
        if !local_edits.is_empty() {
            changes.insert(current_uri.clone(), local_edits);
        }

        if !symbol.info.is_local {
            if let Some(reference_key) = reference_key_for_resolved_symbol(&document, &symbol) {
                self.ensure_workspace_cache().await;
                let cache = self.state.workspace_cache.read().await;
                for (uri, entry) in cache.iter() {
                    if uri == &current_uri {
                        continue;
                    }
                    let mut edits = Vec::new();
                    let mut seen = HashSet::new();
                    for reference in &entry.references {
                        if reference.key != reference_key {
                            continue;
                        }
                        let key = format!(
                            "{}:{}:{}:{}",
                            reference.location.range.start.line,
                            reference.location.range.start.character,
                            reference.location.range.end.line,
                            reference.location.range.end.character
                        );
                        if seen.insert(key) {
                            edits.push(TextEdit {
                                range: reference.location.range,
                                new_text: params.new_name.clone(),
                            });
                        }
                    }
                    if !edits.is_empty() {
                        changes.insert(uri.clone(), edits);
                    }
                }
            }
        }

        if changes.is_empty() {
            return Ok(None);
        }

        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let Some(document) = self
            .state
            .documents
            .read()
            .await
            .get(&params.text_document.uri)
            .cloned()
        else {
            return Ok(None);
        };
        let Some(module) = &document.analysis.module else {
            return Ok(None);
        };

        Ok(Some(DocumentSymbolResponse::Nested(document_symbols(
            module,
        ))))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        self.ensure_workspace_cache().await;
        let symbols = self.workspace_symbols(&params.query).await;
        Ok(Some(symbols))
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let text_position = params.text_document_position_params;
        let Some(document) = self
            .state
            .documents
            .read()
            .await
            .get(&text_position.text_document.uri)
            .cloned()
        else {
            return Ok(None);
        };

        let line = text_position.position.line as usize + 1;
        let column = text_position.position.character as usize + 1;
        let Some(symbol) = document.analysis.symbol_at(line, column) else {
            return Ok(None);
        };
        let Some(definition_span) = symbol.info.def_span else {
            return Ok(None);
        };

        let mut highlights = Vec::new();
        for (span, info) in &document.analysis.symbols {
            if info.def_span != Some(definition_span) {
                continue;
            }

            let kind = if *span == definition_span {
                Some(DocumentHighlightKind::WRITE)
            } else {
                Some(DocumentHighlightKind::READ)
            };

            highlights.push(DocumentHighlight {
                range: span_to_range(*span),
                kind,
            });
        }

        if highlights.is_empty() {
            highlights.push(DocumentHighlight {
                range: span_to_range(definition_span),
                kind: Some(DocumentHighlightKind::WRITE),
            });
        }

        Ok(Some(highlights))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let text_position = params.text_document_position_params;
        let Some(document) = self
            .state
            .documents
            .read()
            .await
            .get(&text_position.text_document.uri)
            .cloned()
        else {
            return Ok(None);
        };
        let Some(module) = &document.analysis.module else {
            return Ok(None);
        };

        let line = text_position.position.line as usize + 1;
        let column = text_position.position.character as usize + 1;
        let Some(call_site) = find_call_site(module, line, column) else {
            return Ok(None);
        };
        let Some(label) = signature_label_for_call(&document, &call_site) else {
            return Ok(None);
        };

        let parameters = split_signature_parameters(&label)
            .into_iter()
            .map(|param| ParameterInformation {
                label: ParameterLabel::Simple(param),
                documentation: None,
            })
            .collect::<Vec<_>>();

        let active_parameter =
            active_parameter_index(&document.text, &call_site, text_position.position)
                .min(parameters.len().saturating_sub(1));

        Ok(Some(SignatureHelp {
            signatures: vec![SignatureInformation {
                label,
                documentation: None,
                parameters: Some(parameters),
                active_parameter: Some(active_parameter as u32),
            }],
            active_signature: Some(0),
            active_parameter: Some(active_parameter as u32),
        }))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let Some(document) = self
            .state
            .documents
            .read()
            .await
            .get(&params.text_document.uri)
            .cloned()
        else {
            return Ok(None);
        };

        let mut actions = Vec::new();
        for diagnostic in &params.context.diagnostics {
            if let Some(action) =
                quick_fix_for_diagnostic(&params.text_document.uri, &document, diagnostic)
            {
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }

        Ok((!actions.is_empty()).then_some(actions))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let Some(document) = self
            .state
            .documents
            .read()
            .await
            .get(&params.text_document.uri)
            .cloned()
        else {
            return Ok(None);
        };
        let Some(module) = &document.analysis.module else {
            return Ok(None);
        };

        let data = semantic_tokens_for_document(&document.text, module, &document.analysis);
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        })))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let Some(document) = self
            .state
            .documents
            .read()
            .await
            .get(&params.text_document.uri)
            .cloned()
        else {
            return Ok(None);
        };

        let raw_hints = collect_let_inlay_hints(&document.analysis);
        if raw_hints.is_empty() {
            return Ok(None);
        }

        let hints = raw_hints
            .into_iter()
            .map(|hint| {
                // Position the hint right after the variable name:
                // span starts at the `let` keyword (1-indexed), `let ` = 4 chars.
                let line = hint.let_span.start_location.line.saturating_sub(1) as u32;
                let col = (hint.let_span.start_location.column.saturating_sub(1)
                    + 4
                    + hint.name.len()) as u32;

                InlayHint {
                    position: Position::new(line, col),
                    label: InlayHintLabel::String(format!(": {}", hint.ty)),
                    kind: Some(InlayHintKind::TYPE),
                    text_edits: None,
                    tooltip: None,
                    padding_left: Some(false),
                    padding_right: Some(true),
                    data: None,
                }
            })
            .collect();

        Ok(Some(hints))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let documents = self.state.documents.read().await;
        let document = documents.get(&uri);

        let mut items: Vec<CompletionItem> = KEYWORDS
            .iter()
            .map(|keyword| CompletionItem {
                label: (*keyword).to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            })
            .collect();
        items.extend(std_api_completion_items());

        if let Some(document) = document {
            if let Some(module) = &document.analysis.module {
                for item in &module.items {
                    items.extend(item_to_completion_items(item));
                }
                items.extend(route_completion_items(module));
            }
        }

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
        match params.command.as_str() {
            COMMAND_RUN_DIAGNOSTICS => {
                if let Some(Value::String(uri_text)) = params.arguments.first() {
                    if let Ok(uri) = Url::parse(uri_text) {
                        self.reanalyze_document(&uri, true).await;
                    }
                }
            }
            COMMAND_LINT_WORKSPACE => {
                let folders: Vec<PathBuf> = params
                    .arguments
                    .iter()
                    .filter_map(|argument| argument.as_str())
                    .map(PathBuf::from)
                    .collect();
                self.run_workspace_lint(&folders).await;
            }
            _ => {}
        }

        Ok(None)
    }
}

