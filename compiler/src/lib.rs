pub mod ast;
pub mod embedded_stdlib;
pub mod error;
pub mod language_service;
pub mod lexer;
pub mod lint;
pub mod numeric;
pub mod parser;
pub mod pipeline;
pub mod semantic;
pub mod span;
pub mod token;

pub use ast::{BinaryOperator, Module, UnaryOperator};
pub use embedded_stdlib::{
    embedded_stdlib_bundle_id, embedded_stdlib_source, embedded_stdlib_sources,
    has_embedded_stdlib_module, EmbeddedStdlibSource,
};
pub use error::{BackendError, CompilerError, LexError, MidendError, ParseError, SemanticError};
pub use language_service::{
    analyze_document, collect_let_inlay_hints, DefinitionInfo, DocumentAnalysis, LetInlayHint,
    ResolvedSymbol,
};
pub use lexer::Lexer;
pub use lint::{LintDiagnostic, LintOptions, LintRule};
pub use parser::Parser;
pub use pipeline::{
    BackendDriver, CompilationOptions, CompilationPipeline, CompilationResult, DebugInfoMode,
    NoopBackend,
};
pub use semantic::analyze_modules;
pub use span::{span_union, Location, Span};
pub use token::{Keyword, Operator, Token, TokenKind};

pub type LexResult = Result<Vec<Token>, Vec<LexError>>;
pub type ParseResult = Result<Module, Vec<ParseError>>;
pub type SemanticResult = Result<(), Vec<SemanticError>>;
