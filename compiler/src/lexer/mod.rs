use crate::{
    error::LexError,
    span::{Location, Span},
    token::{Keyword, Operator, Token, TokenKind},
};

pub struct Lexer<'source> {
    source: &'source str,
    /// Byte offset of `source[0]` inside the enclosing file. Zero for a
    /// normal top-level lex; f-string interpolation sub-parsing sets it so
    /// the inner expression receives absolute, file-unique spans instead of
    /// restarting at offset 0 (where every interpolation would collide in
    /// span-keyed tables such as the semantic type facts).
    origin_offset: usize,
    /// 1-based line/column of `source[0]` inside the enclosing file.
    origin_location: Location,
}

/// Indexed character access used by the tokenizer.
///
/// Spectra's identifiers and punctuation are ASCII, which is also the common
/// case for source files.  The old lexer eagerly materialized a `(byte,
/// char)` tuple for every character, doubling the live memory of an ASCII
/// source before tokenization even began.  Keep indexed access for the
/// existing scanner, but read ASCII directly from the source bytes and only
/// allocate a Unicode index for genuinely non-ASCII input.
struct CharacterTable<'source> {
    source: &'source str,
    unicode: Option<Vec<(usize, char)>>,
}

impl<'source> CharacterTable<'source> {
    fn new(source: &'source str) -> Self {
        let unicode = if source.is_ascii() {
            None
        } else {
            Some(source.char_indices().collect())
        };
        Self { source, unicode }
    }

    fn len(&self) -> usize {
        self.unicode
            .as_ref()
            .map_or(self.source.len(), Vec::len)
    }

    fn get(&self, index: usize) -> Option<(usize, char)> {
        if let Some(unicode) = &self.unicode {
            return unicode.get(index).copied();
        }
        self.source
            .as_bytes()
            .get(index)
            .map(|byte| (index, *byte as char))
    }
}

impl<'source> Lexer<'source> {
    pub fn new(source: &'source str) -> Self {
        Self {
            source,
            origin_offset: 0,
            origin_location: Location::new(1, 1),
        }
    }

    /// Lex `source` as if it began at `origin_offset` / `origin_location` of
    /// the enclosing file. Used to lex one f-string interpolation so its
    /// tokens keep positions inside the real source text.
    pub fn with_origin(
        source: &'source str,
        origin_offset: usize,
        origin_location: Location,
    ) -> Self {
        Self {
            source,
            origin_offset,
            origin_location,
        }
    }

    pub fn tokenize(&self) -> Result<Vec<Token>, Vec<LexError>> {
        let mut tokens = Vec::new();
        let mut errors = Vec::new();

        let characters = CharacterTable::new(self.source);
        let mut index = 0;
        let length = characters.len();
        let mut line = self.origin_location.line;
        let mut column = self.origin_location.column;

        while index < length {
            let Some((offset, ch)) = characters.get(index) else {
                break;
            };
            let start_location = Location::new(line, column);

            match ch {
                ' ' | '\t' | '\r' => {
                    bump_position(ch, &mut line, &mut column);
                    index += 1;
                }
                '\n' => {
                    bump_position(ch, &mut line, &mut column);
                    index += 1;
                }
                '/' if index + 1 < length
                    && characters.get(index + 1).is_some_and(|(_, ch)| ch == '/') =>
                {
                    // Consume line comment start
                    bump_position('/', &mut line, &mut column);
                    index += 1;
                    bump_position('/', &mut line, &mut column);
                    index += 1;

                    while index < length {
                        let Some((_, comment_char)) = characters.get(index) else {
                            break;
                        };
                        if comment_char == '\n' {
                            break;
                        }
                        bump_position(comment_char, &mut line, &mut column);
                        index += 1;
                    }
                }
                '/' if index + 1 < length
                    && characters.get(index + 1).is_some_and(|(_, ch)| ch == '*') =>
                {
                    // Consume block comment /* ... */
                    bump_position('/', &mut line, &mut column);
                    index += 1;
                    bump_position('*', &mut line, &mut column);
                    index += 1;

                    let mut closed = false;
                    while index < length {
                        let Some((_, comment_char)) = characters.get(index) else {
                            break;
                        };
                        if comment_char == '*'
                            && index + 1 < length
                            && characters.get(index + 1).is_some_and(|(_, ch)| ch == '/')
                        {
                            bump_position('*', &mut line, &mut column);
                            index += 1;
                            bump_position('/', &mut line, &mut column);
                            index += 1;
                            closed = true;
                            break;
                        }
                        bump_position(comment_char, &mut line, &mut column);
                        index += 1;
                    }

                    if !closed {
                        let end_location = Location::new(line, column);
                        errors.push(
                            LexError::new(
                                "unterminated block comment",
                                Span::new(offset, self.source.len(), start_location, end_location),
                            )
                            .with_code("L006")
                            .with_hint("Close the block comment with `*/`."),
                        );
                    }
                }
                ch if is_identifier_start(ch) => {
                    // Special case: f"..." is an f-string literal, not an identifier
                    if ch == 'f'
                        && index + 1 < length
                        && characters.get(index + 1).is_some_and(|(_, ch)| ch == '"')
                    {
                        // Consume 'f'
                        bump_position('f', &mut line, &mut column);
                        index += 1;
                        // Consume '"'
                        bump_position('"', &mut line, &mut column);
                        index += 1;

                        // Scan f-string content, preserving {expr} parts as-is
                        let mut raw_content = String::new();
                        let mut terminated = false;

                        while index < length {
                            let Some((_, sc)) = characters.get(index) else {
                                break;
                            };
                            if sc == '\\' && index + 1 < length {
                                let Some((_, escaped)) = characters.get(index + 1) else {
                                    break;
                                };
                                let escape_offset = characters
                                    .get(index)
                                    .map(|(offset, _)| offset)
                                    .unwrap_or(self.source.len());
                                let escape_start = Location::new(line, column);
                                bump_position('\\', &mut line, &mut column);
                                bump_position(escaped, &mut line, &mut column);
                                match escaped {
                                    '"' => raw_content.push('"'),
                                    '\'' => raw_content.push('\''),
                                    '\\' => raw_content.push('\\'),
                                    'n' => raw_content.push('\n'),
                                    't' => raw_content.push('\t'),
                                    'r' => raw_content.push('\r'),
                                    '0' => raw_content.push('\0'),
                                    other => {
                                        // Unknown escape: keep the characters in the
                                        // raw content but report a coded lexical error.
                                        raw_content.push('\\');
                                        raw_content.push(other);
                                        errors.push(
                                            LexError::new(
                                                format!("unknown escape sequence `\\{}`", other),
                                                Span::new(
                                                    escape_offset,
                                                    escape_offset + 1 + other.len_utf8(),
                                                    escape_start,
                                                    Location::new(line, column),
                                                ),
                                            )
                                            .with_code("L008")
                                            .with_hint(
                                                "Supported escapes are \\n, \\t, \\r, \\\\, \\', \\\" and \\0.",
                                            ),
                                        );
                                    }
                                }
                                index += 2;
                            } else if sc == '"' {
                                bump_position('"', &mut line, &mut column);
                                index += 1;
                                terminated = true;
                                break;
                            } else {
                                bump_position(sc, &mut line, &mut column);
                                raw_content.push(sc);
                                index += 1;
                            }
                        }

                        let end_offset = if index < length {
                            characters
                                .get(index)
                                .map(|(offset, _)| offset)
                                .unwrap_or(self.source.len())
                        } else {
                            self.source.len()
                        };
                        let end_location = Location::new(line, column);

                        if terminated {
                            tokens.push(Token::new(
                                TokenKind::FStringLiteral(raw_content),
                                Span::new(offset, end_offset, start_location, end_location),
                            ));
                        } else {
                            errors.push(
                                LexError::new(
                                    "unterminated f-string literal",
                                    Span::new(
                                        offset,
                                        self.source.len(),
                                        start_location,
                                        end_location,
                                    ),
                                )
                                .with_code("L005")
                                .with_hint("Close the f-string with a matching \" character."),
                            );
                        }
                        continue;
                    }

                    // Regular identifier or keyword
                    bump_position(ch, &mut line, &mut column);
                    let mut end_index = index + 1;
                    while end_index < length {
                        let Some((_, next_char)) = characters.get(end_index) else {
                            break;
                        };
                        if is_identifier_continue(next_char) {
                            bump_position(next_char, &mut line, &mut column);
                            end_index += 1;
                        } else {
                            break;
                        }
                    }

                    let end_offset = if end_index < length {
                        characters
                            .get(end_index)
                            .map(|(offset, _)| offset)
                            .unwrap_or(self.source.len())
                    } else {
                        self.source.len()
                    };
                    let end_location = Location::new(line, column);
                    let text = &self.source[offset..end_offset];
                    let token_kind = Keyword::from_identifier(text)
                        .map(TokenKind::Keyword)
                        .unwrap_or_else(|| TokenKind::Identifier(text.to_string()));
                    tokens.push(Token::new(
                        token_kind,
                        Span::new(offset, end_offset, start_location, end_location),
                    ));
                    index = end_index;
                }
                ch if ch.is_ascii_digit() => {
                    bump_position(ch, &mut line, &mut column);
                    let mut end_index = index + 1;

                    // Radix prefixes: 0x (hexadecimal), 0o (octal), 0b (binary).
                    // The prefix selects which digit alphabet plus `_` may follow.
                    let radix = if ch == '0' && end_index < length {
                        match characters.get(end_index).map(|(_, ch)| ch) {
                            Some('x') => Some((16u32, "hexadecimal")),
                            Some('o') => Some((8u32, "octal")),
                            Some('b') => Some((2u32, "binary")),
                            _ => None,
                        }
                    } else {
                        None
                    };

                    // A malformed literal still consumes its characters so the
                    // cursor stays coherent, but no Number token is produced.
                    let mut fatal = false;

                    match radix {
                        Some((_radix_value, radix_name)) => {
                            if let Some((_, prefix)) = characters.get(end_index) {
                                bump_position(prefix, &mut line, &mut column);
                            }
                            end_index += 1;

                            let is_radix_digit = |c: char| match _radix_value {
                                16 => c.is_ascii_hexdigit(),
                                8 => ('0'..='7').contains(&c),
                                _ => c == '0' || c == '1',
                            };

                            let mut saw_digit = false;
                            let mut prev_was_digit = false;
                            while end_index < length {
                                let Some((_, next_char)) = characters.get(end_index) else {
                                    break;
                                };
                                if is_radix_digit(next_char) {
                                    saw_digit = true;
                                    prev_was_digit = true;
                                    bump_position(next_char, &mut line, &mut column);
                                    end_index += 1;
                                } else if next_char == '_' {
                                    let separator_ok = prev_was_digit
                                        && end_index + 1 < length
                                        && characters
                                            .get(end_index + 1)
                                            .is_some_and(|(_, ch)| is_radix_digit(ch));
                                    let separator_offset = characters
                                        .get(end_index)
                                        .map(|(offset, _)| offset)
                                        .unwrap_or(self.source.len());
                                    let separator_start = Location::new(line, column);
                                    bump_position('_', &mut line, &mut column);
                                    end_index += 1;
                                    if !separator_ok {
                                        errors.push(
                                            LexError::new(
                                                format!(
                                                    "invalid `_` placement in {} literal",
                                                    radix_name
                                                ),
                                                Span::new(
                                                    separator_offset,
                                                    separator_offset + 1,
                                                    separator_start,
                                                    Location::new(line, column),
                                                ),
                                            )
                                            .with_code("L007")
                                            .with_hint(
                                                "`_` separates groups of digits and cannot lead, trail, or repeat.",
                                            ),
                                        );
                                        fatal = true;
                                    }
                                    prev_was_digit = false;
                                } else {
                                    break;
                                }
                            }

                            if !saw_digit {
                                errors.push(
                                    LexError::new(
                                        format!("missing digits in {} literal", radix_name),
                                        Span::new(
                                            offset,
                                            self.source.len(),
                                            start_location,
                                            Location::new(line, column),
                                        ),
                                    )
                                    .with_code("L007")
                                    .with_hint(
                                        "The `0x`, `0o`, and `0b` prefixes require at least one valid digit.",
                                    ),
                                );
                                fatal = true;
                            }
                        }
                        None => {
                            // Decimal literal, optionally fractional and/or in
                            // scientific notation: digits, `_` separators, one
                            // dot and one `e`/`E` exponent with optional sign.
                            let mut seen_dot = false;
                            let mut seen_exponent = false;
                            let mut prev_was_digit = true;

                            while end_index < length {
                                let Some((_, next_char)) = characters.get(end_index) else {
                                    break;
                                };
                                if next_char.is_ascii_digit() {
                                    prev_was_digit = true;
                                    bump_position(next_char, &mut line, &mut column);
                                    end_index += 1;
                                } else if next_char == '_' {
                                    let separator_ok = prev_was_digit
                                        && end_index + 1 < length
                                        && characters
                                            .get(end_index + 1)
                                            .is_some_and(|(_, ch)| ch.is_ascii_digit());
                                    let separator_offset = characters
                                        .get(end_index)
                                        .map(|(offset, _)| offset)
                                        .unwrap_or(self.source.len());
                                    let separator_start = Location::new(line, column);
                                    bump_position('_', &mut line, &mut column);
                                    end_index += 1;
                                    if !separator_ok {
                                        errors.push(
                                            LexError::new(
                                                "invalid `_` placement in decimal literal",
                                                Span::new(
                                                    separator_offset,
                                                    separator_offset + 1,
                                                    separator_start,
                                                    Location::new(line, column),
                                                ),
                                            )
                                            .with_code("L007")
                                            .with_hint(
                                                "`_` separates groups of digits and cannot lead, trail, or repeat.",
                                            ),
                                        );
                                        fatal = true;
                                    }
                                    prev_was_digit = false;
                                } else if next_char == '.'
                                    && !seen_dot
                                    && !seen_exponent
                                    && end_index + 1 < length
                                    && characters
                                        .get(end_index + 1)
                                        .is_some_and(|(_, ch)| ch.is_ascii_digit())
                                {
                                    seen_dot = true;
                                    prev_was_digit = true;
                                    bump_position(next_char, &mut line, &mut column);
                                    end_index += 1;
                                } else if (next_char == 'e' || next_char == 'E')
                                    && !seen_exponent
                                    && prev_was_digit
                                {
                                    // Exponent marker: an optional +/- sign followed
                                    // by at least one digit.
                                    let unsigned_digit = end_index + 1 < length
                                        && characters
                                            .get(end_index + 1)
                                            .is_some_and(|(_, ch)| ch.is_ascii_digit());
                                    let signed_digit = end_index + 2 < length
                                        && characters
                                            .get(end_index + 1)
                                            .is_some_and(|(_, ch)| matches!(ch, '+' | '-'))
                                        && characters
                                            .get(end_index + 2)
                                            .is_some_and(|(_, ch)| ch.is_ascii_digit());
                                    if !(unsigned_digit || signed_digit) {
                                        let marker_offset = characters
                                            .get(end_index)
                                            .map(|(offset, _)| offset)
                                            .unwrap_or(self.source.len());
                                        let marker_start = Location::new(line, column);
                                        bump_position(next_char, &mut line, &mut column);
                                        end_index += 1;
                                        errors.push(
                                            LexError::new(
                                                "missing digits in numeric exponent",
                                                Span::new(
                                                    marker_offset,
                                                    self.source.len(),
                                                    marker_start,
                                                    Location::new(line, column),
                                                ),
                                            )
                                            .with_code("L007")
                                            .with_hint(
                                                "Scientific notation needs digits after `e`/`E`, e.g. `1e5` or `2.5E-3`.",
                                            ),
                                        );
                                        fatal = true;
                                        break;
                                    }
                                    seen_exponent = true;
                                    prev_was_digit = false;
                                    bump_position(next_char, &mut line, &mut column);
                                    end_index += 1;
                                    if end_index < length
                                        && characters
                                            .get(end_index)
                                            .is_some_and(|(_, ch)| matches!(ch, '+' | '-'))
                                    {
                                        if let Some((_, sign)) = characters.get(end_index) {
                                            bump_position(sign, &mut line, &mut column);
                                        }
                                        end_index += 1;
                                    }
                                } else {
                                    break;
                                }
                            }
                        }
                    }

                    let end_offset = if end_index < length {
                        characters
                            .get(end_index)
                            .map(|(offset, _)| offset)
                            .unwrap_or(self.source.len())
                    } else {
                        self.source.len()
                    };
                    let end_location = Location::new(line, column);
                    if !fatal {
                        let text = &self.source[offset..end_offset];
                        tokens.push(Token::new(
                            TokenKind::Number(text.to_string()),
                            Span::new(offset, end_offset, start_location, end_location),
                        ));
                    }
                    index = end_index;
                }
                '"' => {
                    // Scan character by character so we can handle escape sequences
                    // and correctly find the closing quote.
                    let mut string_value = String::new();
                    let mut scan = index + 1; // start after the opening `"`
                    bump_position('"', &mut line, &mut column);
                    let mut terminated = false;

                    while scan < length {
                        let Some((_, sc)) = characters.get(scan) else {
                            break;
                        };
                        if sc == '\\' && scan + 1 < length {
                            // Escape sequence
                            let Some((_, escaped)) = characters.get(scan + 1) else {
                                break;
                            };
                            let escape_offset = characters
                                .get(scan)
                                .map(|(offset, _)| offset)
                                .unwrap_or(self.source.len());
                            let escape_start = Location::new(line, column);
                            bump_position('\\', &mut line, &mut column);
                            bump_position(escaped, &mut line, &mut column);
                            match escaped {
                                '"' => string_value.push('"'),
                                '\'' => string_value.push('\''),
                                '\\' => string_value.push('\\'),
                                'n' => string_value.push('\n'),
                                't' => string_value.push('\t'),
                                'r' => string_value.push('\r'),
                                '0' => string_value.push('\0'),
                                other => {
                                    // Unknown escape: keep the characters in the
                                    // value but report a coded lexical error.
                                    string_value.push('\\');
                                    string_value.push(other);
                                    errors.push(
                                        LexError::new(
                                            format!("unknown escape sequence `\\{}`", other),
                                            Span::new(
                                                escape_offset,
                                                escape_offset + 1 + other.len_utf8(),
                                                escape_start,
                                                Location::new(line, column),
                                            ),
                                        )
                                        .with_code("L008")
                                        .with_hint(
                                            "Supported escapes are \\n, \\t, \\r, \\\\, \\', \\\" and \\0.",
                                        ),
                                    );
                                }
                            }
                            scan += 2;
                        } else if sc == '"' {
                            // Closing quote
                            bump_position('"', &mut line, &mut column);
                            scan += 1;
                            terminated = true;
                            break;
                        } else if sc == '\n' {
                            // Newline inside string — still consume but note it
                            bump_position('\n', &mut line, &mut column);
                            string_value.push('\n');
                            scan += 1;
                        } else {
                            bump_position(sc, &mut line, &mut column);
                            string_value.push(sc);
                            scan += 1;
                        }
                    }

                    let end_offset = if scan < length {
                        characters
                            .get(scan)
                            .map(|(offset, _)| offset)
                            .unwrap_or(self.source.len())
                    } else {
                        self.source.len()
                    };
                    let end_location = Location::new(line, column);

                    if terminated {
                        tokens.push(Token::new(
                            TokenKind::StringLiteral(string_value),
                            Span::new(offset, end_offset, start_location, end_location),
                        ));
                    } else {
                        errors.push(
                            LexError::new(
                                "unterminated string literal",
                                Span::new(offset, self.source.len(), start_location, end_location),
                            )
                            .with_code("L002")
                            .with_hint("Close the string with a matching \" character."),
                        );
                    }
                    index = scan;
                }
                '\'' => {
                    // Character literal: 'a', '\n', etc.
                    bump_position('\'', &mut line, &mut column);
                    let mut scan = index + 1;
                    let mut char_value: Option<char> = None;
                    let mut terminated = false;

                    if scan < length {
                        let Some((_, sc)) = characters.get(scan) else {
                            break;
                        };
                        if sc == '\\' && scan + 1 < length {
                            // Escape sequence
                            let Some((_, escaped)) = characters.get(scan + 1) else {
                                break;
                            };
                            bump_position('\\', &mut line, &mut column);
                            bump_position(escaped, &mut line, &mut column);
                            let ch_val = match escaped {
                                '\'' => '\'',
                                '\\' => '\\',
                                'n' => '\n',
                                't' => '\t',
                                'r' => '\r',
                                '0' => '\0',
                                other => other,
                            };
                            char_value = Some(ch_val);
                            scan += 2;
                        } else if sc != '\'' {
                            bump_position(sc, &mut line, &mut column);
                            char_value = Some(sc);
                            scan += 1;
                        }
                    }

                    if scan < length
                        && characters.get(scan).is_some_and(|(_, ch)| ch == '\'')
                    {
                        bump_position('\'', &mut line, &mut column);
                        scan += 1;
                        terminated = true;
                    }

                    let end_offset = if scan < length {
                        characters
                            .get(scan)
                            .map(|(offset, _)| offset)
                            .unwrap_or(self.source.len())
                    } else {
                        self.source.len()
                    };
                    let end_location = Location::new(line, column);

                    if terminated {
                        if let Some(c) = char_value {
                            tokens.push(Token::new(
                                TokenKind::CharLiteral(c),
                                Span::new(offset, end_offset, start_location, end_location),
                            ));
                        } else {
                            errors.push(
                                LexError::new(
                                    "empty character literal",
                                    Span::new(offset, end_offset, start_location, end_location),
                                )
                                .with_code("L004")
                                .with_hint(
                                    "Character literals must contain exactly one character.",
                                ),
                            );
                        }
                    } else {
                        errors.push(
                            LexError::new(
                                "unterminated character literal",
                                Span::new(offset, self.source.len(), start_location, end_location),
                            )
                            .with_code("L003")
                            .with_hint("Close the character literal with a matching ' character."),
                        );
                    }
                    index = scan;
                }
                ch if is_symbol_char(ch) => {
                    // Check for two-character operators
                    let next_char = if index + 1 < length {
                        characters.get(index + 1).map(|(_, ch)| ch)
                    } else {
                        None
                    };

                    let (token_kind, chars_consumed) = match (ch, next_char) {
                        ('=', Some('=')) => (TokenKind::Operator(Operator::EqualEqual), 2),
                        ('!', Some('=')) => (TokenKind::Operator(Operator::NotEqual), 2),
                        ('<', Some('=')) => (TokenKind::Operator(Operator::LessEqual), 2),
                        ('>', Some('=')) => (TokenKind::Operator(Operator::GreaterEqual), 2),
                        ('&', Some('&')) => (TokenKind::Operator(Operator::And), 2),
                        ('|', Some('|')) => (TokenKind::Operator(Operator::Or), 2),
                        // Range operators: ..= and ..
                        ('.', Some('.')) => {
                            let third = if index + 2 < length {
                                characters.get(index + 2).map(|(_, ch)| ch)
                            } else {
                                None
                            };
                            if third == Some('=') {
                                (TokenKind::Operator(Operator::RangeInclusive), 3)
                            } else {
                                (TokenKind::Operator(Operator::Range), 2)
                            }
                        }
                        // Arrow sequences (`->` and `=>`) are not part of the
                        // language surface. Emit a coded diagnostic that points
                        // at the correct replacement syntax and fall through to
                        // the single-character symbol handling.
                        ('-', Some('>')) | ('=', Some('>')) => {
                            let (replacement, usage) = if ch == '-' {
                                ("returns", "function return types")
                            } else {
                                ("then", "match arms")
                            };
                            errors.push(
                                LexError::new(
                                    format!("arrow operator `{}` is not valid Spectra syntax", ch),
                                    Span::new(
                                        offset,
                                        offset + 2,
                                        start_location,
                                        Location::new(line, column + 1),
                                    ),
                                )
                                .with_code("L007")
                                .with_hint(format!(
                                    "Use `{}` for {} instead of `{}`.",
                                    replacement, usage, ch
                                )),
                            );
                            (TokenKind::Symbol(ch), 1)
                        }
                        _ => (TokenKind::Symbol(ch), 1),
                    };

                    for _ in 0..chars_consumed {
                        let Some((_, current_char)) = characters.get(index) else {
                            break;
                        };
                        bump_position(current_char, &mut line, &mut column);
                        index += 1;
                    }

                    let end_offset = if index < length {
                        characters
                            .get(index)
                            .map(|(offset, _)| offset)
                            .unwrap_or(self.source.len())
                    } else {
                        self.source.len()
                    };
                    let end_location = Location::new(line, column);
                    tokens.push(Token::new(
                        token_kind,
                        Span::new(offset, end_offset, start_location, end_location),
                    ));
                }
                _ => {
                    bump_position(ch, &mut line, &mut column);
                    let end_offset = offset + ch.len_utf8();
                    let end_location = Location::new(line, column);
                    errors.push(
                        LexError::new(
                            format!("unexpected character `{}`", ch),
                            Span::new(offset, end_offset, start_location, end_location),
                        )
                        .with_code("L001")
                        .with_hint(
                            "Remove this character or escape it if you intended it to appear literally.",
                        ),
                    );
                    index += 1;
                }
            }
        }

        let eof_span = Span::new(
            self.source.len(),
            self.source.len(),
            Location::new(line, column),
            Location::new(line, column),
        );
        tokens.push(Token::new(TokenKind::EndOfFile, eof_span));

        // Offsets above are relative to this lex buffer; rebase them onto the
        // enclosing file when the buffer is a fragment (f-string `{...}`).
        if self.origin_offset != 0 {
            for token in &mut tokens {
                token.span.start += self.origin_offset;
                token.span.end += self.origin_offset;
            }
            for error in &mut errors {
                error.span.start += self.origin_offset;
                error.span.end += self.origin_offset;
            }
        }

        if errors.is_empty() {
            Ok(tokens)
        } else {
            Err(errors)
        }
    }
}

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_identifier_continue(ch: char) -> bool {
    is_identifier_start(ch) || ch.is_ascii_digit()
}

fn is_symbol_char(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')'
            | '{'
            | '}'
            | '['
            | ']'
            | ','
            | ';'
            | ':'
            | '.'
            | '='
            | '+'
            | '-'
            | '*'
            | '/'
            | '%'
            | '#'
            | '@'
            | '<'
            | '>'
            | '!'
            | '&'
            | '|'
            | '?'
    )
}

/// Advances the line/column cursor by one character.
///
/// **Column convention**: `column` always points to the position *after* the
/// last consumed character — i.e. the column where the *next* character will
/// land.  This makes `end_location` in each `Span` an **exclusive** bound:
/// `end_location.column` is one past the final column of the token.
/// Consumers should display `end_location.column - 1` when they need an
/// inclusive end, or use `end_location` as-is for half-open ranges.
fn bump_position(ch: char, line: &mut usize, column: &mut usize) {
    if ch == '\n' {
        *line += 1;
        *column = 1;
    } else {
        *column += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_table_avoids_ascii_materialization_and_preserves_unicode_offsets() {
        let ascii = CharacterTable::new("module main");
        assert!(ascii.unicode.is_none());
        assert_eq!(ascii.len(), 11);
        assert_eq!(ascii.get(7), Some((7, 'm')));

        let unicode = CharacterTable::new("let café");
        assert!(unicode.unicode.is_some());
        assert_eq!(unicode.get(4), Some((4, 'c')));
        assert_eq!(unicode.get(7), Some((7, 'é')));
    }

    #[test]
    fn lexes_basic_tokens() {
        let source = "module app.core\nfunc main() { return }";
        let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");
        assert!(tokens
            .iter()
            .any(|token| matches!(token.kind, TokenKind::Keyword(Keyword::Module))));
        assert!(tokens
            .iter()
            .any(|token| matches!(token.kind, TokenKind::Identifier(ref ident) if ident == "app")));
        assert!(tokens.iter().any(
            |token| matches!(token.kind, TokenKind::Identifier(ref ident) if ident == "main")
        ));
    }

    fn number_tokens(source: &str) -> Vec<String> {
        Lexer::new(source)
            .tokenize()
            .expect("lexer should succeed")
            .into_iter()
            .filter_map(|token| match token.kind {
                TokenKind::Number(text) => Some(text),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn lexes_radix_prefixed_literals() {
        assert_eq!(number_tokens("0xFF"), vec!["0xFF"]);
        assert_eq!(number_tokens("0xDE_AD"), vec!["0xDE_AD"]);
        assert_eq!(number_tokens("0o17"), vec!["0o17"]);
        assert_eq!(number_tokens("0b1011"), vec!["0b1011"]);
        assert_eq!(number_tokens("0b1010_0001"), vec!["0b1010_0001"]);
    }

    #[test]
    fn lexes_decimal_separators() {
        assert_eq!(number_tokens("1_000_000"), vec!["1_000_000"]);
        assert_eq!(number_tokens("1_2.3_4"), vec!["1_2.3_4"]);
    }

    #[test]
    fn lexes_scientific_notation() {
        assert_eq!(number_tokens("1e5"), vec!["1e5"]);
        assert_eq!(number_tokens("2.5E-3"), vec!["2.5E-3"]);
        assert_eq!(number_tokens("7E+10"), vec!["7E+10"]);
        assert_eq!(number_tokens("1.5e0"), vec!["1.5e0"]);
    }

    #[test]
    fn keeps_plain_integer_and_float_classification_text() {
        assert_eq!(number_tokens("42 3.25"), vec!["42", "3.25"]);
        // `1e` must not swallow the identifier boundary: `e` alone is invalid
        // exponent syntax so the token text stops before producing a value.
        let result = Lexer::new("2x").tokenize();
        assert!(result.is_ok());
        // Hex literal with no digits is rejected.
        assert!(Lexer::new("let x = 0x\n").tokenize().is_err());
        assert!(Lexer::new("let x = 0b2\n").tokenize().is_err());
        assert!(Lexer::new("let x = 0o8\n").tokenize().is_err());
    }

    #[test]
    fn rejects_leading_trailing_and_double_separators_with_l007() {
        for source in ["1_000_", "1__000", "0x_FF", "0xFF_", "0b__1011"] {
            let errors = Lexer::new(source)
                .tokenize()
                .expect_err("bad separator placement should fail");
            assert!(
                errors
                    .iter()
                    .any(|error| error.code.as_deref() == Some("L007")),
                "expected L007 for `{}`, got {:?}",
                source,
                errors.iter().map(|e| &e.message).collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn rejects_missing_exponent_digits_with_l007() {
        let errors = Lexer::new("1e")
            .tokenize()
            .expect_err("`1e` without digits should fail");
        assert!(
            errors
                .iter()
                .any(|error| error.code.as_deref() == Some("L007")),
            "expected L007, got {:?}",
            errors
        );
    }

    #[test]
    fn accepts_all_documented_escapes() {
        let tokens = Lexer::new(r#""\n\t\r\\\'\"\0""#)
            .tokenize()
            .expect("documented escapes should lex");
        assert!(tokens.iter().any(|token| matches!(
            &token.kind,
            TokenKind::StringLiteral(value)
                if value == &"\n\t\r\\'\"\0".to_string()
        )));
    }

    #[test]
    fn rejects_unknown_string_escape_with_l008() {
        let errors = Lexer::new(r#""bad \q escape""#)
            .tokenize()
            .expect_err("unknown escape should fail");
        assert!(
            errors.iter().any(|error| {
                error.code.as_deref() == Some("L008") && error.message.contains("\\q")
            }),
            "expected L008 naming \\q, got {:?}",
            errors
                .iter()
                .map(|e| (&e.code, &e.message))
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn rejects_unknown_fstring_escape_with_l008() {
        let errors = Lexer::new(r#"f"value \x here""#)
            .tokenize()
            .expect_err("unknown f-string escape should fail");
        assert!(
            errors
                .iter()
                .any(|error| error.code.as_deref() == Some("L008")),
            "expected L008, got {:?}",
            errors
        );
    }
}
