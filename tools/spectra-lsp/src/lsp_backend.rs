impl Backend {
    async fn update_config(&self, value: Value) {
        let settings = value.get("spectra").cloned().unwrap_or(value);

        let mut config = self.state.config.write().await;
        if let Some(cli_path) = settings.get("cliPath").and_then(Value::as_str) {
            let trimmed = cli_path.trim();
            if !trimmed.is_empty() {
                config.cli_path = trimmed.to_string();
            }
        }
        if let Some(lint_on_save) = settings.get("lintOnSave").and_then(Value::as_bool) {
            config.lint_on_save = lint_on_save;
        }
    }

    async fn analyze_and_store(&self, uri: Url, text: String, include_lints: bool) {
        do_analyze_and_store(&self.client, &self.state, uri, text, include_lints).await;
    }

    async fn reanalyze_document(&self, uri: &Url, include_lints: bool) {
        if let Some(document) = self.state.documents.read().await.get(uri).cloned() {
            self.analyze_and_store(uri.clone(), document.text, include_lints)
                .await;
            return;
        }

        if let Ok(path) = uri.to_file_path() {
            match fs::read_to_string(&path) {
                Ok(text) => {
                    self.analyze_and_store(uri.clone(), text, include_lints)
                        .await
                }
                Err(error) => {
                    self.client
                        .log_message(
                            MessageType::ERROR,
                            format!("Falha ao ler '{}': {}", path.display(), error),
                        )
                        .await;
                }
            }
        }
    }

    async fn run_workspace_lint(&self, folders: &[PathBuf]) {
        let folder_list = if folders.is_empty() {
            self.state.workspace_folders.read().await.clone()
        } else {
            folders.to_vec()
        };

        for folder in folder_list {
            let mut files = Vec::new();
            collect_spectra_files(&folder, &mut files);
            for file in files {
                let Ok(text) = fs::read_to_string(&file) else {
                    continue;
                };
                let Ok(uri) = Url::from_file_path(&file) else {
                    continue;
                };
                self.analyze_and_store(uri, text, true).await;
            }
        }
    }

    async fn workspace_symbols(&self, query: &str) -> Vec<SymbolInformation> {
        let normalized_query = query.trim().to_ascii_lowercase();
        let cache = self.state.workspace_cache.read().await;
        let mut results = Vec::new();

        for (uri, entry) in cache.iter() {
            for symbol in &entry.symbols {
                let haystack = format!(
                    "{} {}",
                    symbol.name.to_ascii_lowercase(),
                    symbol
                        .detail
                        .clone()
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                );
                if !normalized_query.is_empty() && !haystack.contains(&normalized_query) {
                    continue;
                }

                results.push(workspace_symbol_information(uri, symbol));
            }
        }

        results
    }

    async fn update_workspace_cache(
        &self,
        uri: &Url,
        text: String,
        analysis: DocumentAnalysis,
        modified: Option<SystemTime>,
    ) {
        let symbols = analysis
            .module
            .as_ref()
            .map(|module| workspace_symbol_entries_for_module(&text, module))
            .unwrap_or_default();
        let references = reference_entries_for_analysis(uri, &analysis);

        self.state.workspace_cache.write().await.insert(
            uri.clone(),
            WorkspaceCacheEntry {
                symbols,
                references,
                modified,
            },
        );
    }

    async fn ensure_workspace_cache(&self) {
        let folders = self.state.workspace_folders.read().await.clone();
        let open_documents = self.state.documents.read().await.clone();
        let mut known = HashSet::new();

        for folder in folders {
            let mut files = Vec::new();
            collect_spectra_files(&folder, &mut files);
            for file in files {
                let Ok(uri) = Url::from_file_path(&file) else {
                    continue;
                };
                known.insert(uri.clone());

                if let Some(document) = open_documents.get(&uri) {
                    self.update_workspace_cache(
                        &uri,
                        document.text.clone(),
                        document.analysis.clone(),
                        None,
                    )
                    .await;
                    continue;
                }

                let modified = fs::metadata(&file).and_then(|meta| meta.modified()).ok();
                let should_refresh = {
                    let cache = self.state.workspace_cache.read().await;
                    cache
                        .get(&uri)
                        .map(|entry| entry.modified != modified)
                        .unwrap_or(true)
                };

                if !should_refresh {
                    continue;
                }

                let Ok(text) = fs::read_to_string(&file) else {
                    continue;
                };
                let analysis = analyze_cache_document(&text, &file.to_string_lossy());
                self.update_workspace_cache(&uri, text, analysis, modified)
                    .await;
            }
        }

        self.state
            .workspace_cache
            .write()
            .await
            .retain(|uri, _| known.contains(uri) || open_documents.contains_key(uri));
    }

    async fn refresh_cache_from_disk(&self, uri: &Url) {
        let Ok(path) = uri.to_file_path() else {
            return;
        };
        let Ok(text) = fs::read_to_string(&path) else {
            self.state.workspace_cache.write().await.remove(uri);
            return;
        };

        let analysis = analyze_cache_document(&text, &path.to_string_lossy());
        let modified = fs::metadata(&path).and_then(|meta| meta.modified()).ok();
        self.update_workspace_cache(uri, text, analysis, modified)
            .await;
    }

    async fn format_document(
        &self,
        uri: &Url,
        text: &str,
    ) -> std::result::Result<Option<String>, String> {
        let config = self.state.config.read().await.clone();
        let workspace_folders = self.state.workspace_folders.read().await.clone();
        let cwd = uri
            .to_file_path()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .or_else(|| workspace_folders.first().cloned());

        let mut command = Command::new(&config.cli_path);
        command
            .arg("fmt")
            .arg("--stdin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }

        let mut child = command.spawn().map_err(|error| {
            format!(
                "Falha ao iniciar formatter '{}': {}",
                config.cli_path, error
            )
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .await
                .map_err(|error| format!("Falha ao enviar conteúdo ao formatter: {}", error))?;
        }

        let output = child
            .wait_with_output()
            .await
            .map_err(|error| format!("Falha ao aguardar formatter: {}", error))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                format!("Formatter '{}' terminou com erro.", config.cli_path)
            } else {
                stderr
            });
        }

        let formatted = String::from_utf8(output.stdout)
            .map_err(|error| format!("Saída inválida do formatter: {}", error))?;
        if formatted == text {
            Ok(None)
        } else {
            Ok(Some(formatted))
        }
    }
}

