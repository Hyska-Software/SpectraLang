use serde_json::Value;
use spectra_compiler::{
    analyze_document, collect_let_inlay_hints, CompilationOptions, CompilerError, DocumentAnalysis,
    LintDiagnostic, Span,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

const COMMAND_RUN_DIAGNOSTICS: &str = "spectra.diagnostics.run";
const COMMAND_LINT_WORKSPACE: &str = "spectra.lintWorkspace";
const KEYWORDS: &[&str] = &[
    "module",
    "import",
    "from",
    "func",
    "async",
    "await",
    "record",
    "enum",
    "impl",
    "trait",
    "let",
    "public",
    "internal",
    "mut",
    "Self",
    "match",
    "switch",
    "case",
    "if",
    "else",
    "when",
    "then",
    "otherwise",
    "while",
    "do",
    "for",
    "in",
    "loop",
    "return",
    "break",
    "continue",
    "and",
    "or",
    "not",
    "true",
    "false",
    "type",
    "const",
    "static",
    "as",
    "dyn",
];

#[derive(Debug, Clone)]
struct ServerConfig {
    cli_path: String,
    lint_on_save: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            cli_path: "spectralang".to_string(),
            lint_on_save: true,
        }
    }
}

#[derive(Debug, Clone)]
struct DocumentState {
    text: String,
    analysis: DocumentAnalysis,
}

#[derive(Debug, Clone)]
struct CachedWorkspaceSymbol {
    name: String,
    detail: Option<String>,
    kind: SymbolKind,
    span: Span,
    container_name: Option<String>,
}

#[derive(Debug, Clone)]
struct CachedReference {
    key: String,
    location: Location,
}

#[derive(Debug, Clone)]
struct WorkspaceCacheEntry {
    symbols: Vec<CachedWorkspaceSymbol>,
    references: Vec<CachedReference>,
    modified: Option<SystemTime>,
}

#[derive(Debug, Default)]
struct BackendState {
    documents: RwLock<HashMap<Url, DocumentState>>,
    workspace_cache: RwLock<HashMap<Url, WorkspaceCacheEntry>>,
    workspace_folders: RwLock<Vec<PathBuf>>,
    config: RwLock<ServerConfig>,
    debounce_handles: tokio::sync::Mutex<HashMap<Url, tokio::task::AbortHandle>>,
}

#[derive(Debug)]
struct Backend {
    client: Client,
    state: Arc<BackendState>,
}

include!("lsp_server.rs");
include!("lsp_backend.rs");
include!("lsp_analysis.rs");
include!("lsp_completion.rs");
include!("lsp_hover.rs");
include!("lsp_symbols.rs");
include!("lsp_tokens_calls.rs");
include!("lsp_cache_rename.rs");
include!("lsp_semantic_tokens.rs");
include!("lsp_quickfix.rs");
include!("lsp_entry.rs");
include!("lsp_tests.rs");
