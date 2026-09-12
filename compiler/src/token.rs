use crate::span::Span;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Keyword {
    // Module system
    Module,
    Import,
    From,

    // Declarations
    Func,
    Returns,
    Async,
    Await,
    Record,
    Enum,
    Impl,
    Class,
    Trait,
    Let,

    // Visibility
    Public,
    Internal,
    Mut,

    // Special types
    SelfType, // Self keyword for referring to implementing type

    // Control flow - conditionals
    Match,
    Switch,
    Case,
    If,
    Else,
    When,
    Then,
    Otherwise,
    Unless,

    // Control flow - loops
    While,
    Do,
    For,
    In,
    Loop,

    // Control flow - jumps
    Return,
    Break,
    Continue,

    // Word-form logical operators
    AndWord,
    OrWord,
    NotWord,

    // Literals
    True,
    False,

    // Type/value declarations
    Type,
    Const,
    Static,

    // Type casting / dynamic dispatch
    As,
    Dyn,
}

impl Keyword {
    pub fn from_identifier(identifier: &str) -> Option<Self> {
        match identifier {
            // Module system
            "module" => Some(Self::Module),
            "import" => Some(Self::Import),
            "from" => Some(Self::From),

            // Declarations
            "func" => Some(Self::Func),
            "returns" => Some(Self::Returns),
            "async" => Some(Self::Async),
            "await" => Some(Self::Await),
            "record" => Some(Self::Record),
            "enum" => Some(Self::Enum),
            "impl" => Some(Self::Impl),
            "class" => Some(Self::Class),
            "trait" => Some(Self::Trait),
            "let" => Some(Self::Let),

            // Visibility
            "public" => Some(Self::Public),
            "internal" => Some(Self::Internal),
            "mut" => Some(Self::Mut),

            // Special types
            "Self" => Some(Self::SelfType),

            // Control flow - conditionals
            "match" => Some(Self::Match),
            "switch" => Some(Self::Switch),
            "case" => Some(Self::Case),
            "if" => Some(Self::If),
            "else" => Some(Self::Else),
            "when" => Some(Self::When),
            "then" => Some(Self::Then),
            "otherwise" => Some(Self::Otherwise),
            "unless" => Some(Self::Unless),

            // Control flow - loops
            "while" => Some(Self::While),
            "do" => Some(Self::Do),
            "for" => Some(Self::For),
            "in" => Some(Self::In),
            "loop" => Some(Self::Loop),
            // Control flow - jumps
            "return" => Some(Self::Return),
            "break" => Some(Self::Break),
            "continue" => Some(Self::Continue),

            // Word-form logical operators
            "and" => Some(Self::AndWord),
            "or" => Some(Self::OrWord),
            "not" => Some(Self::NotWord),

            // Literals
            "true" => Some(Self::True),
            "false" => Some(Self::False),

            // Type/value declarations
            "type" => Some(Self::Type),
            "const" => Some(Self::Const),
            "static" => Some(Self::Static),

            // Type casting / dynamic dispatch
            "as" => Some(Self::As),
            "dyn" => Some(Self::Dyn),

            _ => None,
        }
    }
}

impl fmt::Display for Keyword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            // Module system
            Keyword::Module => "module",
            Keyword::Import => "import",
            Keyword::From => "from",

            // Declarations
            Keyword::Func => "func",
            Keyword::Returns => "returns",
            Keyword::Async => "async",
            Keyword::Await => "await",
            Keyword::Record => "record",
            Keyword::Enum => "enum",
            Keyword::Impl => "impl",
            Keyword::Class => "class",
            Keyword::Trait => "trait",
            Keyword::Let => "let",

            // Visibility
            Keyword::Public => "public",
            Keyword::Internal => "internal",
            Keyword::Mut => "mut",

            // Special types
            Keyword::SelfType => "Self",

            // Control flow - conditionals
            Keyword::Match => "match",
            Keyword::Switch => "switch",
            Keyword::Case => "case",
            Keyword::If => "if",
            Keyword::Else => "else",
            Keyword::When => "when",
            Keyword::Then => "then",
            Keyword::Otherwise => "otherwise",
            Keyword::Unless => "unless",

            // Control flow - loops
            Keyword::While => "while",
            Keyword::Do => "do",
            Keyword::For => "for",
            Keyword::In => "in",
            Keyword::Loop => "loop",
            // Control flow - jumps
            Keyword::Return => "return",
            Keyword::Break => "break",
            Keyword::Continue => "continue",

            // Word-form logical operators
            Keyword::AndWord => "and",
            Keyword::OrWord => "or",
            Keyword::NotWord => "not",

            // Literals
            Keyword::True => "true",
            Keyword::False => "false",

            // Type/value declarations
            Keyword::Type => "type",
            Keyword::Const => "const",
            Keyword::Static => "static",

            // Type casting / dynamic dispatch
            Keyword::As => "as",
            Keyword::Dyn => "dyn",
        };

        write!(f, "{}", text)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Identifier(String),
    Number(String),
    Keyword(Keyword),
    Symbol(char),
    Operator(Operator),
    StringLiteral(String),
    /// Character literal: 'a', '\n', etc.
    CharLiteral(char),
    /// F-string raw template: f"Hello, {name}!"
    FStringLiteral(String),
    EndOfFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Operator {
    // Comparison
    EqualEqual,   // ==
    NotEqual,     // !=
    LessEqual,    // <=
    GreaterEqual, // >=

    // Logical
    And, // &&
    Or,  // ||

    // Range operators
    Range,          // ..
    RangeInclusive, // ..=
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

impl fmt::Display for Operator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Operator::EqualEqual => "==",
            Operator::NotEqual => "!=",
            Operator::LessEqual => "<=",
            Operator::GreaterEqual => ">=",
            Operator::And => "&&",
            Operator::Or => "||",
            Operator::Range => "..",
            Operator::RangeInclusive => "..=",
        };

        write!(f, "{}", text)
    }
}
