use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    InvalidIdentifier(String),
    EmptyQuery(&'static str),
    DuplicateColumn(String),
    ColumnValueMismatch { columns: usize, values: usize },
    MissingAssignments,
    MissingPredicate,
    NegativeLimit,
    NegativeOffset,
    InvalidParameter(String),
}

impl QueryError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidIdentifier(_) => "DB2502_INVALID_IDENTIFIER",
            Self::EmptyQuery(_) => "DB2502_EMPTY_QUERY",
            Self::DuplicateColumn(_) => "DB2502_DUPLICATE_COLUMN",
            Self::ColumnValueMismatch { .. } => "DB2502_COLUMN_VALUE_MISMATCH",
            Self::MissingAssignments => "DB2502_MISSING_ASSIGNMENTS",
            Self::MissingPredicate => "DB2502_MISSING_PREDICATE",
            Self::NegativeLimit => "DB2502_NEGATIVE_LIMIT",
            Self::NegativeOffset => "DB2502_NEGATIVE_OFFSET",
            Self::InvalidParameter(_) => "DB2502_INVALID_PARAMETER",
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifier(value) => {
                write!(f, "{}: invalid identifier `{value}`", self.code())
            }
            Self::EmptyQuery(kind) => write!(f, "{}: empty {kind} query", self.code()),
            Self::DuplicateColumn(value) => {
                write!(f, "{}: duplicate column `{value}`", self.code())
            }
            Self::ColumnValueMismatch { columns, values } => {
                write!(f, "{}: {columns} columns but {values} values", self.code())
            }
            Self::MissingAssignments => write!(f, "{}: update has no assignments", self.code()),
            Self::MissingPredicate => {
                write!(f, "{}: destructive query requires a predicate", self.code())
            }
            Self::NegativeLimit => write!(f, "{}: limit cannot be negative", self.code()),
            Self::NegativeOffset => write!(f, "{}: offset cannot be negative", self.code()),
            Self::InvalidParameter(message) => write!(f, "{}: {message}", self.code()),
        }
    }
}

impl std::error::Error for QueryError {}
