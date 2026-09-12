use super::error::QueryError;

pub trait Dialect {
    fn quote_identifier(&self, identifier: &str) -> Result<String, QueryError>;
    fn placeholder(&self, index: usize) -> String;
    /// Whether `OFFSET` is valid without an explicit `LIMIT`.
    ///
    /// Dialects answering `false` require callers to emit an unbounded
    /// `LIMIT` (see [`Dialect::unbounded_limit`]) before the offset clause.
    fn supports_offset_without_limit(&self) -> bool {
        true
    }
    /// Placeholder-free `LIMIT` standing in for "no upper bound".
    fn unbounded_limit(&self) -> &'static str {
        "-1"
    }
}

/// Validates an identifier and quotes each dot-separated part.
///
/// Qualified references such as `table.column` are split so each component is
/// quoted independently; identifiers that legitimately contain a literal dot
/// are not representable and must not be passed here.
fn quote_parts(identifier: &str) -> Result<String, QueryError> {
    if identifier.is_empty()
        || identifier.len() > 128
        || identifier.chars().any(|ch| ch == '\0' || ch.is_control())
    {
        return Err(QueryError::InvalidIdentifier(identifier.to_owned()));
    }
    identifier
        .split('.')
        .map(|part| {
            if part.is_empty() {
                return Err(QueryError::InvalidIdentifier(identifier.to_owned()));
            }
            let escaped = part.replace('"', "\"\"");
            Ok(format!("\"{escaped}\""))
        })
        .collect::<Result<Vec<_>, QueryError>>()
        .map(|parts| parts.join("."))
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SqliteDialect;

#[derive(Debug, Clone, Copy, Default)]
pub struct PostgresDialect;

impl Dialect for PostgresDialect {
    fn quote_identifier(&self, identifier: &str) -> Result<String, QueryError> {
        quote_parts(identifier)
    }

    fn placeholder(&self, index: usize) -> String {
        format!("${index}")
    }
}

impl Dialect for SqliteDialect {
    fn quote_identifier(&self, identifier: &str) -> Result<String, QueryError> {
        quote_parts(identifier)
    }

    fn placeholder(&self, index: usize) -> String {
        format!("?{index}")
    }

    fn supports_offset_without_limit(&self) -> bool {
        // SQLite's grammar requires LIMIT before OFFSET.
        false
    }
}
