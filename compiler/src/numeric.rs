//! Conversion of numeric literal token text into runtime values.
//!
//! The lexer keeps the raw text of a numeric literal (including radix
//! prefixes, digit separators, and exponent notation) inside the
//! [`TokenKind::Number`](crate::token::TokenKind::Number) payload.  All layers
//! that need an actual value — semantic constant folding and mid-end lowering
//! — go through this module so every form is interpreted identically.

/// A parsed numeric literal value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParsedNumber {
    Int(i64),
    Float(f64),
}

/// Parses the body of a numeric literal token.
///
/// Supported forms (as lexed by `crate::lexer`):
/// - decimal integers with `_` separators: `1_000_000`
/// - fractional literals: `3.25`, `.digit` forms are not produced by the lexer
/// - scientific notation: `1e5`, `2.5E-3`, `7e+10`
/// - radix integers: hexadecimal `0x[0-9a-fA-F_]`, octal `0o[0-7_]`,
///   binary `0b[01_]`
///
/// Radix-prefixed literals are always integral; `_` separators are ignored;
/// scientific notation yields [`ParsedNumber::Float`].  Returns `None` when
/// the text does not correspond to any supported value.
pub fn parse_number_literal(text: &str) -> Option<ParsedNumber> {
    let cleaned = text.replace('_', "");
    let lower = cleaned.to_ascii_lowercase();

    if let Some(digits) = lower.strip_prefix("0x") {
        return i64::from_str_radix(digits, 16).ok().map(ParsedNumber::Int);
    }
    if let Some(digits) = lower.strip_prefix("0o") {
        return i64::from_str_radix(digits, 8).ok().map(ParsedNumber::Int);
    }
    if let Some(digits) = lower.strip_prefix("0b") {
        return i64::from_str_radix(digits, 2).ok().map(ParsedNumber::Int);
    }

    if let Ok(int_value) = cleaned.parse::<i64>() {
        return Some(ParsedNumber::Int(int_value));
    }
    cleaned.parse::<f64>().ok().map(ParsedNumber::Float)
}

/// Reports whether a numeric literal token classifies as floating point.
///
/// Fractional dots and scientific exponents make a literal a float; radix
/// prefixes are integers even when their digits contain `e`/`E` (hex digits).
pub fn number_literal_is_float(text: &str) -> bool {
    let cleaned = text.replace('_', "");
    let lower = cleaned.to_ascii_lowercase();
    if lower.starts_with("0x") || lower.starts_with("0o") || lower.starts_with("0b") {
        return false;
    }
    cleaned.contains('.') || lower.contains('e')
}

/// Parses an integer-only literal with wide precision for constant folding
/// paths that must observe values beyond `i64` bounds before rejecting them.
pub fn parse_number_literal_as_i128(text: &str) -> Option<i128> {
    let cleaned = text.replace('_', "");
    let lower = cleaned.to_ascii_lowercase();

    if let Some(digits) = lower.strip_prefix("0x") {
        return i128::from_str_radix(digits, 16).ok();
    }
    if let Some(digits) = lower.strip_prefix("0o") {
        return i128::from_str_radix(digits, 8).ok();
    }
    if let Some(digits) = lower.strip_prefix("0b") {
        return i128::from_str_radix(digits, 2).ok();
    }

    // Float forms do not participate in integer constant expressions.
    cleaned.parse::<i128>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decimal_integer_and_float_forms() {
        assert_eq!(parse_number_literal("42"), Some(ParsedNumber::Int(42)));
        assert_eq!(
            parse_number_literal("1_000_000"),
            Some(ParsedNumber::Int(1_000_000))
        );
        // The lexer never produces a leading sign, but Rust's parser accepts
        // one; keep it valid input rather than asserting rejection.
        assert_eq!(parse_number_literal("-7"), Some(ParsedNumber::Int(-7)));
        assert_eq!(parse_number_literal("2.5"), Some(ParsedNumber::Float(2.5)));
    }

    #[test]
    fn parses_scientific_notation_as_float() {
        assert_eq!(
            parse_number_literal("1e5"),
            Some(ParsedNumber::Float(100000.0))
        );
        assert_eq!(
            parse_number_literal("2.5E-3"),
            Some(ParsedNumber::Float(0.0025))
        );
        assert_eq!(
            parse_number_literal("7E+10"),
            Some(ParsedNumber::Float(7.0e10))
        );
    }

    #[test]
    fn parses_radix_integers() {
        assert_eq!(parse_number_literal("0xFF"), Some(ParsedNumber::Int(255)));
        assert_eq!(
            parse_number_literal("0xDE_AD"),
            Some(ParsedNumber::Int(57005))
        );
        assert_eq!(parse_number_literal("0o17"), Some(ParsedNumber::Int(15)));
        assert_eq!(parse_number_literal("0b1011"), Some(ParsedNumber::Int(11)));
        assert_eq!(
            parse_number_literal("0b1010_0001"),
            Some(ParsedNumber::Int(161))
        );
    }

    #[test]
    fn rejects_malformed_literals() {
        assert_eq!(parse_number_literal(""), None);
        assert_eq!(parse_number_literal("0x"), None);
        assert_eq!(parse_number_literal("0b2"), None);
        assert_eq!(parse_number_literal("abc"), None);
    }

    #[test]
    fn classifies_float_forms() {
        assert!(!number_literal_is_float("42"));
        assert!(!number_literal_is_float("1_000"));
        assert!(!number_literal_is_float("0xBEEF")); // hex digits contain e/f
        assert!(!number_literal_is_float("0o17"));
        assert!(number_literal_is_float("2.5"));
        assert!(number_literal_is_float("1e5"));
        assert!(number_literal_is_float("2.5E-3"));
    }
}
